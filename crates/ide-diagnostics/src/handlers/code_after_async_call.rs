//! CodeAfterAsyncCall diagnostic.
//!
//! Detects code that executes immediately after asynchronous method calls in BSL.
//!
//! ## Why?
//! When using asynchronous methods in 1C:Enterprise client-side code, developers sometimes
//! make the mistake of writing code immediately after an async call. This code executes
//! synchronously without waiting for the async operation to complete, leading to logic errors.
//!
//! Asynchronous methods return immediately and execute in the background. Any code after the
//! async call will execute BEFORE the async operation completes. To properly handle async
//! results, you must use callback functions (`ОписаниеОповещения`/`NotifyDescription`) or
//! async/await patterns.
//!
//! ## Bad practice
//! ```bsl
//! &НаКлиенте
//! Процедура Команда1(Команда)
//!     ДополнительныеПараметры = Новый Структура("Результат", 10);
//!     Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества", ЭтотОбъект);
//!     ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
//!
//!     Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат); // ERROR! Always shows 10
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! Move code that depends on async results into the callback function:
//! ```bsl
//! &НаКлиенте
//! Процедура Команда1(Команда)
//!     ДополнительныеПараметры = Новый Структура("Результат", 10);
//!     Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества", ЭтотОбъект);
//!     ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
//! КонецПроцедуры
//!
//! &НаКлиенте
//! Процедура ПослеВводаКоличества(Число, ДополнительныеПараметры) Экспорт
//!     Если Число <> Неопределено Тогда
//!         ДополнительныеПараметры.Результат = Число;
//!         Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат); // Correct!
//!     КонецЕсли;
//! КонецПроцедуры
//! ```
//!
//! Or use async/await:
//! ```bsl
//! &НаКлиенте
//! Асинх Процедура Команда1(Команда)
//!     Число = Ждать ПоказатьВводЧислаАсинх(1, "Введите количество", 10, 2);
//!     Если Число <> Неопределено Тогда
//!         Сообщить("Введенное количество равно " + Число); // Correct with async/await
//!     КонецЕсли;
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** No (must be enabled via config)
//! - **Severity:** Warning
//! - **Tags:** SUSPICIOUS
//! - **Minutes to fix:** 10
//!
//! ## Implementation
//!
//! This diagnostic is collected during HIR lowering as a byproduct of
//! statement processing. The `from_hir` function converts the BodyDiagnostic
//! to a Diagnostic for display.

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

/// Creates diagnostic from HIR BodyDiagnostic (called from lib.rs dispatch).
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
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;

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
        assert_eq!(diagnostics.len(), 0, "No code after async should be valid");
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
        assert_eq!(diagnostics.len(), 1, "Code after async should be an error");
        // Line 2 (0-indexed from start of code including leading newline)
        assert_diagnostic_range(code, &diagnostics[0], 2, 4, 53);
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
        assert_eq!(diagnostics.len(), 0, "Return after async should be valid");
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
        assert_eq!(diagnostics.len(), 1, "Code after IF containing async should be an error");
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
        assert_eq!(diagnostics.len(), 1, "English async methods should be detected");
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
        assert_eq!(diagnostics.len(), 0, "Qualified calls should be ignored");
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
        assert_eq!(diagnostics.len(), 0, "Break after async in loop should be valid");
    }

    #[test]
    fn test_code_after_async_at_top_level() {
        // Команда1: async call followed directly by code — should trigger
        let code = r#"Процедура Команда1(Команда)
    ДополнительныеПараметры = Новый Структура("Результат", 10);
    Оповещение = Новый ОписаниеОповещения("ПослеВводаКоличества1", ЭтотОбъект);
    ПоказатьВводЧисла(Оповещение, 1, "Введите количество", ДополнительныеПараметры.Результат, 2);
    Сообщить("Введенное количество равно " + ДополнительныеПараметры.Результат);
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_code_after_async_in_if_branch() {
        // Команда2: async inside if-branch, followed by code in same branch — should trigger
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
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_code_after_async_outside_if() {
        // Команда3: async in if-branch, code AFTER КонецЕсли — should trigger
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
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_code_after_async_in_nested_if() {
        // Команда4: nested if with async, code after outer КонецЕсли — should trigger
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
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_code_after_async_in_nested_if_same_block() {
        // КодПослеКонецЕсли: async in nested if, code after inner КонецЕсли in same outer branch
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
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_code_after_async_in_while_loop() {
        // КодПослеЦикла: async in while loop, code after КонецЦикла
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
        // While loop containing async + code after = 2 diagnostics (loop itself + code after)
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn test_code_after_async_in_for_each_loop() {
        // КодПослеЦиклаДляКаждого: async in for-each loop, code after КонецЦикла
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
        // КодПослеЦиклаДляПо: async in for-to loop, code after КонецЦикла
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
        // КодПослеПопытки: async in try, code after КонецПопытки
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
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_diagnostic_only_async_no_following_code() {
        // БезОшибок1: async + comment only — should NOT trigger
        let code = r#"Процедура БезОшибок1(Команда)
    ПоказатьВводЧисла(Оповещение, 1);
    // комментарий
КонецПроцедуры"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_async_in_if_no_following_code() {
        // БезОшибок2: async only in if-branch, nothing after КонецЕсли — should NOT trigger
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
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_return_after_async() {
        // ВозвратПослеАсинхрона: return after async is allowed
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
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_break_after_async_in_loop() {
        // ПрерватьАсинхрона: break after async in loop is allowed
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
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_code_after_loop_with_break_after_async() {
        // ПрерватьПослеАсинхронаИКодПослеЦикла: break after async, but code after loop — triggers
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
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_diagnostic_async_in_mutually_exclusive_branches() {
        // ДваВызоваАсинхронаВоВзаимоисключащихВетках: async in each branch — NOT an error
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
        assert_eq!(diagnostics.len(), 0);
    }
}
