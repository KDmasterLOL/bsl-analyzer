use hir::ModItem;
use ide_db::SymbolKind;
use vfs::FileId;

use crate::DocumentSymbol;

pub(crate) fn document_symbols(
    db: &dyn ide_db::RootDatabase,
    file_id: FileId,
) -> Vec<DocumentSymbol> {
    let item_tree = db.item_tree(file_id);
    let region_tree = db.region_tree(file_id);

    let mut items: Vec<DocumentSymbol> = Vec::new();
    for mod_item in item_tree.top_level_items() {
        let sym = match mod_item {
            ModItem::Procedure(idx) => {
                let proc = item_tree.procedure(*idx);
                DocumentSymbol {
                    name: proc.name.as_str().to_string(),
                    kind: SymbolKind::Procedure,
                    range: proc.source_range,
                    selection_range: proc.name_range,
                    children: Vec::new(),
                }
            }
            ModItem::Function(idx) => {
                let func = item_tree.function(*idx);
                DocumentSymbol {
                    name: func.name.as_str().to_string(),
                    kind: SymbolKind::Function,
                    range: func.source_range,
                    selection_range: func.name_range,
                    children: Vec::new(),
                }
            }
            ModItem::Variable(idx) => {
                let var = item_tree.variable(*idx);
                DocumentSymbol {
                    name: var.name.as_str().to_string(),
                    kind: SymbolKind::Variable,
                    range: var.source_range,
                    selection_range: var.name_range,
                    children: Vec::new(),
                }
            }
        };
        items.push(sym);
    }

    if region_tree.is_empty() {
        return items;
    }

    fn build_region(
        db_region_tree: &hir::RegionTree,
        region_idx: hir::RegionIdx,
        items: &mut Vec<DocumentSymbol>,
    ) -> DocumentSymbol {
        let region = db_region_tree.region(region_idx);
        let region_range = region.range;

        let mut children: Vec<DocumentSymbol> = Vec::new();

        for &child_idx in db_region_tree.children(region_idx) {
            children.push(build_region(db_region_tree, child_idx, items));
        }

        let mut i = 0;
        while i < items.len() {
            if region_range.contains_range(items[i].range) {
                children.push(items.remove(i));
            } else {
                i += 1;
            }
        }

        DocumentSymbol {
            name: region.name.as_str().to_string(),
            kind: SymbolKind::Region,
            range: region_range,
            selection_range: region.name_range,
            children,
        }
    }

    let mut result: Vec<DocumentSymbol> = Vec::new();
    for &root_idx in region_tree.root_regions() {
        result.push(build_region(&region_tree, root_idx, &mut items));
    }

    result.append(&mut items);

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::vfs::{file_set::FileSet, VfsPath};
    use ide_db::RootDatabaseImpl;

    fn setup_db(code: &str) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, code);
        (db, file_id)
    }

    #[test]
    fn test_empty_file() {
        let (db, file_id) = setup_db("");
        let symbols = document_symbols(&db, file_id);
        assert!(symbols.is_empty());
    }

    #[test]
    fn test_procedures_and_functions() {
        let (db, file_id) = setup_db(
            r#"Процедура Проц1()
КонецПроцедуры

Функция Функ1()
КонецФункции"#,
        );
        let symbols = document_symbols(&db, file_id);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "Проц1");
        assert_eq!(symbols[0].kind, SymbolKind::Procedure);
        assert_eq!(symbols[1].name, "Функ1");
        assert_eq!(symbols[1].kind, SymbolKind::Function);
    }

    #[test]
    fn test_variables() {
        let (db, file_id) = setup_db("Перем МояПеременная Экспорт;");
        let symbols = document_symbols(&db, file_id);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "МояПеременная");
        assert_eq!(symbols[0].kind, SymbolKind::Variable);
    }

    #[test]
    fn test_regions_with_nested_items() {
        let (db, file_id) = setup_db(
            r#"#Область ПрограммныйИнтерфейс

Процедура Проц1()
КонецПроцедуры

#КонецОбласти

Процедура Проц2()
КонецПроцедуры"#,
        );
        let symbols = document_symbols(&db, file_id);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "ПрограммныйИнтерфейс");
        assert_eq!(symbols[0].kind, SymbolKind::Region);
        assert_eq!(symbols[0].children.len(), 1);
        assert_eq!(symbols[0].children[0].name, "Проц1");
        assert_eq!(symbols[1].name, "Проц2");
    }
}
