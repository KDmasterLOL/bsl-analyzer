//! UsingThisForm diagnostic.
//!
//! Detects usage of deprecated `ЭтаФорма` / `ThisForm` property.
//! Starting from 1C:Enterprise 8.3.3, should use `ЭтотОбъект` / `ThisObject` instead.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_3,
    tags: &[MetadataTag::Standard, MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::UsingThisForm,
        "Вместо устаревшего свойства \"ЭтаФорма\" следует использовать \"ЭтотОбъект\"",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::Severity;
    #[test]
    fn test_basic_this_form_usage() {
        let code = r#"
Процедура Тест()
    ГлобалтныйМетод(ЭтаФорма);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let this_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingThisForm).collect();

        assert_eq!(this_form_diags.len(), 1);
        assert_eq!(this_form_diags[0].severity, Severity::Information);
    }

    #[test]
    fn test_this_form_field_access() {
        let code = r#"
Процедура Тест()
    ЭтаФорма.Закрыть();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let this_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingThisForm).collect();

        assert_eq!(this_form_diags.len(), 1);
    }

    #[test]
    fn test_this_form_as_parameter_no_diagnostic() {
        let code = r#"
Функция ФункцияСПараметром(ЭтаФорма)
    ЭтаФорма = ПолучитьЭтуФорму();
    ГлобалтныйМетод(ЭтаФорма);
    ЭтаФорма.Закрыть();
    Возврат ЭтаФорма;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let this_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingThisForm).collect();

        assert_eq!(this_form_diags.len(), 0);
    }

    #[test]
    fn test_this_form_function_call_no_diagnostic() {
        let code = r#"
Процедура Тест()
    ЭтаФорма();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let this_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingThisForm).collect();

        assert_eq!(this_form_diags.len(), 0);
    }

    #[test]
    fn test_module_this_form_method_no_diagnostic() {
        let code = r#"
Процедура Тест()
    Модуль.ЭтаФорма();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let this_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingThisForm).collect();

        assert_eq!(this_form_diags.len(), 0);
    }

    #[test]
    fn test_structure_field_no_diagnostic() {
        let code = r#"
Процедура Тест()
    Струткура.ЭтаФорма = "123";
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let this_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingThisForm).collect();

        assert_eq!(this_form_diags.len(), 0);
    }

    #[test]
    fn test_this_form_english() {
        let code = r#"
Procedure Test()
    GlobalMethod(ThisForm);
    ThisForm.Close();
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let this_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingThisForm).collect();

        assert_eq!(this_form_diags.len(), 2);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/UsingThisFormDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(input);

        let this_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingThisForm).collect();

        assert_eq!(
            this_form_diags.len(),
            16,
            "Expected 16 diagnostics, got {}",
            this_form_diags.len()
        );

        // Lines 3-6: function without ЭтаФорма parameter - 4 diagnostics
        assert_diagnostic_range_multiline(input, this_form_diags[0], 3, 20, 3, 28);
        assert_diagnostic_range_multiline(input, this_form_diags[1], 4, 29, 4, 37);
        assert_diagnostic_range_multiline(input, this_form_diags[2], 5, 4, 5, 12);
        assert_diagnostic_range_multiline(input, this_form_diags[3], 6, 12, 6, 20);

        // Lines 13-16: procedure without ЭтаФорма parameter - 4 diagnostics
        assert_diagnostic_range_multiline(input, this_form_diags[4], 13, 19, 13, 27);
        assert_diagnostic_range_multiline(input, this_form_diags[5], 14, 20, 14, 28);
        assert_diagnostic_range_multiline(input, this_form_diags[6], 15, 33, 15, 41);
        assert_diagnostic_range_multiline(input, this_form_diags[7], 16, 12, 16, 20);

        // Lines 40-47: module-level code - 7 diagnostics
        assert_diagnostic_range_multiline(input, this_form_diags[8], 40, 16, 40, 24);
        assert_diagnostic_range_multiline(input, this_form_diags[9], 41, 25, 41, 33);
        assert_diagnostic_range_multiline(input, this_form_diags[10], 42, 0, 42, 8);
        assert_diagnostic_range_multiline(input, this_form_diags[11], 44, 76, 44, 84);
        assert_diagnostic_range_multiline(input, this_form_diags[12], 45, 8, 45, 16);
        assert_diagnostic_range_multiline(input, this_form_diags[13], 47, 14, 47, 22);
        assert_diagnostic_range_multiline(input, this_form_diags[14], 47, 24, 47, 32);

        // Line 54: ЭтаФорма.Реквизит = "123" - 1 diagnostic
        assert_diagnostic_range_multiline(input, this_form_diags[15], 54, 0, 54, 8);
    }
}
