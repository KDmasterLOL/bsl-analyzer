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

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

    let parse = db.parse(file_id);
    assert!(!parse.has_errors());

    let tree = db.item_tree(file_id);
    assert_eq!(tree.top_level_items().len(), 1);

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

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");
    let tree1 = db.item_tree(file_id);
    assert_eq!(tree1.top_level_items().len(), 1);

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

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

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

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

    let module_id = ModuleId::new(file_id);
    let tree1 = db.symbol_tree(module_id);
    assert_eq!(tree1.methods().count(), 1);

    let tree2 = db.symbol_tree(module_id);
    assert_eq!(tree2.methods().count(), 1);

    assert!(Arc::ptr_eq(&tree1, &tree2));
}

#[test]
fn test_symbol_tree_invalidation() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Процедура Тест1() КонецПроцедуры");

    let module_id = ModuleId::new(file_id);
    let tree1 = db.symbol_tree(module_id);
    assert_eq!(tree1.methods().count(), 1);

    db.set_file_text(
        file_id,
        r#"
Процедура Тест1() КонецПроцедуры
Функция Тест2() КонецФункции
        "#,
    );

    let tree2 = db.symbol_tree(module_id);
    assert_eq!(tree2.methods().count(), 2);

    assert!(!Arc::ptr_eq(&tree1, &tree2));
}

#[test]
fn test_symbol_tree_case_insensitive() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Процедура МояПроцедура() КонецПроцедуры");

    let module_id = ModuleId::new(file_id);
    let symbol_tree = db.symbol_tree(module_id);

    use hir::Name;
    assert!(symbol_tree.find_method(&Name::new("МояПроцедура")).is_some());
    assert!(symbol_tree.find_method(&Name::new("мояпроцедура")).is_some());
    assert!(symbol_tree.find_method(&Name::new("МОЯПРОЦЕДУРА")).is_some());
}

#[test]
fn test_symbol_tree_multi_file() {
    let mut db = RootDatabaseImpl::new();

    let mut file_set = FileSet::new();
    let file1 = FileId(0);
    let file2 = FileId(1);
    file_set.insert(file1, VfsPath::new("/module1.bsl"));
    file_set.insert(file2, VfsPath::new("/module2.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file1, SourceRootId(0));
    db.set_file_source_root(file2, SourceRootId(0));

    db.set_file_text(file1, "Процедура Метод1() КонецПроцедуры");

    db.set_file_text(file2, "Функция Метод2() Экспорт КонецФункции");

    let module1 = ModuleId::new(file1);
    let tree1 = db.symbol_tree(module1);
    assert_eq!(tree1.methods().count(), 1);
    assert_eq!(tree1.exported_methods().count(), 0);

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

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(
        file_id,
        r#"
Процедура МояПроцедура()
КонецПроцедуры

Функция МояФункция() Экспорт
КонецФункции
        "#,
    );

    let resolver = Resolver::for_module(module_id);

    let method_id = resolver.resolve_module_method(&db, &Name::new("МояПроцедура"));
    assert!(method_id.is_some());
    assert_eq!(method_id.unwrap().module, module_id);

    let method_id = resolver.resolve_module_method(&db, &Name::new("МояФункция"));
    assert!(method_id.is_some());
    assert_eq!(method_id.unwrap().module, module_id);

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

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Процедура МояПроцедура() КонецПроцедуры");

    let resolver = Resolver::for_module(module_id);

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

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Перем МодульнаяПеременная Экспорт;");

    let resolver = Resolver::for_module(module_id);

    let var_id = resolver.resolve_module_variable(&db, &Name::new("МодульнаяПеременная"));
    assert!(var_id.is_some());
    assert_eq!(var_id.unwrap().module, module_id);

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

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(
        file_id,
        r#"
Процедура Метод()
КонецПроцедуры

Перем Переменная;
        "#,
    );

    let mut expr_scopes = ExprScopes::new();
    expr_scopes.add_parameter(Name::new("Параметр"));

    let root_scope = expr_scopes.root_scope();
    let resolver =
        Resolver::for_module(module_id).push_expr_scope(Arc::new(expr_scopes), root_scope);

    let resolved = resolver.resolve_name(&db, &Name::new("Параметр"));
    assert!(matches!(resolved, Some(Resolution::Local(_))));

    let resolved = resolver.resolve_name(&db, &Name::new("Метод"));
    assert!(matches!(resolved, Some(Resolution::Method(_))));

    let resolved = resolver.resolve_name(&db, &Name::new("Переменная"));
    assert!(matches!(resolved, Some(Resolution::Variable(_))));

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

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Перем Значение;");

    let mut expr_scopes = ExprScopes::new();
    expr_scopes.add_local_variable(expr_scopes.root_scope(), Name::new("Значение"));

    let root_scope = expr_scopes.root_scope();
    let resolver =
        Resolver::for_module(module_id).push_expr_scope(Arc::new(expr_scopes), root_scope);

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

    assert_eq!(resolver.scopes.len(), 2);
}

#[test]
fn test_resolver_cross_module_gated_by_configurations() {
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

    db.set_all_config_paths(vec![(None, std::path::PathBuf::from("/does-not-exist"))]);

    let after = resolver.resolve_path(&db, &path);
    assert!(
        matches!(after, PathResolution::Unresolved(_)),
        "with a config registered but no matching declaration, resolution must fail, got {:?}",
        after
    );
}

#[test]
fn test_all_sdbl_in_file_basic() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
КонецПроцедуры"#,
    );

    let queries = db.all_sdbl_in_file(file_id);
    assert_eq!(queries.len(), 1, "Should extract 1 SDBL query");
    assert!(queries[0].1.is_valid(), "SDBL should parse successfully");

    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос1 = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
    Запрос2 = "ВЫБРАТЬ Наименование ИЗ Справочник.Категории";
КонецПроцедуры"#,
    );

    let queries = db.all_sdbl_in_file(file_id);
    assert_eq!(queries.len(), 2, "Should extract 2 SDBL queries");
    assert!(queries.iter().all(|(_, q)| q.is_valid()));
}

#[test]
fn test_all_sdbl_in_file_keyword_filter() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Строка = "Это просто строка без ключевых слов";
    Запрос = "ВЫБРАТЬ * ИЗ Справочник.Товары";
КонецПроцедуры"#,
    );

    let queries = db.all_sdbl_in_file(file_id);
    assert_eq!(queries.len(), 1, "Should filter by SELECT/ВЫБРАТЬ keyword");
    assert!(queries[0].1.query_text.contains("ВЫБРАТЬ"));
}

#[test]
fn test_all_sdbl_in_file_multiline() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

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

    let query_text = &queries[0].1.query_text;
    assert!(query_text.contains("Ссылка"));
    assert!(query_text.contains("Наименование"));
    assert!(query_text.contains("Справочник.Товары"));
}

#[test]
fn test_all_sdbl_in_file_assignment_patterns() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

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
    assert_eq!(queries.len(), 3, "Should extract queries from various contexts");

    for (_, query_info) in queries.iter() {
        assert!(query_info.is_valid(), "All queries should parse successfully");
    }
}

#[test]
fn test_all_sdbl_in_file_with_parameters() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

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

    assert_eq!(queries.len(), 1, "Should extract query with parameters");

    assert!(queries[0].1.is_valid(), "Query with parameters should parse successfully");

    assert!(queries[0].1.query_text.contains("&Значение1"));
    assert!(queries[0].1.query_text.contains("&Значение2"));
    assert!(queries[0].1.query_text.contains("&Значение3"));
}

#[test]
fn test_module_metadata_creation() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/CommonModules/ОбщегоНазначения/Ext/Module.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

    let module_id = ModuleId::new(file_id);
    let metadata = db.module_metadata(module_id);

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

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

    let module_id = ModuleId::new(file_id);
    let _module_bodies = db.module_bodies(module_id);
    let _module_metadata = db.module_metadata(module_id);
}

#[test]
fn test_module_metadata_cache_invalidation() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");
    let module_id = ModuleId::new(file_id);
    let _metadata1 = db.module_metadata(module_id);

    db.set_file_text(file_id, "Процедура Тест2() КонецПроцедуры");
    let _metadata2 = db.module_metadata(module_id);
}

#[test]
fn test_sdbl_hir_in_file_basic() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
КонецПроцедуры"#,
    );

    let hirs = db.sdbl_hir_in_file(file_id);
    assert_eq!(hirs.len(), 1, "Should have 1 SDBL HIR");

    let (_, sdbl_hir) = &hirs[0];
    assert!(!sdbl_hir.queries()[0].hir.from.is_empty(), "Should have FROM clause");
    assert_eq!(sdbl_hir.queries()[0].hir.from[0].full_name, "Справочник.Товары");
}

#[test]
fn sdbl_hir_for_extension_file_uses_base_configuration_standard_attributes() {
    let temp_dir = tempfile::tempdir().unwrap();
    let main_root = temp_dir.path().join("src/cf");
    let extension_root = temp_dir.path().join("src/cfe/BMS_RU_UT");
    std::fs::create_dir_all(main_root.join("Catalogs")).unwrap();
    std::fs::create_dir_all(extension_root.join("Catalogs")).unwrap();

    std::fs::write(main_root.join("Configuration.xml"), "<Configuration/>").unwrap();
    std::fs::write(extension_root.join("Configuration.xml"), "<Configuration/>").unwrap();
    std::fs::write(
        main_root.join("Catalogs/Номенклатура.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-000000000001">
        <Properties>
            <Name>Номенклатура</Name>
            <Hierarchical>true</Hierarchical>
            <CodeLength>9</CodeLength>
            <DescriptionLength>25</DescriptionLength>
        </Properties>
    </Catalog>
</MetaDataObject>"#,
    )
    .unwrap();
    std::fs::write(
        extension_root.join("Catalogs/Номенклатура.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-000000000002">
        <Properties>
            <ObjectBelonging>Adopted</ObjectBelonging>
            <Name>Номенклатура</Name>
        </Properties>
    </Catalog>
</MetaDataObject>"#,
    )
    .unwrap();

    let mut db = RootDatabaseImpl::new();
    db.set_all_config_paths(vec![
        (None, main_root.clone()),
        (Some("BMS_RU_UT".to_string()), extension_root.clone()),
    ]);

    let file_id = FileId(0);
    let file_path = extension_root.join("CommonModules/Модуль/Ext/Module.bsl");
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new(file_path.to_string_lossy().as_ref()));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Номенклатура.Родитель КАК Родитель ИЗ Справочник.Номенклатура КАК Номенклатура";
КонецПроцедуры"#,
    );

    let hirs = db.sdbl_hir_in_file(file_id);
    assert_eq!(hirs.len(), 1, "Should have 1 SDBL HIR");

    let package = &hirs[0].1;
    let unresolved: Vec<_> = package
        .source_map
        .tokens_by_category(sdbl_hir::TokenCategory::UnresolvedFieldName)
        .iter()
        .map(|token| token.text.as_str())
        .collect();
    let resolved: Vec<_> = package
        .source_map
        .tokens_by_category(sdbl_hir::TokenCategory::FieldName)
        .iter()
        .map(|token| token.text.as_str())
        .collect();

    assert!(resolved.contains(&"Родитель"), "Родитель should resolve: {resolved:?}");
    assert!(!unresolved.contains(&"Родитель"), "Родитель must not be unresolved: {unresolved:?}");
}

#[test]
fn test_sdbl_hir_in_file_multiple_queries() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос1 = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
    Запрос2 = "ВЫБРАТЬ Номер ИЗ Документ.РасходнаяНакладная";
КонецПроцедуры"#,
    );

    let hirs = db.sdbl_hir_in_file(file_id);
    assert_eq!(hirs.len(), 2, "Should have 2 SDBL HIRs");

    assert_eq!(hirs[0].1.queries()[0].hir.from[0].full_name, "Справочник.Товары");

    assert_eq!(hirs[1].1.queries()[0].hir.from[0].full_name, "Документ.РасходнаяНакладная");
}

#[test]
fn test_sdbl_hir_in_file_caching() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
КонецПроцедуры"#,
    );

    let hirs1 = db.sdbl_hir_in_file(file_id);

    let hirs2 = db.sdbl_hir_in_file(file_id);

    assert!(Arc::ptr_eq(&hirs1, &hirs2), "Should return cached result");
}

#[test]
fn test_sdbl_hir_in_file_invalidation() {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);

    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(file_id, SourceRootId(0));

    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
КонецПроцедуры"#,
    );
    let hirs1 = db.sdbl_hir_in_file(file_id);
    assert_eq!(hirs1[0].1.queries()[0].hir.from[0].full_name, "Справочник.Товары");

    db.set_file_text(
        file_id,
        r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Номер ИЗ Документ.Продажа";
КонецПроцедуры"#,
    );
    let hirs2 = db.sdbl_hir_in_file(file_id);

    assert!(!Arc::ptr_eq(&hirs1, &hirs2), "Should invalidate cache on file change");

    assert_eq!(hirs2[0].1.queries()[0].hir.from[0].full_name, "Документ.Продажа");
}

#[test]
fn test_resolved_module_summary_targets() {
    use hir::call_graph::{CallTarget, EdgeProvenance, ResolvedTarget};
    use hir::ConfigsDatabase;

    let mut db = RootDatabaseImpl::new();
    let caller = FileId(0);
    let utils = FileId(1);

    let mut file_set = FileSet::new();
    file_set.insert(caller, VfsPath::new("/src/CommonModules/Клиент/Ext/Module.bsl"));
    file_set.insert(utils, VfsPath::new("/src/CommonModules/Утилиты/Ext/Module.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(caller, SourceRootId(0));
    db.set_file_source_root(utils, SourceRootId(0));

    db.set_file_text(
        utils,
        "Функция ПроверитьИНН() Экспорт КонецФункции\n\
         Процедура Приватная() КонецПроцедуры",
    );
    db.set_file_text(
        caller,
        "Процедура ЛокальнаяЦель() Экспорт КонецПроцедуры\n\
         Процедура Главная() Экспорт\n\
         ЛокальнаяЦель();\n\
         Утилиты.ПроверитьИНН();\n\
         Утилиты.Приватная();\n\
         НетТакогоМодуля.Метод();\n\
         КонецПроцедуры",
    );

    let summary = db.resolved_module_summary(ModuleId::new(caller));
    let caller_module = ModuleId::new(caller);
    let utils_module = ModuleId::new(utils);

    let resolved: Vec<_> =
        summary.edges.iter().filter(|e| e.provenance == EdgeProvenance::Resolved).collect();
    assert_eq!(resolved.len(), 2, "local + exported-qualified call resolve");

    // Local call resolves to a method in the caller's own module.
    assert!(resolved.iter().any(|e| matches!(
        &e.target,
        ResolvedTarget::Method(m) if m.module == caller_module
    )));
    // Exported qualified call resolves to a method in the target common module.
    assert!(resolved.iter().any(|e| matches!(
        &e.target,
        ResolvedTarget::Method(m) if m.module == utils_module
    )));

    // Non-exported qualified target is visible but unreachable across modules,
    // and the original target payload is preserved (surfaced, not dropped).
    let blocked: Vec<_> = summary
        .edges
        .iter()
        .filter(|e| e.provenance == EdgeProvenance::VisibilityBlocked)
        .collect();
    assert_eq!(blocked.len(), 1);
    assert!(matches!(
        &blocked[0].target,
        ResolvedTarget::Unresolved(CallTarget::QualifiedModule { method_name, .. })
            if method_name.as_str() == "Приватная"
    ));

    // Unknown module → honestly surfaced as unresolved with its original name preserved.
    let unresolved: Vec<_> =
        summary.edges.iter().filter(|e| e.provenance == EdgeProvenance::Unresolved).collect();
    assert_eq!(unresolved.len(), 1);
    assert!(matches!(
        &unresolved[0].target,
        ResolvedTarget::Unresolved(CallTarget::QualifiedModule { module_name, .. })
            if module_name.as_str() == "НетТакогоМодуля"
    ));
}

#[test]
fn test_resolved_module_summary_manager_access() {
    use hir::call_graph::{EdgeProvenance, ResolvedTarget};
    use hir::ConfigsDatabase;

    let mut db = RootDatabaseImpl::new();
    let caller = FileId(0);
    let mgr = FileId(1);

    let mut file_set = FileSet::new();
    file_set.insert(caller, VfsPath::new("/src/CommonModules/Клиент/Ext/Module.bsl"));
    file_set.insert(mgr, VfsPath::new("/src/Catalogs/Контрагенты/Ext/ManagerModule.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(caller, SourceRootId(0));
    db.set_file_source_root(mgr, SourceRootId(0));

    db.set_file_text(
        mgr,
        "Функция НайтиПоИНН() Экспорт КонецФункции\n\
         Процедура Внутренняя() КонецПроцедуры",
    );
    db.set_file_text(
        caller,
        "Процедура Главная() Экспорт\n\
         Справочники.Контрагенты.НайтиПоИНН();\n\
         Справочники.Контрагенты.Внутренняя();\n\
         Справочники.Контрагенты.СоздатьЭлемент();\n\
         КонецПроцедуры",
    );

    let summary = db.resolved_module_summary(ModuleId::new(caller));
    let mgr_module = ModuleId::new(mgr);

    // A user-defined, exported manager-module method resolves to its node (Inferred).
    assert!(
        summary.edges.iter().any(|e| e.provenance == EdgeProvenance::Inferred
            && matches!(&e.target, ResolvedTarget::Method(m) if m.module == mgr_module)),
        "Справочники.Контрагенты.НайтиПоИНН should resolve to the manager-module method"
    );
    // A non-exported manager-module method is visible but unreachable across modules.
    assert_eq!(
        summary.edges.iter().filter(|e| e.provenance == EdgeProvenance::VisibilityBlocked).count(),
        1,
        "Справочники.Контрагенты.Внутренняя is non-export → VisibilityBlocked"
    );
    // A platform creation method (СоздатьЭлемент) is not a user node — it touches
    // the metadata object, so it resolves to an Mdo target via a ManagerCreates edge.
    use bsl_metadata::MdoType;
    use hir::call_graph::EdgeKind;
    assert!(
        summary.edges.iter().any(|e| e.provenance == EdgeProvenance::Inferred
            && e.kind == EdgeKind::ManagerCreates
            && matches!(&e.target, ResolvedTarget::Mdo { mdo_type, object_name }
                if *mdo_type == MdoType::Catalog && object_name.as_str() == "Контрагенты")),
        "Платформенный СоздатьЭлемент should resolve to an Mdo node via manager_creates"
    );
}

#[test]
fn test_workspace_call_graph_callers_and_callees() {
    use hir::call_graph::{GraphNode, ResolvedTarget};
    use hir::ConfigsDatabase;

    let mut db = RootDatabaseImpl::new();
    let caller = FileId(0);
    let utils = FileId(1);

    let mut file_set = FileSet::new();
    file_set.insert(caller, VfsPath::new("/src/CommonModules/Клиент/Ext/Module.bsl"));
    file_set.insert(utils, VfsPath::new("/src/CommonModules/Утилиты/Ext/Module.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(caller, SourceRootId(0));
    db.set_file_source_root(utils, SourceRootId(0));

    db.set_file_text(utils, "Функция ПроверитьИНН() Экспорт КонецФункции");
    db.set_file_text(
        caller,
        "Процедура Главная() Экспорт\n\
         Утилиты.ПроверитьИНН();\n\
         КонецПроцедуры",
    );

    let caller_module = ModuleId::new(caller);
    let utils_module = ModuleId::new(utils);

    // Derive the resolved target MethodId without hardcoding a local_id.
    let caller_summary = db.resolved_module_summary(caller_module);
    let target = caller_summary
        .edges
        .iter()
        .find_map(|e| match &e.target {
            ResolvedTarget::Method(m) if m.module == utils_module => Some(*m),
            _ => None,
        })
        .expect("Утилиты.ПроверитьИНН should resolve");

    let graph = db.workspace_call_graph(SourceRootId(0));

    // Reverse adjacency: callers of the utils method include a method in the caller module.
    let callers = graph.callers(&GraphNode::Method(target));
    assert!(!callers.is_empty(), "utils method must have a caller");
    assert!(callers.iter().all(|e| e.to == GraphNode::Method(target)));
    assert!(callers
        .iter()
        .any(|e| matches!(e.from, GraphNode::Method(m) if m.module == caller_module)));

    // Forward adjacency: the caller node lists the utils method as a callee.
    let caller_node = match &callers[0].from {
        GraphNode::Method(_) => callers[0].from.clone(),
        other => panic!("expected a method caller, got {other:?}"),
    };
    let callees = graph.callees(&caller_node);
    assert!(callees.iter().any(|e| e.to == GraphNode::Method(target)));
}

#[test]
fn test_workspace_call_graph_module_code_and_multiple_callers() {
    use hir::call_graph::{GraphNode, ResolvedTarget};
    use hir::ConfigsDatabase;

    let mut db = RootDatabaseImpl::new();
    let caller = FileId(0);
    let utils = FileId(1);

    let mut file_set = FileSet::new();
    file_set.insert(caller, VfsPath::new("/src/CommonModules/Клиент/Ext/Module.bsl"));
    file_set.insert(utils, VfsPath::new("/src/CommonModules/Утилиты/Ext/Module.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(caller, SourceRootId(0));
    db.set_file_source_root(utils, SourceRootId(0));

    db.set_file_text(utils, "Функция Ц() Экспорт КонецФункции");
    // Two methods plus trailing module-body code all call the same target.
    db.set_file_text(
        caller,
        "Процедура П1() Экспорт\n\
         Утилиты.Ц();\n\
         КонецПроцедуры\n\
         Процедура П2() Экспорт\n\
         Утилиты.Ц();\n\
         КонецПроцедуры\n\
         Утилиты.Ц();",
    );

    let caller_module = ModuleId::new(caller);
    let utils_module = ModuleId::new(utils);

    let target = db
        .resolved_module_summary(caller_module)
        .edges
        .iter()
        .find_map(|e| match &e.target {
            ResolvedTarget::Method(m) if m.module == utils_module => Some(*m),
            _ => None,
        })
        .expect("Утилиты.Ц should resolve");

    let graph = db.workspace_call_graph(SourceRootId(0));
    let callers = graph.callers(&GraphNode::Method(target));

    assert_eq!(callers.len(), 3, "two methods + module-body code call the target");
    assert!(
        callers.iter().any(|e| e.from == GraphNode::ModuleCode(caller_module)),
        "module-body call is attributed to the ModuleCode node"
    );
    let method_callers = callers.iter().filter(|e| matches!(e.from, GraphNode::Method(_))).count();
    assert_eq!(method_callers, 2, "П1 and П2 are distinct method callers");

    // The callee is client-capable (default), so no edge — including the
    // ModuleCode caller — is a client→server crossing.
    assert!(callers.iter().all(|e| !e.crosses_client_to_server));
}

#[test]
fn test_workspace_call_graph_client_server_boundary() {
    use hir::call_graph::{GraphNode, ResolvedTarget};
    use hir::ConfigsDatabase;

    let mut db = RootDatabaseImpl::new();
    let caller = FileId(0);
    let utils = FileId(1);

    let mut file_set = FileSet::new();
    file_set.insert(caller, VfsPath::new("/src/CommonModules/Клиент/Ext/Module.bsl"));
    file_set.insert(utils, VfsPath::new("/src/CommonModules/Сервер/Ext/Module.bsl"));
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), source_root);
    db.set_file_source_root(caller, SourceRootId(0));
    db.set_file_source_root(utils, SourceRootId(0));

    db.set_file_text(
        utils,
        "&НаСервере\n\
         Функция СерверныйМетод() Экспорт КонецФункции\n\
         &НаКлиентеНаСервере\n\
         Функция Универсальный() Экспорт КонецФункции",
    );
    db.set_file_text(
        caller,
        "&НаКлиенте\n\
         Процедура Клиентский() Экспорт\n\
         Сервер.СерверныйМетод();\n\
         Сервер.Универсальный();\n\
         КонецПроцедуры",
    );

    let utils_module = ModuleId::new(utils);
    let resolve = |method: &str| {
        db.resolved_module_summary(ModuleId::new(caller))
            .edges
            .iter()
            .filter_map(|e| match &e.target {
                ResolvedTarget::Method(m) if m.module == utils_module => Some(*m),
                _ => None,
            })
            .find(|m| {
                db.symbol_tree(utils_module)
                    .find_method_by_id(*m)
                    .is_some_and(|s| s.name.as_str() == method)
            })
            .unwrap_or_else(|| panic!("Сервер.{method} should resolve"))
    };
    let server_method = resolve("СерверныйМетод");
    let universal = resolve("Универсальный");

    let graph = db.workspace_call_graph(SourceRootId(0));

    // Node dispatch is attached: the &НаСервере target is server-only.
    let dispatch = graph
        .dispatch(&GraphNode::Method(server_method))
        .expect("server method must have known dispatch");
    assert!(dispatch.is_server_only(), "&НаСервере method is server-only");

    // The client→server-only call is flagged as a boundary crossing.
    let server_callers = graph.callers(&GraphNode::Method(server_method));
    assert!(!server_callers.is_empty());
    assert!(
        server_callers.iter().all(|e| e.crosses_client_to_server),
        "&НаКлиенте → &НаСервере is a client→server roundtrip"
    );

    // A &НаКлиентеНаСервере callee is not server-only → NOT a boundary crossing.
    let universal_callers = graph.callers(&GraphNode::Method(universal));
    assert!(!universal_callers.is_empty());
    assert!(
        universal_callers.iter().all(|e| !e.crosses_client_to_server),
        "&НаКлиентеНаСервере callee is reachable on the client — no roundtrip"
    );
}

#[test]
fn test_workspace_call_graph_query_ref_links_method_to_mdo() {
    use bsl_metadata::MdoType;
    use hir::call_graph::{EdgeKind, EdgeProvenance, GraphNode};
    use hir::ConfigsDatabase;

    // The SDBL table must resolve against a configuration, so declare the catalog.
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("src/cf");
    std::fs::create_dir_all(root.join("Catalogs")).unwrap();
    std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
    std::fs::write(
        root.join("Catalogs/Номенклатура.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-000000000001">
        <Properties>
            <Name>Номенклатура</Name>
            <CodeLength>9</CodeLength>
        </Properties>
    </Catalog>
</MetaDataObject>"#,
    )
    .unwrap();

    let mut db = RootDatabaseImpl::new();
    db.set_all_config_paths(vec![(None, root.clone())]);

    let file_id = FileId(0);
    let file_path = root.join("CommonModules/Отчеты/Ext/Module.bsl");
    let mut file_set = FileSet::new();
    file_set.insert(file_id, VfsPath::new(file_path.to_string_lossy().as_ref()));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(file_id, SourceRootId(0));
    db.set_file_text(
        file_id,
        "Процедура Считать() Экспорт\n\
         Запрос = \"ВЫБРАТЬ Код ИЗ Справочник.Номенклатура\";\n\
         КонецПроцедуры",
    );

    let graph = db.workspace_call_graph(SourceRootId(0));
    let qref = graph
        .edges()
        .find(|e| e.kind == EdgeKind::QueryRef)
        .expect("the query reads Справочник.Номенклатура → one query_ref edge");
    assert!(matches!(&qref.from, GraphNode::Method(_)), "the reading method is the edge source");
    assert!(
        matches!(&qref.to, GraphNode::Mdo { mdo_type, object_name }
            if *mdo_type == MdoType::Catalog && object_name.as_str() == "Номенклатура"),
        "the edge targets the read object's Mdo node"
    );
    assert_eq!(qref.provenance, EdgeProvenance::Inferred);
    assert!(!qref.crosses_client_to_server);
}

/// Golden equivalence: building the whole-config graph through the resident
/// `GraphIndex` (the streaming-build path) must produce byte-for-byte the same
/// `WorkspaceCallGraph` as the monolithic Salsa fold, AND the same per-module
/// `ResolvedModuleSummary` (which carries the VisibilityBlocked/Unresolved
/// outcomes the graph itself drops).
///
/// No configuration is registered, so the visibility gate is a no-op and
/// resolution proceeds on the path-based module index alone — exactly like the
/// existing `test_resolved_module_summary_*` fixtures. This lets the calls
/// actually reach every resolution arm. Coverage is asserted explicitly (below)
/// so the equality is not silently vacuous.
#[test]
fn workspace_call_graph_via_index_matches_salsa_fold() {
    use bsl_metadata::MdoType;
    use hir::call_graph::{EdgeKind, EdgeProvenance, ResolvedTarget};
    use hir::graph_index::{
        resolve_module_summary_via_index, workspace_call_graph_via_index, GraphIndex,
    };
    use hir::ConfigsDatabase;

    let files: &[(&str, &str)] = &[
        (
            "/src/CommonModules/Клиент/Ext/Module.bsl",
            "&НаКлиенте\n\
             Процедура Главная() Экспорт\n\
             ЛокальнаяЦель();\n\
             Сервер.Считать();\n\
             Сервер.Приватная();\n\
             НетМодуля.Метод();\n\
             ЭтотОбъект.НетМетода();\n\
             Справочники.Контрагенты.НайтиПоИНН();\n\
             Справочники.Контрагенты.Внутренняя();\n\
             Справочники.Контрагенты.НетТакого();\n\
             Справочники.Номенклатура.СоздатьЭлемент();\n\
             Справочники.Номенклатура.НайтиПоКоду();\n\
             КонецПроцедуры\n\
             &НаКлиенте\n\
             Процедура ЛокальнаяЦель() Экспорт КонецПроцедуры",
        ),
        (
            "/src/CommonModules/Сервер/Ext/Module.bsl",
            "&НаСервере\n\
             Функция Считать() Экспорт КонецФункции\n\
             &НаСервере\n\
             Функция Приватная() КонецФункции",
        ),
        (
            "/src/Catalogs/Контрагенты/Ext/ManagerModule.bsl",
            "Функция НайтиПоИНН() Экспорт КонецФункции\n\
             Процедура Внутренняя() КонецПроцедуры",
        ),
    ];

    let mut db = RootDatabaseImpl::new();
    let mut file_set = FileSet::new();
    for (i, (path, _)) in files.iter().enumerate() {
        file_set.insert(FileId(i as u32), VfsPath::new(*path));
    }
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    for (i, (_, text)) in files.iter().enumerate() {
        let fid = FileId(i as u32);
        db.set_file_source_root(fid, SourceRootId(0));
        db.set_file_text(fid, text);
    }

    // Enumerate modules exactly as the fold does (same iteration order → same
    // edge insertion order, so the two graphs compare equal).
    let source_root = db.source_root_input(SourceRootId(0)).root(&db);
    let file_set = source_root.file_set();
    let modules: Vec<ModuleId> = source_root
        .iter()
        .filter(|&f| hir::is_bsl_source(file_set, f))
        .map(ModuleId::new)
        .collect();

    let salsa = db.workspace_call_graph(SourceRootId(0));
    let index = GraphIndex::build(&db, &modules);
    let via_index = workspace_call_graph_via_index(&db, &modules, &index);

    assert_eq!(via_index, *salsa, "index-backed graph must equal the Salsa fold");

    // Coverage: prove the caller's summary actually hits every resolution arm, so
    // the equality above is not vacuous. (The index path equals this summary by
    // the per-module assertion below, so reaching the arm here proves it there.)
    let caller = db.resolved_module_summary(ModuleId::new(FileId(0)));
    let has = |pred: &dyn Fn(&hir::ResolvedCallEdge) -> bool| caller.edges.iter().any(pred);
    assert!(
        has(&|e| e.provenance == EdgeProvenance::Resolved
            && matches!(e.target, ResolvedTarget::Method(_))),
        "local + exported-qualified → Resolved method"
    );
    assert!(
        caller.edges.iter().filter(|e| e.provenance == EdgeProvenance::VisibilityBlocked).count()
            >= 2,
        "non-exported qualified (Приватная) and manager (Внутренняя) → VisibilityBlocked"
    );
    assert!(
        has(&|e| e.provenance == EdgeProvenance::Unresolved),
        "unknown module / ThisObject method → Unresolved"
    );
    assert!(
        has(&|e| e.provenance == EdgeProvenance::Inferred
            && matches!(e.target, ResolvedTarget::Method(_))),
        "exported manager-module method (НайтиПоИНН) → Inferred method"
    );
    assert!(
        has(&|e| e.kind == EdgeKind::ManagerCreates
            && matches!(&e.target, ResolvedTarget::Mdo { mdo_type, .. } if *mdo_type == MdoType::Catalog)),
        "platform СоздатьЭлемент on a manager-less object → Mdo + ManagerCreates"
    );
    assert!(
        has(&|e| e.kind == EdgeKind::ManagerAccess
            && matches!(e.target, ResolvedTarget::Mdo { .. })),
        "platform find / absent manager method → Mdo + ManagerAccess"
    );

    for &module in &modules {
        let salsa_summary = db.resolved_module_summary(module);
        let index_summary = resolve_module_summary_via_index(&db, module, &index);
        assert_eq!(
            index_summary, *salsa_summary,
            "per-module ResolvedModuleSummary must match for {module:?}"
        );
    }
}
