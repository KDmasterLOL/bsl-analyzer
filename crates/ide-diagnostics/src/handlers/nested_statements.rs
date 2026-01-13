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

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use syntax::{SyntaxKind, SyntaxNode};

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

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("NestedStatements::check").entered();

    if ctx.config.is_disabled(DiagnosticCode::NestedStatements) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();
    traverse_recursive(&root, 0, config.max_allowed_level, &mut diagnostics);

    tracing::debug!(count = diagnostics.len(), "NestedStatements diagnostics found");

    diagnostics
}

fn is_nesting_statement(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
    )
}

/// Traverse AST recursively, tracking nesting depth.
///
/// Returns `true` if this subtree contains nesting statements (IF, WHILE, FOR, TRY).
/// This flag propagation eliminates the need for `node.descendants()` calls,
/// reducing complexity from O(n²) to O(n).
fn traverse_recursive(
    node: &SyntaxNode,
    current_depth: usize,
    max_level: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let is_nesting = is_nesting_statement(node.kind());
    let new_depth = if is_nesting { current_depth + 1 } else { current_depth };

    // Recursively check all children, tracking if any contain nesting statements
    let mut has_nested_child = false;
    for child in node.children() {
        let child_has_nesting = traverse_recursive(&child, new_depth, max_level, diagnostics);
        has_nested_child = has_nested_child || child_has_nesting;
    }

    // Report diagnostic if this is a leaf nesting statement exceeding threshold
    if is_nesting && !has_nested_child && new_depth > max_level {
        let keyword_range = get_first_keyword_range(node);
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::NestedStatements,
            message: "Управляющие конструкции не должны быть вложены слишком глубоко".to_string(),
            severity: Severity::Critical,
            range: keyword_range,
            tags: vec![],
            fixes: vec![],
        });
    }

    // Propagate up: this node or its children contain nesting statements
    is_nesting || has_nested_child
}

fn get_first_keyword_range(node: &SyntaxNode) -> TextRange {
    node.children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| {
            matches!(
                t.kind(),
                SyntaxKind::KW_IF | SyntaxKind::KW_WHILE | SyntaxKind::KW_FOR | SyntaxKind::KW_TRY
            )
        })
        .map(|t| t.text_range())
        .unwrap_or_else(|| node.text_range())
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};

    #[test]
    fn test_no_nesting() {
        let code = r#"Процедура Тест()
    Если А Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_max_nesting_no_violation() {
        let code = r#"Если а Тогда
    Если б Тогда
        Если в Тогда
            Если г Тогда
            КонецЕсли;
        КонецЕсли;
    КонецЕсли;
КонецЕсли;"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "4 levels is the maximum allowed");
    }

    #[test]
    fn test_exceed_max_nesting() {
        let code = r#"Если а Тогда
    Если б Тогда
        Если в Тогда
            Если г Тогда
                Если д Тогда
                КонецЕсли;
            КонецЕсли;
        КонецЕсли;
    КонецЕсли;
КонецЕсли;"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "5 levels exceeds limit of 4");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/NestedStatementsDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 2, "Should match Java implementation (2 diagnostics)");

        assert_diagnostic_range(code, &diagnostics[0], 35, 8, 12);
        assert_diagnostic_range(code, &diagnostics[1], 50, 6, 10);
    }

    #[test]
    fn test_custom_max_level() {
        let code = include_str!("../../test_data/NestedStatementsDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config
            .parameters
            .insert(DiagnosticCode::NestedStatements, serde_json::json!({ "maxAllowedLevel": 6 }));

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert_eq!(diagnostics.len(), 1, "With maxAllowedLevel=6, only 7-level nesting triggers");
        assert_diagnostic_range(code, &diagnostics[0], 50, 6, 10);
    }
}
