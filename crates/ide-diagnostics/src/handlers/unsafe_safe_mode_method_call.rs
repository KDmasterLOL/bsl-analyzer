use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use syntax::{SyntaxKind, SyntaxNode};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::UnsafeSafeModeMethodCall;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() != SyntaxKind::CALL_EXPR {
            continue;
        }

        if let Some(range) = check_unsafe_safe_mode_call(&node) {
            diagnostics.push(Diagnostic {
                code,
                message: "Use explicit comparison with boolean when calling SafeMode method"
                    .to_string(),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    diagnostics
}

fn is_safe_mode_method_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "безопасныйрежим" | "safemode")
}

fn check_unsafe_safe_mode_call(call_node: &SyntaxNode) -> Option<ide_db::TextRange> {
    let callee = call_node.children().next()?;
    let actual_callee = if callee.kind() == SyntaxKind::EXPR {
        callee.children().next().unwrap_or(callee)
    } else {
        callee
    };

    if actual_callee.kind() != SyntaxKind::IDENT {
        return None;
    }

    let name = actual_callee.text().to_string();
    if !is_safe_mode_method_name(&name) {
        return None;
    }

    let method_range = actual_callee.text_range();

    if is_unsafe_context(call_node) {
        Some(method_range)
    } else {
        None
    }
}

fn is_unsafe_context(call_node: &SyntaxNode) -> bool {
    let mut current = call_node.parent();

    while let Some(node) = current {
        match node.kind() {
            SyntaxKind::UNARY_EXPR => {
                if has_not_operator(&node) {
                    return true;
                }
            }
            SyntaxKind::BINARY_EXPR => {
                if has_comparison_operator(&node) {
                    return false;
                }
                if has_boolean_operator(&node) {
                    return true;
                }
            }
            SyntaxKind::PAREN_EXPR | SyntaxKind::EXPR => {}
            SyntaxKind::IF_STMT | SyntaxKind::ELSIF_CLAUSE => {
                let condition = get_if_condition(&node);
                if let Some(cond) = condition {
                    if is_sole_condition(&cond, call_node) {
                        return true;
                    }
                }
                return false;
            }
            SyntaxKind::ASSIGN_STMT => {
                let rhs = get_assignment_rhs(&node);
                if let Some(rhs_node) = rhs {
                    if is_direct_assignment(&rhs_node, call_node) {
                        return false;
                    }
                }
                return false;
            }
            SyntaxKind::ARG_LIST => {
                return false;
            }
            SyntaxKind::CALL_EXPR | SyntaxKind::CALL_STMT => {
                return false;
            }
            SyntaxKind::STMT_LIST
            | SyntaxKind::PROCEDURE_DEF
            | SyntaxKind::FUNCTION_DEF
            | SyntaxKind::SOURCE_FILE => {
                break;
            }
            _ => {}
        }
        current = node.parent();
    }

    false
}

fn has_not_operator(node: &SyntaxNode) -> bool {
    node.children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::KW_NOT)
}

fn has_comparison_operator(node: &SyntaxNode) -> bool {
    node.children_with_tokens().filter_map(|el| el.into_token()).any(|tok| {
        matches!(
            tok.kind(),
            SyntaxKind::EQ
                | SyntaxKind::NEQ
                | SyntaxKind::LT
                | SyntaxKind::LE
                | SyntaxKind::GT
                | SyntaxKind::GE
        )
    })
}

fn has_boolean_operator(node: &SyntaxNode) -> bool {
    node.children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| matches!(tok.kind(), SyntaxKind::KW_AND | SyntaxKind::KW_OR))
}

fn get_if_condition(if_node: &SyntaxNode) -> Option<SyntaxNode> {
    if_node.children().find(|n| n.kind() == SyntaxKind::EXPR)
}

fn is_sole_condition(condition_node: &SyntaxNode, call_node: &SyntaxNode) -> bool {
    let call_range = call_node.text_range();

    for descendant in condition_node.descendants() {
        if descendant.kind() == SyntaxKind::CALL_EXPR && descendant.text_range() == call_range {
            let has_binary = condition_node
                .descendants()
                .any(|n| n.kind() == SyntaxKind::BINARY_EXPR && has_comparison_operator(&n));
            return !has_binary;
        }
    }
    false
}

fn get_assignment_rhs(assign_node: &SyntaxNode) -> Option<SyntaxNode> {
    assign_node.children().nth(1)
}

fn is_direct_assignment(rhs_node: &SyntaxNode, call_node: &SyntaxNode) -> bool {
    let rhs_range = rhs_node.text_range();
    let call_range = call_node.text_range();

    if rhs_range == call_range {
        return true;
    }

    let inner = if rhs_node.kind() == SyntaxKind::EXPR { rhs_node.children().next() } else { None };

    if let Some(inner_node) = inner {
        if inner_node.text_range() == call_range {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_ast_diagnostic;

    #[test]
    fn test_safe_direct_assignment() {
        let code = r#"
Процедура Тест()
    Перем1 = БезопасныйРежим();
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_safe_method_argument() {
        let code = r#"
Процедура Тест()
    Перем2 = Метод(БезопасныйРежим());
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_safe_explicit_comparison() {
        let code = r#"
Процедура Тест()
    Если БезопасныйРежим() <> Ложь Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_unsafe_sole_condition() {
        let code = r#"
Процедура Тест()
    Если БезопасныйРежим() Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_unsafe_with_not() {
        let code = r#"
Процедура Тест()
    Если Не БезопасныйРежим() Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_unsafe_with_or() {
        let code = r#"
Процедура Тест()
    Если БезопасныйРежим() ИЛИ Тест = Истина Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_comprehensive_fixture() {
        let code = include_str!("../test_data/UnsafeSafeModeMethodCallDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 10, "Expected exactly 10 diagnostics matching Java");
    }

    #[test]
    fn test_comprehensive_fixture_positions() {
        use crate::test_utils::assert_diagnostic_range;

        let code = include_str!("../test_data/UnsafeSafeModeMethodCallDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 10);

        assert_diagnostic_range(code, &diagnostics[0], 1, 9, 24);
        assert_diagnostic_range(code, &diagnostics[1], 3, 17, 32);
        assert_diagnostic_range(code, &diagnostics[2], 7, 12, 27);
        assert_diagnostic_range(code, &diagnostics[3], 11, 33, 48);
        assert_diagnostic_range(code, &diagnostics[4], 14, 47, 62);
        assert_diagnostic_range(code, &diagnostics[5], 16, 50, 65);
        assert_diagnostic_range(code, &diagnostics[6], 18, 34, 49);
        assert_diagnostic_range(code, &diagnostics[7], 20, 34, 49);
        assert_diagnostic_range(code, &diagnostics[8], 23, 20, 35);
        assert_diagnostic_range(code, &diagnostics[9], 26, 9, 24);
    }
}
