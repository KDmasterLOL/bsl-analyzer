//! Find References implementation.
//!
//! This module implements "Find References" functionality, which allows
//! finding all usages of a symbol.

use hir::{Semantics, Symbol};
use ide_db::RootDatabase;
use syntax::TextSize;
use vfs::FileId;

use crate::Location;

/// Find all references to the symbol at the given position.
///
/// Returns a vector of locations pointing to all references of the symbol,
/// or an empty vector if no symbol is found at the position.
///
/// For Iteration 8, this only works within the same file.
/// Cross-file references require a reference index (Iteration 9+).
pub fn find_references<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
) -> Vec<Location> {
    let sema = Semantics::new(db);

    // Get the symbol at the cursor position
    let symbol = match sema.symbol_at_position(file_id, offset) {
        Some(s) => s,
        None => return Vec::new(),
    };

    // Find references based on symbol type
    match symbol {
        Symbol::Method(method) => {
            let method_id = method.id();
            let file_ranges = sema.find_method_references(method_id);

            // Convert FileRange to Location
            file_ranges
                .into_iter()
                .map(|fr| Location { file_id: fr.file_id, range: fr.range })
                .collect()
        }
        Symbol::Variable(variable) => {
            let variable_id = variable.id();
            let file_ranges = sema.find_variable_references(variable_id);

            // Convert FileRange to Location
            file_ranges
                .into_iter()
                .map(|fr| Location { file_id: fr.file_id, range: fr.range })
                .collect()
        }
        Symbol::Parameter(_name) => {
            // Parameters don't have references tracking in Iteration 8
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::sync::Arc;
    use vfs::{file_set::FileSet, VfsPath};

    fn create_db_with_file(source: &str) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::default();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), Arc::new(source_root));
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
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

        // Find references from the definition
        let def_offset = source.find("МояПроцедура").unwrap();
        let offset = TextSize::from(def_offset as u32);

        let references = find_references(&db, file_id, offset);

        // Should find at least the definition and the calls
        assert!(
            references.len() >= 3,
            "Expected at least 3 references, found {}",
            references.len()
        );

        // All references should be in the same file
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

        // Find references from the declaration
        let decl_offset = source.find("МодульнаяПеременная").unwrap();
        let offset = TextSize::from(decl_offset as u32);

        let references = find_references(&db, file_id, offset);

        // Should find the declaration and usages
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

        // Find from lowercase usage
        let call_offset = source.find("мояпроцедура").unwrap();
        let offset = TextSize::from(call_offset as u32);

        let references = find_references(&db, file_id, offset);

        // Should find all case variants
        assert!(
            references.len() >= 3,
            "Expected at least 3 references, found {}",
            references.len()
        );
    }

    #[test]
    fn test_find_references_not_found() {
        let source = r#"
Процедура Тест()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        // Position on keyword "Процедура"
        let offset = source.find("Процедура").unwrap();
        let offset = TextSize::from(offset as u32);

        let references = find_references(&db, file_id, offset);
        // Keywords are not symbols
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

        // Find from the call site (second occurrence)
        let call_offset = source.rfind("МояПроцедура").unwrap();
        let offset = TextSize::from(call_offset as u32);

        let references = find_references(&db, file_id, offset);

        // Should find both definition and call
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

        // Find from definition
        let def_offset = source.find("МояФункция").unwrap();
        let offset = TextSize::from(def_offset as u32);

        let references = find_references(&db, file_id, offset);

        // Should find definition and both calls
        assert!(
            references.len() >= 3,
            "Expected at least 3 references, found {}",
            references.len()
        );
    }
}
