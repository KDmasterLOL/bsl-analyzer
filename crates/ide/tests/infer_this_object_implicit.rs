use bsl_metadata::MdoType;
use bsl_platform::PlatformDataInner;
use hir::{HirDatabase, InferenceDiagnostic, MetadataKind, Name, Ty, UnresolvedMethodKind};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use vfs::{FileId, VfsPath};

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn data_processor_object_module_path() -> PathBuf {
    designer_fixture_path().join("DataProcessors/ТестоваяОбработка/Ext/ObjectModule.bsl")
}

fn catalog_object_module_path() -> PathBuf {
    designer_fixture_path().join("Catalogs/Справочник1/Ext/ObjectModule.bsl")
}

fn common_module_path() -> PathBuf {
    designer_fixture_path().join("CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl")
}

fn data_processor_form_module_path() -> PathBuf {
    designer_fixture_path().join("DataProcessors/ТестоваяОбработка/Forms/Форма/Ext/Form/Module.bsl")
}

fn setup_at(path: PathBuf, text: &str) -> (RootDatabaseImpl, FileId) {
    let file_id = FileId::from_raw(1);
    let mut db = RootDatabaseImpl::new();
    let mut file_set = vfs::FileSet::default();
    file_set.insert(file_id, VfsPath::new(path.to_string_lossy().to_string()));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(file_id, SourceRootId(0));
    db.set_file_text(file_id, text);
    db.set_all_config_paths(vec![(None, designer_fixture_path())]);
    (db, file_id)
}

fn setup_data_processor(text: &str) -> (RootDatabaseImpl, FileId) {
    setup_at(data_processor_object_module_path(), text)
}

fn setup_catalog(text: &str) -> (RootDatabaseImpl, FileId) {
    setup_at(catalog_object_module_path(), text)
}

fn has_platform_data() -> bool {
    !PlatformDataInner::instance().all_methods().is_empty()
}

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<Ty> {
    db.infer(file_id).var_types.get(var_lower).cloned()
}

fn unresolved_method_kinds(db: &RootDatabaseImpl, file_id: FileId) -> Vec<UnresolvedMethodKind> {
    db.infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect()
}

fn assert_unknown_var(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) {
    let ty = var_ty(db, file_id, var_lower);
    assert!(
        ty.as_ref().is_none_or(Ty::is_unknown),
        "{var_lower} must stay Unknown or absent from var_types, got {ty:?}"
    );
}

#[test]
fn implicit_tabular_section_value_position() {
    let text = r#"
Функция Тест()
    Р = НастройкиЭксель;
    Возврат Р;
КонецФункции
"#;
    let (db, file_id) = setup_data_processor(text);
    assert_eq!(
        var_ty(&db, file_id, "р"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::TabularSection { parent: MdoType::DataProcessor },
            name: Name::new("ТестоваяОбработка.НастройкиЭксель"),
        }),
    );
}

#[test]
fn implicit_attribute_value_position() {
    let text = r#"
Функция Тест()
    Р = АдресСайта;
    Возврат Р;
КонецФункции
"#;
    let (db, file_id) = setup_data_processor(text);
    assert_eq!(var_ty(&db, file_id, "р"), Some(Ty::String));
}

#[test]
fn implicit_standard_attribute_ссылка_value_position() {
    let text = r#"
Функция Тест()
    Р = Ссылка;
    Возврат Р;
КонецФункции
"#;
    let (db, file_id) = setup_catalog(text);
    assert_eq!(
        var_ty(&db, file_id, "р"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::CatalogRef, name: Name::new("Справочник1")
        }),
    );
}

#[test]
fn implicit_self_blocked_by_parameter_shadow() {
    let text = r#"
Функция Тест(НастройкиЭксель)
    Р = НастройкиЭксель;
    Возврат Р;
КонецФункции
"#;
    let (db, file_id) = setup_data_processor(text);
    assert_unknown_var(&db, file_id, "р");
}

#[test]
fn implicit_self_blocked_by_перем_shadow() {
    let text = r#"
Перем НастройкиЭксель;

Функция Тест()
    Р = НастройкиЭксель;
    Возврат Р;
КонецФункции
"#;
    let (db, file_id) = setup_data_processor(text);
    assert_unknown_var(&db, file_id, "р");
}

#[test]
#[ignore = "needs fixture CommonModules/НастройкиЭксель metadata; current fixture cannot express visible CommonModule collision without mutation"]
fn user_common_module_wins_value_position() {
    let text = r#"
Функция Тест()
    Р = НастройкиЭксель;
    Возврат Р;
КонецФункции
"#;
    let (db, file_id) = setup_data_processor(text);
    assert_unknown_var(&db, file_id, "р");
}

#[test]
#[ignore = "needs fixture extension for platform-global-name collision"]
fn implicit_self_wins_over_platform_global() {
    let text = r#"
Функция Тест()
    Р = Метаданные;
    Возврат Р;
КонецФункции
"#;
    let (db, file_id) = setup_data_processor(text);
    assert_ne!(var_ty(&db, file_id, "р"), Some(Ty::Unknown));
}

#[test]
fn implicit_tabular_section_method_dispatches() {
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available");
        return;
    }

    let text = r#"
Функция Тест()
    ТЗ = НастройкиЭксель.Выгрузить();
    Возврат ТЗ;
КонецФункции
"#;
    let (db, file_id) = setup_data_processor(text);
    assert!(unresolved_method_kinds(&db, file_id).is_empty());
    assert_eq!(var_ty(&db, file_id, "тз"), Some(Ty::ValueTable { projection: None }));
}

#[test]
fn implicit_tabular_section_method_not_found() {
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available");
        return;
    }

    let text = r#"
Процедура Тест()
    НастройкиЭксель.НетТакогоМетода();
КонецПроцедуры
"#;
    let (db, file_id) = setup_data_processor(text);
    assert_eq!(unresolved_method_kinds(&db, file_id), vec![UnresolvedMethodKind::MethodNotFound]);
}

#[test]
fn implicit_object_method_call() {
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available");
        return;
    }

    let text = r#"
Процедура Тест()
    Записать();
    ЭтотОбъект.Записать();
КонецПроцедуры
"#;
    let (db, file_id) = setup_catalog(text);
    assert!(unresolved_method_kinds(&db, file_id).is_empty());
}

#[test]
#[ignore = "pre-existing gap: resolve_iter_element_ty / metadata_kind_to_prefix_and_mdo return None for MetadataRef{TabularSection,..} (platform_manager_lookup.rs:205-206). Same gap blocks Для Каждого X Из ЭтотОбъект.НастройкиЭксель. Follow-up scope."]
fn implicit_tabular_section_for_each() {
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available");
        return;
    }

    let text = r#"
Процедура Тест()
    Для Каждого Стр Из НастройкиЭксель Цикл
        Х = Стр;
    КонецЦикла;
КонецПроцедуры
"#;
    let (db, file_id) = setup_data_processor(text);
    assert_eq!(
        var_ty(&db, file_id, "стр"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: MdoType::DataProcessor },
            name: Name::new("ТестоваяОбработка.НастройкиЭксель"),
        }),
    );
}

#[test]
#[ignore = "pre-existing gap: lookup_field for MetadataRef{TabularSection,..} terminates in enumerate_fields without falling through to platform_property_lookup, so .Колонки/.Количество on a tabular-section receiver are invisible (field_lookup.rs:179-184). Same gap blocks ЭтотОбъект.НастройкиЭксель.Колонки. Follow-up scope."]
fn implicit_field_access_without_call() {
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available");
        return;
    }

    let text = r#"
Функция Тест()
    К = НастройкиЭксель.Колонки;
    Возврат К;
КонецФункции
"#;
    let (db, file_id) = setup_data_processor(text);
    let ty = var_ty(&db, file_id, "к");
    assert!(ty.as_ref().is_some_and(|ty| !ty.is_unknown()), "К must be typed, got {ty:?}");
}

#[test]
fn non_object_module_does_not_get_implicit_self() {
    let text = r#"
Функция Тест() Экспорт
    Р = НастройкиЭксель;
    Возврат Р;
КонецФункции
"#;
    let (db, file_id) = setup_at(common_module_path(), text);
    assert_unknown_var(&db, file_id, "р");
}

#[test]
fn form_module_does_not_get_object_implicit_self() {
    let text = r#"
Функция Тест()
    Ф = Объект;
    Р = АдресСайта;
    Возврат Ф;
КонецФункции
"#;
    let (db, file_id) = setup_at(data_processor_form_module_path(), text);
    assert!(matches!(var_ty(&db, file_id, "ф"), Some(Ty::FormData { underlying: Some(_), .. })));
    assert_unknown_var(&db, file_id, "р");
}
