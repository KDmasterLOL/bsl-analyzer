use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Reports parser errors produced by the BSL syntax parser.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ParseError;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();

    parse
        .errors()
        .iter()
        .map(|err| Diagnostic {
            code,
            message: err.structured().format_ru(),
            severity: ctx.severity(code),
            range: err.range(),
            tags: ctx.tags(code),
            fixes: vec![],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    #[test]
    fn test_parse_error_basic() {
        let code = r#"
Процедура а()

КонецПроцедуры

Процедура в()

    Для Каждого Элемент Из Коллекция Цикл
        Если НЕ Тогда

        КонецЕсли;
    КонецЦикла;

КонецПроцедуры

"#;
        let diagnostics = check_ast_diagnostic(code, super::check);

        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();

        assert!(!parse_errors.is_empty(), "Expected at least one parse error");
    }

    #[test]
    fn test_no_parse_errors_in_valid_code() {
        let code = r#"
Процедура Тест()
    А = 1;
    Б = 2;
    Возврат А + Б;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert_eq!(parse_errors.len(), 0, "Valid code should have no parse errors");
    }

    #[test]
    fn test_parse_error_if_without_condition() {
        let code = r#"
Процедура Тест()
    Если НЕ Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(!parse_errors.is_empty(), "Expected parse error for 'Если НЕ Тогда'");
    }

    #[test]
    fn test_parse_error_unclosed_string() {
        let code = r#"
Процедура Тест()
    А = "незакрытая строка
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(!parse_errors.is_empty(), "Expected parse error for unclosed string");
    }

    #[test]
    fn test_parse_error_bare_identifier() {
        let code = r#"
Процедура Тест()
КонецПроцедуры
HHH
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(!parse_errors.is_empty(), "Expected parse error for bare identifier 'HHH'");
    }

    #[test]
    fn test_parse_error_eof_fixture() {
        let code = r#"Процедура ОтключитьСканерШтрихкодов() Экспорт

КонецПроцедуры
HHH"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(!parse_errors.is_empty(), "Expected parse error for EOF fixture with 'HHH'");
    }

    #[test]
    fn test_parse_error_expected_then() {
        let code = r#"
Процедура Тест()
    Если НЕ КонецЕсли
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(
            parse_errors.iter().any(|d| d.message.contains("Ожидалось 'Тогда'")),
            "Expected parse error message for missing 'Тогда', got: {:?}",
            parse_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_parse_error_expected_rparen() {
        let code = "Процедура Тест(";
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(
            parse_errors.iter().any(|d| d.message == "Ожидалось ')', встречено конец файла"),
            "Expected parse error message for missing ')', got: {:?}",
            parse_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_parse_error_expected_end_procedure() {
        let code = "Процедура Тест()";
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(
            parse_errors.iter().any(|d| d.message.contains("Ожидалось 'КонецПроцедуры'")),
            "Expected parse error message for missing 'КонецПроцедуры', got: {:?}",
            parse_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_no_parse_error_for_bom() {
        // UTF-8 BOM at start of file should not trigger ParseError
        let code = "\u{FEFF}Процедура Тест()\n    А = 1;\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(parse_errors.is_empty(), "BOM should not trigger parse error");
    }

    #[test]
    fn test_return_without_semicolon_before_elseif() {
        let code = r#"
Процедура Тест()
    Если А Тогда
        Возврат
    ИначеЕсли Б Тогда
        Возврат
    Иначе
        Возврат
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(
            parse_errors.is_empty(),
            "Return without semicolon before ИначеЕсли/Иначе/КонецЕсли should not trigger parse error"
        );
    }

    #[test]
    fn test_unicode_letter_in_identifier() {
        let code = r#"
Процедура Тест()
    ПараметрΔE = 1;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(
            parse_errors.is_empty(),
            "Unicode letters (Greek Δ) in identifiers should not trigger parse error"
        );
    }

    #[test]
    fn test_async_procedure_with_await() {
        let code = r#"
&НаКлиенте
Асинх Процедура Сформировать()
    Если ЗначениеЗаполнено(ПутьКФайлу) И Не ЗначениеЗаполнено(Адрес) Тогда
        Результат = Ждать ПоместитьФайлНаСерверАсинх(,,, ПутьКФайлу, УникальныйИдентификатор);
        Если ТипЗнч(Результат) = Тип("ОписаниеПомещенногоФайла") И Не Результат.ПомещениеФайлаОтменено Тогда
            Адрес = Результат.Адрес;
        КонецЕсли;
    КонецЕсли;
    СформироватьОтчет();
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        for e in &parse_errors {
            let start: usize = e.range.start().into();
            let end: usize = e.range.end().into();
            let snippet = &code[start..end.min(code.len())];
            eprintln!("Parse error at {:?}: '{}'", e.range, snippet);
        }
        assert_eq!(parse_errors.len(), 0, "Async procedure with Await should parse without errors");
    }

    #[test]
    fn test_no_parse_error_for_bom_with_region() {
        // UTF-8 BOM + CRLF + #Область (common in 1C exports)
        let code =
            "\u{FEFF}\r\n#Область Test\r\nПроцедура Тест()\r\nКонецПроцедуры\r\n#КонецОбласти";
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(parse_errors.is_empty(), "BOM with region should not trigger parse error");
    }

    #[test]
    fn test_no_parse_error_for_spaced_preprocessor_directives() {
        let code = r#"
Процедура Тест()
    # Если ВебКлиент Тогда
        Возврат;
    # Иначе
        Сообщить("Не веб");
    # КонецЕсли
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(
            parse_errors.is_empty(),
            "Spaced preprocessor directives should not trigger parse error"
        );
    }

    #[test]
    fn test_no_parse_error_for_iso_date_literal() {
        let code = r#"
Функция МинимальнаяДата()
    Возврат '0001-01-01';
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(parse_errors.is_empty(), "ISO date literal should not trigger parse error");
    }

    #[test]
    fn test_no_parse_error_for_trailing_dot_numeric_literal() {
        let code = r#"
Процедура Тест(Значение)
    Если Значение < 0. Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(
            parse_errors.is_empty(),
            "Numeric literal with trailing dot should not trigger parse error"
        );
    }

    fn assert_no_parse_errors_for(code: &str, message: &str) {
        let diagnostics = check_ast_diagnostic(code, super::check);
        let parse_errors: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ParseError).collect();
        assert!(parse_errors.is_empty(), "{message}");
    }

    #[test]
    fn test_no_parse_error_for_dotted_datetime_literals() {
        let code = r#"
Процедура Тест()
    Начало = '1000.01.01 00:00.00';
    Конец = '2099.12.31 23:59.59';
КонецПроцедуры
"#;
        assert_no_parse_errors_for(
            code,
            "Dotted date/time literals should not trigger parse error",
        );
    }

    #[test]
    fn test_no_parse_error_for_comma_date_literal_argument() {
        let code = r#"
Процедура Тест()
    Минимальная = Дата('0001,01,01');
КонецПроцедуры
"#;
        assert_no_parse_errors_for(
            code,
            "Comma-separated date literal should not trigger parse error",
        );
    }

    #[test]
    fn test_no_parse_error_for_non_breaking_space_before_statement() {
        let code = "Процедура Тест()\n\
    \u{00A0}Данные.Вставить(\"Ключ\" \"\");\n\
КонецПроцедуры";
        assert_no_parse_errors_for(
            code,
            "Non-breaking space before statement should not trigger parse error",
        );
    }

    #[test]
    fn test_no_parse_error_for_chained_less_than_comparison() {
        let code = r#"
Процедура Тест(Значение)
    Если 60 < Значение <= 3600 Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        assert_no_parse_errors_for(
            code,
            "Chained less-than comparison should not trigger parse error",
        );
    }

    #[test]
    fn test_no_parse_error_for_chained_not_equal_comparison() {
        let code = r#"
Процедура Тест(Блок)
    Значение1 = Блок[0] <> Блок[2] <> Блок[4];
КонецПроцедуры
"#;
        assert_no_parse_errors_for(
            code,
            "Chained not-equal comparison should not trigger parse error",
        );
    }

    #[test]
    fn test_no_parse_error_for_raise_without_semicolon_before_end_try() {
        let code = r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        ВызватьИсключение
    КонецПопытки;
КонецПроцедуры
"#;
        assert_no_parse_errors_for(
            code,
            "Raise without semicolon before КонецПопытки should not trigger parse error",
        );
    }

    #[test]
    fn test_no_parse_error_for_parenthesized_preprocessor_condition() {
        let code = r#"
Процедура Тест()
    #Если (Не ВебКлиент) И (Не МобильныйКлиент) Тогда
        ИмяАрхива = "stat.zip";
    #КонецЕсли
КонецПроцедуры
"#;
        assert_no_parse_errors_for(
            code,
            "Parenthesized preprocessor condition should not trigger parse error",
        );
    }

    #[test]
    fn test_no_parse_error_for_multiline_nstr_argument() {
        let code = r#"Процедура Тест()
    ТекстПодсказки = НСтр("ru = 'Доплата может производиться картой,
        "а также наличными.'");
КонецПроцедуры"#;
        assert_no_parse_errors_for(code, "Multiline NStr argument should not trigger parse error");
    }
}
