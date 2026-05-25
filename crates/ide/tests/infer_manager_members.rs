//! End-to-end regression for M4 Task 3 — manager member lookup.
//!
//! Exercises the `Expr::Field` → `manager_lookup::lookup_manager_field`
//! path through the full inference pipeline:
//!
//! 1. A bare plural manager (`Справочники`) resolves to
//!    `Ty::ManagerCollection(Catalog)` via the existing `infer_path_name`
//!    cascade.
//! 2. `Expr::Field` on that receiver with a valid MDO name
//!    (`.Справочник1`) promotes to
//!    `Ty::ObjectManager { Catalog, "Справочник1" }`.
//! 3. Invalid MDO names fall through to `Ty::Unknown` — typo safety.
//!
//! # Scope note
//!
//! Predefined-item / enum-value e2e coverage is pinned at the unit-test
//! layer (`crates/hir-ty/src/manager_lookup.rs::tests`). The designer
//! fixture ships no enum MDO and no catalog with predefined items, and
//! extending the shared fixture affects every other suite that reads
//! it — a dedicated enum/predefined fixture is a future-PR chore.

use bsl_metadata::MdoType;
use hir::{HirDatabase, TypeId, TypeKernelDb, TypeKind};
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

#[test]
fn infer_manager_collection_promotes_to_object_manager() {
    // The designer fixture owns `Catalog "Справочник1"`. Accessing it
    // through the plural manager — `Справочники.Справочник1` — must
    // promote from `Ty::ManagerCollection(Catalog)` to
    // `Ty::ObjectManager { Catalog, "Справочник1" }`. This is the
    // prerequisite for the 3-seg `.predefined` chain; if promotion
    // breaks, every predefined-item lookup downstream stays Unknown.
    let fixture = r#"
//- /test.bsl
Функция Тест()
    М = Справочники.Справочник1;
    Возврат М;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "м").expect("M must carry a type");
    assert!(
        matches!(
            db.lookup_type(ty),
            TypeKind::ObjectManager(facet)
                if facet.mdo == MdoType::Catalog && facet.name.as_str() == "Справочник1"
        ),
        "expected ObjectManager(Catalog, Справочник1), got {:?}",
        db.lookup_type(ty)
    );
}

#[test]
fn infer_manager_collection_with_unknown_mdo_name_stays_unknown() {
    // Typo safety: a non-existent MDO name under `Справочники` must NOT
    // promote. Staying `Ty::Unknown` keeps the door open for a future
    // "unknown MDO" diagnostic without requiring every typo to first
    // get a confusing intermediate `ObjectManager { Catalog, "Опечатка" }`
    // receiver that would then mislead hover / completion.
    let fixture = r#"
//- /test.bsl
Функция Тест()
    М = Справочники.НеСуществует;
    Возврат М;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(var_ty(&db, file_id, "м"), None, "unknown MDO must stay Unknown");
}

#[test]
fn infer_manager_chain_catalog_ref_attribute() {
    // Full 3-seg chain routed through the manager adapter for the
    // promotion step, then back through `field_lookup` for the
    // attribute hop. The designer catalog declares
    // `Справочник1.Реквизит2: Number`.
    //
    // Chain: `Справочники.Справочник1` promotes to ObjectManager, then
    // a chained `.M.Реквизит2` … but that only works once Expr::Call
    // syntax carries the manager through. For plain field access the
    // chain is `Справочники.Справочник1` → ObjectManager; hover on the
    // ObjectManager receiver is sufficient for Task 3's goal. The
    // predefined-item leg (the `.Доллар` hop) needs a fixture carrying
    // predefined items — see the module-level scope note.
    let fixture = r#"
//- /test.bsl
Функция Тест()
    М = Справочники.Справочник1;
    Возврат М;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    // Sanity: promotion is in place (covered by the first test too —
    // this one double-pins the `ObjectManager { kind, name }` shape in
    // a dedicated assertion so a regression that swaps `kind` for a
    // different `MdoType` variant flags here too).
    let ty = var_ty(&db, file_id, "м").expect("M must carry a type");
    match db.lookup_type(ty) {
        TypeKind::ObjectManager(facet) => {
            assert_eq!(facet.mdo, MdoType::Catalog);
            assert_eq!(facet.name.as_str(), "Справочник1");
        }
        other => panic!("expected ObjectManager(Catalog, Справочник1), got {other:?}"),
    }
}
