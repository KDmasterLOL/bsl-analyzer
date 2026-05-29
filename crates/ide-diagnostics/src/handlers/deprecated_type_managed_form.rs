use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_14,
    tags: &[MetadataTag::Standard, MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(type_name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::DeprecatedTypeManagedForm;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let message = get_message(type_name);

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

fn get_message(arg_value: &str) -> String {
    let lower = arg_value.to_lowercase();
    if lower == "управляемаяформа" {
        "Использование устаревшего типа \"УправляемаяФорма\". \
         Рекомендуется использовать \"ФормаКлиентскогоПриложения\""
            .to_string()
    } else {
        "Usage of deprecated type \"ManagedForm\". \
         Recommended to use \"ClientApplicationForm\""
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::Severity;
    use expect_test::expect;
    #[test]
    fn test_deprecated_type_russian() {
        let code = r#"
Процедура Тест()
    Если ТипЗнч(Форма) = Тип("УправляемаяФорма") Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedTypeManagedForm)
            .collect();

        expect![[r#"
            DeprecatedTypeManagedForm @ 3:30..3:48
              message: Использование устаревшего типа "УправляемаяФорма". Рекомендуется использовать "ФормаКлиентскогоПриложения"
              severity: Hint"#]].assert_eq(&format_diags(code, &deprecated_diags));
        assert_eq!(deprecated_diags[0].severity, Severity::Hint);
        assert!(deprecated_diags[0].message.contains("УправляемаяФорма"));
    }

    #[test]
    fn test_deprecated_type_english() {
        let code = r#"
Procedure Test()
    If TypeOf(Form) = Type("ManagedForm") Then
        Return;
    EndIf;
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedTypeManagedForm)
            .collect();

        expect![[r#"
            DeprecatedTypeManagedForm @ 3:28..3:41
              message: Usage of deprecated type "ManagedForm". Recommended to use "ClientApplicationForm"
              severity: Hint"#]].assert_eq(&format_diags(code, &deprecated_diags));
        assert!(deprecated_diags[0].message.contains("ManagedForm"));
    }

    #[test]
    fn test_string_literal_not_detected() {
        let code = r#"
Процедура Тест()
    Сообщить("УправляемаяФорма");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedTypeManagedForm)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &deprecated_diags));
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Т1 = ТИП("УПРАВЛЯЕМАЯФОРМА");
    Т2 = тип("управляемаяформа");
    Т3 = Тип("УправляемаяФорма");
    Т4 = TYPE("MANAGEDFORM");
    Т5 = type("managedform");
    Т6 = Type("ManagedForm");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedTypeManagedForm)
            .collect();

        expect![[r#"
            DeprecatedTypeManagedForm @ 3:14..3:32
              message: Использование устаревшего типа "УправляемаяФорма". Рекомендуется использовать "ФормаКлиентскогоПриложения"
              severity: Hint
            DeprecatedTypeManagedForm @ 4:14..4:32
              message: Использование устаревшего типа "УправляемаяФорма". Рекомендуется использовать "ФормаКлиентскогоПриложения"
              severity: Hint
            DeprecatedTypeManagedForm @ 5:14..5:32
              message: Использование устаревшего типа "УправляемаяФорма". Рекомендуется использовать "ФормаКлиентскогоПриложения"
              severity: Hint
            DeprecatedTypeManagedForm @ 6:15..6:28
              message: Usage of deprecated type "ManagedForm". Recommended to use "ClientApplicationForm"
              severity: Hint
            DeprecatedTypeManagedForm @ 7:15..7:28
              message: Usage of deprecated type "ManagedForm". Recommended to use "ClientApplicationForm"
              severity: Hint
            DeprecatedTypeManagedForm @ 8:15..8:28
              message: Usage of deprecated type "ManagedForm". Recommended to use "ClientApplicationForm"
              severity: Hint"#]].assert_eq(&format_diags(code, &deprecated_diags));
    }

    #[test]
    fn test_russian_in_if_triggers_string_literal_does_not() {
        let code = r#"Процедура Тест()
    Если ТипЗнч(Форма) = Тип("УправляемаяФорма") Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры

Процедура Тест2()
    Сообщить("УправляемаяФорма");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedTypeManagedForm)
            .collect();

        expect![[r#"
            DeprecatedTypeManagedForm @ 2:30..2:48
              message: Использование устаревшего типа "УправляемаяФорма". Рекомендуется использовать "ФормаКлиентскогоПриложения"
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));
        assert!(diags[0].message.contains("УправляемаяФорма"));
    }

    #[test]
    fn test_english_in_if_triggers() {
        let code = r#"Procedure Test()
    If TypeOf(Form) = Type("ManagedForm") Then
        Return;
    EndIf;
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedTypeManagedForm)
            .collect();

        expect![[r#"
            DeprecatedTypeManagedForm @ 2:28..2:41
              message: Usage of deprecated type "ManagedForm". Recommended to use "ClientApplicationForm"
              severity: Hint"#]].assert_eq(&format_diags(code, &diags));
        assert!(diags[0].message.contains("ManagedForm"));
    }
}
