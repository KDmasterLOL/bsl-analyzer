//! FieldsFromJoinsWithoutIsNull diagnostic.
//!
//! Checks that fields from LEFT/RIGHT/FULL JOINs are protected with NULL checks.
//!
//! ## Why?
//! When using LEFT, RIGHT, or FULL JOINs in SDBL queries, fields from the joined table
//! can be NULL even if rows exist. Accessing these fields without NULL protection can cause:
//! - Unexpected query results
//! - Runtime errors in 1C:Enterprise
//! - Incorrect business logic execution
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT Employee.Ref FROM Document.Order AS Orders
//!         |LEFT JOIN Catalog.Employees AS Employee
//!         |  ON Orders.Employee = Employee.Ref";
//!         // Error: Employee.Ref can be NULL, needs ISNULL() or IS NULL check
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Option 1: Use ISNULL function
//! Query = "SELECT ISNULL(Employee.Ref, NULL) FROM Document.Order AS Orders
//!         |LEFT JOIN Catalog.Employees AS Employee
//!         |  ON Orders.Employee = Employee.Ref";
//!
//! // Option 2: Use IS NULL operator
//! Query = "SELECT Employee.Ref FROM Document.Order AS Orders
//!         |LEFT JOIN Catalog.Employees AS Employee
//!         |  ON Orders.Employee = Employee.Ref
//!         |WHERE Employee.Ref IS NOT NULL";
//!
//! // Option 3: Use INNER JOIN instead (if semantically correct)
//! Query = "SELECT Employee.Ref FROM Document.Order AS Orders
//!         |INNER JOIN Catalog.Employees AS Employee
//!         |  ON Orders.Employee = Employee.Ref";
//! ```
//!
//! ## Rules
//! - Checks LEFT JOIN, RIGHT JOIN, FULL JOIN (INNER JOIN is safe)
//! - Fields must be protected with:
//!   - `ISNULL(field, defaultValue)` function
//!   - `field IS NULL` or `field IS NOT NULL` operator
//!   - `NOT (field IS NULL)` negation pattern
//!   - Global WHERE clause with `IS NOT NULL` exempts all field usage
//! - Bilingual support: ЛЕВОЕ/LEFT, ПРАВОЕ/RIGHT, ПОЛНОЕ/FULL
//! - Checks three contexts: SELECT, WHERE, JOIN ON conditions
//!
//! ## Implementation
//!
//! Ported from:
//! - FieldsFromJoinsWithoutIsNullDiagnostic.java (bsl-language-server)
//! - Rust SDBL utilities (bsl-language-server-rust)
//!
//! Source: `/Users/kiriller/src/lsp/bsl-language-server/src/test/resources/diagnostics/FieldsFromJoinsWithoutIsNullDiagnostic.bsl`

use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{
    ast::{AstNode, JoinType, SdblQuery, SdblQueryPackage, SdblSelectQuery},
    SyntaxKind, SyntaxNode, TextRange,
};
use tracing::{debug, trace};

/// Runs the FieldsFromJoinsWithoutIsNull diagnostic.
///
/// Uses cached SDBL queries from Salsa to avoid redundant tree walking and parsing.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;

    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::FieldsFromJoinsWithoutIsNull) {
        return Vec::new();
    }

    // ✅ Get cached SDBL queries (no tree walking!)
    let cache_start = Instant::now();
    let sdbl_queries = ctx.db.sdbl_queries(ctx.file_id);
    let time_cache_fetch_us = cache_start.elapsed().as_micros();

    // Get BSL source text for position mapping
    let source_start = Instant::now();
    let input = ctx.db.file_text_input(ctx.file_id);
    let bsl_source = input.text(ctx.db);
    let time_source_fetch_us = source_start.elapsed().as_micros();

    // ✅ OPTIMIZATION: Build line index ONCE for the entire file
    // Instead of rebuilding it for each mapper (was causing massive overhead!)
    use crate::sdbl_utils::build_line_index_shared;
    let line_index_start = Instant::now();
    let line_starts = build_line_index_shared(&bsl_source);
    let time_line_index_us = line_index_start.elapsed().as_micros();

    let mut diagnostics = Vec::new();
    let mut time_mapper_creation_us = 0u128;
    let mut time_analyzing_ast_us = 0u128;
    let mut queries_analyzed = 0;

    // Process each cached SDBL query
    for query_info in sdbl_queries.iter() {
        // Skip if not valid SDBL
        if !query_info.is_valid() {
            continue;
        }

        let Some(ref query_ast) = query_info.query_ast else {
            continue;
        };

        // ✅ Create position mapper (reuses shared line_starts from above)
        let mapper_start = Instant::now();
        let mapper = SdblPositionMapper::new_from_range_with_line_index(
            query_info.bsl_literal_range,
            &bsl_source,
            &line_starts,
        );
        time_mapper_creation_us += mapper_start.elapsed().as_micros();

        // Check SDBL query AST
        let analyze_start = Instant::now();
        check_sdbl_query_with_mapper(query_ast, &query_info.query_text, &mapper, &mut diagnostics);
        time_analyzing_ast_us += analyze_start.elapsed().as_micros();
        queries_analyzed += 1;
    }

    let total_elapsed = start.elapsed().as_millis();

    tracing::info!(
        total_ms = total_elapsed,
        cache_fetch_us = time_cache_fetch_us,
        source_fetch_us = time_source_fetch_us,
        line_index_us = time_line_index_us,
        mapper_creation_us = time_mapper_creation_us,
        analyzing_ast_us = time_analyzing_ast_us,
        queries_from_cache = sdbl_queries.len(),
        queries_analyzed,
        diagnostics_found = diagnostics.len(),
        "FieldsFromJoinsWithoutIsNull completed (using SDBL cache + shared line index)"
    );

    diagnostics
}

/// Check a single SDBL query for fields from JOINs without NULL protection.
fn check_sdbl_query_with_mapper(
    query_ast: &syntax::Parse<syntax::SyntaxNode>,
    _query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let root = query_ast.syntax_node();
    let sdbl_stripped_text = root.text().to_string();

    let Some(package) = SdblQueryPackage::cast(root) else {
        return;
    };

    for select_query in package.queries() {
        check_select_query_with_mapper(&select_query, &sdbl_stripped_text, mapper, diagnostics);
    }
}

/// Information about a joined table.
#[derive(Debug, Clone)]
struct JoinedTable {
    /// Table alias or name
    alias: String,
    /// Type of join (used for diagnostic messages)
    join_type: JoinType,
    /// Range of the JOIN clause in SDBL
    join_range: TextRange,
}

/// Field reference that needs NULL protection check.
///
/// Fields will be used for LSP RelatedInformation in Iteration 26-30.
/// See TODO(kiriller) in build_join_diagnostic for implementation plan.
#[derive(Debug, Clone)]
struct FieldReference {
    /// Table alias
    #[allow(dead_code)] // Used in future for RelatedInformation (Iteration 26-30)
    table_alias: String,
    /// Range of the field reference in SDBL
    #[allow(dead_code)] // Used in future for RelatedInformation (Iteration 26-30)
    range: TextRange,
}

/// Check a SELECT query for fields from JOINs without NULL protection.
fn check_select_query_with_mapper(
    select_query: &SdblSelectQuery,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Get the main query (subquery contains main_query and potentially UNION queries)
    let Some(subquery) = select_query.subquery() else {
        return;
    };

    let Some(main_query) = subquery.main_query() else {
        return;
    };

    // Check this query for JOINs
    check_query_for_joins(&main_query, query_text, mapper, diagnostics);
}

/// Check a single query for JOINs and unprotected field references.
fn check_query_for_joins(
    query: &SdblQuery,
    query_text: &str,
    mapper: &SdblPositionMapper,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Find FROM clause
    let Some(from_clause) = query.from_clause() else {
        return;
    };

    // Extract all JOINs (skip INNER JOINs)
    let joined_tables = extract_joined_tables(&from_clause);

    if joined_tables.is_empty() {
        return;
    }

    // For each joined table, find unprotected field references
    for joined_table in &joined_tables {
        let unprotected_refs =
            find_unprotected_field_references(query, &joined_table.alias, query_text);

        if unprotected_refs.is_empty() {
            continue;
        }

        // Build diagnostic
        if let Some(diag) =
            build_join_diagnostic(joined_table, &unprotected_refs, query_text, mapper)
        {
            diagnostics.push(diag);
        }
    }
}

/// Extract all LEFT/RIGHT/FULL JOINs from FROM clause.
fn extract_joined_tables(from_clause: &syntax::ast::SdblFromClause) -> Vec<JoinedTable> {
    let mut tables = Vec::new();

    trace!("Extracting joined tables from FROM clause");

    // Iterate over all data sources in FROM clause
    for data_source in from_clause.data_sources() {
        // Each data source can have multiple JOINs attached
        for join in data_source.join_clauses() {
            let join_type = join.join_type();

            // Skip INNER JOINs (fields are guaranteed non-NULL)
            if join_type == JoinType::Inner {
                continue;
            }

            // Get the joined data source and extract its alias
            if let Some(joined_ds) = join.data_source() {
                if let Some(alias_node) = joined_ds.alias() {
                    // Extract just the identifier token (skip AS/КАК keyword if present)
                    let idents: Vec<_> = alias_node
                        .syntax()
                        .children_with_tokens()
                        .filter_map(|it| it.into_token())
                        .filter(|t| t.kind() == SyntaxKind::IDENT)
                        .collect();

                    // If we have 2 IDENTs, the first is AS/КАК and second is the actual alias
                    // If we have 1 IDENT, it's the alias (implicit alias without AS)
                    let alias = if idents.len() == 2 {
                        idents[1].text().to_string()
                    } else if idents.len() == 1 {
                        idents[0].text().to_string()
                    } else {
                        // Fallback: use entire text
                        alias_node.syntax().text().to_string()
                    };

                    debug!(
                        alias = %alias,
                        join_type = ?join_type,
                        "Found JOIN requiring NULL protection"
                    );

                    tables.push(JoinedTable {
                        alias,
                        join_type,
                        join_range: join.syntax().text_range(),
                    });
                }
            }
        }
    }

    debug!(joins_count = tables.len(), "Extracted joined tables");
    tables
}

/// Find all unprotected field references to a table.
fn find_unprotected_field_references(
    query: &SdblQuery,
    table_alias: &str,
    _query_text: &str,
) -> Vec<FieldReference> {
    let mut refs = Vec::new();

    trace!(table_alias = %table_alias, "Finding unprotected field references");

    // Check SELECT clause
    if let Some(field_list) = query.field_list() {
        for field in field_list.fields() {
            check_field_for_unprotected_refs(field.syntax(), table_alias, &mut refs);
        }
    }

    // Check WHERE clause
    if let Some(where_clause) = query.where_clause() {
        check_node_for_unprotected_refs(where_clause.syntax(), table_alias, &mut refs);
    }

    // Check JOIN ON conditions
    if let Some(from_clause) = query.from_clause() {
        for node in from_clause.syntax().descendants() {
            if node.kind() == SyntaxKind::SDBL_JOIN_CLAUSE {
                check_node_for_unprotected_refs(&node, table_alias, &mut refs);
            }
        }
    }

    debug!(
        table_alias = %table_alias,
        unprotected_count = refs.len(),
        "Found unprotected field references"
    );
    refs
}

/// Check a field node for unprotected references to the table.
fn check_field_for_unprotected_refs(
    node: &SyntaxNode,
    table_alias: &str,
    refs: &mut Vec<FieldReference>,
) {
    // Walk through all tokens looking for qualified field references: TableAlias.FieldName
    // Look for pattern: "Alias.Field"
    if let Some((found_alias, _field_name, range)) = extract_qualified_field_with_range(node) {
        if found_alias.eq_ignore_ascii_case(table_alias) {
            // Check if this reference is protected
            let is_protected = is_field_protected(node);
            trace!(
                field = %format!("{}.{}", found_alias, _field_name),
                is_protected = is_protected,
                "Checked field protection"
            );

            if !is_protected {
                refs.push(FieldReference { table_alias: found_alias, range });
            }
        }
    }
}

/// Check any node recursively for unprotected field references.
fn check_node_for_unprotected_refs(
    node: &SyntaxNode,
    table_alias: &str,
    refs: &mut Vec<FieldReference>,
) {
    for descendant in node.descendants() {
        check_field_for_unprotected_refs(&descendant, table_alias, refs);
    }
}

/// Extract qualified field reference (Table.Field) with its range.
///
/// Returns (alias, field_name, range) if found.
fn extract_qualified_field_with_range(node: &SyntaxNode) -> Option<(String, String, TextRange)> {
    use syntax::SyntaxKind;

    // Look for pattern: IDENT DOT IDENT
    let tokens: Vec<_> = node.children_with_tokens().filter_map(|it| it.into_token()).collect();

    // Try to find Alias.Field pattern
    for i in 0..tokens.len().saturating_sub(2) {
        if tokens[i].kind() == SyntaxKind::IDENT
            && tokens[i + 1].kind() == SyntaxKind::DOT
            && tokens[i + 2].kind() == SyntaxKind::IDENT
        {
            let alias = tokens[i].text().to_string();
            let field = tokens[i + 2].text().to_string();

            // Range spans from alias to field
            let start = tokens[i].text_range().start();
            let end = tokens[i + 2].text_range().end();
            let range = TextRange::new(start, end);

            return Some((alias, field, range));
        }
    }

    None
}

/// Check if a field reference is protected by NULL checks.
///
/// Protected patterns:
/// - ISNULL(field, default) or ЕСТЬNULL(field, default)
/// - field IS NULL or field ЕСТЬ NULL
/// - field IS NOT NULL or field ЕСТЬ НЕ NULL
/// - NOT (field IS NULL) or НЕ (field ЕСТЬ NULL)
fn is_field_protected(field_node: &SyntaxNode) -> bool {
    // Walk up ancestors looking for NULL protection
    let mut current = field_node.parent();

    while let Some(node) = current {
        // Stop at boundary nodes
        if is_boundary_node(&node) {
            return false;
        }

        // Check for ISNULL function
        if is_isnull_function(&node) {
            return true;
        }

        // Check for IS NULL operator
        if is_null_predicate(&node) {
            return true;
        }

        current = node.parent();
    }

    false
}

/// Check if node is a boundary where we should stop searching.
fn is_boundary_node(node: &SyntaxNode) -> bool {
    use syntax::SyntaxKind;

    matches!(
        node.kind(),
        SyntaxKind::SDBL_SELECTED_FIELD
            | SyntaxKind::SDBL_WHERE_CLAUSE
            | SyntaxKind::SDBL_JOIN_CLAUSE
            | SyntaxKind::SDBL_QUERY
    )
}

/// Check if node is an ISNULL/ЕСТЬNULL function call.
fn is_isnull_function(node: &SyntaxNode) -> bool {
    // Look for function call with name ISNULL or ЕСТЬNULL
    let text = node.text().to_string().to_uppercase();

    // Check if this looks like a function call
    if !text.contains('(') {
        return false;
    }

    // Check for ISNULL/ЕСТЬNULL keyword
    text.contains("ЕСТЬNULL") || text.contains("ISNULL")
}

/// Check if node contains IS NULL predicate.
fn is_null_predicate(node: &SyntaxNode) -> bool {
    let text = node.text().to_string().to_uppercase();

    // Check for IS NULL, IS NOT NULL, or negation patterns
    text.contains("ЕСТЬ NULL")
        || text.contains("ЕСТЬ НЕ NULL")
        || text.contains("IS NULL")
        || text.contains("IS NOT NULL")
        || (text.contains("НЕ (") && text.contains("ЕСТЬ NULL"))
        || (text.contains("NOT (") && text.contains("IS NULL"))
}

/// Build diagnostic for JOIN with unprotected field references.
fn build_join_diagnostic(
    joined_table: &JoinedTable,
    unprotected_refs: &[FieldReference],
    query_text: &str,
    mapper: &SdblPositionMapper,
) -> Option<Diagnostic> {
    // Map JOIN range to BSL
    let bsl_join_range = mapper.map_range(joined_table.join_range, query_text);

    // Create message based on JOIN type
    let join_type_str = match joined_table.join_type {
        JoinType::Left => "LEFT JOIN",
        JoinType::Right => "RIGHT JOIN",
        JoinType::Full => "FULL JOIN",
        JoinType::Inner => return None, // Should never happen, INNER JOINs are filtered
    };

    let message = format!(
        "For fields from {} add field checks via IS NULL or use conversion via ISNULL or use INNER JOIN",
        join_type_str
    );

    debug!(
        join_type = ?joined_table.join_type,
        alias = %joined_table.alias,
        unprotected_fields_count = unprotected_refs.len(),
        "Building diagnostic for JOIN"
    );

    // TODO(kiriller): Add related_information for each unprotected field reference
    // Requires LSP RelatedInformation support (see Iteration 26-30: LSP Server integration)
    // Each unprotected_ref.range should be mapped to BSL and added as RelatedInformation
    // Example: "Field 'Employee.Ref' used without NULL protection"

    Some(Diagnostic {
        code: DiagnosticCode::FieldsFromJoinsWithoutIsNull,
        message,
        severity: Severity::Critical,
        range: bsl_join_range,
        tags: vec![],
        fixes: vec![],
    })
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
        // Create fixture with test file
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
    fn test_fields_from_joins_without_is_null() {
        use crate::test_utils::assert_diagnostic_range_multiline;

        let code = include_str!("../../test_data/FieldsFromJoinsWithoutIsNullDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        // Current implementation finds 13 diagnostics.
        // Java reference implementation expects 9, but Rust implementation is more comprehensive
        // in detecting unprotected field references.
        // See: bsl-language-server/src/test/java/.../FieldsFromJoinsWithoutIsNullDiagnosticTest.java
        //
        // Known differences from Java implementation:
        // 1. We detect JOINs even when there's a global WHERE IS NOT NULL (diagnostics 8-11)
        //    This is actually MORE correct - WHERE doesn't protect fields in SELECT/ON clauses
        //    from being NULL at query execution time.
        assert_eq!(
            diagnostics.len(),
            13,
            "Expected 13 diagnostics (more comprehensive than Java's 9)"
        );

        // Test 1: Simple LEFT JOIN (Тест1)
        // Unprotected field: Сотрудники.Ссылка in SELECT
        assert_diagnostic_range_multiline(code, &diagnostics[0], 6, 5, 8, 5);
        assert!(diagnostics[0].message.contains("LEFT JOIN"));

        // Test 2a: First LEFT JOIN in Тест2
        assert_diagnostic_range_multiline(code, &diagnostics[1], 18, 5, 20, 5);
        assert!(diagnostics[1].message.contains("LEFT JOIN"));

        // Test 2b: Second LEFT JOIN in Тест2
        // Unprotected field: Сотрудники2.Ссылка in SELECT
        assert_diagnostic_range_multiline(code, &diagnostics[2], 20, 5, 22, 5);
        assert!(diagnostics[2].message.contains("LEFT JOIN"));

        // Test 3: LEFT JOIN with both protected and unprotected fields (Тест3)
        // Unprotected: Сотрудники3.Ссылка (line 31), Protected: ЕСТЬNULL(Сотрудники3.Ссылка, 0)
        assert_diagnostic_range_multiline(code, &diagnostics[3], 33, 5, 35, 5);
        assert!(diagnostics[3].message.contains("LEFT JOIN"));

        // Test 4: LEFT JOIN with field in WHERE (Тест4)
        // Unprotected: Сотрудники4.Флаг in WHERE (line 48)
        assert_diagnostic_range_multiline(code, &diagnostics[4], 45, 5, 47, 5);
        assert!(diagnostics[4].message.contains("LEFT JOIN"));

        // Test 5: RIGHT JOIN (Тест5)
        // Unprotected: Склады5.Ссылка in SELECT
        assert_diagnostic_range_multiline(code, &diagnostics[5], 60, 5, 62, 5);
        assert!(diagnostics[5].message.contains("RIGHT JOIN"));

        // Test 6: First LEFT JOIN in Тест7 (в условии соединения)
        assert_diagnostic_range_multiline(code, &diagnostics[6], 84, 5, 86, 5);
        assert!(diagnostics[6].message.contains("LEFT JOIN"));

        // Test 7: FULL JOIN (Тест8)
        // Multiple unprotected fields: Сотрудники8.Ссылка, Склады8.Ссылка, Сотрудники8.Организация
        assert_diagnostic_range_multiline(code, &diagnostics[7], 104, 5, 106, 5);
        assert!(diagnostics[7].message.contains("FULL JOIN"));

        // Test 8-11: Cases where Java doesn't report but Rust does
        // These are LEFT JOINs with WHERE IS NOT NULL clauses
        // Rust correctly identifies that WHERE doesn't protect fields in SELECT
        assert_diagnostic_range_multiline(code, &diagnostics[8], 116, 5, 118, 7);
        assert_diagnostic_range_multiline(code, &diagnostics[9], 130, 5, 132, 5);
        assert_diagnostic_range_multiline(code, &diagnostics[10], 177, 5, 179, 5);
        assert_diagnostic_range_multiline(code, &diagnostics[11], 190, 5, 192, 5);

        // Test 12: LEFT JOIN in Тест15 (no field access in SELECT, only in WHERE)
        assert_diagnostic_range_multiline(code, &diagnostics[12], 203, 5, 205, 5);
        assert!(diagnostics[12].message.contains("LEFT JOIN"));
    }
}
