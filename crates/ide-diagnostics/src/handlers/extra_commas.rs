//! ExtraCommas diagnostic
//!
//! Detects trailing commas in function/method call argument lists.
//!
//! **Source (Java):** bsl-language-server/ExtraCommasDiagnostic.java
//! **Source (Rust tree-sitter):** bsl-language-server-rust/rules/extra_commas.rs
//! **Test file:** ExtraCommasDiagnostic.bsl
//!
//! ## Why?
//! Trailing commas in BSL function calls are syntax errors or cause unexpected behavior.
//! They reduce code readability and can lead to confusion with optional parameters.
//!
//! ## Bad practice
//! ```bsl
//! Результат = Метод(Парам1, Парам2,);     // Trailing comma
//! Результат = Метод(Парам1, Парам2,,,);   // Multiple trailing commas
//! ```
//!
//! ## Good practice
//! ```bsl
//! Результат = Метод(Парам1, Парам2);
//! Результат = Метод(Парам1, , Парам2);    // Empty arg is OK
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{NodeOrToken, SyntaxKind, SyntaxNode};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::ExtraCommas) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() == SyntaxKind::ARG_LIST {
            if let Some(comma_range) = find_trailing_comma(&node) {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::ExtraCommas,
                    message: "Trailing comma".to_string(),
                    severity: Severity::Critical,
                    range: comma_range,
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }
    }

    diagnostics
}

/// Find the first trailing comma in an ARG_LIST node.
/// Returns the TextRange of the first trailing comma, or None.
fn find_trailing_comma(arg_list: &SyntaxNode) -> Option<ide_db::TextRange> {
    // Collect all children_with_tokens into a Vec and iterate backwards
    let tokens: Vec<_> = arg_list.children_with_tokens().collect();
    let mut iter = tokens.iter().rev().filter(|element| !is_trivia(element));

    // First should be R_PAREN
    let r_paren = iter.next()?;
    if !matches!(r_paren, NodeOrToken::Token(t) if t.kind() == SyntaxKind::R_PAREN) {
        return None;
    }

    // Next should be either COMMA (bad) or expression/L_PAREN (good)
    let prev = iter.next()?;
    match prev {
        NodeOrToken::Token(token) if token.kind() == SyntaxKind::COMMA => Some(token.text_range()),
        _ => None,
    }
}

/// Check if an element is trivia (whitespace, newline, comment)
fn is_trivia(element: &NodeOrToken<SyntaxNode, syntax::SyntaxToken>) -> bool {
    matches!(
        element,
        NodeOrToken::Token(t) if matches!(
            t.kind(),
            SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range;
    use crate::{DiagnosticsConfig, DiagnosticsContext};
    use ide_db::base_db::SourceDatabase;
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let config = Rc::new(DiagnosticsConfig::default());
        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_extra_commas() {
        let code = include_str!("../../test_data/ExtraCommasDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 6, "Expected 6 diagnostics");

        // Line 9 (0-indexed line 8): Метод1(Парам1, , Парам2,)
        assert_diagnostic_range(code, &diagnostics[0], 8, 35, 36);

        // Line 10: Метод2(Парам1, Парам2,,,)
        assert_diagnostic_range(code, &diagnostics[1], 9, 35, 36);

        // Line 11: Модуль.Метод3(Парам1, Парам2, Парам3,, )
        assert_diagnostic_range(code, &diagnostics[2], 10, 49, 50);

        // Line 12: Модуль.Метод4(Парам1, , Парам2,,,,)
        assert_diagnostic_range(code, &diagnostics[3], 11, 45, 46);

        // Line 14: Если Метод5(Парам1, , Парам2,,,,) Тогда
        assert_diagnostic_range(code, &diagnostics[4], 13, 31, 32);

        // Line 18: Если Модуль.Метод6(Парам1, , Парам2,,,,) Тогда
        assert_diagnostic_range(code, &diagnostics[5], 17, 38, 39);
    }

    #[test]
    fn test_no_trailing_commas() {
        let code = r#"
Результат = Метод(Парам1, Парам2);
Результат = Метод(Парам1, , Парам2);
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_single_trailing_comma() {
        let code = r#"
Результат = Метод(А, Б,);
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_multiple_trailing_commas() {
        let code = r#"
Результат = Метод(А, Б,,,);
"#;
        let diagnostics = check_diagnostic(code);
        // Only first trailing comma is reported
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_empty_call() {
        let code = r#"
Результат = Метод();
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }
}
