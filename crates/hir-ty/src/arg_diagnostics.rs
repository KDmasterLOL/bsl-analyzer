//! Narrowing-aware argument-type diagnostic query.
//!
//! Inference (`crate::infer::infer_query`) only **records** call-site
//! `(args, params)` pairs into [`crate::infer::InferenceResult::call_arg_bindings`]
//! — it no longer emits [`InferenceDiagnostic::TypeMismatch`] for
//! arguments directly. This module's [`arg_diagnostics_query`] runs the
//! actual mismatch check **after** inference, so it can consult the
//! [`crate::narrow`] overlay for each argument without forcing
//! `infer_query` to depend on `narrow_query` (which would create a
//! Salsa cycle: `narrow → infer → narrow`).
//!
//! ## Dependency graph
//!
//! ```text
//! arg_diagnostics  →  infer  →  module_bodies
//!                  →  narrow  →  infer
//!                            →  module_bodies
//! ```
//!
//! Acyclic: `arg_diagnostics` is a downstream consumer, `narrow` already
//! depends on `infer`, and inference no longer reaches into narrowing.
//!
//! ## Producer-locality
//!
//! Each emitted diagnostic carries the owning [`DefWithBodyId`] so the
//! ide-diagnostics layer can resolve the body-local [`ExprId`] back to a
//! [`syntax::TextRange`] through the right [`hir_def::body::BodySourceMap`]
//! — same pattern as [`InferenceResult::diagnostics`].

use std::sync::Arc;
use std::time::{Duration, Instant};

use hir_def::body::Body;
use hir_def::hir::Expr;
use hir_def::{DefWithBodyId, ExprId, IdConversion, ModuleId};
use vfs::FileId;

use crate::db::HirDatabase;
use crate::infer::{InferenceDiagnostic, ParamsShape};
use crate::narrow::{narrowed_type_at, NarrowState};
use crate::Ty;

/// Salsa query: emit `TypeMismatch` diagnostics for call arguments,
/// applying the narrowing overlay before each per-arg assignability
/// check.
///
/// Iterates the [`InferenceResult::call_arg_bindings`] recorded by
/// `infer_query`, resolves each binding's body, computes
/// `narrow_or_base(arg_id)` once per argument, and runs the
/// per-arg / overloaded acceptance rule that previously lived inside
/// `InferenceContext::emit_arg_type_mismatches[_overloaded]`.
///
/// **Boundary of responsibility:** narrowed types are used **only**
/// to decide whether to emit a `TypeMismatch` (and which overload to
/// blame for the message). They are NOT fed back into `infer_query`'s
/// return-type selection — that stays based on the inference's own
/// (base-type) overload pick. Without this discipline a downstream
/// query could feed narrowing back into inference and trigger
/// surprising re-inference cascades.
///
/// # Caching
///
/// Cached by Salsa. Invalidates when:
/// - `infer(file_id)` invalidates (file edits, dependency changes).
/// - `narrow(file_id, owner)` invalidates for any binding's owner —
///   transitively triggered by the same file/dep edits.
/// - `type_narrowing_enabled()` flips — `narrow_or_base` reads it on
///   every call so the next query observes the new value.
pub fn arg_diagnostics_query(
    db: &dyn HirDatabase,
    file_id: FileId,
) -> Arc<Vec<(DefWithBodyId, InferenceDiagnostic)>> {
    let _span = tracing::info_span!("arg_diagnostics_query", ?file_id).entered();
    let query_start = Instant::now();

    let infer = db.infer(file_id);
    if infer.call_arg_bindings.is_empty() {
        return Arc::new(Vec::new());
    }

    let module_bodies = db.module_bodies(ModuleId { file_id });
    let narrowing_enabled = db.type_narrowing_enabled();

    let mut out: Vec<(DefWithBodyId, InferenceDiagnostic)> = Vec::new();

    // Reuse the narrowing overlay across consecutive bindings whose
    // owner matches: bindings are appended to `call_arg_bindings` in
    // body order, so a body's bindings are contiguous, and `db.narrow`
    // returns the same `Arc` for every arg of that body.
    //
    // Without this cache, an N-arg call site triggers N independent
    // Salsa lookups for `narrow(file_id, owner)` + N feature-flag reads
    // even though both produce the same value. Hot bodies have dozens
    // of call sites with several args each — the redundant lookups
    // dominated the diff in `arg_diagnostics_query`'s self-time.
    let mut cached_owner: Option<DefWithBodyId> = None;
    let mut cached_narrow: Option<Arc<dataflow::DataflowResult<NarrowState>>> = None;

    // Per-owner accounting for the slow-query profiler. Bindings are
    // appended in body order (see `infer_query`), so each owner's
    // bindings are contiguous; close out the previous owner whenever
    // the loop sees a new one. Cost recorded is wall time inside the
    // loop body, which captures per-owner narrow lookup + per-binding
    // assignability work.
    let mut owner_stats: Vec<OwnerStat> = Vec::new();
    let mut current: Option<OwnerInProgress> = None;

    for binding in &infer.call_arg_bindings {
        let body: &Body = match resolve_body(&module_bodies, binding.owner) {
            Some(body) => body,
            None => continue,
        };

        if cached_owner != Some(binding.owner) {
            if let Some(prev) = current.take() {
                owner_stats.push(prev.finish());
            }
            current = Some(OwnerInProgress::new(binding.owner));

            cached_owner = Some(binding.owner);
            cached_narrow =
                if narrowing_enabled { db.narrow(file_id, binding.owner) } else { None };
        }
        if let Some(state) = current.as_mut() {
            state.bindings += 1;
            state.args += binding.args.len();
        }
        let narrow = cached_narrow.as_deref();

        let arg_types: Vec<Ty> = binding
            .args
            .iter()
            .map(|arg_id| {
                let base =
                    infer.type_of_expr_in(binding.owner, *arg_id).cloned().unwrap_or(Ty::Unknown);
                narrow_arg(narrow, body, *arg_id, base)
            })
            .collect();

        match &binding.params {
            ParamsShape::Single(params) => {
                emit_single(&binding.args, &arg_types, params, &mut out, binding.owner)
            }
            ParamsShape::Overloaded { flat, overloads } => {
                emit_overloaded(&binding.args, &arg_types, flat, overloads, &mut out, binding.owner)
            }
        }
    }

    if let Some(prev) = current.take() {
        owner_stats.push(prev.finish());
    }

    log_owner_stats(query_start.elapsed(), &infer.call_arg_bindings, &out, &mut owner_stats);

    Arc::new(out)
}

/// Per-owner timing accumulator. `start` is sampled at the iteration
/// that opens this owner's segment; `finish()` snapshots the elapsed
/// at the iteration that opens the *next* owner's segment.
///
/// **Caveats** (acceptable for one-shot profiling, not for hard SLOs):
/// - Boundary bleed: the snapshot happens *after* `resolve_body()` has
///   already run for the next owner's first binding, so a few µs of
///   the successor leak into this owner's elapsed.
/// - Owner contiguity: if `infer.call_arg_bindings` ever interleaves
///   owners (today it doesn't — `infer_query` appends in body order),
///   the same owner produces multiple `OwnerStat` segments and ranks
///   lower than its true cost in the top-K sort.
struct OwnerInProgress {
    owner: DefWithBodyId,
    start: Instant,
    bindings: usize,
    args: usize,
}

impl OwnerInProgress {
    fn new(owner: DefWithBodyId) -> Self {
        Self { owner, start: Instant::now(), bindings: 0, args: 0 }
    }

    fn finish(self) -> OwnerStat {
        OwnerStat {
            owner: self.owner,
            elapsed: self.start.elapsed(),
            bindings: self.bindings,
            args: self.args,
        }
    }
}

struct OwnerStat {
    owner: DefWithBodyId,
    elapsed: Duration,
    bindings: usize,
    args: usize,
}

/// Emit a per-query summary + top-5 hottest owners when the whole
/// `arg_diagnostics_query` ran longer than the profiling threshold.
///
/// The threshold gates noise: most files finish in milliseconds, but a
/// handful of "hot" files (e.g. `ОбщегоНазначения`) burn tens of
/// seconds and dominate first-open latency — those are the ones we
/// need to see in the log without drowning every other call.
fn log_owner_stats(
    total_elapsed: Duration,
    bindings: &[crate::infer::CallArgBinding],
    out: &[(DefWithBodyId, InferenceDiagnostic)],
    owner_stats: &mut [OwnerStat],
) {
    const SLOW_THRESHOLD: Duration = Duration::from_millis(500);
    if total_elapsed < SLOW_THRESHOLD {
        return;
    }

    owner_stats.sort_by_key(|s| std::cmp::Reverse(s.elapsed));
    tracing::info!(
        total_elapsed_ms = total_elapsed.as_millis() as u64,
        total_bindings = bindings.len(),
        total_owners = owner_stats.len(),
        diagnostics = out.len(),
        "arg_diagnostics_query summary",
    );
    for (rank, stat) in owner_stats.iter().take(5).enumerate() {
        tracing::info!(
            rank = rank + 1,
            owner = ?stat.owner,
            elapsed_ms = stat.elapsed.as_millis() as u64,
            bindings = stat.bindings,
            args = stat.args,
            "arg_diagnostics_query top owner",
        );
    }
}

/// Apply the narrowing overlay to a single argument expression.
///
/// Equivalent to [`crate::narrow::narrow_or_base`] but takes the
/// pre-fetched [`dataflow::DataflowResult`] so the caller can hoist
/// the `db.narrow(...)` Salsa lookup out of the per-argument loop.
/// `narrow_or_base` itself remains the right entry point for one-shot
/// callers (`Semantics::type_of_expr`); the bulk caller in
/// `arg_diagnostics_query` would otherwise re-pay the lookup cost
/// once per arg.
fn narrow_arg(
    narrow: Option<&dataflow::DataflowResult<NarrowState>>,
    body: &Body,
    expr_id: ExprId,
    base: Ty,
) -> Ty {
    let Some(result) = narrow else {
        return base;
    };
    let Expr::Path(name) = body.expr(expr_id) else {
        return base;
    };
    match narrowed_type_at(result, expr_id.to_idx(), name) {
        Some(narrowed) if !matches!(narrowed, Ty::Unknown) => narrowed,
        _ => base,
    }
}

/// Resolve the [`Body`] for a recorded binding's owner.
///
/// Returns `None` for stale owners that no longer resolve to a body in
/// `module_bodies` — this can happen if a binding survived an
/// invalidation race (mostly impossible since both `infer_query` and
/// `arg_diagnostics_query` see the same Salsa revision, but the
/// defensive `None` matches the `narrow_query` shape).
fn resolve_body(module_bodies: &hir_def::ModuleBodies, owner: DefWithBodyId) -> Option<&Body> {
    match owner {
        DefWithBodyId::ModuleCode => module_bodies.module_code(),
        DefWithBodyId::Method(local_id) => module_bodies.body(local_id),
    }
}

/// Single-signature path. Mirrors the legacy
/// `InferenceContext::emit_arg_type_mismatches`: per-pair
/// `is_assignable` against the narrowed `arg_types[i]`.
///
/// Walks `min(args.len(), params.len())` so an unpaired tail (caught
/// separately by `MismatchedArgCount`) doesn't double-fire.
fn emit_single(
    args: &[ExprId],
    arg_types: &[Ty],
    params: &[Ty],
    out: &mut Vec<(DefWithBodyId, InferenceDiagnostic)>,
    owner: DefWithBodyId,
) {
    for ((arg_id, arg_ty), param_ty) in args.iter().zip(arg_types.iter()).zip(params.iter()) {
        if !crate::subtype::is_assignable(arg_ty, param_ty) {
            out.push((
                owner,
                InferenceDiagnostic::TypeMismatch {
                    expr: *arg_id,
                    expected: param_ty.clone(),
                    actual: arg_ty.clone(),
                },
            ));
        }
    }
}

/// Multi-overload path. Mirrors the legacy
/// `InferenceContext::emit_arg_type_mismatches_overloaded`:
/// silently accept iff *any* declared overload's per-arg
/// `is_assignable` check passes (and that overload has enough slots
/// for the supplied args). When no overload accepts, blame the one
/// whose declared arity is closest to `args.len()` — falling back to
/// `flat` if the overload set is empty.
///
/// All comparisons run against the **narrowed** `arg_types`; mixing
/// narrowed and base types here would split the contract between
/// "what's accepted" and "what the message says is wrong" and reopen
/// the false-positive bug this query exists to fix.
fn emit_overloaded(
    args: &[ExprId],
    arg_types: &[Ty],
    flat: &[Ty],
    overloads: &[Arc<[Ty]>],
    out: &mut Vec<(DefWithBodyId, InferenceDiagnostic)>,
    owner: DefWithBodyId,
) {
    if overloads.is_empty() {
        emit_single(args, arg_types, flat, out, owner);
        return;
    }

    let any_accepts = overloads.iter().any(|params| {
        if args.len() > params.len() {
            return false;
        }
        arg_types.iter().zip(params.iter()).all(|(a, p)| crate::subtype::is_assignable(a, p))
    });
    if any_accepts {
        return;
    }

    let chosen: &[Ty] = overloads
        .iter()
        .min_by_key(|params| params.len().abs_diff(args.len()))
        .map(|p| p.as_ref())
        .unwrap_or(flat);
    emit_single(args, arg_types, chosen, out, owner);
}
