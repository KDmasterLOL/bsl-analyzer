//! Reports assignments to platform properties that are marked read-only.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Name, Ty};
use ide_db::TextRange;

// Warning-level: the user may be intentionally trying to call a setter
// that BSL doesn't actually expose (a common mistake when porting code
// between platforms), but a misfire on a stale HBK entry should not
// block a build. The emit guard in `hir-ty::infer::Stmt::Assign` only
// pushes on a confirmed platform-property hit, so false positives
// require a genuine HBK drift rather than receiver-shape confusion.
pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from `InferenceDiagnostic::ReadOnlyPropertyAssignment`.
pub fn from_hir(
    receiver_ty: &Ty,
    field_name: &Name,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let message = format!(
        "Свойство '{}' типа '{}' доступно только для чтения",
        field_name.as_str(),
        receiver_ty.display_name()
    );
    crate::simple_hir_diagnostic(DiagnosticCode::ReadOnlyPropertyAssignment, message, range, ctx)
}
