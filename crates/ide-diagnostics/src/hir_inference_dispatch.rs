//! Type-inference diagnostic dispatch.
//!
//! Collects `InferenceDiagnostic`s produced by `hir-ty::infer` and routes
//! each one to its handler's `from_hir()` function.
//!
//! ## Dispatch flow
//!
//! 1. Pull the file-level [`InferenceResult`] via `ctx.infer()`.
//! 2. For each `(owner, diag)` pair: resolve `ExprId → TextRange` via the
//!    corresponding [`BodySourceMap`]. Method bodies are keyed by local_id
//!    (`DefWithBodyId::Method`); module-level code uses the dedicated
//!    `module_code_result()` slot.
//! 3. Drop diagnostics we can't locate (missing body, ExprId out of range);
//!    the alternative — falling back to byte 0 — would splay a red squiggle
//!    over the module header and mislead the user.
//! 4. Dispatch to the matching handler module.
//!
//! Kept next to `hir_dispatch.rs` because the two crossways collect HIR-
//! provenance diagnostics; this one is the type-inference equivalent of the
//! `BodyDiagnostic` channel.

use crate::{handlers, Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{BodySourceMap, DefWithBodyId, ExprId, InferenceDiagnostic, RedundantAccessKind};
use ide_db::TextRange;

/// Diagnostic codes produced by the type-inference collector.
///
/// `RedundantAccessToObject` and `MissedRequiredParameter` also appear
/// in `HIR_DIAGNOSTICS` (the `BodyDiagnostic` channel — three-level and
/// `ЭтотОбъект`/local shapes), and are listed here additionally because
/// inference now also emits the two-level CommonModule variant after
/// the body-lowering classification was lifted into the inference
/// layer (see `InferenceDiagnostic::RedundantAccessToObjectTwoLevel`
/// and `MissedRequiredParameterCommonModule`).
pub(crate) const INFERENCE_DIAGNOSTICS: &[DiagnosticCode] = &[
    DiagnosticCode::UnresolvedMethodCall,
    DiagnosticCode::MismatchedArgCount,
    DiagnosticCode::TypeMismatch,
    DiagnosticCode::UnresolvedField,
    DiagnosticCode::ReadOnlyPropertyAssignment,
    DiagnosticCode::RedundantAccessToObject,
    DiagnosticCode::MissedRequiredParameter,
];

/// Collect inference-produced diagnostics for the current file.
pub fn collect_inference_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Early exit when all BSL-TY-* codes are disabled.
    if !ctx.config.any_enabled(INFERENCE_DIAGNOSTICS) {
        return Vec::new();
    }

    let infer = ctx.infer();
    if infer.diagnostics.is_empty() {
        return Vec::new();
    }

    dispatch_pairs(ctx, &infer.diagnostics)
}

/// Collect narrowing-aware argument-mismatch diagnostics for the
/// current file.
///
/// Mirror of [`collect_inference_diagnostics`] for the
/// [`hir::HirDatabase::arg_diagnostics`] query. Runs as its own
/// orchestrator stage right after inference so diagnostics produced
/// **after** the narrowing overlay reach the same dispatch /
/// deduplication path as the rest of the BSL-TY-* family.
pub fn collect_arg_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // `arg_diagnostics_query` only ever produces `TypeMismatch`; gate
    // on that single code rather than the full BSL-TY-* set.
    if !ctx.config.any_enabled(&[DiagnosticCode::TypeMismatch]) {
        return Vec::new();
    }

    let arg_diags = ctx.arg_diagnostics();
    if arg_diags.is_empty() {
        return Vec::new();
    }

    dispatch_pairs(ctx, &arg_diags)
}

/// Shared `(owner, diag) → Diagnostic` resolution used by both
/// inference-stage and arg-stage collectors.
///
/// The `BodySourceMap` lookup, `ExprId → TextRange` resolution and
/// drop-on-miss policy are all common — duplicating them between the
/// two collectors would be a guaranteed source of skew the next time
/// the source-map shape changes.
fn dispatch_pairs(
    ctx: &DiagnosticsContext,
    pairs: &[(DefWithBodyId, InferenceDiagnostic)],
) -> Vec<Diagnostic> {
    let module_bodies = ctx.module_bodies();
    let mut diagnostics = Vec::new();

    for (owner, diag) in pairs {
        let source_map = match owner {
            DefWithBodyId::Method(local_id) => module_bodies.source_map(*local_id),
            DefWithBodyId::ModuleCode => module_bodies.module_code_result().map(|r| &r.source_map),
        };
        let Some(source_map) = source_map else {
            tracing::debug!(
                ?owner,
                ?diag,
                "dropping inference diagnostic: owning body has no source map"
            );
            continue;
        };

        let Some(range) = diagnostic_range(source_map, diag) else {
            tracing::debug!(?owner, ?diag, "dropping inference diagnostic: ExprId has no range");
            continue;
        };

        if let Some(d) = dispatch_inference_diagnostic(diag, range, ctx) {
            diagnostics.push(d);
        }
    }

    diagnostics
}

/// Extract the `ExprId` an inference diagnostic points at.
fn diagnostic_expr(diag: &InferenceDiagnostic) -> ExprId {
    match diag {
        InferenceDiagnostic::UnresolvedMethodCall { expr, .. } => *expr,
        InferenceDiagnostic::MismatchedArgCount { call_expr, .. } => *call_expr,
        InferenceDiagnostic::TypeMismatch { expr, .. } => *expr,
        InferenceDiagnostic::UnresolvedField { expr, .. } => *expr,
        InferenceDiagnostic::ReadOnlyPropertyAssignment { lhs, .. } => *lhs,
        InferenceDiagnostic::RedundantAccessToObjectTwoLevel { expr, .. } => *expr,
        InferenceDiagnostic::MissedRequiredParameterCommonModule { expr, .. } => *expr,
    }
}

fn diagnostic_range(source_map: &BodySourceMap, diag: &InferenceDiagnostic) -> Option<TextRange> {
    source_map.expr_range(diagnostic_expr(diag))
}

/// Route a single inference diagnostic to its handler.
///
/// Exhaustive match (no wildcard) so the compiler flags new
/// `InferenceDiagnostic` variants before they silently slip into the
/// production pipeline.
fn dispatch_inference_diagnostic(
    diag: &InferenceDiagnostic,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    match diag {
        InferenceDiagnostic::UnresolvedMethodCall { receiver_name, method_name, kind, .. } => {
            handlers::unresolved_method_call::from_hir(
                receiver_name,
                method_name,
                *kind,
                range,
                ctx,
            )
        }
        InferenceDiagnostic::MismatchedArgCount { required_count, total_count, found, .. } => {
            handlers::mismatched_arg_count::from_hir(
                *required_count,
                *total_count,
                *found,
                range,
                ctx,
            )
        }
        InferenceDiagnostic::TypeMismatch { expected, actual, .. } => {
            handlers::type_mismatch::from_hir(*expected, *actual, range, ctx)
        }
        InferenceDiagnostic::UnresolvedField { receiver_ty, field_name, .. } => {
            handlers::unresolved_field::from_hir(*receiver_ty, field_name, range, ctx)
        }
        InferenceDiagnostic::ReadOnlyPropertyAssignment { receiver_ty, field_name, .. } => {
            handlers::read_only_property::from_hir(*receiver_ty, field_name, range, ctx)
        }
        InferenceDiagnostic::RedundantAccessToObjectTwoLevel { module, .. } => {
            // Reuse the existing handler that already validates against
            // module metadata (CommonModule type + DontUse reuse +
            // matching name). Inference promises the receiver resolves
            // to a CommonModule via `user_common_module_exists`, so
            // the handler's CommonModule gate is the right second
            // check — same shape as the lowering-emitted ThreeLevel /
            // ThisObject siblings.
            //
            // `RedundantAccessKind::TwoLevel { module: String }` lives
            // in hir-def (re-exported as `hir::RedundantAccessKind`);
            // converting `Name → String` here keeps the inference
            // payload `Name`-typed (cheap clone, interned) while the
            // body-diagnostic schema stays `String`.
            let kind = RedundantAccessKind::TwoLevel { module: module.as_str().to_string() };
            handlers::redundant_access_to_object::from_hir(&kind, range, ctx)
        }
        InferenceDiagnostic::MissedRequiredParameterCommonModule {
            callee, module, args, ..
        } => {
            // CommonModule shape: `module = Some(name)`, `mdo_type` and
            // `mdo_name` both `None`. The handler routes to
            // `check_qualified_call`, which resolves the method via
            // SymbolTree and validates each required parameter slot
            // against the boolean presence array — identical to how
            // the lowering-emitted version flows, just sourced from
            // inference.
            handlers::missed_required_parameter::from_hir(
                callee.as_str(),
                Some(module.as_str()),
                None,
                None,
                args,
                range,
                ctx,
            )
        }
    }
}
