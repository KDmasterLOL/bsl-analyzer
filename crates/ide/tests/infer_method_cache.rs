//! Salsa cache invariants for `infer_method_query`.
//!
//! Pins the per-method cell partitioning that narrow IDE callers
//! (`Semantics::type_of_expr`, `narrow_query`, hover, highlight,
//! goto-def) depend on:
//!
//! - **Cross-method isolation across edits**: editing one method's
//!   body inside a file does not invalidate other methods'
//!   `infer_method` cells. The other cells keep their cached
//!   `Arc<BodyInferenceResult>` verbatim because
//!   `method_body_query`'s output is structurally equal across the
//!   re-lowering, and Salsa short-circuits downstream invalidation
//!   on `Eq`.
//! - **Same-revision identity**: repeated `infer_method` calls within
//!   one revision return the same `Arc` — cursor moves on the same
//!   body pay nothing past the first warm.
//! - **Cross-method independence**: warming one method's cell does
//!   not invalidate any other; querying a sibling cell after the warm
//!   leaves the first cell untouched.
//!
//! `Arc::ptr_eq` is the load-bearing assertion mechanism: if Salsa
//! re-executed `infer_method_query`, the returned Arc would be a
//! fresh allocation, so `ptr_eq` is true if and only if the cell was
//! reused from cache.

use std::sync::Arc;

use hir::{infer_method_query, Builders, DefDatabase, MethodId, MethodIdInput, ModuleId, Name};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use test_fixture::Fixture;
use vfs::FileId;

/// Build a freshly-primed `RootDatabaseImpl` against a single
/// `/test.bsl` file containing two methods (A and B). Returns the
/// populated db, the file_id, and the `MethodId` for each method by
/// name. Uses `test_fixture::Fixture` to allocate the FileId + VfsPath
/// the same way every other ide integration test does.
fn setup_two_method_module(source: &str) -> (RootDatabaseImpl, FileId, MethodId, MethodId) {
    let wrapped = format!("//- /test.bsl\n{source}");
    let fixture = Fixture::parse(&wrapped);
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
    let file_id = *fixture.files.keys().next().expect("fixture must produce one file");

    let symbol_tree = db.symbol_tree(ModuleId::new(file_id));
    let mid_a =
        symbol_tree.find_method(&Name::new("A")).expect("fixture must declare procedure A").id;
    let mid_b =
        symbol_tree.find_method(&Name::new("B")).expect("fixture must declare procedure B").id;
    (db, file_id, mid_a, mid_b)
}

/// The invariant test: editing B does not invalidate A's
/// `infer_method` cell. `Arc::ptr_eq` is the load-bearing assertion —
/// if Salsa re-executed A's query, the returned Arc would be a fresh
/// allocation.
#[test]
fn edit_in_one_method_keeps_other_methods_infer_cell_warm() {
    const SOURCE_BEFORE: &str = r#"
Процедура A()
    Х = "fixed";
КонецПроцедуры

Процедура B()
    Y = 1;
КонецПроцедуры
"#;
    // Edit ONLY B's body — A's text byte range stays untouched.
    const SOURCE_AFTER: &str = r#"
Процедура A()
    Х = "fixed";
КонецПроцедуры

Процедура B()
    Y = 2;
КонецПроцедуры
"#;

    let (mut db, file_id, mid_a, mid_b) = setup_two_method_module(SOURCE_BEFORE);

    // Warm A's and B's `infer_method` cells.
    let a_input = MethodIdInput::new(&db, mid_a);
    let b_input = MethodIdInput::new(&db, mid_b);
    let a_before: Arc<_> = infer_method_query(&db, a_input);
    let b_before: Arc<_> = infer_method_query(&db, b_input);

    // Edit the file — only B's body changes.
    db.set_file_text(file_id, SOURCE_AFTER);

    // Re-query both. A's cell must stay warm; B's must update.
    //
    // We have to re-intern the `MethodIdInput`s against the new
    // revision because `MethodIdInput::new` is salsa-interned per
    // revision. The MethodId payload is the same (same module +
    // local_id), so the new inputs identify the same logical method.
    let a_input2 = MethodIdInput::new(&db, mid_a);
    let b_input2 = MethodIdInput::new(&db, mid_b);
    let a_after: Arc<_> = infer_method_query(&db, a_input2);
    let b_after: Arc<_> = infer_method_query(&db, b_input2);

    // The headline Phase L invariant: A's cell stayed warm.
    //
    // Salsa's tracked-query machinery propagates "no change" via
    // structural equality on outputs. `method_body_query` (O.8)
    // returns `Arc<Body>` whose Body has a PartialEq impl; when
    // A's body is structurally unchanged after re-lowering, Salsa
    // short-circuits and keeps A's downstream `infer_method` cell.
    // The returned Arc is therefore literally the same allocation.
    assert!(
        Arc::ptr_eq(&a_before, &a_after),
        "Editing B's body invalidated A's infer_method cell — Phase L \
         per-method partitioning regression. Pre-Phase L this was \
         expected (file-wide infer_query aggregate); post-Phase L \
         A's cell must stay warm."
    );

    // Sanity: B's cell is allowed to change (we DID edit B). We
    // assert distinct Arcs to confirm the test fixture actually
    // exercises a real edit — if both were ptr_eq, the test would
    // be vacuous because nothing changed.
    assert!(
        !Arc::ptr_eq(&b_before, &b_after),
        "Test fixture broken: B's body was edited but its infer_method \
         cell returned the same Arc — the edit didn't take effect or \
         Salsa skipped re-execution incorrectly."
    );

    let b_after_var = b_after.var_types.get("y").copied();
    assert!(
        matches!(b_after_var, Some(ty) if ty == db.number(None, None)),
        "B's edited body should still infer Y = <Number>; got {b_after_var:?}"
    );
}

/// Cross-revision identity for cursor moves: re-running narrow
/// queries within the same revision (no text edits) returns the same
/// Arc. Confirms the per-owner cell is the right granularity — a
/// cursor move within method A re-uses the same payload.
#[test]
fn repeated_narrow_queries_within_revision_share_arc() {
    const SOURCE: &str = r#"
Процедура A()
    Х = "stable";
КонецПроцедуры

Процедура B()
    Y = 7;
КонецПроцедуры
"#;
    let (db, _file_id, mid_a, _mid_b) = setup_two_method_module(SOURCE);

    let a_input = MethodIdInput::new(&db, mid_a);
    let r1 = infer_method_query(&db, a_input);
    let r2 = infer_method_query(&db, a_input);
    let r3 = infer_method_query(&db, a_input);

    assert!(Arc::ptr_eq(&r1, &r2));
    assert!(Arc::ptr_eq(&r2, &r3));
}

/// Cross-method independence: warming A's cell first does not
/// require subsequent queries to also touch the wrapper. Calling
/// `infer_method(B)` after warming A must hit B's cell directly
/// without re-executing A.
#[test]
fn warming_one_method_does_not_invalidate_other() {
    const SOURCE: &str = r#"
Процедура A()
    Х = 1;
КонецПроцедуры

Процедура B()
    Y = 2;
КонецПроцедуры
"#;
    let (db, _file_id, mid_a, mid_b) = setup_two_method_module(SOURCE);

    let a_input = MethodIdInput::new(&db, mid_a);
    let b_input = MethodIdInput::new(&db, mid_b);

    // Warm A first.
    let a_first = infer_method_query(&db, a_input);
    // Query B (cold cell). Should populate B without affecting A.
    let _b = infer_method_query(&db, b_input);
    // Re-query A — must still be the same Arc as the first warm.
    let a_second = infer_method_query(&db, a_input);

    assert!(
        Arc::ptr_eq(&a_first, &a_second),
        "Querying B's cell invalidated A's cell — cross-method Salsa \
         cache leak"
    );
}
