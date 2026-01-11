//! Go to Definition implementation.
//!
//! This module implements "Go to Definition" functionality, which allows
//! navigating from a symbol usage to its definition.

use hir::{Method, Semantics, Symbol, Variable};
use ide_db::RootDatabase;
use syntax::TextSize;
use vfs::FileId;

use crate::{NavigationTarget, SymbolKind};

/// Go to the definition of the symbol at the given position.
///
/// Returns a navigation target pointing to the symbol's definition,
/// or None if no symbol is found at the position.
///
/// For Iteration 8, this only works within the same file.
/// Cross-file navigation requires module graph (Iteration 9.5+).
pub fn goto_definition<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
) -> Option<NavigationTarget> {
    let _span =
        tracing::info_span!("goto_definition", ?file_id, offset = u32::from(offset)).entered();

    let sema = Semantics::new(db);

    // Get the symbol at the cursor position
    let symbol = {
        let _span = tracing::debug_span!("symbol_at_position").entered();
        sema.symbol_at_position(file_id, offset)?
    };

    // Convert the symbol to a navigation target
    match symbol {
        Symbol::Method(method) => method_to_navigation_target(method),
        Symbol::Variable(variable) => variable_to_navigation_target(variable),
        Symbol::Parameter(_name) => {
            // Parameters don't have a separate definition location in Iteration 8
            // They're defined inline in the parameter list
            None
        }
    }
}

/// Convert a Method to a NavigationTarget.
fn method_to_navigation_target<DB: RootDatabase>(
    method: Method<'_, DB>,
) -> Option<NavigationTarget> {
    let file_id = method.id().module.file_id;
    let range = method.source_range()?;
    let name = method.name().as_str().to_string();

    let kind = if method.is_function() { SymbolKind::Function } else { SymbolKind::Procedure };

    Some(NavigationTarget { file_id, range, name, kind })
}

/// Convert a Variable to a NavigationTarget.
fn variable_to_navigation_target<DB: RootDatabase>(
    variable: Variable<'_, DB>,
) -> Option<NavigationTarget> {
    let file_id = variable.id().module.file_id;
    let range = variable.source_range()?;
    let name = variable.name().as_str().to_string();

    Some(NavigationTarget { file_id, range, name, kind: SymbolKind::Variable })
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

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
        db.set_file_text(file_id, source);

        (db, file_id)
    }

    #[test]
    fn test_goto_definition_method() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры

Функция Тест()
    МояПроцедура();
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);

        // Find position of method call "МояПроцедура" in the body
        let call_offset = source.rfind("МояПроцедура").unwrap();
        let offset = TextSize::from(call_offset as u32);

        let target = goto_definition(&db, file_id, offset);
        assert!(target.is_some());

        let target = target.unwrap();
        assert_eq!(target.file_id, file_id);
        assert_eq!(target.name, "МояПроцедура");
        assert_eq!(target.kind, SymbolKind::Procedure);
        assert!(!target.range.is_empty());
    }

    #[test]
    fn test_goto_definition_function() {
        let source = r#"
Функция МояФункция()
    Возврат 1;
КонецФункции

Процедура Тест()
    Результат = МояФункция();
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        // Find position of function call
        let call_offset = source.rfind("МояФункция").unwrap();
        let offset = TextSize::from(call_offset as u32);

        let target = goto_definition(&db, file_id, offset);
        assert!(target.is_some());

        let target = target.unwrap();
        assert_eq!(target.name, "МояФункция");
        assert_eq!(target.kind, SymbolKind::Function);
    }

    #[test]
    fn test_goto_definition_variable() {
        let source = r#"
Перем МодульнаяПеременная;

Процедура Тест()
    МодульнаяПеременная = 1;
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        // Find position of variable usage
        let usage_offset = source.rfind("МодульнаяПеременная").unwrap();
        let offset = TextSize::from(usage_offset as u32);

        let target = goto_definition(&db, file_id, offset);
        assert!(target.is_some());

        let target = target.unwrap();
        assert_eq!(target.name, "МодульнаяПеременная");
        assert_eq!(target.kind, SymbolKind::Variable);
    }

    #[test]
    fn test_goto_definition_case_insensitive() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры

Функция Тест()
    мояпроцедура();
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);

        // Find lowercase call
        let call_offset = source.find("мояпроцедура").unwrap();
        let offset = TextSize::from(call_offset as u32);

        let target = goto_definition(&db, file_id, offset);
        assert!(target.is_some());

        // Should resolve to the definition with original case
        let target = target.unwrap();
        assert_eq!(target.name, "МояПроцедура");
    }

    #[test]
    fn test_goto_definition_not_found() {
        let source = r#"
Процедура Тест()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        // Position on keyword "Процедура"
        let offset = source.find("Процедура").unwrap();
        let offset = TextSize::from(offset as u32);

        let target = goto_definition(&db, file_id, offset);
        // Keywords are not navigable
        assert!(target.is_none());
    }

    #[test]
    fn test_goto_definition_on_declaration() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);

        // Position on the method name in declaration
        let decl_offset = source.find("МояПроцедура").unwrap();
        let offset = TextSize::from(decl_offset as u32);

        let target = goto_definition(&db, file_id, offset);
        assert!(target.is_some());

        // Should navigate to itself
        let target = target.unwrap();
        assert_eq!(target.name, "МояПроцедура");
    }

    /// Helper function to create a database with multiple files
    fn create_multi_file_db(files: &[(&str, &str)]) -> (RootDatabaseImpl, Vec<FileId>) {
        let mut db = RootDatabaseImpl::default();
        let mut file_ids = Vec::new();

        let mut file_set = FileSet::new();
        for (idx, (path, _)) in files.iter().enumerate() {
            let file_id = FileId(idx as u32);
            file_set.insert(file_id, VfsPath::new(path));
            file_ids.push(file_id);
        }

        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);

        for (file_id, (_, source)) in file_ids.iter().zip(files.iter()) {
            db.set_file_source_root(*file_id, SourceRootId(0));
            db.set_file_text(*file_id, source);
        }

        (db, file_ids)
    }

    #[test]
    fn test_goto_definition_cross_file() {
        // File 1: CommonModule with export method
        let common_module = r#"
Функция ЭкспортнаяФункция() Экспорт
    Возврат 42;
КонецФункции
        "#;

        // File 2: Form module calling the common module method
        let form_module = r#"
Процедура Тест()
    Результат = ОбщийМодуль.ЭкспортнаяФункция();
КонецПроцедуры
        "#;

        let files = &[
            ("CommonModules/ОбщийМодуль/Ext/Module.bsl", common_module),
            ("Forms/Form1/Ext/Form/Module.bsl", form_module),
        ];

        let (db, file_ids) = create_multi_file_db(files);

        // Find the position of "ЭкспортнаяФункция" in the call (file 2)
        let call_offset = form_module.rfind("ЭкспортнаяФункция").unwrap();
        let offset = TextSize::from(call_offset as u32);

        let target = goto_definition(&db, file_ids[1], offset);
        assert!(target.is_some(), "Should resolve cross-file method call");

        let target = target.unwrap();
        assert_eq!(target.file_id, file_ids[0], "Should navigate to CommonModule file");
        assert_eq!(target.name, "ЭкспортнаяФункция");
        assert_eq!(target.kind, SymbolKind::Function);
    }

    #[test]
    fn test_goto_definition_cross_file_case_insensitive() {
        let common_module = r#"
Функция МояФункция() Экспорт
    Возврат 1;
КонецФункции
        "#;

        let form_module = r#"
Процедура Тест()
    общиймодуль.мояфункция();
КонецПроцедуры
        "#;

        let files = &[
            ("CommonModules/ОбщийМодуль/Ext/Module.bsl", common_module),
            ("Forms/Form1/Ext/Form/Module.bsl", form_module),
        ];

        let (db, file_ids) = create_multi_file_db(files);

        // Find lowercase method name in call
        let call_offset = form_module.find("мояфункция").unwrap();
        let offset = TextSize::from(call_offset as u32);

        let target = goto_definition(&db, file_ids[1], offset);
        assert!(target.is_some(), "Should resolve case-insensitive");

        let target = target.unwrap();
        assert_eq!(target.file_id, file_ids[0]);
        assert_eq!(target.name, "МояФункция"); // Original case preserved
    }

    #[test]
    fn test_goto_definition_cross_file_not_found() {
        let common_module = r#"
Функция СуществуетМетод() Экспорт
    Возврат 1;
КонецФункции
        "#;

        let form_module = r#"
Процедура Тест()
    ОбщийМодуль.НеСуществуетМетод();
КонецПроцедуры
        "#;

        let files = &[
            ("CommonModules/ОбщийМодуль/Ext/Module.bsl", common_module),
            ("Forms/Form1/Ext/Form/Module.bsl", form_module),
        ];

        let (db, file_ids) = create_multi_file_db(files);

        let call_offset = form_module.find("НеСуществуетМетод").unwrap();
        let offset = TextSize::from(call_offset as u32);

        let target = goto_definition(&db, file_ids[1], offset);
        // Should return None for non-existent method
        assert!(target.is_none(), "Should not resolve non-existent method");
    }

    #[test]
    fn test_goto_definition_non_export_method() {
        let common_module = r#"
Функция ВнутренняяФункция()
    Возврат 1;
КонецФункции
        "#;

        let form_module = r#"
Процедура Тест()
    ОбщийМодуль.ВнутренняяФункция();
КонецПроцедуры
        "#;

        let files = &[
            ("CommonModules/ОбщийМодуль/Ext/Module.bsl", common_module),
            ("Forms/Form1/Ext/Form/Module.bsl", form_module),
        ];

        let (db, file_ids) = create_multi_file_db(files);

        let call_offset = form_module.find("ВнутренняяФункция").unwrap();
        let offset = TextSize::from(call_offset as u32);

        let target = goto_definition(&db, file_ids[1], offset);
        // Should return None for non-export method
        assert!(target.is_none(), "Should not navigate to non-export method");
    }

    #[test]
    fn test_goto_definition_same_file_still_works() {
        // Ensure backward compatibility - same-file navigation still works
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

        let target = goto_definition(&db, file_id, offset);
        assert!(target.is_some(), "Same-file navigation should still work");

        let target = target.unwrap();
        assert_eq!(target.file_id, file_id);
        assert_eq!(target.name, "МояПроцедура");
    }
}
