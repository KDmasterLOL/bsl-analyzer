//! ExportVariables diagnostic
//!
//! Detects exported module variables.
//!
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

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Design, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR ModuleVarDecl.
///
/// Called from lib.rs when iterating over module_vars with is_export == true.
pub fn from_hir(_name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::ExportVariables;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "It is recommended not to use global variables. They often might cause issues that cannot be easily located".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
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
    fn test_multiple_exported_and_private_vars() {
        // Two exported vars, one private, one inside procedure (not exported)
        let code = "Перем Перем1 Экспорт;\nПерем Перем2;\nПерем Перем53 Экспорт;\n\nПроцедура МетодСодержащийПеременную()\n    Перем ПеременнаяМодуля, ПеременнаяЭкспорт;\nКонецПроцедуры";
        let diagnostics = check_hir_diagnostic(code);
        let export_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExportVariables).collect();
        assert_eq!(export_diags.len(), 2, "Expected 2 exported variables");
        // Line 0: "Перем Перем1 Экспорт;" — name span cols 6-12
        assert_diagnostic_range(code, export_diags[0], 0, 6, 12);
        // Line 2: "Перем Перем53 Экспорт;" — name span cols 6-13
        assert_diagnostic_range(code, export_diags[1], 2, 6, 13);
    }

    #[test]
    fn test_commented_export_not_detected() {
        // Commented-out export should not trigger
        let code = "// Перем Закомментированная Экспорт;";
        let diagnostics = check_hir_diagnostic(code);
        let export_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExportVariables).collect();
        assert_eq!(export_diags.len(), 0, "Commented export should not trigger diagnostic");
    }
}
