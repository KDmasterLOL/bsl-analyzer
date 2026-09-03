use crate::define_metadata;
use crate::metadata::*;
use crate::{BodyContext, Diagnostic, DiagnosticCode};
use hir::LocalRange;
use regex::Regex;
use std::sync::Arc;

use crate::utils::regex_cache::cached_regex;
use stdx::case::CaseExt;
use syntax::SyntaxKind;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const SIMPLE_TAGS: &[&str] = &["todo", "fixme", "!!", "mrg", "@", "отладка", "debug"];

fn check_default_tags(comment_text: &str) -> bool {
    let after_slashes = match comment_text.strip_prefix("//") {
        Some(s) => s,
        None => return false,
    };

    let trimmed = after_slashes.trim_start();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.fold_lower();

    for tag in SIMPLE_TAGS {
        if lower.starts_with(tag) {
            return true;
        }
    }

    if lower.starts_with("для") && lower.contains("отладки") {
        return true;
    }

    if trimmed.starts_with("{{КОНСТРУКТОР_")
        || trimmed.starts_with("}}КОНСТРУКТОР_")
        || trimmed.starts_with("{{MRG")
        || trimmed.starts_with("}}MRG")
    {
        return true;
    }

    let lower_trimmed = &lower;
    if lower_trimmed.starts_with("вставить")
        && lower_trimmed.contains("содержимое")
        && lower_trimmed.contains("обработчика")
    {
        return true;
    }

    if lower_trimmed.starts_with("paste")
        && lower_trimmed.contains("handler")
        && lower_trimmed.contains("content")
    {
        return true;
    }

    if lower_trimmed.starts_with("insert")
        && lower_trimmed.contains("handler")
        && (lower_trimmed.contains("code") || lower_trimmed.contains("content"))
    {
        return true;
    }

    false
}

fn build_custom_pattern(service_tags: &str) -> Arc<Regex> {
    let pattern = format!(r"(?im)//\s*({service_tags})");
    cached_regex(&pattern).unwrap_or_else(|| cached_regex(r"(?i)//\s*(todo)").unwrap())
}

pub fn check_body(ctx: &BodyContext, acc: &mut Vec<Diagnostic<LocalRange>>) {
    let code = DiagnosticCode::UsingServiceTag;
    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    let custom_pattern = ctx.config.get_string(code, "serviceTags").map(build_custom_pattern);

    let mut diagnostics = Vec::new();

    for token in ctx.tokens().filter(|token| token.kind() == SyntaxKind::COMMENT) {
        let text = token.text();

        let is_match = match &custom_pattern {
            Some(pattern) => pattern.is_match(text),
            None => check_default_tags(text),
        };

        if is_match {
            let display_tag = extract_display_tag(text);
            diagnostics.push(Diagnostic {
                code,
                message: format!("Обнаружен служебный тег \"{display_tag}\""),
                range: LocalRange::of_detached_node(token.text_range()),
                severity: ctx.severity(code),
                tags: ctx.tags(code),
                fixes: Vec::new(),
            });
        }
    }

    acc.extend(diagnostics);
}

fn extract_display_tag(comment_text: &str) -> &str {
    let after_slashes = comment_text.strip_prefix("//").unwrap_or(comment_text);
    let trimmed = after_slashes.trim_start();

    let end_pos = trimmed
        .char_indices()
        .take(30)
        .find(|(_, c)| c.is_whitespace() || *c == ':')
        .map(|(i, _)| i)
        .unwrap_or_else(|| trimmed.char_indices().nth(30).map(|(i, _)| i).unwrap_or(trimmed.len()));

    let tag = &trimmed[..end_pos];
    if tag.is_empty() {
        comment_text
    } else {
        tag
    }
}

#[cfg(test)]
mod tests {
    use super::check_body;
    use crate::test_utils::{check_body_diagnostic_with_config, check_diagnostics_snapshot_for};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;
    #[test]
    fn test_todo_tag() {
        let code = "// TODO: fix this";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingServiceTag,
            expect![[r#"
            UsingServiceTag @ 1:1..1:18
              message: Обнаружен служебный тег "TODO"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_fixme_tag() {
        let code = "// FIXME: broken";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingServiceTag,
            expect![[r#"
            UsingServiceTag @ 1:1..1:17
              message: Обнаружен служебный тег "FIXME"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_debug_tag_russian() {
        let code = "// отладка";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingServiceTag,
            expect![[r#"
            UsingServiceTag @ 1:1..1:11
              message: Обнаружен служебный тег "отладка"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_no_diagnostic_for_normal_comment() {
        let code = "// This is a normal comment";
        check_diagnostics_snapshot_for(code, DiagnosticCode::UsingServiceTag, expect![[r#""#]]);
    }

    #[test]
    fn test_case_insensitive() {
        let code = "// todo: something";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingServiceTag,
            expect![[r#"
            UsingServiceTag @ 1:1..1:19
              message: Обнаружен служебный тег "todo"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_comprehensive_fixture() {
        let code = r#"// Тестовый модуль для проверки служебных тегов в комментариях
// TODO: удалить после внедрения БСП
Функция СкопироватьСтруктуру(ВходящаяСтруктура)

    Если ТипЗнч(ВходящаяСтруктура) <> Тип("Структура") Тогда
        Возврат Новый Структура;
    КонецЕсли;

    НоваяСтруктура = Новый Структура;
    Для Каждого КлючЗначение Из ВходящаяСтруктура Цикл
        НоваяСтруктура.Вставить(КлючЗначение.Ключ, КлючЗначение.Значение);
    КонецЦикла;

    // FIXME: Добавиить новое значение в структуру
    // НоваяСтруктура.Вставить("Тест", "TODO");
    Возврат НоваяСтруктура;

КонецФункции

// Просто описание функции
Функция ВернутьЗначение()
    // TODO: просто проверка?
    Возврат 1;
КонецФункции

// !!!_nik: Такой-то текст
Процедура ПроверочнаяПроцедура() // !!_nik: Такой-то текст

    // @nik
    ЧтоТо = Истина; // отладка

    // debug
    Если ЧтоТо Тогда // отладка

        // дляотладки

    КонецЕсли;

КонецПроцедуры

Процедура ПроверочнаяФункция()

    //{{КОНСТРУКТОР_ЗАПРОСА_С_ОБРАБОТКОЙ_РЕЗУЛЬТАТА
    // Данный фрагмент построен конструктором.
    // При повторном использовании конструктора, внесенные вручную изменения будут утеряны!!!

    Запрос = Новый Запрос;
    Запрос.Текст =
        "ВЫБРАТЬ
        |	Автомобили.Ссылка КАК Ссылка
        |ИЗ
        |	Справочник.Автомобили КАК Автомобили";

    РезультатЗапроса = Запрос.Выполнить();

    ВыборкаДетальныеЗаписи = РезультатЗапроса.Выбрать();

    Пока ВыборкаДетальныеЗаписи.Следующий() Цикл
        // Вставить обработку выборки ВыборкаДетальныеЗаписи
    КонецЦикла;

    //}}КОНСТРУКТОР_ЗАПРОСА_С_ОБРАБОТКОЙ_РЕЗУЛЬТАТА

КонецПроцедуры

//{{MRG[  ]
//            ПредставлениеПодразделения = Подразделение.Наименование;
//}}MRG[  ]

// Просто описание функции
Функция ВернутьВтороеЗначение()
    // для отладки: просто проверка?
    Возврат 1;
КонецФункции

Процедура ОбработкаПроверкиЗаполнения(Отказ, ПроверяемыеРеквизиты)
    // С таким комментарием генерирует 1С обработчики
    // Вставить содержимое обработчика.
КонецПроцедуры

Procedure Posting(Cancel, PostingMode)
    // Вариант на английском для модуля объекта
    // Insert handler code.
EndProcedure

&AtServer
Procedure Command1AtServer()
    // Вариант на английском для модуля формы
    // Insert handler contents.
EndProcedure

&AtClient
Procedure Command1(Command)
	Command1AtServer();
EndProcedure

&НаКлиенте
Процедура ОбработкаКоманды(ПараметрКоманды, ПараметрыВыполненияКоманды)
    //Вставить содержимое обработчика.
	//ПараметрыФормы = Новый Структура("", );
	//ОткрытьФорму("Обработка.Обработка1.Форма", ПараметрыФормы, ПараметрыВыполненияКоманды.Источник, ПараметрыВыполненияКоманды.Уникальность, ПараметрыВыполненияКоманды.Окно, ПараметрыВыполненияКоманды.НавигационнаяСсылка);
КонецПроцедуры

&AtClient
Procedure CommandProcessing(CommandParameter, CommandExecuteParameters)
    //Paste handler content.
	//FormParameters = New Structure("", );
	//OpenForm("Document.Document1.ListForm", FormParameters, CommandExecuteParameters.Source, CommandExecuteParameters.Uniqueness, CommandExecuteParameters.Window, CommandExecuteParameters.URL);
EndProcedure

&AtClient
Procedure AfterWrite(WriteParameters)
    //Insert handler contents
EndProcedure"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingServiceTag,
            expect![[r#"
            UsingServiceTag @ 2:1..2:37
              message: Обнаружен служебный тег "TODO"
              severity: Hint
            UsingServiceTag @ 14:5..14:51
              message: Обнаружен служебный тег "FIXME"
              severity: Hint
            UsingServiceTag @ 22:5..22:30
              message: Обнаружен служебный тег "TODO"
              severity: Hint
            UsingServiceTag @ 26:1..26:27
              message: Обнаружен служебный тег "!!!_nik"
              severity: Hint
            UsingServiceTag @ 27:34..27:59
              message: Обнаружен служебный тег "!!_nik"
              severity: Hint
            UsingServiceTag @ 29:5..29:12
              message: Обнаружен служебный тег "@nik"
              severity: Hint
            UsingServiceTag @ 30:21..30:31
              message: Обнаружен служебный тег "отладка"
              severity: Hint
            UsingServiceTag @ 32:5..32:13
              message: Обнаружен служебный тег "debug"
              severity: Hint
            UsingServiceTag @ 33:22..33:32
              message: Обнаружен служебный тег "отладка"
              severity: Hint
            UsingServiceTag @ 35:9..35:22
              message: Обнаружен служебный тег "дляотладки"
              severity: Hint
            UsingServiceTag @ 43:5..43:52
              message: Обнаружен служебный тег "{{КОНСТРУКТОР_ЗАПРОСА_С_ОБРАБО"
              severity: Hint
            UsingServiceTag @ 62:5..62:52
              message: Обнаружен служебный тег "}}КОНСТРУКТОР_ЗАПРОСА_С_ОБРАБО"
              severity: Hint
            UsingServiceTag @ 66:1..66:12
              message: Обнаружен служебный тег "{{MRG["
              severity: Hint
            UsingServiceTag @ 68:1..68:12
              message: Обнаружен служебный тег "}}MRG["
              severity: Hint
            UsingServiceTag @ 72:5..72:37
              message: Обнаружен служебный тег "для"
              severity: Hint
            UsingServiceTag @ 78:5..78:40
              message: Обнаружен служебный тег "Вставить"
              severity: Hint
            UsingServiceTag @ 83:5..83:28
              message: Обнаружен служебный тег "Insert"
              severity: Hint
            UsingServiceTag @ 89:5..89:32
              message: Обнаружен служебный тег "Insert"
              severity: Hint
            UsingServiceTag @ 99:5..99:39
              message: Обнаружен служебный тег "Вставить"
              severity: Hint
            UsingServiceTag @ 106:5..106:29
              message: Обнаружен служебный тег "Paste"
              severity: Hint
            UsingServiceTag @ 113:5..113:30
              message: Обнаружен служебный тег "Insert"
              severity: Hint"#]],
        );
    }

    #[test]
    fn test_custom_service_tags() {
        let code = r#"// Тестовый модуль для проверки служебных тегов в комментариях
// TODO: удалить после внедрения БСП
Функция СкопироватьСтруктуру(ВходящаяСтруктура)

    Если ТипЗнч(ВходящаяСтруктура) <> Тип("Структура") Тогда
        Возврат Новый Структура;
    КонецЕсли;

    НоваяСтруктура = Новый Структура;
    Для Каждого КлючЗначение Из ВходящаяСтруктура Цикл
        НоваяСтруктура.Вставить(КлючЗначение.Ключ, КлючЗначение.Значение);
    КонецЦикла;

    // FIXME: Добавиить новое значение в структуру
    // НоваяСтруктура.Вставить("Тест", "TODO");
    Возврат НоваяСтруктура;

КонецФункции

// Просто описание функции
Функция ВернутьЗначение()
    // TODO: просто проверка?
    Возврат 1;
КонецФункции

// !!!_nik: Такой-то текст
Процедура ПроверочнаяПроцедура() // !!_nik: Такой-то текст

    // @nik
    ЧтоТо = Истина; // отладка

    // debug
    Если ЧтоТо Тогда // отладка

        // дляотладки

    КонецЕсли;

КонецПроцедуры

Процедура ПроверочнаяФункция()

    //{{КОНСТРУКТОР_ЗАПРОСА_С_ОБРАБОТКОЙ_РЕЗУЛЬТАТА
    // Данный фрагмент построен конструктором.
    // При повторном использовании конструктора, внесенные вручную изменения будут утеряны!!!

    Запрос = Новый Запрос;
    Запрос.Текст =
        "ВЫБРАТЬ
        |	Автомобили.Ссылка КАК Ссылка
        |ИЗ
        |	Справочник.Автомобили КАК Автомобили";

    РезультатЗапроса = Запрос.Выполнить();

    ВыборкаДетальныеЗаписи = РезультатЗапроса.Выбрать();

    Пока ВыборкаДетальныеЗаписи.Следующий() Цикл
        // Вставить обработку выборки ВыборкаДетальныеЗаписи
    КонецЦикла;

    //}}КОНСТРУКТОР_ЗАПРОСА_С_ОБРАБОТКОЙ_РЕЗУЛЬТАТА

КонецПроцедуры

//{{MRG[  ]
//            ПредставлениеПодразделения = Подразделение.Наименование;
//}}MRG[  ]

// Просто описание функции
Функция ВернутьВтороеЗначение()
    // для отладки: просто проверка?
    Возврат 1;
КонецФункции

Процедура ОбработкаПроверкиЗаполнения(Отказ, ПроверяемыеРеквизиты)
    // С таким комментарием генерирует 1С обработчики
    // Вставить содержимое обработчика.
КонецПроцедуры

Procedure Posting(Cancel, PostingMode)
    // Вариант на английском для модуля объекта
    // Insert handler code.
EndProcedure

&AtServer
Procedure Command1AtServer()
    // Вариант на английском для модуля формы
    // Insert handler contents.
EndProcedure

&AtClient
Procedure Command1(Command)
	Command1AtServer();
EndProcedure

&НаКлиенте
Процедура ОбработкаКоманды(ПараметрКоманды, ПараметрыВыполненияКоманды)
    //Вставить содержимое обработчика.
	//ПараметрыФормы = Новый Структура("", );
	//ОткрытьФорму("Обработка.Обработка1.Форма", ПараметрыФормы, ПараметрыВыполненияКоманды.Источник, ПараметрыВыполненияКоманды.Уникальность, ПараметрыВыполненияКоманды.Окно, ПараметрыВыполненияКоманды.НавигационнаяСсылка);
КонецПроцедуры

&AtClient
Procedure CommandProcessing(CommandParameter, CommandExecuteParameters)
    //Paste handler content.
	//FormParameters = New Structure("", );
	//OpenForm("Document.Document1.ListForm", FormParameters, CommandExecuteParameters.Source, CommandExecuteParameters.Uniqueness, CommandExecuteParameters.Window, CommandExecuteParameters.URL);
EndProcedure

&AtClient
Procedure AfterWrite(WriteParameters)
    //Insert handler contents
EndProcedure"#;
        let mut config = DiagnosticsConfig::default();
        config
            .parameters
            .insert(DiagnosticCode::UsingServiceTag, serde_json::json!({"serviceTags": "todo"}));

        let diagnostics = check_body_diagnostic_with_config(code, config, check_body);
        assert_eq!(diagnostics.len(), 2, "With only 'todo' tag, should have 2 diagnostics");
        crate::test_utils::assert_diagnostic_range(code, &diagnostics[0], 1, 0, 36);
        crate::test_utils::assert_diagnostic_range(code, &diagnostics[1], 21, 4, 29);
    }
}
