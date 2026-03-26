//! FormDataToValue diagnostic.
//!
//! Detects use of ДанныеФормыВЗначение() / FormDataToValue() method in context methods.
//!
//! ## Why?
//! Using FormDataToValue() in methods with context is bad practice:
//! - Creates unnecessary form context dependency
//! - May cause performance issues with large data
//! - Better to use direct value manipulation or FormAttributeToValue()
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** INFO
//! - **Type:** CODE_SMELL
//! - **Tags:** BADPRACTICE
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - detects FormDataToValue calls during HIR lowering.
//!
//! Ported from:

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when FormDataToValue diagnostic is emitted during lowering.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::FormDataToValue,
        "Обнаружено использование метода ДанныеФормыВЗначение",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;
    #[test]
    fn test_qualified_call_in_procedure_no_annotation() {
        // Тест(): Форма.ДанныеФормыВЗначение - qualified call in unannotated procedure triggers
        let code = r#"Процедура Тест()
    Форма=Док.ПолучитьФорму("ФормаДокумента");
    ДФ = Форма.ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 1, "Qualified call in plain procedure triggers");
    }

    #[test]
    fn test_global_call_with_server_annotation() {
        // &НаСервере Тест2(): bare ДанныеФормыВЗначение triggers
        let code = r#"&НаСервере
Функция Тест2()
    ДФ = ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецФункции"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 1, "Global call in @НаСервере function triggers");
    }

    #[test]
    fn test_server_no_context_does_not_trigger() {
        // &НаСервереБезКонтекста: should NOT trigger
        let code = r#"&НаСервереБезКонтекста
Процедура Тест2()
    ДФ = ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 0, "БезКонтекста should NOT trigger");
    }

    #[test]
    fn test_client_server_no_context_does_not_trigger() {
        // &НаКлиентеНаСервереБезКонтекста: should NOT trigger
        let code = r#"&НаКлиентеНаСервереБезКонтекста
Процедура Тест2()
    ДФ = ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 0, "НаКлиентеНаСервереБезКонтекста should NOT trigger");
    }

    #[test]
    fn test_english_qualified_call_triggers() {
        // English: Form.FormDataToValue in plain procedure triggers
        let code = r#"Procedure Test()
    Form = Doc.GetForm("DocumentForm");
    FD = Form.FormDataToValue(Object, Type("ValueTable"));
EndProcedure"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 1, "English qualified call in plain procedure triggers");
    }

    #[test]
    fn test_english_global_call_triggers() {
        // English: bare FormDataToValue in plain function triggers
        let code = r#"Function Test2()
    FormDataToValue(Object, Type("ValueTable"));
EndFunction"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 1, "English global call in plain function triggers");
    }

    #[test]
    fn test_global_call_with_context() {
        let code = r#"
Процедура Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 1, "Should detect global call in context method");
    }

    #[test]
    fn test_qualified_call_with_context() {
        let code = r#"
Процедура Тест()
    Форма.ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 1, "Should detect qualified call in context method");
    }

    #[test]
    fn test_no_context_annotation_skipped() {
        let code = r#"
&НаСервереБезКонтекста
Процедура Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 0, "Should skip БезКонтекста methods");
    }

    #[test]
    fn test_client_at_server_no_context_skipped() {
        let code = r#"
&НаКлиентеНаСервереБезКонтекста
Процедура Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 0, "Should skip НаКлиентеНаСервереБезКонтекста");
    }

    #[test]
    fn test_server_annotation_detected() {
        let code = r#"
&НаСервере
Функция Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 1, "Should detect in @НаСервере methods");
    }

    #[test]
    fn test_client_annotation_detected() {
        let code = r#"
&НаКлиенте
Процедура Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 1, "Should detect in @НаКлиенте methods");
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    FormDataToValue(Object, Type("ValueTable"));
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 1, "Should detect English method names");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    ДАННЫЕФОРМЫВЗНАЧЕНИЕ(Объект, Тип("ТаблицаЗначений"));
    ДАННЫЕформыВзначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 2, "Should be case-insensitive");
    }

    #[test]
    fn test_no_call_ignored() {
        let code = r#"
Процедура Тест()
    Метод = ДанныеФормыВЗначение;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        assert_eq!(form_diags.len(), 0, "Method references without calls should be ignored");
    }
}
