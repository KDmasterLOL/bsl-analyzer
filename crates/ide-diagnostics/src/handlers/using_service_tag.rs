//! Reports service tags and placeholder comments left in code.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use regex::Regex;
use syntax::{NodeOrToken, SyntaxKind, SyntaxNode};

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

    let lower = trimmed.to_lowercase();

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

fn build_custom_pattern(service_tags: &str) -> Regex {
    let pattern = format!(r"(?im)//\s*({service_tags})");
    Regex::new(&pattern).unwrap_or_else(|_| Regex::new(r"(?i)//\s*(todo)").unwrap())
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::UsingServiceTag;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let custom_pattern = ctx.config.get_string(code, "serviceTags").map(build_custom_pattern);

    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    for token in collect_comment_tokens(&root) {
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
                range: token.text_range(),
                severity: ctx.severity(code),
                tags: ctx.tags(code),
                fixes: Vec::new(),
            });
        }
    }

    diagnostics
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

fn collect_comment_tokens(root: &SyntaxNode) -> Vec<syntax::SyntaxToken> {
    let mut tokens = Vec::new();
    for element in root.descendants_with_tokens() {
        if let NodeOrToken::Token(token) = element {
            if token.kind() == SyntaxKind::COMMENT {
                tokens.push(token);
            }
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_todo_tag() {
        let code = "// TODO: fix this";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("TODO"));
    }

    #[test]
    fn test_fixme_tag() {
        let code = "// FIXME: broken";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("FIXME"));
    }

    #[test]
    fn test_debug_tag_russian() {
        let code = "// отладка";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_diagnostic_for_normal_comment() {
        let code = "// This is a normal comment";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_case_insensitive() {
        let code = "// todo: something";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
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
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 21, "Should find 21 diagnostics");

        assert_diagnostic_range(code, &diagnostics[0], 1, 0, 36);
        assert_diagnostic_range(code, &diagnostics[1], 13, 4, 50);
        assert_diagnostic_range(code, &diagnostics[2], 21, 4, 29);
        assert_diagnostic_range(code, &diagnostics[3], 25, 0, 26);
        assert_diagnostic_range(code, &diagnostics[4], 26, 33, 58);
        assert_diagnostic_range(code, &diagnostics[5], 28, 4, 11);
        assert_diagnostic_range(code, &diagnostics[6], 29, 20, 30);
        assert_diagnostic_range(code, &diagnostics[7], 31, 4, 12);
        assert_diagnostic_range(code, &diagnostics[8], 32, 21, 31);
        assert_diagnostic_range(code, &diagnostics[9], 34, 8, 21);
        assert_diagnostic_range(code, &diagnostics[10], 42, 4, 51);
        assert_diagnostic_range(code, &diagnostics[11], 61, 4, 51);
        assert_diagnostic_range(code, &diagnostics[12], 65, 0, 11);
        assert_diagnostic_range(code, &diagnostics[13], 67, 0, 11);
        assert_diagnostic_range(code, &diagnostics[14], 71, 4, 36);
        assert_diagnostic_range(code, &diagnostics[15], 77, 4, 39);
        assert_diagnostic_range(code, &diagnostics[16], 82, 4, 27);
        assert_diagnostic_range(code, &diagnostics[17], 88, 4, 31);
        assert_diagnostic_range(code, &diagnostics[18], 98, 4, 38);
        assert_diagnostic_range(code, &diagnostics[19], 105, 4, 28);
        assert_diagnostic_range(code, &diagnostics[20], 112, 4, 29);
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

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 2, "With only 'todo' tag, should have 2 diagnostics");
        assert_diagnostic_range(code, &diagnostics[0], 1, 0, 36);
        assert_diagnostic_range(code, &diagnostics[1], 21, 4, 29);
    }
}
