use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const WRITE_LOG_EVENT_METHOD_PARAMS_COUNT: usize = 5;

#[allow(clippy::too_many_arguments)]
pub fn from_hir(
    in_except_block: bool,
    arg_count: usize,
    log_level_empty: bool,
    comment_empty: bool,
    has_error_log_level: bool,
    has_detail_error_description: bool,
    except_has_raise: bool,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::UsageWriteLogEvent;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    if arg_count < WRITE_LOG_EVENT_METHOD_PARAMS_COUNT {
        return Some(Diagnostic {
            code,
            message: "Неверное число параметров метода".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    if log_level_empty {
        return Some(Diagnostic {
            code,
            message: "Не указан 2й параметр с типом \"УровеньЖурналаРегистрации\"".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    if comment_empty {
        return Some(Diagnostic {
            code,
            message: "Не указан 5й параметр \"Комментарий\"".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    if in_except_block {
        if !has_error_log_level {
            return Some(Diagnostic {
                code,
                message: "Нужно указывать уровень \"Ошибка\" при записи в журнал регистрации внутри блока Исключение-КонецПопытки".to_string(),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }

        if !has_detail_error_description && !except_has_raise {
            return Some(Diagnostic {
                code,
                message: "В тексте комментария нет вызова \"ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())\"".to_string(),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_wrong_number_params() {
        let code = r#"
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 3:5..3:40
              message: Неверное число параметров метода
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_two_params_wrong_count() {
        let code = r#"
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 3:5..3:74
              message: Неверное число параметров метода
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_four_params_wrong_count() {
        let code = r#"
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , );
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 3:5..3:78
              message: Неверное число параметров метода
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_no_second_parameter() {
        let code = r#"
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие",
      ,
      , , ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 3:5..5:62
              message: Не указан 2й параметр с типом "УровеньЖурналаРегистрации"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_no_comment() {
        let code = r#"
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , , );
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 3:5..3:80
              message: Не указан 5й параметр "Комментарий"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_wrong_log_level_in_except() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Предупреждение, , ,
            "Текст");
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 6:9..7:21
              message: Нужно указывать уровень "Ошибка" при записи в журнал регистрации внутри блока Исключение-КонецПопытки
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_missing_detail_error_in_except() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , ,
            ОписаниеОшибки());
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 6:9..7:30
              message: В тексте комментария нет вызова "ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_plain_string_comment_in_except() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , ,
            "Комментарий 1");
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 6:9..7:29
              message: В тексте комментария нет вызова "ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_concatenation_without_detail_error_in_except() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ТекстОшибки = "Описание" + Метод();
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , ,
            "Еще текст " + ТекстОшибки + Метод());
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 7:9..8:50
              message: В тексте комментария нет вызова "ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_unassigned_variable_in_except() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , ,
            "Еще текст " + НетПрисвоения);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 6:9..7:42
              message: В тексте комментария нет вызова "ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_variable_assigned_above_try_used_in_except() {
        let code = r#"
Процедура Тест()
    ТекстОшибки = "";
    Попытка
        А = 10;
    Исключение
        ЗаписьЖурналаРегистрации("Событие",
            УровеньЖурналаРегистрации.Ошибка,,,
            ТекстОшибки);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_correct_usage_outside_except() {
        let code = r#"
Процедура Тест()
    ЗаписьЖурналаРегистрации("Событие",
        УровеньЖурналаРегистрации.Ошибка, , ,
        ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_correct_usage_in_except_with_raise() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие",
            УровеньЖурналаРегистрации.Ошибка, , ,
            ОписаниеОшибки());
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_correct_usage_in_except_with_detail() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , ,
            ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_variable_with_detail_error() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ТекстОшибки = ПодробноеПредставлениеОшибки(ИнформацияОбОшибке());
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , ,
            ТекстОшибки);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_brief_error_used_directly_as_comment_in_except() {
        let code = r#"
Процедура Тест()
    Попытка
        СоздатьФайлНаДиске();
    Исключение
        ЗаписьЖурналаРегистрации("Событие",
            УровеньЖурналаРегистрации.Ошибка,,,
            КраткоеПредставлениеОшибки(ИнформацияОбОшибке()));
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 6:9..8:62
              message: В тексте комментария нет вызова "ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_variable_traced_to_brief_error_in_except() {
        let code = r#"
Процедура Тест()
    Попытка
        СоздатьФайлНаДиске();
    Исключение
        ТекстСообщения = КраткоеПредставлениеОшибки(ИнформацияОбОшибке());
        ЗаписьЖурналаРегистрации("Событие",
            УровеньЖурналаРегистрации.Ошибка,,,
            ТекстСообщения);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 7:9..9:28
              message: В тексте комментария нет вызова "ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_two_variables_wrong_one_used_in_except() {
        let code = r#"
Процедура Тест()
    Попытка
        СоздатьФайлНаДиске();
    Исключение
        ТекстСообщения = КраткоеПредставлениеОшибки(ИнформацияОбОшибке());
        ДругойТекстСообщения = ПодробноеПредставлениеОшибки(ИнформацияОбОшибке());
        ЗаписьЖурналаРегистрации("Событие",
            УровеньЖурналаРегистрации.Ошибка,,,
            ТекстСообщения);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 8:9..10:28
              message: В тексте комментария нет вызова "ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_brief_error_concatenated_with_description_in_except() {
        let code = r#"
Процедура Тест(Знач СсылкаНаДанные, Знач Блокировка)
    Попытка
        Блокировка.Заблокировать();
    Исключение
        КороткийТекстСообщения = КраткоеПредставлениеОшибки(ИнформацияОбОшибке()) + ОписаниеОшибки();
        ЗаписьЖурналаРегистрации(
            "Событие",
            УровеньЖурналаРегистрации.Ошибка,
            ,
            СсылкаНаДанные,
            КороткийТекстСообщения);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 7:9..12:36
              message: В тексте комментария нет вызова "ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_variable_param_used_as_comment_outside_except() {
        let code = r#"
Процедура Тест(Знач ПодробноеПредставлениеОшибки)
    ЗаписьЖурналаРегистрации("Событие",
        УровеньЖурналаРегистрации.Ошибка,,,
        ПодробноеПредставлениеОшибки);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_normal_write_log_with_variable_comment_outside_except() {
        let code = r#"
Процедура Тест(Знач ИмяСобытия, Знач СсылкаНаДанные)
    ТекстЗаписи = ТекстОтвета();
    ЗаписьЖурналаРегистрации(
        ИмяСобытия,
        УровеньЖурналаРегистрации.Ошибка,
        ,
        СсылкаНаДанные,
        ТекстЗаписи);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_variable_traced_via_string_function_in_except() {
        let code = r#"
Процедура Тест(Знач ИмяСобытия, Знач СсылкаНаДанные)
    Попытка
        Блокировка.Заблокировать();
    Исключение
        ТекстСообщения = СтроковыеФункцииКлиентСервер.ПодставитьПараметрыВСтроку(
            "Не удалось: %1",
            ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
        ЗаписьЖурналаРегистрации(
            ИмяСобытия,
            УровеньЖурналаРегистрации.Ошибка,
            ,
            СсылкаНаДанные,
            ТекстСообщения);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_variable_traced_via_concatenation_with_detail_error_in_except() {
        let code = r#"
Процедура Тест(Знач ИмяСобытия, Знач СсылкаНаДанные, Знач Выборка)
    Попытка
        Блокировка.Заблокировать();
    Исключение
        ТекстСообщения =
            "Не удалось установить разделение" + " = "
                + Формат(Выборка.ОбластьДанных, "ЧГ=0")
                + Символы.ПС + ПодробноеПредставлениеОшибки(ИнформацияОбОшибке());
        ЗаписьЖурналаРегистрации(
            ИмяСобытия,
            УровеньЖурналаРегистрации.Ошибка,
            ,
            СсылкаНаДанные,
            ТекстСообщения);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_dynamic_log_level_variable_in_except() {
        let code = r#"
Процедура Тест(Знач ИмяСобытия, Знач СсылкаНаДанные, Знач Выборка)
    Попытка
        Блокировка.Заблокировать();
    Исключение
        ТекстСообщения =
            "Не удалось" + Символы.ПС + ПодробноеПредставлениеОшибки(ИнформацияОбОшибке());
        ЗаписьЖурналаРегистрации(
            ИмяСобытия,
            УровеньОшибки(),
            ,
            СсылкаНаДанные,
            ТекстСообщения);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    WriteLogEvent("Event");
EndProcedure
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 3:5..3:27
              message: Неверное число параметров метода
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    ЗАПИСЬЖУРНАЛАРЕГИСТРАЦИИ("Событие");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 3:5..3:40
              message: Неверное число параметров метода
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_error_processing_module() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖР,
            , , ОбработкаОшибок.ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_error_processing_module_brief_used_directly() {
        let code = r#"
Процедура Тест()
    Попытка
        СоздатьФайлНаДиске();
    Исключение
        ЗаписьЖурналаРегистрации("Событие",
            УровеньЖурналаРегистрации.Ошибка,,,
            ОбработкаОшибок.КраткоеПредставлениеОшибки(ИнформацияОбОшибке()));
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 6:9..8:78
              message: В тексте комментария нет вызова "ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_error_processing_module_variable_traced_to_brief() {
        let code = r#"
Процедура Тест()
    Попытка
        СоздатьФайлНаДиске();
    Исключение
        ТекстСообщения = ОбработкаОшибок.КраткоеПредставлениеОшибки(ИнформацияОбОшибке());
        ЗаписьЖурналаРегистрации("Событие",
            УровеньЖурналаРегистрации.Ошибка,,,
            ТекстСообщения);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 7:9..9:28
              message: В тексте комментария нет вызова "ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_error_processing_module_two_variables_wrong_one_used() {
        let code = r#"
Процедура Тест()
    Попытка
        СоздатьФайлНаДиске();
    Исключение
        ТекстСообщения = ОбработкаОшибок.КраткоеПредставлениеОшибки(ИнформацияОбОшибке());
        ДругойТекстСообщения = ОбработкаОшибок.ПодробноеПредставлениеОшибки(ИнформацияОбОшибке());
        ЗаписьЖурналаРегистрации("Событие",
            УровеньЖурналаРегистрации.Ошибка,,,
            ТекстСообщения);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 8:9..10:28
              message: В тексте комментария нет вызова "ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_error_processing_module_brief_concatenated_with_description() {
        let code = r#"
Процедура Тест(Знач СсылкаНаДанные, Знач Блокировка)
    Попытка
        Блокировка.Заблокировать();
    Исключение
        КороткийТекстСообщения = ОбработкаОшибок.КраткоеПредставлениеОшибки(ИнформацияОбОшибке()) + ОписаниеОшибки();
        ЗаписьЖурналаРегистрации(
            "Событие",
            УровеньЖурналаРегистрации.Ошибка,
            ,
            СсылкаНаДанные,
            КороткийТекстСообщения);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsageWriteLogEvent,
            expect![[r#"
            UsageWriteLogEvent @ 7:9..12:36
              message: В тексте комментария нет вызова "ПодробноеПредставлениеОшибки(ИнформацияОбОшибке())"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_error_processing_module_variable_traced_to_detail() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ТекстОшибки = ОбработкаОшибок.ПодробноеПредставлениеОшибки(ИнформацияОбОшибке());
        ЗаписьЖурналаРегистрации("Событие", УровеньЖурналаРегистрации.Ошибка, , ,
            ТекстОшибки);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_error_processing_module_variable_log_level_in_except() {
        let code = r#"
Процедура Тест()
    Попытка
        Метод();
    Исключение
        ЗаписьЖурналаРегистрации("Событие", УровеньЖР,
            , , ОбработкаОшибок.ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_error_processing_module_detail_in_string_concat_outside_except() {
        let code = r#"
Процедура Тест(Знач ИмяСобытия)
    ЗаписьЖурналаРегистрации(ИмяСобытия,
        УровеньЖурналаРегистрации.Ошибка, , ,
        "Ошибка: " + ОбработкаОшибок.ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_error_processing_module_string_function_in_except() {
        let code = r#"
Процедура Тест(Знач ИмяСобытия, Знач СсылкаНаДанные)
    Попытка
        Блокировка.Заблокировать();
    Исключение
        ТекстСообщения = СтроковыеФункцииКлиентСервер.ПодставитьПараметрыВСтроку(
            "Не удалось: %1",
            ОбработкаОшибок.ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));
        ЗаписьЖурналаРегистрации(
            ИмяСобытия,
            УровеньЖурналаРегистрации.Ошибка,
            ,
            СсылкаНаДанные,
            ТекстСообщения);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }

    #[test]
    fn test_error_processing_module_concatenation_with_detail_in_except() {
        let code = r#"
Процедура Тест(Знач ИмяСобытия, Знач СсылкаНаДанные, Знач Выборка)
    Попытка
        Блокировка.Заблокировать();
    Исключение
        ТекстСообщения =
            "Не удалось" + " = " + Формат(Выборка.ОбластьДанных, "ЧГ=0")
                + Символы.ПС + ОбработкаОшибок.ПодробноеПредставлениеОшибки(ИнформацияОбОшибке());
        ЗаписьЖурналаРегистрации(
            ИмяСобытия,
            УровеньЖурналаРегистрации.Ошибка,
            ,
            СсылкаНаДанные,
            ТекстСообщения);
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsageWriteLogEvent, expect![[r#""#]]);
    }
}
