//! EmptyCodeBlock diagnostic
//!
//! Detects empty code blocks in control structures (if/while/for/etc).
//!
//!
//! BSL supports empty code blocks in control structures, but they often indicate
//! incomplete implementation or unintended code.  This diagnostic helps detect such cases.
//!
//! ## Empty blocks detected:
//! - Empty if/then blocks
//! - Empty elsif blocks
//! - Empty else blocks
//! - Empty while/for/foreach loops
//!
//! ## NOT checked (other diagnostics handle these):
//! - Empty function/procedure bodies (handled by other diagnostic)
//! - Empty try/except blocks (handled by other diagnostic)
//!
//! ## Configuration
//! - `commentAsCode` (boolean, default: false) - If true, blocks containing only comments
//!   are NOT considered empty
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! The diagnostic is emitted in `hir-def/body/lower/stmt.rs` during statement lowering
//! for if/elsif/else/while/for/foreach/try/except blocks.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::EmptyCodeBlock` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(DiagnosticCode::EmptyCodeBlock, "Пустой блок кода", range, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_empty_code_block() {
        let code = include_str!("../../test_data/EmptyCodeBlockDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let empty_block_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::EmptyCodeBlock).collect();

        // Expected 6 diagnostics at specific positions
        // HIR lowering now matches bsl-language-server behavior (excludes try/except blocks)
        assert_eq!(
            empty_block_diags.len(),
            6,
            "Expected 6 diagnostics to match bsl-language-server"
        );

        // Line 6 (0-indexed line 5), cols 1-6 (Иначе)
        assert_diagnostic_range_multiline(code, empty_block_diags[0], 5, 1, 5, 6);

        // Line 18 (0-indexed line 17), cols 2-18 (Пока Истина Цикл)
        assert_diagnostic_range_multiline(code, empty_block_diags[1], 17, 2, 17, 18);

        // Line 25 (0-indexed line 24), cols 4-21 (Если Истина Тогда)
        assert_diagnostic_range_multiline(code, empty_block_diags[2], 24, 4, 24, 21);

        // Line 36 (0-indexed line 35), cols 0-16 (Если а = 0 Тогда)
        assert_diagnostic_range_multiline(code, empty_block_diags[3], 35, 0, 35, 16);

        // Line 38 (0-indexed line 37), cols 0-21 (ИначеЕсли А = 1 Тогда)
        assert_diagnostic_range_multiline(code, empty_block_diags[4], 37, 0, 37, 21);

        // Line 39 (0-indexed line 38), cols 4-9 (Иначе)
        assert_diagnostic_range_multiline(code, empty_block_diags[5], 38, 4, 38, 9);
    }
}
