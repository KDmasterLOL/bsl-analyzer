//! JoinWithVirtualTable diagnostic.
//!
//! Detects usage of virtual tables in JOIN operations in SDBL queries.
//!
//! ## Why?
//! Joins with virtual tables cause performance issues in 1C:Enterprise.
//! Virtual tables (СрезПоследних, Остатки, Обороты, etc.) are computed on-the-fly
//! and joining with them creates unpredictable performance characteristics.
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT T.Ref FROM Catalog.Items AS Items
//!          LEFT JOIN InformationRegister.Prices.SliceLast AS T
//!          ON Items.Ref = T.Item";
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Use temporary tables or separate queries instead:
//! Query = "SELECT Prices.* INTO TempPrices
//!          FROM InformationRegister.Prices.SliceLast AS Prices;
//!
//!          SELECT T.Ref FROM Catalog.Items AS Items
//!          LEFT JOIN TempPrices AS T ON Items.Ref = T.Item";
//! ```
//!
//! ## Implementation
//! Ported from:

use crate::define_metadata;
use crate::metadata::*;
use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use sdbl_hir;
use tracing::debug;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Standard, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    let code = DiagnosticCode::JoinWithVirtualTable;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let sdbl_hirs = ctx.sdbl_hir_in_file();
    let bsl_source = ctx.file_text();
    let sdbl_queries = ctx.all_sdbl_in_file();

    use crate::sdbl_utils::build_line_index_shared;
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    for ((_expr_id, sdbl_package), (_query_expr_id, query_info)) in
        sdbl_hirs.iter().zip(sdbl_queries.iter())
    {
        let mapper = SdblPositionMapper::from_query_info(query_info, &bsl_source, &line_starts);

        for hir_diag in sdbl_package.all_diagnostics() {
            if let sdbl_hir::SdblDiagnostic::JoinWithVirtualTable { range, .. } = hir_diag {
                let bsl_range = mapper.map_range(*range, &query_info.query_text);

                diagnostics.push(Diagnostic {
                    code,
                    message: "Не следует использовать соединения с виртуальными таблицами"
                        .to_string(),
                    severity: ctx.severity(code),
                    range: bsl_range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
            }
        }
    }

    debug!(
        time_ms = start.elapsed().as_millis(),
        diagnostics_found = diagnostics.len(),
        "JoinWithVirtualTable completed"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;
    use crate::{DiagnosticCode, Severity};

    #[test]
    fn test_join_with_virtual_table_single_line() {
        // Single-line query: LEFT JOIN with СрезПоследних
        let code = r#"Процедура Тест1()
    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Ссылка Из Справочник.Справочник1 СПр Левое соединение РегистрСведений.Курсы.СрезПоследних КАК Т По СПр.Поле1 = Т.Валюта";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 JoinWithVirtualTable diagnostic");
        assert_eq!(diagnostics[0].code, DiagnosticCode::JoinWithVirtualTable);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
    }

    #[test]
    fn test_join_with_virtual_table_multiline_left() {
        // Multiline query: LEFT JOIN with Остатки
        let code = r#"Процедура Тест2()
    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Измерение Из Справочник.Справочник1
    |СПр Левое соединение
    |РегистрНакопления.Склады.Остатки(Склад = &Параметр) КАК Т
    |По СПр.Поле1 = Т.Местонахождение";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 JoinWithVirtualTable diagnostic");
    }

    #[test]
    fn test_join_with_virtual_table_multiline_right() {
        // Multiline query: RIGHT JOIN with Остатки
        let code = r#"Процедура Тест3()
    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Регистратор Из Справочник.Справочник1
    |СПр Правое соединение
    |РегистрНакопления.Склады.Остатки(Склад = &Параметр) КАК Т
    |По СПр.Поле1 = Т.Местонахождение";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 JoinWithVirtualTable diagnostic");
    }

    #[test]
    fn test_join_with_two_virtual_tables() {
        // Query with two virtual tables in JOINs → 2 diagnostics
        let code = r#"Процедура Тест4()
    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Измерение
    | Из РегистрСведений.Курсы.СрезПоследних(&Период) как Курсы Левое соединение
    |РегистрНакопления.Склады.Остатки(Склад = &Параметр) КАК Т
    |По Курсы.Поле1 = Т.Измерение";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 2, "Expected 2 JoinWithVirtualTable diagnostics");
    }

    #[test]
    fn test_virtual_table_in_from_no_join_no_trigger() {
        // Virtual table used in FROM without any JOIN → no diagnostic
        let code = r#"Процедура Тест7()
    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Ссылка
    | Из РегистрНакопления.Склады.Остатки(Склад = &Параметр) как Р,
    |(Выбрать СС.Ссылка Из Справочник.Справочник2 КАК СС Где СС.Ссылка = &Параметр) КАК Т";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Virtual table in FROM without JOIN should not trigger");
    }

    #[test]
    fn test_simple_join_with_virtual_table() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ЛЕВОЕ СОЕДИНЕНИЕ РегистрСведений.Курсы.СрезПоследних КАК Т ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "JOIN with virtual table should trigger");
    }

    #[test]
    fn test_no_false_positive_regular_table() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Справочник.Товары ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Цены ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "JOIN with regular table should not trigger");
    }

    #[test]
    fn test_no_false_positive_virtual_table_without_join() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрНакопления.Склады.Остатки(Склад = &Параметр) КАК Р";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Virtual table in FROM without JOIN should not trigger");
    }

    #[test]
    fn test_virtual_table_in_from_with_join() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрСведений.Курсы.СрезПоследних(&Период) КАК К ЛЕВОЕ СОЕДИНЕНИЕ Т2 ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Virtual table in FROM with JOIN should trigger");
    }

    #[test]
    fn test_multiple_virtual_tables_in_joins() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ *
    |ИЗ Справочник.Товары
    |ЛЕВОЕ СОЕДИНЕНИЕ РегистрСведений.Курсы.СрезПоследних КАК К ПО ID
    |ЛЕВОЕ СОЕДИНЕНИЕ РегистрНакопления.Склады.Остатки КАК О ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 2, "Should detect both virtual tables in JOINs");
    }
}
