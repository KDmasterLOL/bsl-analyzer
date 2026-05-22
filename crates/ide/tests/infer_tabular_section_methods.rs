//! End-to-end inference tests for the TabularSection method bridge.
//!
//! Covers the full BSL chain
//! ```bsl
//!   ОбъектСпр   = Справочники.Справочник1.СоздатьЭлемент();
//!   НоваяСтрока = ОбъектСпр.ТабличнаяЧасть1.Добавить();
//!   Кол         = НоваяСтрока.Реквизит2;
//!   НомСтр      = НоваяСтрока.НомерСтроки;
//! ```
//! against the designer fixture at `crates/bsl-metadata/fixtures/designer`,
//! which already declares `Catalog Справочник1` with
//! `ТабличнаяЧасть1` (`Реквизит1: String`, `Реквизит2: Number`).
//!
//! The receiver `СправочникОбъект.Справочник1` is provided by a
//! JSDoc-annotated CommonModule function — the same trick as
//! `infer_field_lookup.rs` — so the test does not depend on the manager
//! 3-segment call path.

use hir::{HirDatabase, InferenceDiagnostic, MetadataKind, Name, Ty, UnresolvedMethodKind};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use test_fixture::Fixture;
use vfs::FileId;

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn setup(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    let fixture = Fixture::parse(fixture_text);
    let mut db = RootDatabaseImpl::new();
    let mut file_set = vfs::FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
    }
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    for (file_id, file) in &fixture.files {
        db.set_file_source_root(*file_id, SourceRootId(0));
        db.set_file_text(*file_id, &file.content);
    }
    db.set_all_config_paths(vec![(None, designer_fixture_path())]);
    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    (db, test_file)
}

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<Ty> {
    db.infer(file_id).var_types.get(var_lower).cloned()
}

const OBJECT_RETURNING_MODULE: &str = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникОбъект.Справочник1
Функция Объект() Экспорт
    Возврат Неопределено;
КонецФункции

"#;

#[test]
fn infer_full_tabular_section_chain() {
    // The motivating user scenario, end-to-end. After the bridge:
    //   - ОбъектСпр    : MetadataRef { CatalogObject, "Справочник1" }
    //   - Тч           : MetadataRef { TabularSection { Catalog }, "Справочник1.ТабличнаяЧасть1" }
    //   - НоваяСтрока  : MetadataRef { TabularSectionRow { Catalog }, "Справочник1.ТабличнаяЧасть1" }
    //   - Кол          : Number  (custom row attribute Реквизит2)
    //   - НомСтр       : Number  (platform standard row property НомерСтроки)
    let fixture = format!(
        r#"{OBJECT_RETURNING_MODULE}
//- /test.bsl
Функция Тест()
    ОбъектСпр   = ПервыйОбщийМодуль.Объект();
    Тч          = ОбъектСпр.ТабличнаяЧасть1;
    НоваяСтрока = Тч.Добавить();
    Кол         = НоваяСтрока.Реквизит2;
    НомСтр      = НоваяСтрока.НомерСтроки;
    Возврат Кол;
КонецФункции
"#
    );
    let (db, file_id) = setup(&fixture);

    assert_eq!(
        var_ty(&db, file_id, "объектспр"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::CatalogObject, name: Name::new("Справочник1")
        }),
        "ОбъектСпр must carry CatalogObject — JSDoc-typed receiver is the entry point",
    );
    assert_eq!(
        var_ty(&db, file_id, "тч"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::TabularSection { parent: bsl_metadata::MdoType::Catalog },
            name: Name::new("Справочник1.ТабличнаяЧасть1"),
        }),
        "field-lookup must promote ТабличнаяЧасть1 to TabularSection",
    );
    assert_eq!(
        var_ty(&db, file_id, "новаястрока"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: bsl_metadata::MdoType::Catalog },
            name: Name::new("Справочник1.ТабличнаяЧасть1"),
        }),
        "Добавить() must rebind to TabularSectionRow with parent: Catalog",
    );
    assert_eq!(
        var_ty(&db, file_id, "кол"),
        Some(Ty::Number),
        "row attribute Реквизит2 must resolve to Number",
    );
    assert_eq!(
        var_ty(&db, file_id, "номстр"),
        Some(Ty::Number),
        "platform standard row property НомерСтроки must resolve to Number",
    );
}

#[test]
fn infer_tabular_section_count_returns_number() {
    let fixture = format!(
        r#"{OBJECT_RETURNING_MODULE}
//- /test.bsl
Функция Тест()
    ОбъектСпр = ПервыйОбщийМодуль.Объект();
    Кол       = ОбъектСпр.ТабличнаяЧасть1.Количество();
    Возврат Кол;
КонецФункции
"#
    );
    let (db, file_id) = setup(&fixture);
    assert_eq!(var_ty(&db, file_id, "кол"), Some(Ty::Number));
}

#[test]
fn infer_tabular_section_unload_returns_value_table() {
    let fixture = format!(
        r#"{OBJECT_RETURNING_MODULE}
//- /test.bsl
Функция Тест()
    ОбъектСпр = ПервыйОбщийМодуль.Объект();
    ТЗ        = ОбъектСпр.ТабличнаяЧасть1.Выгрузить();
    Возврат ТЗ;
КонецФункции
"#
    );
    let (db, file_id) = setup(&fixture);
    assert_eq!(var_ty(&db, file_id, "тз"), Some(Ty::ValueTable { projection: None }));
}

#[test]
fn unresolved_method_call_fires_on_tabular_section_typo() {
    // After the bridge, `mdo_kind_to_plural` returns Some for
    // TabularSection — so a typo'd method name surfaces as
    // `UnresolvedMethodCall` instead of being silently swallowed.
    let fixture = format!(
        r#"{OBJECT_RETURNING_MODULE}
//- /test.bsl
Процедура Тест()
    ОбъектСпр = ПервыйОбщийМодуль.Объект();
    ОбъектСпр.ТабличнаяЧасть1.НетТакогоМетодаНаТЧ();
КонецПроцедуры
"#
    );
    let (db, file_id) = setup(&fixture);
    let infer = db.infer(file_id);
    let unresolved: Vec<_> = infer
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall {
                receiver_name, method_name, kind, ..
            } => Some((receiver_name.clone(), method_name.clone(), *kind)),
            _ => None,
        })
        .collect();
    assert_eq!(
        unresolved.len(),
        1,
        "exactly one UnresolvedMethodCall must fire on a TS typo, got {unresolved:?}",
    );
    let (receiver, method, kind) = &unresolved[0];
    assert_eq!(method.as_str(), "НетТакогоМетодаНаТЧ");
    assert!(matches!(kind, UnresolvedMethodKind::MethodNotFound));
    assert_eq!(
        receiver.as_str(),
        "Справочники.Справочник1.ТабличнаяЧасть1",
        "receiver must render as <Plural>.<MdoName>.<Section>",
    );
}

#[test]
fn no_unresolved_method_call_on_valid_tabular_section_method() {
    // Negative twin of the typo test: the bridge resolves the call,
    // the diagnostic stays silent.
    let fixture = format!(
        r#"{OBJECT_RETURNING_MODULE}
//- /test.bsl
Процедура Тест()
    ОбъектСпр = ПервыйОбщийМодуль.Объект();
    ОбъектСпр.ТабличнаяЧасть1.Добавить();
КонецПроцедуры
"#
    );
    let (db, file_id) = setup(&fixture);
    let infer = db.infer(file_id);
    let unresolved: Vec<_> = infer
        .diagnostics
        .iter()
        .filter(|(_, d)| matches!(d, InferenceDiagnostic::UnresolvedMethodCall { .. }))
        .collect();
    assert!(
        unresolved.is_empty(),
        "valid TS method must not trigger UnresolvedMethodCall, got {unresolved:?}",
    );
}
