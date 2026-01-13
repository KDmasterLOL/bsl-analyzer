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
//! - FullOuterJoinQueryDiagnostic.java (bsl-language-server)
//! - full_outer_join_query.rs (bsl-language-server-rust)

use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use sdbl_hir;
use tracing::debug;

/// Runs the FullOuterJoinQuery diagnostic.
///
/// Uses SDBL HIR with diagnostics collected during lowering.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::FullOuterJoinQuery) {
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
        let mapper = SdblPositionMapper::new_from_range_with_line_index(
            query_info.bsl_literal_range,
            &bsl_source,
            &line_starts,
        );

        // Emit diagnostics from HIR
        for hir_diag in sdbl_package.all_diagnostics() {
            if let sdbl_hir::SdblDiagnostic::FullOuterJoin { range } = hir_diag {
                let bsl_range = mapper.map_range(*range, &query_info.query_text);

                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::FullOuterJoinQuery,
                    message: "Using FULL OUTER JOIN significantly reduces query performance. \
                              Consider rewriting using UNION with LEFT JOIN"
                        .to_string(),
                    severity: Severity::Warning,
                    range: bsl_range,
                    tags: vec![],
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
    use crate::{DiagnosticCode, Severity};

    #[test]
    fn test_full_outer_join_query_from_fixture() {
        let code = include_str!("../../test_data/FullOuterJoinQueryDiagnostic.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);

        // Should detect exactly 1 FULL OUTER JOIN in the fixture
        assert_eq!(diagnostics.len(), 1, "Expected 1 FULL OUTER JOIN");

        // Verify it's the correct diagnostic type
        assert_eq!(diagnostics[0].code, DiagnosticCode::FullOuterJoinQuery);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert!(diagnostics[0].message.contains("FULL OUTER JOIN"));

        // Verify the diagnostic is in the query string (lines 4-13)
        // The FULL OUTER JOIN is on line 11 in the file
        let range_text = &code[diagnostics[0].range];
        assert!(
            range_text.contains("ПОЛНОЕ") || range_text.contains("FULL"),
            "Diagnostic should highlight the FULL JOIN keywords"
        );
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
