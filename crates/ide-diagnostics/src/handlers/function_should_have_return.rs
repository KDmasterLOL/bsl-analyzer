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

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::FunctionShouldHaveReturn` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::FunctionShouldHaveReturn,
        "Функция должна содержать хотя бы один оператор Возврат",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
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

        // Check position: function name "БезВозврата" on line 0
        assert_diagnostic_range(code, return_diags[0], 0, 8, 19);
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

    /// Test with Java fixture - must match Java test expectations exactly
    /// Java test: assertThat(diagnostics).hasSize(1); hasRange(0, 8, 0, 26);
    #[test]
    fn test_fixture_function_should_have_return() {
        let code = include_str!("../../test_data/FunctionShouldHaveReturnDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let return_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionShouldHaveReturn)
            .collect();

        // Java expects exactly 1 diagnostic
        assert_eq!(
            return_diags.len(),
            1,
            "Java test expects 1 diagnostic, got {}",
            return_diags.len()
        );

        // Java expects: hasRange(0, 8, 0, 26) - function name "ФункцияБезВозврата"
        assert_diagnostic_range(code, return_diags[0], 0, 8, 26);
    }
}
