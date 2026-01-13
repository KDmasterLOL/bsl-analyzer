//! NestedTernaryOperator diagnostic.
//!
//! Detects nested usage of ternary operator `?(condition, true_value, false_value)`.
//!
//! ## Why?
//! Nested ternary operators are hard to read and understand, especially when:
//! - Used inside IF/ELSIF conditions (alternative: extract to variable)
//! - Nested within another ternary operator (alternative: use IF statement)
//!
//! ## Bad practice
//! ```bsl
//! // Ternary in IF condition - hard to read
//! Если ?(Условие1, А, Б) = 1 Тогда
//!     // ...
//! КонецЕсли;
//!
//! // Nested ternary - very confusing
//! Результат = ?(Условие1, ?(Условие2, 1, 2), ?(Условие3, 3, 4));
//! ```
//!
//! ## Good practice
//! Extract complex conditions or use explicit IF statements:
//! ```bsl
//! // Extract ternary to variable
//! Значение = ?(Условие1, А, Б);
//! Если Значение = 1 Тогда
//!     // ...
//! КонецЕсли;
//!
//! // Replace nested ternary with IF statement
//! Если Условие1 Тогда
//!     Если Условие2 Тогда
//!         Результат = 1;
//!     Иначе
//!         Результат = 2;
//!     КонецЕсли;
//! Иначе
//!     Результат = 3;
//! КонецЕсли;
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** WARNING
//! - **Tags:** BADPRACTICE (concept)
//! - **Minutes to fix:** 10
//!
//! ## Implementation
//! Ported from: NestedTernaryOperatorDiagnostic.java (bsl-language-server)
//!
//! Uses node-based API pattern from rust-analyzer. Checks three cases:
//! 1. `IF_STMT` - finds ternary operators in IF conditions
//! 2. `ELSIF_CLAUSE` - finds ternary operators in ELSIF conditions
//! 3. `TERNARY_EXPR` - finds nested ternary operators within another ternary
//!
//! ## Note
//! This diagnostic uses AST (not HIR) because it checks structural properties only.
//! AST descendant traversal is simpler than HIR arena traversal for pattern matching.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode};

/// Check a single syntax node for nested ternary operators (node-based API).
///
/// This is called from collect_text_diagnostics() for each node in single AST pass.
/// Pattern from rust-analyzer: crates/ide-diagnostics/src/handlers/*.rs
///
/// ## Cases checked
/// 1. **IF_STMT** - Reports ternary operators in IF conditions
/// 2. **ELSIF_CLAUSE** - Reports ternary operators in ELSIF conditions
/// 3. **TERNARY_EXPR** - Reports nested ternary operators within another ternary
pub fn check_node(node: &SyntaxNode, acc: &mut Vec<Diagnostic>, ctx: &DiagnosticsContext) {
    if ctx.config.is_disabled(DiagnosticCode::NestedTernaryOperator) {
        return;
    }

    match node.kind() {
        // Case 1: Ternary in IF condition
        SyntaxKind::IF_STMT => {
            if let Some(condition) = find_if_condition(node) {
                find_and_report_ternaries(&condition, acc);
            }
        }
        // Case 2: Ternary in ELSIF condition
        SyntaxKind::ELSIF_CLAUSE => {
            if let Some(condition) = find_elsif_condition(node) {
                find_and_report_ternaries(&condition, acc);
            }
        }
        // Case 3: Nested ternary within another ternary
        SyntaxKind::TERNARY_EXPR => {
            for nested in node.descendants().skip(1) {
                if nested.kind() == SyntaxKind::TERNARY_EXPR {
                    acc.push(make_diagnostic(&nested));
                }
            }
        }
        _ => {}
    }
}

/// Main entry point for NestedTernaryOperator diagnostic.
///
/// Traverses AST and calls `check_node()` for each node.
/// This is the traditional entry point for diagnostics that don't use text-based API yet.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("NestedTernaryOperator::check").entered();

    if ctx.config.is_disabled(DiagnosticCode::NestedTernaryOperator) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        check_node(&node, &mut diagnostics, ctx);
    }

    tracing::debug!(count = diagnostics.len(), "NestedTernaryOperator diagnostics found");

    diagnostics
}

/// Find the condition expression of an IF statement.
///
/// Structure: `IF_STMT` → `EXPR` (condition) → `THEN` → ...
fn find_if_condition(if_stmt: &SyntaxNode) -> Option<SyntaxNode> {
    if_stmt.children().find(|n| n.kind() == SyntaxKind::EXPR)
}

/// Find the condition expression of an ELSIF clause.
///
/// Structure: `ELSIF_CLAUSE` → `EXPR` (condition) → ...
fn find_elsif_condition(elsif_clause: &SyntaxNode) -> Option<SyntaxNode> {
    elsif_clause.children().find(|n| n.kind() == SyntaxKind::EXPR)
}

/// Find and report all ternary operators within an expression tree.
///
/// Used to detect ternary operators in IF/ELSIF conditions.
fn find_and_report_ternaries(condition: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    for node in condition.descendants() {
        if node.kind() == SyntaxKind::TERNARY_EXPR {
            diagnostics.push(make_diagnostic(&node));
        }
    }
}

/// Create a diagnostic for a nested ternary operator.
fn make_diagnostic(node: &SyntaxNode) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::NestedTernaryOperator,
        message: "Не рекомендуется использовать вложенный тернарный оператор".to_string(),
        severity: Severity::Warning,
        range: node.text_range(),
        tags: vec![],
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range_multiline, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/NestedTernaryOperatorDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            4,
            "Should find exactly 4 diagnostics (matching Java implementation)"
        );

        assert_diagnostic_range_multiline(code, &diagnostics[0], 2, 13, 8, 14);
        assert_diagnostic_range_multiline(code, &diagnostics[1], 13, 5, 13, 50);
        assert_diagnostic_range_multiline(code, &diagnostics[2], 13, 73, 13, 104);
        assert_diagnostic_range_multiline(code, &diagnostics[3], 22, 12, 22, 71);
    }

    #[test]
    fn test_no_diagnostic_for_simple_ternary() {
        let code = r#"
Результат = ?(Условие, Истина, Ложь);
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert!(diagnostics.is_empty(), "Simple ternary should not trigger diagnostic");
    }

    #[test]
    fn test_nested_ternary_in_assignment() {
        let code = r#"
Результат = ?(Условие1, ?(Условие2, 1, 2), 3);
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Nested ternary in assignment should trigger diagnostic");
    }

    #[test]
    fn test_ternary_in_if_condition() {
        let code = r#"
Если ?(А, Б, В) = 1 Тогда
    Х = 1;
КонецЕсли;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Ternary in if condition should trigger diagnostic");
    }

    #[test]
    fn test_ternary_in_elsif_condition() {
        let code = r#"
Если Условие Тогда
    Х = 1;
ИначеЕсли ?(А, Б, В) = 1 Тогда
    Х = 2;
КонецЕсли;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Ternary in elsif condition should trigger diagnostic");
    }

    #[test]
    fn test_disabled() {
        let code = r#"
Результат = ?(Условие1, ?(Условие2, 1, 2), 3);
"#;
        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::NestedTernaryOperator);

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert!(diagnostics.is_empty(), "Disabled diagnostic should not find anything");
    }
}
