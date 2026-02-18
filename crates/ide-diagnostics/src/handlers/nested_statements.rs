//! NestedStatements diagnostic.
//!
//! Detects control flow statements (IF, WHILE, FOR, TRY) nested too deeply.
//!
//! ## Why?
//! Deeply nested control structures make code hard to read, understand, and test.
//! They often indicate poor decomposition and lack of abstraction.
//!
//! ## Bad practice
//! ```bsl
//! Если условие1 Тогда
//!     Если условие2 Тогда
//!         Если условие3 Тогда
//!             Если условие4 Тогда
//!                 Если условие5 Тогда  // 5 levels - violation!
//!                     // deep nested logic
//!                 КонецЕсли;
//!             КонецЕсли;
//!         КонецЕсли;
//!     КонецЕсли;
//! КонецЕсли;
//! ```
//!
//! ## Good practice
//! Extract logic into separate functions or use early returns:
//! ```bsl
//! Если НЕ условие1 Тогда
//!     Возврат;
//! КонецЕсли;
//!
//! Если НЕ условие2 Тогда
//!     Возврат;
//! КонецЕсли;
//!
//! // main logic here (flat structure)
//! ```
//!
//! ## Configuration
//! - **maxAllowedLevel** (default: 4) - Maximum allowed nesting depth
//! - **Enabled by default:** Yes
//! - **Severity:** CRITICAL
//! - **Tags:** BRAINOVERLOAD (concept)
//! - **Minutes to fix:** 30
//!
//! ## Implementation
//! Ported from: NestedStatementsDiagnostic.java (bsl-language-server)
//!
//! Algorithm:
//! - Recursive AST traversal with depth tracking
//! - Counts nesting levels for IF, WHILE, FOR, FOR_EACH, TRY statements
//! - Uses boolean flag propagation to identify leaf statements efficiently
//! - Reports the deepest (leaf) statement that exceeds threshold
//!
//! ## Performance
//! - **Time complexity:** O(n) where n = number of AST nodes (single pass)
//! - **Optimization:** Returns boolean flag from recursion instead of calling `node.descendants()`
//! - Avoids O(n²) complexity by propagating "has nested child" information bottom-up
//!
//! ## Note
//! This diagnostic uses AST (not HIR) because it checks structural properties only.
//! AST tree traversal is simpler and more efficient for this use case.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

const DEFAULT_MAX_ALLOWED_LEVEL: usize = 4;

struct Config {
    max_allowed_level: usize,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let max_allowed_level = ctx
            .config
            .get_int(DiagnosticCode::NestedStatements, "maxAllowedLevel")
            .unwrap_or(DEFAULT_MAX_ALLOWED_LEVEL as i64) as usize;

        Self { max_allowed_level }
    }
}

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::NestedStatements` is encountered.
/// Applies configuration filtering (maxAllowedLevel).
pub fn from_hir(
    _method_name: &str,
    depth: u32,
    _is_function: bool,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::NestedStatements;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let config = Config::from_context(ctx);
    if (depth as usize) <= config.max_allowed_level {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Управляющие конструкции не должны быть вложены слишком глубоко".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        assert_diagnostic_range, check_hir_diagnostic, check_hir_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_no_nesting() {
        let code = r#"Процедура Тест()
    Если А Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_max_nesting_no_violation() {
        let code = r#"Процедура Тест()
Если а Тогда
    Если б Тогда
        Если в Тогда
            Если г Тогда
            КонецЕсли;
        КонецЕсли;
    КонецЕсли;
КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();
        assert_eq!(diagnostics.len(), 0, "4 levels is the maximum allowed");
    }

    #[test]
    fn test_exceed_max_nesting() {
        let code = r#"Процедура Тест()
Если а Тогда
    Если б Тогда
        Если в Тогда
            Если г Тогда
                Если д Тогда
                КонецЕсли;
            КонецЕсли;
        КонецЕсли;
    КонецЕсли;
КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();
        assert_eq!(diagnostics.len(), 1, "5 levels exceeds limit of 4");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/NestedStatementsDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();

        assert_eq!(diagnostics.len(), 2, "Should match Java implementation (2 diagnostics)");

        assert_diagnostic_range(code, diagnostics[0], 35, 8, 12);
        assert_diagnostic_range(code, diagnostics[1], 50, 6, 10);
    }

    #[test]
    fn test_custom_max_level() {
        let code = include_str!("../../test_data/NestedStatementsDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config
            .parameters
            .insert(DiagnosticCode::NestedStatements, serde_json::json!({ "maxAllowedLevel": 6 }));

        let diagnostics =
            check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();

        assert_eq!(diagnostics.len(), 1, "With maxAllowedLevel=6, only 7-level nesting triggers");
        assert_diagnostic_range(code, diagnostics[0], 50, 6, 10);
    }

    #[test]
    fn test_hir_detection() {
        let code = r#"
Процедура Тест()
    Если а Тогда
        Если б Тогда
            Если в Тогда
                Если г Тогда
                    Если д Тогда
                    КонецЕсли;
                КонецЕсли;
            КонецЕсли;
        КонецЕсли;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let nested: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();

        assert_eq!(nested.len(), 1, "HIR should detect 1 NestedStatements (depth 5)");
    }
}
