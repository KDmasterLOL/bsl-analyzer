//! Per-method body lowering (Phase O.8 — replant of Phase J.4).
//!
//! Salsa-tracked query keyed by [`MethodIdInput`] that returns the
//! lowered [`Body`] for a single method. This is the first method-graph
//! query in the Phase O Phase J replay — upstream of
//! `method_return_type_query` (O.10) and the cascade-typed inference
//! path (O.11).
//!
//! # Residency contract
//!
//! Phase O ships the total-VFS invariant (commit `6c578f3a`): every
//! BSL `FileId` registered in a `FileSet` has a populated
//! `FileTextInput`. This query therefore omits the J.4 sentinel-on-cold
//! gate (`if !db.file_resident(file_id) { return Body::default(); }`);
//! tracked text reads inside `db.parse(file_id)` panic by Salsa
//! contract if the invariant is violated.
//!
//! The two graceful-degradation branches that remain are belt-and-
//! suspenders for *symbol-tree / parse mismatches* (the `MethodId`
//! refers to a node that no longer exists in the current parse — e.g.
//! after an unsynchronised edit) and return an empty `Body` so callers
//! observe "no statements" rather than panic.
//!
//! Diagnostics produced by `lower_method_with_externals` are dropped
//! here intentionally. O.8 keeps the query narrow; a co-located
//! `method_body_diagnostics_query` is deferred until a downstream
//! diagnostics migration needs it.

use std::sync::Arc;

use base_db::FileIdInput;

use crate::body::{lower_method_with_externals, Body};
use crate::{DefDatabase, MethodIdInput};

/// Lazy per-method body lowering.
///
/// See module docs for the residency contract and rationale.
#[salsa::tracked(lru = 4096)]
pub fn method_body_query<'db>(db: &'db dyn DefDatabase, method: MethodIdInput<'db>) -> Arc<Body> {
    let mid = method.method_id(db);
    let file_id = mid.module.file_id;

    let _span =
        tracing::info_span!("method_body_query", file_id = file_id.0, local_id = mid.local_id)
            .entered();

    // Locate the method's PROCEDURE_DEF / FUNCTION_DEF node via the
    // symbol tree. `find_method_by_id` is O(methods-per-file) — fine
    // for typical BSL modules (~30 methods).
    let file_id_input = FileIdInput::new(db, file_id);
    let symbol_tree = crate::symbol_tree::symbol_tree_query(db, file_id_input);
    let Some(method_symbol) = symbol_tree.find_method_by_id(mid) else {
        tracing::warn!(?mid, "method_body_query: MethodId not found in symbol tree");
        return Arc::new(Body::default());
    };

    let parse = db.parse(file_id);
    let Some(method_node) = method_symbol.syntax_node(&parse) else {
        // Source moved on between SymbolTree construction and this
        // query — caller's invariant violation, treat as empty.
        tracing::warn!(?mid, "method_body_query: syntax node not found at recorded range");
        return Arc::new(Body::default());
    };

    let result = lower_method_with_externals(&method_node, method_symbol.is_function, None);
    Arc::new(result.body)
}
