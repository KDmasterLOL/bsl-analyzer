//! IfElseDuplicatedCondition diagnostic
//!
//! Detects identical conditions in if/elsif chains.
//!
//! ## Why?
//! When if/elsif branches have identical conditions, the second branch will never
//! be executed. This usually indicates a copy-paste error or logic mistake.
//!
//! ## Bad practice
//! ```bsl
//! Если п = 1 Тогда
//!     т = 1;
//! ИначеЕсли п = 2 Тогда
//!     т = 2;
//! ИначеЕсли п = 1 Тогда    // Will never execute!
//!     т = 3;
//! КонецЕсли;
//! ```
//!
//! ## Good practice
//! ```bsl
//! Если п = 1 Тогда
//!     т = 1;
//! ИначеЕсли п = 2 Тогда
//!     т = 2;
//! ИначеЕсли п = 3 Тогда    // Fixed condition
//!     т = 3;
//! КонецЕсли;
//! ```
//!
//! ## Implementation
//!
//! Migrated to HIR-based collection (rust-analyzer pattern).
//!
//! Source: bsl-language-server/src/main/java/.../diagnostics/IfElseDuplicatedConditionDiagnostic.java
//! Source: bsl-language-server-rust/crates/bsl-diagnostics/src/rules/if_else_duplicated_condition.rs

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when IfElseDuplicatedCondition diagnostic is emitted during lowering.
pub fn from_hir(
    first_occurrence_index: usize,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::IfElseDuplicatedCondition;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции {})",
            first_occurrence_index + 1
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;

    #[test]
    fn test_simple_duplicate() {
        let code = r#"
Процедура Тест()
    Если x = 1 Тогда
        т = 1;
    ИначеЕсли x = 2 Тогда
        т = 2;
    ИначеЕсли x = 1 Тогда
        т = 3;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        assert_eq!(dupl_diags.len(), 1, "Expected 1 diagnostic for duplicate x = 1 condition");
    }

    #[test]
    fn test_no_duplicates() {
        let code = r#"
Процедура Тест()
    Если x = 1 Тогда
        т = 1;
    ИначеЕсли x = 2 Тогда
        т = 2;
    ИначеЕсли x = 3 Тогда
        т = 3;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        assert_eq!(dupl_diags.len(), 0, "Should not report different conditions");
    }

    #[test]
    fn test_case_insensitive_variables() {
        let code = r#"
Процедура Тест()
    Если п = 1 Тогда
        т = 1;
    ИначеЕсли П = 1 Тогда
        т = 2;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        assert_eq!(
            dupl_diags.len(),
            1,
            "Should detect п = 1 and П = 1 as identical (case-insensitive)"
        );
    }

    #[test]
    fn test_whitespace_normalization() {
        let code = r#"
Процедура Тест()
    Если п = 1 Тогда
        т = 1;
    ИначеЕсли П     =   1 Тогда
        т = 2;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        assert_eq!(
            dupl_diags.len(),
            1,
            "Should detect conditions as identical despite whitespace differences"
        );
    }

    #[test]
    fn test_string_case_sensitive() {
        let code = r#"
Процедура Тест()
    Если (Знак = "Ё") Тогда
        Возврат 0;
    ИначеЕсли (Знак = "ё") Тогда
        Возврат 1;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        assert_eq!(dupl_diags.len(), 0, "String literals should be case-sensitive: 'Ё' != 'ё'");
    }

    #[test]
    fn test_string_same_case() {
        let code = r#"
Процедура Тест()
    Если (Знак = "ё") Тогда
        Возврат 0;
    ИначеЕсли (Знак = "ё") Тогда
        Возврат 1;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        assert_eq!(dupl_diags.len(), 1, "Should detect identical string literal conditions");
    }

    #[test]
    fn test_nested_if_independent() {
        let code = r#"
Процедура Тест()
    Если п = 1 Тогда
        Если п = 2 Тогда
            т = 1;
        ИначеЕсли п = 2 Тогда
            т = 2;
        КонецЕсли;
    ИначеЕсли п = 1 Тогда
        т = 3;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        // Should find 2 diagnostics:
        // 1. Inner if: п = 2 duplicate
        // 2. Outer if: п = 1 duplicate
        assert_eq!(dupl_diags.len(), 2, "Should detect duplicates in both outer and inner if");
    }

    #[test]
    fn test_comprehensive_fixture() {
        let code = include_str!("../../test_data/IfElseDuplicatedConditionDiagnostic.bsl");

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        let found_count = dupl_diags.len();

        // Show which lines we found
        let mut found_lines: Vec<u32> = dupl_diags
            .iter()
            .map(|d| {
                let (line, _, _, _) = range_to_line_col(code, d.range);
                line
            })
            .collect();
        found_lines.sort();

        // Expected diagnostics on duplicate conditions:
        // Lines 4, 6, 10 - duplicates of п = 1 (first group) -> 2 diagnostics (6, 10)
        // Lines 18, 28 - duplicates of п = 1 (second group, outer if) -> 1 diagnostic (28)
        // Lines 21, 23 - duplicates of п = 2 (nested if) -> 1 diagnostic (23)
        // Lines 42, 44, 46 - duplicates of (Знак = "ё") -> 2 diagnostics (44, 46)
        // Total: 2 + 1 + 1 + 2 = 6 diagnostics

        // Note: Java approach reports 4 diagnostics (one per group with relatedInformation)
        // Our approach reports 6 diagnostics (one per duplicate, not counting first occurrence)
        // Both are valid, ours is more explicit

        assert_eq!(
            found_count, 6,
            "Should find 6 diagnostics (one per duplicate condition), found {}",
            found_count
        );

        // Verify expected lines (0-indexed in fixture output)
        // Lines 5, 9, 22, 27, 43, 45 correspond to 1-indexed 6, 10, 23, 28, 44, 46
        let expected_lines = vec![5, 9, 22, 27, 43, 45];
        assert_eq!(
            found_lines, expected_lines,
            "Diagnostics should be on lines {:?}",
            expected_lines
        );
    }
}
