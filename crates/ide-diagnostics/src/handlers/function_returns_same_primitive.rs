use crate::define_metadata;
use crate::metadata::*;
use crate::AnalysisContext;
use crate::{Diagnostic, DiagnosticCode};
use hir::LocalRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: LocalRange, ctx: &AnalysisContext) -> Option<Diagnostic<LocalRange>> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::FunctionReturnsSamePrimitive,
        "Функция всегда возвращает одно и то же примитивное значение. \
         Замените функцию на константу или переменную модуля.",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_fixture_all_branches_return_true() {
        let code = r#"Функция ПроверитьСтроку(Знач СтрокаТаблицы)
    Если ЭтоХорошаяСтрока(СтрокаТаблицы) Тогда
        ДелаемЧтоТо();
        Возврат Истина;
    ИначеЕсли ЭтоТожеНеплохаяСтрока(СтрокаТаблицы) Тогда
        ДелаемДругоеЧтоТо();
        Возврат Истина;
     Иначе
        Возврат Истина;
    КонецЕсли;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#"
            FunctionReturnsSamePrimitive @ 1:9..1:24
              message: Функция всегда возвращает одно и то же примитивное значение. Замените функцию на константу или переменную модуля.
              severity: Major"#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_fixture_all_branches_return_same_string() {
        let code = r#"Функция Метод1()
    Значение = "Фича";
    Если Фича = "Дирижабль" Тогда
        Возврат "Фича";
    ИначеЕсли Фича = "Ага" Тогда
        Возврат "Фича";
    КонецЕсли;
    Возврат "Фича";
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#"
            FunctionReturnsSamePrimitive @ 1:9..1:15
              message: Функция всегда возвращает одно и то же примитивное значение. Замените функцию на константу или переменную модуля.
              severity: Major"#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_fixture_all_branches_return_same_number() {
        let code = r#"Функция СтавкаНДС(Ставка)
    Значение = 20;
    Если Ставка = "Ставка18" Тогда
        Возврат 20;
    КонецЕсли;
    Возврат 20;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#"
            FunctionReturnsSamePrimitive @ 1:9..1:18
              message: Функция всегда возвращает одно и то же примитивное значение. Замените функцию на константу или переменную модуля.
              severity: Major"#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_fixture_attachable_prefix_skipped() {
        let code = r#"Функция Подключаемый_КакаяТоКоманда(Команда)

    Если ЗначениеЗаполнено(ТекущаяДата) Тогда
        Возврат Null;
    КонецЕсли;

    Возврат NULL;

КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_fixture_non_attachable_null_triggers() {
        let code = r#"Функция КакаяТоКоманда(Команда)

    Если ЗначениеЗаполнено(ТекущаяДата) Тогда
        Возврат Null;
    КонецЕсли;

    Возврат NULL;

КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#"
            FunctionReturnsSamePrimitive @ 1:9..1:23
              message: Функция всегда возвращает одно и то же примитивное значение. Замените функцию на константу или переменную модуля.
              severity: Major"#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_fixture_case_insensitive_string_same() {
        let code = r#"Функция ПроверкаРегистраДляСтрок()

    Тип = 1;
    Если Тип = 1 Тогда
        Возврат "Значение";
    ИначеЕсли Тип = 2 Тогда
        Возврат "значение";
    Иначе
        Возврат "ЗНАЧЕНИЕ";
    КонецЕсли;

КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#"
            FunctionReturnsSamePrimitive @ 1:9..1:33
              message: Функция всегда возвращает одно и то же примитивное значение. Замените функцию на константу или переменную модуля.
              severity: Major"#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_single_return_no_diagnostic() {
        let code = r#"
Функция БудемТестироватьФункциональность()
    Возврат Ложь;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_returns_variable_no_diagnostic() {
        let code = r#"
Функция СтавкаНДС2(Ставка)
    Значение = 20;
    Если Ставка = "Ставка18" Тогда
        Возврат Значение;
    КонецЕсли;
    Возврат Значение;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_different_primitives_no_diagnostic() {
        let code = r#"
Функция Проверка(Условие)
    Если Условие Тогда
        Возврат Истина;
    Иначе
        Возврат Ложь;
    КонецЕсли;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_same_boolean_triggers() {
        let code = r#"
Функция ПроверитьСтроку(СтрокаТаблицы)
    Если Условие1 Тогда
        Возврат Истина;
    ИначеЕсли Условие2 Тогда
        Возврат Истина;
    Иначе
        Возврат Истина;
    КонецЕсли;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#"
            FunctionReturnsSamePrimitive @ 2:9..2:24
              message: Функция всегда возвращает одно и то же примитивное значение. Замените функцию на константу или переменную модуля.
              severity: Major"#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_same_number_triggers() {
        let code = r#"
Функция СтавкаНДС(Ставка)
    Если Ставка = "Ставка18" Тогда
        Возврат 20;
    КонецЕсли;
    Возврат 20;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#"
            FunctionReturnsSamePrimitive @ 2:9..2:18
              message: Функция всегда возвращает одно и то же примитивное значение. Замените функцию на константу или переменную модуля.
              severity: Major"#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_same_string_triggers() {
        let code = r#"
Функция Метод1()
    Если Фича = "Дирижабль" Тогда
        Возврат "Фича";
    ИначеЕсли Фича = "Ага" Тогда
        Возврат "Фича";
    КонецЕсли;
    Возврат "Фича";
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#"
            FunctionReturnsSamePrimitive @ 2:9..2:15
              message: Функция всегда возвращает одно и то же примитивное значение. Замените функцию на константу или переменную модуля.
              severity: Major"#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_null_case_insensitive() {
        let code = r#"
Функция КакаяТоКоманда(Команда)
    Если ЗначениеЗаполнено(ТекущаяДата) Тогда
        Возврат Null;
    КонецЕсли;
    Возврат NULL;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#"
            FunctionReturnsSamePrimitive @ 2:9..2:23
              message: Функция всегда возвращает одно и то же примитивное значение. Замените функцию на константу или переменную модуля.
              severity: Major"#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_attachable_skipped() {
        let code = r#"
Функция Подключаемый_КакаяТоКоманда(Команда)
    Если ЗначениеЗаполнено(ТекущаяДата) Тогда
        Возврат Null;
    КонецЕсли;
    Возврат NULL;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &func_diags));
    }

    #[test]
    fn test_attachable_english_skipped() {
        let code = r#"
Function Attachable_RandomAction(Command)
    If ValueIsFilled(CurrentDate) Then
        Return Undefined;
    EndIf;
    Return Undefined;
EndFunction
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &func_diags));
    }
}
