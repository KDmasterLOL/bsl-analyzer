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
//!
//! Ported from:

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
    fn test_comprehensive() {
        let code = include_str!("../../test_data/CodeAfterAsyncCallDiagnostic.bsl");

        let diagnostics = check_diagnostic(code);

        // Expected 10 diagnostics (from reference test)
        assert_eq!(diagnostics.len(), 10, "Should find 10 diagnostics");

        // Verify exact positions match bsl-language-server test expectations (line:col ranges)
        // Format: hasRange(line, startCol, endCol) from reference test
        assert_diagnostic_range(code, &diagnostics[0], 4, 4, 97);
        assert_diagnostic_range(code, &diagnostics[1], 21, 8, 101);
        assert_diagnostic_range(code, &diagnostics[2], 34, 8, 101);
        assert_diagnostic_range(code, &diagnostics[3], 48, 12, 105);
        assert_diagnostic_range(code, &diagnostics[4], 63, 12, 105);
        assert_diagnostic_range(code, &diagnostics[5], 78, 12, 105);
        assert_diagnostic_range(code, &diagnostics[6], 93, 12, 105);
        assert_diagnostic_range(code, &diagnostics[7], 108, 12, 105);
        assert_diagnostic_range(code, &diagnostics[8], 123, 12, 105);
        assert_diagnostic_range(code, &diagnostics[9], 270, 12, 105);
    }
}
