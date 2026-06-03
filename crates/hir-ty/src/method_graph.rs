use std::sync::Arc;

use bsl_types::builders::Builders;
use bsl_types::kind::TypeId;
use hir_def::MethodIdInput;

use crate::db::HirDatabase;
use crate::infer::{BodyInferenceResult, InferenceContext};

#[salsa::tracked(
    lru = 16384,
    cycle_fn = method_return_type_cycle,
    cycle_initial = method_return_type_initial,
)]
pub fn method_return_type_query<'db>(
    db: &'db dyn HirDatabase,
    method: MethodIdInput<'db>,
) -> TypeId {
    let mid = method.method_id(db);
    let _span = tracing::info_span!(
        "method_return_type",
        file_id = mid.module.file_id.0,
        local_id = mid.local_id,
    )
    .entered();

    let body = db.method_body(method);
    let mut ctx = InferenceContext::new_for_method(db, method, &body);
    ctx.infer_all();
    let result = ctx.finish();

    let unknown = db.unknown();
    let return_tys: Vec<TypeId> = result
        .return_expr_ids
        .iter()
        .filter_map(|id| result.expr_types.get(id).copied())
        .filter(|tid| *tid != unknown)
        .collect();

    let ty = if return_tys.is_empty() { unknown } else { db.union(return_tys) };

    tracing::debug!(
        ?mid,
        ?ty,
        return_exprs = result.return_expr_ids.len(),
        "method_return_type complete",
    );
    ty
}

#[allow(
    clippy::needless_lifetimes,
    reason = "Salsa callback signature requires explicit lifetimes"
)]
pub fn method_return_type_initial<'db>(
    db: &'db dyn HirDatabase,
    _id: salsa::Id,
    _method: MethodIdInput<'db>,
) -> TypeId {
    db.unknown()
}

#[allow(
    clippy::needless_lifetimes,
    reason = "Salsa callback signature requires explicit lifetimes"
)]
pub fn method_return_type_cycle<'db>(
    db: &'db dyn HirDatabase,
    _cycle: &salsa::Cycle,
    last_provisional: &TypeId,
    value: TypeId,
    _method: MethodIdInput<'db>,
) -> TypeId {
    let unknown = db.unknown();
    match (*last_provisional, value) {
        (_, v) if v == unknown => *last_provisional,
        (l, _) if l == unknown => value,
        (l, v) if l == v => value,
        (l, v) => db.union(vec![l, v]),
    }
}

#[salsa::tracked(lru = 16384)]
pub fn infer_method_query<'db>(
    db: &'db dyn HirDatabase,
    method: MethodIdInput<'db>,
) -> Arc<BodyInferenceResult> {
    let mid = method.method_id(db);
    let _span = tracing::info_span!(
        "infer_method",
        file_id = mid.module.file_id.0,
        local_id = mid.local_id,
    )
    .entered();

    let body = db.method_body(method);
    let mut ctx = InferenceContext::new_for_method(db, method, &body);
    ctx.infer_all();
    Arc::new(ctx.finish())
}
