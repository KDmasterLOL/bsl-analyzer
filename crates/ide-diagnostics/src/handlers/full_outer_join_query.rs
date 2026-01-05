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
use syntax::ast::{JoinType, SdblQueryPackage};
use tracing::debug;

/// Runs the FullOuterJoinQuery diagnostic.
///
/// Uses cached SDBL queries from Salsa to avoid redundant tree walking and parsing.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::FullOuterJoinQuery) {
        return Vec::new();
    }

    // Get cached SDBL queries from HIR
    let sdbl_queries = ctx.db.all_sdbl_in_file(ctx.file_id);

    let input = ctx.db.file_text_input(ctx.file_id);
    let bsl_source = input.text(ctx.db);

    // Build shared line index once (performance optimization)
    use crate::sdbl_utils::build_line_index_shared;
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    for (_expr_id, query_info) in sdbl_queries.iter() {
        if !query_info.is_valid() {
            continue;
        }
        let Some(ref query_ast) = query_info.query_ast else {
            continue;
        };

        let mapper = SdblPositionMapper::new_from_range_with_line_index(
            query_info.bsl_literal_range,
            &bsl_source,
            &line_starts,
        );

        check_query(query_ast, &query_info.query_text, &mapper, &mut diagnostics);
    }

    debug!(
        time_ms = start.elapsed().as_millis(),
        diagnostics_found = diagnostics.len(),
        "FullOuterJoinQuery completed"
    );

    diagnostics
}

/// Check a single SDBL query for FULL OUTER JOINs.
fn check_query(
    query_ast: &syntax::Parse<syntax::SyntaxNode>,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use syntax::ast::AstNode;

    let root = query_ast.syntax_node();

    let Some(package) = SdblQueryPackage::cast(root) else {
        return;
    };

    for select_query in package.queries() {
        let Some(subquery) = select_query.subquery() else {
            continue;
        };
        let Some(main_query) = subquery.main_query() else {
            continue;
        };
        let Some(from_clause) = main_query.from_clause() else {
            continue;
        };

        // Recursively check data sources for FULL JOINs
        fn check_data_source(
            data_source: syntax::ast::SdblDataSource,
            mapper: &SdblPositionMapper,
            query_text: &str,
            diagnostics: &mut Vec<Diagnostic>,
        ) {
            // Check all join clauses in this data source
            for join in data_source.join_clauses() {
                let join_type = join.join_type();

                if join_type == JoinType::Full {
                    // Found FULL [OUTER] JOIN - create diagnostic
                    let sdbl_range = join.syntax().text_range();
                    let bsl_range = mapper.map_range(sdbl_range, query_text);

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

                // Recursively check nested data sources in this join
                if let Some(nested_source) = join.data_source() {
                    check_data_source(nested_source, mapper, query_text, diagnostics);
                }
            }
        }

        // Check all top-level data sources
        for data_source in from_clause.data_sources() {
            check_data_source(data_source, mapper, query_text, diagnostics);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use test_fixture::Fixture;

    /// Helper to run diagnostic on BSL code
    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        let fixture_content = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_content);
        let file_id = fixture.first_file().expect("No file in fixture");

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let config = Rc::new(DiagnosticsConfig::default());
        let ctx = crate::DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_full_outer_join_query_from_fixture() {
        let code = include_str!("../../test_data/FullOuterJoinQueryDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

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
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "FULL JOIN should trigger");
    }

    #[test]
    fn test_simple_russian() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ПОЛНОЕ СОЕДИНЕНИЕ Т2 ПО Т1.ID = Т2.ID";
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
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
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "LEFT JOIN should not trigger");
    }

    #[test]
    fn test_full_join_without_outer() {
        let code = r#"
Процедура Тест()
    Query = "SELECT * FROM T1 ПОЛНОЕ СОЕДИНЕНИЕ T2 ПО T1.ID = T2.ID";
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "FULL JOIN without OUTER should trigger");
    }

    #[test]
    fn test_multiple_full_joins() {
        let code = r#"
Процедура Тест()
    Query = "SELECT * FROM T1 FULL OUTER JOIN T2 ON T1.A = T2.A FULL OUTER JOIN T3 ON T1.B = T3.B";
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
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
        let diagnostics = check_diagnostic(code);
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
        let diagnostics = check_diagnostic(code);
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
        let diagnostics = check_diagnostic(code);
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
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect FULL JOIN with functions in SELECT");
    }
}
