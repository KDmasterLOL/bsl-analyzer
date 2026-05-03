//! Go to Definition implementation.
//!
//! This module implements "Go to Definition" functionality, which allows
//! navigating from a symbol usage to its definition.
//!
//! Uses the unified Definition enum for resolution.

use hir::{classify_token, Definition, NameClass, Semantics};
use ide_db::RootDatabase;
use syntax::TextSize;
use vfs::FileId;

use crate::{NavigationTarget, SymbolKind};

/// Go to the definition of the symbol at the given position.
///
/// Returns a navigation target pointing to the symbol's definition,
/// or None if no symbol is found at the position.
///
/// Dispatch is driven by [`hir::classify_token`] — every name-token
/// resolves through the same classifier the rest of the IDE layer
/// uses, so keyword-shaped names sitting in field-tail slots
/// (`Запрос.Выполнить`, where `Выполнить` is `KW_EXECUTE`) reach the
/// resolver instead of being rejected by an `IDENT`-only gate.
///
/// Supports:
/// - Same-file navigation (methods, variables, parameters)
/// - Cross-file navigation (qualified names like `Module.Method`)
/// - Builtin functions / platform methods / MDO types (no source
///   target — returns `None`)
pub fn goto_definition<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
) -> Option<NavigationTarget> {
    let _span =
        tracing::info_span!("goto_definition", ?file_id, offset = u32::from(offset)).entered();

    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let token = root.token_at_offset(offset).right_biased()?;

    let sema = Semantics::new(db);

    let definition = match classify_token(&token) {
        NameClass::FieldName { token, .. } => {
            // Platform-method dispatch is authoritative when it
            // matches: a hit means we resolved the field tail through
            // the type-aware platform index, and that target is final
            // — even if it has no source to navigate to (`BuiltinMethod`
            // for `Запрос.Выполнить`). Falling back to qualified-name
            // resolution in that case would let the workspace path
            // resolver match `[Запрос, Выполнить]` against an unrelated
            // CommonModule that happens to share the receiver name and
            // export a same-name method, jumping the user to an
            // unrelated definition.
            if let Some(d) = sema.resolve_method_call_to_definition(file_id, &token) {
                return definition_to_navigation_target(db, &d);
            }
            // Platform-method dispatch missed entirely. Try the
            // qualified-name path for cross-module
            // (`ОбщегоНазначения.МойМетод`), MDO objects
            // (`Документы.ПКО`), and manager-module methods.
            sema.resolve_name_to_definition(file_id, &token)?
        }
        NameClass::FreeName { token } => sema.resolve_name_to_definition(file_id, &token)?,
        NameClass::TypeRef { .. }
        | NameClass::Literal { .. }
        | NameClass::Keyword { .. }
        | NameClass::Other => return None,
    };

    definition_to_navigation_target(db, &definition)
}

/// Convert a Definition to a NavigationTarget.
///
/// This is the unified conversion function for all definition types.
/// Returns None for builtins, MDO types, and unresolved symbols.
fn definition_to_navigation_target<DB: RootDatabase>(
    db: &DB,
    definition: &Definition,
) -> Option<NavigationTarget> {
    match definition {
        Definition::Method(method_id) => {
            let file_id = method_id.module.file_id;
            let tree = db.item_tree(file_id);

            for (idx, item) in tree.top_level_items().iter().enumerate() {
                if idx == method_id.local_id as usize {
                    match item {
                        hir::ModItem::Procedure(proc_idx) => {
                            let proc = tree.procedure(*proc_idx);
                            let range = proc.source_range;
                            let name = proc.name.as_str().to_string();
                            return Some(NavigationTarget {
                                file_id,
                                range,
                                name,
                                kind: SymbolKind::Procedure,
                            });
                        }
                        hir::ModItem::Function(func_idx) => {
                            let func = tree.function(*func_idx);
                            let range = func.source_range;
                            let name = func.name.as_str().to_string();
                            return Some(NavigationTarget {
                                file_id,
                                range,
                                name,
                                kind: SymbolKind::Function,
                            });
                        }
                        _ => {}
                    }
                }
            }
            None
        }
        Definition::Variable(var_id) => {
            let file_id = var_id.module.file_id;
            let tree = db.item_tree(file_id);

            for (idx, item) in tree.top_level_items().iter().enumerate() {
                if idx == var_id.local_id as usize {
                    if let hir::ModItem::Variable(var_idx) = item {
                        let var = tree.variable(*var_idx);
                        let range = var.source_range;
                        let name = var.name.as_str().to_string();
                        return Some(NavigationTarget {
                            file_id,
                            range,
                            name,
                            kind: SymbolKind::Variable,
                        });
                    }
                }
            }
            None
        }
        Definition::Parameter { method_id, param_name, .. } => {
            // Navigate to the parameter in the method signature
            let file_id = method_id.module.file_id;
            let tree = db.item_tree(file_id);

            for (idx, item) in tree.top_level_items().iter().enumerate() {
                if idx == method_id.local_id as usize {
                    let range = match item {
                        hir::ModItem::Procedure(proc_idx) => tree.procedure(*proc_idx).source_range,
                        hir::ModItem::Function(func_idx) => tree.function(*func_idx).source_range,
                        _ => continue,
                    };

                    return Some(NavigationTarget {
                        file_id,
                        range, // For now, navigate to the whole method (TODO: narrow to param)
                        name: param_name.as_str().to_string(),
                        kind: SymbolKind::Variable, // Parameters are variables in BSL
                    });
                }
            }
            None
        }
        Definition::Local { method_id, var_name } => {
            // Navigate to the local variable declaration
            // For now, navigate to the containing method (TODO: find exact declaration)
            let file_id = method_id.module.file_id;
            let tree = db.item_tree(file_id);

            for (idx, item) in tree.top_level_items().iter().enumerate() {
                if idx == method_id.local_id as usize {
                    let range = match item {
                        hir::ModItem::Procedure(proc_idx) => tree.procedure(*proc_idx).source_range,
                        hir::ModItem::Function(func_idx) => tree.function(*func_idx).source_range,
                        _ => continue,
                    };

                    return Some(NavigationTarget {
                        file_id,
                        range,
                        name: var_name.as_str().to_string(),
                        kind: SymbolKind::Variable,
                    });
                }
            }
            None
        }
        // Builtins, MDO types, modules, etc. - no navigation target
        Definition::BuiltinFunction(_)
        | Definition::BuiltinMethod { .. }
        | Definition::BuiltinMethodHandle { .. }
        | Definition::MdoCollectionType(_)
        | Definition::MdoObject { .. }
        | Definition::MdoManagerModule { .. }
        | Definition::Module(_)
        | Definition::VirtualTableField { .. }
        | Definition::Unresolved => None,
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

    // Helper for multi-file scenarios
    fn create_db_with_two_files(
        caller_source: &str,
        caller_path: &str,
        manager_source: &str,
        manager_path: &str,
    ) -> (RootDatabaseImpl, FileId, FileId) {
        let mut db = RootDatabaseImpl::default();
        let caller_file = FileId(0);
        let manager_file = FileId(1);

        // Set up source root with both files
        let mut file_set = FileSet::new();
        file_set.insert(caller_file, VfsPath::new(caller_path));
        file_set.insert(manager_file, VfsPath::new(manager_path));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(caller_file, SourceRootId(0));
        db.set_file_source_root(manager_file, SourceRootId(0));

        // Set file text
        db.set_file_text(caller_file, caller_source);
        db.set_file_text(manager_file, manager_source);

        (db, caller_file, manager_file)
    }

    #[test]
    fn test_goto_definition_manager_module_method() {
        // Setup: two-file scenario
        let caller_source = r#"
Процедура Тест()
    РегистрыСведений.ТестовыйРегистр.МетодМенеджера();
КонецПроцедуры
        "#;

        let manager_source = r#"
Процедура МетодМенеджера() Экспорт
    // Implementation
КонецПроцедуры
        "#;

        let (db, caller_file, manager_file) = create_db_with_two_files(
            caller_source,
            "/test/Catalogs/Test/Ext/ObjectModule.bsl",
            manager_source,
            "/test/InformationRegisters/ТестовыйРегистр/Ext/ManagerModule.bsl",
        );

        // Position on "МетодМенеджера" in call
        let offset = TextSize::from(caller_source.rfind("МетодМенеджера").unwrap() as u32);

        let result = goto_definition(&db, caller_file, offset);

        assert!(result.is_some(), "Should resolve manager module method");
        let nav = result.unwrap();
        assert_eq!(nav.file_id, manager_file);
        assert_eq!(nav.name, "МетодМенеджера");
        assert_eq!(nav.kind, SymbolKind::Procedure);
    }

    #[test]
    fn test_goto_definition_manager_module_method_not_exported() {
        // Manager method WITHOUT "Экспорт"
        let caller_source = r#"
Процедура Тест()
    Документы.ТестовыйДокумент.ВнутреннийМетод();
КонецПроцедуры
        "#;

        let manager_source = r#"
Процедура ВнутреннийМетод()
    // NOT exported - should not be resolvable
КонецПроцедуры
        "#;

        let (db, caller_file, _manager_file) = create_db_with_two_files(
            caller_source,
            "/test/Catalogs/Test/Ext/ObjectModule.bsl",
            manager_source,
            "/test/Documents/ТестовыйДокумент/Ext/ManagerModule.bsl",
        );

        let offset = TextSize::from(caller_source.rfind("ВнутреннийМетод").unwrap() as u32);
        let result = goto_definition(&db, caller_file, offset);

        // Should NOT resolve to non-exported method
        assert!(result.is_none(), "Non-exported methods should not resolve");
    }

    #[test]
    fn test_goto_definition_manager_module_metadata_not_found() {
        let caller_source = r#"
Процедура Тест()
    Справочники.НесуществующийОбъект.Метод();
КонецПроцедуры
        "#;

        let (db, caller_file) = create_db_with_file(caller_source);

        let offset = TextSize::from(caller_source.rfind("Метод").unwrap() as u32);
        let result = goto_definition(&db, caller_file, offset);

        // Should return None when metadata object not found
        assert!(result.is_none(), "Should not resolve non-existent metadata object");
    }

    /// Regression guard for Layer B: goto on `Запрос.Выполнить()`
    /// must NOT panic and must return `None` (the platform method has
    /// no source to navigate to). Pre-migration the IDENT-only entry
    /// gate dropped `KW_EXECUTE` before classification — but the
    /// codepath through `resolve_method_call_to_definition` is the
    /// same one hover uses, so a goto request still has to resolve
    /// the field-tail, just to a `BuiltinMethod` definition that
    /// produces no nav target. This test pins both halves: the
    /// resolution succeeds, the navigation returns `None`.
    #[test]
    fn test_goto_definition_keyword_method_after_dot_returns_none() {
        let source = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ 1";
    Результат = Запрос.Выполнить();
КонецПроцедуры
        "#;
        let (db, file_id) = create_db_with_file(source);

        let offset = TextSize::from(source.rfind("Выполнить").unwrap() as u32);
        let target = goto_definition(&db, file_id, offset);
        // Platform methods have no navigable source — None is correct.
        assert!(target.is_none(), "platform method navigation must be None, got: {target:?}");
    }

    /// Hard regression for the codex stop-time review: goto on
    /// `Запрос.Выполнить()` must NOT jump to an unrelated CommonModule
    /// that happens to share the receiver's name and exports a method
    /// called `Выполнить`. The receiver is a typed local variable
    /// (`Новый Запрос` — platform Query type), and the field-tail
    /// must resolve through the platform-method dispatch, which
    /// claims the slot exclusively. Pre-fix the goto fell back to
    /// qualified-name resolution after the platform dispatch
    /// returned a `BuiltinMethod` (no source), and the workspace
    /// resolver matched `[Запрос, Выполнить]` against the bogus
    /// CommonModule.
    #[test]
    fn test_goto_definition_platform_method_does_not_leak_to_workspace_module() {
        let bogus = r#"
Функция Выполнить() Экспорт
    Возврат "не должно резолвиться";
КонецФункции
        "#;
        let caller = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ 1";
    Результат = Запрос.Выполнить();
КонецПроцедуры
        "#;

        let files = &[
            ("CommonModules/Запрос/Ext/Module.bsl", bogus),
            ("Forms/Form1/Ext/Form/Module.bsl", caller),
        ];
        let (db, file_ids) = create_multi_file_db(files);

        let offset = TextSize::from(caller.rfind("Выполнить").unwrap() as u32);
        let target = goto_definition(&db, file_ids[1], offset);

        // The bug we're guarding against is `Some(file_ids[0])` —
        // jumping to the unrelated CommonModule. `None` (platform
        // method, no source) or any non-bogus target is acceptable.
        if let Some(t) = target {
            assert_ne!(
                t.file_id, file_ids[0],
                "goto must not leak from platform method to a workspace module that happens to share the receiver's name"
            );
        }
    }

    #[test]
    fn test_goto_definition_manager_module_case_insensitive() {
        let caller_source = r#"
Процедура Тест()
    // Lowercase call
    регистрысведений.тестовыйрегистр.методменеджера();
КонецПроцедуры
        "#;

        let manager_source = r#"
Процедура МетодМенеджера() Экспорт
КонецПроцедуры
        "#;

        let (db, caller_file, manager_file) = create_db_with_two_files(
            caller_source,
            "/test/Catalogs/Test/Ext/ObjectModule.bsl",
            manager_source,
            "/test/InformationRegisters/тестовыйрегистр/Ext/ManagerModule.bsl",
        );

        let offset = TextSize::from(caller_source.rfind("методменеджера").unwrap() as u32);
        let result = goto_definition(&db, caller_file, offset);

        assert!(result.is_some());
        let nav = result.unwrap();
        assert_eq!(nav.file_id, manager_file);
        // Should preserve original case from definition
        assert_eq!(nav.name, "МетодМенеджера");
    }
}
