//! Per-method graph queries (Phase O Phase J replay).
//!
//! Hosts the cascade-typing primitives that drive `method_return_type_query`
//! and (in later commits) the bare same-module fn-call wiring inside
//! `dispatch_bare_ident_field_call`. Each query is keyed by
//! [`MethodIdInput`] and consumes the lazy per-method body
//! lowering shipped in O.8.

use hir_def::ty::Ty;
use hir_def::MethodIdInput;

use crate::db::HirDatabase;
use crate::infer::InferenceContext;

// ============================================================================
// Phase O.10 — per-method return-type inference (cascade-typing primitive)
// ============================================================================

/// Salsa-tracked: infer the return type of a single method body.
///
/// Walks the body via the O.7 `InferenceContext::new_for_method`
/// primitive, then unions the inferred types of every
/// `Stmt::Return { value: Some(_) }` statement reached during
/// inference. Diagnostics emitted by per-method inference are
/// discarded — `infer_query` is the single source of truth for
/// client-visible diagnostics.
///
/// # Residency
///
/// Phase O total-VFS invariant (`6c578f3a`) guarantees that any BSL
/// fid registered in a `FileSet` has populated `FileTextInput`. There
/// is no `file_resident` gate here — tracked text reads inside
/// `db.parse(file_id)` panic by Salsa contract if the invariant is
/// violated. The two `Body::default()` fallback branches inside
/// `method_body_query` (O.8) act as belt-and-suspenders for symbol-
/// tree / parse mismatches; this query degrades to `Ty::Unknown` on
/// those mismatches by virtue of seeing an empty body (no return
/// statements → empty `return_expr_ids` → `Ty::Unknown`).
///
/// # Cycle handling
///
/// Self-recursion (`Функция M() Возврат M() КонецФункции`) and mutual
/// recursion are cycle-safe via salsa 0.26's `cycle_fn` /
/// `cycle_initial`. The initial value is `Ty::Unknown` (lattice
/// bottom); the cycle step is a monotone-growing lattice merge —
/// once a specific `Ty` is computed for a method, the cycle handler
/// refuses to demote it back to `Unknown`. Convergence is bounded by
/// the lattice height.
///
/// Phase O.10 ships these handlers as scaffolding — the cascade
/// wiring that recursively calls this query (`dispatch_bare_ident_field_call`
/// gate-3) lands in O.11.
///
/// # LRU
///
/// `lru = 16384` — sized for ERP-scale workspaces (~750k methods)
/// with the planned enrichment gate keeping populated cell counts
/// bounded. Dial down only if smoke proves memory pressure.
///
/// # Production callers (planned)
///
/// O.11 (cascade typing — bare same-module fn-call wiring +
/// `materialise_signature_enriched`). O.10 itself ships the query
/// alone with test coverage; no production caller exists yet.
#[salsa::tracked(
    lru = 16384,
    cycle_fn = method_return_type_cycle,
    cycle_initial = method_return_type_initial,
)]
pub fn method_return_type_query<'db>(db: &'db dyn HirDatabase, method: MethodIdInput<'db>) -> Ty {
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

    // Preserve precision via `Ty::union`. The smart constructor at
    // `crates/hir-def/src/ty.rs:1005-1017` flattens nested unions,
    // sorts (Ty: Ord), dedupes, and collapses singletons. Empty input
    // collapses to `Ty::Unknown`.
    //
    // Explicit absence semantics: a missing entry in `expr_types`
    // contributes `Ty::Unknown`; we filter Unknown entries before
    // unioning so the union does not accidentally collapse to Unknown
    // (e.g. a function with `Возврат X;` where X did not infer to a
    // concrete type).
    let return_tys: Vec<Ty> = result
        .return_expr_ids
        .iter()
        .map(|id| result.expr_types.get(id).cloned().unwrap_or(Ty::Unknown))
        .filter(|t| !matches!(t, Ty::Unknown))
        .collect();

    let ty = if return_tys.is_empty() { Ty::Unknown } else { Ty::union(return_tys) };

    tracing::debug!(
        ?mid,
        ?ty,
        return_exprs = result.return_expr_ids.len(),
        "method_return_type complete",
    );
    ty
}

/// Cycle-recovery seed for [`method_return_type_query`]. Lattice
/// bottom is `Ty::Unknown` — the cycle iteration ascends from there
/// to the body-inferred type on subsequent iterations.
#[allow(clippy::needless_lifetimes)] // Salsa attr requires explicit signature
pub fn method_return_type_initial<'db>(
    _db: &'db dyn HirDatabase,
    _id: salsa::Id,
    _method: MethodIdInput<'db>,
) -> Ty {
    Ty::Unknown
}

/// Cycle-iteration step for [`method_return_type_query`].
///
/// **Job: monotone-growing lattice merge.** The lattice is the union
/// semilattice over `Ty`; ⊥ = `Ty::Unknown`. Joining must be
/// commutative, idempotent, and never lose precision so salsa can
/// detect convergence via structural equality.
///
/// Cases:
/// * `value == Ty::Unknown`            → keep `last_provisional`
///   (no information demotion: ⊥ ⊔ x = x).
/// * `last_provisional == Ty::Unknown` → adopt `value` (same rule
///   from the other side: x ⊔ ⊥ = x).
/// * `last_provisional == value`       → fixed point reached.
/// * **distinct concrete Tys**         → `Ty::union(vec![..])`. This
///   prevents oscillation between two concrete provisionals (e.g.
///   `Ty::String` ↔ `Ty::Number` flipping between cycle iterations);
///   the union strictly grows the type lattice so the next iteration
///   either matches it (converged) or grows further.
///
/// `Ty::union` (`crates/hir-def/src/ty.rs:1005-1021`) flattens nested
/// unions, sorts via the structural `Ord` derive, dedupes, and
/// collapses singletons — output is deterministic across iterations,
/// which is what salsa needs to recognise a fixed point.
#[allow(clippy::needless_lifetimes)] // Salsa attr requires explicit signature
pub fn method_return_type_cycle<'db>(
    _db: &'db dyn HirDatabase,
    _cycle: &salsa::Cycle,
    last_provisional: &Ty,
    value: Ty,
    _method: MethodIdInput<'db>,
) -> Ty {
    match (last_provisional, &value) {
        (_, Ty::Unknown) => last_provisional.clone(),
        (Ty::Unknown, _) => value,
        (last, v) if last == v => value,
        (last, _) => Ty::union(vec![last.clone(), value]),
    }
}
