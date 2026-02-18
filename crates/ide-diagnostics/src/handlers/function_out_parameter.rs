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

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when FunctionOutParameter diagnostic is emitted during lowering.
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::FunctionOutParameter;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Функция изменяет параметр '{}'. Используйте возвращаемое значение вместо выходного параметра",
            name
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;

    #[test]
    fn test_function_out_parameter() {
        let code = include_str!("../../test_data/FunctionOutParameterDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FunctionOutParameter).collect();

        assert_eq!(func_diags.len(), 1, "Expected 1 diagnostic");

        assert_diagnostic_range(code, func_diags[0], 5, 4, 5);
        assert!(func_diags[0].message.contains("а"));
    }

    #[test]
    fn test_procedure_allowed() {
        let code = r#"
Процедура Тест(А, Знач Б)
    А = 1;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FunctionOutParameter).collect();
        assert_eq!(func_diags.len(), 0, "Procedures are allowed to modify parameters");
    }

    #[test]
    fn test_val_parameter_not_flagged() {
        let code = r#"
Функция Тест(Знач А, Знач Б)
    А = 1;
    Возврат А;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FunctionOutParameter).collect();
        assert_eq!(func_diags.len(), 0, "Val parameters can be modified (local copy)");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Функция Тест(Параметр)
    ПАРАМЕТР = 1;
    Возврат ПАРАМЕТР;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FunctionOutParameter).collect();
        assert_eq!(func_diags.len(), 1, "Should detect case-insensitive match");
    }

    #[test]
    fn test_only_simple_assignment() {
        let code = r#"
Функция Тест(Объект)
    Объект.Свойство = 1;
    Возврат Объект;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FunctionOutParameter).collect();
        assert_eq!(func_diags.len(), 0, "Property assignment should not be flagged");
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
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FunctionOutParameter).collect();
        assert_eq!(func_diags.len(), 2, "Should detect multiple parameter modifications");
    }
}
