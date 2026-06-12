use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use stdx::case::CaseExt;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::DeprecatedMessage;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let message = get_message(name);

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

fn get_message(method_name: &str) -> String {
    let lower = method_name.fold_lower();
    if lower == "сообщить" {
        "Используйте \"ОбщегоНазначения.СообщитьПользователю\" вместо устаревшего \"Сообщить\""
            .to_string()
    } else {
        "Use \"CommonUse.MessageToUser\" instead of deprecated \"Message\"".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::Severity;
    use expect_test::expect;
    #[test]
    fn test_deprecated_russian() {
        let code = r#"
Процедура Тест()
    Сообщить("Операция выполнена");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMessage)
            .collect();

        expect![[r#"
            DeprecatedMessage @ 3:5..3:13
              message: Используйте "ОбщегоНазначения.СообщитьПользователю" вместо устаревшего "Сообщить"
              severity: Information"#]].assert_eq(&format_diags(code, &deprecated_diags));
        assert_eq!(deprecated_diags[0].severity, Severity::Information);
        assert!(deprecated_diags[0].message.contains("СообщитьПользователю"));
    }

    #[test]
    fn test_deprecated_english() {
        let code = r#"
Procedure Test()
    Message("Operation completed");
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMessage)
            .collect();

        expect![[r#"
            DeprecatedMessage @ 3:5..3:12
              message: Use "CommonUse.MessageToUser" instead of deprecated "Message"
              severity: Information"#]]
        .assert_eq(&format_diags(code, &deprecated_diags));
        assert!(deprecated_diags[0].message.contains("MessageToUser"));
    }

    #[test]
    fn test_object_method_excluded() {
        let code = r#"
Процедура Тест()
    Модуль.Сообщить("Операция выполнена");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMessage)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &deprecated_diags));
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    СООБЩИТЬ("A");
    сообщить("B");
    Сообщить("C");
    СообЩить("D");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMessage)
            .collect();

        expect![[r#"
            DeprecatedMessage @ 3:5..3:13
              message: Используйте "ОбщегоНазначения.СообщитьПользователю" вместо устаревшего "Сообщить"
              severity: Information
            DeprecatedMessage @ 4:5..4:13
              message: Используйте "ОбщегоНазначения.СообщитьПользователю" вместо устаревшего "Сообщить"
              severity: Information
            DeprecatedMessage @ 5:5..5:13
              message: Используйте "ОбщегоНазначения.СообщитьПользователю" вместо устаревшего "Сообщить"
              severity: Information
            DeprecatedMessage @ 6:5..6:13
              message: Используйте "ОбщегоНазначения.СообщитьПользователю" вместо устаревшего "Сообщить"
              severity: Information"#]].assert_eq(&format_diags(code, &deprecated_diags));
    }

    #[test]
    fn test_inside_if_block() {
        let code = r#"
Процедура А()
    Если Истина Тогда
        MessaGe("А");
        Модуль.Сообщить();
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMessage)
            .collect();
        expect![[r#"
            DeprecatedMessage @ 4:9..4:16
              message: Use "CommonUse.MessageToUser" instead of deprecated "Message"
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diags));
        assert!(diags[0].message.contains("MessageToUser"));
    }

    #[test]
    fn test_module_level_call() {
        let code = r#"
Сообщить("А");
Модуль.Сообщить();
ДругойМетод();
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMessage)
            .collect();
        expect![[r#"
            DeprecatedMessage @ 2:1..2:9
              message: Используйте "ОбщегоНазначения.СообщитьПользователю" вместо устаревшего "Сообщить"
              severity: Information"#]].assert_eq(&format_diags(code, &diags));
        assert!(diags[0].message.contains("СообщитьПользователю"));
    }
}
