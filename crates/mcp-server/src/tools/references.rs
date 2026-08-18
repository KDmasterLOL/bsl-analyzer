//! Agent-facing `references` tool: every occurrence of ONE symbol, with what
//! each occurrence does at its site.
//!
//! Thin projection over [`ide::find_references_by_name`]. Every semantic
//! decision — which symbol an anchor names, whether it names more than one,
//! which occurrences count, and what kind each is — is taken in `ide`. This
//! module translates roots into file identities, projects places under the
//! location contract, applies the display limit and the output budget, and
//! assembles the envelope.
//!
//! The one thing it must never do is answer an empty list where the search
//! could not run: `outcome` carries that fact, and the whole tool exists
//! because a bare zero is what a client renames on.

use std::path::Path;

use bsl_search::WorkspaceRoots;
use ide::{
    BodySource, FileIdSet, ReferenceAnchor, ReferenceArea, ReferenceHit, ReferenceKind,
    ReferencesOutcome, ReferencesRequest, ReferencesResult,
};
use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;
use serde_json::{json, Map, Value};

use crate::diagnostics_state::DiagnosticsResident;
use crate::tools::location as loc;
use crate::tools::name_answer::{insert_resolved_location, NameAnswer};
use crate::tools::response::{structured, trim_items_to_budget, DEFAULT_OUTPUT_BUDGET_TOKENS};

/// Version of THIS tool's response shape. Separate from the location block's
/// own version: the block travels across tools and cannot be versioned by one.
pub(crate) const SCHEMA_VERSION: &str = "1";

pub(crate) const DEFAULT_LIMIT: usize = 50;
pub(crate) const MAX_LIMIT: usize = 500;
pub(crate) const DEFAULT_MAX_FILES: usize = 2000;
pub(crate) const MAX_MAX_FILES: usize = 10_000;

/// The request as the tool's parameters spell it, before any of it is resolved
/// against a resident.
pub(crate) struct Params<'a> {
    pub symbol: Option<&'a str>,
    /// Narrows where the DECLARATION is looked for — the way out of `ambiguous`.
    pub anchor_root_id: Option<&'a str>,
    /// Positional anchor: the root `path` is spelled against.
    pub root_id: Option<&'a str>,
    pub path: Option<&'a str>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    /// Narrows the REFERENCES shown, not the anchor.
    pub area_root_id: Option<&'a str>,
    pub area_path_prefix: Option<&'a str>,
    pub kinds: &'a [String],
    pub include_declaration: Option<bool>,
    pub limit: Option<usize>,
    pub max_files: Option<usize>,
}

/// What the tool answered, before it is stamped with the resident's identity.
pub(crate) struct Answer {
    body: Map<String, Value>,
    /// Whose data composes the body — decided in `ide`, echoed here.
    source: BodySource,
    completeness: loc::Completeness,
    /// Nothing matched by name. Held so the caller can retry once against a
    /// rescanned resident, the way `symbol_info` does with a card miss.
    miss: bool,
}

impl Answer {
    /// Whether the answer found nothing at all — a miss that a symbol added since the
    /// last throttled drift scan would produce just as well as an absent one.
    pub(crate) fn is_miss(&self) -> bool {
        self.miss
    }
}

/// How many files a `(root_id, path_prefix)` selection matched, published so a
/// mistyped prefix reads as "nothing is there" rather than "no references".
struct Selection {
    files: FileIdSet,
    root_id: Option<String>,
    path_prefix: Option<String>,
}

pub(crate) fn answer(
    resident: &DiagnosticsResident,
    params: &Params<'_>,
    max_output_tokens: usize,
) -> Result<Answer, McpError> {
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
    let max_files = params.max_files.unwrap_or(DEFAULT_MAX_FILES).min(MAX_MAX_FILES);

    let anchor = build_anchor(resident, params)?;
    // A position names its file outright, so there is no declaration to look for
    // in a root — and a parameter that is validated against the root table and
    // then dropped is worse than one that was refused: the caller is told its
    // root is wrong, never that its narrowing did nothing.
    if params.anchor_root_id.is_some() && matches!(anchor, ReferenceAnchor::Position { .. }) {
        return Err(McpError::invalid_params(
            "'anchor_root_id' picks which root the DECLARATION is looked for in, so it goes \
             with 'symbol'; a 'path'+'line' anchor already names one file. Use \
             'area_root_id' to narrow which references are shown",
            None,
        ));
    }
    let anchor_selection = match params.anchor_root_id {
        Some(root_id) => Some(select(resident, Some(root_id), None)?),
        None => None,
    };
    let area_selection = match (params.area_root_id, params.area_path_prefix) {
        (None, None) => None,
        (root_id, prefix) => Some(select(resident, root_id, prefix)?),
    };

    let request = ReferencesRequest {
        anchor,
        anchor_files: anchor_selection.as_ref().map(|s| s.files.clone()),
        area: ReferenceArea { files: area_selection.as_ref().map(|s| s.files.clone()) },
        kinds: parse_kinds(params.kinds)?,
        include_declaration: params.include_declaration.unwrap_or(true),
        max_files,
    };

    let db = resident.db();
    let result = ide::find_references_by_name(db, &request);
    Ok(render(
        db,
        resident.workspace_roots(),
        &result,
        params,
        area_selection,
        limit,
        max_output_tokens,
        resident.unread_count(),
    ))
}

/// Publish the answer with the identity of whoever composed its body.
pub(crate) fn finish(answer: Answer, revision: u64, topology: u64, stale: bool) -> CallToolResult {
    let Answer { mut body, source, completeness, miss: _ } = answer;
    let freshness = match source {
        // A body of reference hits, or of a symbol the resident resolved, is the
        // resident's own answer at one revision — even when it was the name
        // dictionary that found the anchor.
        BodySource::Resident => loc::Freshness::new(loc::FreshnessSource::Resident, completeness)
            .with_revision(revision)
            .with_topology(topology)
            .with_stale(stale),
        // A body of dictionary candidates spans several artefacts with different
        // revisions; borrowing the resident's would claim an identity they do
        // not share.
        BodySource::NameDictionary => NameAnswer::freshness(completeness),
    };
    body.insert("freshness".into(), freshness.to_value());
    structured(Value::Object(body))
}

fn build_anchor(
    resident: &DiagnosticsResident,
    params: &Params<'_>,
) -> Result<ReferenceAnchor, McpError> {
    if let Some(symbol) = params.symbol.map(str::trim).filter(|s| !s.is_empty()) {
        if params.path.is_some() {
            return Err(McpError::invalid_params(
                "pass either 'symbol' or 'path'+'line', not both: they anchor on different \
                 things and the answer would not say which one it followed",
                None,
            ));
        }
        // `root_id` qualifies `path` and nothing else. Accepting it beside a
        // symbol — and validating it against the root table, as `select` would —
        // tells the caller its narrowing was understood while the walk spans
        // every root. `anchor_root_id` is the parameter that narrows a name.
        if params.line.is_some() || params.column.is_some() {
            return Err(McpError::invalid_params(
                "'line'/'column' belong to a 'path' anchor; a 'symbol' is resolved by name \
                 and has no position to start from",
                None,
            ));
        }
        if params.root_id.is_some() {
            return Err(McpError::invalid_params(
                "'root_id' spells out which root 'path' is relative to, so it needs a \
                 'path'; to look for a DECLARATION of 'symbol' in one root pass \
                 'anchor_root_id'",
                None,
            ));
        }
        return Ok(ReferenceAnchor::Name(symbol.to_string()));
    }

    let path = params.path.ok_or_else(|| {
        McpError::invalid_params("one of 'symbol' or 'path'+'line' is required", None)
    })?;
    let resolved = resident
        .resolve_rooted_path(params.root_id, Path::new(path))
        .map_err(|error| McpError::invalid_params(error.to_string(), None))?;
    let file_id = resident.file_id_for(resolved.as_path()).ok_or_else(|| {
        if resident.is_unread(resolved.as_path()) {
            McpError::invalid_params(
                format!(
                    "'{}' is a workspace .bsl file whose bytes could not be read; it is held \
                     out of service and re-read every drift window",
                    resolved.display()
                ),
                None,
            )
        } else {
            McpError::invalid_params(
                format!("'{}' is not a resident workspace .bsl file", resolved.display()),
                None,
            )
        }
    })?;
    let line = params
        .line
        .ok_or_else(|| McpError::invalid_params("'line' is required with 'path'", None))?;
    Ok(ReferenceAnchor::Position { file_id, line, column: params.column.unwrap_or(0) })
}

/// Translate a `(root_id, path_prefix)` selection into the set of files it
/// names.
///
/// Identity, not string comparison: a root is DECLARED with one spelling and
/// INDEXED with another whenever a link is involved, so a prefix built from the
/// declared spelling would match none of the indexed paths and the answer would
/// be an honest-looking zero. The attributor that mints every published pair is
/// asked instead, and the prefix is then compared inside the key — the same
/// address the histogram publishes.
fn select(
    resident: &DiagnosticsResident,
    root_id: Option<&str>,
    path_prefix: Option<&str>,
) -> Result<Selection, McpError> {
    let roots = resident.workspace_roots();
    if let Some(root_id) = root_id {
        if !roots.contains_id(root_id) {
            return Err(McpError::invalid_params(
                format!(
                    "no source root is registered under '{root_id}'; `search` reports the \
                     roots this workspace knows"
                ),
                None,
            ));
        }
    }
    let prefix = path_prefix.map(|prefix| prefix.replace('\\', "/").trim_matches('/').to_string());

    let mut files = FileIdSet::default();
    for (path, file_id) in resident.files() {
        // `root_of` and not `key_of_path`: the latter canonicalises what it is
        // given, and a resident path is canonical already — one `canonicalize`
        // syscall per workspace file, on a configuration with tens of thousands
        // of them, for an answer that is in hand. Both spellings are passed as
        // the one the resident holds, so the attribution is the same and the
        // declared-spelling fallback still catches a path canonicalisation could
        // not resolve.
        let Some(key) = roots.root_of(path, path) else { continue };
        if root_id.is_some_and(|wanted| key.root_id != wanted) {
            continue;
        }
        if let Some(prefix) = &prefix {
            let key_path = key.path.replace('\\', "/");
            let under = key_path == *prefix
                || key_path.starts_with(&format!("{prefix}/"))
                || prefix.is_empty();
            if !under {
                continue;
            }
        }
        files.insert(file_id);
    }

    Ok(Selection {
        files,
        root_id: root_id.map(str::to_string),
        path_prefix: prefix.filter(|p| !p.is_empty()),
    })
}

fn parse_kinds(kinds: &[String]) -> Result<Option<Vec<ReferenceKind>>, McpError> {
    if kinds.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(kinds.len());
    for kind in kinds {
        let parsed = ReferenceKind::parse(kind.trim()).ok_or_else(|| {
            McpError::invalid_params(
                format!("unknown reference kind '{kind}'; expected {}", KIND_VOCABULARY),
                None,
            )
        })?;
        out.push(parsed);
    }
    Ok(Some(out))
}

pub(crate) const KIND_VOCABULARY: &str = "declaration | call | write | read";

fn outcome_str(outcome: &ReferencesOutcome) -> &'static str {
    match outcome {
        ReferencesOutcome::Resolved => "resolved",
        ReferencesOutcome::Ambiguous => "ambiguous",
        ReferencesOutcome::NotFound => "not_found",
        ReferencesOutcome::UnsupportedSymbol { .. } => "unsupported_symbol",
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the projection needs the database, the root table, the answer, the request that \
              produced it and both caps; grouping them would be a struct built at the one call \
              site and taken apart in the first statement"
)]
fn render(
    db: &ide::RootDatabaseImpl,
    roots: &WorkspaceRoots,
    result: &ReferencesResult,
    params: &Params<'_>,
    area: Option<Selection>,
    limit: usize,
    max_output_tokens: usize,
    // Files the resident holds out of service because their bytes could not be read. A
    // walk over "the workspace" that quietly means "the part of it that could be read" is
    // the silent incompleteness this tool exists to end.
    unread_files: usize,
) -> Answer {
    let mut body = Map::new();
    body.insert("schema_version".into(), json!(SCHEMA_VERSION));
    body.insert("outcome".into(), json!(outcome_str(&result.outcome)));
    // The name the walk ran on, not the one that arrived: `build_anchor` trims it, and an
    // echo that still carries the caller's whitespace is a different string from the one the
    // answer is about.
    if let Some(symbol) = params.symbol.map(str::trim).filter(|s| !s.is_empty()) {
        body.insert("symbol".into(), json!(symbol));
    }
    if let Some(area) = &area {
        body.insert(
            "area".into(),
            json!({
                "root_id": area.root_id,
                "path_prefix": area.path_prefix,
                // A prefix that names nothing says so here, instead of being
                // read off an empty reference list as "no references".
                "files_in_area": area.files.len(),
            }),
        );
    }

    let total = result.hits.len();
    let mut completeness = loc::Completeness::complete();

    match &result.outcome {
        ReferencesOutcome::Resolved => {
            let shown: Vec<&ReferenceHit> = result.hits.iter().take(limit).collect();
            let mut items: Vec<Value> = shown.iter().map(|hit| hit_value(db, roots, hit)).collect();
            let budget_cut = trim_items_to_budget(&mut items, max_output_tokens);
            let capped = total > shown.len();

            body.insert("references".into(), Value::Array(items));
            body.insert("total".into(), json!(total));
            body.insert("total_is_lower_bound".into(), json!(result.files_capped));
            // Two answers taken with a truncated walk count different candidate
            // sets, so the narrowing strategy that replaces a cursor does not
            // apply to them — said in a field rather than left to be inferred.
            body.insert("narrowing_comparable".into(), json!(!result.files_capped));
            body.insert("files_scanned".into(), json!(result.files_scanned));

            let used_tokens = items_tokens(&body);
            let histogram_budget = max_output_tokens.saturating_sub(used_tokens);
            let mut buckets: Vec<Value> = result
                .per_file
                .iter()
                .map(|(file_id, count)| {
                    let mut entry = Map::new();
                    insert_resolved_location(
                        &ide::resolve_file_range(db, *file_id, None, None),
                        Some(roots),
                        &mut entry,
                    );
                    entry.insert("count".into(), json!(count));
                    Value::Object(entry)
                })
                .collect();
            let buckets_before = buckets.len();
            let histogram_over_budget = trim_items_to_budget(&mut buckets, histogram_budget);
            // `trim_items_to_budget` keeps one item even when that item alone
            // exceeds the budget, and reports the overflow. Reporting THAT as a
            // truncated histogram would claim whole files were lost while the
            // counts still add up to `total` — the flag exists to say the
            // opposite, and I10 ties it to a strictly smaller sum.
            let histogram_cut = buckets.len() < buckets_before;
            body.insert("files".into(), Value::Array(buckets));
            // A shortened histogram loses whole files, and the caller walks it
            // to see everything the limit hid — so its truncation is named
            // apart from the body's.
            body.insert("histogram_truncated".into(), json!(histogram_cut));

            completeness = completeness
                .when(
                    capped,
                    loc::ReasonCode::ResultCap,
                    "more references than `limit`; narrow with `area_root_id`/\
                     `area_path_prefix`/`kinds`, or walk `files`",
                )
                .when(
                    result.files_capped,
                    loc::ReasonCode::ResultCap,
                    "the walk stopped at `max_files`, so `total` is a lower bound; raise \
                     `max_files` or narrow the area before the walk",
                )
                .when(
                    budget_cut || histogram_over_budget,
                    loc::ReasonCode::OutputBudget,
                    "trimmed to fit `max_output_tokens`",
                );
        }
        ReferencesOutcome::Ambiguous | ReferencesOutcome::NotFound => {
            let mut budget_cut = false;
            if !result.declarations.is_empty() {
                let candidates: Vec<Value> = result
                    .declarations
                    .iter()
                    .map(|declaration| {
                        let mut entry = Map::new();
                        insert_resolved_location(
                            &ide::resolve_file_range(
                                db,
                                declaration.file_id,
                                Some(declaration.name_range),
                                declaration.enclosing_range,
                            ),
                            Some(roots),
                            &mut entry,
                        );
                        entry.insert(
                            "kind".into(),
                            json!(match declaration.kind {
                                ide::DeclarationKind::Method => "method",
                                ide::DeclarationKind::Variable => "variable",
                            }),
                        );
                        Value::Object(entry)
                    })
                    .collect();
                // The hint names the axis that can actually separate THESE
                // declarations. `anchor_root_id` separates roots and nothing
                // else, and an object module and its manager module sit in one
                // root — pointing an agent at it there would send it down a road
                // that cannot arrive.
                let roots_differ = distinct_roots(&candidates) > 1;
                let mut candidates = candidates;
                budget_cut |= trim_items_to_budget(&mut candidates, max_output_tokens);
                body.insert("declarations".into(), Value::Array(candidates));
                body.insert(
                    "resolution_hint".into(),
                    json!(if roots_differ {
                        "pass `anchor_root_id` to pick one declaration"
                    } else {
                        "these declarations share a root: anchor positionally with the `path` and `line` of the one you want"
                    }),
                );
            }
            if let Some(lookup) = &result.candidates {
                let (lookup_body, lookup_completeness, cut) =
                    lookup_value(db, roots, lookup, remaining_budget(&body, max_output_tokens));
                completeness = lookup_completeness;
                budget_cut |= cut;
                body.insert("lookup".into(), lookup_body);
                // A dictionary ambiguity is not separated by a root — its
                // candidates are spread over kinds, not roots — and the axis
                // that ends it is their own qualified name, passed back as
                // `symbol`. Leaving the field out here would send the agent to
                // the tool description, which can only name a general axis.
                if matches!(result.outcome, ReferencesOutcome::Ambiguous)
                    && !body.contains_key("resolution_hint")
                {
                    body.insert(
                        "resolution_hint".into(),
                        json!("pass one `lookup.candidates[].address.symbol` back as `symbol`"),
                    );
                }
            }
            completeness = completeness.when(
                budget_cut,
                loc::ReasonCode::OutputBudget,
                "candidate list trimmed to fit `max_output_tokens`",
            );
        }
        ReferencesOutcome::UnsupportedSymbol { category } => {
            body.insert(
                "unsupported".into(),
                json!({
                    "category": category.as_str(),
                    "reason": "this symbol has no reference walk; an empty list would be \
                               indistinguishable from a proven zero",
                }),
            );
            // The dictionary composed this answer too when the category came from
            // one of its candidates, and the envelope says so — `name-dictionary`
            // has no revision of its own, and the per-source truth is the
            // `providers` array. Dropping it here would leave an envelope that
            // names a source and shows nothing of what it could and could not
            // consult.
            if let Some(lookup) = &result.candidates {
                let (lookup_body, lookup_completeness, cut) =
                    lookup_value(db, roots, lookup, remaining_budget(&body, max_output_tokens));
                completeness = lookup_completeness.when(
                    cut,
                    loc::ReasonCode::OutputBudget,
                    "candidate list trimmed to fit `max_output_tokens`",
                );
                body.insert("lookup".into(), lookup_body);
            }
        }
    }

    Answer {
        body,
        source: result.body_source,
        completeness: completeness
            .when(
                result.anchor_candidates_capped,
                loc::ReasonCode::ResultCap,
                "the anchor was chosen from a capped candidate list, so the outcome itself \
                 may miss a declaration; pass a qualified `symbol` to decide it exactly",
            )
            .when(
                unread_files > 0,
                loc::ReasonCode::UnreadableFiles,
                "some workspace files could not be read and were left out of the walk",
            ),
        miss: matches!(result.outcome, ReferencesOutcome::NotFound),
    }
}

/// The dictionary's own answer, under its own key: this tool's `total` counts
/// occurrences and the dictionary's counts candidates, and one key carrying two
/// incomparable numbers is the confusion `outcome` exists to end.
fn lookup_value(
    db: &ide::RootDatabaseImpl,
    roots: &WorkspaceRoots,
    lookup: &ide::NameLookupResult,
    budget: usize,
) -> (Value, loc::Completeness, bool) {
    let mut answer = NameAnswer::render(db, Some(roots), lookup);
    let before = answer.candidates.len();
    let cut = trim_items_to_budget(&mut answer.candidates, budget);
    // `truncated` is the dictionary's own promise that the list is complete
    // against `total`. Trimming the list here and leaving the flag alone would
    // break that promise from the outside, where nothing but the two counts
    // could betray it.
    answer.truncated |= answer.candidates.len() < before;
    let mut lookup_body = Map::new();
    let completeness = answer.insert_into(&mut lookup_body);
    (Value::Object(lookup_body), completeness, cut)
}

/// What the budget has left after the body already written — the same
/// ~4-chars-a-token unit the trimming measures in.
fn remaining_budget(body: &Map<String, Value>, max_output_tokens: usize) -> usize {
    max_output_tokens.saturating_sub(items_tokens(body))
}

/// How many distinct roots a declaration list spans — the question that decides
/// whether narrowing by root can separate them at all.
fn distinct_roots(declarations: &[Value]) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    for declaration in declarations {
        let Some(root_id) = declaration["location"]["root_id"].as_str() else { continue };
        if !seen.contains(&root_id) {
            seen.push(root_id);
        }
    }
    seen.len()
}

fn items_tokens(body: &Map<String, Value>) -> usize {
    serde_json::to_string(body).map(|text| text.len()).unwrap_or(0).div_ceil(4)
}

fn hit_value(db: &ide::RootDatabaseImpl, roots: &WorkspaceRoots, hit: &ReferenceHit) -> Value {
    let mut entry = Map::new();
    insert_resolved_location(
        &ide::resolve_file_range(db, hit.file_id, Some(hit.range), hit.enclosing_range),
        Some(roots),
        &mut entry,
    );
    entry.insert("kind".into(), json!(hit.kind.as_str()));
    Value::Object(entry)
}

/// The default budget, so the tool and its schema quote one number.
pub(crate) const DEFAULT_BUDGET: usize = DEFAULT_OUTPUT_BUDGET_TOKENS;

/// The published shape of this tool's `structuredContent`.
///
/// A schema and nothing else: the body is assembled as a `Value` because a place
/// and a freshness envelope are serialized by the location contract alone, and
/// re-declaring their shape here would be a second spelling to keep in agreement
/// with the first. What this type does declare is which keys this tool serves and
/// which of them are always there.
#[derive(schemars::JsonSchema)]
#[allow(
    dead_code,
    reason = "the type exists to publish `outputSchema`; the body it describes is built as a \
              `Value` so the location contract stays the only serializer of a place"
)]
pub(crate) struct ReferencesResponse {
    /// Version of this response shape. Always `"1"`.
    schema_version: String,
    /// `resolved` | `ambiguous` | `not_found` | `unsupported_symbol`.
    outcome: String,
    /// The `symbol` the request anchored on, echoed back.
    symbol: Option<String>,
    /// The area filter as it was applied, with how many files it selected.
    area: Option<Value>,
    /// `resolved` only: the occurrences, each with a `location` (or
    /// `location_unavailable`) and a `kind`.
    references: Option<Vec<Value>>,
    /// `resolved` only: how many occurrences passed the area and kind filters,
    /// counted before `limit`.
    total: Option<usize>,
    /// `resolved` only: the walk stopped at `max_files`, so `total` counts only
    /// what was walked.
    total_is_lower_bound: Option<bool>,
    /// `resolved` only: whether this answer may be compared with a narrower one.
    /// False once the walk was truncated: the two counted different candidate sets.
    narrowing_comparable: Option<bool>,
    /// `resolved` only: how many candidate files were walked.
    files_scanned: Option<usize>,
    /// `resolved` only: per-file counts over the whole `total`, most-populated
    /// first — what a caller walks instead of a cursor.
    files: Option<Vec<Value>>,
    /// `resolved` only: the histogram itself was cut by the output budget, so
    /// whole files are missing from it.
    histogram_truncated: Option<bool>,
    /// `ambiguous` by qualified name: the declarations to choose between.
    declarations: Option<Vec<Value>>,
    /// How to turn an `ambiguous` answer into a resolved one.
    resolution_hint: Option<String>,
    /// `unsupported_symbol` only: what the name turned out to be — `category` from
    /// the closed vocabulary `common_module` | `module` | `metadata_object` |
    /// `metadata_member` | `form` | `platform_member` | `unknown_scope`, and why
    /// there is no list.
    unsupported: Option<Value>,
    /// `ambiguous`/`not_found` by name: the name dictionary's answer —
    /// `candidates`, its own `total`/`total_exact`/`truncated` over THEM, and the
    /// `providers` it could and could not consult. Nested because its `total`
    /// counts candidates while this tool's counts occurrences.
    lookup: Option<Value>,
    /// Who answered, at which revision and topology, and whether the answer is whole.
    freshness: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The section of the tools document that describes this tool.
    ///
    /// The bound is not decoration: `call` and `read` occur throughout that file in other
    /// senses, so a gate reading the whole document would pass on a vocabulary this tool's
    /// own section never names — and that section is what a consumer writes its closed list
    /// from.
    fn references_section() -> &'static str {
        let document = include_str!("../../../../docs/mcp/TOOLS_AND_EXTENSION.md");
        let after_heading = document
            .split_once("\n## Ссылки на символ\n")
            .expect("the document describes this tool under its own heading")
            .1;
        match after_heading.split_once("\n## ") {
            Some((section, _)) => section,
            None => after_heading,
        }
    }

    /// Two closed vocabularies leave this tool on the wire, and their only worth over
    /// free-form strings is that a consumer may match on them. That holds while the document
    /// a consumer reads carries every value.
    #[test]
    fn every_published_value_is_named_where_the_tool_is_described() {
        let section = references_section();

        let outcomes = [
            ReferencesOutcome::Resolved,
            ReferencesOutcome::Ambiguous,
            ReferencesOutcome::NotFound,
            ReferencesOutcome::UnsupportedSymbol {
                category: ide::UnsupportedCategory::UnknownScope,
            },
        ];
        for outcome in &outcomes {
            let code = outcome_str(outcome);
            assert!(
                section.contains(&format!("`{code}`")),
                "outcome `{code}` is served but the tool's section does not name it",
            );
        }

        for category in ide::UnsupportedCategory::ALL {
            let code = category.as_str();
            assert!(
                section.contains(&format!("`{code}`")),
                "category `{code}` is served but the tool's section does not name it",
            );
        }

        for kind in [
            ReferenceKind::Declaration,
            ReferenceKind::Call,
            ReferenceKind::Write,
            ReferenceKind::Read,
        ] {
            let code = kind.as_str();
            assert!(
                section.contains(&format!("`{code}`")),
                "kind `{code}` is served but the tool's section does not name it",
            );
            assert!(
                KIND_VOCABULARY.contains(code),
                "kind `{code}` is served but the published vocabulary omits it",
            );
        }
    }

    /// Every kind the classifier can answer round-trips through the wire spelling — the one
    /// a `kinds` filter is written with. A value that serialized to a string the parser
    /// refuses would make its own filter unusable.
    #[test]
    fn every_kind_parses_back_from_its_wire_spelling() {
        for kind in [
            ReferenceKind::Declaration,
            ReferenceKind::Call,
            ReferenceKind::Write,
            ReferenceKind::Read,
        ] {
            assert_eq!(ReferenceKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ReferenceKind::parse("handler"), None, "an undeclared kind is refused");
    }

    /// A workspace the resident could not read whole is not a workspace-wide enumeration,
    /// and the answer has to say so — `symbol_info` already stamps the same code on the same
    /// universe of files. The control is the identical answer with no unread files: same
    /// body, same anchor, one number apart.
    #[test]
    fn an_unread_file_makes_the_walk_partial() {
        use base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use vfs::{FileId, FileSet, VfsPath};

        let mut db = ide::RootDatabaseImpl::new();
        let module = FileId(0);
        let mut file_set = FileSet::default();
        file_set.insert(module, VfsPath::new("/ws/src/cf/CommonModules/Продажи/Ext/Module.bsl"));
        db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
        db.set_file_source_root(module, SourceRootId(0));
        db.set_file_text(module, "Процедура Расчёт() Экспорт\nКонецПроцедуры\n");

        let (roots, _) = bsl_search::WorkspaceRoots::build(
            std::path::Path::new("/ws"),
            std::path::Path::new("/ws/src/cf"),
            &[],
        );
        let request = ide::ReferencesRequest {
            anchor: ide::ReferenceAnchor::Name("Продажи.Расчёт".to_owned()),
            anchor_files: None,
            area: ide::ReferenceArea::default(),
            kinds: None,
            include_declaration: true,
            max_files: DEFAULT_MAX_FILES,
        };
        let result = ide::find_references_by_name(&db, &request);
        let params = Params {
            symbol: Some("Продажи.Расчёт"),
            anchor_root_id: None,
            root_id: None,
            path: None,
            line: None,
            column: None,
            area_root_id: None,
            area_path_prefix: None,
            kinds: &[],
            include_declaration: None,
            limit: None,
            max_files: None,
        };

        let read_whole =
            render(&db, &roots, &result, &params, None, DEFAULT_LIMIT, DEFAULT_BUDGET, 0);
        assert!(
            read_whole.completeness.is_complete(),
            "control: nothing was held out of service: {:?}",
            read_whole.completeness.to_value(),
        );

        let with_a_hole =
            render(&db, &roots, &result, &params, None, DEFAULT_LIMIT, DEFAULT_BUDGET, 1);
        let reasons = with_a_hole.completeness.to_value();
        assert!(
            reasons["reasons"]
                .as_array()
                .expect("reasons")
                .iter()
                .any(|reason| reason["code"] == "unreadable_files"),
            "a file the resident could not read is a gap in the walk: {reasons}",
        );
    }
}
