use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Lockinos],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Adaptable,
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::UsingObjectNotAvailableUnix` is encountered.
pub fn from_hir(type_name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::UsingObjectNotAvailableUnix;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Проверить, что задействованы аналоги \"{}\" при работе в Unix-клиенте.",
            type_name
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;

    fn filter(diagnostics: &[crate::Diagnostic]) -> Vec<&crate::Diagnostic> {
        diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingObjectNotAvailableUnix).collect()
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/UsingObjectNotAvailableUnixDiagnostic.bsl");
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);

        assert_eq!(diags.len(), 3, "Expected 3 diagnostics, got {}", diags.len());

        assert_diagnostic_range(code, diags[0], 3, 11, 54);
        assert_diagnostic_range(code, diags[1], 4, 11, 83);
        assert_diagnostic_range(code, diags[2], 20, 9, 20);
    }

    #[test]
    fn test_com_object_without_guard() {
        let code = r#"
Процедура Тест()
    obj = Новый COMОбъект("test");
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::UsingObjectNotAvailableUnix);
    }

    #[test]
    fn test_mail_without_guard() {
        let code = r#"
Процедура Тест()
    Почта = Новый Почта;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 1);
    }

    #[test]
    fn test_with_linux_guard() {
        let code = r#"
Процедура Тест()
    Если СистемнаяИнформация.ТипПлатформы = ТипПлатформы.Linux_x86 Тогда
        Почта = Новый Почта;
    КонецЕсли;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "Should not trigger with Linux guard");
    }

    #[test]
    fn test_with_windows_guard() {
        let code = r#"
Процедура Тест()
    Если СистемнаяИнформация.ТипПлатформы = ТипПлатформы.Windows_x86 Тогда
        Почта = Новый Почта;
    КонецЕсли;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "Should not trigger with Windows guard");
    }

    #[test]
    fn test_with_macos_guard() {
        let code = r#"
Процедура Тест()
    Если СистемнаяИнформация.ТипПлатформы = ТипПлатформы.MacOS_x86 Тогда
        Почта = Новый Почта;
    КонецЕсли;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "Should not trigger with MacOS guard");
    }

    #[test]
    fn test_nested_if_with_guard() {
        let code = r#"
Процедура Тест()
    Если Не СистемнаяИнформация.ТипПлатформы = ТипПлатформы.Linux_x86 Тогда
        Если Истина Тогда
            Почта = Новый Почта;
        КонецЕсли;
    КонецЕсли;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "Should not trigger with nested guard");
    }

    #[test]
    fn test_internet_mail_not_triggered() {
        let code = r#"
Процедура Тест()
    Почта = Новый ИнтернетПочта();
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "ИнтернетПочта should not trigger");
    }

    #[test]
    fn test_english_mail() {
        let code = r#"
Процедура Тест()
    m = New Mail;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 1, "English Mail should trigger");
    }

    #[test]
    fn test_english_com_object() {
        let code = r#"
Процедура Тест()
    obj = New COMObject("test");
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 1, "English COMObject should trigger");
    }

    #[test]
    fn test_with_platform_guard() {
        let code = r#"
Процедура Тест()
    Если ТипПлатформы.Windows Тогда
        obj = Новый COMОбъект("test");
    КонецЕсли;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "HIR should NOT detect with Windows guard");
    }
}
