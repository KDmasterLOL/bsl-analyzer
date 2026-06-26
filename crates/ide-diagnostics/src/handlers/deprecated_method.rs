use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_platform::deprecation::DeprecationEntry;
use ide_db::TextRange;
use stdx::case::CaseExt;

use super::deprecated_platform_facts::{deprecated_method_fact, replacement_for_name};

pub const DEPRECATED_METHODS_8310: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_10,
    tags: &[MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};
pub const DEPRECATED_METHODS_8317: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_17,
    tags: &[MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let (code, replacement) = get_diagnostic_code_and_replacement(name)?;

    if ctx.config.is_disabled(code) {
        return None;
    }

    let message = get_message(name, replacement);

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

fn get_diagnostic_code_and_replacement(name: &str) -> Option<(DiagnosticCode, &'static str)> {
    let (code, entry) = deprecated_method_fact(name)?;
    Some((code, replacement_for_method(entry, name)?))
}

fn replacement_for_method(entry: &DeprecationEntry, name: &str) -> Option<&'static str> {
    replacement_for_name(entry, name)
}

fn get_message(method_name: &str, replacement: &str) -> String {
    let lower = method_name.fold_lower();
    let is_russian = lower.chars().any(|c| c as u32 > 127);

    if is_russian {
        format!("Метод \"{}\" устарел. Следует использовать \"{}\".", method_name, replacement)
    } else {
        format!("Method \"{}\" is deprecated. You should use \"{}\".", method_name, replacement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{check_hir_diagnostic, format_diags};
    use expect_test::expect;
    #[test]
    fn test_deprecated_8310_russian() {
        let code = r#"
Процедура Тест()
    УстановитьКраткийЗаголовокПриложения("Заголовок");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethods8310)
            .collect();

        expect![[r#"
            DeprecatedMethods8310 @ 3:5..3:54
              message: Метод "УстановитьКраткийЗаголовокПриложения" устарел. Следует использовать "КлиентскоеПриложение.УстановитьКраткийЗаголовок".
              severity: Hint"#]].assert_eq(&format_diags(code, &deprecated_diags));
        assert!(deprecated_diags[0].message.contains("КлиентскоеПриложение"));
    }

    #[test]
    fn test_deprecated_8310_english() {
        let code = r#"
Procedure Test()
    Caption = GetShortApplicationCaption();
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethods8310)
            .collect();

        expect![[r#"
            DeprecatedMethods8310 @ 3:15..3:43
              message: Method "GetShortApplicationCaption" is deprecated. You should use "ClientApplication.GetShortCaption".
              severity: Hint"#]].assert_eq(&format_diags(code, &deprecated_diags));
        assert!(deprecated_diags[0].message.contains("ClientApplication"));
    }

    #[test]
    fn test_deprecated_8317_russian() {
        let code = r#"
Процедура Тест()
    Описание = КраткоеПредставлениеОшибки(ИнформацияОбОшибке());
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethods8317)
            .collect();

        expect![[r#"
            DeprecatedMethods8317 @ 3:16..3:64
              message: Метод "КраткоеПредставлениеОшибки" устарел. Следует использовать "МенеджерОбработкиОшибок.КраткоеПредставлениеОшибки".
              severity: Hint"#]].assert_eq(&format_diags(code, &deprecated_diags));
        let message = &deprecated_diags[0].message;
        assert!(message.contains("МенеджерОбработкиОшибок"));
    }

    #[test]
    fn test_not_triggered_for_method_calls() {
        let code = r#"
Процедура Тест()
    // Метод объекта - не должен триггериться
    Модуль.ПолучитьКраткийЗаголовокПриложения();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| {
                d.code == DiagnosticCode::DeprecatedMethods8310
                    || d.code == DiagnosticCode::DeprecatedMethods8317
            })
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &deprecated_diags));
    }
}
