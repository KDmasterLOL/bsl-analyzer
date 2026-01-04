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
use syntax::ast::{AstNode, SdblQueryPackage};
use syntax::SyntaxKind;
use tracing::debug;

/// Runs the LogicalOrInTheWhereSectionOfQuery diagnostic.
///
/// Uses cached SDBL queries from Salsa to avoid redundant tree walking and parsing.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::LogicalOrInTheWhereSectionOfQuery) {
        return Vec::new();
    }

    let sdbl_queries = ctx.db.sdbl_queries(ctx.file_id);

    let input = ctx.db.file_text_input(ctx.file_id);
    let bsl_source = input.text(ctx.db);

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
        "LogicalOrInTheWhereSectionOfQuery completed"
    );

    diagnostics
}

/// Check a single SDBL query for OR operators in WHERE clauses.
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

        check_query_where_clause(&main_query, query_text, mapper, diagnostics);
        check_subqueries_in_from(&main_query, query_text, mapper, diagnostics);
        check_subqueries_in_where(&main_query, query_text, mapper, diagnostics);
    }
}

/// Check WHERE clause for OR operators.
///
/// Uses `descendants_with_tokens()` to find ALL OR tokens recursively,
/// including those nested inside parentheses. This matches Java's
/// `Trees.findAllTokenNodes(ctx.where, SDBLParser.OR)` behavior.
fn check_query_where_clause(
    query: &syntax::ast::SdblQuery,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(where_clause) = query.where_clause() else {
        return;
    };

    for element in where_clause.syntax().descendants_with_tokens() {
        if let Some(token) = element.as_token() {
            if token.kind() == SyntaxKind::KW_OR {
                let sdbl_range = token.text_range();
                let bsl_range = mapper.map_range(sdbl_range, query_text);

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
}

/// Recursively check subqueries in WHERE clause.
///
/// Example: `WHERE ID IN (SELECT ID FROM T2 WHERE A = 1 OR B = 2)`
///                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
///                        Nested subquery with WHERE in WHERE expression
///
/// Note: We don't call check_query_where_clause here because descendants_with_tokens()
/// in the parent call already traverses all nested subqueries.
fn check_subqueries_in_where(
    query: &syntax::ast::SdblQuery,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use syntax::ast::SdblSubquery;

    let Some(where_clause) = query.where_clause() else {
        return;
    };

    for node in where_clause.syntax().descendants() {
        if node.kind() == SyntaxKind::SDBL_SUBQUERY {
            if let Some(subquery) = SdblSubquery::cast(node) {
                if let Some(nested_query) = subquery.main_query() {
                    check_subqueries_in_from(&nested_query, query_text, mapper, diagnostics);
                }
            }
        }
    }
}

/// Recursively check subqueries in FROM clause.
///
/// Example: `SELECT * FROM (SELECT ID FROM T2 WHERE A = 1 OR B = 2)`
///                         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
///                         Nested subquery with WHERE
///
/// Note: We need to check WHERE clauses of subqueries in FROM because they are
/// in a different subtree from the main query's WHERE clause.
fn check_subqueries_in_from(
    query: &syntax::ast::SdblQuery,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use syntax::ast::SdblSubquery;

    let Some(from_clause) = query.from_clause() else {
        return;
    };

    for node in from_clause.syntax().descendants() {
        if node.kind() == SyntaxKind::SDBL_SUBQUERY {
            if let Some(subquery) = SdblSubquery::cast(node) {
                if let Some(nested_query) = subquery.main_query() {
                    check_query_where_clause(&nested_query, query_text, mapper, diagnostics);
                    check_subqueries_in_from(&nested_query, query_text, mapper, diagnostics);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
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

        let diagnostics = check(&ctx);
        let content = fixture.files.get(&file_id).unwrap().content.to_string();
        (diagnostics, content)
    }

    #[test]
    fn test_from_java_fixture() {
        let code = include_str!("../../test_data/LogicalOrInTheWhereSectionOfQueryDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 6, "Expected 6 diagnostics");

        // Java uses 1-based line numbers, test file shows:
        // Line 8 (Java) = Line 7 (Rust 0-indexed)
        assert_diagnostic_range(&file_content, &diagnostics[0], 7, 15, 18);
        assert_diagnostic_range(&file_content, &diagnostics[1], 19, 8, 11);
        assert_diagnostic_range(&file_content, &diagnostics[2], 31, 38, 41);
        assert_diagnostic_range(&file_content, &diagnostics[3], 43, 8, 11);
        assert_diagnostic_range(&file_content, &diagnostics[4], 44, 36, 39);
        assert_diagnostic_range(&file_content, &diagnostics[5], 58, 21, 24);
    }

    #[test]
    fn test_simple_or_in_where() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT Name FROM Products WHERE Type = 1 OR Category = 2";
КонецПроцедуры"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_russian_or_keyword() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Товары ГДЕ Цена = 100 ИЛИ Количество = 0";
КонецПроцедуры"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_or_in_parentheses() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T WHERE A = 1 AND (B = 2 OR C = 3)";
КонецПроцедуры"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect OR inside parentheses");
    }

    #[test]
    fn test_multiple_or_in_where() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T WHERE A = 1 OR B = 2 OR C = 3";
КонецПроцедуры"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 2, "Should detect both OR operators");
    }

    #[test]
    fn test_nested_subquery() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T1 WHERE ID IN (SELECT ID FROM T2 WHERE A = 1 OR B = 2)";
КонецПроцедуры"#;
        let (diagnostics, _content) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect OR in nested subquery WHERE");
    }

    #[test]
    fn test_no_false_positives_case_expression() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT CASE WHEN Flag OR True THEN 1 ELSE 0 END AS Result FROM T WHERE ID = 1";
КонецПроцедуры"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Should NOT detect OR in CASE expression (not in WHERE)");
    }

    #[test]
    fn test_no_false_positives_join_on() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T1 LEFT JOIN T2 ON T1.A = T2.A OR T1.B = T2.B WHERE T1.ID = 1";
КонецПроцедуры"#;
        let (diagnostics, _) = check_diagnostic(code);
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
        let (diagnostics, _) = check_diagnostic(code);
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
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect OR inside parentheses after AND");
    }

    #[test]
    fn test_sdbl_with_parameters() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T WHERE Field1 = &Param1 AND (Field2 = &Param2 OR Field3 = &Param3)";
КонецПроцедуры"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect OR with parameters");
    }
}
