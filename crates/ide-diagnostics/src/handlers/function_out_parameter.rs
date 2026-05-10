//! Reports function parameters used as output parameters.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates a diagnostic from the HIR lowering result.
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::FunctionOutParameter;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Функция изменяет параметр '{}'. Используйте возвращаемое значение вместо выходного параметра",
            name
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
    use expect_test::expect;

    #[test]
    fn test_function_out_parameter() {
        let code = r#"Процедура А(А, Знач Б)
    А = 1;
КонецПроцедуры

Функция Б(А, Знач Б)
    а = 1;

    Если А = 1 Тогда
    КонецЕсли;

    Б = 2;

КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionOutParameter)
            .collect();

        // Only function Б modifies parameter А (by-reference); procedure А is allowed
        expect![[r#"
            FunctionOutParameter @ 6:5..6:6
              message: Функция изменяет параметр 'а'. Используйте возвращаемое значение вместо выходного параметра
              severity: Warning"#]].assert_eq(&format_diags(code, &func_diags));
        assert!(func_diags[0].message.contains("а")); // snapshot-skip: message-substring assertion intentionally retained.
    }

    #[test]
    fn test_procedure_allowed() {
        let code = r#"
Процедура Тест(А, Знач Б)
    А = 1;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionOutParameter)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_val_parameter_not_flagged() {
        let code = r#"
Функция Тест(Знач А, Знач Б)
    А = 1;
    Возврат А;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionOutParameter)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Функция Тест(Параметр)
    ПАРАМЕТР = 1;
    Возврат ПАРАМЕТР;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionOutParameter)
            .collect();
        expect![[r#"
            FunctionOutParameter @ 3:5..3:13
              message: Функция изменяет параметр 'ПАРАМЕТР'. Используйте возвращаемое значение вместо выходного параметра
              severity: Warning"#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_only_simple_assignment() {
        let code = r#"
Функция Тест(Объект)
    Объект.Свойство = 1;
    Возврат Объект;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionOutParameter)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_multiple_violations() {
        let code = r#"
Функция Обработка(Данные, Результат)
    Данные = Новый Массив;
    Результат = ОбработатьДанные(Данные);
    Возврат Истина;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionOutParameter)
            .collect();
        expect![[r#"
            FunctionOutParameter @ 3:5..3:11
              message: Функция изменяет параметр 'Данные'. Используйте возвращаемое значение вместо выходного параметра
              severity: Warning
            FunctionOutParameter @ 4:5..4:14
              message: Функция изменяет параметр 'Результат'. Используйте возвращаемое значение вместо выходного параметра
              severity: Warning"#]].assert_eq(&format_diags(code, &func_diags));
    }
}
