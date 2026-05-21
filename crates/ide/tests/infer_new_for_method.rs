//! Behavioural test for `InferenceContext::new_for_method` (Phase O.7).
//!
//! O.7 ships the per-method inference constructor alone; the cascade
//! query (`method_return_type_query`) and its callers land in later
//! commits (O.8–O.11). The constructor is a thin wrapper over
//! `InferenceContext::new` that resolves the salsa-interned
//! `MethodIdInput` into `(file_id, DefWithBodyId::Method(local_id))`
//! so callers do not have to thread those fields manually.
//!
//! Coverage here uses the existing `module_bodies_query` (develop
//! surface) to obtain a `&Body` and wraps it in a fresh `Arc<Body>` —
//! mirroring the pattern in `infer_query` itself
//! (`crates/hir-ty/src/infer.rs:3100,3107`). The dedicated
//! `method_body_query` returning `Arc<Body>` lands in O.8.

use hir::{DefDatabase, DefWithBodyId, InferenceContext, MethodIdInput, ModuleId, Name};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::sync::Arc;
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

/// `new_for_method` derives `file_id` and `DefWithBodyId::Method(local_id)`
/// from the salsa-interned `MethodIdInput` and produces a working
/// per-body context that runs `infer_all` + `finish` without invoking
/// `infer_query` / `module_bodies_query` on its own.
#[test]
fn new_for_method_drives_single_body_inference_without_infer_query() {
    let (db, test_fid) = setup(
        r#"
//- /test.bsl
Процедура Foo()
    Х = "привет";
КонецПроцедуры
"#,
    );

    let module_id = ModuleId::new(test_fid);
    let symbol_tree = db.symbol_tree(module_id);
    let mid = symbol_tree.find_method(&Name::new("Foo")).expect("fixture must declare Foo").id;
    let method_input = MethodIdInput::new(&db, mid);

    let module_bodies = db.module_bodies(module_id);
    let body = module_bodies.body(mid.local_id).expect("Foo must have a body");
    let body_arc = Arc::new(body.clone());

    let mut ctx = InferenceContext::new_for_method(&db, method_input, &body_arc);
    ctx.infer_all();
    let result = ctx.finish();

    assert_eq!(
        result.owner,
        DefWithBodyId::Method(mid.local_id),
        "constructor must derive Method(local_id) owner from MethodIdInput"
    );
    assert!(!result.expr_types.is_empty(), "non-empty body must yield expr_types after infer_all");
    assert!(
        result.var_types.keys().any(|k| k.to_lowercase() == "х"),
        "literal string assignment must yield a var_types entry for `Х` (got keys: {:?})",
        result.var_types.keys().collect::<Vec<_>>()
    );
}
