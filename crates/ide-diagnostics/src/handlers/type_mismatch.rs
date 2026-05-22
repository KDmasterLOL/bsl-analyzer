//! TypeMismatch diagnostic.
//!
//! Emitted from `hir-ty::infer` when an expression's inferred type doesn't
//! match what the surrounding context expects.
//!
//! **No live emitter today.** The M3 inference code has the emission site
//! stubbed out; the emitter lands in M4 Task 7 (`is_assignable_to`). This
//! handler is wired into the dispatch now so that (a) the exhaustive
//! `match` in `hir_inference_dispatch` stays exhaustive as the enum grows,
//! and (b) Task 7 only has to flip the emission guard — no second plumbing
//! pass through `code.rs` / `handlers.rs` / `lib.rs`.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::Ty;
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

/// Creates diagnostic from `InferenceDiagnostic::TypeMismatch`.
pub fn from_hir(
    expected: &Ty,
    actual: &Ty,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let locale = ctx.locale();
    // `display(locale)` expands `Ty::Union` to `A | B`, so the user sees
    // the actual member types instead of the coarse `Составной`/`Composite`
    // label. Hover and completion already use the same surface for the
    // same reason.
    let message = format!(
        "Несоответствие типов: ожидалось '{}', получено '{}'",
        expected.display(locale),
        actual.display(locale)
    );
    crate::simple_hir_diagnostic(DiagnosticCode::TypeMismatch, message, range, ctx)
}
