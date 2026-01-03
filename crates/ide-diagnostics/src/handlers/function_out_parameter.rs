//! FunctionOutParameter diagnostic
//!
//! Detects when a function modifies its by-reference parameters (output parameters).
//!
//! **Source (Java):** bsl-language-server/FunctionOutParameterDiagnostic.java
//! **Source (Rust tree-sitter):** bsl-language-server-rust/rules/using_cancel_parameter.rs (similar pattern)
//!
//! ## Why?
//! Functions in BSL should not modify their parameters. This is a code smell that makes
//! code harder to understand and maintain. Functions should use return values instead of
//! output parameters.
//!
//! **Note:** This diagnostic only applies to functions, not procedures. Procedures are
//! allowed to modify parameters.
//!
//! ## Bad practice
//! ```bsl
//! Функция Вычислить(Данные, Знач Режим)  // Данные - by reference (no Знач)
//!     Данные = ОбработатьДанные();  // Bad! Modifying parameter
//!     Возврат Истина;
//! КонецФункции
//! ```
//!
//! ## Good practice
//! ```bsl
//! Функция Вычислить(Знач Данные, Знач Режим)  // All by value
//!     Результат = ОбработатьДанные();
//!     Возврат Результат;
//! КонецФункции
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use std::collections::HashSet;
use syntax::{ast::AstNode, SyntaxKind};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::FunctionOutParameter) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() == SyntaxKind::FUNCTION_DEF {
            if let Some(func) = syntax::ast::FunctionDef::cast(node) {
                check_function(&func, &mut diagnostics);
            }
        }
    }

    diagnostics
}

fn check_function(func: &syntax::ast::FunctionDef, diagnostics: &mut Vec<Diagnostic>) {
    let Some(param_list) = func.param_list() else {
        return;
    };

    let by_ref_params: HashSet<String> = param_list
        .params()
        .filter(|p| p.val_keyword().is_none())
        .filter_map(|p| p.name().map(|n| n.text().to_lowercase()))
        .collect();

    if by_ref_params.is_empty() {
        return;
    }

    let Some(body) = func.body() else {
        return;
    };

    for node in body.syntax().descendants() {
        if node.kind() == SyntaxKind::ASSIGN_STMT {
            if let Some(lvalue_ident) = get_lvalue_ident(&node) {
                let ident_text = lvalue_ident.text().to_lowercase();

                if by_ref_params.contains(&ident_text) {
                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::FunctionOutParameter,
                        message: format!(
                            "Функция изменяет параметр '{}'. Используйте возвращаемое значение вместо выходного параметра",
                            lvalue_ident.text()
                        ),
                        severity: Severity::Major,
                        range: lvalue_ident.text_range(),
                        tags: vec![],
                        fixes: vec![],
                    });
                }
            }
        }
    }
}

fn get_lvalue_ident(assign_stmt: &syntax::SyntaxNode) -> Option<syntax::SyntaxToken> {
    // PARSER CHANGE: ASSIGN_STMT structure changed
    // Old: ASSIGN_STMT -> EXPR (entire "A = B")
    // New: ASSIGN_STMT -> [LHS tokens/nodes] -> EXPR (RHS value)
    //
    // We need to extract tokens from the left-hand side (before first EXPR)

    // Find the first EXPR child (right-hand side)
    let rhs_expr = assign_stmt.children().find(|n| n.kind() == SyntaxKind::EXPR)?;
    let rhs_start = rhs_expr.text_range().start();

    // Collect all tokens before the RHS (i.e., left-hand side)
    let all_tokens: Vec<_> = assign_stmt
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.text_range().start() < rhs_start)
        .collect();

    // Check if left-hand side contains complex expressions (field access, indexing, calls)
    // If so, this is not a simple parameter assignment
    if all_tokens
        .iter()
        .any(|t| matches!(t.kind(), SyntaxKind::DOT | SyntaxKind::L_BRACKET | SyntaxKind::L_PAREN))
    {
        return None;
    }

    // Return the first IDENT token (should be the only one for simple assignment)
    all_tokens.into_iter().find(|t| t.kind() == SyntaxKind::IDENT)
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

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
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

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_function_out_parameter() {
        let code =
            include_str!("function_out_parameter/fixtures/FunctionOutParameterDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");

        assert_diagnostic_range(&file_content, &diagnostics[0], 5, 4, 5);
        assert!(diagnostics[0].message.contains("а"));
    }

    #[test]
    fn test_procedure_allowed() {
        let code = r#"
Процедура Тест(А, Знач Б)
    А = 1;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Procedures are allowed to modify parameters");
    }

    #[test]
    fn test_val_parameter_not_flagged() {
        let code = r#"
Функция Тест(Знач А, Знач Б)
    А = 1;
    Возврат А;
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Val parameters can be modified (local copy)");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Функция Тест(Параметр)
    ПАРАМЕТР = 1;
    Возврат ПАРАМЕТР;
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect case-insensitive match");
    }

    #[test]
    fn test_only_simple_assignment() {
        let code = r#"
Функция Тест(Объект)
    Объект.Свойство = 1;
    Возврат Объект;
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Property assignment should not be flagged");
    }

    #[test]
    fn test_multiple_violations() {
        let code = r#"
Функция Обработка(Данные, Результат)
    Данные = Новый Массив;
    Результат = ОбработатьДанные(Данные);
    Возврат Истина;
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 2, "Should detect multiple parameter modifications");
    }
}
