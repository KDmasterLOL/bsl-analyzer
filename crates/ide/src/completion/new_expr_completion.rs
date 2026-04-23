//! Completion in the `Новый <cursor>` position.
//!
//! Narrow, standalone context: when the cursor sits right after the `Новый`
//! keyword (possibly mid-typing an identifier), only platform types that have
//! at least one constructor should be offered. All other sources (locals,
//! keywords, global functions, MDO plurals, methods) are suppressed — showing
//! them would be actively misleading, since BSL does not accept any of them
//! in this position.

use bsl_platform::{platform_type_query, PlatformDataInner, TypeNameInput};
use ide_db::RootDatabase;
use rustc_hash::FxHashSet;
use syntax::{SyntaxKind, SyntaxToken};

use super::{CompletionItem, CompletionItemKind, CompletionPosition};

/// Returns `Some(items)` (possibly empty) when the cursor is inside a
/// `Новый <type>` context — even an empty list must short-circuit the
/// dispatcher so that BSL/MDO/platform completions don't clutter the popup.
/// Returns `None` to delegate to other resolvers.
pub(super) fn new_expr_completions<DB: RootDatabase>(
    db: &DB,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    let parse = db.parse(position.file_id);
    let root = parse.syntax_node();
    let cursor_token = root.token_at_offset(position.offset).left_biased()?;

    // Prefix = partially typed identifier when the cursor is on an IDENT.
    // For `Новый $0` (cursor on whitespace) prefix is empty.
    let (prefix, anchor) = if cursor_token.kind() == SyntaxKind::IDENT {
        (cursor_token.text().to_string(), Some(cursor_token.clone()))
    } else {
        (String::new(), Some(cursor_token.clone()))
    };

    if !is_after_new_keyword(anchor.as_ref()?) {
        return None;
    }

    let data = PlatformDataInner::instance();
    let prefix_lower = prefix.to_lowercase();
    let mut seen = FxHashSet::default();
    let mut items = Vec::new();

    // Deduplicate by english type name — a single type with N overloads must
    // surface as one completion entry. We rank by english name for stable
    // ordering across regenerations.
    let mut ctors: Vec<_> = data.all_constructors().iter().collect();
    ctors.sort_by(|a, b| a.type_name.as_str().cmp(b.type_name.as_str()));

    for ctor in ctors {
        if !seen.insert(ctor.type_name.as_str().to_string()) {
            continue;
        }
        // Public API: Salsa query, not the private `PlatformDataInner::get_type`.
        let Some(ty) = platform_type_query(db, TypeNameInput::new(db, ctor.type_name.to_string()))
        else {
            continue;
        };
        let russian = ty.name.to_string();
        let english = ty.english_name.to_string();

        // Fuzzy-ish starts_with on both language names, which matches existing
        // completion filters in platform_completion::render_completion_detail.
        if !prefix_lower.is_empty()
            && !russian.to_lowercase().starts_with(&prefix_lower)
            && !english.to_lowercase().starts_with(&prefix_lower)
        {
            continue;
        }

        items.push(CompletionItem {
            label: russian.to_string(),
            detail: Some(format!("{russian} / {english}")),
            kind: CompletionItemKind::Constructor,
            insert_text: russian.to_string(),
            documentation: None,
            sort_text: None,
            filter_text: Some(format!("{russian} {english}")),
            source: None,
        });
    }

    // Empty `items` is a valid signal "we're in `Новый ` but no type matches
    // the prefix" — still return Some to suppress other completion sources.
    Some(items)
}

/// `anchor` is the token the cursor sits on (or the nearest left-biased one).
/// Walks backwards across trivia tokens and returns `true` iff the first
/// non-trivia predecessor is `KW_NEW`.
///
/// We skip whitespace/newline/comment tokens explicitly — `prev_token()`
/// already returns siblings in source order, but rowan's token tree does not
/// fold trivia for us here.
fn is_after_new_keyword(anchor: &SyntaxToken) -> bool {
    let mut cur =
        if anchor.kind() == SyntaxKind::IDENT { anchor.prev_token() } else { Some(anchor.clone()) };
    while let Some(t) = cur.clone() {
        match t.kind() {
            SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT => {
                cur = t.prev_token();
            }
            SyntaxKind::KW_NEW => return true,
            _ => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Analysis;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::vfs::{file_set::FileSet, VfsPath};
    use ide_db::RootDatabaseImpl;

    fn setup(code: &str) -> (Analysis, vfs::FileId, u32) {
        let cursor = code.find("$0").expect("$0 marker");
        let cleaned = code.replace("$0", "");
        let offset = cursor as u32;

        let mut db = RootDatabaseImpl::new();
        let file_id = vfs::FileId(0);
        let mut fs = FileSet::new();
        fs.insert(file_id, VfsPath::new("/test.bsl"));
        db.set_source_root(SourceRootId(0), SourceRoot::new_local(fs));
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, &cleaned);
        (Analysis::from_database(db), file_id, offset)
    }

    fn all_labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    #[test]
    fn offers_only_constructors_in_new_position_empty_prefix() {
        // `Новый $0` — should list platform types with ctors, nothing else.
        let code = "Процедура Тест()
    Х = Новый $0
КонецПроцедуры";
        let (analysis, file_id, offset) = setup(code);
        let items = analysis.completions(file_id, offset, None);
        assert!(!items.is_empty(), "must offer at least one platform type");
        assert!(
            items.iter().all(|i| i.kind == CompletionItemKind::Constructor),
            "all items must be Constructor kind, got kinds: {:?}",
            items.iter().map(|i| i.kind).collect::<Vec<_>>()
        );
        let labels = all_labels(&items);
        assert!(labels.contains(&"Массив".to_string()), "labels={labels:?}");
        assert!(labels.contains(&"Структура".to_string()), "labels={labels:?}");
    }

    #[test]
    fn filters_by_prefix_russian() {
        let code = "Процедура Тест()
    Х = Новый Масс$0
КонецПроцедуры";
        let (analysis, file_id, offset) = setup(code);
        let items = analysis.completions(file_id, offset, None);
        let labels = all_labels(&items);
        assert!(
            labels.iter().all(|l| l.to_lowercase().starts_with("масс")),
            "every label must match prefix Масс, got: {labels:?}"
        );
        assert!(labels.contains(&"Массив".to_string()), "Массив must be offered");
    }

    #[test]
    fn does_not_fire_outside_new_position() {
        // Pure BSL completion — no `Новый` preceding.
        let code = "Процедура Тест()
    Х = $0
КонецПроцедуры";
        let (analysis, file_id, offset) = setup(code);
        let items = analysis.completions(file_id, offset, None);
        // Must NOT all be Constructor — regular BSL/keyword/global-fn completion
        // should be kicking in here.
        let constructor_only =
            !items.is_empty() && items.iter().all(|i| i.kind == CompletionItemKind::Constructor);
        assert!(!constructor_only, "outside `Новый ` we must not lock into constructor-only mode");
    }
}
