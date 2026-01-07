//! EmptyRegion diagnostic.
//!
//! Detects empty code regions that contain only comments, whitespace, or nested empty regions.
//!
//! ## Why?
//! Empty regions serve no purpose and clutter the code. They should either contain
//! meaningful code or be removed entirely.
//!
//! ## Bad practice
//! ```bsl
//! #Область ПустаяОбласть
//! // Только комментарий
//! #КонецОбласти
//! ```
//!
//! ## Good practice
//! ```bsl
//! #Область ПолезнаяОбласть
//! Перем Счетчик;
//! #КонецОбласти
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Information (INFO)
//! - **Tags:** STANDARD
//! - **Minutes to fix:** 1
//!
//! ## Nested Regions
//! Handles nested empty regions correctly:
//! - Reports both inner and outer if both empty
//! - Reports only inner if outer has code
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! Preprocessor regions ARE processed by HIR lowering for control flow analysis.
//! The diagnostic is emitted in `hir-def/body/lower/preproc.rs` during region processing.
//!
//! Ported from:
//! - EmptyRegionDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - empty_region.rs (bsl-language-server-rust) - Algorithm reference

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::EmptyRegion` is encountered.
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::EmptyRegion) {
        return None;
    }

    Some(Diagnostic {
        code: DiagnosticCode::EmptyRegion,
        message: format!("Область '{}' пуста", name),
        severity: Severity::Information,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/EmptyRegionDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let empty_region_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::EmptyRegion).collect();

        assert_eq!(empty_region_diags.len(), 3, "Should match Java: 3 diagnostics");

        assert_diagnostic_range_multiline(code, empty_region_diags[0], 0, 0, 2, 13);
        assert!(empty_region_diags[0].message.contains("Тест"));

        assert_diagnostic_range_multiline(code, empty_region_diags[1], 10, 0, 15, 13);
        assert!(empty_region_diags[1].message.contains("ВнешняяОбласть"));

        assert_diagnostic_range_multiline(code, empty_region_diags[2], 12, 0, 14, 13);
        assert!(empty_region_diags[2].message.contains("ВнутренняяОбласть"));
    }

    #[test]
    fn test_region_with_variables() {
        let code = r#"
#Область Переменные
Перем А;
#КонецОбласти
        "#;
        let diagnostics = check_hir_diagnostic(code);
        let empty_region_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::EmptyRegion).collect();
        assert_eq!(empty_region_diags.len(), 0, "Region with variable is not empty");
    }

    #[test]
    fn test_region_with_function() {
        let code = r#"
#Область ПрограммныйИнтерфейс
Функция Тест()
КонецФункции
#КонецОбласти
        "#;
        let diagnostics = check_hir_diagnostic(code);
        let empty_region_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::EmptyRegion).collect();
        assert_eq!(empty_region_diags.len(), 0, "Region with function is not empty");
    }

    #[test]
    fn test_nested_both_empty() {
        let code = r#"
#Область Внешняя
    #Область Внутренняя
    #КонецОбласти
#КонецОбласти
        "#;
        let diagnostics = check_hir_diagnostic(code);
        let empty_region_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::EmptyRegion).collect();
        assert_eq!(empty_region_diags.len(), 2, "Both nested empty regions reported");
    }

    #[test]
    fn test_nested_outer_has_code() {
        let code = r#"
#Область Внешняя
    Перем А;
    #Область Внутренняя
    #КонецОбласти
#КонецОбласти
        "#;
        let diagnostics = check_hir_diagnostic(code);
        let empty_region_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::EmptyRegion).collect();
        assert_eq!(empty_region_diags.len(), 1, "Only inner empty region reported");
        assert!(empty_region_diags[0].message.contains("Внутренняя"));
    }

    #[test]
    fn test_bilingual_keywords() {
        let code = r#"
#Region Test
// comment only
#EndRegion

#Область Тест
// comment only
#КонецОбласти
        "#;
        let diagnostics = check_hir_diagnostic(code);
        let empty_region_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::EmptyRegion).collect();
        assert_eq!(empty_region_diags.len(), 2, "Both English and Russian empty regions reported");
    }
}
