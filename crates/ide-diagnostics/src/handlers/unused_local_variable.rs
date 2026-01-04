//! Diagnostic: UnusedLocalVariable
//!
//! Detects local variables that are declared but never used.
//!
//! ## Severity
//! Info (with Unnecessary tag)
//!
//! ## Example
//! ```bsl
//! // Bad - unused variable
//! Процедура Тест()
//!     Перем НеИспользуется;  // Warning: unused
//!     Сообщить("Привет");
//! КонецПроцедуры
//!
//! // Good - variable is used
//! Процедура Тест()
//!     Перем Сообщение;
//!     Сообщение = "Привет";
//!     Сообщить(Сообщение);
//! КонецПроцедуры
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticTag, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::UnusedVariable` is encountered.
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::UnusedLocalVariable) {
        return None;
    }
    Some(Diagnostic {
        code: DiagnosticCode::UnusedLocalVariable,
        message: format!("Переменная \"{}\" объявлена, но не используется", name),
        severity: Severity::Information,
        range,
        tags: vec![DiagnosticTag::Unnecessary],
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;

    #[test]
    fn test_unused_var_in_procedure() {
        let code = r#"Процедура Тест()
    Перем НеИспользуется;
    Сообщить("Привет");
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(unused_diags.len(), 1, "Expected 1 UnusedLocalVariable diagnostic");
        // Check position: variable name "НеИспользуется" on line 1
        assert_diagnostic_range(code, unused_diags[0], 1, 10, 24);
    }

    #[test]
    fn test_used_var_no_diagnostic() {
        let code = r#"Процедура Тест()
    Перем Сообщение;
    Сообщение = "Привет";
    Сообщить(Сообщение);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Used variable should not trigger diagnostic"
        );
    }

    #[test]
    fn test_unused_loop_variable() {
        let code = r#"Процедура Тест()
    Для Индекс = 1 По 10 Цикл
        Сообщить("Итерация");
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(unused_diags.len(), 1, "Unused loop variable should trigger diagnostic");
    }

    #[test]
    fn test_used_loop_variable() {
        let code = r#"Процедура Тест()
    Для Индекс = 1 По 10 Цикл
        Сообщить(Индекс);
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Used loop variable should not trigger diagnostic"
        );
    }

    #[test]
    fn test_unused_foreach_variable() {
        let code = r#"Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Сообщить("Итерация");
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(unused_diags.len(), 1, "Unused foreach variable should trigger diagnostic");
    }

    #[test]
    fn test_multiple_unused_vars() {
        let code = r#"Процедура Тест()
    Перем А, Б, В;
    Сообщить(Б);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        // А and В are unused, Б is used
        assert_eq!(unused_diags.len(), 2, "Expected 2 unused variables (А and В)");
    }

    #[test]
    fn test_case_insensitive_usage() {
        let code = r#"Процедура Тест()
    Перем Переменная;
    ПЕРЕМЕННАЯ = 10;
    Сообщить(переменная);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Case-insensitive usage should count as used"
        );
    }
}
