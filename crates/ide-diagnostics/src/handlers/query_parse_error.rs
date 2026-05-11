//! Reports SDBL query texts that contain parse errors.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use syntax::SyntaxKind;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Sql, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

/// Runs the QueryParseError diagnostic.
///
/// Checks SDBL queries for parse errors using `all_sdbl_in_file()`.
/// Detects errors by:
/// 1. Looking for ERROR nodes in the SDBL AST (parser is error-tolerant)
/// 2. Checking for trailing dots in REFS expressions (e.g., `ССЫЛКА Документ.`)
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::QueryParseError;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let sdbl_queries = ctx.all_sdbl_in_file();
    let mut diagnostics = Vec::new();

    for (_query_expr_id, query_info) in sdbl_queries.iter() {
        let has_parse_error = query_info
            .query_ast
            .as_ref()
            .map(|ast| {
                let root = ast.syntax_node();

                // Check 1: ERROR nodes in AST
                let has_error_nodes = root.descendants().any(|n| n.kind() == SyntaxKind::ERROR);

                // Check 2: Trailing dot in REFS expression (e.g., ССЫЛКА Документ.)
                // This is a common error when dynamically constructing queries
                let has_trailing_dot = root.descendants().any(|n| {
                    if n.kind() == SyntaxKind::SDBL_REFS_EXPR {
                        has_trailing_dot_in_refs(&n)
                    } else {
                        false
                    }
                });

                has_error_nodes || has_trailing_dot
            })
            .unwrap_or(true); // No AST means parse failed completely

        if has_parse_error {
            diagnostics.push(Diagnostic {
                code,
                message: "Текст запроса содержит ошибки".to_string(),
                severity: ctx.severity(code),
                range: query_info.bsl_literal_range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    diagnostics
}

/// Check if SDBL_REFS_EXPR has a trailing dot without type name.
///
/// Valid: `ССЫЛКА Документ.ПриходныйОрдер` - has IDENT after DOT
/// Invalid: `ССЫЛКА Документ.` - DOT is last significant child
fn has_trailing_dot_in_refs(node: &syntax::SyntaxNode) -> bool {
    let children: Vec<_> = node.children_with_tokens().collect();

    // Find last DOT position
    let last_dot_pos = children
        .iter()
        .rposition(|child| child.as_token().map(|t| t.kind() == SyntaxKind::DOT).unwrap_or(false));

    let Some(dot_pos) = last_dot_pos else {
        return false;
    };

    // Check if there's an IDENT after the DOT (ignoring whitespace)
    let has_ident_after_dot = children[dot_pos + 1..]
        .iter()
        .any(|child| child.as_token().map(|t| t.kind() == SyntaxKind::IDENT).unwrap_or(false));

    !has_ident_after_dot
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_detects_parse_errors_in_query_texts() {
        let code = r#"ТекстЗапроса =
"ВЫБРАТЬ Максимум(ССЫЛКА = &Параметр) КАК УсловиеВАгрегатнойФункции
|ИЗ Справочник.Контрагенты";

ТекстЗапросаОшибка1 =
"ВЫБРАТЬ
|ИЗ Справочник.Контрагенты как";

ТекстЗапросаОшибка2 =
"ВЫБРАТЬ Поле
|ИЗ Справочник.Контрагенты КАК Контрагенты ЛЕВОЕ СОЕДИНЕНИЕ "
+ "Документ.Накладная ПО Контрагенты.Ссылка = Покупатель";

ТекстЗапросаОшибка3 =
"ВЫБРАТЬ Поле
|ИЗ Справочник.Контрагенты КАК Контрагенты
|ЛЕВОЕ СОЕДИНЕНИЕ
|Документ.Накладная ПО Контрагенты.Ссылка = Покупатель
|ГДЕ
| Условие >";

ТекстЗапросаОшибка4 =
"ВЫБРАТЬ Поле
|ИЗ Справочник.Контрагенты КАК Контрагенты
|ЛЕВОЕ СОЕДИНЕНИЕ
|Документ.Накладная ПО Контрагенты.Ссылка = Покупатель
|;
|ВЫБРАТЬ Поле
|ИЗ
|";

ТекстНеЗапроса5 =
"ВЫБРАТЬ значение?";

ТекстНеЗапроса6 =
"Вам нужно выбрать значение из списка";

ТекстЗапроса =
"ВЫБРАТЬ ПОЛЕ
|ИЗ РегистрНакопления.Регистр2.Остатки(Дата1, Дата2, (Измерение1, Измерение2) В
| (Выбрать Поле1, Поле2 Из Справочник.Справочник1))";"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::QueryParseError,
            expect![[r#"
                QueryParseError @ 10:1..11:62
                  message: Текст запроса содержит ошибки
                  severity: Warning
                QueryParseError @ 15:1..20:13
                  message: Текст запроса содержит ошибки
                  severity: Warning
                QueryParseError @ 23:1..30:3
                  message: Текст запроса содержит ошибки
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_valid_query_no_diagnostic() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ Справочник.Контрагенты";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::QueryParseError, expect![[r#""#]]);
    }

    #[test]
    fn test_incomplete_where() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ Таблица ГДЕ Условие >";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::QueryParseError,
            expect![[r#"
                QueryParseError @ 3:14..3:53
                  message: Текст запроса содержит ошибки
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_incomplete_from() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ  ";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::QueryParseError,
            expect![[r#"
                QueryParseError @ 3:14..3:33
                  message: Текст запроса содержит ошибки
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_incomplete_select_with_from() {
        // Query must have SELECT + keyword (FROM/WHERE/etc) to be detected as SDBL
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ   ";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::QueryParseError,
            expect![[r#"
                QueryParseError @ 3:14..3:34
                  message: Текст запроса содержит ошибки
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_multiline_incomplete_where() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле
             |ИЗ Таблица
             |ГДЕ Условие >";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::QueryParseError,
            expect![[r#"
                QueryParseError @ 3:14..5:29
                  message: Текст запроса содержит ошибки
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_batch_with_partial_error() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ Таблица1;
             |ВЫБРАТЬ Поле2 ИЗ";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::QueryParseError,
            expect![[r#"
                QueryParseError @ 3:14..4:32
                  message: Текст запроса содержит ошибки
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_select_constants_without_from() {
        // Valid SDBL: SELECT without FROM clause (returns constants)
        let code = r#"
Процедура Тест()
    ТекстЗапроса = "Выбрать 1 КАК ЧисловаяКонстанта, 2, ""Строка""";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::QueryParseError, expect![[r#""#]]);
    }

    #[test]
    fn test_parameter_as_data_source_no_diagnostic() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |    ТЗ.ИмяКолонки КАК Поле,
             |    ТЗ.Серия КАК Серия
             |ПОМЕСТИТЬ ВТ
             |ИЗ
             |    &ТЗ КАК ТЗ
             |;
             |
             |////////////////////////////////////////////////////////////////////////////////
             |ВЫБРАТЬ
             |    Остатки.Номенклатура КАК Номенклатура,
             |    Остатки.Количество КАК Количество
             |ПОМЕСТИТЬ ОстаткиWMS
             |ИЗ
             |    &ВМС_Остатки КАК ВМС_Остатки";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::QueryParseError, expect![[r#""#]]);
    }

    #[test]
    fn test_false_positive_complex_query_with_comments() {
        let code = include_str!("fixtures/query_parse_error_complex_valid.bsl");
        check_diagnostics_snapshot_for(code, DiagnosticCode::QueryParseError, expect![[r#""#]]);
    }

    #[test]
    fn test_complex_valid_query() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |    Товары.Номенклатура КАК Номенклатура,
             |    ЕСТЬNULL(ПланПродаж.Сумма, 0) КАК СуммаПлан
             |ИЗ
             |    Товары КАК Товары
             |        ЛЕВОЕ СОЕДИНЕНИЕ ПланПродаж
             |        ПО Товары.ID = ПланПродаж.ID";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::QueryParseError, expect![[r#""#]]);
    }

    #[test]
    fn test_trailing_dot_triggers_diagnostic() {
        // Query with trailing dot in REFS - should trigger diagnostic
        let code = r#"
Процедура Тест()
    Запрос.Текст = "ВЫБРАТЬ Поле ИЗ Т ГДЕ Поле ССЫЛКА Документ.";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::QueryParseError,
            expect![[r#"
                QueryParseError @ 3:20..3:65
                  message: Текст запроса содержит ошибки
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_valid_refs_no_diagnostic() {
        // Query with complete REFS - should NOT trigger diagnostic
        let code = r#"
Процедура Тест()
    Запрос.Текст = "ВЫБРАТЬ Поле ИЗ Т ГДЕ Поле ССЫЛКА Документ.ПриходныйОрдер";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::QueryParseError, expect![[r#""#]]);
    }

    #[test]
    fn test_dynamic_query_with_trailing_dot() {
        // Dynamic query construction with trailing dot - should trigger diagnostic
        let code = r#"
Процедура Тест()
    Запрос.Текст = "ВЫБРАТЬ
                   |    Задания.Источник КАК Документ
                   |ИЗ
                   |    РегистрСведений.Задания КАК Задания
                   |ГДЕ
                   |    Задания.Источник ССЫЛКА Документ."+ИмяДокумента+"";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::QueryParseError,
            expect![[r#"
                QueryParseError @ 3:20..8:59
                  message: Текст запроса содержит ошибки
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_original_user_query_trailing_dot() {
        // Original user query from issue - should trigger diagnostic
        let code = r#"
Процедура Тест()
    Запрос.Текст = "ВЫБРАТЬ
                   |    ЗаданияДляПроцессаОбработкиВходногоКонтроля.Источник КАК Документ,
                   |    ЗаданияДляПроцессаОбработкиВходногоКонтроля.ИдентификаторЗадания КАК ИдентификаторЗадания,
                   |    ЗаданияДляПроцессаОбработкиВходногоКонтроля.Дата КАК Дата,
                   |    ЗаданияДляПроцессаОбработкиВходногоКонтроля.ДатаОбработки КАК ДатаОбработки,
                   |    ЗаданияДляПроцессаОбработкиВходногоКонтроля.Обработано КАК Обработано,
                   |    ЗаданияДляПроцессаОбработкиВходногоКонтроля.Ошибка КАК Ошибка
                   |ИЗ
                   |    РегистрСведений.ЗаданияДляПроцессаОбработкиВходногоКонтроля КАК ЗаданияДляПроцессаОбработкиВходногоКонтроля
                   |ГДЕ
                   |    Не ЗаданияДляПроцессаОбработкиВходногоКонтроля.Обработано
                   |    И ЗаданияДляПроцессаОбработкиВходногоКонтроля.Источник ССЫЛКА Документ."+ИмяДокумента+"";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::QueryParseError,
            expect![[r#"
                QueryParseError @ 3:20..14:97
                  message: Текст запроса содержит ошибки
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_in_with_multiple_values_no_diagnostic() {
        let code = r#"
Процедура Тест()
    Запрос.Текст = "ВЫБРАТЬ
                   |    Т.Поле КАК Поле
                   |ИЗ
                   |    Справочник.Таблица КАК Т
                   |ГДЕ
                   |    Т.Статус В (ЗНАЧЕНИЕ(Перечисление.Статусы.Новый), ЗНАЧЕНИЕ(Перечисление.Статусы.Ошибка))";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::QueryParseError, expect![[r#""#]]);
    }

    #[test]
    fn test_complex_query_with_in_values_no_diagnostic() {
        let code = r#"
Процедура Тест()
    Запрос.Текст = "ВЫБРАТЬ
                   |    ОчередьЗапросовERP.Идентификатор КАК Идентификатор,
                   |    ОчередьЗапросовERP.Публикация КАК Публикация,
                   |    ОчередьЗапросовERP.ОбъектЗапроса КАК ОбъектЗапроса,
                   |    ОчередьЗапросовERP.Параметры КАК Параметры,
                   |    ОчередьЗапросовERP.Статус КАК Статус,
                   |    ОчередьЗапросовERP.Таймштамп КАК Таймштамп,
                   |    ОчередьЗапросовERP.ТекстСообщенияОбОшибке КАК ТекстСообщенияОбОшибке,
                   |    ОчередьЗапросовERP.Попытка КАК Попытка,
                   |    ОчередьЗапросовERP.ДатаОтправки КАК ДатаОтправки
                   |ИЗ
                   |    РегистрСведений.ОчередьЗапросовERP КАК ОчередьЗапросовERP
                   |ГДЕ
                   |    ОчередьЗапросовERP.Статус В (ЗНАЧЕНИЕ(Перечисление.СтатусыОчередиЗапросов.Новый), ЗНАЧЕНИЕ(Перечисление.СтатусыОчередиЗапросов.Ошибка))
                   |    И (ОчередьЗапросовERP.Попытка <= &Попытка
                   |            ИЛИ &Попытка = 0)
                   |    И (ОчередьЗапросовERP.ОбъектЗапроса = &ОбъектЗапроса
                   |            ИЛИ &ВсеОбъекты)
                   |
                   |УПОРЯДОЧИТЬ ПО
                   |    Публикация,
                   |    Таймштамп";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::QueryParseError, expect![[r#""#]]);
    }

    #[test]
    fn test_count_distinct_no_diagnostic() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ КОЛИЧЕСТВО(РАЗЛИЧНЫЕ Т.Поле) КАК Кол ИЗ Таблица КАК Т";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::QueryParseError, expect![[r#""#]]);
    }

    #[test]
    fn test_user_complex_query_no_diagnostic() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ
    |    ТаблицаИзменений.РасходныйОрдерНаТовары КАК РасходныйОрдерНаТовары,
    |    ТаблицаИзменений.Склад КАК Склад,
    |    ТаблицаИзменений.Номенклатура КАК Номенклатура,
    |    ТаблицаИзменений.Назначение КАК Назначение,
    |    ТаблицаИзменений.Характеристика КАК Характеристика,
    |    ТаблицаИзменений.Серия КАК Серия,
    |    ТаблицаИзменений.Распоряжение КАК Распоряжение,
    |    СУММА(ТаблицаИзменений.КОформлению) КАК КОформлениюИзменение
    |ПОМЕСТИТЬ ДвиженияИзменение
    |ИЗ
    |    (ВЫБРАТЬ
    |        ДвиженияПередЗаписью.РасходныйОрдерНаТовары КАК РасходныйОрдерНаТовары,
    |        ДвиженияПередЗаписью.Склад КАК Склад,
    |        ДвиженияПередЗаписью.Номенклатура КАК Номенклатура,
    |        ДвиженияПередЗаписью.Характеристика КАК Характеристика,
    |        ДвиженияПередЗаписью.Назначение КАК Назначение,
    |        ДвиженияПередЗаписью.Серия КАК Серия,
    |        ДвиженияПередЗаписью.Распоряжение КАК Распоряжение,
    |        ДвиженияПередЗаписью.КОформлениюПередЗаписью КАК КОформлению
    |    ИЗ
    |        ДвиженияТоварыКОформлениюВнутреннихПотребленийПередЗаписью КАК ДвиженияПередЗаписью
    |
    |    ОБЪЕДИНИТЬ ВСЕ
    |
    |    ВЫБРАТЬ
    |        Таблица.РасходныйОрдерНаТовары,
    |        Таблица.РасходныйОрдерНаТовары.Склад,
    |        Таблица.Номенклатура,
    |        Таблица.Характеристика,
    |        Таблица.Назначение,
    |        Таблица.Серия,
    |        Таблица.Распоряжение,
    |        ВЫБОР
    |            КОГДА Таблица.ВидДвижения = ЗНАЧЕНИЕ(ВидДвиженияНакопления.Приход)
    |                ТОГДА -Таблица.КОформлению
    |            ИНАЧЕ Таблица.КОформлению
    |        КОНЕЦ
    |    ИЗ
    |        РегистрНакопления.ТоварыКОформлениюВнутреннихПотреблений КАК Таблица
    |    ГДЕ
    |        Таблица.Регистратор = &Регистратор) КАК ТаблицаИзменений
    |
    |СГРУППИРОВАТЬ ПО
    |    ТаблицаИзменений.РасходныйОрдерНаТовары,
    |    ТаблицаИзменений.Склад,
    |    ТаблицаИзменений.Номенклатура,
    |    ТаблицаИзменений.Назначение,
    |    ТаблицаИзменений.Характеристика,
    |    ТаблицаИзменений.Серия,
    |    ТаблицаИзменений.Распоряжение
    |
    |ИМЕЮЩИЕ
    |    СУММА(ТаблицаИзменений.КОформлению) <> 0
    |;
    |
    |////////////////////////////////////////////////////////////////////////////////
    |ВЫБРАТЬ
    |    ДвиженияИзменение.Распоряжение КАК ЗВП
    |ИЗ
    |    ДвиженияИзменение КАК ДвиженияИзменение
    |
    |СГРУППИРОВАТЬ ПО
    |    ДвиженияИзменение.Распоряжение
    |;
    |
    |////////////////////////////////////////////////////////////////////////////////
    |УНИЧТОЖИТЬ ДвиженияТоварыКОформлениюВнутреннихПотребленийПередЗаписью";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::QueryParseError, expect![[r#""#]]);
    }

    #[test]
    fn test_valid_subquery_with_bsl_comment_between_lines() {
        // Real-world case: BSL comment between multiline string continuation lines.
        // The query itself is valid — the comment should be ignored by SDBL extraction.
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ РАЗРЕШЕННЫЕ
    |    Набор.Распоряжение          КАК Распоряжение,
    |    Набор.Номенклатура          КАК Номенклатура,
    |    Набор.Характеристика        КАК Характеристика,
    |    Набор.КодСтроки             КАК КодСтроки,
    |    Набор.Серия                 КАК Серия,
    |    Набор.Склад                 КАК Склад,
    |    СУММА(Набор.Заказано)       КАК Заказано,
    |    СУММА(Набор.КОформлению)    КАК КОформлению,
    |    СУММА(Набор.КПередаче)      КАК КПередаче
    |ПОМЕСТИТЬ ИмяТаблицы
    |ИЗ(
    |    ВЫБРАТЬ
    |        Таблица.Распоряжение          КАК Распоряжение,
    |        Таблица.Номенклатура          КАК Номенклатура,
    |        Таблица.Характеристика        КАК Характеристика,
    |        Таблица.КодСтроки             КАК КодСтроки,
    |        Таблица.Серия                 КАК Серия,
    |        Таблица.Склад                 КАК Склад,
    |        Таблица.ЗаказаноОборот        КАК Заказано,
    |        Таблица.КОформлениюОборот     КАК КОформлению,
    |        Таблица.КПередачеОборот       КАК КПередаче
    |    ИЗ
    |        РегистрНакопления.РаспоряженияНаОтгрузку.Обороты(,,, &ОтборПоИзмерениям) КАК Таблица
    // Комментарий между строками продолжения
    |) КАК Набор
    |
    |СГРУППИРОВАТЬ ПО
    |    Набор.Распоряжение,
    |    Набор.Номенклатура,
    |    Набор.Характеристика,
    |    Набор.КодСтроки,
    |    Набор.Серия,
    |    Набор.Склад";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::QueryParseError, expect![[r#""#]]);
    }

    #[test]
    fn test_tabular_part_field_list_no_diagnostic() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос(
        "ВЫБРАТЬ
        |    БлокиНавигации.ВерсияДанных,
        |    БлокиНавигации.ВключатьРеестры,
        |    БлокиНавигации.Заголовок,
        |    БлокиНавигации.ИмяПредопределенныхДанных,
        |    БлокиНавигации.Команды.(
        |        ВспомогательнаяКоманда,
        |        НомерСтроки,
        |        ДополнительныйПоказатель,
        |        ОсновнаяКомандаПоказатель,
        |        СкрытьВЕще,
        |        Ссылка),
        |    БлокиНавигации.Наименование,
        |    БлокиНавигации.НаименованиеЯзык1,
        |    БлокиНавигации.ЗаголовокЯзык1,
        |    БлокиНавигации.ПодсказкаЯзык1,
        |    БлокиНавигации.Подсказка,
        |    БлокиНавигации.ПометкаУдаления,
        |    БлокиНавигации.Предопределенный,
        |    БлокиНавигации.Представление,
        |    БлокиНавигации.Ссылка,
        |    БлокиНавигации.Условие,
        |    ЕСТЬNULL(НастройкиБлоковНавигации.ЗначениеНастройки, НЕОПРЕДЕЛЕНО) КАК НастройкиБлокаНавигации
        |ИЗ
        |    Справочник.БлокиНавигации КАК БлокиНавигации
        |        ЛЕВОЕ СОЕДИНЕНИЕ РегистрСведений.НастройкиБлоковНавигации КАК НастройкиБлоковНавигации
        |        ПО БлокиНавигации.Ссылка = НастройкиБлоковНавигации.БлокНавигации
        |        И НастройкиБлоковНавигации.Пользователь = &ТекущийПользователь
        |ГДЕ
        |    БлокиНавигации.Ссылка В (&БлокиНавигации)
        |    И БлокиНавигации.ПометкаУдаления = ЛОЖЬ");
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::QueryParseError, expect![[r#""#]]);
    }
}
