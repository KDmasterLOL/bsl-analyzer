//! End-to-end regressions for `ЭтотОбъект` in an information register
//! `RecordSetModule.bsl`.
//!
//! These tests use the shared Designer fixture and inject only module text into
//! the salsa database. The fixture directory itself is never written.

use hir::{HirDatabase, MetadataKind, Name, Ty};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use vfs::{FileId, VfsPath};

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn register_recordset_module_path() -> PathBuf {
    designer_fixture_path().join("InformationRegisters/РегистрСведений1/Ext/RecordSetModule.bsl")
}

fn catalog_object_module_path() -> PathBuf {
    designer_fixture_path().join("Catalogs/Справочник1/Ext/ObjectModule.bsl")
}

fn common_module_path() -> PathBuf {
    designer_fixture_path().join("CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl")
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

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<Ty> {
    db.infer(file_id).var_types.get(var_lower).cloned()
}

#[test]
fn infer_this_record_set_resolves_in_information_register_module() {
    let text = r#"
Функция Тест()
    Набор = ЭтотОбъект;
    Возврат Набор;
КонецФункции
"#;
    let (db, file_id) = setup_at(register_recordset_module_path(), text);
    assert_eq!(
        var_ty(&db, file_id, "набор"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecordSet,
            name: Name::new("РегистрСведений1"),
        }),
    );
}

#[test]
fn infer_this_record_set_english_spelling() {
    let text = "Функция Тест()\n    T = ThisObject;\n    Return T;\nКонецФункции";
    let (db, file_id) = setup_at(register_recordset_module_path(), text);
    assert_eq!(
        var_ty(&db, file_id, "t"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecordSet,
            name: Name::new("РегистрСведений1"),
        }),
    );
}

#[test]
fn for_each_yields_information_register_record_kind() {
    let text = r#"
Процедура Тест()
    Для Каждого З Из ЭтотОбъект Цикл
        Х = З;
    КонецЦикла;
КонецПроцедуры
"#;
    let (db, file_id) = setup_at(register_recordset_module_path(), text);
    assert_eq!(
        var_ty(&db, file_id, "з"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecord,
            name: Name::new("РегистрСведений1"),
        }),
    );
}

#[test]
fn record_dimension_resolves() {
    let text = r#"
Процедура Тест()
    Для Каждого З Из ЭтотОбъект Цикл
        Р = З.Справочник1;
    КонецЦикла;
КонецПроцедуры
"#;
    let (db, file_id) = setup_at(register_recordset_module_path(), text);
    assert_eq!(
        var_ty(&db, file_id, "р"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::CatalogRef, name: Name::new("Справочник1")
        }),
    );
}

#[test]
fn record_standard_period_resolves() {
    let text = r#"
Процедура Тест()
    Для Каждого З Из ЭтотОбъект Цикл
        П = З.Период;
    КонецЦикла;
КонецПроцедуры
"#;
    let (db, file_id) = setup_at(register_recordset_module_path(), text);
    assert_eq!(var_ty(&db, file_id, "п"), Some(Ty::Date));
}

#[test]
fn record_standard_active_resolves() {
    let text = r#"
Процедура Тест()
    Для Каждого З Из ЭтотОбъект Цикл
        А = З.Активность;
    КонецЦикла;
КонецПроцедуры
"#;
    let (db, file_id) = setup_at(register_recordset_module_path(), text);
    assert_eq!(var_ty(&db, file_id, "а"), Some(Ty::Boolean));
}

#[test]
fn additional_properties_implicit_bare_resolves() {
    let text = r#"
Функция Тест()
    С = ДополнительныеСвойства;
    Возврат С;
КонецФункции
"#;
    let (db, file_id) = setup_at(register_recordset_module_path(), text);
    assert_eq!(var_ty(&db, file_id, "с"), Some(Ty::Structure));
}

#[test]
fn object_module_does_not_produce_record_set_metadata_ref() {
    let text = r#"
Функция Тест()
    Х = ЭтотОбъект;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup_at(catalog_object_module_path(), text);
    assert!(
        !matches!(
            var_ty(&db, file_id, "х"),
            Some(Ty::MetadataRef {
                kind: MetadataKind::InformationRegisterRecordSet
                    | MetadataKind::AccumulationRegisterRecordSet
                    | MetadataKind::AccountingRegisterRecordSet
                    | MetadataKind::CalculationRegisterRecordSet,
                ..
            })
        ),
        "Catalog ObjectModule must not infer any *RecordSet kind",
    );
}

#[test]
fn common_module_does_not_produce_record_set_metadata_ref() {
    let text = r#"
Функция Тест() Экспорт
    Набор = ЭтотОбъект;
    Возврат Набор;
КонецФункции
"#;
    let (db, file_id) = setup_at(common_module_path(), text);

    let infer = db.infer(file_id);
    let has_record_set = infer.var_types.values().any(|ty| {
        matches!(
            ty,
            Ty::MetadataRef {
                kind: MetadataKind::InformationRegisterRecordSet
                    | MetadataKind::AccumulationRegisterRecordSet
                    | MetadataKind::AccountingRegisterRecordSet
                    | MetadataKind::CalculationRegisterRecordSet,
                ..
            }
        )
    });
    assert!(!has_record_set);
}
