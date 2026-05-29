use bsl_metadata::{FormType, ModuleType};
use hir::AnnotationKind;
use ide_db::TextRange;

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::FormModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Unpredictable, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ServerSideExportFormMethod;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let metadata = ctx.module_metadata();

    if metadata.module_type != ModuleType::FormModule {
        return Vec::new();
    }

    match &metadata.form {
        Some(form) if form.form_type == FormType::Managed => {}
        _ => return Vec::new(),
    }

    let item_tree = ctx.item_tree();
    let mut diagnostics = Vec::new();

    for (_, proc) in item_tree.procedures() {
        if proc.is_export && !has_client_annotation(&proc.annotations) {
            diagnostics.push(make_diagnostic(proc.name_range, code, ctx));
        }
    }

    for (_, func) in item_tree.functions() {
        if func.is_export && !has_client_annotation(&func.annotations) {
            diagnostics.push(make_diagnostic(func.name_range, code, ctx));
        }
    }

    diagnostics
}

fn has_client_annotation(annotations: &[hir::Annotation]) -> bool {
    annotations.iter().any(|a| a.kind == AnnotationKind::AtClient)
}

fn make_diagnostic(range: TextRange, code: DiagnosticCode, ctx: &DiagnosticsContext) -> Diagnostic {
    Diagnostic {
        code,
        message: "Запрещено создавать серверные экспортные методы в форме".to_string(),
        range,
        severity: ctx.severity(code),
        tags: ctx.tags(code),
        fixes: Vec::new(),
    }
}
