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
use hir::{BodySourceMap, DefWithBodyId, ExprId, InferenceDiagnostic};
use ide_db::TextRange;

/// Diagnostic codes produced by the type-inference collector.
pub(crate) const INFERENCE_DIAGNOSTICS: &[DiagnosticCode] = &[
    DiagnosticCode::UnresolvedMethodCall,
    DiagnosticCode::MismatchedArgCount,
    DiagnosticCode::TypeMismatch,
    DiagnosticCode::UnresolvedField,
    DiagnosticCode::ReadOnlyPropertyAssignment,
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

    let module_bodies = ctx.module_bodies();

    let mut diagnostics = Vec::new();

    for (owner, diag) in &infer.diagnostics {
        // Resolve the owning BodySourceMap for this diagnostic's body.
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
            handlers::type_mismatch::from_hir(expected, actual, range, ctx)
        }
        InferenceDiagnostic::UnresolvedField { receiver_ty, field_name, .. } => {
            handlers::unresolved_field::from_hir(receiver_ty, field_name, range, ctx)
        }
        InferenceDiagnostic::ReadOnlyPropertyAssignment { receiver_ty, field_name, .. } => {
            handlers::read_only_property::from_hir(receiver_ty, field_name, range, ctx)
        }
    }
}
