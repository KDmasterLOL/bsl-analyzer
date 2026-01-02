//! AssignAliasFieldsInQuery diagnostic.
//!
//! Checks that all fields in SDBL subqueries have explicit aliases with AS/КАК keyword.
//!
//! ## Why?
//! Subqueries are often used in FROM clauses. Explicit aliases make queries more readable
//! and maintainable. Without AS keyword, it's unclear whether the identifier is an alias
//! or part of the field expression.
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT * FROM (SELECT Ref FROM Catalog.Products) AS Sub";
//!         // Error: 'Ref' should be 'Ref AS Ref' (missing AS keyword)
//!
//! Query = "SELECT * FROM (SELECT Name, Code FROM Catalog.Products) AS Sub";
//!         // Error: both 'Name' and 'Code' need explicit AS keyword
//! ```
//!
//! ## Good practice
//! ```bsl
//! Query = "SELECT * FROM (SELECT Ref AS Ref FROM Catalog.Products) AS Sub";
//!
//! Query = "SELECT * FROM (SELECT * FROM Table) AS Sub"; // OK: asterisk doesn't need alias
//!
//! Query = "SELECT Name FROM Catalog.Products"; // OK: main query not checked
//! ```
//!
//! ## Rules
//! - Only subqueries are checked (not main queries)
//! - Asterisk fields (`*`, `Table.*`) don't require aliases
//! - AS/КАК keyword must be explicit (implicit aliases are forbidden)
//! - UNION: only first query in UNION is checked
//!
//! ## Implementation
//!
//! Ported from:
//! - AssignAliasFieldsInQueryDiagnostic.java (bsl-language-server)
//! - assign_alias_fields_in_query.rs (bsl-language-server-rust)
//!
//! Adapted to use full SDBL parser with AST instead of token-based approach.

use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{
    ast::{AstNode, SdblAlias, SdblQuery, SdblQueryPackage, SdblSelectQuery, SdblSelectedField},
    Parse, SyntaxKind, SyntaxNode, TextRange,
};

/// Runs the AssignAliasFieldsInQuery diagnostic.
///
/// Uses cached SDBL queries from Salsa to avoid redundant tree walking and parsing.
/// Checks for fields without AS keyword and reports diagnostics at correct BSL positions.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;

    let start = Instant::now();

    // Check if diagnostic is disabled
    if ctx.config.is_disabled(DiagnosticCode::AssignAliasFieldsInQuery) {
        return Vec::new();
    }

    // ✅ NEW: Get cached SDBL queries (no tree walking!)
    let cache_start = Instant::now();
    let sdbl_queries = ctx.db.sdbl_queries(ctx.file_id);
    let cache_ms = cache_start.elapsed().as_micros();

    // Get BSL source text for position mapping
    let source_start = Instant::now();
    let input = ctx.db.file_text_input(ctx.file_id);
    let bsl_source = input.text(ctx.db);
    let source_ms = source_start.elapsed().as_micros();

    let mut diagnostics = Vec::new();

    // Time measurements
    let mut time_mapper_creation_us = 0u128;
    let mut time_analyzing_ast_us = 0u128;
    let mut queries_analyzed = 0;

    // OPTIMIZATION: Build line index ONCE for the entire file
    // Instead of rebuilding it for each of the 102 mappers (was 241ms overhead!)
    use crate::sdbl_utils::build_line_index_shared;
    let line_index_start = Instant::now();
    let line_starts = build_line_index_shared(&bsl_source);
    let time_line_index_us = line_index_start.elapsed().as_micros();

    // Process each cached SDBL query
    for query_info in sdbl_queries.iter() {
        // Skip if not valid SDBL
        if !query_info.is_valid() {
            continue;
        }

        let Some(ref query_ast) = query_info.query_ast else {
            continue;
        };

        tracing::debug!(query_len = query_info.query_text.len(), "Analyzing SDBL query from cache");

        // Create position mapper (reuses shared line_starts from above)
        let mapper_start = Instant::now();
        let mapper = SdblPositionMapper::new_from_range_with_line_index(
            query_info.bsl_literal_range,
            &bsl_source,
            &line_starts,
        );
        time_mapper_creation_us += mapper_start.elapsed().as_micros();

        // Check SDBL query AST (already parsed!)
        let analyze_start = Instant::now();
        check_sdbl_query_optimized(query_ast, &query_info.query_text, &mapper, &mut diagnostics);
        time_analyzing_ast_us += analyze_start.elapsed().as_micros();
        queries_analyzed += 1;
    }

    let total_elapsed = start.elapsed().as_millis();

    tracing::debug!(
        total_ms = total_elapsed,
        cache_fetch_us = cache_ms,
        source_fetch_us = source_ms,
        line_index_us = time_line_index_us,
        mapper_creation_us = time_mapper_creation_us,
        analyzing_ast_us = time_analyzing_ast_us,
        queries_from_cache = sdbl_queries.len(),
        queries_analyzed,
        diagnostics_found = diagnostics.len(),
        "[PROFILE] AssignAliasFieldsInQuery"
    );

    diagnostics
}

/// Check a single SDBL query for fields without AS keyword (optimized).
///
/// OPTIMIZATION: Uses cached query_text directly instead of root.text().to_string()
/// This eliminates string allocation for each query.
fn check_sdbl_query_optimized(
    parse: &Parse<SyntaxNode>,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let root = parse.syntax_node();

    // Get query package
    let Some(package) = SdblQueryPackage::cast(root) else {
        return;
    };

    // Check each SELECT query
    // OPTIMIZATION: Use query_text from cache instead of root.text().to_string()
    for select_query in package.queries() {
        check_select_query_with_mapper(&select_query, query_text, mapper, diagnostics);
    }
}

/// Check a SELECT query for fields without AS keyword (with position mapping).
fn check_select_query_with_mapper(
    select_query: &SdblSelectQuery,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use std::time::Instant;

    let Some(subquery) = select_query.subquery() else {
        return;
    };

    // Check main query fields (parent is subquery ✓)
    let check_start = Instant::now();
    let diag_count_before = diagnostics.len();

    if let Some(main_query) = subquery.main_query() {
        check_query_fields_and_subqueries_with_mapper(&main_query, query_text, mapper, diagnostics);
    }

    let check_us = check_start.elapsed().as_micros();
    let diag_count = diagnostics.len() - diag_count_before;

    if check_us > 10000 {
        tracing::debug!(check_us, diag_count, "[PROFILE] check_select_query_with_mapper (>10ms)");
    }

    // Note: UNION queries are NOT checked (parent is union node, not subquery)
}

/// Check query fields AND recursively check subqueries in FROM clause (with mapper).
fn check_query_fields_and_subqueries_with_mapper(
    query: &SdblQuery,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use std::time::Instant;

    // Check fields in this query
    let fields_start = Instant::now();
    check_query_fields_with_mapper(query, query_text, mapper, diagnostics);
    let fields_us = fields_start.elapsed().as_micros();

    // Recursively check subqueries in FROM clause
    let from_start = Instant::now();
    if let Some(from_clause) = query.from_clause() {
        check_from_clause_for_subqueries_with_mapper(&from_clause, query_text, mapper, diagnostics);
    }
    let from_us = from_start.elapsed().as_micros();

    if fields_us > 5000 || from_us > 5000 {
        tracing::debug!(fields_us, from_us, "[PROFILE] check_query_fields_and_subqueries");
    }
}

/// Check fields in a query for missing AS keyword (with mapper).
fn check_query_fields_with_mapper(
    query: &SdblQuery,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(field_list) = query.field_list() else {
        return;
    };

    for field in field_list.fields() {
        check_field_with_mapper(&field, query_text, mapper, diagnostics);
    }
}

/// Recursively check subqueries in FROM clause (with mapper).
fn check_from_clause_for_subqueries_with_mapper(
    from_clause: &syntax::ast::SdblFromClause,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use std::time::Instant;
    use syntax::ast::{AstNode, SdblSubquery};
    use syntax::SyntaxKind;

    // Walk descendants looking for SDBL_SUBQUERY nodes
    // These are subqueries in FROM clause like: FROM (SELECT ... ) AS Sub
    let desc_start = Instant::now();
    let mut subquery_count = 0;

    for node in from_clause.syntax().descendants() {
        if node.kind() == SyntaxKind::SDBL_SUBQUERY {
            if let Some(subquery) = SdblSubquery::cast(node) {
                subquery_count += 1;
                // Check the main query in this subquery (parent is subquery ✓)
                if let Some(main_query) = subquery.main_query() {
                    check_query_fields_and_subqueries_with_mapper(
                        &main_query,
                        query_text,
                        mapper,
                        diagnostics,
                    );
                }
            }
        }
    }

    let desc_us = desc_start.elapsed().as_micros();
    if desc_us > 5000 {
        tracing::debug!(desc_us, subquery_count, "[PROFILE] check_from_clause descendants()");
    }
}

/// Check a single field for missing AS keyword (with mapper).
fn check_field_with_mapper(
    field: &SdblSelectedField,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Skip asterisk fields (they don't need aliases)
    if field.is_asterisk() {
        return;
    }

    // Check if field has alias
    if let Some(alias) = field.alias() {
        // Alias exists, check if it has AS keyword
        if !alias.has_as_keyword() {
            // ERROR: Alias without AS keyword (implicit alias)
            // Report diagnostic on the whole field expression (not just alias)
            add_diagnostic_for_alias_with_mapper(field, &alias, query_text, mapper, diagnostics);
        }
    } else {
        // ERROR: Field without alias at all
        add_diagnostic_for_field_with_mapper(field, query_text, mapper, diagnostics);
    }
}

/// Add diagnostic for alias without AS keyword (with position mapping).
/// Reports the diagnostic on the whole field expression (column ref + alias).
fn add_diagnostic_for_alias_with_mapper(
    field: &SdblSelectedField,
    alias: &SdblAlias,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use std::time::Instant;

    let alias_name = alias.name().unwrap_or_else(|| "<unknown>".to_string());

    // Use the whole field's range (expression + alias)
    // Get first token from expression and last token from alias
    let token_start = Instant::now();
    let sdbl_range = match (field.expression(), alias.identifier()) {
        (Some(expr), Some(alias_ident)) => {
            // Get first non-whitespace token from expression
            let first_token = expr.first_token();
            let first_non_ws = first_token.and_then(|t| {
                if t.kind() == SyntaxKind::WHITESPACE {
                    t.next_token()
                } else {
                    Some(t)
                }
            });

            // Use alias identifier as last token (it's already trimmed)
            if let Some(first) = first_non_ws {
                TextRange::new(first.text_range().start(), alias_ident.text_range().end())
            } else {
                field.syntax().text_range()
            }
        }
        _ => field.syntax().text_range(),
    };
    let token_us = token_start.elapsed().as_micros();

    let map_start = Instant::now();
    let bsl_range = mapper.map_range(sdbl_range, query_text);
    let map_us = map_start.elapsed().as_micros();

    if token_us > 100 || map_us > 100 {
        tracing::trace!(token_us, map_us, "[PROFILE] add_diagnostic_for_alias");
    }

    diagnostics.push(Diagnostic {
        code: DiagnosticCode::AssignAliasFieldsInQuery,
        message: format!(
            "Поле '{}' должно иметь явный псевдоним с ключевым словом AS/КАК",
            alias_name
        ),
        severity: Severity::Warning,
        range: bsl_range, // Now BSL-relative!
        tags: vec![],
        fixes: vec![],
    });
}

/// Add diagnostic for field without alias (with position mapping).
fn add_diagnostic_for_field_with_mapper(
    field: &SdblSelectedField,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use std::time::Instant;

    // Get SDBL range - trim leading/trailing whitespace from expression
    let token_start = Instant::now();
    let sdbl_range = if let Some(expr) = field.expression() {
        let full_range = expr.text_range();

        // Get first and last non-whitespace tokens to exclude leading/trailing whitespace
        let first_token = expr.first_token();
        let last_token = expr.last_token();

        match (first_token, last_token) {
            (Some(_first), Some(_last)) => {
                // Skip leading whitespace tokens
                let first_non_ws = expr.first_token().and_then(|t| {
                    if t.kind() == SyntaxKind::WHITESPACE {
                        t.next_token()
                    } else {
                        Some(t)
                    }
                });

                // Skip trailing whitespace/comment tokens
                let last_non_trivia = expr.last_token().and_then(|t| {
                    let mut token = t;
                    while matches!(
                        token.kind(),
                        SyntaxKind::WHITESPACE | SyntaxKind::COMMENT | SyntaxKind::NEWLINE
                    ) {
                        token = token.prev_token()?;
                    }
                    Some(token)
                });

                match (first_non_ws, last_non_trivia) {
                    (Some(first_real), Some(last_real)) => TextRange::new(
                        first_real.text_range().start(),
                        last_real.text_range().end(),
                    ),
                    _ => full_range,
                }
            }
            _ => full_range,
        }
    } else {
        field.syntax().text_range()
    };
    let token_us = token_start.elapsed().as_micros();

    let map_start = Instant::now();
    let bsl_range = mapper.map_range(sdbl_range, query_text);
    let map_us = map_start.elapsed().as_micros();

    if token_us > 100 || map_us > 100 {
        tracing::trace!(token_us, map_us, "[PROFILE] add_diagnostic_for_field");
    }

    diagnostics.push(Diagnostic {
        code: DiagnosticCode::AssignAliasFieldsInQuery,
        message: "Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК".to_string(),
        severity: Severity::Warning,
        range: bsl_range, // Now BSL-relative!
        tags: vec![],
        fixes: vec![],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        sdbl_utils::extract_string_content,
        test_utils::{assert_diagnostic_range, range_to_line_col},
        DiagnosticsConfig,
    };
    use parser::parse_sdbl;
    use std::sync::Arc;

    /// Check a single SDBL query for fields without AS keyword (test version).
    ///
    /// This version doesn't use position mapping and returns SDBL-relative positions.
    /// Used by standalone SDBL tests.
    fn check_sdbl_query(query_text: &str, diagnostics: &mut Vec<Diagnostic>) {
        use syntax::ast::{AstNode, SdblSubquery};
        use syntax::SyntaxKind;

        // Try to parse as SDBL
        let parse = parse_sdbl(query_text);

        // If parse has errors, skip (might not be SDBL)
        if parse.has_errors() {
            return;
        }

        let root = parse.syntax_node();

        // Get query package
        let Some(package) = SdblQueryPackage::cast(root) else {
            return;
        };

        // Check each SELECT query
        for select_query in package.queries() {
            let Some(subquery) = select_query.subquery() else {
                continue;
            };

            // Check main query fields
            if let Some(main_query) = subquery.main_query() {
                // Check fields in this query
                if let Some(field_list) = main_query.field_list() {
                    for field in field_list.fields() {
                        // Skip asterisk fields
                        if field.is_asterisk() {
                            continue;
                        }

                        // Check if field has alias
                        if let Some(alias) = field.alias() {
                            if !alias.has_as_keyword() {
                                // ERROR: Alias without AS keyword
                                let alias_name =
                                    alias.name().unwrap_or_else(|| "<unknown>".to_string());
                                diagnostics.push(Diagnostic {
                                    code: DiagnosticCode::AssignAliasFieldsInQuery,
                                    message: format!(
                                        "Поле '{}' должно иметь явный псевдоним с ключевым словом AS/КАК",
                                        alias_name
                                    ),
                                    severity: Severity::Warning,
                                    range: alias.syntax().text_range(),
                                    tags: vec![],
                                    fixes: vec![],
                                });
                            }
                        } else {
                            // ERROR: Field without alias
                            diagnostics.push(Diagnostic {
                                code: DiagnosticCode::AssignAliasFieldsInQuery,
                                message: "Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК".to_string(),
                                severity: Severity::Warning,
                                range: field.syntax().text_range(),
                                tags: vec![],
                                fixes: vec![],
                            });
                        }
                    }
                }

                // Check subqueries in FROM clause
                if let Some(from_clause) = main_query.from_clause() {
                    for node in from_clause.syntax().descendants() {
                        if node.kind() == SyntaxKind::SDBL_SUBQUERY {
                            if let Some(sub) = SdblSubquery::cast(node) {
                                if let Some(sub_main_query) = sub.main_query() {
                                    if let Some(field_list) = sub_main_query.field_list() {
                                        for field in field_list.fields() {
                                            if field.is_asterisk() {
                                                continue;
                                            }
                                            if let Some(alias) = field.alias() {
                                                if !alias.has_as_keyword() {
                                                    let alias_name = alias
                                                        .name()
                                                        .unwrap_or_else(|| "<unknown>".to_string());
                                                    diagnostics.push(Diagnostic {
                                                        code: DiagnosticCode::AssignAliasFieldsInQuery,
                                                        message: format!(
                                                            "Поле '{}' должно иметь явный псевдоним с ключевым словом AS/КАК",
                                                            alias_name
                                                        ),
                                                        severity: Severity::Warning,
                                                        range: alias.syntax().text_range(),
                                                        tags: vec![],
                                                        fixes: vec![],
                                                    });
                                                }
                                            } else {
                                                diagnostics.push(Diagnostic {
                                                    code: DiagnosticCode::AssignAliasFieldsInQuery,
                                                    message: "Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК".to_string(),
                                                    severity: Severity::Warning,
                                                    range: field.syntax().text_range(),
                                                    tags: vec![],
                                                    fixes: vec![],
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Helper to check a standalone SDBL query (for testing)
    fn check_standalone_query(query_text: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        check_sdbl_query(query_text, &mut diagnostics);
        diagnostics
    }

    /// Helper to run diagnostic on BSL code (like all_function_path_must_have_return)
    fn check_diagnostic(code: &str, config: DiagnosticsConfig) -> (Vec<Diagnostic>, String) {
        use ide_db::base_db::SourceDatabase;
        use ide_db::RootDatabaseImpl;
        use test_fixture::Fixture;

        // Create fixture with test file
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        // Create database
        let mut db = RootDatabaseImpl::new();

        // Set file content in database from fixture
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        // Create diagnostics context
        use ide_db::RootDatabase;
        // RootDatabase trait object is not Send/Sync (Salsa is single-threaded).
        // Arc is used for trait object lifetime management in tests, not thread-safety.
        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let ctx = crate::DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        // Run diagnostic
        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_field_with_explicit_as() {
        // Should pass - has AS keyword
        let query = "SELECT Name AS ProductName FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_field_without_as_keyword() {
        // Should fail - implicit alias (no AS keyword)
        let query = "SELECT Name ProductName FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("AS/КАК"));
    }

    #[test]
    fn test_field_without_alias() {
        // Should fail - no alias at all
        let query = "SELECT Name FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("псевдоним"));
    }

    #[test]
    fn test_asterisk_field() {
        // Should pass - asterisk doesn't need alias
        let query = "SELECT * FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_table_asterisk() {
        // Should pass - Table.* doesn't need alias
        let query = "SELECT Products.* FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_multiple_fields_mixed() {
        // Mixed: some with AS, some without
        let query = "SELECT Name AS ProductName, Code ProductCode, Price FROM Products";
        let diagnostics = check_standalone_query(query);
        // Should have 2 errors: Code (implicit) and Price (no alias)
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn test_russian_kak_keyword() {
        // Russian КАК keyword should work
        let query = "ВЫБРАТЬ Имя КАК ИмяПродукта ИЗ Товары";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_union_query() {
        // UNION query - first query checked, UNION query not checked (per Java impl)
        let query = "SELECT Name AS N FROM Products UNION SELECT Title FROM Services";
        let diagnostics = check_standalone_query(query);
        // First query OK (Name AS N), second query (Title without alias) not checked
        // Because we only check main query, not UNION queries
        // NOTE: This matches Java implementation behavior
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_multiline_string_extraction() {
        use ide_db::base_db::{RootQueryDb, SourceDatabase};
        use ide_db::RootDatabaseImpl;
        use test_fixture::Fixture;

        let code = r#"Процедура Тест()
Query = "ВЫБРАТЬ
	|  Ссылка
	|ИЗ
	|  Справочник.Валюты";
КонецПроцедуры"#;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let parse = db.parse(file_id);
        let root = parse.syntax_node();

        // Print tree structure for debugging
        println!("Tree:");
        println!("{:#?}", root);

        // Find LITERAL nodes
        let mut found_literal = false;
        for node in root.descendants() {
            println!("Node: {:?}", node.kind());
            if node.kind() == SyntaxKind::LITERAL {
                found_literal = true;
                if let Some(extracted) = extract_string_content(&node) {
                    println!("Extracted: {:?}", extracted);
                    assert!(extracted.contains("ВЫБРАТЬ"));
                    assert!(extracted.contains("Ссылка"));
                    assert!(extracted.contains("Справочник"));
                }
            }
        }

        assert!(found_literal, "Should have found a LITERAL node");
    }

    #[test]
    fn test_sdbl_russian_query() {
        // Test that SDBL parser handles Russian queries
        let query = "ВЫБРАТЬ Ссылка, Код КАК К ИЗ Справочник.Валюты";

        // Parse SDBL and print debug info
        let parse = parser::parse_sdbl(query);
        println!("Parse has errors: {}", parse.has_errors());
        println!("Parse tree:\n{:#?}", parse.syntax_node());

        let diagnostics = check_standalone_query(query);
        println!("Diagnostics for Russian query: {}", diagnostics.len());
        for d in &diagnostics {
            println!("  - {}", d.message);
        }
        // Should have 1 error: Ссылка without alias
        assert_eq!(diagnostics.len(), 1);
    }

    /// Test from Java: AssignAliasFieldsInQueryDiagnosticTest.java
    ///
    /// Expected 5 diagnostics:
    /// - Line 3, columns 3-16 (Валюты.Ссылка without alias)
    /// - Line 5, columns 3-17 (Валюты.Код Код - implicit alias)
    /// - Line 21, columns 3-16 (Валюты.Ссылка without alias - second query)
    /// - Line 23, columns 3-17 (Валюты.Код Код - implicit alias - second query)
    /// - Line 42, columns 4-17 (Валюты.Ссылка without alias - in subquery)
    #[test]
    fn test_java_diagnostic_compatibility() {
        // Load exact copy of Java test fixture
        let code = include_str!("../../test_data/AssignAliasFieldsInQueryDiagnostic.bsl");
        let config = DiagnosticsConfig::default();

        // Run diagnostic check
        let (diagnostics, file_content) = check_diagnostic(code, config);

        // Java test expects exactly 5 diagnostics at (0-indexed lines):
        // - Line 3, cols 3-16 (Валюты.Ссылка without alias)
        // - Line 5, cols 3-17 (Валюты.Код Код without AS)
        // - Line 21, cols 3-16 (Валюты.Ссылка without alias)
        // - Line 23, cols 3-17 (Валюты.Код Код without AS)
        // - Line 42, cols 4-17 (Валюты.Ссылка in subquery without alias)

        // Debug: print all diagnostics
        for (i, diag) in diagnostics.iter().enumerate() {
            let (start_line, start_col, _end_line, end_col) =
                range_to_line_col(&file_content, diag.range);
            eprintln!(
                "Diagnostic {}: line {}, cols {}-{}, message: {}",
                i, start_line, start_col, end_col, diag.message
            );
        }

        assert_eq!(diagnostics.len(), 5, "Expected 5 diagnostics to match Java implementation");

        // Verify exact positions match Java test expectations
        assert_diagnostic_range(&file_content, &diagnostics[0], 3, 3, 16); // Валюты.Ссылка
        assert_diagnostic_range(&file_content, &diagnostics[1], 5, 3, 17); // Валюты.Код Код
        assert_diagnostic_range(&file_content, &diagnostics[2], 21, 3, 16); // Second query
        assert_diagnostic_range(&file_content, &diagnostics[3], 23, 3, 17); // Second query
        assert_diagnostic_range(&file_content, &diagnostics[4], 42, 4, 17); // Nested subquery
    }
}
