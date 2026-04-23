use std::sync::Arc;

use base_db::{RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use hir::{DefDatabase, ModuleId};
use vfs::FileId;
use vfs::{file_set::FileSet, VfsPath};

use super::RootDatabaseImpl;
use crate::RootDatabase;

#[test]
fn test_root_database_basic() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set file text
    db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

    // Test parse query
    let parse = db.parse(file_id);
    assert!(!parse.has_errors());

    // Test item_tree query
    let tree = db.item_tree(file_id);
    assert_eq!(tree.top_level_items().len(), 1);

    // Test module_data query
    let module_id = ModuleId::new(file_id);
    let module_data = db.module_data(module_id);
    assert_eq!(module_data.procedures.len(), 1);
    assert_eq!(module_data.functions.len(), 0);
    assert_eq!(module_data.variables.len(), 0);
}

#[test]
fn test_incremental_item_tree() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Initial content
    db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");
    let tree1 = db.item_tree(file_id);
    assert_eq!(tree1.top_level_items().len(), 1);

    // Change content - should invalidate cache
    db.set_file_text(
        file_id,
        r#"
Процедура Тест1() КонецПроцедуры
Функция Тест2() КонецФункции
        "#,
    );
    let tree2 = db.item_tree(file_id);
    assert_eq!(tree2.top_level_items().len(), 2);
}

#[test]
fn test_symbol_tree_query() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set file text
    db.set_file_text(
        file_id,
        r#"
Процедура ПерваяПроцедура()
КонецПроцедуры

Функция ВтораяФункция() Экспорт
КонецФункции

Перем МодульнаяПеременная;
        "#,
    );

    // Test symbol_tree query
    let module_id = ModuleId::new(file_id);
    let symbol_tree = db.symbol_tree(module_id);

    assert_eq!(symbol_tree.methods().count(), 2);
    assert_eq!(symbol_tree.variables().count(), 1);
    assert_eq!(symbol_tree.exported_methods().count(), 1);
}

#[test]
fn test_symbol_tree_caching() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set initial content
    db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

    let module_id = ModuleId::new(file_id);
    let tree1 = db.symbol_tree(module_id);
    assert_eq!(tree1.methods().count(), 1);

    // Second call should return cached result
    let tree2 = db.symbol_tree(module_id);
    assert_eq!(tree2.methods().count(), 1);

    // Verify it's the same Arc (cached)
    assert!(Arc::ptr_eq(&tree1, &tree2));
}

#[test]
fn test_symbol_tree_invalidation() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Initial content
    db.set_file_text(file_id, "Процедура Тест1() КонецПроцедуры");

    let module_id = ModuleId::new(file_id);
    let tree1 = db.symbol_tree(module_id);
    assert_eq!(tree1.methods().count(), 1);

    // Change content - should invalidate cache
    db.set_file_text(
        file_id,
        r#"
Процедура Тест1() КонецПроцедуры
Функция Тест2() КонецФункции
        "#,
    );

    let tree2 = db.symbol_tree(module_id);
    assert_eq!(tree2.methods().count(), 2);

    // Should NOT be the same Arc (invalidated)
    assert!(!Arc::ptr_eq(&tree1, &tree2));
}

#[test]
fn test_symbol_tree_case_insensitive() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Процедура МояПроцедура() КонецПроцедуры");

    let module_id = ModuleId::new(file_id);
    let symbol_tree = db.symbol_tree(module_id);

    // Case-insensitive lookup
    use hir::Name;
    assert!(symbol_tree.find_method(&Name::new("МояПроцедура")).is_some());
    assert!(symbol_tree.find_method(&Name::new("мояпроцедура")).is_some());
    assert!(symbol_tree.find_method(&Name::new("МОЯПРОЦЕДУРА")).is_some());
}

#[test]
fn test_symbol_tree_multi_file() {
    let mut db = RootDatabaseImpl::new();

    // Set up source root
    let mut file_set = FileSet::new();
    let file1 = FileId(0);
    let file2 = FileId(1);
    file_set.insert(file1, VfsPath::new("/module1.bsl"));
    file_set.insert(file2, VfsPath::new("/module2.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file1, SourceRootId(0));
    db.set_file_source_root(file2, SourceRootId(0));

    // File 1
    db.set_file_text(file1, "Процедура Метод1() КонецПроцедуры");

    // File 2
    db.set_file_text(file2, "Функция Метод2() Экспорт КонецФункции");

    // Check file 1
    let module1 = ModuleId::new(file1);
    let tree1 = db.symbol_tree(module1);
    assert_eq!(tree1.methods().count(), 1);
    assert_eq!(tree1.exported_methods().count(), 0);

    // Check file 2
    let module2 = ModuleId::new(file2);
    let tree2 = db.symbol_tree(module2);
    assert_eq!(tree2.methods().count(), 1);
    assert_eq!(tree2.exported_methods().count(), 1);
}

#[test]
fn test_resolver_resolve_module_method() {
    use hir::Resolver;
    use hir::{ModuleId, Name};

    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);
    let module_id = ModuleId::new(file_id);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Use actual BSL code instead of manually constructing ItemTree
    db.set_file_text(
        file_id,
        r#"
Процедура МояПроцедура()
КонецПроцедуры

Функция МояФункция() Экспорт
КонецФункции
        "#,
    );

    // Create resolver
    let resolver = Resolver::for_module(module_id);

    // Resolve procedure
    let method_id = resolver.resolve_module_method(&db, &Name::new("МояПроцедура"));
    assert!(method_id.is_some());
    assert_eq!(method_id.unwrap().module, module_id);

    // Resolve function
    let method_id = resolver.resolve_module_method(&db, &Name::new("МояФункция"));
    assert!(method_id.is_some());
    assert_eq!(method_id.unwrap().module, module_id);

    // Not found
    let method_id = resolver.resolve_module_method(&db, &Name::new("НеСуществует"));
    assert!(method_id.is_none());
}

#[test]
fn test_resolver_resolve_module_method_case_insensitive() {
    use hir::Resolver;
    use hir::{ModuleId, Name};

    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);
    let module_id = ModuleId::new(file_id);

    // Set up
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Процедура МояПроцедура() КонецПроцедуры");

    let resolver = Resolver::for_module(module_id);

    // Different cases should all resolve
    assert!(resolver.resolve_module_method(&db, &Name::new("МояПроцедура")).is_some());
    assert!(resolver.resolve_module_method(&db, &Name::new("мояпроцедура")).is_some());
    assert!(resolver.resolve_module_method(&db, &Name::new("МОЯПРОЦЕДУРА")).is_some());
}

#[test]
fn test_resolver_resolve_module_variable() {
    use hir::Resolver;
    use hir::{ModuleId, Name};

    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);
    let module_id = ModuleId::new(file_id);

    // Set up
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Перем МодульнаяПеременная Экспорт;");

    let resolver = Resolver::for_module(module_id);

    // Resolve variable
    let var_id = resolver.resolve_module_variable(&db, &Name::new("МодульнаяПеременная"));
    assert!(var_id.is_some());
    assert_eq!(var_id.unwrap().module, module_id);

    // Not found
    let var_id = resolver.resolve_module_variable(&db, &Name::new("НеСуществует"));
    assert!(var_id.is_none());
}

#[test]
fn test_resolver_resolve_name_hierarchy() {
    use hir::ExprScopes;
    use hir::{ModuleId, Name};
    use hir::{Resolution, Resolver};

    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);
    let module_id = ModuleId::new(file_id);

    // Set up
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Create module with method and variable
    db.set_file_text(
        file_id,
        r#"
Процедура Метод()
КонецПроцедуры

Перем Переменная;
        "#,
    );

    // Create resolver with expression scope
    let mut expr_scopes = ExprScopes::new();
    expr_scopes.add_parameter(Name::new("Параметр"));

    let root_scope = expr_scopes.root_scope();
    let resolver =
        Resolver::for_module(module_id).push_expr_scope(Arc::new(expr_scopes), root_scope);

    // Resolve parameter (local scope)
    let resolved = resolver.resolve_name(&db, &Name::new("Параметр"));
    assert!(matches!(resolved, Some(Resolution::Local(_))));

    // Resolve method (module scope)
    let resolved = resolver.resolve_name(&db, &Name::new("Метод"));
    assert!(matches!(resolved, Some(Resolution::Method(_))));

    // Resolve variable (module scope)
    let resolved = resolver.resolve_name(&db, &Name::new("Переменная"));
    assert!(matches!(resolved, Some(Resolution::Variable(_))));

    // Not found
    let resolved = resolver.resolve_name(&db, &Name::new("НеСуществует"));
    assert!(resolved.is_none());
}

#[test]
fn test_resolver_shadowing_local_over_module() {
    use hir::ExprScopes;
    use hir::{ModuleId, Name};
    use hir::{Resolution, Resolver};

    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);
    let module_id = ModuleId::new(file_id);

    // Set up
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Create module variable with name "Значение"
    db.set_file_text(file_id, "Перем Значение;");

    // Create local variable with the same name
    let mut expr_scopes = ExprScopes::new();
    expr_scopes.add_local_variable(expr_scopes.root_scope(), Name::new("Значение"));

    let root_scope = expr_scopes.root_scope();
    let resolver =
        Resolver::for_module(module_id).push_expr_scope(Arc::new(expr_scopes), root_scope);

    // Should resolve to local variable (shadows module variable)
    let resolved = resolver.resolve_name(&db, &Name::new("Значение"));
    assert!(matches!(resolved, Some(Resolution::Local(_))));
}

#[test]
fn test_resolver_with_workspace_scope() {
    use hir::ModuleId;
    use hir::Resolver;

    let file_id = FileId(0);
    let module_id = ModuleId::new(file_id);

    let resolver = Resolver::with_workspace_scope(module_id);

    // Should have WorkspaceScope and ModuleScope
    assert_eq!(resolver.scopes.len(), 2);
}

#[test]
fn test_resolver_cross_module_gated_by_configurations() {
    // When a configuration is registered but the BSL file for a CommonModule
    // is NOT declared in that configuration, `resolve_cross_module` must
    // return `Unresolved` without falling back to path-based lookup.
    //
    // We simulate this by registering a non-existent configuration path,
    // which forces `load_configuration` onto its empty-fallback branch. That
    // leaves the registered config with zero common_modules — so any module
    // call the fixture otherwise "sees" via `module_index` must be rejected
    // by the new metadata visibility gate.
    use hir::{ModuleId, Name, PathResolution, QualifiedName, Resolver};

    let mut db = RootDatabaseImpl::new();
    let test_file = FileId(0);
    let om_file = FileId(1);

    let mut file_set = FileSet::new();
    file_set.insert(test_file, VfsPath::new("/test.bsl"));
    file_set.insert(om_file, VfsPath::new("/CommonModules/ОбщегоНазначения/Ext/Module.bsl"));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(test_file, SourceRootId(0));
    db.set_file_source_root(om_file, SourceRootId(0));

    db.set_file_text(test_file, "Процедура Тест() КонецПроцедуры");
    db.set_file_text(om_file, "Функция ПолучитьЗначение() Экспорт\n    Возврат 1;\nКонецФункции");

    // Sanity: with no config registered, path-based lookup currently finds
    // ОбщегоНазначения (baseline before the gate kicks in).
    let resolver = Resolver::with_workspace_scope(ModuleId::new(test_file));
    let path = QualifiedName::from_segments([
        Name::new("ОбщегоНазначения"),
        Name::new("ПолучитьЗначение"),
    ]);
    let before = resolver.resolve_path(&db, &path);
    assert!(
        matches!(before, PathResolution::Method(_)),
        "baseline: empty-config fallback must still resolve path-based lookup, got {:?}",
        before
    );

    // Register a non-existent config path — `load_configuration` will
    // silently produce an empty `Configuration`, so the visibility gate
    // sees one config with zero common_modules declared.
    db.set_all_config_paths(vec![(None, std::path::PathBuf::from("/does-not-exist"))]);

    let after = resolver.resolve_path(&db, &path);
    assert!(
        matches!(after, PathResolution::Unresolved(_)),
        "with a config registered but no matching declaration, resolution must fail, got {:?}",
        after
    );
}

// ========== SDBL Integration Tests (migrated from base-db) ==========

#[test]
fn test_all_sdbl_in_file_basic() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set file with SDBL query
    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
КонецПроцедуры"#,
    );

    // Should extract query
    let queries = db.all_sdbl_in_file(file_id);
    assert_eq!(queries.len(), 1, "Should extract 1 SDBL query");
    assert!(queries[0].1.is_valid(), "SDBL should parse successfully");

    // Change file to have multiple queries
    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос1 = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
    Запрос2 = "ВЫБРАТЬ Наименование ИЗ Справочник.Категории";
КонецПроцедуры"#,
    );

    // Should extract both queries
    let queries = db.all_sdbl_in_file(file_id);
    assert_eq!(queries.len(), 2, "Should extract 2 SDBL queries");
    assert!(queries.iter().all(|(_, q)| q.is_valid()));
}

#[test]
fn test_all_sdbl_in_file_keyword_filter() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Strings without SELECT/ВЫБРАТЬ keywords should be skipped
    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Строка = "Это просто строка без ключевых слов";
    Запрос = "ВЫБРАТЬ * ИЗ Справочник.Товары";
КонецПроцедуры"#,
    );

    let queries = db.all_sdbl_in_file(file_id);
    // Should only extract strings with SELECT/ВЫБРАТЬ
    assert_eq!(queries.len(), 1, "Should filter by SELECT/ВЫБРАТЬ keyword");
    assert!(queries[0].1.query_text.contains("ВЫБРАТЬ"));
}

#[test]
fn test_all_sdbl_in_file_multiline() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Test multiline SDBL query with | prefix
    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |    Ссылка,
             |    Наименование
             |ИЗ Справочник.Товары";
КонецПроцедуры"#,
    );

    let queries = db.all_sdbl_in_file(file_id);
    assert_eq!(queries.len(), 1, "Should extract multiline SDBL query");
    assert!(queries[0].1.is_valid(), "Multiline query should parse successfully");

    // Verify content contains all parts
    let query_text = &queries[0].1.query_text;
    assert!(query_text.contains("Ссылка"));
    assert!(query_text.contains("Наименование"));
    assert!(query_text.contains("Справочник.Товары"));
}

#[test]
fn test_all_sdbl_in_file_assignment_patterns() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Test various assignment patterns
    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    // Direct assignment
    Запрос1 = "ВЫБРАТЬ * ИЗ Справочник.Товары";

    // Assignment in method call
    Результат = ВыполнитьЗапрос("ВЫБРАТЬ * ИЗ Документ.Продажа");

    // Assignment in array
    Массив = Новый Массив();
    Массив.Добавить("ВЫБРАТЬ * ИЗ Регистр.Остатки");
КонецПроцедуры"#,
    );

    let queries = db.all_sdbl_in_file(file_id);
    // Should extract all SDBL strings regardless of assignment pattern
    assert_eq!(queries.len(), 3, "Should extract queries from various contexts");

    // Verify all queries are valid
    for (_, query_info) in queries.iter() {
        assert!(query_info.is_valid(), "All queries should parse successfully");
    }
}

#[test]
fn test_all_sdbl_in_file_with_parameters() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Test SDBL query with parameters (&Parameter syntax)
    db.set_file_text(
        file_id,
        r#"Процедура ПолучитьДанные()
    Запрос = "ВЫБРАТЬ
             |    Ссылка,
             |    Наименование
             |ИЗ Справочник.Товары
             |ГДЕ
             |    Код = &Значение1
             |    И Наименование ПОДОБНО &Значение2
             |    И Родитель = &Значение3";
КонецПроцедуры"#,
    );

    let queries = db.all_sdbl_in_file(file_id);

    // Should extract query with parameters
    assert_eq!(queries.len(), 1, "Should extract query with parameters");

    // Verify query is valid (parses successfully)
    assert!(queries[0].1.is_valid(), "Query with parameters should parse successfully");

    // Verify query text contains parameters
    assert!(queries[0].1.query_text.contains("&Значение1"));
    assert!(queries[0].1.query_text.contains("&Значение2"));
    assert!(queries[0].1.query_text.contains("&Значение3"));
}

#[test]
fn test_module_metadata_creation() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/CommonModules/ОбщегоНазначения/Ext/Module.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set file text
    db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

    // Test module_metadata query
    let module_id = ModuleId::new(file_id);
    let metadata = db.module_metadata(module_id);

    // Should create metadata successfully
    // We don't have configuration loaded yet (Phase 2), so metadata will be minimal
    // But the Arc<ModuleMetadata> structure should be created
    assert_eq!(
        metadata.module_type,
        bsl_metadata::ModuleType::CommonModule,
        "Should detect CommonModule type from path"
    );
}

#[test]
fn test_module_bodies_and_metadata_separate() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set file text
    db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

    // Test module_bodies and module_metadata are separate queries
    let module_id = ModuleId::new(file_id);
    let _module_bodies = db.module_bodies(module_id);
    let _module_metadata = db.module_metadata(module_id);

    // Both should work independently (metadata is now accessed separately)
    // This is the correct pattern for performance - no cloning of ModuleBodies
}

#[test]
fn test_module_metadata_cache_invalidation() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set initial file text and get metadata
    db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");
    let module_id = ModuleId::new(file_id);
    let _metadata1 = db.module_metadata(module_id);

    // Change file text (should invalidate cache)
    db.set_file_text(file_id, "Процедура Тест2() КонецПроцедуры");
    let _metadata2 = db.module_metadata(module_id);

    // Test passes if we can call metadata again after invalidation
}

// ========== SDBL HIR Tests ==========

#[test]
fn test_sdbl_hir_in_file_basic() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set file with SDBL query
    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
КонецПроцедуры"#,
    );

    // Should extract and lower query to HIR
    let hirs = db.sdbl_hir_in_file(file_id);
    assert_eq!(hirs.len(), 1, "Should have 1 SDBL HIR");

    // Verify HIR structure
    let (_, sdbl_hir) = &hirs[0];
    assert!(!sdbl_hir.queries()[0].hir.from.is_empty(), "Should have FROM clause");
    assert_eq!(sdbl_hir.queries()[0].hir.from[0].full_name, "Справочник.Товары");
}

#[test]
fn test_sdbl_hir_in_file_multiple_queries() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set file with multiple SDBL queries
    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос1 = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
    Запрос2 = "ВЫБРАТЬ Номер ИЗ Документ.РасходнаяНакладная";
КонецПроцедуры"#,
    );

    // Should extract and lower both queries
    let hirs = db.sdbl_hir_in_file(file_id);
    assert_eq!(hirs.len(), 2, "Should have 2 SDBL HIRs");

    // Verify first query
    assert_eq!(hirs[0].1.queries()[0].hir.from[0].full_name, "Справочник.Товары");

    // Verify second query
    assert_eq!(hirs[1].1.queries()[0].hir.from[0].full_name, "Документ.РасходнаяНакладная");
}

#[test]
fn test_sdbl_hir_in_file_caching() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Set file with SDBL query
    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
КонецПроцедуры"#,
    );

    // First call
    let hirs1 = db.sdbl_hir_in_file(file_id);

    // Second call should return cached result
    let hirs2 = db.sdbl_hir_in_file(file_id);

    // Verify same Arc (cached)
    assert!(Arc::ptr_eq(&hirs1, &hirs2), "Should return cached result");
}

#[test]
fn test_sdbl_hir_in_file_invalidation() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    // Set up source root
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    // Initial query
    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
КонецПроцедуры"#,
    );
    let hirs1 = db.sdbl_hir_in_file(file_id);
    assert_eq!(hirs1[0].1.queries()[0].hir.from[0].full_name, "Справочник.Товары");

    // Change query
    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Номер ИЗ Документ.Продажа";
КонецПроцедуры"#,
    );
    let hirs2 = db.sdbl_hir_in_file(file_id);

    // Should NOT be same Arc (invalidated)
    assert!(!Arc::ptr_eq(&hirs1, &hirs2), "Should invalidate cache on file change");

    // Should have new content
    assert_eq!(hirs2[0].1.queries()[0].hir.from[0].full_name, "Документ.Продажа");
}
