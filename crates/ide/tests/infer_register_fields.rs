//! End-to-end regression for M4 Task 2 — register field lookup.
//!
//! These tests exercise the `Expr::Field` → `FieldLookup::lookup_on_register`
//! path through the full inference pipeline:
//! 1. A JSDoc-annotated CommonModule function returns a register receiver
//!    type (`РегистрСведенийКлючЗаписи.РегистрСведений1`).
//! 2. The test BSL file calls it, binds the return to a variable, and
//!    then accesses a dimension / resource / attribute.
//! 3. Inference routes the field access through `field_lookup`, which
//!    reads the real `Configuration.registers` loaded from the designer
//!    fixture via `db.configurations(file_id)`, and returns the per-part
//!    lowered `Ty` (or the symbolic `Register{Dimension,Resource,
//!    Attribute}` fallback if `attr_type` is absent).
//!
//! The designer fixture at `crates/bsl-metadata/fixtures/designer` owns
//! `InformationRegister "РегистрСведений1"` with one dimension
//! `Справочник1: CatalogRef.Справочник1` (see the XML file of the same
//! name). If the fixture shape changes these tests are the canary —
//! each assertion names the exact dimension/resource/attribute it
//! depends on.
//!
//! # Scope note
//!
//! E2E coverage here exercises the dimension path end-to-end because
//! that is the only register part the shared designer fixture declares.
//! Resources, attributes, and the symbolic-fallback (untyped part) path
//! are pinned exhaustively at the unit-test layer in
//! `crates/hir-ty/src/field_lookup.rs::tests` (see the
//! `field_lookup_register_*` matrix). Extending the XML fixture to
//! cover the remaining parts at e2e level is a future-PR chore —
//! shared-fixture edits affect every other test that reads it.

use hir::{HirDatabase, MetadataKind, TypeId, TypeKernelDb, TypeKind};
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

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<TypeId> {
    db.infer(file_id).var_types.get(var_lower).copied()
}

fn assert_metadata_ref(
    db: &RootDatabaseImpl,
    actual: Option<TypeId>,
    kind: MetadataKind,
    name: &str,
) {
    let actual = actual.expect("expected metadata ref type");
    assert!(
        matches!(
            db.lookup_type(actual),
            TypeKind::MetadataRef(facet)
                if facet.kind == kind && facet.name.as_str() == name
        ),
        "expected MetadataRef({kind:?}, {name}), got {:?}",
        db.lookup_type(actual)
    );
}

#[test]
fn infer_register_dimension_resolves_to_catalog_ref() {
    // The designer fixture's `РегистрСведений1` has a single dimension
    // `Справочник1` typed `CatalogRef.Справочник1`. Accessing it on a
    // `РегистрСведенийКлючЗаписи` (InformationRegisterRef) receiver
    // must produce `Ty::MetadataRef { CatalogRef, "Справочник1" }` —
    // this is the happy path that proves `lookup_on_register` reads
    // `Configuration.registers` and lowers the typed dimension through
    // `TyLoweringContext`.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   РегистрСведенийКлючЗаписи.РегистрСведений1
Функция Ключ() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    К = ПервыйОбщийМодуль.Ключ();
    С = К.Справочник1;
    Возврат С;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_metadata_ref(&db, var_ty(&db, file_id, "с"), MetadataKind::CatalogRef, "Справочник1");
}

#[test]
fn infer_for_each_over_record_set_yields_record_kind() {
    // `Для Каждого Запись Из Набор Цикл` over an InformationRegister
    // record-set receiver must bind the loop variable to
    // `MetadataRef { InformationRegisterRecord, <ИмяРегистра> }` per
    // HBK iteration spec. This is the receiver shape that lookups for
    // dimensions/resources/attributes + standard properties + platform
    // methods all key off of (now that `register_parent_for_kind` has
    // the *Record arm).
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   РегистрСведенийНаборЗаписей.РегистрСведений1
Функция Набор() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест()
    Н = ПервыйОбщийМодуль.Набор();
    Для Каждого Запись Из Н Цикл
        Х = Запись;
    КонецЦикла;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert_metadata_ref(
        &db,
        var_ty(&db, file_id, "запись"),
        MetadataKind::InformationRegisterRecord,
        "РегистрСведений1",
    );
}

#[test]
fn infer_register_record_dimension_resolves_through_field_lookup() {
    // After `Для Каждого Запись Из Набор`, accessing
    // `Запись.Справочник1` must resolve through the same
    // `lookup_on_register` path as the *RecordSet/*Ref kinds — the new
    // `*Record` arm in `register_parent_for_kind` is what unlocks this.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   РегистрСведенийНаборЗаписей.РегистрСведений1
Функция Набор() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест()
    Н = ПервыйОбщийМодуль.Набор();
    Для Каждого Запись Из Н Цикл
        С = Запись.Справочник1;
    КонецЦикла;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert_metadata_ref(&db, var_ty(&db, file_id, "с"), MetadataKind::CatalogRef, "Справочник1");
}

#[test]
fn infer_record_set_dimension_regression() {
    // Insurance regression: the *RecordSet dimension path used to work
    // before the *Record arm was added. Adding new match arms to
    // `register_parent_for_kind` could regress its existing kinds, so
    // this test pins the *RecordSet path explicitly.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   РегистрСведенийНаборЗаписей.РегистрСведений1
Функция Набор() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    Н = ПервыйОбщийМодуль.Набор();
    С = Н.Справочник1;
    Возврат С;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_metadata_ref(&db, var_ty(&db, file_id, "с"), MetadataKind::CatalogRef, "Справочник1");
}

#[test]
fn infer_register_missing_field_stays_unknown() {
    // Accessing a non-existent field on a register receiver must fall
    // through to `Ty::Unknown` — same contract as the regular MDO field
    // miss: the variable gets no entry in `var_types`. The separate
    // `UnresolvedField` diagnostic path (M4 Task 1) consumes the `None`
    // to emit its warning.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   РегистрСведенийКлючЗаписи.РегистрСведений1
Функция Ключ() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    К = ПервыйОбщийМодуль.Ключ();
    Х = К.НесуществующееПоле;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        None,
        "missing register part must stay Unknown (no var_types entry)",
    );
}
