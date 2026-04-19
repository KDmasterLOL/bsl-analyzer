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

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::EmptyRegion` is encountered.
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::EmptyRegion;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!("Область '{}' пуста", name),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    #[test]
    fn test_comment_only_region_is_empty() {
        let code = r#"#Область Тест
// комментарий
#КонецОбласти"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::EmptyRegion).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range_multiline(code, diags[0], 0, 0, 2, 13);
        assert!(diags[0].message.contains("Тест"));
    }

    #[test]
    fn test_outer_region_with_only_inner_empty_region() {
        // Both outer and inner are reported when outer contains only an empty inner region
        let code = r#"#Область ВнешняяОбласть
// комментарий
#Область ВнутренняяОбласть

#КонецОбласти
#КонецОбласти"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::EmptyRegion).collect();
        assert_eq!(diags.len(), 2);
        assert!(diags.iter().any(|d| d.message.contains("ВнешняяОбласть")));
        assert!(diags.iter().any(|d| d.message.contains("ВнутренняяОбласть")));
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
