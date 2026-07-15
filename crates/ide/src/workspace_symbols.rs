use hir::{is_bsl_source, ModItem};
use ide_db::base_db::SourceRootId;
use ide_db::{RootDatabase, SymbolKind};
use stdx::case::CaseExt;
use syntax::TextRange;
use vfs::FileId;

/// A workspace-wide symbol — an exported top-level declaration the user can jump
/// to via `workspace/symbol`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file_id: FileId,
    /// Full declaration span (LSP `location.range`).
    pub range: TextRange,
    /// Name token span (used when the client wants the selection).
    pub selection_range: TextRange,
}

/// Cap results so a broad query cannot flood the client.
const MAX_RESULTS: usize = 256;

/// Exported procedures, functions and module variables across the source root
/// whose name contains `query` (case-insensitive, bilingual folding).
///
/// Body-free: reads only item trees, which are salsa-cached and LRU-evicted, so
/// the whole-root scan stays within the compact-storage budget. An empty query
/// returns nothing rather than dumping every symbol.
pub fn workspace_symbols<DB: RootDatabase>(
    db: &DB,
    source_root_id: SourceRootId,
    query: &str,
) -> Vec<WorkspaceSymbol> {
    let needle = query.fold_lower();
    if needle.is_empty() {
        return Vec::new();
    }

    let source_root = db.source_root_input(source_root_id).root(db);
    let file_set = source_root.file_set();

    let mut symbols = Vec::new();
    for file_id in source_root.iter() {
        if !is_bsl_source(file_set, file_id) {
            continue;
        }
        db.unwind_if_revision_cancelled();
        collect_file_symbols(db, file_id, &needle, &mut symbols);
    }

    rank(&mut symbols, &needle);
    symbols.truncate(MAX_RESULTS);
    symbols
}

fn collect_file_symbols<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    needle: &str,
    out: &mut Vec<WorkspaceSymbol>,
) {
    let item_tree = db.item_tree(file_id);
    for mod_item in item_tree.top_level_items() {
        let (name, is_export, range, selection_range, kind) = match mod_item {
            ModItem::Procedure(idx) => {
                let proc = item_tree.procedure(*idx);
                (
                    &proc.name,
                    proc.is_export,
                    proc.source_range,
                    proc.name_range,
                    SymbolKind::Procedure,
                )
            }
            ModItem::Function(idx) => {
                let func = item_tree.function(*idx);
                (
                    &func.name,
                    func.is_export,
                    func.source_range,
                    func.name_range,
                    SymbolKind::Function,
                )
            }
            ModItem::Variable(idx) => {
                let var = item_tree.variable(*idx);
                (&var.name, var.is_export, var.source_range, var.name_range, SymbolKind::Variable)
            }
        };
        if !is_export {
            continue;
        }
        if !name.as_str().fold_lower().contains(needle) {
            continue;
        }
        out.push(WorkspaceSymbol {
            name: name.as_str().to_string(),
            kind,
            file_id,
            range,
            selection_range,
        });
    }
}

/// Prefix matches first, then alphabetical — a cheap, stable ordering that keeps
/// the most likely target near the top without a full fuzzy scorer.
fn rank(symbols: &mut [WorkspaceSymbol], needle: &str) {
    symbols.sort_by_cached_key(|symbol| {
        let lower = symbol.name.fold_lower();
        let prefix_rank = u8::from(!lower.starts_with(needle));
        (prefix_rank, lower)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use vfs::{file_set::FileSet, VfsPath};

    fn db_with_files(files: &[(&str, &str)]) -> RootDatabaseImpl {
        let mut db = RootDatabaseImpl::default();
        let mut file_set = FileSet::new();
        for (i, (path, _)) in files.iter().enumerate() {
            file_set.insert(FileId(i as u32), VfsPath::new(*path));
        }
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        for (i, (_, text)) in files.iter().enumerate() {
            let file_id = FileId(i as u32);
            db.set_file_source_root(file_id, SourceRootId(0));
            db.set_file_text(file_id, text);
        }
        db
    }

    fn names(symbols: &[WorkspaceSymbol]) -> Vec<&str> {
        symbols.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn finds_exported_methods_across_files() {
        let db = db_with_files(&[
            ("/a.bsl", "Функция ОбщийРасчёт() Экспорт\nКонецФункции\n"),
            ("/b.bsl", "Процедура ОбщаяЗапись() Экспорт\nКонецПроцедуры\n"),
        ]);
        let found = workspace_symbols(&db, SourceRootId(0), "Общ");
        assert_eq!(names(&found), vec!["ОбщаяЗапись", "ОбщийРасчёт"]);
    }

    #[test]
    fn excludes_non_exported_methods() {
        let db = db_with_files(&[(
            "/a.bsl",
            "Функция Публичная() Экспорт\nКонецФункции\n\nФункция Приватная()\nКонецФункции\n",
        )]);
        let found = workspace_symbols(&db, SourceRootId(0), "Пуб");
        assert_eq!(names(&found), vec!["Публичная"]);
        assert!(workspace_symbols(&db, SourceRootId(0), "Прив").is_empty());
    }

    #[test]
    fn empty_query_returns_nothing() {
        let db = db_with_files(&[("/a.bsl", "Функция Ф() Экспорт\nКонецФункции\n")]);
        assert!(workspace_symbols(&db, SourceRootId(0), "").is_empty());
    }

    #[test]
    fn maps_kinds_and_folds_case() {
        let db = db_with_files(&[(
            "/a.bsl",
            "Перем СчётчикЗапросов Экспорт;\n\nПроцедура ВыполнитьЗапрос() Экспорт\nКонецПроцедуры\n\nФункция ПолучитьЗапрос() Экспорт\nКонецФункции\n",
        )]);
        // Lowercase query matches the mixed-case declarations.
        let found = workspace_symbols(&db, SourceRootId(0), "запрос");
        let kinds: Vec<(&str, SymbolKind)> =
            found.iter().map(|s| (s.name.as_str(), s.kind)).collect();
        assert!(kinds.contains(&("СчётчикЗапросов", SymbolKind::Variable)), "{kinds:?}");
        assert!(kinds.contains(&("ВыполнитьЗапрос", SymbolKind::Procedure)), "{kinds:?}");
        assert!(kinds.contains(&("ПолучитьЗапрос", SymbolKind::Function)), "{kinds:?}");
    }

    #[test]
    fn prefix_matches_rank_first() {
        let db = db_with_files(&[(
            "/a.bsl",
            "Функция ЗначениеПоУмолчанию() Экспорт\nКонецФункции\n\nФункция ПолучитьЗначение() Экспорт\nКонецФункции\n",
        )]);
        let found = workspace_symbols(&db, SourceRootId(0), "Значение");
        // The name that STARTS with the query outranks the one that only contains it.
        assert_eq!(names(&found), vec!["ЗначениеПоУмолчанию", "ПолучитьЗначение"]);
    }
}
