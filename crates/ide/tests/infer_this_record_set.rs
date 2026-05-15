//! End-to-end regressions for `ЭтотОбъект` in an information register
//! `RecordSetModule.bsl`.
//!
//! These tests use the shared Designer fixture and inject only module text into
//! the salsa database. The fixture directory itself is never written.

use hir::{HirDatabase, MetadataKind, Name, Ty};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use vfs::{FileId, VfsPath};

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn register_recordset_module_path() -> PathBuf {
    designer_fixture_path().join("InformationRegisters/РегистрСведений1/Ext/RecordSetModule.bsl")
}

fn temp_register_recordset_module_path(config_path: &std::path::Path) -> PathBuf {
    config_path.join("InformationRegisters/РегистрСведенийE2E/Ext/RecordSetModule.bsl")
}

fn temp_accumulation_recordset_module_path(config_path: &std::path::Path) -> PathBuf {
    config_path.join("AccumulationRegisters/РегистрНакопленияE2E/Ext/RecordSetModule.bsl")
}

fn temp_calculation_recordset_module_path(config_path: &std::path::Path) -> PathBuf {
    config_path.join("CalculationRegisters/РегистрРасчетаE2E/Ext/RecordSetModule.bsl")
}

fn catalog_object_module_path() -> PathBuf {
    designer_fixture_path().join("Catalogs/Справочник1/Ext/ObjectModule.bsl")
}

fn common_module_path() -> PathBuf {
    designer_fixture_path().join("CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl")
}

fn setup_at(path: PathBuf, text: &str) -> (RootDatabaseImpl, FileId) {
    setup_at_with_config(path, designer_fixture_path(), text)
}

fn setup_at_with_config(
    path: PathBuf,
    config_path: PathBuf,
    text: &str,
) -> (RootDatabaseImpl, FileId) {
    let file_id = FileId::from_raw(1);
    let mut db = RootDatabaseImpl::new();
    let mut file_set = vfs::FileSet::default();
    file_set.insert(file_id, VfsPath::new(path.to_string_lossy().to_string()));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(file_id, SourceRootId(0));
    db.set_file_text(file_id, text);
    db.set_all_config_paths(vec![(None, config_path)]);
    (db, file_id)
}

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<Ty> {
    db.infer(file_id).var_types.get(var_lower).cloned()
}

fn temp_designer_config_with_register_recorders() -> PathBuf {
    let suffix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir()
        .join(format!("bsl-analyzer-register-recorders-{}-{suffix}", std::process::id()));
    fs::create_dir_all(root.join("InformationRegisters")).unwrap();
    fs::create_dir_all(root.join("AccumulationRegisters")).unwrap();
    fs::create_dir_all(root.join("CalculationRegisters")).unwrap();
    fs::create_dir_all(root.join("Documents")).unwrap();
    fs::create_dir_all(root.join("CommonModules")).unwrap();

    fs::write(
        root.join("InformationRegisters/РегистрСведенийE2E.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <InformationRegister uuid="11111111-1111-1111-1111-111111111111">
        <Properties>
            <Name>РегистрСведенийE2E</Name>
            <InformationRegisterPeriodicity>Second</InformationRegisterPeriodicity>
        </Properties>
    </InformationRegister>
</MetaDataObject>"#,
    )
    .unwrap();
    fs::write(
        root.join("AccumulationRegisters/РегистрНакопленияE2E.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <AccumulationRegister uuid="22222222-2222-2222-2222-222222222222">
        <Properties>
            <Name>РегистрНакопленияE2E</Name>
        </Properties>
    </AccumulationRegister>
</MetaDataObject>"#,
    )
    .unwrap();
    fs::write(
        root.join("CalculationRegisters/РегистрРасчетаE2E.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <CalculationRegister uuid="44444444-4444-4444-4444-444444444444">
        <Properties>
            <Name>РегистрРасчетаE2E</Name>
            <Periodicity>Month</Periodicity>
            <ActionPeriod>false</ActionPeriod>
        </Properties>
    </CalculationRegister>
</MetaDataObject>"#,
    )
    .unwrap();
    for (file, doc_name) in
        [("ДокументE2E1.xml", "ДокументE2E1"), ("ДокументE2E2.xml", "ДокументE2E2")]
    {
        fs::write(
            root.join("Documents").join(file),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:xr="http://v8.1c.ru/8.3/xcf/readable" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" version="2.10">
    <Document uuid="33333333-3333-3333-3333-333333333333">
        <Properties>
            <Name>{doc_name}</Name>
            <RegisterRecords>
                <xr:Item xsi:type="xr:MDObjectRef">InformationRegister.РегистрСведенийE2E</xr:Item>
                <xr:Item xsi:type="xr:MDObjectRef">AccumulationRegister.РегистрНакопленияE2E</xr:Item>
                <xr:Item xsi:type="xr:MDObjectRef">CalculationRegister.РегистрРасчетаE2E</xr:Item>
            </RegisterRecords>
        </Properties>
    </Document>
</MetaDataObject>"#
            ),
        )
        .unwrap();
    }

    root
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
    // `Период` is `PresenceCondition::IsPeriodic`-gated in
    // `bsl_platform::standard_mdo_attributes`. The HBK cascade in
    // `push_platform_prefix_properties` correctly defers to that gate
    // (the spec is the sole arbiter of which standard attributes are
    // visible on the configured register). So the test must use a
    // *periodic* register — the designer-fixture `РегистрСведений1` is
    // `Nonperiodical` and would not expose `Период`.
    let config_path = temp_designer_config_with_register_recorders();
    let text = r#"
Процедура Тест()
    Для Каждого З Из ЭтотОбъект Цикл
        П = З.Период;
    КонецЦикла;
КонецПроцедуры
"#;
    let (db, file_id) =
        setup_at_with_config(temp_register_recordset_module_path(&config_path), config_path, text);
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
fn record_recorder_resolves_to_union_of_recorders() {
    let config_path = temp_designer_config_with_register_recorders();
    let text = r#"
Процедура Тест()
    Для Каждого Запись Из ЭтотОбъект Цикл
        Р = Запись.Регистратор;
    КонецЦикла;
КонецПроцедуры
"#;
    let (db, file_id) =
        setup_at_with_config(temp_register_recordset_module_path(&config_path), config_path, text);
    assert_eq!(
        var_ty(&db, file_id, "р"),
        Some(Ty::union(vec![
            Ty::MetadataRef {
                kind: MetadataKind::DocumentRef, name: Name::new("ДокументE2E1")
            },
            Ty::MetadataRef {
                kind: MetadataKind::DocumentRef, name: Name::new("ДокументE2E2")
            },
        ])),
    );
}

#[test]
fn filter_recorder_resolves_to_filter_item() {
    let config_path = temp_designer_config_with_register_recorders();
    let text = r#"
Процедура Тест()
    Э = ЭтотОбъект.Отбор.Регистратор;
КонецПроцедуры
"#;
    let (db, file_id) = setup_at_with_config(
        temp_accumulation_recordset_module_path(&config_path),
        config_path,
        text,
    );
    assert_eq!(var_ty(&db, file_id, "э"), Some(Ty::PlatformObject(Name::new("ЭлементОтбора"))),);
}

#[test]
fn filter_calculation_register_recorder_resolves() {
    let config_path = temp_designer_config_with_register_recorders();
    let text = r#"
Процедура Тест()
    Э = ЭтотОбъект.Отбор.Регистратор;
КонецПроцедуры
"#;
    let (db, file_id) = setup_at_with_config(
        temp_calculation_recordset_module_path(&config_path),
        config_path,
        text,
    );
    assert_eq!(var_ty(&db, file_id, "э"), Some(Ty::PlatformObject(Name::new("ЭлементОтбора"))),);
}

#[test]
fn filter_period_resolves_for_periodic_inforeg() {
    let config_path = temp_designer_config_with_register_recorders();
    let text = r#"
Процедура Тест()
    Э = ЭтотОбъект.Отбор.Период;
КонецПроцедуры
"#;
    let (db, file_id) =
        setup_at_with_config(temp_register_recordset_module_path(&config_path), config_path, text);
    assert_eq!(var_ty(&db, file_id, "э"), Some(Ty::PlatformObject(Name::new("ЭлементОтбора"))),);
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
