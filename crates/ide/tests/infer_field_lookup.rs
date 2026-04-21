//! Behavioural tests for the `Expr::Field` → `FieldLookup` rewire landed
//! in M3 Task 8.
//!
//! These tests exercise the full pipeline end-to-end:
//! 1. A JSDoc-annotated CommonModule function in the fixture returns a
//!    metadata-reference type (`СправочникСсылка.Справочник1`).
//! 2. The test BSL file calls it, binds the return to a variable, and
//!    then accesses a field on that variable.
//! 3. Inference must route `Expr::Field` through `field_lookup`, reading
//!    the real `Configuration` loaded from the designer fixture via
//!    `db.configurations(file_id)`, and return the attribute's lowered
//!    `Ty`.
//!
//! The designer fixture at `crates/bsl-metadata/fixtures/designer` owns
//! `Catalog "Справочник1"` with:
//! - `CodeLength=9` → standard attribute `Код: String`
//! - `Реквизит1: String (length 10)`
//! - `Реквизит2: Number (digits 10, fraction 0)`
//! - `Реквизит3: Boolean`
//! - tabular section `ТабличнаяЧасть1` with `Реквизит1: String`,
//!   `Реквизит2: Number`.
//!
//! If the fixture changes shape these tests are the canary — the
//! assertions name the exact fields they depend on.
//!
//! The caller CommonModule (`ПервыйОбщийМодуль`) is one of the common
//! modules the designer configuration actually declares in its
//! `Configuration.xml`; using a module name the CFE visibility gate
//! doesn't recognise would fail resolution before `Expr::Field` ever
//! runs. The test overrides the module's body through the fixture VFS
//! (same absolute path inside the workspace-style layout), so the JSDoc
//! return hint is the one the Resolver reads.

use hir::{HirDatabase, Name, Ty};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use test_fixture::Fixture;
use vfs::FileId;

/// Absolute path of the designer fixture, resolved at compile time so
/// the test doesn't depend on the cargo-run CWD.
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

    // Point the metadata bridge at the designer fixture — that's what
    // puts `Catalog Справочник1` into `db.configurations(file_id)` so
    // `FieldLookup` can resolve its attributes.
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

#[test]
fn infer_field_catalog_custom_attribute_typed() {
    // Custom attribute `Реквизит2: Number`. JSDoc on the CommonModule
    // function gives the call site a `Ty::MetadataRef { CatalogRef,
    // "Справочник1" }`; `.Реквизит2` must flow through `FieldLookup`
    // against the real `Configuration` and produce `Ty::Number`. This
    // is the baseline proof that the `Expr::Field` rewire reads from
    // `db.configurations(file_id)`.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникСсылка.Справочник1 - ссылка
Функция Ссылка() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    С = ПервыйОбщийМодуль.Ссылка();
    Р = С.Реквизит2;
    Возврат Р;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "р"),
        Some(Ty::Number),
        "Expr::Field must resolve Реквизит2 on Ty::MetadataRef → Ty::Number"
    );
}

#[test]
fn infer_field_catalog_standard_attribute_typed() {
    // Standard attribute `Код` (CodeLength=9 → String). Standard
    // attributes are added by the XML loader to `mdo.attributes`, so
    // FieldLookup hits them through the same path as custom ones — this
    // test proves the rewire doesn't special-case custom vs. standard.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникСсылка.Справочник1
Функция Ссылка() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    С = ПервыйОбщийМодуль.Ссылка();
    К = С.Код;
    Возврат К;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "к"),
        Some(Ty::String),
        "Standard attribute Код must resolve to Ty::String (CodeLength=9)"
    );
}

#[test]
fn infer_field_catalog_boolean_attribute_typed() {
    // `Реквизит3: Boolean` — separate test pins the Boolean branch of
    // `TypeRef::from_attribute_type` and guards against a regression
    // where only Number / String flow through but Boolean silently
    // degrades to Unknown.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникСсылка.Справочник1
Функция Ссылка() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    С = ПервыйОбщийМодуль.Ссылка();
    Б = С.Реквизит3;
    Возврат Б;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(var_ty(&db, file_id, "б"), Some(Ty::Boolean));
}

#[test]
fn infer_field_unknown_attribute_stays_none() {
    // Regression guard: unresolved field lookup returns Ty::Unknown, which
    // `infer_stmt` treats as "no assignment tracked" (the var_types map
    // gets no entry). This is the same "don't lie about types we don't
    // know" contract as the jsdoc-missing test in `infer_jsdoc_types`.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникСсылка.Справочник1
Функция Ссылка() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    С = ПервыйОбщийМодуль.Ссылка();
    Х = С.НесуществующееПоле;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        None,
        "unresolved field must produce Ty::Unknown (no var_types entry)"
    );
}

#[test]
fn infer_field_tabular_section_promotes_to_tabular_section_ty() {
    // `С.ТабличнаяЧасть1` on a `СправочникСсылка.Справочник1` receiver
    // must promote to `Ty::MetadataRef { TabularSection { parent:
    // Catalog }, "Справочник1.ТабличнаяЧасть1" }`. This is the
    // first half of the chain `С.ТабличнаяЧасть1[0].Реквизит2` that
    // M4 narrowing will build on; Task 8 proves the promotion survives
    // the Expr::Field rewire.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникСсылка.Справочник1
Функция Ссылка() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    С = ПервыйОбщийМодуль.Ссылка();
    Т = С.ТабличнаяЧасть1;
    Возврат Т;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "т").expect("Tabular-section access should set a var type");
    match ty {
        Ty::MetadataRef { kind, name } => {
            assert_eq!(
                kind,
                hir::MetadataKind::TabularSection { parent: bsl_metadata::MdoType::Catalog },
                "promoted kind must be TabularSection {{ parent: Catalog }}"
            );
            assert_eq!(name, Name::new("Справочник1.ТабличнаяЧасть1"));
        }
        other => panic!("expected Ty::MetadataRef(TabularSection), got {other:?}"),
    }
}
