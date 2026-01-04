//! FunctionReturnsSamePrimitive diagnostic
//!
//! Detects functions that always return the same primitive value in all branches.
//!
//! **Source (Java):** bsl-language-server/FunctionReturnsSamePrimitiveDiagnostic.java
//! **Source (Rust tree-sitter):** bsl-language-server-rust/rules/function_returns_same_primitive.rs
//!
//! ## Why?
//! Functions that always return the same constant value are useless and indicate poor design:
//! - Should be replaced with a constant or variable
//! - Wastes performance on function calls
//! - Misleading - looks like computed value
//! - Harder to maintain
//!
//! ## Bad practice
//! ```bsl
//! Функция ПолучитьВерсию()
//!     Если Условие Тогда
//!         Возврат "1.0";
//!     Иначе
//!         Возврат "1.0";  // Always returns same value!
//!     КонецЕсли;
//! КонецФункции
//!
//! Функция ПроверкаДанных(Данные)
//!     Если ЭтоПравильно(Данные) Тогда
//!         Возврат Истина;
//!     Иначе
//!         Возврат Истина;  // Always returns True!
//!     КонецЕсли;
//! КонецФункции
//! ```
//!
//! ## Good practice
//! ```bsl
//! Перем Версия = "1.0";  // Use constant/variable
//!
//! Функция ПолучитьВерсию()
//!     Возврат ВычислитьВерсию();  // Computed value
//! КонецФункции
//!
//! Функция ПроверкаДанных(Данные)
//!     Если ЭтоПравильно(Данные) Тогда
//!         Возврат Истина;
//!     Иначе
//!         Возврат Ложь;  // Different values
//!     КонецЕсли;
//! КонецФункции
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{ast::AstNode, SyntaxKind, SyntaxNode};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::FunctionReturnsSamePrimitive) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() == SyntaxKind::FUNCTION_DEF {
            if let Some(func) = syntax::ast::FunctionDef::cast(node.clone()) {
                check_function(&func, &mut diagnostics);
            }
        }
    }

    diagnostics
}

fn check_function(func: &syntax::ast::FunctionDef, diagnostics: &mut Vec<Diagnostic>) {
    // TODO: skipAttachable parameter check
    // Check if function name starts with "Подключаемый_" or "Attachable_"
    if let Some(name_token) = func.name() {
        let name = name_token.text();
        let name_lower = name.to_lowercase();
        if name_lower.starts_with("подключаемый_") || name_lower.starts_with("attachable_")
        {
            // Skip attachable methods by default
            // TODO: make this configurable
            return;
        }
    }

    // Find all return statements in the function
    let return_stmts: Vec<_> =
        func.syntax().descendants().filter(|n| n.kind() == SyntaxKind::RETURN_STMT).collect();

    // Need at least 2 return statements to detect
    if return_stmts.len() < 2 {
        return;
    }

    // Extract expressions from return statements
    let mut expressions = Vec::new();
    for ret_stmt in &return_stmts {
        if let Some(expr) = get_return_expression(ret_stmt) {
            expressions.push(expr);
        }
    }

    // All returns must have expressions (not empty "Возврат;")
    if expressions.len() != return_stmts.len() {
        return;
    }

    // Check if all expressions are primitive (no identifiers/function calls)
    let all_primitive = expressions.iter().all(is_primitive_expression);
    if !all_primitive {
        return;
    }

    // Compare all expressions (case-insensitive by default)
    // TODO: caseSensitiveForString parameter
    let first_text = get_expression_text(&expressions[0], false);
    let all_same =
        expressions[1..].iter().all(|expr| get_expression_text(expr, false) == first_text);

    if all_same {
        // Report diagnostic on function name
        if let Some(name_token) = func.name() {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::FunctionReturnsSamePrimitive,
                message: "Функция всегда возвращает одно и то же примитивное значение. \
                     Замените функцию на константу или переменную модуля."
                    .to_string(),
                severity: Severity::Major,
                range: name_token.text_range(),
                tags: vec![],
                fixes: vec![],
            });
        }
    }
}

/// Get the expression being returned from a return statement
fn get_return_expression(ret_stmt: &SyntaxNode) -> Option<SyntaxNode> {
    // Return statement structure: RETURN_STMT → KW_RETURN + optional expression
    ret_stmt
        .children()
        .find(|child| !matches!(child.kind(), SyntaxKind::KW_RETURN | SyntaxKind::SEMICOLON))
}

/// Check if expression is a primitive constant (not a variable/function call)
/// Primitives: NUMBER, STRING, TRUE_KW, FALSE_KW, NULL_KW, UNDEFINED_KW
fn is_primitive_expression(expr: &SyntaxNode) -> bool {
    // Check if expression contains any identifiers (variables/function calls)
    // If it has IDENT or CALL_EXPR, it's not a primitive
    !expr.descendants_with_tokens().any(|elem| {
        if let Some(token) = elem.as_token() {
            matches!(token.kind(), SyntaxKind::IDENT)
        } else if let Some(node) = elem.as_node() {
            matches!(node.kind(), SyntaxKind::CALL_EXPR)
        } else {
            false
        }
    })
}

/// Get text representation of expression for comparison
fn get_expression_text(expr: &SyntaxNode, case_sensitive_for_string: bool) -> String {
    let text = expr.text().to_string();

    // If expression contains a string and case_sensitive is true, return as-is
    if case_sensitive_for_string
        && expr.descendants_with_tokens().any(|elem| {
            elem.as_token()
                .map(|t| {
                    matches!(
                        t.kind(),
                        SyntaxKind::STRING
                            | SyntaxKind::STRING_START
                            | SyntaxKind::STRING_TAIL
                            | SyntaxKind::STRING_PART
                    )
                })
                .unwrap_or(false)
        })
    {
        return text;
    }

    // Otherwise, normalize to uppercase for comparison
    text.to_uppercase()
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
            file_set: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_fixture() {
        let code = include_str!("../../tests/fixtures/FunctionReturnsSamePrimitiveDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        // With default parameters (skipAttachable=true, caseSensitiveForString=false)
        // Java expects 5 diagnostics at:
        // - Line 0 (ПроверитьСтроку), cols 8-23
        // - Line 25 (Метод1), cols 8-14
        // - Line 35 (СтавкаНДС), cols 8-17
        // - Line 62 (КакаяТоКоманда), cols 8-22
        // - Line 82 (ПроверкаРегистраДляСтрок), cols 8-32

        assert_eq!(diagnostics.len(), 5, "Expected 5 diagnostics with default config");

        // Java test uses 0-based line numbers
        // Our fixture is identical to Java (no extra comments at start)

        // ПроверитьСтроку - line 0
        assert_diagnostic_range(&file_content, &diagnostics[0], 0, 8, 23);

        // Метод1 - line 25
        assert_diagnostic_range(&file_content, &diagnostics[1], 25, 8, 14);

        // СтавкаНДС - line 35
        assert_diagnostic_range(&file_content, &diagnostics[2], 35, 8, 17);

        // КакаяТоКоманда - line 62
        assert_diagnostic_range(&file_content, &diagnostics[3], 62, 8, 22);

        // ПроверкаРегистраДляСтрок - line 82
        assert_diagnostic_range(&file_content, &diagnostics[4], 82, 8, 32);
    }

    #[test]
    fn test_single_return_no_diagnostic() {
        let code = r#"
Функция БудемТестироватьФункциональность()
    Возврат Ложь;
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Single return should not trigger");
    }

    #[test]
    fn test_returns_variable_no_diagnostic() {
        let code = r#"
Функция СтавкаНДС2(Ставка)
    Значение = 20;
    Если Ставка = "Ставка18" Тогда
        Возврат Значение;
    КонецЕсли;
    Возврат Значение;
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Returning variable should not trigger (not primitive)");
    }

    #[test]
    fn test_different_primitives_no_diagnostic() {
        let code = r#"
Функция Проверка(Условие)
    Если Условие Тогда
        Возврат Истина;
    Иначе
        Возврат Ложь;
    КонецЕсли;
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Different primitive values should not trigger");
    }

    #[test]
    fn test_same_boolean_triggers() {
        let code = r#"
Функция ПроверитьСтроку(СтрокаТаблицы)
    Если Условие1 Тогда
        Возврат Истина;
    ИначеЕсли Условие2 Тогда
        Возврат Истина;
    Иначе
        Возврат Истина;
    КонецЕсли;
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Same boolean should trigger");
    }

    #[test]
    fn test_same_number_triggers() {
        let code = r#"
Функция СтавкаНДС(Ставка)
    Если Ставка = "Ставка18" Тогда
        Возврат 20;
    КонецЕсли;
    Возврат 20;
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Same number should trigger");
    }

    #[test]
    fn test_same_string_triggers() {
        let code = r#"
Функция Метод1()
    Если Фича = "Дирижабль" Тогда
        Возврат "Фича";
    ИначеЕсли Фича = "Ага" Тогда
        Возврат "Фича";
    КонецЕсли;
    Возврат "Фича";
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Same string should trigger");
    }

    #[test]
    fn test_null_case_insensitive() {
        let code = r#"
Функция КакаяТоКоманда(Команда)
    Если ЗначениеЗаполнено(ТекущаяДата) Тогда
        Возврат Null;
    КонецЕсли;
    Возврат NULL;
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(
            diagnostics.len(),
            1,
            "Null and NULL should be treated as same (case-insensitive)"
        );
    }

    #[test]
    fn test_attachable_skipped() {
        let code = r#"
Функция Подключаемый_КакаяТоКоманда(Команда)
    Если ЗначениеЗаполнено(ТекущаяДата) Тогда
        Возврат Null;
    КонецЕсли;
    Возврат NULL;
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Attachable methods should be skipped by default");
    }

    #[test]
    fn test_attachable_english_skipped() {
        let code = r#"
Function Attachable_RandomAction(Command)
    If ValueIsFilled(CurrentDate) Then
        Return Undefined;
    EndIf;
    Return Undefined;
EndFunction
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Attachable_ (English) methods should be skipped");
    }
}
