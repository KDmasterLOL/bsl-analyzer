//! ExecuteExternalCode diagnostic.
//!
//! Detects usage of Execute() statements and Eval()/Вычислить() method calls
//! which can lead to arbitrary code execution vulnerabilities.
//!
//! ## Severity
//! CRITICAL (VULNERABILITY)
//!
//! ## Tags
//! ERROR, STANDARD
//!
//! ## Examples
//!
//! ```bsl
//! // ❌ Bad: Execute statement on server
//! &НаСервере
//! Процедура ВыполнитьПроизвольныйКод(Строка)
//!     Выполнить(Строка); // CRITICAL: Arbitrary code execution
//! КонецПроцедуры
//!
//! // ❌ Bad: Eval method call
//! Функция ВычислитьЗначение(Строка)
//!     Возврат Вычислить(Строка); // CRITICAL: Arbitrary code execution
//! КонецФункции
//!
//! // ✅ Good: Client-only code (exempted)
//! &НаКлиенте
//! Процедура ВыполнитьНаКлиенте(Строка)
//!     Выполнить(Строка); // OK: Client-side execution is permitted
//! КонецПроцедуры
//! ```
//!
//! ## References
//! - 1C Standard: https://its.1c.ru/db/v8std#content:770:hdoc
//! - Java implementation: bsl-language-server/diagnostics/ExecuteExternalCodeDiagnostic.java

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use syntax::{SyntaxKind, SyntaxNode, TextSize};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::ExecuteExternalCode) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();
    let mut seen_ranges = std::collections::HashSet::new();

    // ✅ OPTIMIZATION: Collect tokens ONCE instead of O(N²) nested tree traversal
    let tokens: Vec<_> = root.descendants_with_tokens().collect();

    // Check EXECUTE_STMT nodes
    for element in tokens.iter() {
        if let Some(node) = element.as_node() {
            if node.kind() == SyntaxKind::EXECUTE_STMT && !is_in_client_only_context(node) {
                let mut range = node.text_range();
                if node.text().to_string().ends_with(';') {
                    range = TextRange::new(range.start(), range.end() - TextSize::from(1));
                }
                if seen_ranges.insert(range) {
                    diagnostics.push(create_diagnostic(range));
                }
            }
        }
    }

    // Check Eval/Вычислить calls (IDENT + LPAREN without preceding DOT)
    let token_slice: Vec<_> = tokens.iter().filter_map(|el| el.as_token()).collect();
    for (i, token) in token_slice.iter().enumerate() {
        if token.kind() == SyntaxKind::IDENT {
            let method_name = token.text().to_lowercase();
            if method_name != "вычислить" && method_name != "eval" {
                continue;
            }

            let next_is_lparen =
                token_slice.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

            if next_is_lparen {
                let prev_is_dot = i
                    .checked_sub(1)
                    .and_then(|idx| token_slice.get(idx))
                    .map(|t| t.kind() == SyntaxKind::DOT)
                    .unwrap_or(false);

                if !prev_is_dot {
                    // Found Eval/Вычислить call - check if in client-only context
                    if let Some(node) = token.parent() {
                        if !is_in_client_only_context(&node) {
                            // Extract range (method name + args)
                            let start = token.text_range().start();
                            let mut end = token.text_range().end();

                            for next_token in token_slice.iter().skip(i + 1) {
                                end = next_token.text_range().end();
                                if next_token.kind() == SyntaxKind::R_PAREN {
                                    break;
                                }
                            }

                            let range = TextRange::new(start, end);
                            if seen_ranges.insert(range) {
                                diagnostics.push(create_diagnostic(range));
                            }
                        }
                    }
                }
            }
        }
    }

    diagnostics
}

fn create_diagnostic(range: TextRange) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::ExecuteExternalCode,
        message: "It is forbidden to execute external code on the server".to_string(),
        range,
        severity: Severity::Critical,
        tags: vec![],
        fixes: vec![],
    }
}

/// Check if node is inside a client-only function or procedure.
///
/// Client-only means: has ONLY &НаКлиенте annotation (no other annotations).
fn is_in_client_only_context(node: &SyntaxNode) -> bool {
    let Some(parent) = find_parent_function_or_procedure(node) else {
        return false;
    };

    is_client_only_method(&parent)
}

/// Check if method (function or procedure) has ONLY &НаКлиенте annotation.
///
/// Uses text-based API: works directly with SyntaxNode instead of typed AST wrappers.
fn is_client_only_method(method: &SyntaxNode) -> bool {
    let annotations = find_annotations(method);

    // Must have exactly ONE annotation
    if annotations.len() != 1 {
        return false;
    }

    is_client_annotation(&annotations[0])
}

/// Find all ANNOTATION nodes that are children of the method.
///
/// Annotations can be either ANNOTATION or COMPILER_DIRECTIVE nodes.
fn find_annotations(method: &SyntaxNode) -> Vec<SyntaxNode> {
    method
        .children()
        .filter(|child| {
            matches!(child.kind(), SyntaxKind::ANNOTATION | SyntaxKind::COMPILER_DIRECTIVE)
        })
        .collect()
}

/// Check if annotation is &НаКлиенте or &AtClient.
///
/// Uses text-based API: searches for ANN_AT_CLIENT token inside the annotation node.
fn is_client_annotation(ann: &SyntaxNode) -> bool {
    ann.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|token| token.kind() == SyntaxKind::ANN_AT_CLIENT)
}

/// Find parent function or procedure node.
fn find_parent_function_or_procedure(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.ancestors()
        .find(|n| matches!(n.kind(), SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::DiagnosticsConfig;
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
            file_set: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/ExecuteExternalCodeDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 5, "Expected 5 diagnostics");

        assert_diagnostic_range(code, &diagnostics[0], 8, 4, 21);
        assert_diagnostic_range(code, &diagnostics[1], 13, 4, 21);
        assert_diagnostic_range(code, &diagnostics[2], 18, 12, 29);
        assert_diagnostic_range(code, &diagnostics[3], 23, 12, 29);
        assert_diagnostic_range(code, &diagnostics[4], 31, 12, 29);
    }

    #[test]
    fn test_client_only_exemption() {
        let code = r#"
&НаКлиенте
Процедура ВыполнитьНаКлиенте(Строка)
    Выполнить(Строка);
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Client-only code should be exempted");
    }

    #[test]
    fn test_server_annotation() {
        let code = r#"
&НаСервере
Процедура ВыполнитьНаСервере(Строка)
    Выполнить(Строка);
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Server-side code should be detected");
    }

    #[test]
    fn test_eval_call() {
        let code = r#"
Функция ВычислитьЗначение(Строка)
    Возврат Вычислить(Строка);
КонецФункции
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Eval call should be detected");
    }

    #[test]
    fn test_qualified_eval_ignored() {
        let code = r#"
Функция ВычислитьЗначение(Объект)
    Возврат Объект.Вычислить();
КонецФункции
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Qualified calls should be ignored");
    }

    #[test]
    fn test_similar_method_name_ignored() {
        let code = r#"
Функция БезОшибок(Строка)
    Возврат ВычислитьЧтоТо(Строка);
КонецФункции
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Similar method names should be ignored");
    }

    #[test]
    fn test_client_at_server_annotation() {
        let code = r#"
&НаКлиентеНаСервере
Функция ВычислитьЗначение(Строка)
    Возврат Вычислить(Строка);
КонецФункции
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(
            diagnostics.len(),
            1,
            "Client+Server annotation should be detected (has server context)"
        );
    }
}
