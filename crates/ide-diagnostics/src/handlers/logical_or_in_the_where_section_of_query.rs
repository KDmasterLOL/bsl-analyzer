//! LogicalOrInTheWhereSectionOfQuery diagnostic.
//!
//! Detects OR operators in WHERE clauses of SDBL queries.
//!
//! ## Why?
//! OR operators in WHERE clauses prevent the 1C:Enterprise query optimizer from using indexes
//! effectively. When the optimizer encounters OR conditions, it typically performs full table
//! scans instead of index seeks, leading to:
//! - Dramatically slower query execution (10x-100x slower)
//! - Higher memory consumption for large result sets
//! - Increased lock contention and blocking
//! - Poor scalability with large datasets
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT Name, Price
//!          FROM Products
//!          WHERE Type = 1 OR Category = 2";
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Use UNION instead to allow index usage on each condition:
//! Query = "SELECT Name, Price
//!          FROM Products
//!          WHERE Type = 1
//!          UNION
//!          SELECT Name, Price
//!          FROM Products
//!          WHERE Category = 2";
//! ```
//!
//! ## Implementation
//! Ported from:
//! - LogicalOrInTheWhereSectionOfQueryDiagnostic.java (bsl-language-server)

use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use sdbl_hir;
use tracing::debug;

/// Runs the LogicalOrInTheWhereSectionOfQuery diagnostic.
///
/// Uses SDBL HIR with diagnostics collected during lowering.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::LogicalOrInTheWhereSectionOfQuery) {
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
            if let sdbl_hir::SdblDiagnostic::LogicalOrInWhere { range } = hir_diag {
                let bsl_range = mapper.map_range(*range, &query_info.query_text);

                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
                    message: "Using OR operator in WHERE clause severely degrades query performance. Consider rewriting using UNION or restructuring conditions".to_string(),
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
        "LogicalOrInTheWhereSectionOfQuery completed"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_sdbl_diagnostic};

    #[test]
    fn test_from_java_fixture() {
        let code = include_str!("../../test_data/LogicalOrInTheWhereSectionOfQueryDiagnostic.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 6, "Expected 6 diagnostics");

        // Java uses 1-based line numbers, test file shows:
        // Line 8 (Java) = Line 7 (Rust 0-indexed)
        assert_diagnostic_range(code, &diagnostics[0], 7, 15, 18);
        assert_diagnostic_range(code, &diagnostics[1], 19, 8, 11);
        assert_diagnostic_range(code, &diagnostics[2], 31, 38, 41);
        assert_diagnostic_range(code, &diagnostics[3], 43, 8, 11);
        assert_diagnostic_range(code, &diagnostics[4], 44, 36, 39);
        assert_diagnostic_range(code, &diagnostics[5], 58, 21, 24);
    }

    #[test]
    fn test_simple_or_in_where() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT Name FROM Products WHERE Type = 1 OR Category = 2";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_russian_or_keyword() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Товары ГДЕ Цена = 100 ИЛИ Количество = 0";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_or_in_parentheses() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T WHERE A = 1 AND (B = 2 OR C = 3)";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect OR inside parentheses");
    }

    #[test]
    fn test_multiple_or_in_where() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T WHERE A = 1 OR B = 2 OR C = 3";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 2, "Should detect both OR operators");
    }

    #[test]
    fn test_nested_subquery() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T1 WHERE ID IN (SELECT ID FROM T2 WHERE A = 1 OR B = 2)";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect OR in nested subquery WHERE");
    }

    #[test]
    fn test_no_false_positives_case_expression() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT CASE WHEN Flag OR True THEN 1 ELSE 0 END AS Result FROM T WHERE ID = 1";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should NOT detect OR in CASE expression (not in WHERE)");
    }

    #[test]
    fn test_no_false_positives_join_on() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T1 LEFT JOIN T2 ON T1.A = T2.A OR T1.B = T2.B WHERE T1.ID = 1";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Should NOT detect OR in JOIN ON clause (different diagnostic)"
        );
    }

    #[test]
    fn test_no_where_clause() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM Products";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not fail on missing WHERE");
    }

    #[test]
    fn test_and_with_or_in_parentheses() {
        let code = r#"
Процедура Тест()
    Запрос.Текст = "ВЫБРАТЬ Таблица.Наименование
    |ИЗ Справочник.Товары КАК Таблица
    |ГДЕ
    |   Таблица.Поле1 = 1
    |   И (Таблица.Поле2 = 2 ИЛИ Таблица.Поле3 = 3)";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect OR inside parentheses after AND");
    }

    #[test]
    fn test_sdbl_with_parameters() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T WHERE Field1 = &Param1 AND (Field2 = &Param2 OR Field3 = &Param3)";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect OR with parameters");
    }
}
