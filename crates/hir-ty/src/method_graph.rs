//! Per-method graph queries (Phase O Phase J replay).
//!
//! Hosts the cascade-typing primitives that drive `method_return_type_query`
//! and (in later commits) the bare same-module fn-call wiring inside
//! `dispatch_bare_ident_field_call`. Each query is keyed by
//! [`MethodIdInput`] and consumes the lazy per-method body
//! lowering shipped in O.8.

use std::sync::Arc;

use bsl_types::builders::Builders;
use bsl_types::kind::TypeId;
use hir_def::MethodIdInput;

use crate::db::HirDatabase;
use crate::infer::{BodyInferenceResult, InferenceContext};

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

    // Preserve precision via `db.union`, the kernel intern gateway:
    // `canonicalise_union` flattens nested unions, dedupes, sorts by
    // `TypeId::raw()`, and collapses singletons. Empty input → `db.unknown()`.
    //
    // Explicit absence semantics: a missing entry in `expr_types` (or a
    // present `db.unknown()` one) contributes Unknown; we drop those before
    // unioning so the union does not collapse to Unknown (e.g. a function
    // with `Возврат X;` where X did not infer to a concrete type). Since
    // §4.D, `expr_types` already stores `TypeId` — no bridge needed.
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

/// Cycle-recovery seed for [`method_return_type_query`]. Lattice
/// bottom is `db.unknown()` — the cycle iteration ascends from there
/// to the body-inferred type on subsequent iterations.
#[allow(clippy::needless_lifetimes)] // Salsa attr requires explicit signature
pub fn method_return_type_initial<'db>(
    db: &'db dyn HirDatabase,
    _id: salsa::Id,
    _method: MethodIdInput<'db>,
) -> TypeId {
    db.unknown()
}

/// Cycle-iteration step for [`method_return_type_query`].
///
/// **Job: monotone-growing lattice merge.** The lattice is the union
/// semilattice over `Ty`; ⊥ = `Ty::Unknown`. Joining must be
/// commutative, idempotent, and never lose precision so salsa can
/// detect convergence via structural equality.
///
/// Cases:
/// * `value == db.unknown()`            → keep `last_provisional`
///   (no information demotion: ⊥ ⊔ x = x).
/// * `last_provisional == db.unknown()` → adopt `value` (same rule
///   from the other side: x ⊔ ⊥ = x).
/// * `last_provisional == value`        → fixed point reached.
/// * **distinct concrete TypeIds**      → `db.union(vec![..])`. This
///   prevents oscillation between two concrete provisionals (e.g.
///   `Число` ↔ `Строка` flipping between cycle iterations); the union
///   strictly grows the type lattice so the next iteration either
///   matches it (converged) or grows further.
///
/// `db.union` (kernel `canonicalise_union`) flattens nested unions,
/// dedupes, sorts by `TypeId::raw()`, and collapses singletons — output
/// is a deterministic interned `TypeId` across iterations, which is what
/// salsa needs to recognise a fixed point (cheap `TypeId` equality).
/// Unknown never enters `db.union` here (the explicit cases above handle
/// it), so the kernel's Unknown-absorption rule does not perturb the
/// fixpoint.
#[allow(clippy::needless_lifetimes)] // Salsa attr requires explicit signature
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

// ============================================================================
// Phase O.15 — per-method inference primitive (narrow-infer Lni.5 surface)
// ============================================================================

/// Salsa-tracked: per-method inference output.
///
/// Returns the `BodyInferenceResult` for a single method body, keyed
/// on the salsa-interned [`MethodIdInput`]. Narrow callers (hover,
/// completion, `narrow_query`, `type_of_expr_query`) route through
/// [`InferOwnerResult::Method`] (`hir-ty::infer`) and read this
/// directly, bypassing the file-wide `infer_query` aggregate. After
/// O.16's wrapper rewrite, `infer_query` itself fans out to this
/// query per body and folds the results.
///
/// # Body source
///
/// `db.method_body(method)` (Phase O.8) supplies the per-method body.
/// The `Body::default()` fallback branches inside `method_body_query`
/// — fired only on a symbol-tree / parse mismatch — yield an empty
/// body, and running `InferenceContext::new_for_method + infer_all`
/// on that empty body produces the same empty payload that
/// [`BodyInferenceResult::empty_for`] returns. No explicit miss check
/// needed here.
///
/// # Residency
///
/// Phase O total-VFS invariant (`6c578f3a`) — no `file_resident` gate.
/// Tracked text reads inside `db.parse(...)` panic by Salsa contract
/// if the invariant is violated upstream of this query.
///
/// # Cycle safety
///
/// `infer_method` calls `db.method_body` (cycle-free),
/// `method_return_type_query` (cycle-safe via the cycle handlers
/// above), and `proc_signature_query` (cycle-free per
/// `proc_signature.rs:110-118`). It does **not** call
/// `infer_module_code` (no module-code seed by design) or
/// `infer_query`. Recursive method calls flow through
/// `method_return_type_query`'s cycle handlers, never through
/// `infer_method` itself, so this query needs no cycle attributes.
///
/// # LRU
///
/// `lru = 16384` — matches [`method_return_type_query`] so the two
/// per-method tables age in lock-step. PLAN-v3 R3 / Codex Round 1 C1
/// rationale: ERP-scale workspaces (~750k methods) need a working-set
/// LRU; Phase L's Lni.11 measurement gate will adjust this with
/// per-query event counters.
///
/// # Production callers (planned)
///
/// O.16 (`infer_query` thin wrapper fans out per method); O.17
/// (`Semantics::type_of_expr`, `narrow_query`, `type_of_expr_query`,
/// `add_implicit_locals`, `symbol_for_body_local` migrate to route
/// through [`InferOwnerResult::Method`] which calls this query).
/// O.15 itself ships the query alone with test coverage; no
/// production caller exists yet.
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
