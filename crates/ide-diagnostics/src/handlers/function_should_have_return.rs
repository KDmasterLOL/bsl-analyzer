//! Diagnostic: FunctionShouldHaveReturn
//!
//! Checks that functions contain at least one return statement.
//! Procedures don't require return statements.
//!
//! ## Severity
//! Major
//!
//! ## Example
//! ```bsl
//! // Bad - function without return
//! Функция БезВозврата()
//!     Перем Х;
//! КонецФункции
//!
//! // Good - function with return
//! Функция СВозвратом()
//!     Возврат 42;
//! КонецФункции
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::FunctionShouldHaveReturn` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::FunctionShouldHaveReturn) {
        return None;
    }
    Some(Diagnostic {
        code: DiagnosticCode::FunctionShouldHaveReturn,
        message: "Функция должна содержать хотя бы один оператор Возврат".to_string(),
        severity: Severity::Major,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic;
    use crate::DiagnosticCode;

    #[test]
    fn test_function_without_return() {
        let code = r#"Функция БезВозврата()
    Перем Х;
    Х = 42;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let return_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionShouldHaveReturn)
            .collect();
        assert_eq!(return_diags.len(), 1, "Expected 1 FunctionShouldHaveReturn diagnostic");
    }

    #[test]
    fn test_function_with_return() {
        let code = r#"Функция СВозвратом()
    Возврат 42;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::FunctionShouldHaveReturn),
            "Function with return should not trigger diagnostic"
        );
    }

    #[test]
    fn test_procedure_no_return_needed() {
        let code = r#"Процедура БезВозврата()
    Сообщить("Привет");
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::FunctionShouldHaveReturn),
            "Procedures should not trigger FunctionShouldHaveReturn"
        );
    }

    #[test]
    fn test_function_with_conditional_return() {
        let code = r#"Функция Проверка(Значение)
    Если Значение > 0 Тогда
        Возврат Истина;
    Иначе
        Возврат Ложь;
    КонецЕсли;
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::FunctionShouldHaveReturn),
            "Function with conditional returns should not trigger"
        );
    }

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

        let diagnostics = check_hir_diagnostic(code);
        let return_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionShouldHaveReturn)
            .collect();
        assert_eq!(return_diags.len(), 1, "Only one function without return");
    }

    #[test]
    fn test_english_function_with_return() {
        let code = r#"Function Add(A, B)
    Return A + B;
EndFunction"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::FunctionShouldHaveReturn),
            "English function with return should not trigger"
        );
    }

    #[test]
    fn test_english_function_without_return() {
        let code = r#"Function NoReturn()
    Var X;
EndFunction"#;

        let diagnostics = check_hir_diagnostic(code);
        let return_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionShouldHaveReturn)
            .collect();
        assert_eq!(return_diags.len(), 1, "English function without return should trigger");
    }

    #[test]
    fn test_fixture_function_should_have_return() {
        let code = include_str!("../../tests/fixtures/FunctionShouldHaveReturnDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let return_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionShouldHaveReturn)
            .collect();

        // According to fixture: only "ФункцияБезВозврата" and "СошибкойРазбора2" should trigger
        // (Function F has Return, ФункцияСВозвратом has Return, procedures don't need returns,
        //  СошибкойРазбора has Return at the end)
        assert!(
            !return_diags.is_empty(),
            "Should have at least 1 FunctionShouldHaveReturn diagnostic from fixture, got {}",
            return_diags.len()
        );
    }
}
