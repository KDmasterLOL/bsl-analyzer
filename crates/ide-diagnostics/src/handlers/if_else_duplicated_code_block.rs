//! IfElseDuplicatedCodeBlock diagnostic
//!
//! Detects identical code blocks in if/elseif/else branches.
//!
//! ## Why?
//! When if/else branches contain identical code, the condition is meaningless
//! and the code should be simplified.
//!
//! ## Bad practice
//! ```bsl
//! Если Условие Тогда
//!     ПоказатьПредупреждение("Ошибка");
//!     Возврат;
//! Иначе
//!     ПоказатьПредупреждение("Ошибка");
//!     Возврат;
//! КонецЕсли;
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Remove the condition, keep the common code
//! ПоказатьПредупреждение("Ошибка");
//! Возврат;
//! ```
//!
//! ## Source
//! Source: bsl-language-server/src/main/java/.../diagnostics/IfElseDuplicatedCodeBlockDiagnostic.java
//! Source: bsl-language-server-rust/crates/bsl-diagnostics/src/rules/if_else_duplicated_code_block.rs

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::IfElseDuplicatedCodeBlock` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::IfElseDuplicatedCodeBlock) {
        return None;
    }
    Some(Diagnostic {
        code: DiagnosticCode::IfElseDuplicatedCodeBlock,
        message: "Ветки Если и Иначе содержат идентичный код".to_string(),
        severity: Severity::Warning,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        assert_diagnostic_range_multiline, check_hir_diagnostic, range_to_line_col,
    };
    use crate::DiagnosticCode;

    #[test]
    fn test_simple_if_else_duplicate() {
        let code = r#"Процедура Тест()
    Если x = 1 Тогда
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    Иначе
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        assert_eq!(diags.len(), 1, "Expected 1 diagnostic for duplicate if/else blocks");
    }

    #[test]
    fn test_different_blocks() {
        let code = r#"Процедура Тест()
    Если x = 1 Тогда
        ПоказатьПредупреждение("Ошибка 1");
    Иначе
        ПоказатьПредупреждение("Ошибка 2");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        assert_eq!(diags.len(), 0, "Should not report different blocks");
    }

    #[test]
    fn test_elsif_duplicate() {
        let code = r#"Процедура Тест()
    Если x = 1 Тогда
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    ИначеЕсли x = 2 Тогда
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        assert_eq!(diags.len(), 1, "Expected 1 diagnostic for duplicate if/elsif blocks");
    }

    #[test]
    fn test_empty_blocks_ignored() {
        let code = r#"Процедура Тест()
    Если x = 1 Тогда
    ИначеЕсли x = 2 Тогда
    Иначе
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        assert_eq!(diags.len(), 0, "Empty blocks should be ignored");
    }

    #[test]
    fn test_comprehensive_fixture() {
        let code = include_str!(
            "if_else_duplicated_code_block/fixtures/IfElseDuplicatedCodeBlockDiagnostic.bsl"
        );

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();

        // Java version finds 5 diagnostics
        let found_count = diags.len();
        eprintln!("Found {} diagnostics (Java expects 5)", found_count);

        // Show which lines we found
        let mut found_lines: Vec<u32> = diags
            .iter()
            .map(|d| {
                let (line, _, _, _) = range_to_line_col(code, d.range);
                line
            })
            .collect();
        found_lines.sort();
        eprintln!("Found on lines: {:?}", found_lines);
        eprintln!("Expected lines: 10, 27, 40, 41, 54");

        assert_eq!(
            found_count, 5,
            "Should find 5 diagnostics (100% Java compatibility), found {}",
            found_count
        );

        // Sort diagnostics by line number for consistent checking
        let mut sorted_diagnostics: Vec<_> = diags.into_iter().collect();
        sorted_diagnostics.sort_by_key(|d| {
            let (line, col, _, _) = range_to_line_col(code, d.range);
            (line, col)
        });

        // Debug: print actual ranges
        for (i, diag) in sorted_diagnostics.iter().enumerate() {
            let (start_line, start_col, end_line, end_col) = range_to_line_col(code, diag.range);
            eprintln!(
                "Diagnostic {}: Range({}, {}, {}, {})",
                i + 1,
                start_line,
                start_col,
                end_line,
                end_col
            );
        }

        // Test 1: Line 10 (simple if/else duplicate)
        // then-branch STMT_LIST starts on line 11
        assert_diagnostic_range_multiline(code, sorted_diagnostics[0], 10, 1, 12, 0);

        // Test 2: Line 27 (if/elsif duplicate)
        // then-branch STMT_LIST starts on line 28
        assert_diagnostic_range_multiline(code, sorted_diagnostics[1], 27, 1, 29, 0);

        // Test 3: Line 38 outer if with nested duplicates
        // then-branch STMT_LIST spans lines 40-49
        assert_diagnostic_range_multiline(code, sorted_diagnostics[2], 40, 1, 50, 0);

        // Test 4: Line 41 (nested inner if)
        // then-branch STMT_LIST starts on line 42
        assert_diagnostic_range_multiline(code, sorted_diagnostics[3], 41, 2, 43, 1);

        // Test 5: Line 54 (nested inner if in else branch)
        // then-branch STMT_LIST starts on line 55
        assert_diagnostic_range_multiline(code, sorted_diagnostics[4], 54, 2, 56, 1);
    }
}
