//! TooManyReturns diagnostic
//!
//! Detects methods with too many return statements.
//!
//! **Source:** bsl-language-server/TooManyReturnsDiagnostic.java
//!
//! ## Why?
//!
//! Excessive return statements increase method complexity and reduce readability.
//! Multiple exit points make code harder to understand and maintain.
//!
//! ## Configuration
//!
//! ### `maxReturnsCount` (integer)
//! Maximum allowed return statements per method.
//! Default: `3`
//!
//! ## Implementation
//!
//! **This is a HIR-based diagnostic** - return statements are collected during AST→HIR lowering.
//! The handler applies configuration filtering (maxReturnsCount).

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 20,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_MAX_RETURNS_COUNT: i64 = 3;

pub fn from_hir(
    method_name: &str,
    method_name_range: TextRange,
    returns: &[TextRange],
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::TooManyReturns;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let max_returns_count =
        ctx.config.get_int(code, "maxReturnsCount").unwrap_or(DEFAULT_MAX_RETURNS_COUNT);

    if (returns.len() as i64) <= max_returns_count {
        return None;
    }

    let message = format!(
        "Метод \"{}\" содержит {} возвратов при максимально допустимом {}",
        method_name,
        returns.len(),
        max_returns_count
    );

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range: method_name_range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_too_many_returns_default() {
        let code = include_str!("../../test_data/TooManyReturnsDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TooManyReturns).collect();

        assert_eq!(diags.len(), 1, "Expected 1 diagnostic, got {}", diags.len());

        assert_diagnostic_range(code, diags[0], 11, 8, 21);
    }

    #[test]
    fn test_three_returns_ok() {
        let code = include_str!("../../test_data/TooManyReturnsDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let three_returns_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.code == DiagnosticCode::TooManyReturns && d.message.contains("ТриВозврата")
            })
            .collect();

        assert_eq!(
            three_returns_diags.len(),
            0,
            "ТриВозврата should not trigger diagnostic (3 returns is OK)"
        );
    }

    #[test]
    fn test_five_returns() {
        let code = include_str!("../../test_data/TooManyReturnsDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let five_returns_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.code == DiagnosticCode::TooManyReturns && d.message.contains("ПятьВозвратов")
            })
            .collect();

        assert_eq!(
            five_returns_diags.len(),
            1,
            "ПятьВозвратов should trigger diagnostic (5 returns)"
        );
        assert_diagnostic_range(code, five_returns_diags[0], 11, 8, 21);
    }
}
