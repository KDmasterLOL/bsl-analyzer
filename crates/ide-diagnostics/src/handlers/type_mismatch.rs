use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::TypeId;
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    expected: TypeId,
    actual: TypeId,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let locale = ctx.locale();
    let message = format!(
        "Несоответствие типов: ожидалось '{}', получено '{}'",
        ctx.kernel_type_display(expected, locale),
        ctx.kernel_type_display(actual, locale)
    );
    crate::simple_hir_diagnostic(DiagnosticCode::TypeMismatch, message, range, ctx)
}
