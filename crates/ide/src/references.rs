use hir::{
    normalize_match_name, normalize_usage_name, Name, ReferenceScope, SemanticSymbol, Semantics,
};
use ide_db::base_db::{RootQueryDb, SourceDatabase};
use ide_db::{RootDatabase, RootDatabaseImpl};
use rustc_hash::FxHashSet;
use salsa::Database;
use syntax::{SyntaxKind, TextRange, TextSize};
use vfs::FileId;

use crate::declarations::{
    classify_unreferenceable, resolve_declarations, Declaration, UnsupportedCategory,
};
use crate::name_lookup::{lookup_names, NameLookupResult, NameMatchTier, NameQuery};
use crate::reference_kind::{classify_reference_token, ReferenceKind};
use crate::Location;

pub fn find_references<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
) -> Vec<Location> {
    let _span = tracing::info_span!("find_references", ?file_id).entered();

    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    let token = match root.token_at_offset(offset).right_biased() {
        Some(t) if t.kind().is_name_token() => t,
        _ => return Vec::new(),
    };

    let sema = Semantics::new(db);
    let symbol = match sema.symbol_for_token(file_id, &token) {
        Some(symbol) => symbol,
        None => return Vec::new(),
    };

    let scope = symbol.reference_scope(db);
    tracing::debug!(
        ?scope,
        target_name = %symbol.name.as_str(),
        "Reference scope determined"
    );

    let files_to_search: Vec<FileId> = match scope {
        ReferenceScope::FileLocal => vec![file_id],
        ReferenceScope::Unknown => return Vec::new(),
        ReferenceScope::ModuleSymbolWorkspace => {
            workspace_candidate_files(db, file_id, &symbol.name)
        }
    };

    let mut all_references = Vec::new();
    for &search_file_id in &files_to_search {
        db.unwind_if_revision_cancelled();
        let references = find_references_in_file(db, search_file_id, &symbol);
        all_references.extend(references);
    }

    tracing::info!(
        total_references = all_references.len(),
        files_searched = files_to_search.len(),
        "Find references completed"
    );

    all_references
}

fn workspace_candidate_files<DB: RootDatabase>(
    db: &DB,
    current_file: FileId,
    target_name: &Name,
) -> Vec<FileId> {
    let source_root_input = db.file_source_root_input(current_file);
    let source_root_id = source_root_input.source_root_id(db);
    let aggregator = db.name_usage_index(source_root_id);
    let normalized = normalize_usage_name(target_name);
    aggregator.files_with(&normalized).to_vec()
}

pub(crate) fn find_references_in_file<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    target_symbol: &SemanticSymbol,
) -> Vec<Location> {
    let _span = tracing::debug_span!("find_references_in_file", ?file_id).entered();

    // Popular names (standard event handlers) yield hundreds of candidate
    // files; the memoised per-file offsets replace a full token walk per
    // request with a lookup, so only the actual occurrences pay resolution.
    let normalized = normalize_match_name(&target_symbol.name);
    let occurrences = db.file_name_offsets_ref(file_id);
    let offsets = occurrences.offsets(&normalized);
    if offsets.is_empty() {
        return Vec::new();
    }

    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let sema = Semantics::new(db);

    let mut references = Vec::new();

    for &offset in offsets {
        let Some(token) = root.token_at_offset(TextSize::from(offset)).right_biased() else {
            continue;
        };
        if !token.kind().is_name_token()
            || u32::from(token.text_range().start()) != offset
            || !Name::new(token.text()).eq_ignore_case(&target_symbol.name)
        {
            continue;
        }

        let Some(candidate_symbol) = sema.symbol_for_token(file_id, &token) else {
            continue;
        };

        if candidate_symbol.key == target_symbol.key {
            let range = token.text_range();
            references.push(Location { file_id, range });
        }
    }

    tracing::debug!(
        count = references.len(),
        target_name = %target_symbol.name.as_str(),
        "Found references"
    );

    references
}

// --- reference search by name -----------------------------------------------------------

/// The files an area or an anchor is confined to. A set of ids and not a path
/// prefix: a root is declared with one spelling and indexed with another
/// whenever a symlink is involved, and a prefix comparison would then match
/// nothing while reporting an honest-looking zero.
pub type FileIdSet = FxHashSet<FileId>;

/// How many dictionary candidates one anchor resolution may consider.
const ANCHOR_CANDIDATE_LIMIT: usize = 64;

/// What the caller points at.
#[derive(Debug, Clone)]
pub enum ReferenceAnchor {
    /// A qualified name of one to three segments, or a short one.
    Name(String),
    /// 0-based line and 0-based character offset within that line.
    Position { file_id: FileId, line: u32, column: u32 },
}

/// Which part of the workspace the answer is confined to.
#[derive(Debug, Clone, Default)]
pub struct ReferenceArea {
    /// `None` — the whole workspace.
    pub files: Option<FileIdSet>,
}

#[derive(Debug, Clone)]
pub struct ReferencesRequest {
    pub anchor: ReferenceAnchor,
    /// Files among which the DECLARATION is looked for. `None` — the whole
    /// workspace. Narrows the candidate set before its size is counted, so it
    /// is the way out of [`ReferencesOutcome::Ambiguous`] — unlike
    /// [`ReferenceArea`], which narrows the references already found.
    pub anchor_files: Option<FileIdSet>,
    pub area: ReferenceArea,
    /// `None` — every kind.
    pub kinds: Option<Vec<ReferenceKind>>,
    pub include_declaration: bool,
    /// Cap on candidate files walked. Reaching it makes the total a lower
    /// bound, which the caller is told about rather than left to guess.
    pub max_files: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceHit {
    pub file_id: FileId,
    /// The occurrence itself.
    pub range: TextRange,
    /// The method the occurrence sits in, when it sits in one.
    pub enclosing_range: Option<TextRange>,
    pub kind: ReferenceKind,
}

/// Whether the symbol was determined at all — a different question from whether
/// the answer is complete, which the freshness envelope answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferencesOutcome {
    /// The anchor is one symbol; the hit list IS the answer, and an empty list
    /// means exactly zero references.
    Resolved,
    /// More than one symbol answers to this anchor; no references are counted.
    Ambiguous,
    /// Nothing matched exactly enough to anchor on.
    NotFound,
    /// The name was resolved precisely, but nothing can enumerate references to
    /// it. Told apart from a zero-length list on purpose: a client renames on
    /// the strength of that zero.
    UnsupportedSymbol { category: UnsupportedCategory },
}

/// Whose data COMPOSES the body of the answer — not who was asked along the way.
///
/// The location contract gives the name dictionary a null revision and a null
/// topology fingerprint, because several artefacts with different revisions
/// stand behind it. Stamping that on a body the resident composed would erase
/// the fingerprint the caller needs; stamping the resident's revision on a body
/// of dictionary candidates would claim an identity those candidates do not
/// share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodySource {
    Resident,
    NameDictionary,
}

#[derive(Debug, Clone)]
pub struct ReferencesResult {
    pub outcome: ReferencesOutcome,
    pub body_source: BodySource,
    /// Every reference that survived the area and kind filters, before any
    /// display limit. Empty for every outcome other than `Resolved`.
    pub hits: Vec<ReferenceHit>,
    /// Per-file counts over the same set as `hits`, most-populated first.
    pub per_file: Vec<(FileId, usize)>,
    /// The declarations found when the anchor is a qualified name — one when
    /// resolved, several when ambiguous.
    pub declarations: Vec<Declaration>,
    /// The dictionary's answer, when the dictionary is what composed the body.
    pub candidates: Option<NameLookupResult>,
    pub files_scanned: usize,
    /// `max_files` cut the walk short: `hits.len()` is a lower bound, and two
    /// answers taken under this flag are not comparable with each other.
    pub files_capped: bool,
    /// The exact matches alone filled the dictionary's candidate budget, so one more
    /// may have been cut before the anchor was chosen. Unlike [`Self::files_capped`],
    /// this cap can change the OUTCOME itself — a declaration that fell off the list is
    /// one the answer could have resolved to or been ambiguous about — so it travels
    /// with every outcome, not just `Resolved`.
    pub anchor_candidates_capped: bool,
}

impl ReferencesResult {
    fn outcome_only(outcome: ReferencesOutcome, body_source: BodySource) -> Self {
        Self {
            outcome,
            body_source,
            hits: Vec::new(),
            per_file: Vec::new(),
            declarations: Vec::new(),
            candidates: None,
            files_scanned: 0,
            files_capped: false,
            anchor_candidates_capped: false,
        }
    }
}

/// Find every reference to the symbol the request anchors on.
pub fn find_references_by_name(db: &RootDatabaseImpl, req: &ReferencesRequest) -> ReferencesResult {
    let _span = tracing::info_span!("find_references_by_name").entered();

    match &req.anchor {
        ReferenceAnchor::Name(name) => resolve_by_name(db, name, req),
        ReferenceAnchor::Position { file_id, line, column } => {
            resolve_by_position(db, *file_id, *line, *column, req)
        }
    }
}

/// Stage one, then stage two — and they are not interchangeable. A qualified
/// name is matched against declarations; the name dictionary compares the whole
/// needle with a short member name and would score `Продажи.Расчёт` against
/// `Расчёт` at no tier at all.
fn resolve_by_name(db: &RootDatabaseImpl, name: &str, req: &ReferencesRequest) -> ReferencesResult {
    let mut declarations = resolve_declarations(db, name);
    if let Some(files) = &req.anchor_files {
        declarations.retain(|decl| files.contains(&decl.file_id));
    }

    match declarations.len() {
        1 => {
            let declaration = declarations[0];
            let Some(symbol) = symbol_at(db, declaration.file_id, declaration.name_range.start())
            else {
                // The declaration is there and its symbol is not: nothing knows
                // where to look, which is what this category says.
                return ReferencesResult::outcome_only(
                    ReferencesOutcome::UnsupportedSymbol {
                        category: UnsupportedCategory::UnknownScope,
                    },
                    BodySource::Resident,
                );
            };
            let mut result = collect_references(db, declaration.file_id, &symbol, req);
            result.declarations = declarations;
            result
        }
        0 => {
            let dictionary = resolve_by_dictionary(db, name, req);
            // The dictionary is holding a declaration this name can be walked from, so
            // the string is not "something no walk enumerates" however the card surface
            // reads it. Asking the card first is what made a module's own `Сообщить` a
            // platform member: `symbol_info` answers about the platform for a bare name,
            // and it is right about the string — but the walk exists all the same.
            if dictionary.anchorable {
                return dictionary.result;
            }
            match classify_unreferenceable(db, name) {
                Some(category) => ReferencesResult::outcome_only(
                    ReferencesOutcome::UnsupportedSymbol { category },
                    BodySource::Resident,
                ),
                None => dictionary.result,
            }
        }
        _ => ReferencesResult {
            declarations,
            ..ReferencesResult::outcome_only(ReferencesOutcome::Ambiguous, BodySource::Resident)
        },
    }
}

/// Stage two: a short or inexact name, answered by the name dictionary.
///
/// Only `Exact` and `CaseInsensitive` can anchor — a prefix or a substring
/// match names a different symbol, and counting its references would answer a
/// question nobody asked.
/// What stage two made of the name, and whether the dictionary held a declaration the
/// name could be walked from at all — the question that decides whether the card surface
/// gets to call the name unwalkable.
struct DictionaryAnswer {
    result: ReferencesResult,
    anchorable: bool,
}

fn resolve_by_dictionary(
    db: &RootDatabaseImpl,
    name: &str,
    req: &ReferencesRequest,
) -> DictionaryAnswer {
    let query = NameQuery::new(name, ANCHOR_CANDIDATE_LIMIT);
    let lookup = lookup_names(db, &query, &[]);

    let exact: Vec<&crate::name_lookup::NameCandidate> = lookup
        .candidates
        .iter()
        .filter(|candidate| {
            matches!(candidate.match_tier, NameMatchTier::Exact | NameMatchTier::CaseInsensitive)
        })
        .collect();

    // Whether a reference walk EXISTS is decided before any narrowing: an exact
    // match with a range of its own is a declaration something can be walked
    // from, whatever file set the caller passed.
    let anchorable: Vec<&crate::name_lookup::NameCandidate> = exact
        .iter()
        .copied()
        .filter(|candidate| candidate.place.is_some_and(|place| place.range.is_some()))
        .collect();

    // The anchor's file set narrows WHICH DECLARATION is taken, so it applies to
    // the candidates that could be one. Applying it a step earlier would drop a
    // platform member — which belongs to no root by construction — and turn
    // `unsupported_symbol` into `not_found`: a narrowing parameter would be
    // changing the CLASS of the answer, not the set it is drawn from.
    let anchored: Vec<&crate::name_lookup::NameCandidate> = anchorable
        .iter()
        .copied()
        .filter(|candidate| match &req.anchor_files {
            Some(files) => candidate.place.is_some_and(|place| files.contains(&place.file_id)),
            None => true,
        })
        .collect();

    // What could never be an anchor, whatever the file set: an exact match with no
    // range of its own. Kept apart from "an anchor that this file set excludes",
    // because only the first means "no reference walk exists for this symbol".
    let unanchorable: Vec<UnsupportedCategory> = exact
        .iter()
        .filter(|candidate| candidate.place.is_none_or(|place| place.range.is_none()))
        .filter_map(|candidate| UnsupportedCategory::from_name_category(candidate.category))
        .collect();

    let result = match anchored.len() {
        1 => {
            let place = anchored[0].place.expect("filtered on a place");
            let range = place.range.expect("filtered on a range");
            match symbol_at(db, place.file_id, range.start()) {
                // The body is the reference list the resident walked, even
                // though it was the dictionary that found the anchor.
                Some(symbol) => collect_references(db, place.file_id, &symbol, req),
                None => ReferencesResult {
                    candidates: Some(lookup.clone()),
                    ..ReferencesResult::outcome_only(
                        ReferencesOutcome::NotFound,
                        BodySource::NameDictionary,
                    )
                },
            }
        }
        // Matched exactly, but nothing to walk: a metadata object, a platform
        // member, a module as a whole. Saying `not_found` here would deny a
        // name the search did find — while saying it about a method the anchor's
        // file set merely excluded would deny a walk that exists. A short name
        // is regularly both at once (a module method named like a platform
        // member), so the walk's existence is read off `anchorable`, which the
        // file set never touched, and not off "nothing anchored here".
        0 if anchorable.is_empty() && !unanchorable.is_empty() => {
            let category = unanchorable[0];
            ReferencesResult {
                candidates: Some(lookup.clone()),
                ..ReferencesResult::outcome_only(
                    ReferencesOutcome::UnsupportedSymbol { category },
                    BodySource::NameDictionary,
                )
            }
        }
        0 => ReferencesResult {
            candidates: Some(lookup.clone()),
            ..ReferencesResult::outcome_only(
                ReferencesOutcome::NotFound,
                BodySource::NameDictionary,
            )
        },
        _ => ReferencesResult {
            candidates: Some(lookup.clone()),
            ..ReferencesResult::outcome_only(
                ReferencesOutcome::Ambiguous,
                BodySource::NameDictionary,
            )
        },
    };

    DictionaryAnswer {
        // The list is sorted by tier before it is cut, so exact matches are the last
        // to go: they are lost only once they fill the budget by themselves.
        // `NameLookupResult::truncated` is the wrong signal here — it is also true when
        // an unrelated provider capped its own prefix matches.
        result: ReferencesResult {
            anchor_candidates_capped: exact.len() >= ANCHOR_CANDIDATE_LIMIT,
            ..result
        },
        anchorable: !anchorable.is_empty(),
    }
}

/// The fallback anchor: a position, for what no name addresses — a local, a
/// parameter.
///
/// Each way of missing gets its own outcome. Today's `find_references` folds
/// three different facts into one empty vector, and that is the defect this
/// surface exists to close.
fn resolve_by_position(
    db: &RootDatabaseImpl,
    file_id: FileId,
    line: u32,
    column: u32,
    req: &ReferencesRequest,
) -> ReferencesResult {
    let Some(offset) = offset_for_line_col(db, file_id, line, column) else {
        return ReferencesResult::outcome_only(ReferencesOutcome::NotFound, BodySource::Resident);
    };
    let Some(symbol) = symbol_at(db, file_id, offset) else {
        return ReferencesResult::outcome_only(ReferencesOutcome::NotFound, BodySource::Resident);
    };
    collect_references(db, file_id, &symbol, req)
}

/// `column` is a 0-based CHARACTER offset within the line, as the surface
/// spells it; line-index columns are byte offsets, so the conversion walks
/// characters — the identifiers are Cyrillic.
fn offset_for_line_col(
    db: &RootDatabaseImpl,
    file_id: FileId,
    line: u32,
    column: u32,
) -> Option<TextSize> {
    let text = db.file_text(file_id);
    let line_index = line_index::LineIndex::new(&text);
    let line_start = line_index.try_line_start(line)?;
    let line_str = text[u32::from(line_start) as usize..].split('\n').next().unwrap_or("");
    let byte_in_line =
        line_str.char_indices().nth(column as usize).map_or(line_str.len(), |(i, _)| i);
    let offset = line_start + TextSize::from(byte_in_line as u32);

    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let token = root.token_at_offset(offset).right_biased()?;
    // At a line end `right_biased` selects the next line's first token, which
    // would answer for a symbol the caller never pointed at.
    if line_index.line_col(token.text_range().start()).line != line {
        return None;
    }
    Some(offset)
}

fn symbol_at(db: &RootDatabaseImpl, file_id: FileId, offset: TextSize) -> Option<SemanticSymbol> {
    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let token = root.token_at_offset(offset).right_biased()?;
    if !token.kind().is_name_token() {
        return None;
    }
    Semantics::new(db).symbol_for_token(file_id, &token)
}

fn collect_references(
    db: &RootDatabaseImpl,
    home_file: FileId,
    symbol: &SemanticSymbol,
    req: &ReferencesRequest,
) -> ReferencesResult {
    let scope = symbol.reference_scope(db);
    let candidate_files: Vec<FileId> = match scope {
        ReferenceScope::FileLocal => vec![home_file],
        // The one case where an empty list is not an answer: nothing knows
        // where to look, so nothing was looked at.
        ReferenceScope::Unknown => {
            return ReferencesResult::outcome_only(
                ReferencesOutcome::UnsupportedSymbol {
                    category: UnsupportedCategory::UnknownScope,
                },
                BodySource::Resident,
            )
        }
        ReferenceScope::ModuleSymbolWorkspace => {
            workspace_candidate_files(db, home_file, &symbol.name)
        }
    };

    // The area is applied to the candidate files, before the walk: the total
    // must count what the filter admits, not what it later hides.
    let candidate_files: Vec<FileId> = match &req.area.files {
        Some(files) => candidate_files.into_iter().filter(|id| files.contains(id)).collect(),
        None => candidate_files,
    };

    let files_capped = candidate_files.len() > req.max_files;
    let scanned = &candidate_files[..candidate_files.len().min(req.max_files)];

    let mut hits = Vec::new();
    for &file_id in scanned {
        db.unwind_if_revision_cancelled();
        let locations = find_references_in_file(db, file_id, symbol);
        if locations.is_empty() {
            continue;
        }
        let parse = db.parse(file_id);
        let root = parse.syntax_node();
        for location in locations {
            let Some(token) = root.token_at_offset(location.range.start()).right_biased() else {
                continue;
            };
            let kind = classify_reference_token(&token);
            if let Some(wanted) = &req.kinds {
                if !wanted.contains(&kind) {
                    continue;
                }
            }
            if !req.include_declaration && kind == ReferenceKind::Declaration {
                continue;
            }
            let enclosing_range = token
                .parent_ancestors()
                .find(|node| {
                    matches!(node.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF)
                })
                .map(|node| node.text_range());
            hits.push(ReferenceHit { file_id, range: location.range, enclosing_range, kind });
        }
    }

    hits.sort_by_key(|hit| (hit.file_id.0, hit.range.start()));

    let mut per_file: Vec<(FileId, usize)> = Vec::new();
    for hit in &hits {
        match per_file.last_mut() {
            Some((file_id, count)) if *file_id == hit.file_id => *count += 1,
            _ => per_file.push((hit.file_id, 1)),
        }
    }
    per_file.sort_by_key(|(file_id, count)| (std::cmp::Reverse(*count), file_id.0));

    ReferencesResult {
        outcome: ReferencesOutcome::Resolved,
        body_source: BodySource::Resident,
        hits,
        per_file,
        declarations: Vec::new(),
        candidates: None,
        files_scanned: scanned.len(),
        files_capped,
        anchor_candidates_capped: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use vfs::{file_set::FileSet, VfsPath};

    fn create_db_with_file(source: &str) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::default();
        let file_id = FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, source);

        (db, file_id)
    }

    #[test]
    fn test_find_method_references() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры

Функция Тест()
    МояПроцедура();
    МояПроцедура();
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);

        let def_offset = source.find("МояПроцедура").unwrap();
        let offset = TextSize::from(def_offset as u32);

        let references = find_references(&db, file_id, offset);

        assert!(
            references.len() >= 3,
            "Expected at least 3 references, found {}",
            references.len()
        );

        for loc in &references {
            assert_eq!(loc.file_id, file_id);
            assert!(!loc.range.is_empty());
        }
    }

    #[test]
    fn test_find_variable_references() {
        let source = r#"
Перем МодульнаяПеременная;

Процедура Тест()
    МодульнаяПеременная = 1;
    Результат = МодульнаяПеременная + 2;
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        let decl_offset = source.find("МодульнаяПеременная").unwrap();
        let offset = TextSize::from(decl_offset as u32);

        let references = find_references(&db, file_id, offset);

        assert!(
            references.len() >= 3,
            "Expected at least 3 references, found {}",
            references.len()
        );

        for loc in &references {
            assert_eq!(loc.file_id, file_id);
        }
    }

    #[test]
    fn test_find_references_case_insensitive() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры

Функция Тест()
    мояпроцедура();
    МОЯПРОЦЕДУРА();
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);

        let call_offset = source.find("мояпроцедура").unwrap();
        let offset = TextSize::from(call_offset as u32);

        let references = find_references(&db, file_id, offset);

        assert!(
            references.len() >= 3,
            "Expected at least 3 references, found {}",
            references.len()
        );
    }

    #[test]
    fn find_references_survive_edit_that_shifts_offsets() {
        let source = "Процедура МояПроцедура()\nКонецПроцедуры\n\nПроцедура Тест()\n    МояПроцедура();\nКонецПроцедуры\n";
        let (mut db, file_id) = create_db_with_file(source);

        let def_offset = source.find("МояПроцедура").unwrap();
        let before = find_references(&db, file_id, TextSize::from(def_offset as u32));
        assert_eq!(before.len(), 2);

        // Prepend a comment so every occurrence moves; memoised per-file
        // offsets must be recomputed, not replayed at the stale positions.
        let shifted = format!("// сдвиг\n{source}");
        db.set_file_text(file_id, &shifted);

        let def_offset = shifted.find("МояПроцедура").unwrap();
        let after = find_references(&db, file_id, TextSize::from(def_offset as u32));
        assert_eq!(after.len(), 2);
        for loc in &after {
            let start = usize::from(loc.range.start());
            assert_eq!(&shifted[start..start + "МояПроцедура".len()], "МояПроцедура");
        }
    }

    #[test]
    fn find_references_final_sigma_keeps_token_walk_semantics() {
        // Final-sigma pair: `eq_ignore_case`-equal tokens whose contextual
        // `to_lowercase` keys differ. Local-symbol keys normalise via
        // `fold_lower` (`SemanticSymbolKey::BodyLocal`), so the usage never
        // matches the declaration's key: exactly one reference, same as the
        // pre-offsets token walk. The offsets bucket uses the per-char fold
        // (`normalize_match_name`) so its candidate set equals the old
        // `eq_ignore_case` prefilter; this pins that neither a missed token
        // nor a new false match appears for such identifiers.
        let source = "Процедура Тест(ΟΔΟΣ)\n    Рез = οδοσ + 1;\nКонецПроцедуры\n";
        let (db, file_id) = create_db_with_file(source);

        let decl_offset = source.find("ΟΔΟΣ").unwrap();
        let references = find_references(&db, file_id, TextSize::from(decl_offset as u32));
        assert_eq!(references.len(), 1, "declaration only, got {references:?}");
        assert_eq!(usize::from(references[0].range.start()), decl_offset);
    }

    #[test]
    fn test_find_references_not_found() {
        let source = r#"
Процедура Тест()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        let offset = source.find("Процедура").unwrap();
        let offset = TextSize::from(offset as u32);

        let references = find_references(&db, file_id, offset);
        assert!(references.is_empty());
    }

    #[test]
    fn test_find_references_from_usage() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры

Функция Тест()
    МояПроцедура();
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);

        let call_offset = source.rfind("МояПроцедура").unwrap();
        let offset = TextSize::from(call_offset as u32);

        let references = find_references(&db, file_id, offset);

        assert!(
            references.len() >= 2,
            "Expected at least 2 references, found {}",
            references.len()
        );
    }

    #[test]
    fn test_find_references_function() {
        let source = r#"
Функция МояФункция()
    Возврат 1;
КонецФункции

Процедура Тест()
    Результат = МояФункция();
    Другой = МояФункция();
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        let def_offset = source.find("МояФункция").unwrap();
        let offset = TextSize::from(def_offset as u32);

        let references = find_references(&db, file_id, offset);

        assert!(
            references.len() >= 3,
            "Expected at least 3 references, found {}",
            references.len()
        );
    }

    #[test]
    fn test_find_parameter_references() {
        let source = r#"
Процедура Тест(МойПараметр)
    Если МойПараметр > 0 Тогда
        Результат = МойПараметр + 1;
    КонецЕсли;
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        let param_offset = source.find("МойПараметр").unwrap();
        let offset = TextSize::from(param_offset as u32);

        let references = find_references(&db, file_id, offset);

        println!("Found {} parameter references", references.len());

        assert_eq!(
            references.len(),
            3,
            "Expected exactly 3 references (declaration + 2 usages), found {}",
            references.len()
        );
    }

    #[test]
    fn test_find_implicit_local_references_do_not_cross_methods() {
        let source = r#"
Процедура Первый()
    НаборЗаписей = 1;
    Сообщить(НаборЗаписей);
КонецПроцедуры

Процедура Второй()
    НаборЗаписей = 2;
    Сообщить(НаборЗаписей);
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let offset = TextSize::from(source.find("НаборЗаписей").unwrap() as u32);

        let references = find_references(&db, file_id, offset);

        assert_eq!(references.len(), 2, "implicit locals must be scoped to their body");
        for location in references {
            let start: u32 = location.range.start().into();
            assert!(
                start < source.find("Процедура Второй").unwrap() as u32,
                "reference from the second method leaked into the first method result"
            );
        }
    }

    #[test]
    fn test_find_implicit_local_references_split_by_inferred_type() {
        let source = r#"
Процедура Тест()
    НаборЗаписей = 1;
    Сообщить(НаборЗаписей);

    НаборЗаписей = "строка";
    Сообщить(НаборЗаписей);
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let second_assignment = source.find("НаборЗаписей = \"строка\"").unwrap() as u32;

        let first_offset = TextSize::from(source.find("НаборЗаписей").unwrap() as u32);
        let first_refs = find_references(&db, file_id, first_offset);
        assert_eq!(first_refs.len(), 2, "number-typed implicit local should stay separate");
        assert!(
            first_refs.iter().all(|loc| u32::from(loc.range.start()) < second_assignment),
            "string-typed occurrences leaked into number-typed references: {first_refs:?}"
        );

        let second_offset = TextSize::from(second_assignment);
        let second_refs = find_references(&db, file_id, second_offset);
        assert_eq!(second_refs.len(), 2, "string-typed implicit local should stay separate");
        assert!(
            second_refs.iter().all(|loc| u32::from(loc.range.start()) >= second_assignment),
            "number-typed occurrences leaked into string-typed references: {second_refs:?}"
        );
    }

    #[test]
    fn test_find_local_variable_references() {
        let source = r#"
Процедура Тест()
    Перем МояПеременная;

    МояПеременная = 10;
    Результат = МояПеременная * 2;
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        let var_offset = source.find("МояПеременная").unwrap();
        let offset = TextSize::from(var_offset as u32);

        let references = find_references(&db, file_id, offset);

        println!("Found {} local variable references", references.len());

        assert_eq!(
            references.len(),
            3,
            "Expected exactly 3 references (declaration + 2 usages), found {}",
            references.len()
        );
    }

    #[test]
    fn test_find_references_no_false_positives() {
        let source = r#"
Перем Значение;

Процедура Тест1()
    Перем Значение;  // Local variable with same name

    Значение = 1;
КонецПроцедуры

Процедура Тест2()
    Значение = 2;  // Module variable
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        let module_var_offset = source.find("Значение").unwrap();
        let offset = TextSize::from(module_var_offset as u32);

        let references = find_references(&db, file_id, offset);

        println!("Found {} module variable references", references.len());
        for (i, loc) in references.iter().enumerate() {
            let start: u32 = loc.range.start().into();
            let end: u32 = loc.range.end().into();
            let text = &source[start as usize..end as usize];
            println!("  Ref {}: offset={}, text={:?}", i, start, text);
        }

        assert!(
            references.len() >= 2,
            "Expected at least 2 references (module var), found {}",
            references.len()
        );
    }

    #[test]
    fn test_find_module_method_multiple_files() {
        let mut db = RootDatabaseImpl::default();

        let file1_id = FileId(0);
        let file1_source = r#"
Процедура МояПроцедура() Экспорт
    // Определение
КонецПроцедуры

Функция Тест1()
    МояПроцедура();  // Вызов в том же модуле
КонецФункции
        "#;

        let file2_id = FileId(1);
        let file2_source = r#"
Процедура МояПроцедура()
    // Другая процедура с тем же именем
КонецПроцедуры

Функция Тест2()
    МояПроцедура();  // Вызов локального метода
КонецПроцедуры
        "#;

        let mut file_set = FileSet::new();
        file_set.insert(file1_id, VfsPath::new("/module1.bsl"));
        file_set.insert(file2_id, VfsPath::new("/module2.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file1_id, SourceRootId(0));
        db.set_file_source_root(file2_id, SourceRootId(0));

        db.set_file_text(file1_id, file1_source);
        db.set_file_text(file2_id, file2_source);

        let def_offset = file1_source.find("МояПроцедура").unwrap();
        let offset = TextSize::from(def_offset as u32);

        let references = find_references(&db, file1_id, offset);

        println!("Found {} references for file1 method", references.len());
        for (i, loc) in references.iter().enumerate() {
            println!("  Ref {}: file={:?}, range={:?}", i, loc.file_id, loc.range);
        }

        assert_eq!(references.len(), 2, "Expected 2 references in file1 only");

        for loc in &references {
            assert_eq!(loc.file_id, file1_id, "All references should be in file1");
        }
    }

    #[test]
    fn test_find_module_variable_multiple_files() {
        let mut db = RootDatabaseImpl::default();

        let file1_id = FileId(0);
        let file1_source = r#"
Перем МояПеременная Экспорт;

Процедура Тест1()
    МояПеременная = 10;
    Сообщить(МояПеременная);
КонецПроцедуры
        "#;

        let file2_id = FileId(1);
        let file2_source = r#"
Перем МояПеременная;

Процедура Тест2()
    МояПеременная = 20;
КонецПроцедуры
        "#;

        let mut file_set = FileSet::new();
        file_set.insert(file1_id, VfsPath::new("/module1.bsl"));
        file_set.insert(file2_id, VfsPath::new("/module2.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file1_id, SourceRootId(0));
        db.set_file_source_root(file2_id, SourceRootId(0));

        db.set_file_text(file1_id, file1_source);
        db.set_file_text(file2_id, file2_source);

        let def_offset = file1_source.find("МояПеременная").unwrap();
        let offset = TextSize::from(def_offset as u32);

        let references = find_references(&db, file1_id, offset);

        println!("Found {} variable references for file1", references.len());

        assert_eq!(references.len(), 3, "Expected 3 references in file1 only");

        for loc in &references {
            assert_eq!(loc.file_id, file1_id, "All references should be in file1");
        }
    }

    #[test]
    fn test_local_symbols_only_in_current_file() {
        let mut db = RootDatabaseImpl::default();

        let file1_id = FileId(0);
        let file1_source = r#"
Процедура Метод1()
    Перем ЛокальнаяПеременная;
    ЛокальнаяПеременная = 1;
    Сообщить(ЛокальнаяПеременная);
КонецПроцедуры
        "#;

        let file2_id = FileId(1);
        let file2_source = r#"
Процедура Метод2()
    Перем ЛокальнаяПеременная;  // Same name, different scope
    ЛокальнаяПеременная = 2;
КонецПроцедуры
        "#;

        let mut file_set = FileSet::new();
        file_set.insert(file1_id, VfsPath::new("/local1.bsl"));
        file_set.insert(file2_id, VfsPath::new("/local2.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file1_id, SourceRootId(0));
        db.set_file_source_root(file2_id, SourceRootId(0));

        db.set_file_text(file1_id, file1_source);
        db.set_file_text(file2_id, file2_source);

        let def_offset = file1_source.find("ЛокальнаяПеременная").unwrap();
        let offset = TextSize::from(def_offset as u32);

        let references = find_references(&db, file1_id, offset);

        println!("Found {} local variable references", references.len());

        assert_eq!(references.len(), 3, "Expected exactly 3 references in same file");

        for loc in &references {
            assert_eq!(
                loc.file_id, file1_id,
                "Local variable references should not cross file boundaries"
            );
        }
    }

    #[test]
    fn export_method_uses_name_usage_index_to_narrow_scope() {
        let mut db = RootDatabaseImpl::default();
        let file_a = FileId(0);
        let file_b = FileId(1);
        let file_c = FileId(2);

        let file_a_src = r#"
Процедура МояПроцедура() Экспорт
    МояПроцедура();
КонецПроцедуры
"#;
        let file_b_src = r#"
Процедура МояПроцедура()
    МояПроцедура();
КонецПроцедуры
"#;
        let file_c_src = r#"
Процедура НеПохожийМетод()
КонецПроцедуры
"#;

        let mut file_set = FileSet::new();
        file_set.insert(file_a, VfsPath::new("/a.bsl"));
        file_set.insert(file_b, VfsPath::new("/b.bsl"));
        file_set.insert(file_c, VfsPath::new("/c.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_a, SourceRootId(0));
        db.set_file_source_root(file_b, SourceRootId(0));
        db.set_file_source_root(file_c, SourceRootId(0));
        db.set_file_text(file_a, file_a_src);
        db.set_file_text(file_b, file_b_src);
        db.set_file_text(file_c, file_c_src);

        let def_offset = file_a_src.find("МояПроцедура").unwrap();
        let references = find_references(&db, file_a, TextSize::from(def_offset as u32));

        assert_eq!(references.len(), 2, "expected definition + 1 call in file A");
        for loc in &references {
            assert_eq!(loc.file_id, file_a);
        }
    }

    #[test]
    fn non_export_method_stays_file_local() {
        let mut db = RootDatabaseImpl::default();

        let file1_id = FileId(0);
        let file1_source = r#"
Процедура Помощник()
КонецПроцедуры

Процедура Тест1()
    Помощник();
КонецПроцедуры
"#;
        let file2_id = FileId(1);
        let file2_source = r#"
Процедура Помощник()
КонецПроцедуры

Процедура Тест2()
    Помощник();
КонецПроцедуры
"#;

        let mut file_set = FileSet::new();
        file_set.insert(file1_id, VfsPath::new("/a.bsl"));
        file_set.insert(file2_id, VfsPath::new("/b.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file1_id, SourceRootId(0));
        db.set_file_source_root(file2_id, SourceRootId(0));
        db.set_file_text(file1_id, file1_source);
        db.set_file_text(file2_id, file2_source);

        let def_offset = file1_source.find("Помощник").unwrap();
        let references = find_references(&db, file1_id, TextSize::from(def_offset as u32));

        assert_eq!(references.len(), 2, "definition + 1 call in file 1");
        for loc in &references {
            assert_eq!(
                loc.file_id, file1_id,
                "non-export procedure references must not cross file boundaries"
            );
        }
    }
}
