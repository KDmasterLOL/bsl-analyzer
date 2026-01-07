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
//! - FormDataToValueDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - form_data_to_value.rs (bsl-language-server-rust) - Reference implementation

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when FormDataToValue diagnostic is emitted during lowering.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::FormDataToValue) {
        return None;
    }

    Some(Diagnostic {
        code: DiagnosticCode::FormDataToValue,
        message: "Use of FormDataToValue method detected".to_string(),
        range,
        severity: Severity::Information,
        tags: vec![],
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/FormDataToValueDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();

        assert_eq!(form_diags.len(), 4, "Expected 4 diagnostics");

        // Java reports 1-indexed, Rust uses 0-indexed
        // Java Line 3 = Rust Line 2, etc.
        assert_diagnostic_range(code, form_diags[0], 2, 15, 35); // Line 3: Форма.ДанныеФормыВЗначение
        assert_diagnostic_range(code, form_diags[1], 7, 9, 29); // Line 8: ДанныеФормыВЗначение
        assert_diagnostic_range(code, form_diags[2], 22, 14, 29); // Line 23: Form.FormDataToValue
        assert_diagnostic_range(code, form_diags[3], 26, 4, 19); // Line 27: FormDataToValue
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
