//! Behavioural tests for `method_body_with_source_map_query` (Phase O.9).
//!
//! Sister of [`method_body_query`] (O.8): returns the lowered `Body`
//! together with its `BodySourceMap` so IDE features can map offsets
//! ↔ HIR nodes (hover, narrow infer, diagnostics). Same residency
//! contract as O.8 (no `file_resident` gate; total-VFS invariant
//! O.2 guarantees populated text).

use hir::{DefDatabase, MethodIdInput, ModuleId, Name};
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

/// The pair query yields a body matching the file-level `module_bodies`
/// slice for the same method, and the accompanying `BodySourceMap`
/// agrees with the file-level source map on a known expression range.
#[test]
fn method_body_with_source_map_matches_module_bodies_slice() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Процедура Foo()
    А = 1;
    Б = 2;
КонецПроцедуры
"#,
    );

    let module_id = ModuleId::new(fid);
    let symbol_tree = db.symbol_tree(module_id);
    let mid = symbol_tree.find_method(&Name::new("Foo")).expect("Foo declared").id;
    let method_input = MethodIdInput::new(&db, mid);

    let pair = db.method_body_with_source_map(method_input);
    let (body, source_map) = &*pair;

    let module_bodies = db.module_bodies(module_id);
    let whole_body = module_bodies.body(mid.local_id).expect("module_bodies has Foo");
    let whole_map = module_bodies.source_map(mid.local_id).expect("module_bodies has source_map");

    assert_eq!(
        body.stmt_count(),
        whole_body.stmt_count(),
        "per-method body must match module_bodies slice"
    );
    assert!(body.expr_count() > 0, "two assignments must yield expressions");

    // Pick a known expression range via the file-level source map's
    // forward (id → range) accessor, then confirm the per-method map's
    // reverse accessor agrees. `whole_body.exprs_iter()` enumerates all
    // expressions actually produced by lowering — no guesses about
    // arena indices or byte offsets.
    let probe =
        whole_body.exprs_iter().find_map(|(id, _)| whole_map.expr_range(id).map(|r| (id, r)));
    if let Some((id, range)) = probe {
        assert_eq!(
            source_map.expr_at_range(range),
            Some(id),
            "per-method source map must agree with module_bodies on reverse-range lookup"
        );
    }
}

/// Salsa cache: a second call returns the same `Arc` payload pointer.
#[test]
fn method_body_with_source_map_caches_via_salsa() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Процедура Тест()
    А = 1;
КонецПроцедуры
"#,
    );

    let module_id = ModuleId::new(fid);
    let symbol_tree = db.symbol_tree(module_id);
    let mid = symbol_tree.find_method(&Name::new("Тест")).expect("Тест declared").id;
    let method_input = MethodIdInput::new(&db, mid);

    let first = db.method_body_with_source_map(method_input);
    let second = db.method_body_with_source_map(method_input);
    assert!(
        Arc::ptr_eq(&first, &second),
        "salsa-cached pair query must return the same Arc on a cache hit"
    );
}
