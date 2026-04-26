//! GetFormMethod diagnostic.
//!
//! Detects usage of `ПолучитьФорму()` / `GetForm()` methods.
//!
//! ## Why?
//! Current 1C recommendations prefer opening forms through
//! `ОткрытьФорму()` / `OpenForm()` instead of obtaining a form object first.
//! This diagnostic flags direct `ПолучитьФорму()` / `GetForm()` calls as a
//! conservative project rule built on top of that guidance.
//!
//! ## Bad practice
//! ```bsl
//! Процедура ОткрытьСправочник()
//!     Форма = ПолучитьФорму("Справочник.Номенклатура.ФормаСписка");  // Error!
//!     Форма.Открыть();
//! КонецПроцедуры
//!
//! Процедура ОткрытьДокумент()
//!     Док = Документы.ЗаявкаНаОперацию.СоздатьДокумент();
//!     Форма = Док.ПолучитьФорму("ФормаДокумента");  // Error!
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура ОткрытьСправочник()
//!     ОткрытьФорму("Справочник.Номенклатура.ФормаСписка");  // Correct!
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Major (ERROR)
//! - **Tags:** ERROR
//! - **Minutes to fix:** 15
//!
//! ## References
//! Checks both global and object method calls emitted from local HIR lowering.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when GetFormMethod diagnostic is emitted during lowering.
pub fn from_hir(
    method_name: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::GetFormMethod;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Использование метода '{}' приводит к ошибкам. Используйте 'ОткрытьФорму()' вместо него",
            method_name
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
    fn test_no_get_form() {
        let code = r#"
Процедура ОткрытьСправочник()
    ОткрытьФорму("Справочник.Номенклатура.ФормаСписка");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let get_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::GetFormMethod).collect();
        assert_eq!(get_form_diags.len(), 0, "Should NOT detect - using ОткрытьФорму");
    }

    #[test]
    fn test_global_get_form_russian() {
        let code = r#"
Процедура Тест2()
    ФормаРедактора = ПолучитьФорму("Обработка.УниверсальныйРедактор.Форма");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let get_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::GetFormMethod).collect();
        assert_eq!(get_form_diags.len(), 1, "Should detect global ПолучитьФорму");
        assert!(get_form_diags[0].message.contains("ПолучитьФорму"));
    }

    #[test]
    fn test_global_get_form_english() {
        let code = r#"
Procedure Test2()
    Form = GetForm("Document.PlanOperation.Form");
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let get_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::GetFormMethod).collect();
        assert_eq!(get_form_diags.len(), 1, "Should detect global GetForm");
        assert!(get_form_diags[0].message.contains("GetForm"));
    }

    #[test]
    fn test_object_method_get_form_russian() {
        let code = r#"
Процедура Тест()
    Док = Документы.ЗаявкаНаОперацию.СоздатьДокумент();
    Форма = Док.ПолучитьФорму("ФормаДокумента");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let get_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::GetFormMethod).collect();
        assert_eq!(get_form_diags.len(), 1, "Should detect object method ПолучитьФорму");
    }

    #[test]
    fn test_object_method_get_form_english() {
        let code = r#"
Procedure Test()
    Doc = Documents.PlanOperation.CreateDocument();
    Form = Doc.GetForm("DocumentForm");
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let get_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::GetFormMethod).collect();
        assert_eq!(get_form_diags.len(), 1, "Should detect object method GetForm");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Форма1 = получитьформу("Форма1");
    Форма2 = ПОЛУЧИТЬФОРМУ("Форма2");
    Форма3 = ПолучитьФОРМУ("Форма3");
    Форма4 = getform("Form4");
    Форма5 = GETFORM("Form5");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let get_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::GetFormMethod).collect();
        assert_eq!(get_form_diags.len(), 5, "Should detect all case variations");
    }

    #[test]
    fn test_multiple_calls() {
        let code = r#"
Процедура Тест()
    Форма1 = ПолучитьФорму("Форма1");
    Форма2 = GetForm("Form2");
    Док = Документы.Документ.СоздатьДокумент();
    Форма3 = Док.ПолучитьФорму("Форма3");
    Форма4 = Док.GetForm("Form4");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let get_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::GetFormMethod).collect();
        assert_eq!(get_form_diags.len(), 4, "Should detect all 4 calls");
    }

    #[test]
    fn test_detects_mixed_get_form_calls() {
        let code = r#"Процедура Тест()
    Док=Документы.ЗаявкаНаОперацию.СоздатьДокумент();
    Форма=Док.ПолучитьФорму("ФормаДокумента"); // Срабатывание здесь
КонецПроцедуры

Процедура Тест2()
    ФормаРедактора = ПолучитьФорму("Обработка.УниверсальныйРедактор.Форма"); // срабатывание здесь
КонецПроцедуры

Procedure Test()
    Doc = Documents.PlanOperation.CreateDocument();
    Form = Doc.GetForm("DocumentForm"); // срабатывание здесь
EndProcedure

Procedure Test2()
    Form = GetForm("Document.PlanOperation.Form"); // срабатывание здесь
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let get_form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::GetFormMethod).collect();

        // Expected 4 diagnostics
        assert_eq!(get_form_diags.len(), 4, "Expected 4 diagnostics");

        // Expected diagnostic ranges (0-based lines):

        assert_diagnostic_range(code, get_form_diags[0], 2, 14, 27);
        assert_diagnostic_range(code, get_form_diags[1], 6, 21, 34);
        assert_diagnostic_range(code, get_form_diags[2], 11, 15, 22);
        assert_diagnostic_range(code, get_form_diags[3], 15, 11, 18);
    }
}
