//! ExportVariables diagnostic
//!
//! Detects exported module variables.
//!
//! **Source (Java):** bsl-language-server/ExportVariablesDiagnostic.java
//! **Source (Rust tree-sitter):** bsl-language-server-rust/rules/export_variables.rs
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - uses module variables collected during HIR lowering.
//!
//! Module-level variables are collected in `ModuleBodies.module_vars` with `is_export` flag.
//! This diagnostic checks all exported variables (is_export == true).
//!
//! Exported module variables are considered bad practice because they create
//! tight coupling and make code harder to maintain. Use getter/setter methods instead.
//!
//! ## Bad practice
//! ```bsl
//! Перем МояПеременная Экспорт;  // Exported variable
//! ```
//!
//! ## Good practice
//! ```bsl
//! Перем МояПеременная;  // Private variable
//!
//! Функция ПолучитьМояПеременная() Экспорт
//!     Возврат МояПеременная;
//! КонецФункции
//!
//! Процедура УстановитьМояПеременная(Значение) Экспорт
//!     МояПеременная = Значение;
//! КонецПроцедуры
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Creates diagnostic from HIR ModuleVarDecl.
///
/// Called from lib.rs when iterating over module_vars with is_export == true.
pub fn from_hir(_name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::ExportVariables) {
        return None;
    }

    Some(Diagnostic {
        code: DiagnosticCode::ExportVariables,
        message: "It is recommended not to use global variables. They often might cause issues that cannot be easily located".to_string(),
        severity: Severity::Warning,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_no_export() {
        let code = r#"
Перем МояПеременная;

Процедура Инициализация()
    МояПеременная = 0;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let export_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExportVariables).collect();
        assert_eq!(export_diags.len(), 0, "Private variable should not trigger diagnostic");
    }

    #[test]
    fn test_simple_export() {
        let code = r#"Перем МояПеременная Экспорт;"#;
        let diagnostics = check_hir_diagnostic(code);
        let export_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExportVariables).collect();
        assert_eq!(export_diags.len(), 1, "Exported variable should trigger diagnostic");
    }

    #[test]
    fn test_inside_procedure() {
        let code = r#"
Процедура Тест()
    Перем ПеременнаяМодуля, ПеременнаяЭкспорт;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let export_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExportVariables).collect();
        // Variables inside procedures cannot be exported
        assert_eq!(export_diags.len(), 0);
    }

    #[test]
    fn test_bilingual() {
        let code_ru = r#"Перем МояПеременная Экспорт;"#;
        let diagnostics_ru = check_hir_diagnostic(code_ru);
        let export_diags_ru: Vec<_> =
            diagnostics_ru.iter().filter(|d| d.code == DiagnosticCode::ExportVariables).collect();
        assert_eq!(export_diags_ru.len(), 1, "Russian keyword should work");

        let code_en = r#"Var MyVariable Export;"#;
        let diagnostics_en = check_hir_diagnostic(code_en);
        let export_diags_en: Vec<_> =
            diagnostics_en.iter().filter(|d| d.code == DiagnosticCode::ExportVariables).collect();
        assert_eq!(export_diags_en.len(), 1, "English keyword should work");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/ExportVariablesDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let export_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExportVariables).collect();

        // Should find 2 exported variables (Перем1 and Перем53)
        assert_eq!(export_diags.len(), 2, "Expected 2 exported variables");

        // Diagnostic 0: Перем1 - HIR range is just the variable name
        // Line 0: "Перем Перем1 Экспорт;"
        //               ^^^^^^
        //               6-12
        assert_diagnostic_range(code, export_diags[0], 0, 6, 12);

        // Diagnostic 1: Перем53 - HIR range is just the variable name
        // Line 2: "Перем Перем53 Экспорт;"
        //               ^^^^^^^
        //               6-13
        assert_diagnostic_range(code, export_diags[1], 2, 6, 13);
    }
}
