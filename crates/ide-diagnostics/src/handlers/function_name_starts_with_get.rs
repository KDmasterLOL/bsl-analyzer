use crate::define_metadata;
use crate::metadata::*;
use crate::AnalysisContext;
use crate::{Diagnostic, DiagnosticCode};
use hir::LocalRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    name: &str,
    range: LocalRange,
    ctx: &AnalysisContext,
) -> Option<Diagnostic<LocalRange>> {
    let code = DiagnosticCode::FunctionNameStartsWithGet;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!("Имя функции '{}' не должно начинаться с 'Получить'", name),
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
    use expect_test::expect;

    #[test]
    fn test_function_name_starts_with_get() {
        let code = r#"// Source comment
Функция ПолучитьИмяПоКоду()

КонецФункции

Функция НеПолучитьИмяПоКоду()

КонецФункции

Функция ИмяПоКоду()

КонецФункции

Процедура ПолучитьИмяПоКоду()

КонецПроцедуры

Function GetNameByCode()

EndFunction

Function NotGetNameByCode()

EndFunction

Function NameByCode()

EndFunction

Procedure GetNameByCode()

EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionNameStartsWithGet)
            .collect();

        expect![[r#"
            FunctionNameStartsWithGet @ 2:9..2:26
              message: Имя функции 'ПолучитьИмяПоКоду' не должно начинаться с 'Получить'
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &func_diags));

        assert!(func_diags[0].message.contains("ПолучитьИмяПоКоду"));
    }

    #[test]
    fn test_no_get_prefix() {
        let code = r#"
Функция ИмяПоКоду()
    Возврат "Имя";
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionNameStartsWithGet)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Функция ПОЛУЧИТЬДАННЫЕ()
    Возврат "Данные";
КонецФункции

Функция получитьзначение()
    Возврат 42;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionNameStartsWithGet)
            .collect();
        expect![[r#"
            FunctionNameStartsWithGet @ 2:9..2:23
              message: Имя функции 'ПОЛУЧИТЬДАННЫЕ' не должно начинаться с 'Получить'
              severity: Hint
            FunctionNameStartsWithGet @ 6:9..6:25
              message: Имя функции 'получитьзначение' не должно начинаться с 'Получить'
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_procedure_not_detected() {
        let code = r#"
Процедура ПолучитьИмяПоКоду()
    // Процедура не должна срабатывать
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionNameStartsWithGet)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_english_get_not_detected() {
        let code = r#"
Function GetNameByCode()
    Return "Name";
EndFunction
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionNameStartsWithGet)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_partial_match_not_detected() {
        let code = r#"
Функция НеПолучитьИмяПоКоду()
    Возврат "Имя";
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionNameStartsWithGet)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &func_diags));
    }
}
