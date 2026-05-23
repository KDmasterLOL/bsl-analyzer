//! End-to-end regression for Track 1 Step J — `Ty::ThisManager` +
//! coercion in a workspace `ManagerModule.bsl`.
//!
//! Mirrors `infer_this_object.rs` but for the manager axis: a bare
//! `ЭтотОбъект` / `ThisObject` identifier inside `<Folder>/<MDO>/Ext/
//! ManagerModule.bsl` resolves to `Ty::ThisManager { owner: (MdoType,
//! Name) }`, NOT `Ty::ThisObject`. Coercion lands on
//! `Ty::ObjectManager { kind, name }` — the same shape qualified
//! manager access (`Справочники.<X>`, `РегистрыСведений.<X>`) already
//! produces — so platform manager methods (`СоздатьЭлемент()`,
//! `НайтиПоКоду()`, …) and workspace `ManagerModule.bsl` exports
//! resolve through the existing dispatch channels with no new code on
//! the consumer side.
//!
//! # Why a separate file
//!
//! Catches the same class of regressions the object-axis tests catch
//! (provenance pinning, gate scope, fall-through to `Ty::Unknown` on
//! out-of-scope module kinds), but for the manager axis. Pinning the
//! register path explicitly answers a stop-time review concern that
//! the gate might silently exclude register `MdoType`s — every
//! register flavour has `manager_type_prefix() = Some(_)` (see
//! `bsl_metadata::MdoType::manager_type_prefix`), so the gate is
//! permissive across the full register family; this file proves that
//! end-to-end with `InformationRegister`.

use bsl_metadata::MdoType;
use hir::{HirDatabase, InferenceDiagnostic, Name, Ty};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use vfs::{FileId, VfsPath};

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn catalog_manager_module_path() -> PathBuf {
    designer_fixture_path().join("Catalogs/Справочник1/Ext/ManagerModule.bsl")
}

/// Register manager-module path. The `InformationRegisters/.../Ext/
/// ManagerModule.bsl` shape is recognised by `metadata.rs::module_type`
/// as `ModuleType::ManagerModule` regardless of whether the parent is a
/// catalog or a register; the register's `MdoType::InformationRegister`
/// is then plumbed through `metadata.mdo` because the configuration's
/// metadata index has the register declared. This is the path that
/// pins the "registers participate too" contract end-to-end.
fn register_manager_module_path() -> PathBuf {
    designer_fixture_path().join("InformationRegisters/РегистрСведений1/Ext/ManagerModule.bsl")
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
    let id = db.infer(file_id).var_types.get(var_lower).copied()?;
    Some(hir::ty_bridge::typeid_to_ty(db, id))
}

#[test]
fn infer_this_manager_resolves_to_catalog_owner() {
    // Bare `ЭтотОбъект` inside a Catalog's `ManagerModule` must resolve
    // to `Ty::ThisManager { (Catalog, "Справочник1") }` — NOT
    // `Ty::ThisObject` (which the ObjectModule path produces). The
    // distinction matters because coercion targets diverge:
    // `ThisManager` → `Ty::ObjectManager` (manager dispatch),
    // `ThisObject` → `Ty::MetadataRef { CatalogObject, .. }` (record
    // dispatch).
    let text = r#"
Функция Тест()
    Э = ЭтотОбъект;
    Возврат Э;
КонецФункции
"#;
    let (db, file_id) = setup_at(catalog_manager_module_path(), text);
    assert_eq!(
        var_ty(&db, file_id, "э"),
        Some(Ty::ThisManager { owner: (MdoType::Catalog, Name::new("Справочник1")) }),
    );
}

#[test]
fn infer_this_manager_english_spelling() {
    // Same case-insensitive bilingual gate as the ObjectModule path.
    let text = r#"
Функция Test()
    T = ThisObject;
    Возврат T;
КонецФункции
"#;
    let (db, file_id) = setup_at(catalog_manager_module_path(), text);
    assert_eq!(
        var_ty(&db, file_id, "t"),
        Some(Ty::ThisManager { owner: (MdoType::Catalog, Name::new("Справочник1")) }),
    );
}

#[test]
fn infer_this_manager_resolves_in_information_register_module() {
    // Pins the register-axis claim explicitly. `InformationRegister`'s
    // `manager_type_prefix() = Some("InformationRegisterManager")` is
    // the gate `this_object::resolve_this_manager_owner` and
    // `coerce_to_metadata_ref` share, so a register's
    // `ManagerModule.bsl` must produce `Ty::ThisManager { (Register,
    // name) }` exactly the same way Catalog does. A regression that
    // narrows the gate to a hand-picked subset of `MdoType` (instead
    // of the `manager_type_prefix` table) would surface here as
    // `var_ty(...) == None` or a `Ty::Unknown` instead of the expected
    // `ThisManager`.
    let text = r#"
Функция Тест()
    Э = ЭтотОбъект;
    Возврат Э;
КонецФункции
"#;
    let (db, file_id) = setup_at(register_manager_module_path(), text);
    assert_eq!(
        var_ty(&db, file_id, "э"),
        Some(Ty::ThisManager {
            owner: (MdoType::InformationRegister, Name::new("РегистрСведений1"))
        }),
        "InformationRegister's `ЭтотОбъект` must produce Ty::ThisManager — \
         no MdoType-narrowing regression in the gate"
    );
}

#[test]
fn infer_this_manager_in_common_module_stays_unknown() {
    // Non-`ManagerModule` files where `resolve_this_manager_owner` returns
    // `None` must not produce `Ty::ThisManager`. Common module is the
    // canonical case; pinning it here guards the gate's "manager
    // module only" contract symmetrically to `ThisObject`'s
    // "object module only" pin in `infer_this_object.rs`.
    let text = r#"
Функция Тест() Экспорт
    Возврат ЭтотОбъект;
КонецФункции
"#;
    let (db, file_id) = setup_at(common_module_path(), text);

    let infer = db.infer(file_id);
    // Phase 3 §4.D: var_types stores TypeId; bridge before matching.
    let bridge = |tid: &hir::TypeId| hir::ty_bridge::typeid_to_ty(&db, *tid);
    let has_this_manager =
        infer.var_types.values().any(|tid| matches!(bridge(tid), Ty::ThisManager { .. }));
    assert!(!has_this_manager, "common module must not produce Ty::ThisManager");
    let has_this_object =
        infer.var_types.values().any(|tid| matches!(bridge(tid), Ty::ThisObject { .. }));
    assert!(!has_this_object, "common module must not produce Ty::ThisObject either");
}

#[test]
fn infer_this_manager_unknown_field_does_not_escalate_to_unresolved_field() {
    // Step J's UnresolvedField predicate (`infer.rs:1016`) deliberately
    // does NOT include `Ty::ThisManager` as authoritative: the coerced
    // `Ty::ObjectManager` has no attribute table in `lookup_field` /
    // `enumerate_fields`, so a miss there is not conclusive — managers
    // expose predefined items via the `ManagerCollection` indexing
    // path, not via field lookup. Promoting `ThisManager` to
    // authoritative would emit spurious `UnresolvedField` for valid
    // `ЭтотОбъект.<PredefinedName>` access.
    //
    // This test pins that posture: `ЭтотОбъект.НесуществующееПоле`
    // inside a ManagerModule must NOT emit `UnresolvedField` (we've
    // chosen "silent miss" over "false positive" until predefined-item
    // resolution lands as a separate enhancement).
    let text = r#"
Функция Тест()
    Х = ЭтотОбъект.НесуществующееПоле;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup_at(catalog_manager_module_path(), text);

    let infer = db.infer(file_id);
    let unresolved_count = infer
        .diagnostics
        .iter()
        .filter(|(_, d)| matches!(d, InferenceDiagnostic::UnresolvedField { .. }))
        .count();
    assert_eq!(
        unresolved_count, 0,
        "ManagerModule's ЭтотОбъект.<missing field> must NOT escalate to \
         UnresolvedField — see Step J docs in `field_lookup.rs` for the boundary"
    );
}
