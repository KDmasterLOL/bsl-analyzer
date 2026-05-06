//! End-to-end regression for M4 Task 5 — `Ty::ThisObject` + coercion.
//!
//! Exercises the `Expr::Path("ЭтотОбъект")` → `Ty::ThisObject` →
//! `FieldLookup` coercion pipeline end-to-end:
//!
//! 1. A bare `ЭтотОбъект` / `ThisObject` identifier inside an
//!    `ObjectModule` resolves to
//!    `Ty::ThisObject { owner: (MdoType, Name) }` with the enclosing
//!    MDO's kind and name.
//! 2. `ЭтотОбъект.Attribute` on that receiver coerces the receiver to
//!    `Ty::MetadataRef { *Object, name }` at the `FieldLookup` adapter
//!    entry and resolves the attribute through the MDO's declared
//!    attribute list.
//! 3. Non-`ObjectModule` files (common modules, test harness defaults)
//!    where `resolve_this_object` returns `None` fall through and
//!    `ЭтотОбъект` stays `Ty::Unknown` — no spurious promotion.
//!
//! # Scope note
//!
//! This test file uses the designer fixture's `Catalog Справочник1`
//! because that is the MDO with declared attributes (`Реквизит1`,
//! `Реквизит2`, `Реквизит3`) and a known `xs:decimal` typed field
//! (`Реквизит2 → Ty::Number`). Document / ExchangePlan /
//! ChartOfAccounts `*Object` coercion is pinned exhaustively at the
//! unit-test layer in `crates/hir-ty/src/this_object.rs::tests`;
//! extending the designer fixture to cover every MDO family at e2e
//! level would ripple through every other suite that reads it and is
//! out of scope for Task 5.

use bsl_metadata::MdoType;
use hir::{HirDatabase, InferenceDiagnostic, MetadataKind, Name, Ty};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use vfs::{FileId, VfsPath};

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

/// Absolute path to the catalog `ObjectModule.bsl` inside the designer
/// fixture. Using the real filesystem path means
/// `RootDatabaseImpl::find_configuration_root` (which walks up looking
/// for `CommonModules/` or `Configuration.xml`) locates the designer
/// fixture's root, and `build_module_metadata` populates
/// `ModuleMetadata::mdo` with `Catalog "Справочник1"`. That is what
/// `Resolver::resolve_this_object` keys off.
fn catalog_object_module_path() -> PathBuf {
    designer_fixture_path().join("Catalogs/Справочник1/Ext/ObjectModule.bsl")
}

fn common_module_path() -> PathBuf {
    designer_fixture_path().join("CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl")
}

/// Task ObjectModule path — exercises the new `*Object` companions
/// (`TaskObject`) added together with form `Объект` projection. The
/// fixture has a single user attribute (`Комментарий`) so a coercion
/// regression on the new variants would surface as a missing field
/// type at the `ЭтотОбъект.Комментарий` field-access site.
fn task_object_module_path() -> PathBuf {
    designer_fixture_path().join("Tasks/ТестоваяЗадача/Ext/ObjectModule.bsl")
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

fn setup(text: &str) -> (RootDatabaseImpl, FileId) {
    setup_at(catalog_object_module_path(), text)
}

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<Ty> {
    db.infer(file_id).var_types.get(var_lower).cloned()
}

#[test]
fn infer_this_object_resolves_to_catalog_owner() {
    // Bare `ЭтотОбъект` inside a Catalog `ObjectModule` must resolve
    // to `Ty::ThisObject { owner: (Catalog, "Справочник1") }`. The
    // owner pair is the provenance carrier that future diagnostics
    // and rename features rely on — collapsing to MetadataRef at the
    // Ty level would erase it.
    let text = r#"
Функция Тест()
    Э = ЭтотОбъект;
    Возврат Э;
КонецФункции
"#;
    let (db, file_id) = setup(text);
    assert_eq!(
        var_ty(&db, file_id, "э"),
        Some(Ty::ThisObject { owner: (MdoType::Catalog, Name::new("Справочник1")) }),
    );
}

#[test]
fn infer_this_object_english_spelling() {
    // `ThisObject` (English) must resolve identically to `ЭтотОбъект`
    // — the intercept in `infer_path_name` is case-insensitive and
    // bilingual, matching the rest of BSL's identifier-handling
    // conventions.
    let text = r#"
Функция Test()
    T = ThisObject;
    Возврат T;
КонецФункции
"#;
    let (db, file_id) = setup(text);
    assert_eq!(
        var_ty(&db, file_id, "t"),
        Some(Ty::ThisObject { owner: (MdoType::Catalog, Name::new("Справочник1")) }),
    );
}

#[test]
fn infer_this_object_field_access_resolves_via_coercion() {
    // `ЭтотОбъект.Реквизит2` — the coercion at `FieldLookup`'s entry
    // rewrites the receiver to `MetadataRef { CatalogObject,
    // "Справочник1" }`, then the regular MDO-attribute walk finds
    // `Реквизит2` typed as `xs:decimal` (→ `Ty::Number`). This is the
    // full-pipeline proof: intercept → ThisObject → coerce → field
    // lookup → lowered type.
    let text = r#"
Функция Тест()
    Ч = ЭтотОбъект.Реквизит2;
    Возврат Ч;
КонецФункции
"#;
    let (db, file_id) = setup(text);
    assert_eq!(
        var_ty(&db, file_id, "ч"),
        Some(Ty::Number),
        "ЭтотОбъект.Реквизит2 must coerce to CatalogObject and resolve to Number",
    );
}

#[test]
fn infer_this_object_unknown_field_stays_unknown() {
    // Typo safety: accessing a non-existent attribute on `ЭтотОбъект`
    // must fall through to `Ty::Unknown` (no `var_types` entry for
    // `х`) AND emit an `UnresolvedField` diagnostic — after the
    // coercion, the receiver IS authoritative (the catalog's
    // attribute list was actually checked), so the miss is
    // user-actionable in exactly the same way as a miss on an
    // explicit `CatalogRef.Справочник1` receiver.
    let text = r#"
Функция Тест()
    Х = ЭтотОбъект.НесуществующийРеквизит;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(text);
    assert_eq!(var_ty(&db, file_id, "х"), None);

    let infer = db.infer(file_id);
    let unresolved: Vec<_> = infer
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedField { receiver_ty, field_name, .. } => {
                Some((receiver_ty.clone(), field_name.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        unresolved.len(),
        1,
        "exactly one UnresolvedField must fire on ЭтотОбъект miss, got {unresolved:?}"
    );
    let (receiver_ty, field_name) = &unresolved[0];
    assert_eq!(
        receiver_ty,
        &Ty::ThisObject { owner: (MdoType::Catalog, Name::new("Справочник1")) },
        "receiver_ty must preserve ThisObject provenance for the diagnostic"
    );
    assert_eq!(field_name.as_str(), "НесуществующийРеквизит");
}

#[test]
fn infer_this_object_in_common_module_stays_unknown() {
    // `Resolver::resolve_this_object` returns `None` for any
    // non-`ObjectModule` — CommonModule is the canonical case. The
    // intercept in `infer_path_name` observes the `None` and lets the
    // name fall through the normal cascade, landing on `Ty::Unknown`
    // (the identifier is not a valid receiver in a common module).
    //
    // This pins the "not all modules promote" contract so a
    // follow-up PR that adds form / record-set support cannot
    // accidentally start promoting common-module `ЭтотОбъект` too.
    let text = r#"
Функция Тест() Экспорт
    Возврат ЭтотОбъект;
КонецФункции
"#;
    let (db, file_id) = setup_at(common_module_path(), text);

    // No `var_types` entry in the common-module body carries
    // `Ty::ThisObject { .. }`. A regression that starts promoting
    // common-module `ЭтотОбъект` would violate Task 5's scope
    // (coercion only covers `*Object` MDO kinds).
    let infer = db.infer(file_id);
    let has_this_object = infer.var_types.values().any(|ty| matches!(ty, Ty::ThisObject { .. }));
    assert!(!has_this_object, "common module must not produce Ty::ThisObject");
}

#[test]
fn infer_this_object_coercion_pins_object_kind() {
    // Double-pins the coercion target shape from Task 5's perspective.
    // If a regression swaps the coerced `MetadataKind` (e.g. returns
    // `CatalogRef` instead of `CatalogObject`), `Ссылка` on the
    // coerced receiver would still work (both kinds carry the same
    // attribute list via `find_metadata_object`) — so pin the kind
    // through the `Ссылка` attribute, which lowers to
    // `CatalogRef.Справочник1` under the catalog and therefore proves
    // the intermediate receiver was indeed resolved against the
    // catalog, not something else.
    let text = r#"
Функция Тест()
    С = ЭтотОбъект.Ссылка;
    Возврат С;
КонецФункции
"#;
    let (db, file_id) = setup(text);
    assert_eq!(
        var_ty(&db, file_id, "с"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::CatalogRef, name: Name::new("Справочник1")
        }),
    );
}

#[test]
fn infer_this_object_resolves_in_task_object_module() {
    // Coverage for the new `*Object` companions added together with
    // form `Объект` projection. `MetadataKind::object_kind_for` now
    // returns `Some(TaskObject)` for `MdoType::Task`, so a Task's
    // `ObjectModule.bsl` must produce `Ty::ThisObject {(Task, name)}`
    // and `ЭтотОбъект.<attr>` must coerce through the same path the
    // Catalog cases above exercise — proving the new variants are
    // wired end-to-end (resolver gate → ThisObject → coerce →
    // field_lookup), not just compiled in.
    let text = r#"
Функция Тест()
    Э = ЭтотОбъект;
    К = ЭтотОбъект.Комментарий;
    Возврат Э;
КонецФункции
"#;
    let (db, file_id) = setup_at(task_object_module_path(), text);
    assert_eq!(
        var_ty(&db, file_id, "э"),
        Some(Ty::ThisObject { owner: (MdoType::Task, Name::new("ТестоваяЗадача")) }),
        "ЭтотОбъект in TaskObject must resolve to Ty::ThisObject(Task)",
    );
    assert_eq!(
        var_ty(&db, file_id, "к"),
        Some(Ty::String),
        "ЭтотОбъект.Комментарий in TaskObject must coerce to MetadataRef and resolve to String",
    );
}
