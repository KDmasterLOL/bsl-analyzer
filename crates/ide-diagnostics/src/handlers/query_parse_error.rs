//! QueryParseError diagnostic.
//!
//! Detects SDBL queries with parse errors.
//!
//! ## Why?
//! SDBL query text must be syntactically correct and should open in the query builder.
//! Parse errors indicate incomplete or malformed queries that will fail at runtime.
//!
//! ## Bad practice
//! ```bsl
//! Query.Text = "SELECT Field
//!              |FROM Table AS";  // Incomplete alias
//! ```
//!
//! ## Good practice
//! ```bsl
//! Query.Text = "SELECT Field
//!              |FROM Table AS T";  // Complete query
//! ```
//!
//! ## Implementation
//!
//! This diagnostic operates at AST level (not HIR) because:
//! - SDBL HIR is only built for syntactically correct queries
//! - Parse errors are already available in `SdblQueryInfo.query_ast`
//! - Method `SdblQueryInfo.is_valid()` returns `false` when parse errors exist

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use syntax::SyntaxKind;
use tracing::debug;
use crate::define_metadata;
use crate::metadata::*;

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
    use std::time::Instant;
    let start = Instant::now();

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

    debug!(
        time_ms = start.elapsed().as_millis(),
        diagnostics_found = diagnostics.len(),
        "QueryParseError completed"
    );

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
    use super::check;
    use crate::test_utils::{assert_diagnostic_range_multiline, check_sdbl_diagnostic};
    use crate::{DiagnosticCode, Severity};
    #[test]
    fn test_query_parse_error_from_fixture() {
        let code = include_str!("../../test_data/QueryParseErrorDiagnostic.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);

        // Java expects 3 diagnostics:
        // - Lines 10-11: incomplete JOIN (first part of concatenated string)
        // - Lines 15-20: incomplete WHERE (Условие >)
        // - Lines 28-29: incomplete FROM in batch (we detect whole batch 23-30)
        assert_eq!(diagnostics.len(), 3, "Expected 3 parse error diagnostics");

        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::QueryParseError);
            assert_eq!(diag.severity, Severity::Warning);
        }
    }

    #[test]
    fn test_valid_query_no_diagnostic() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ Справочник.Контрагенты";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Valid query should not trigger diagnostic");
    }

    #[test]
    fn test_incomplete_where() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ Таблица ГДЕ Условие >";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Incomplete WHERE should trigger diagnostic");
        assert_eq!(diagnostics[0].code, DiagnosticCode::QueryParseError);
    }

    #[test]
    fn test_incomplete_from() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ  ";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Incomplete FROM should trigger diagnostic");
    }

    #[test]
    fn test_incomplete_select_with_from() {
        // Query must have SELECT + keyword (FROM/WHERE/etc) to be detected as SDBL
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ   ";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Incomplete FROM should trigger diagnostic");
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Incomplete WHERE in multiline should trigger diagnostic");
        // Lines are 0-indexed: line 2 = "    Запрос = ..."
        assert_diagnostic_range_multiline(code, &diagnostics[0], 2, 13, 4, 28);
    }

    #[test]
    fn test_batch_with_partial_error() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ Таблица1;
             |ВЫБРАТЬ Поле2 ИЗ";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Batch with partial error should trigger one diagnostic");
    }

    #[test]
    fn test_select_constants_without_from() {
        // Valid SDBL: SELECT without FROM clause (returns constants)
        let code = r#"
Процедура Тест()
    ТекстЗапроса = "Выбрать 1 КАК ЧисловаяКонстанта, 2, ""Строка""";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "SELECT without FROM is valid SDBL, should not trigger diagnostic"
        );
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Parameter as FROM data source should not trigger diagnostic"
        );
    }

    #[test]
    fn test_false_positive_complex_query_with_comments() {
        let code = include_str!("../../test_data/QueryParseErrorFalsePositive.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);
        if !diagnostics.is_empty() {
            for diag in &diagnostics {
                let (sl, sc, el, ec) = crate::test_utils::range_to_line_col(code, diag.range);
                eprintln!("Diagnostic: {} at {}:{}..{}:{}", diag.message, sl, sc, el, ec);
            }
        }
        assert_eq!(
            diagnostics.len(),
            0,
            "Complex query with BSL comments and &Parameter should not trigger diagnostic"
        );
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Valid complex query should not trigger diagnostic");
    }

    #[test]
    fn test_trailing_dot_triggers_diagnostic() {
        // Query with trailing dot in REFS - should trigger diagnostic
        let code = r#"
Процедура Тест()
    Запрос.Текст = "ВЫБРАТЬ Поле ИЗ Т ГДЕ Поле ССЫЛКА Документ.";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Trailing dot in REFS should trigger diagnostic");
        assert_eq!(diagnostics[0].code, DiagnosticCode::QueryParseError);
    }

    #[test]
    fn test_valid_refs_no_diagnostic() {
        // Query with complete REFS - should NOT trigger diagnostic
        let code = r#"
Процедура Тест()
    Запрос.Текст = "ВЫБРАТЬ Поле ИЗ Т ГДЕ Поле ССЫЛКА Документ.ПриходныйОрдер";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Valid REFS should not trigger diagnostic");
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "Dynamic query with trailing dot should trigger diagnostic"
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "Original user query with 'ССЫЛКА Документ.' should trigger diagnostic"
        );
        assert_eq!(diagnostics[0].code, DiagnosticCode::QueryParseError);
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "IN with multiple VALUE() should not trigger diagnostic");
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Complex query with IN (VALUE(), VALUE()) should not trigger diagnostic"
        );
    }
}
