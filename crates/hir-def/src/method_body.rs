//! Per-method lowering, keyed by the method and fed by its detached syntax.
//!
//! The chain `method_syntax → method_lower → method_body → infer_method`
//! only pays off while every link is retained: an evicted link has no old
//! value to compare against, so its dependents re-run as if it had changed.
//! The caps therefore never shrink down the chain — each is at least the cap
//! of the link below it (`infer_method` holds 8192).

use std::sync::Arc;

use crate::body::{lower_method_with_externals, Body, LowerResult};
use crate::method_syntax::method_syntax_query;
use crate::{DefDatabase, MethodIdInput};

#[salsa::tracked(lru = 8192, heap_size = crate::body::lower_result_heap, returns(ref))]
pub fn method_lower_query<'db>(
    db: &'db dyn DefDatabase,
    method: MethodIdInput<'db>,
) -> Option<Arc<LowerResult>> {
    let mid = method.method_id(db);
    let _span = tracing::info_span!(
        "method_lower_query",
        file_id = mid.module.file_id.0,
        local_id = ?mid.local_id
    )
    .entered();

    let Some(syntax) = method_syntax_query(db, method) else {
        tracing::warn!(?mid, "method_lower_query: no method under this key");
        return None;
    };
    Some(Arc::new(lower_detached_method(&syntax.detached_root(), syntax.is_function())))
}

/// Lower a method from a tree rooted at the method itself. The line index is
/// the method's own, so line-dependent lowering (method size, one statement
/// per line) sees the same lines whether the method is lowered here or as
/// part of the whole file.
pub fn lower_detached_method(root: &syntax::SyntaxNode, is_function: bool) -> LowerResult {
    let line_index = Arc::new(line_index::LineIndex::new(&root.text().to_string()));
    lower_method_with_externals(root, is_function, Some(line_index))
}

#[salsa::tracked(lru = 8192, heap_size = crate::body::body_arc_heap, returns(ref))]
pub fn method_body_query<'db>(db: &'db dyn DefDatabase, method: MethodIdInput<'db>) -> Arc<Body> {
    match method_lower_query(db, method) {
        Some(lowered) => Arc::clone(&lowered.body),
        None => Arc::new(Body::default()),
    }
}

/// Retention caps of the two lowering memos; see `set_lowering_lru_sweep_mode`.
pub(crate) fn set_lru_capacity(db: &mut dyn DefDatabase, cap: usize) {
    method_lower_query::set_lru_capacity(db, cap);
    method_body_query::set_lru_capacity(db, cap);
}
