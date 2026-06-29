use std::sync::Arc;
use std::time::{Duration, Instant};

use hir_def::body::Body;
use hir_def::hir::Expr;
use hir_def::{DefWithBodyId, ExprId, IdConversion, ModuleId};
use vfs::FileId;

use bsl_types::builders::Builders;
use bsl_types::kind::TypeId;

use crate::db::HirDatabase;
use crate::infer::{InferenceDiagnostic, ParamsShape};
use crate::narrow::{narrowed_type_at, NarrowState};

pub fn arg_diagnostics_query(
    db: &dyn HirDatabase,
    file_id: FileId,
) -> Arc<Vec<(DefWithBodyId, InferenceDiagnostic)>> {
    let _span = tracing::info_span!("arg_diagnostics_query", ?file_id).entered();
    let query_start = Instant::now();

    let infer_start = Instant::now();
    let infer = db.infer(file_id);
    let infer_ns = infer_start.elapsed().as_nanos();

    if infer.call_arg_bindings.is_empty() {
        return Arc::new(Vec::new());
    }

    let module_bodies = db.module_bodies(ModuleId { file_id });
    let narrowing_enabled = db.type_narrowing_enabled();

    let mut out: Vec<(DefWithBodyId, InferenceDiagnostic)> = Vec::new();

    let mut cached_owner: Option<DefWithBodyId> = None;
    let mut cached_narrow: Option<Arc<dataflow::DataflowResult<NarrowState>>> = None;
    let mut narrow_attempted: bool = false;
    let mut narrow_skipped_owners: u64 = 0;

    let mut owner_stats: Vec<OwnerStat> = Vec::new();
    let mut current: Option<OwnerInProgress> = None;

    let mut narrow_ns: u128 = 0;
    let mut narrow_arg_ns: u128 = 0;
    let mut emit_ns: u128 = 0;

    for binding in &infer.call_arg_bindings {
        let body: &Body = match resolve_body(&module_bodies, binding.owner) {
            Some(body) => body,
            None => continue,
        };

        if cached_owner != Some(binding.owner) {
            if let Some(prev) = current.take() {
                owner_stats.push(prev.finish());
            }
            if narrowing_enabled && cached_owner.is_some() && !narrow_attempted {
                narrow_skipped_owners += 1;
            }
            current = Some(OwnerInProgress::new(binding.owner));

            cached_owner = Some(binding.owner);
            cached_narrow = None;
            narrow_attempted = false;
        }
        if let Some(state) = current.as_mut() {
            state.bindings += 1;
            state.args += binding.args.len();
        }

        if narrowing_enabled && !narrow_attempted {
            let any_path_arg =
                binding.args.iter().any(|arg_id| matches!(body.expr(*arg_id), Expr::Path(_)));
            if any_path_arg {
                let narrow_start = Instant::now();
                cached_narrow = db.narrow(file_id, binding.owner);
                narrow_ns += narrow_start.elapsed().as_nanos();
                narrow_attempted = true;
            }
        }
        let narrow = cached_narrow.as_deref();

        let narrow_arg_start = Instant::now();
        let arg_types: Vec<TypeId> = binding
            .args
            .iter()
            .map(|arg_id| {
                let base = infer.type_id_of_expr_in(binding.owner, *arg_id).unwrap_or(db.unknown());
                narrow_arg(db, narrow, body, *arg_id, base)
            })
            .collect();
        narrow_arg_ns += narrow_arg_start.elapsed().as_nanos();

        let emit_start = Instant::now();
        let from_doc_comment = binding.params_from_doc_comment;
        match &binding.params {
            ParamsShape::Single(params) => emit_single(
                db,
                &binding.args,
                &arg_types,
                params,
                &mut out,
                binding.owner,
                from_doc_comment,
            ),
            ParamsShape::Overloaded { flat, overloads } => emit_overloaded(
                db,
                &binding.args,
                &arg_types,
                flat,
                overloads,
                &mut out,
                binding.owner,
                from_doc_comment,
            ),
        }
        emit_ns += emit_start.elapsed().as_nanos();
    }

    if let Some(prev) = current.take() {
        owner_stats.push(prev.finish());
    }
    if narrowing_enabled && cached_owner.is_some() && !narrow_attempted {
        narrow_skipped_owners += 1;
    }

    let stage_breakdown =
        StageBreakdown { infer_ns, narrow_ns, narrow_arg_ns, emit_ns, narrow_skipped_owners };
    log_owner_stats(
        query_start.elapsed(),
        &infer.call_arg_bindings,
        &out,
        &mut owner_stats,
        stage_breakdown,
    );

    Arc::new(out)
}

#[derive(Default)]
struct StageBreakdown {
    infer_ns: u128,
    narrow_ns: u128,
    narrow_arg_ns: u128,
    emit_ns: u128,
    narrow_skipped_owners: u64,
}

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

fn log_owner_stats(
    total_elapsed: Duration,
    bindings: &[crate::infer::CallArgBinding],
    out: &[(DefWithBodyId, InferenceDiagnostic)],
    owner_stats: &mut [OwnerStat],
    stages: StageBreakdown,
) {
    const SLOW_THRESHOLD: Duration = Duration::from_millis(500);
    if total_elapsed < SLOW_THRESHOLD {
        return;
    }

    owner_stats.sort_by_key(|s| std::cmp::Reverse(s.elapsed));

    let total_ms = total_elapsed.as_millis() as u64;
    let infer_ms = (stages.infer_ns / 1_000_000) as u64;
    let narrow_ms = (stages.narrow_ns / 1_000_000) as u64;
    let narrow_arg_ms = (stages.narrow_arg_ns / 1_000_000) as u64;
    let emit_ms = (stages.emit_ns / 1_000_000) as u64;
    let attributed_ms =
        infer_ms.saturating_add(narrow_ms).saturating_add(narrow_arg_ms).saturating_add(emit_ms);
    let query_self_ms = total_ms.saturating_sub(attributed_ms);

    tracing::info!(
        total_elapsed_ms = total_ms,
        total_bindings = bindings.len(),
        total_owners = owner_stats.len(),
        diagnostics = out.len(),
        infer_ms,
        narrow_ms,
        narrow_arg_ms,
        emit_ms,
        query_self_ms,
        narrow_skipped_owners = stages.narrow_skipped_owners,
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

fn narrow_arg(
    db: &dyn HirDatabase,
    narrow: Option<&dataflow::DataflowResult<NarrowState>>,
    body: &Body,
    expr_id: ExprId,
    base: TypeId,
) -> TypeId {
    let Some(result) = narrow else {
        return base;
    };
    let Expr::Path(name) = body.expr(expr_id) else {
        return base;
    };
    narrowed_type_at(db, result, body, expr_id.to_idx(), name).unwrap_or(base)
}

fn resolve_body(module_bodies: &hir_def::ModuleBodies, owner: DefWithBodyId) -> Option<&Body> {
    match owner {
        DefWithBodyId::ModuleCode => module_bodies.module_code(),
        DefWithBodyId::Method(local_id) => module_bodies.body(local_id),
    }
}

fn emit_single(
    db: &dyn HirDatabase,
    args: &[ExprId],
    arg_types: &[TypeId],
    params: &[TypeId],
    out: &mut Vec<(DefWithBodyId, InferenceDiagnostic)>,
    owner: DefWithBodyId,
    from_doc_comment: bool,
) {
    for ((arg_id, &arg_ty), &param_id) in args.iter().zip(arg_types.iter()).zip(params.iter()) {
        if !crate::subtype::is_coercible_to(db, arg_ty, param_id) {
            out.push((
                owner,
                InferenceDiagnostic::TypeMismatch {
                    expr: *arg_id,
                    expected: param_id,
                    actual: arg_ty,
                    from_doc_comment,
                },
            ));
        }
    }
}

// A thin emit helper threading the call's data straight to the diagnostic sink; bundling the
// arguments into a struct would only add an indirection local to this file.
#[allow(clippy::too_many_arguments)]
fn emit_overloaded(
    db: &dyn HirDatabase,
    args: &[ExprId],
    arg_types: &[TypeId],
    flat: &[TypeId],
    overloads: &[Arc<[TypeId]>],
    out: &mut Vec<(DefWithBodyId, InferenceDiagnostic)>,
    owner: DefWithBodyId,
    from_doc_comment: bool,
) {
    if overloads.is_empty() {
        emit_single(db, args, arg_types, flat, out, owner, from_doc_comment);
        return;
    }

    let any_accepts = overloads.iter().any(|params| {
        if args.len() > params.len() {
            return false;
        }
        arg_types
            .iter()
            .zip(params.iter())
            .all(|(&a, &p)| crate::subtype::is_coercible_to(db, a, p))
    });
    if any_accepts {
        return;
    }

    let chosen: &[TypeId] = overloads
        .iter()
        .min_by_key(|params| params.len().abs_diff(args.len()))
        .map(|p| p.as_ref())
        .unwrap_or(flat);
    emit_single(db, args, arg_types, chosen, out, owner, from_doc_comment);
}
