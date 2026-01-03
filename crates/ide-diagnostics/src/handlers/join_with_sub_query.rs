//! JoinWithSubQuery diagnostic.
//!
//! Detects usage of subqueries in JOIN operations in SDBL queries.
//!
//! ## Why?
//! Joins with subqueries cause severe performance issues in 1C:Enterprise.
//! The query optimizer struggles with subqueries in JOINs, leading to:
//! - Extremely slow query execution, especially under low server load
//! - Unpredictable performance (fast sometimes, very slow other times)
//! - Significant execution time differences across different DBMS
//! - Performance degradation over time as statistics become stale
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT T.Ref FROM Catalog.Items AS Ref
//!          LEFT JOIN (SELECT S.Ref FROM Catalog.Suppliers WHERE S.Active = TRUE) AS T
//!          ON Ref.Supplier = T.Ref";
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Use temporary tables or metadata objects instead:
//! Query = "SELECT Suppliers.Ref INTO TempSuppliers
//!          FROM Catalog.Suppliers AS Suppliers WHERE Suppliers.Active = TRUE;
//!
//!          SELECT T.Ref FROM Catalog.Items AS Ref
//!          LEFT JOIN TempSuppliers AS T ON Ref.Supplier = T.Ref";
//! ```
//!
//! ## Implementation
//! Ported from:
//! - JoinWithSubQueryDiagnostic.java (bsl-language-server)

use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::ast::{AstNode, SdblQueryPackage};
use tracing::debug;

/// Runs the JoinWithSubQuery diagnostic.
///
/// Uses cached SDBL queries from Salsa to avoid redundant tree walking and parsing.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::JoinWithSubQuery) {
        return Vec::new();
    }

    // Get cached SDBL queries from Salsa
    let sdbl_queries = ctx.db.sdbl_queries(ctx.file_id);

    let input = ctx.db.file_text_input(ctx.file_id);
    let bsl_source = input.text(ctx.db);

    // Build shared line index once (performance optimization)
    use crate::sdbl_utils::build_line_index_shared;
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    for query_info in sdbl_queries.iter() {
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
        "JoinWithSubQuery completed"
    );

    diagnostics
}

/// Check a single SDBL query for subqueries in JOINs.
fn check_query(
    query_ast: &syntax::Parse<syntax::SyntaxNode>,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let root = query_ast.syntax_node();

    let Some(package) = SdblQueryPackage::cast(root) else {
        return;
    };

    for select_query in package.queries() {
        let Some(subquery) = select_query.subquery() else {
            continue;
        };

        // Check all queries (main query + UNION queries)
        for query in subquery.queries() {
            let Some(from_clause) = query.from_clause() else {
                continue;
            };

            // Check all top-level data sources
            for data_source in from_clause.data_sources() {
                check_data_source(data_source, mapper, query_text, diagnostics);
            }
        }
    }
}

/// Recursively check a data source for subqueries in JOIN contexts.
fn check_data_source(
    data_source: syntax::ast::SdblDataSource,
    mapper: &SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Case 1: Data source is a subquery that has JOINs
    // Java: visitDataSources() - checks !joinPart().isEmpty() && subquery() != null
    let has_joins = data_source.join_clauses().next().is_some();

    if has_joins {
        if let Some(subquery) = data_source.subquery() {
            // Report diagnostic on the subquery
            let sdbl_range = subquery.syntax().text_range();
            let bsl_range = mapper.map_range(sdbl_range, query_text);

            diagnostics.push(Diagnostic {
                code: DiagnosticCode::JoinWithSubQuery,
                message: "Don't use a join with sub queries. \
                          Joins with subqueries cause severe performance issues."
                    .to_string(),
                severity: Severity::Major,
                range: bsl_range,
                tags: vec![],
                fixes: vec![],
            });
        }
    }

    // Case 2: Check each JOIN clause for subqueries
    // Java: visitJoinPart() - checks dataSource().subquery() != null
    for join in data_source.join_clauses() {
        if let Some(join_data_source) = join.data_source() {
            // Check if the JOIN's data source is a subquery
            if let Some(subquery) = join_data_source.subquery() {
                // Report diagnostic on the subquery
                let sdbl_range = subquery.syntax().text_range();
                let bsl_range = mapper.map_range(sdbl_range, query_text);

                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::JoinWithSubQuery,
                    message: "Don't use a join with sub queries. \
                              Joins with subqueries cause severe performance issues."
                        .to_string(),
                    severity: Severity::Major,
                    range: bsl_range,
                    tags: vec![],
                    fixes: vec![],
                });
            }

            // Recursively check nested data sources in the join
            check_data_source(join_data_source, mapper, query_text, diagnostics);
        }
    }

    // Case 3: If this data source is a subquery, recursively check its inner queries
    // This handles nested subqueries like: SELECT * FROM (SELECT ... FROM (SELECT ...) JOIN ...)
    if let Some(subquery) = data_source.subquery() {
        // Check all queries within this subquery (main query + UNION queries)
        for inner_query in subquery.queries() {
            if let Some(from_clause) = inner_query.from_clause() {
                // Recursively check all data sources in the inner query
                for inner_data_source in from_clause.data_sources() {
                    check_data_source(inner_data_source, mapper, query_text, diagnostics);
                }
            }
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
        };

        check(&ctx)
    }

    #[test]
    fn test_join_with_sub_query_from_fixture() {
        let code = include_str!("../../test_data/JoinWithSubQueryDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 7, "Expected 7 JoinWithSubQuery diagnostics");

        // Verify all are correct type and severity
        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::JoinWithSubQuery);
            assert_eq!(diag.severity, Severity::Major);
        }
    }

    #[test]
    fn test_simple_left_join_with_subquery() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ЛЕВОЕ СОЕДИНЕНИЕ (ВЫБРАТЬ * ИЗ Т2) КАК С ПО Т1.ID = С.ID";
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "LEFT JOIN with subquery should trigger");
    }

    #[test]
    fn test_no_false_positive_table_join() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Справочник.Товары ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Цены ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "JOIN with table should not trigger");
    }

    #[test]
    fn test_no_false_positive_subquery_without_join() {
        let code = r#"
Процедура Тест7()
    Запрос = "ВЫБРАТЬ * ИЗ (ВЫБРАТЬ * ИЗ Т1) КАК С1, (ВЫБРАТЬ * ИЗ Т2) КАК С2";
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Subqueries in FROM without JOINs should not trigger");
    }

    #[test]
    fn test_right_join_with_subquery() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ПРАВОЕ СОЕДИНЕНИЕ (ВЫБРАТЬ * ИЗ Т2) КАК С ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "RIGHT JOIN with subquery should trigger");
    }

    #[test]
    fn test_inner_join_with_subquery() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ВНУТРЕННЕЕ СОЕДИНЕНИЕ (ВЫБРАТЬ * ИЗ Т2) ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "INNER JOIN with subquery should trigger");
    }

    #[test]
    fn test_subquery_in_from_with_joins() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ (ВЫБРАТЬ * ИЗ Т1) КАК С ЛЕВОЕ СОЕДИНЕНИЕ Т2 ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Subquery in FROM with JOINs should trigger");
    }

    #[test]
    fn test_multiline_subquery_in_from_with_joins() {
        // This matches Тест4 from the fixture
        let code = r#"
Процедура Тест()
    Запрос = "Выбрать Т.Ссылка
    | Из (Выбрать СС.Ссылка Из Справочник.Справочник1 КАК СС Где СС.Ссылка = &Параметр) как СПр Левое соединение
    |(Выбрать СС.Ссылка Из Справочник.Справочник2 КАК СС Где СС.Ссылка = &Параметр) КАК Т
    |По СПр.Поле1 = Т.Ссылка";
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(
            diagnostics.len(),
            2,
            "Should detect both: subquery in FROM with JOINs + subquery in JOIN"
        );
    }
}
