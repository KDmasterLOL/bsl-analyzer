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
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    method_name: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::CodeAfterAsyncCall;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "После вызова асинхронного метода '{}' есть строки кода. Код выполнится немедленно, не дожидаясь завершения асинхронной операции",
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
    use crate::test_utils::{check_hir_diagnostic, format_diags};
    use crate::DiagnosticCode;
    use expect_test::expect;

    fn check_diagnostic(code: &str) -> Vec<crate::Diagnostic> {
        check_hir_diagnostic(code)
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::CodeAfterAsyncCall)
            .collect()
    }

    #[test]
    fn test_no_code_after_async() {
        let code = r#"
Процедура Тест()
    ПоказатьВводЧисла(Оповещение, 1, "Текст", 10, 2);
    // Только комментарий
КонецПроцедуры
"#;

        let diagnostics = check_diagnostic(code);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_code_after_async() {
        let code = r#"
Процедура Тест()
    ПоказатьВводЧисла(Оповещение, 1, "Текст", 10, 2);
    Сообщить("Ошибка!");
КонецПроцедуры
"#;

        let diagnostics = check_diagnostic(code);
        expect![[r#"
            CodeAfterAsyncCall @ 3:5..3:54
              message: После вызова асинхронного метода 'ПоказатьВводЧисла' есть строки кода. Код выполнится немедленно, не дожидаясь завершения асинхронной операции
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_return_after_async() {
        let code = r#"
Процедура Тест()
    ПоказатьВводЧисла(Оповещение, 1, "Текст", 10, 2);
    Возврат;
КонецПроцедуры
"#;

        let diagnostics = check_diagnostic(code);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_async_in_if_with_code_after() {
        let code = r#"
Процедура Тест()
    Если Условие Тогда
        ПоказатьВводЧисла(Оповещение, 1, "Текст", 10, 2);
    КонецЕсли;
    Сообщить("Ошибка!");
КонецПроцедуры
"#;

        let diagnostics = check_diagnostic(code);
        expect![[r#"
            CodeAfterAsyncCall @ 4:9..4:58
              message: После вызова асинхронного метода 'ПоказатьВводЧисла' есть строки кода. Код выполнится немедленно, не дожидаясь завершения асинхронной операции
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_english_methods() {
        let code = r#"
Procedure Test()
    ShowInputNumber(Notification, 1, "Text", 10, 2);
    Message("Error!");
EndProcedure
"#;

        let diagnostics = check_diagnostic(code);
        expect![[r#"
            CodeAfterAsyncCall @ 3:5..3:53
              message: После вызова асинхронного метода 'ShowInputNumber' есть строки кода. Код выполнится немедленно, не дожидаясь завершения асинхронной операции
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_qualified_call_ignored() {
        let code = r#"
Процедура Тест()
    Форма.ПоказатьВводЧисла(Оповещение, 1, "Текст", 10, 2);
    Сообщить("OK");
КонецПроцедуры
"#;

        let diagnostics = check_diagnostic(code);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_break_after_async_in_loop() {
        let code = r#"
Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        ПоказатьВводЧисла(Оповещение, 1, "Текст", 10, 2);
        Прервать;
    КонецЦикла;
КонецПроцедуры
"#;

        let diagnostics = check_diagnostic(code);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_code_after_async_at_top_level() {
        let code = r#"Процедура Команда1(Команда)
    ДополнительныеПараметры = Новый Структура("Результат", 10);
    Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
    ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
    Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат);
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        expect![[r#"
            CodeAfterAsyncCall @ 4:5..4:98
              message: После вызова асинхронного метода 'ПоказатьВводЧисла' есть строки кода. Код выполнится немедленно, не дожидаясь завершения асинхронной операции
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_code_after_async_in_if_branch() {
        let code = r#"Процедура Команда2(Команда)
    Если Условие Тогда
        ДополнительныеПараметры = Новый Структура("Результат", 10);
        Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
        ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
        Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат);
    Иначе
        ДругаяВетка();
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        expect![[r#"
            CodeAfterAsyncCall @ 5:9..5:102
              message: После вызова асинхронного метода 'ПоказатьВводЧисла' есть строки кода. Код выполнится немедленно, не дожидаясь завершения асинхронной операции
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_code_after_async_outside_if() {
        let code = r#"Процедура Команда3(Команда)
    Если Условие Тогда
        ДополнительныеПараметры = Новый Структура("Результат", 10);
        Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
        ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
    Иначе
        ДругаяВетка();
    КонецЕсли;
    Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат);
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        expect![[r#"
            CodeAfterAsyncCall @ 5:9..5:102
              message: После вызова асинхронного метода 'ПоказатьВводЧисла' есть строки кода. Код выполнится немедленно, не дожидаясь завершения асинхронной операции
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_code_after_async_in_nested_if() {
        let code = r#"Процедура Команда4(Команда)
    Если Условие Тогда
        ДополнительныеПараметры = Новый Структура("Результат", 10);
        Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
        Если Условие Тогда
            ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
        КонецЕсли;
    Иначе
        ДругаяВетка();
    КонецЕсли;
    Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат);
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        expect![[r#"
            CodeAfterAsyncCall @ 6:13..6:106
              message: После вызова асинхронного метода 'ПоказатьВводЧисла' есть строки кода. Код выполнится немедленно, не дожидаясь завершения асинхронной операции
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_code_after_async_in_nested_if_same_block() {
        let code = r#"Процедура КодПослеКонецЕсли(Команда)
    Если Условие Тогда
        ДополнительныеПараметры = Новый Структура("Результат", 10);
        Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
        Если Условие Тогда
            ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
        КонецЕсли;
        Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат);
    Иначе
        ДругаяВетка();
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        expect![[r#"
            CodeAfterAsyncCall @ 6:13..6:106
              message: После вызова асинхронного метода 'ПоказатьВводЧисла' есть строки кода. Код выполнится немедленно, не дожидаясь завершения асинхронной операции
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_code_after_async_in_while_loop() {
        let code = r#"Процедура КодПослеЦикла(Команда)
    Если Условие Тогда
        ДополнительныеПараметры = Новый Структура("Результат", 10);
        Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
        Пока Условие Цикл
            ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
        КонецЦикла;
        Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат);
    Иначе
        ДругаяВетка();
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn test_code_after_async_in_for_each_loop() {
        let code = r#"Процедура КодПослеЦиклаДляКаждого(Команда)
    Если Условие Тогда
        ДополнительныеПараметры = Новый Структура("Результат", 10);
        Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
        Для Каждого Элемент Из Коллекция Цикл
            ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
        КонецЦикла;
        Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат);
    Иначе
        ДругаяВетка();
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn test_code_after_async_in_for_to_loop() {
        let code = r#"Процедура КодПослеЦиклаДляПо(Команда)
    Если Условие Тогда
        ДополнительныеПараметры = Новый Структура("Результат", 10);
        Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
        Для Счетчик = 1 По 10 Цикл
            ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
        КонецЦикла;
        Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат);
    Иначе
        ДругаяВетка();
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn test_code_after_async_after_try() {
        let code = r#"Процедура КодПослеПопытки(Команда)
    Если Условие Тогда
        ДополнительныеПараметры = Новый Структура("Результат", 10);
        Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
        Попытка
            ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
        Исключение
            КодВПопытке = 1;
        КонецПопытки;
        Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат);
    Иначе
        ДругаяВетка();
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        expect![[r#"
            CodeAfterAsyncCall @ 6:13..6:106
              message: После вызова асинхронного метода 'ПоказатьВводЧисла' есть строки кода. Код выполнится немедленно, не дожидаясь завершения асинхронной операции
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_only_async_no_following_code() {
        let code = r#"Процедура БезОшибок1(Команда)
    ПоказатьВводЧисла(Оповещение, 1);
    // комментарий
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_async_in_if_no_following_code() {
        let code = r#"Процедура БезОшибок2(Команда)
    Если Условие Тогда
        ДополнительныеПараметры = Новый Структура("Результат", 10);
        Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
        ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
    Иначе
        ДругаяВетка();
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_return_after_async() {
        let code = r#"Процедура ВозвратПослеАсинхрона(Команда)
    Если Условие Тогда
        ДополнительныеПараметры = Новый Структура("Результат", 10);
        Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
        ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
        Возврат;
    КонецЕсли;
    КодВКонцеМетода();
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_break_after_async_in_loop() {
        let code = r#"Процедура ПрерватьАсинхрона(Команда)
    Если Условие Тогда
        Для Каждого Элемент Из Коллекция Цикл
            ДополнительныеПараметры = Новый Структура("Результат", 10);
            Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
            ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
            Прервать;
        КонецЦикла;
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_code_after_loop_with_break_after_async() {
        let code = r#"Процедура ПрерватьПослеАсинхронаИКодПослеЦикла(Команда)
    Если Условие Тогда
        Для Каждого Элемент Из Коллекция Цикл
            ДополнительныеПараметры = Новый Структура("Результат", 10);
            Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
            ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
            Прервать;
        КонецЦикла;
    КонецЕсли;
    КодПослеЦикла();
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        expect![[r#"
            CodeAfterAsyncCall @ 6:13..6:106
              message: После вызова асинхронного метода 'ПоказатьВводЧисла' есть строки кода. Код выполнится немедленно, не дожидаясь завершения асинхронной операции
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_diagnostic_async_in_mutually_exclusive_branches() {
        let code = r#"Процедура ДваВызоваАсинхронаВоВзаимоисключащихВетках(Команда)
    ДополнительныеПараметры = Новый Структура("Результат", 10);
    Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
    Если Условие Тогда
        ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
    Иначе
        ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }
}
