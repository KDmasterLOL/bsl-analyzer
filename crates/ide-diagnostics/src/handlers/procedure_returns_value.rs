//! ProcedureReturnsValue diagnostic.
//!
//! Detects return statements with values inside procedures.
//! Only functions can return values, procedures must use `Return;` without value.
//!
//! ## Bad practice
//! ```bsl
//! Процедура Тест()
//!     Возврат 42;  // Error: procedure cannot return value
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Тест()
//!     Возврат;  // OK: procedure without return value
//! КонецПроцедуры
//!
//! Функция Тест()
//!     Возврат 42;  // OK: function can return value
//! КонецФункции
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Blocker
//! - **Type:** ERROR

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::ProcedureReturnsValue;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Процедура не должна возвращать значение".to_string(),
        range,
        severity: ctx.severity(code),
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;

    #[test]
    fn test_procedure_with_return_value() {
        let code = r#"Процедура Тест()
    Возврат 42;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ProcedureReturnsValue)
            .collect();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_procedure_without_return_value_ok() {
        let code = r#"Процедура Тест()
    Возврат;
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        assert!(diagnostics.iter().all(|d| d.code != DiagnosticCode::ProcedureReturnsValue));
    }

    #[test]
    fn test_function_with_return_value_ok() {
        let code = r#"Функция Тест()
    Возврат 42;
КонецФункции"#;
        let diagnostics = check_hir_diagnostic(code);
        assert!(diagnostics.iter().all(|d| d.code != DiagnosticCode::ProcedureReturnsValue));
    }

    #[test]
    fn test_fixture_java_compatibility() {
        let code = include_str!("../../test_data/ProcedureReturnsValueDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ProcedureReturnsValue)
            .collect();
        assert_eq!(diags.len(), 3);
        // Note: Our RETURN_STMT includes `;`, so end columns match line length
        // Line 9: `    Возврат Тест;` - cols 4-17
        assert_diagnostic_range(code, diags[0], 8, 4, 17);
        // Line 17: `        Возврат ОдноЗначение() + " 2";` - cols 8-38
        assert_diagnostic_range(code, diags[1], 16, 8, 38);
        // Line 29: `            Возврат Накопитель;` - cols 12-31
        assert_diagnostic_range(code, diags[2], 28, 12, 31);
    }
}
