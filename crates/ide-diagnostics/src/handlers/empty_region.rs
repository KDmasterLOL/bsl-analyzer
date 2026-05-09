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
//! Track 2 Phase C §3.4 — handler reads the cached
//! `RegionTree::is_region_empty` bit (computed at AST→`RegionTree`
//! lowering time). The legacy `BodyDiagnostic::EmptyRegion` emit in
//! `body/lower/preproc.rs` was retired in this slice; the
//! single-source classification lives next to the rest of the region
//! data on `RegionData`.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

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

/// Track 2 Phase C §3.4 — handler-side detection consuming the
/// cached `RegionTree::is_region_empty` bit via `ctx.region_tree()`.
/// Replaces the legacy `from_hir` adapter (BodyDiagnostic-fed) and
/// the AST walks in `body/lower/preproc::is_empty_region`.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::EmptyRegion;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let region_tree = ctx.region_tree();
    let mut out = Vec::new();
    for (idx, region) in region_tree.regions() {
        if !region_tree.is_region_empty(idx) {
            continue;
        }
        out.push(Diagnostic {
            code,
            message: format!("Область '{}' пуста", region.name.as_str()),
            severity: ctx.severity(code),
            range: region.range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
    // Sort by source position for deterministic output (the
    // `Arena::iter` order isn't position-sorted by construction).
    out.sort_by_key(|d| d.range.start());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    /// Codex round-A regression guard for the §3.4 migration: the
    /// retired `lower_region_stmts` emit ran inside method lowering
    /// for method-local regions. Without recursing into method
    /// bodies in `RegionTreeBuilder::collect_regions`, the migrated
    /// handler would silently miss them.
    #[test]
    fn test_method_local_empty_region_is_detected() {
        let code = r#"Процедура Тест()
    #Область ВнутриМетода
    // комментарий
    #КонецОбласти
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::EmptyRegion).collect();
        assert_eq!(diags.len(), 1, "method-local empty region must still be detected after §3.4");
        assert!(diags[0].message.contains("ВнутриМетода"));
    }

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
