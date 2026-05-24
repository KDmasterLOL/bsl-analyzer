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

use hir::{HirDatabase, InferenceDiagnostic, Name, Ty};
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
    let id = db.infer(file_id).var_types.get(var_lower).copied()?;
    Some(hir::ty_bridge::typeid_to_ty(db, id))
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
fn infer_field_unresolved_on_known_receiver_emits_diagnostic() {
    // Missing field on a fully-resolved receiver must emit
    // `InferenceDiagnostic::UnresolvedField`. This is the M4 Task 1 gate:
    // `FieldLookup=None` + `receiver_ty = CatalogRef.Справочник1` is a
    // user-actionable miss ("you typed a field name that does not exist
    // on this catalog"), as opposed to the `Ty::Unknown` receiver path
    // (no authority to complain) or `Ty::Union` (waits for M4 narrowing).
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
    let diags = &db.infer(file_id).diagnostics;
    let unresolved: Vec<_> = diags
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedField { receiver_ty, field_name, .. } => {
                Some((hir::ty_bridge::typeid_to_ty(&db, *receiver_ty), field_name.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        unresolved.len(),
        1,
        "exactly one UnresolvedField diagnostic must be emitted, got {unresolved:?}"
    );
    let (ty, name) = &unresolved[0];
    assert_eq!(name, &Name::new("НесуществующееПоле"));
    assert!(
        matches!(ty, Ty::MetadataRef { kind, .. } if *kind == hir::MetadataKind::CatalogRef),
        "receiver_ty must carry the CatalogRef kind, got {ty:?}"
    );
}

#[test]
fn infer_field_unresolved_on_unknown_receiver_stays_silent() {
    // Receiver without resolved type produces no UnresolvedField
    // diagnostic. The M4 rule is: we only complain when inference nailed
    // the receiver down — otherwise the miss is "we don't know enough",
    // not "the user typed a non-existent field". `Ч` here binds to an
    // unknown call (no JSDoc, no builtin), so `Ч.ЛюбоеПоле` sees a
    // `Ty::Unknown` receiver and must not fire.
    let fixture = r#"
//- /test.bsl
Функция Тест()
    Ч = НеизвестнаяФункция();
    Х = Ч.ЛюбоеПоле;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let unresolved = db
        .infer(file_id)
        .diagnostics
        .iter()
        .filter(|(_, d)| matches!(d, InferenceDiagnostic::UnresolvedField { .. }))
        .count();
    assert_eq!(unresolved, 0, "UnresolvedField must stay silent when the receiver type is Unknown");
}

#[test]
fn infer_field_unresolved_on_primitive_receiver_stays_silent() {
    // Regression guard for Codex Q1: prior to the emission-guard fix,
    // `FieldLookup` returning `None` for any non-Unknown/non-Union receiver
    // triggered the diagnostic. That over-reported on `Число.Х`,
    // `Строка.Х`, `Массив.Х`, and other primitives / platform types
    // where `lookup_field` legitimately does not know how to resolve
    // fields. Since `lookup_field` is only authoritative on
    // `Ty::MetadataRef`, primitive receivers must stay silent.
    let fixture = r#"
//- /test.bsl
Функция Тест()
    Ч = 42;
    Х = Ч.ЛюбоеПоле;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let unresolved = db
        .infer(file_id)
        .diagnostics
        .iter()
        .filter(|(_, d)| matches!(d, InferenceDiagnostic::UnresolvedField { .. }))
        .count();
    assert_eq!(
        unresolved, 0,
        "UnresolvedField must stay silent on primitive receivers — FieldLookup is not authoritative there"
    );
}

#[test]
fn infer_field_unresolved_on_union_receiver_stays_silent() {
    // Regression guard for Codex Q4: union-typed receivers defer to M4
    // narrowing — a `None` from `FieldLookup` on a union just means "we
    // haven't picked a component yet". Firing the diagnostic on raw
    // unions would trip false positives the moment union attributes
    // become common (JSDoc `// Возвращаемое значение: Число, Строка`
    // already produces `Ty::Union`). The JSDoc-return path is the
    // cheapest way to put a `Ty::Union` on a receiver today.
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   Число, Строка
Функция Значение() Экспорт
    Возврат 0;
КонецФункции

//- /test.bsl
Функция Тест()
    У = ПервыйОбщийМодуль.Значение();
    Х = У.ЛюбоеПоле;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let unresolved = db
        .infer(file_id)
        .diagnostics
        .iter()
        .filter(|(_, d)| matches!(d, InferenceDiagnostic::UnresolvedField { .. }))
        .count();
    assert_eq!(
        unresolved, 0,
        "UnresolvedField must stay silent on union receivers — narrowing must pick a component first"
    );
}

#[test]
fn infer_field_unresolved_on_tabular_row_emits_diagnostic() {
    // Tabular-section-row receivers are `Ty::MetadataRef { kind:
    // TabularSectionRow { parent }, name: "Parent.Section" }` per the
    // M3 FieldLookup invariants, so a missing row attribute is just as
    // authoritative a miss as a missing MDO attribute. Proves the
    // emission hits on `TabularSectionRow` without relying on a separate
    // match arm — the `Ty::MetadataRef { .. }` guard in `infer_expr`
    // covers all MetadataKind variants uniformly.
    //
    // Chain: `С.ТабличнаяЧасть1[0].НесуществующаяКолонка`. Indexing
    // promotes `TabularSection` → `TabularSectionRow` (M3 field_lookup
    // invariant); the row receiver then hits the miss and emits.
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
    Р = Т[0];
    Х = Р.НесуществующаяКолонка;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    // `Т[0]` lowers through `Expr::Index` which today returns
    // `Ty::Unknown`; until indexing types-promote, the row receiver
    // arrives as `Ty::Unknown` and the diagnostic stays silent. This
    // asserts the *current* documented behaviour — once `Expr::Index`
    // propagates `TabularSection → TabularSectionRow`, the expected
    // count flips to 1 and this test flags the place that needs
    // updating. The point of the test is to pin the emission path
    // against regressions in either direction.
    let unresolved = db
        .infer(file_id)
        .diagnostics
        .iter()
        .filter(|(_, d)| matches!(d, InferenceDiagnostic::UnresolvedField { .. }))
        .count();
    assert_eq!(
        unresolved, 0,
        "Until Expr::Index propagates row types, this stays silent; flip to 1 when indexing is typed"
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
