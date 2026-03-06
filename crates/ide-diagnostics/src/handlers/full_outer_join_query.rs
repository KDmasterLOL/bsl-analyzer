//! FullOuterJoinQuery diagnostic.
//!
//! Detects usage of FULL OUTER JOIN in SDBL queries.
//!
//! ## Why?
//! FULL OUTER JOIN operations have severe performance implications in 1C:Enterprise.
//! The query optimizer struggles with full outer joins, leading to slow execution
//! and high memory consumption.
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT T1.Field1, T2.Field2
//!          FROM Table1 AS T1
//!          FULL OUTER JOIN Table2 AS T2
//!          ON T1.ID = T2.ID";
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Use UNION of LEFT JOINs instead:
//! Query = "SELECT T1.Field1, T2.Field2
//!          FROM Table1 AS T1
//!          LEFT JOIN Table2 AS T2 ON T1.ID = T2.ID
//!          UNION ALL
//!          SELECT NULL AS Field1, T2.Field2
//!          FROM Table2 AS T2
//!          LEFT JOIN Table1 AS T1 ON T2.ID = T1.ID
//!          WHERE T1.ID IS NULL";
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

/// Runs the FullOuterJoinQuery diagnostic.
///
/// Uses SDBL HIR with diagnostics collected during lowering.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    let code = DiagnosticCode::FullOuterJoinQuery;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // Get SDBL HIR with collected diagnostics
    let sdbl_hirs = ctx.sdbl_hir_in_file();

    let bsl_source = ctx.file_text();

    // Get cached SDBL queries for position mapping
    let sdbl_queries = ctx.all_sdbl_in_file();

    // Build shared line index
    use crate::sdbl_utils::build_line_index_shared;
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    // Iterate SDBL HIRs and corresponding query infos in parallel
    // Both are sorted by position in file, so we can zip them
    for ((_expr_id, sdbl_package), (_query_expr_id, query_info)) in
        sdbl_hirs.iter().zip(sdbl_queries.iter())
    {
        let mapper = SdblPositionMapper::from_query_info(query_info, &bsl_source, &line_starts);

        // Emit diagnostics from HIR
        for hir_diag in sdbl_package.all_diagnostics() {
            if let sdbl_hir::SdblDiagnostic::FullOuterJoin { range } = hir_diag {
                let bsl_range = mapper.map_range(*range, &query_info.query_text);

                diagnostics.push(Diagnostic {
                    code,
                    message: "Using FULL OUTER JOIN significantly reduces query performance. \
                              Consider rewriting using UNION with LEFT JOIN"
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
        "FullOuterJoinQuery completed"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;
    use crate::DiagnosticCode;
    #[test]
    fn test_fixture_full_outer_join_detected_left_join_not() {
        // Fixture Тест1: has ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ -> 1 diagnostic
        // Fixture Тест2: has only LEFT JOINs -> 0 diagnostics
        let code_test1 = r#"Процедура Тест1()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
                   |    Товары.Номенклатура КАК Номенклатура,
                   |    ЕСТЬNULL(ПланПродаж.Сумма, 0) КАК СуммаПлан,
                   |    ЕСТЬNULL(ФактическиеПродажи.Сумма, 0) КАК СуммаФакт
                   |ИЗ
                   |    Товары КАК Товары
                   |        ЛЕВОЕ СОЕДИНЕНИЕ ПланПродаж КАК ПланПродаж
                   |            ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ ФактическиеПродажи КАК ФактическиеПродажи
                   |            ПО ПланПродаж.Номенклатура = ФактическиеПродажи.Номенклатура
                   |        ПО Товары.Номенклатура = ПланПродаж.Номенклатура";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code_test1, check);
        assert_eq!(diagnostics.len(), 1, "ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ should trigger");
        assert_eq!(diagnostics[0].code, DiagnosticCode::FullOuterJoinQuery);
        assert!(diagnostics[0].message.contains("FULL OUTER JOIN"));
        let range_text = &code_test1[diagnostics[0].range];
        assert!(
            range_text.contains("ПОЛНОЕ") || range_text.contains("FULL"),
            "Diagnostic should highlight FULL JOIN keywords, got: '{}'",
            range_text
        );

        let code_test2 = r#"Процедура Тест2()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
                   |    Товары.Номенклатура КАК Номенклатура
                   |ИЗ
                   |    Товары КАК Товары
                   |        ЛЕВОЕ СОЕДИНЕНИЕ ПланПродаж КАК ПланПродаж
                   |            ЛЕВОЕ СОЕДИНЕНИЕ ФактическиеПродажи КАК ФактическиеПродажи
                   |            ПО ПланПродаж.Номенклатура = ФактическиеПродажи.Номенклатура
                   |        ПО Товары.Номенклатура = ПланПродаж.Номенклатура";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code_test2, check);
        assert_eq!(diagnostics.len(), 0, "LEFT JOINs only should not trigger");
    }

    #[test]
    fn test_simple_english() {
        let code = r#"
Procedure Test()
    Query = "SELECT * FROM T1 FULL JOIN T2 ON T1.ID = T2.ID";
EndProcedure
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "FULL JOIN should trigger");
    }

    #[test]
    fn test_simple_russian() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ПОЛНОЕ СОЕДИНЕНИЕ Т2 ПО Т1.ID = Т2.ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "ПОЛНОЕ СОЕДИНЕНИЕ should trigger");
    }

    #[test]
    fn test_no_false_positives_left_join() {
        let code = r#"
Процедура Тест2()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
                   |    Товары.Номенклатура
                   |ИЗ
                   |    Товары КАК Товары
                   |        ЛЕВОЕ СОЕДИНЕНИЕ ПланПродаж";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "LEFT JOIN should not trigger");
    }

    #[test]
    fn test_full_join_without_outer() {
        let code = r#"
Процедура Тест()
    Query = "SELECT * FROM T1 ПОЛНОЕ СОЕДИНЕНИЕ T2 ПО T1.ID = T2.ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "FULL JOIN without OUTER should trigger");
    }

    #[test]
    fn test_multiple_full_joins() {
        let code = r#"
Процедура Тест()
    Query = "SELECT * FROM T1 FULL OUTER JOIN T2 ON T1.A = T2.A FULL OUTER JOIN T3 ON T1.B = T3.B";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 2, "Should detect multiple FULL JOINs");
    }

    #[test]
    fn test_multiline_simple() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ *
             |ИЗ Товары
             |    ПОЛНОЕ СОЕДИНЕНИЕ Продажи
             |    ПО Товары.ID = Продажи.ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect FULL JOIN in multiline query");
    }

    #[test]
    fn test_multiline_with_comment() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ *
             |ИЗ Товары
             |    ПОЛНОЕ СОЕДИНЕНИЕ Продажи // тест
             |    ПО Товары.ID = Продажи.ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect FULL JOIN even with comment");
    }

    #[test]
    fn test_nested_joins_like_fixture() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
                   |    Товары.Номенклатура
                   |ИЗ
                   |    Товары КАК Товары
                   |        ЛЕВОЕ СОЕДИНЕНИЕ ПланПродаж КАК ПланПродаж
                   |            ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ ФактическиеПродажи КАК ФактическиеПродажи
                   |            ПО ПланПродаж.Номенклатура = ФактическиеПродажи.Номенклатура
                   |        ПО Товары.Номенклатура = ПланПродаж.Номенклатура";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect nested FULL JOIN");
    }

    #[test]
    fn test_with_function_calls_in_select() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
                   |    Товары.Номенклатура КАК Номенклатура,
                   |    ЕСТЬNULL(ПланПродаж.Сумма, 0) КАК СуммаПлан
                   |ИЗ
                   |    Товары КАК Товары
                   |        ПОЛНОЕ СОЕДИНЕНИЕ ПланПродаж
                   |        ПО Товары.ID = ПланПродаж.ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect FULL JOIN with functions in SELECT");
    }
}
