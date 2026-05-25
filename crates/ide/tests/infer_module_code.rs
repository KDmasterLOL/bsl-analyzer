//! Behavioural tests for `infer_module_code_query` (Phase O.14).
//!
//! O.14 ships the per-file module-code Salsa primitive that O.16's
//! `infer_query` wrapper will consume to skip re-walking the
//! module-code body. Narrow callers (completion in module scope,
//! `narrow_query` with `ModuleCode` owner) reach the same data via
//! [`InferOwnerResult::ModuleCode`] in later slices.
//!
//! These tests pin three properties of the query in isolation, before
//! O.17 wires narrow callers through `db.infer_module_code`:
//!
//! - empty-body contract — a file with no module-code body returns a
//!   default result (empty maps, owner = `ModuleCode`);
//! - populated module-level state — a `Перем` declaration with an
//!   explicit literal assignment shows up in `var_types`;
//! - Salsa cache hit identity — two consecutive calls inside the same
//!   revision share the same `Arc`.

use std::sync::Arc;

use hir::{infer_module_code_query, Builders, DefWithBodyId, HirDatabase};
use ide_db::base_db::{FileIdInput, SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use test_fixture::Fixture;
use vfs::FileId;

fn setup(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    let fixture = Fixture::parse(fixture_text);
    let mut db = RootDatabaseImpl::new();
    let mut file_set = vfs::FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
        db.set_file_text(*file_id, &file.content);
    }
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    for file_id in fixture.files.keys() {
        db.set_file_source_root(*file_id, SourceRootId(0));
    }
    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    (db, test_file)
}

/// File with no module-code body (only a method declaration) — the
/// query returns a defaulted result. Critical for callers that need a
/// uniform "always returns Arc" contract.
#[test]
fn no_module_code_body_returns_default() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Функция F()
    Возврат "x";
КонецФункции
"#,
    );
    let input = FileIdInput::new(&db, fid);
    let result = infer_module_code_query(&db, input);
    assert_eq!(result.owner, DefWithBodyId::ModuleCode);
    assert!(result.var_types.is_empty(), "no module-code body → empty var_types");
    assert!(result.expr_types.is_empty(), "no module-code body → empty expr_types");
    assert!(result.diagnostics.is_empty());
}

/// Module-level implicit local — bare `Х = "hello"` (no `Перем`)
/// — surfaces in `var_types`. Mirrors the slice of `infer_query`'s
/// file-wide aggregate that comes from the module-code body. NB:
/// module-level `Перем X;` declarations intentionally stay
/// `Ty::Unknown` per current semantics (see PLAN-v3 §4.4), so this
/// test uses an implicit-local assignment instead.
#[test]
fn module_level_implicit_local_assignment_populates_var_types() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Х = "hello";
"#,
    );
    let input = FileIdInput::new(&db, fid);
    let result = infer_module_code_query(&db, input);
    assert_eq!(result.owner, DefWithBodyId::ModuleCode);
    assert_eq!(result.var_types.get("х").copied(), Some(db.string(None, false)),);
}

/// Two consecutive calls inside the same Salsa revision return the
/// SAME `Arc`. Confirms the `#[salsa::tracked]` cache wiring landed —
/// without it, every IDE interaction would pay for a full
/// module-code body walk.
#[test]
fn salsa_cache_hit_shares_arc() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Перем Г;
Г = 42;
"#,
    );
    let input = FileIdInput::new(&db, fid);
    let r1 = infer_module_code_query(&db, input);
    let r2 = infer_module_code_query(&db, input);
    assert!(Arc::ptr_eq(&r1, &r2), "second call within the same revision must hit the Salsa cache");
}

/// Smoke: the query body must call `module_bodies` (cycle-free) and
/// not panic on a bilingual mixed-case identifier. Asserts the
/// implicit-locals map contains the lowercase key — the same shape
/// completion will read after O.17.
#[test]
fn bilingual_implicit_local_shows_in_implicit_locals() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
ИмяРеквизита = "Наименование";
"#,
    );
    let input = FileIdInput::new(&db, fid);
    let result = infer_module_code_query(&db, input);
    assert!(
        result.implicit_locals.contains_key("имяреквизита"),
        "module-code implicit local must be tracked under its lowercase key (got: {:?})",
        result.implicit_locals.keys().collect::<Vec<_>>()
    );
}

/// `db.infer_module_code(file_id)` (the trait delegator wired in
/// `RootDatabaseImpl`) must return the same `Arc` as the direct
/// `infer_module_code_query(db, FileIdInput::new(db, file_id))` call.
/// Pins the trait-method wiring against a future divergence.
#[test]
fn trait_method_delegates_to_query() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Перем А;
А = Истина;
"#,
    );
    let via_trait = db.infer_module_code(fid);
    let via_query = infer_module_code_query(&db, FileIdInput::new(&db, fid));
    assert!(
        Arc::ptr_eq(&via_trait, &via_query),
        "RootDatabaseImpl::infer_module_code must delegate to the query without an extra layer"
    );
}
