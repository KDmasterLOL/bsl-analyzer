//! IfConditionComplexity diagnostic.
//!
//! Detects overly complex if conditions with too many boolean operations.
//!
//! ## Why?
//! Complex if conditions are hard to understand:
//! - Reduced readability
//! - Difficult to debug
//! - Error-prone
//! - Should be extracted to variables
//!
//! ## Bad practice
//! ```bsl
//! Если А И Б ИЛИ В И Г Тогда  // Too complex!
//!     ВыполнитьДействие();
//! КонецЕсли;
//! ```
//!
//! ## Good practice
//! ```bsl
//! УсловиеВыполнено = (А И Б) ИЛИ (В И Г);
//! Если УсловиеВыполнено Тогда
//!     ВыполнитьДействие();
//! КонецЕсли;
//! ```
//!
//! ## Implementation
//!
//! Migrated to HIR-based collection (rust-analyzer pattern).
//!
//! Ported from:
//! - IfConditionComplexityDiagnostic.java (bsl-language-server)
//! - if_condition_complexity.rs (bsl-language-server-rust)
//!
//! Adapted to use Rowan SyntaxNode during HIR lowering.
//!
//! ### Key algorithm:
//! - Java: `Trees.findAllRuleNodes(expression, BSLParser.RULE_boolOperation).size() + 1`
//! - Rust: Count all BINARY_EXPR nodes with AND/OR operators + 1
//! - Default max complexity: 3
//!
//! ### Diagnostic range:
//! - Java: `diagnosticStorage.addDiagnostic(expression)` - entire expression
//! - Rust: Same - entire expression range

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Default maximum if condition complexity
const DEFAULT_MAX_IF_CONDITION_COMPLEXITY: usize = 3;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when IfConditionComplexity diagnostic is emitted during lowering.
pub fn from_hir(
    complexity: usize,
    max_complexity_default: usize,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::IfConditionComplexity) {
        return None;
    }

    // Get maxIfConditionComplexity parameter from config (default: 3)
    let max_complexity = ctx
        .config
        .get_int(DiagnosticCode::IfConditionComplexity, "maxIfConditionComplexity")
        .map(|v| v as usize)
        .unwrap_or(DEFAULT_MAX_IF_CONDITION_COMPLEXITY);

    // Re-check against user config (lowering used default threshold)
    if complexity <= max_complexity {
        return None;
    }

    // Update max_complexity in message to reflect actual config value
    // (lowering emitted with default, we use user config)
    let _ = max_complexity_default; // Silence unused warning

    Some(Diagnostic {
        code: DiagnosticCode::IfConditionComplexity,
        message: format!(
            "Условие имеет сложность {} (максимум {}). Упростите условие или вынесите части в переменные.",
            complexity, max_complexity
        ),
        severity: Severity::Warning,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::{DiagnosticCode, Severity};

    /// Test simple condition (should pass)
    #[test]
    fn test_simple_condition() {
        let code = r#"Процедура Тест()
    Если А И Б Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should NOT detect - complexity = 2 (1 AND + 1 = 2)
        assert_eq!(if_diags.len(), 0);
    }

    /// Test at threshold (should pass)
    #[test]
    fn test_at_threshold() {
        let code = r#"Процедура Тест()
    Если А И Б ИЛИ В Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should NOT detect - complexity = 3 (2 ops: AND + OR = 2, complexity = 2+1 = 3)
        assert_eq!(if_diags.len(), 0);
    }

    /// Test complex condition (should fail)
    #[test]
    fn test_complex_condition() {
        let code = r#"Процедура Тест()
    Если А И Б ИЛИ В И Г Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should detect - complexity = 4 (3 ops: AND, OR, AND = 3, complexity = 3+1 = 4)
        assert_eq!(if_diags.len(), 1);
        assert_eq!(if_diags[0].code, DiagnosticCode::IfConditionComplexity);
        assert_eq!(if_diags[0].severity, Severity::Warning);
        assert!(if_diags[0].message.contains("сложность 4"));
        assert!(if_diags[0].message.contains("максимум 3"));
    }

    /// Test elsif clause
    #[test]
    fn test_elseif_complex() {
        let code = r#"Процедура Тест()
    Если А Тогда
        Сообщить("1");
    ИначеЕсли Б И В ИЛИ Г И Д Тогда
        Сообщить("2");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should detect in elseif - complexity = 4
        assert_eq!(if_diags.len(), 1);
        assert_eq!(if_diags[0].code, DiagnosticCode::IfConditionComplexity);
    }

    /// Test English keywords
    #[test]
    fn test_english_condition() {
        let code = r#"Procedure Test()
    If A And B Or C And D Then
        Message("OK");
    EndIf;
EndProcedure"#;

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Should detect - complexity = 4
        assert_eq!(if_diags.len(), 1);
    }

    /// Integration test matching Java test structure
    ///
    /// Based on IfConditionComplexityDiagnosticTest.java
    /// Uses the same test file: IfConditionComplexityDiagnostic.bsl
    ///
    /// Expected diagnostics (from Java test):
    /// - Line 2, col 5 → line 10, col 51
    /// - Line 27, col 6 → line 30, col 60
    /// - Line 45, col 5 → line 48, col 36
    /// - Line 51, col 10 → line 57, col 37
    #[test]
    fn test_if_condition_complexity() {
        let code = include_str!("../../test_data/IfConditionComplexityDiagnostic.bsl");

        let diagnostics = check_hir_diagnostic(code);
        let if_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfConditionComplexity)
            .collect();

        // Java test expects: assertThat(diagnostics).hasSize(4);
        assert_eq!(if_diags.len(), 4, "Expected 4 diagnostics");

        // Verify each diagnostic range matches Java implementation
        // Java uses 0-based line/column indexing
        assert_diagnostic_range_multiline(code, if_diags[0], 2, 5, 10, 51);
        assert_diagnostic_range_multiline(code, if_diags[1], 27, 6, 30, 60);
        assert_diagnostic_range_multiline(code, if_diags[2], 45, 5, 48, 36);
        assert_diagnostic_range_multiline(code, if_diags[3], 51, 10, 57, 37);
    }
}
