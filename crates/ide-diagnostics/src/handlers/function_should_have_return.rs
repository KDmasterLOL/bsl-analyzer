//! FunctionShouldHaveReturn diagnostic.
//!
//! Checks that all functions have at least one return statement.
//!
//! ## Why?
//! Functions should explicitly return a value. Missing return statements
//! can lead to unexpected `Undefined` values and make code harder to understand.
//!
//! ## Bad practice
//! ```bsl
//! Функция ВычислитьСумму(А, Б)
//!     Результат = А + Б;
//! КонецФункции  // Missing Возврат!
//! ```
//!
//! ## Good practice
//! ```bsl
//! Функция ВычислитьСумму(А, Б)
//!     Возврат А + Б;
//! КонецФункции
//! ```
//!
//! ## Implementation
//!
//! Ported from:
//! - FunctionShouldHaveReturnDiagnostic.java (bsl-language-server)
//! - function_should_have_return.rs (bsl-language-server-rust)
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.
//!
//! ### Key differences from Java implementation:
//! - Java uses `Trees.findAllTokenNodes(ctx, BSLLexer.RETURN_KEYWORD)` to find returns
//! - Rust uses `node.descendants().any(|n| n.kind() == SyntaxKind::RETURN_STMT)`
//! - Both check `Trees.treeContainsErrors()` to skip functions with parse errors
//!
//! ### Diagnostic range:
//! - Java: `diagnosticStorage.addDiagnostic(subName)` - adds diagnostic on function name
//! - Rust: Finds first IDENT token before PARAM_LIST (same as AllFunctionPathMustHaveReturn)

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode};

/// Runs the FunctionShouldHaveReturn diagnostic.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Check if diagnostic is disabled
    if ctx.config.is_disabled(DiagnosticCode::FunctionShouldHaveReturn) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    // Find all function definitions (not procedures)
    for node in root.descendants() {
        if node.kind() == SyntaxKind::FUNCTION_DEF {
            if let Some(diag) = check_function(&node) {
                diagnostics.push(diag);
            }
        }
    }

    diagnostics
}

/// Check a single function for missing return statement
fn check_function(func_node: &SyntaxNode) -> Option<Diagnostic> {
    // Check if function has parse errors (skip if it does)
    // This matches Java's `Trees.treeContainsErrors(ctx)` check
    if has_parse_errors(func_node) {
        return None;
    }

    // Check if function has at least one return statement
    let has_return = has_return_statement(func_node);
    if has_return {
        return None;
    }

    // Get function name for diagnostic range
    // The function name is the first IDENT token that appears before PARAM_LIST
    let name_token = func_node
        .children_with_tokens()
        .take_while(|el| !matches!(el.kind(), SyntaxKind::PARAM_LIST))
        .filter_map(|el| el.into_token())
        .filter(|tok| !tok.kind().is_trivia()) // Skip trivia tokens
        .find(|tok| tok.kind() == SyntaxKind::IDENT);

    let name_range =
        name_token.map(|tok| tok.text_range()).unwrap_or_else(|| func_node.text_range());

    Some(Diagnostic {
        code: DiagnosticCode::FunctionShouldHaveReturn,
        message: "Функция должна содержать хотя бы один оператор Возврат".to_string(),
        severity: Severity::Major,
        range: name_range,
        tags: vec![],
        fixes: vec![],
    })
}

/// Check if function has at least one return statement
fn has_return_statement(func_node: &SyntaxNode) -> bool {
    func_node.descendants().any(|n| n.kind() == SyntaxKind::RETURN_STMT)
}

/// Check if node contains parse errors
/// This replicates Java's `Trees.treeContainsErrors(ctx)` check
fn has_parse_errors(node: &SyntaxNode) -> bool {
    // Rowan marks parse errors with ERROR nodes in the CST
    node.descendants().any(|n| n.kind() == SyntaxKind::ERROR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::assert_diagnostic_range, DiagnosticsConfig};
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
            file_set: None,
        };

        // Run diagnostic
        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    /// Integration test matching Java test structure
    ///
    /// Based on FunctionShouldHaveReturnDiagnosticTest.java
    /// Uses the same test file: FunctionShouldHaveReturnDiagnostic.bsl
    ///
    /// Expected: 1 diagnostic at line 0 (ФункцияБезВозврата), columns 8-26
    #[test]
    fn test_function_should_have_return() {
        let code = include_str!("../../tests/fixtures/FunctionShouldHaveReturnDiagnostic.bsl");

        let (diagnostics, file_content) = check_diagnostic(code);

        // Java test expects: assertThat(diagnostics).hasSize(1);
        // assertThat(diagnostics, true).hasRange(0, 8, 0, 26);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");

        // Line 0 (first line), columns 8-26: "ФункцияБезВозврата"
        assert_eq!(diagnostics[0].code, DiagnosticCode::FunctionShouldHaveReturn);
        assert_eq!(diagnostics[0].severity, Severity::Major);
        assert_diagnostic_range(&file_content, &diagnostics[0], 0, 8, 26);
    }

    /// Test function without return
    #[test]
    fn test_function_without_return() {
        let code = r#"Функция БезВозврата()
    Перем Х;
    Х = 42;
КонецФункции"#;

        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::FunctionShouldHaveReturn);
        assert!(diagnostics[0].message.contains("Возврат"));
        // "Функция" (0) + " " (7) + "БезВозврата" (8-18)
        assert_diagnostic_range(&file_content, &diagnostics[0], 0, 8, 19);
    }

    /// Test function with return
    #[test]
    fn test_function_with_return() {
        let code = r#"Функция Сложить(А, Б)
    Возврат А + Б;
КонецФункции"#;

        let (diagnostics, _file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 0, "Function with return should not trigger diagnostic");
    }

    /// Test function with conditional return
    #[test]
    fn test_function_with_conditional_return() {
        let code = r#"Функция Проверка(Значение)
    Если Значение > 0 Тогда
        Возврат Истина;
    Иначе
        Возврат Ложь;
    КонецЕсли;
КонецФункции"#;

        let (diagnostics, _file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 0, "Function with conditional returns should not trigger");
    }

    /// Test procedure (should not be checked)
    #[test]
    fn test_procedure_not_checked() {
        let code = r#"Процедура БезВозврата()
    Сообщить("Привет");
КонецПроцедуры"#;

        let (diagnostics, _file_content) = check_diagnostic(code);

        // Procedures don't need return statements
        assert_eq!(diagnostics.len(), 0, "Procedures should not be checked");
    }

    /// Test multiple functions (only one without return)
    #[test]
    fn test_multiple_functions() {
        let code = r#"Функция Первая()
    Возврат 1;
КонецФункции

Функция Вторая()
    Перем Х;
    Х = 2;
КонецФункции

Функция Третья()
    Возврат 3;
КонецФункции"#;

        let (diagnostics, file_content) = check_diagnostic(code);

        // Only "Вторая" function should be flagged
        assert_eq!(diagnostics.len(), 1, "Only one function without return");
        assert_diagnostic_range(&file_content, &diagnostics[0], 4, 8, 14);
    }

    /// Test function with parse errors (should be skipped)
    /// Matches Java's `Trees.treeContainsErrors(ctx)` check
    #[test]
    fn test_function_with_parse_errors() {
        let code = r#"Функция СошибкойРазбора()
    Если Тогда
    КонецЕсли;
    Возврат;
КонецФункции"#;

        let (diagnostics, _file_content) = check_diagnostic(code);

        // Functions with parse errors should be skipped
        assert_eq!(diagnostics.len(), 0, "Functions with parse errors should be skipped");
    }

    /// Test English keywords (bilingual support)
    #[test]
    fn test_english_function() {
        let code = r#"Function Add(A, B)
    Return A + B;
EndFunction"#;

        let (diagnostics, _file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 0, "English function with return should not trigger");
    }

    /// Test English function without return
    #[test]
    fn test_english_function_without_return() {
        let code = r#"Function NoReturn()
    Var X;
EndFunction"#;

        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 1, "English function without return should trigger");
        assert_diagnostic_range(&file_content, &diagnostics[0], 0, 9, 17);
    }
}
