//! ExcessiveAutoTestCheck diagnostic.
//!
//! Detects excessive checks for deprecated "АвтоТест" parameter.
//!
//! Standard 772 "Interaction with automated testing tools" has been deprecated.
//! If-statements that check for "АвтоТест" and immediately return are no longer needed.
//!
//! ## Bad practice
//! ```bsl
//! Процедура ПриСозданииНаСервере()
//!     Если Параметры.Свойство("АвтоТест") Тогда
//!         Возврат;
//!     КонецЕсли;
//! КонецПроцедуры
//! ```
//!
//! ## Why deprecated
//! The 1C standard 772 requiring "АвтоТест" checks has been revoked.
//! This pattern should be removed from code.
//!
//! ## Sources
//! - **Java:** bsl-language-server/ExcessiveAutoTestCheckDiagnostic.java
//! - **Rust:** bsl-language-server-rust/rules/excessive_auto_test_check.rs

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use regex::Regex;
use std::sync::OnceLock;
use syntax::{SyntaxKind, SyntaxNode, TextRange};

/// Pattern to match AutoTest checks in condition expressions.
///
/// Matches 4 variants:
/// 1. `.Свойство("АвтоТест")` (Russian property call)
/// 2. `= "АвтоТест"` (Russian equality, with optional whitespace)
/// 3. `.Property("AutoTest")` (English property call)
/// 4. `= "AutoTest"` (English equality, with optional whitespace)
fn autotest_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(
            r#"(\.Свойство\("АвтоТест"\)|=\s*"АвтоТест"|\.Property\("AutoTest"\)|=\s*"AutoTest")"#,
        )
        .expect("Invalid regex pattern")
    })
}

/// Check if statement list contains only a return statement.
///
/// Returns true if the statement list has exactly one child that is a RETURN_STMT.
/// Ignores whitespace and comments.
fn has_only_return_statement(stmt_list: &SyntaxNode) -> bool {
    let statements: Vec<_> = stmt_list
        .children()
        .filter(|n| !matches!(n.kind(), SyntaxKind::WHITESPACE | SyntaxKind::COMMENT))
        .collect();

    statements.len() == 1 && statements[0].kind() == SyntaxKind::RETURN_STMT
}

/// Check if if-statement should be flagged (optimized version).
///
/// Returns Some(TextRange) if the if-statement:
/// 1. Condition matches AutoTest pattern
/// 2. Body contains only a return statement
fn check_if_statement_optimized(
    if_node: &SyntaxNode,
    return_stmts_by_parent: &std::collections::HashMap<syntax::TextSize, Vec<SyntaxNode>>,
) -> Option<TextRange> {
    let pattern = autotest_pattern();

    // First, try to find STMT_LIST among direct children (normal case)
    let stmt_list_candidate = if_node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);

    // If no STMT_LIST found among children, or if it doesn't have only return,
    // try to find RETURN_STMT using pre-collected map (workaround for parser bug with `=`)
    if stmt_list_candidate.is_none()
        || !has_only_return_statement(stmt_list_candidate.as_ref().unwrap())
    {
        // Check if there's an ERROR node (indicates parser issue)
        let has_error = if_node.children().any(|n| n.kind() == SyntaxKind::ERROR);

        if has_error {
            // Workaround: Count RETURN_STMT nodes that are descendants of this IF_STMT
            // by checking if any pre-collected returns are within this if_node's range
            let if_range = if_node.text_range();
            let return_count = return_stmts_by_parent
                .values()
                .flatten()
                .filter(|r| if_range.contains_range(r.text_range()))
                .count();

            // Should have exactly one RETURN_STMT for this diagnostic
            if return_count != 1 {
                return None;
            }

            // Check pattern in full if-statement text
            let if_text = if_node.text().to_string();
            if pattern.is_match(&if_text) {
                return Some(if_node.text_range());
            }
        }
        return None;
    }

    // Normal case: STMT_LIST found and has only return
    let if_text = if_node.text().to_string();
    if pattern.is_match(&if_text) {
        return Some(if_node.text_range());
    }

    None
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::ExcessiveAutoTestCheck) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Optimized: Collect IF_STMT nodes and RETURN_STMT nodes in one pass
    let mut if_stmts = Vec::new();
    let mut return_stmts_by_parent = std::collections::HashMap::new();

    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::IF_STMT => {
                if_stmts.push(node);
            }
            SyntaxKind::RETURN_STMT => {
                // Track return statements for ERROR case workaround
                if let Some(parent) = node.parent() {
                    return_stmts_by_parent
                        .entry(parent.text_range().start())
                        .or_insert_with(Vec::new)
                        .push(node);
                }
            }
            _ => {}
        }
    }

    // Check each IF_STMT
    for if_node in if_stmts {
        if let Some(range) = check_if_statement_optimized(&if_node, &return_stmts_by_parent) {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::ExcessiveAutoTestCheck,
                message: "Excessive check for deprecated 'АвтоТест' parameter".to_string(),
                severity: Severity::Information,
                range,
                tags: vec![],
                fixes: vec![],
            });
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::*;

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/ExcessiveAutoTestCheckDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        // Should find 6 diagnostics (matching Java implementation)
        assert_eq!(diagnostics.len(), 6, "Expected 6 diagnostics");

        // Assert exact ranges (adapted for Rowan parser)
        // Note: Ranges differ slightly from Java due to different parser implementations
        // Java ranges (0-indexed): (3,4,7,13), (14,4,16,13), (22,4,26,13), (46,4,48,9), (54,4,56,9), (62,4,66,9)
        // Our ranges (with +1 line offset for comment, and adjusted end columns):
        assert_diagnostic_range_multiline(code, &diagnostics[0], 4, 4, 8, 14);
        assert_diagnostic_range_multiline(code, &diagnostics[1], 15, 4, 17, 14);
        assert_diagnostic_range_multiline(code, &diagnostics[2], 23, 4, 27, 14);
        assert_diagnostic_range_multiline(code, &diagnostics[3], 47, 4, 49, 10);
        assert_diagnostic_range_multiline(code, &diagnostics[4], 55, 4, 57, 10);
        assert_diagnostic_range_multiline(code, &diagnostics[5], 63, 4, 67, 10);
    }

    #[test]
    fn test_russian_property() {
        let code = r#"
Процедура Тест()
    Если Параметры.Свойство("АвтоТест") Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_english_property() {
        let code = r#"
Procedure Test()
    If Parameters.Property("AutoTest") Then
        Return;
    EndIf;
EndProcedure
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_russian_equality() {
        let code = r#"
Процедура Тест()
    Если Переменная = "АвтоТест" Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_english_equality() {
        let code = r#"
Procedure Test()
    If Variable = "AutoTest" Then
        Return;
    EndIf;
EndProcedure
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");
    }

    #[test]
    fn test_multiple_statements_no_error() {
        let code = r#"
Процедура Тест()
    Если Параметры.Свойство("АвтоТест") Тогда
        Действие();
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should NOT flag when multiple statements");
    }

    #[test]
    fn test_no_return_no_error() {
        let code = r#"
Процедура Тест()
    Если Параметры.Свойство("АвтоТест") Тогда
        Действие();
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should NOT flag when no return");
    }

    #[test]
    fn test_no_autotest_check() {
        let code = r#"
Процедура Тест()
    Если Параметры.Свойство("ДругойПараметр") Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should NOT flag without AutoTest");
    }
}
