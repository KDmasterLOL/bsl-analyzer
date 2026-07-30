use crate::{handlers, Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{BodySourceMap, DefWithBodyId, ExprId, InferenceDiagnostic, RedundantAccessKind};
use ide_db::TextRange;

pub(crate) const INFERENCE_DIAGNOSTICS: &[DiagnosticCode] = &[
    DiagnosticCode::UnresolvedMethodCall,
    DiagnosticCode::MismatchedArgCount,
    DiagnosticCode::TypeMismatch,
    DiagnosticCode::TypeMismatchByDocComment,
    DiagnosticCode::UnresolvedField,
    DiagnosticCode::ReadOnlyPropertyAssignment,
    DiagnosticCode::GlobalPropertyNotWritable,
    DiagnosticCode::DeprecatedPlatformApi,
    DiagnosticCode::RedundantAccessToObject,
    DiagnosticCode::MissedRequiredParameter,
    DiagnosticCode::UnavailableInEnvironment,
    DiagnosticCode::ModuleAccessibility,
];

pub fn collect_inference_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if !ctx.config.any_enabled(INFERENCE_DIAGNOSTICS) {
        return Vec::new();
    }

    let infer = ctx.infer();
    if infer.diagnostics.is_empty() {
        return Vec::new();
    }

    dispatch_pairs(ctx, &infer.diagnostics)
}

pub fn collect_arg_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if !ctx.config.any_enabled(&[
        DiagnosticCode::MismatchedArgCount,
        DiagnosticCode::TypeMismatch,
        DiagnosticCode::TypeMismatchByDocComment,
    ]) {
        return Vec::new();
    }

    let arg_diags = ctx.arg_diagnostics();
    if arg_diags.is_empty() {
        return Vec::new();
    }

    dispatch_pairs(ctx, &arg_diags)
}

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

fn diagnostic_expr(diag: &InferenceDiagnostic) -> ExprId {
    match diag {
        InferenceDiagnostic::UnresolvedMethodCall { expr, .. } => *expr,
        InferenceDiagnostic::MismatchedArgCount { call_expr, .. } => *call_expr,
        InferenceDiagnostic::TypeMismatch { expr, .. } => *expr,
        InferenceDiagnostic::UnresolvedField { expr, .. } => *expr,
        InferenceDiagnostic::ReadOnlyPropertyAssignment { lhs, .. } => *lhs,
        InferenceDiagnostic::GlobalPropertyNotWritable { lhs, .. } => *lhs,
        InferenceDiagnostic::DeprecatedPlatformMember { expr, .. } => *expr,
        InferenceDiagnostic::RedundantAccessToObjectTwoLevel { expr, .. } => *expr,
        InferenceDiagnostic::MissedRequiredParameterCommonModule { expr, .. } => *expr,
        InferenceDiagnostic::MissedRequiredParameterManagerModule { expr, .. } => *expr,
        InferenceDiagnostic::RedundantAccessToObjectThreeLevel { expr, .. } => *expr,
        InferenceDiagnostic::UnavailableInEnvironment { expr, .. } => *expr,
        InferenceDiagnostic::ModuleAccessibility { expr, .. } => *expr,
    }
}

fn diagnostic_range(source_map: &BodySourceMap, diag: &InferenceDiagnostic) -> Option<TextRange> {
    source_map.expr_range(diagnostic_expr(diag))
}

fn anchor_on_leading_name(
    ctx: &DiagnosticsContext,
    range: TextRange,
    name: &hir::Name,
) -> TextRange {
    let parse = ctx.parse();
    let Some(token) = parse.syntax_node().token_at_offset(range.start()).right_biased() else {
        return range;
    };
    if token.text_range().start() == range.start() && token.text() == name.as_str() {
        token.text_range()
    } else {
        range
    }
}

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
        InferenceDiagnostic::TypeMismatch { expected, actual, from_doc_comment, .. } => {
            if *from_doc_comment {
                handlers::type_mismatch_by_doc_comment::from_hir(*expected, *actual, range, ctx)
            } else {
                handlers::type_mismatch::from_hir(*expected, *actual, range, ctx)
            }
        }
        InferenceDiagnostic::UnresolvedField { receiver_ty, field_name, .. } => {
            handlers::unresolved_field::from_hir(*receiver_ty, field_name, range, ctx)
        }
        InferenceDiagnostic::ReadOnlyPropertyAssignment { receiver_ty, field_name, .. } => {
            handlers::read_only_property::from_hir(*receiver_ty, field_name, range, ctx)
        }
        InferenceDiagnostic::GlobalPropertyNotWritable { name, .. } => {
            handlers::global_property_not_writable::from_hir(name, range, ctx)
        }
        InferenceDiagnostic::DeprecatedPlatformMember {
            type_name,
            member_name,
            is_property,
            ..
        } => handlers::deprecated_platform_api::from_hir(
            type_name,
            member_name,
            *is_property,
            range,
            ctx,
        ),
        InferenceDiagnostic::RedundantAccessToObjectTwoLevel { module, .. } => {
            let kind = RedundantAccessKind::TwoLevel { module: module.as_str().to_string() };
            handlers::redundant_access_to_object::from_hir(&kind, range, ctx)
        }
        InferenceDiagnostic::MissedRequiredParameterManagerModule {
            callee,
            mdo_type,
            mdo_name,
            args,
            ..
        } => handlers::missed_required_parameter::from_hir(
            callee.as_str(),
            None,
            Some(mdo_type.as_str()),
            Some(mdo_name.as_str()),
            args,
            range,
            ctx,
        ),
        InferenceDiagnostic::RedundantAccessToObjectThreeLevel { mdo_type, mdo_name, .. } => {
            let kind = RedundantAccessKind::ThreeLevel {
                mdo_type: mdo_type.as_str().to_string(),
                mdo_name: mdo_name.as_str().to_string(),
            };
            handlers::redundant_access_to_object::from_hir(&kind, range, ctx)
        }
        InferenceDiagnostic::MissedRequiredParameterCommonModule {
            callee, module, args, ..
        } => handlers::missed_required_parameter::from_hir(
            callee.as_str(),
            Some(module.as_str()),
            None,
            None,
            args,
            range,
            ctx,
        ),
        InferenceDiagnostic::UnavailableInEnvironment { name, member_kind, missing, .. } => {
            // The verdict on a three-level call (`Справочники.X.Метод()`) is
            // anchored on the synthetic callee, whose range spans the whole
            // call; the message names the collection root — the call's first
            // token. Shrink to that token when it is the named root, a no-op
            // for a bare name whose range already is the identifier.
            let range = if matches!(member_kind, hir::EnvMemberKind::GlobalProperty) {
                anchor_on_leading_name(ctx, range, name)
            } else {
                range
            };
            handlers::unavailable_in_environment::from_hir(name, *member_kind, *missing, range, ctx)
        }
        InferenceDiagnostic::ModuleAccessibility { name, callee_kind, missing, .. } => {
            handlers::module_accessibility::from_hir(name, *callee_kind, *missing, range, ctx)
        }
    }
}
