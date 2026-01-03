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
//! Ported from:
//! - IfConditionComplexityDiagnostic.java (bsl-language-server)
//! - if_condition_complexity.rs (bsl-language-server-rust)
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.
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
use syntax::{SyntaxKind, SyntaxNode, TextSize};

/// Default maximum if condition complexity
const DEFAULT_MAX_IF_CONDITION_COMPLEXITY: usize = 3;

/// Runs the IfConditionComplexity diagnostic.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Check if diagnostic is disabled
    if ctx.config.is_disabled(DiagnosticCode::IfConditionComplexity) {
        return Vec::new();
    }

    // Get maxIfConditionComplexity parameter (default: 3)
    let max_complexity = ctx
        .config
        .get_int(DiagnosticCode::IfConditionComplexity, "maxIfConditionComplexity")
        .map(|v| v as usize)
        .unwrap_or(DEFAULT_MAX_IF_CONDITION_COMPLEXITY);

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    // Find all if statements and elsif clauses
    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::IF_STMT => {
                // Check if branch condition
                if let Some(expr) = find_if_condition(&node) {
                    if let Some(diag) = check_expression(&expr, max_complexity) {
                        diagnostics.push(diag);
                    }
                }
            }
            SyntaxKind::ELSIF_CLAUSE => {
                // Check elsif branch condition
                if let Some(expr) = find_elsif_condition(&node) {
                    if let Some(diag) = check_expression(&expr, max_complexity) {
                        diagnostics.push(diag);
                    }
                }
            }
            _ => {}
        }
    }

    diagnostics
}

/// Find condition expression in IF_STMT
/// Structure: IF_STMT → EXPR → ...
fn find_if_condition(if_stmt: &SyntaxNode) -> Option<SyntaxNode> {
    // The first EXPR child is the condition
    if_stmt.children().find(|n| n.kind() == SyntaxKind::EXPR)
}

/// Find condition expression in ELSIF_CLAUSE
/// Structure: ELSIF_CLAUSE → EXPR → ...
fn find_elsif_condition(elsif_clause: &SyntaxNode) -> Option<SyntaxNode> {
    // The first EXPR child is the condition
    elsif_clause.children().find(|n| n.kind() == SyntaxKind::EXPR)
}

/// Check expression complexity
///
/// Complexity = number of boolean operations (AND/OR) + 1
/// This matches Java's `Trees.findAllRuleNodes(expression, BSLParser.RULE_boolOperation).size() + 1`
fn check_expression(expr: &SyntaxNode, max_complexity: usize) -> Option<Diagnostic> {
    let bool_op_count = count_bool_operations(expr);
    let complexity = bool_op_count + 1;

    if complexity > max_complexity {
        // Trim trailing whitespace from expression range to match Java behavior
        // Java ANTLR doesn't include trailing whitespace in expression nodes
        let range = trim_trailing_whitespace(expr);

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
    } else {
        None
    }
}

/// Trim trailing whitespace from a node's text range
///
/// Java ANTLR parser doesn't include trailing whitespace in expression nodes,
/// but Rowan CST includes all tokens including whitespace.
/// This function trims trailing whitespace to match Java behavior.
fn trim_trailing_whitespace(node: &SyntaxNode) -> ide_db::TextRange {
    let text = node.text().to_string();
    let trimmed = text.trim_end();
    let trimmed_len = trimmed.len();
    let original_len = text.len();

    if trimmed_len == original_len {
        // No trailing whitespace
        node.text_range()
    } else {
        // Trim trailing whitespace from range
        let start = node.text_range().start();
        let end = start + TextSize::from(trimmed_len as u32);
        TextRange::new(start, end)
    }
}

/// Count boolean operations (AND/OR) in expression
///
/// Counts all BINARY_EXPR nodes that have AND or OR operator
fn count_bool_operations(expr: &SyntaxNode) -> usize {
    let mut count = 0;

    // Traverse all descendants looking for BINARY_EXPR with AND/OR
    for node in expr.descendants() {
        if node.kind() == SyntaxKind::BINARY_EXPR {
            // Check if it has AND or OR operator
            if has_bool_operator(&node) {
                count += 1;
            }
        }
    }

    count
}

/// Check if BINARY_EXPR has a boolean operator (AND/OR)
fn has_bool_operator(binary_expr: &SyntaxNode) -> bool {
    // Look for KW_AND or KW_OR token in children
    binary_expr.children_with_tokens().any(|child| {
        child
            .as_token()
            .is_some_and(|tok| matches!(tok.kind(), SyntaxKind::KW_AND | SyntaxKind::KW_OR))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::assert_diagnostic_range_multiline, DiagnosticsConfig};
    use ide_db::RootDatabase;
    use std::sync::Arc;

    /// Helper to run diagnostic on test code
    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        use ide_db::base_db::SourceDatabase;
        use ide_db::RootDatabaseImpl;
        use test_fixture::Fixture;

        // Create fixture with test file
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        // Create database
        let mut db = RootDatabaseImpl::new();

        // Set file content in database from fixture
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        // Create diagnostics context
        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        // Run diagnostic
        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    /// Test simple condition (should pass)
    #[test]
    fn test_simple_condition() {
        let code = r#"Процедура Тест()
    Если А И Б Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);

        // Should NOT detect - complexity = 2 (1 AND + 1 = 2)
        assert_eq!(diagnostics.len(), 0);
    }

    /// Test at threshold (should pass)
    #[test]
    fn test_at_threshold() {
        let code = r#"Процедура Тест()
    Если А И Б ИЛИ В Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);

        // Should NOT detect - complexity = 3 (2 ops: AND + OR = 2, complexity = 2+1 = 3)
        assert_eq!(diagnostics.len(), 0);
    }

    /// Test complex condition (should fail)
    #[test]
    fn test_complex_condition() {
        let code = r#"Процедура Тест()
    Если А И Б ИЛИ В И Г Тогда
        Сообщить("OK");
    КонецЕсли;
КонецПроцедуры"#;

        let (diagnostics, _file_content) = check_diagnostic(code);

        // Should detect - complexity = 4 (3 ops: AND, OR, AND = 3, complexity = 3+1 = 4)
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::IfConditionComplexity);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert!(diagnostics[0].message.contains("сложность 4"));
        assert!(diagnostics[0].message.contains("максимум 3"));
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

        let (diagnostics, _) = check_diagnostic(code);

        // Should detect in elseif - complexity = 4
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::IfConditionComplexity);
    }

    /// Test English keywords
    #[test]
    fn test_english_condition() {
        let code = r#"Procedure Test()
    If A And B Or C And D Then
        Message("OK");
    EndIf;
EndProcedure"#;

        let (diagnostics, _) = check_diagnostic(code);

        // Should detect - complexity = 4
        assert_eq!(diagnostics.len(), 1);
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
        let code = include_str!("../../tests/fixtures/IfConditionComplexityDiagnostic.bsl");

        let (diagnostics, file_content) = check_diagnostic(code);

        // Java test expects: assertThat(diagnostics).hasSize(4);
        assert_eq!(diagnostics.len(), 4, "Expected 4 diagnostics");

        // Verify each diagnostic range matches Java implementation
        // Java uses 0-based line/column indexing
        assert_diagnostic_range_multiline(&file_content, &diagnostics[0], 2, 5, 10, 51);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[1], 27, 6, 30, 60);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[2], 45, 5, 48, 36);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[3], 51, 10, 57, 37);
    }
}
