//! SDBL AST to HIR lowering.
//!
//! Transforms SDBL syntax trees into semantic HIR with:
//! - Type inference from metadata
//! - Name resolution (tables, fields, aliases)
//! - Semantic diagnostics collection

mod clauses;
mod context;
mod diagnostics;
mod expr;
mod from_clause;
mod join_clause;
mod select_fields;
#[cfg(test)]
mod tests;
mod union;

use syntax::ast::{AstNode, SdblQueryPackage};
use syntax::Parse;

use crate::hir::SdblHir;
use crate::source_map::SdblSourceMap;

pub use context::LoweringContext;

use bsl_metadata::Configuration;

/// Result of SDBL lowering.
///
/// Contains both the HIR and source mapping for semantic highlighting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblLowerResult {
    /// The lowered HIR.
    pub hir: SdblHir,

    /// Source mapping for semantic highlighting.
    pub source_map: SdblSourceMap,
}

/// Lower SDBL AST to HIR with source mapping.
///
/// # Arguments
///
/// * `sdbl_ast` - Parsed SDBL syntax tree
/// * `metadata` - Optional 1C configuration metadata for table/field resolution
///
/// # Returns
///
/// `SdblPackage` containing HIR for all queries in the package,
/// collected diagnostics from all queries, and source mapping
/// for semantic highlighting.
///
/// # Example
///
/// ```ignore
/// let sdbl_ast = parser::parse_sdbl("SELECT ...; SELECT ...");
/// let metadata = load_configuration()?;
/// let package = lower_sdbl_to_hir(&sdbl_ast, Some(&metadata));
///
/// // Get diagnostics from all queries
/// for diag in package.all_diagnostics() {
///     println!("Error: {}", diag.message());
/// }
///
/// // Find query at cursor position
/// if let Some(query) = package.query_at_offset(cursor_offset) {
///     // Use query.hir for completion
/// }
///
/// // Use source map for semantic highlighting
/// for (token, category) in package.source_map.all_tokens() {
///     println!("Token: {} at {:?}", token.text, token.range);
/// }
/// ```
pub fn lower_sdbl_to_hir(
    sdbl_ast: &Parse<syntax::SyntaxNode>,
    metadata: Option<&Configuration>,
) -> crate::hir::SdblPackage {
    let _span = tracing::debug_span!("lower_sdbl_to_hir").entered();

    let root = sdbl_ast.syntax_node();

    // Try to cast root as query package
    let Some(package) = SdblQueryPackage::cast(root) else {
        tracing::debug!("Failed to cast root as SdblQueryPackage");
        return crate::hir::SdblPackage::empty();
    };

    // Create lowering context
    let mut ctx = LoweringContext::new(metadata);

    // Lower ALL SELECT queries in the package
    let all_queries: Vec<_> = package.queries().collect();

    if all_queries.is_empty() {
        tracing::debug!("No queries in package");
        return crate::hir::SdblPackage::empty();
    }

    tracing::debug!(query_count = all_queries.len(), "lower_sdbl_to_hir: found queries in package");

    // Lower ALL queries and track their ranges
    // Each SdblSelectQuery may contain multiple queries (main + UNION)
    let mut sdbl_queries = Vec::new();
    for (select_index, select_query) in all_queries.iter().enumerate() {
        // Get subquery which contains main query + UNION queries
        let Some(subquery) = select_query.subquery() else {
            tracing::debug!(select_index, "No subquery in SdblSelectQuery");
            continue;
        };

        // Lower main query first
        if let Some(main_query) = subquery.main_query() {
            // Push scope frame for main query (clears FROM/JOIN scope, keeps temp tables)
            ctx.scope.push_frame();

            let query_hir = ctx.lower_query(&main_query, false);
            // IMPORTANT: Use select_query.text_range() to include outer SELECT clause
            // when query has INTO clause (e.g., SELECT ... ПОМЕСТИТЬ ... ИЗ (subquery))
            let range = select_query.syntax().text_range();

            tracing::debug!(
                select_index,
                query_index = 0,
                from_tables = query_hir.from.len(),
                join_tables = query_hir.joins.len(),
                range = ?range,
                "lowered main query"
            );

            // Pop scope frame (removes FROM/JOIN tables, keeps temp tables in parent)
            ctx.scope.pop_frame();

            // Register temporary table if this query creates one
            if let Some(ref temp_name) = query_hir.into_table {
                // Extract fields from SELECT clause
                let temp_fields: Vec<crate::hir::FieldDef> = query_hir
                    .select
                    .fields
                    .iter()
                    .filter_map(|f| {
                        f.alias_or_name()
                            .map(|name| crate::hir::FieldDef::new(name.as_str(), f.ty.clone()))
                    })
                    .collect();

                tracing::debug!(
                    temp_table = %temp_name,
                    fields_count = temp_fields.len(),
                    "registering temporary table in scope for subsequent queries"
                );

                ctx.scope.add_temp_table(temp_name.to_string(), temp_fields);
            }

            sdbl_queries.push(crate::hir::SdblQuery { hir: query_hir, range });
        }

        // Then lower UNION queries with keyword recording
        for (union_index, union_clause) in subquery.union_clauses().enumerate() {
            // Record UNION keyword
            ctx.record_keyword_by_text(
                union_clause.syntax(),
                "UNION",
                "ОБЪЕДИНИТЬ",
                crate::source_map::TokenCategory::Modifier,
            );

            // Record ALL keyword if present
            if union_clause.has_all() {
                ctx.record_keyword_by_text(
                    union_clause.syntax(),
                    "ALL",
                    "ВСЕ",
                    crate::source_map::TokenCategory::Modifier,
                );
            }

            // Lower the UNION query
            if let Some(union_query) = union_clause.query() {
                // Push scope frame for UNION query (clears FROM/JOIN scope, keeps temp tables)
                ctx.scope.push_frame();

                let query_hir = ctx.lower_query(&union_query, true);
                let range = union_query.syntax().text_range();

                tracing::debug!(
                    select_index,
                    union_index,
                    from_tables = query_hir.from.len(),
                    join_tables = query_hir.joins.len(),
                    range = ?range,
                    "lowered UNION query"
                );

                // Pop scope frame
                ctx.scope.pop_frame();

                // Register temporary table if this query creates one
                if let Some(ref temp_name) = query_hir.into_table {
                    // Extract fields from SELECT clause
                    let temp_fields: Vec<crate::hir::FieldDef> = query_hir
                        .select
                        .fields
                        .iter()
                        .filter_map(|f| {
                            f.alias_or_name()
                                .map(|name| crate::hir::FieldDef::new(name.as_str(), f.ty.clone()))
                        })
                        .collect();

                    tracing::debug!(
                        temp_table = %temp_name,
                        fields_count = temp_fields.len(),
                        "registering temporary table in scope for subsequent queries"
                    );

                    ctx.scope.add_temp_table(temp_name.to_string(), temp_fields);
                }

                sdbl_queries.push(crate::hir::SdblQuery { hir: query_hir, range });
            }
        }
    }

    // Finalize source map (sort token lists)
    ctx.source_map.finalize();

    // Return package with all queries
    crate::hir::SdblPackage { queries: sdbl_queries, source_map: ctx.source_map }
}

impl LoweringContext<'_> {
    /// Lower a single SDBL query (main query or query from UNION).
    ///
    /// This method processes one query from a SELECT statement, which can be:
    /// - The main query before UNION
    /// - A query after UNION/UNION ALL
    ///
    /// # Arguments
    /// * `query` - The query AST node to lower
    /// * `is_union` - Whether this query is part of a UNION clause (skips alias diagnostics)
    pub(crate) fn lower_query(
        &mut self,
        query: &syntax::ast::SdblQuery,
        is_union: bool,
    ) -> SdblHir {
        // Record SELECT keyword
        self.record_keyword_by_text(
            query.syntax(),
            "SELECT",
            "ВЫБРАТЬ",
            crate::source_map::TokenCategory::ClauseKeyword,
        );

        // NOTE: Scope is managed by push_frame/pop_frame in the main loop.
        // Each query in UNION gets a fresh scope frame for FROM/JOIN tables,
        // but temporary tables from previous queries are preserved.

        // 1. Lower FROM clause first (establishes scope)
        let from = self.lower_from_clause(query.from_clause());

        // 2. Register tables in scope
        for table in &from {
            self.scope.add_table(table.clone());
        }

        // 3. Lower JOINs
        let joins = self.lower_joins(query);
        for join in &joins {
            self.scope.add_table(join.table.clone());
        }

        // 4. Extract DISTINCT and TOP from limitations
        let (distinct, top) = self.extract_limitations(query.syntax());

        // 5. Lower SELECT clause (uses scope for name resolution)
        // Skip alias diagnostics for UNION queries (check for errors is done per-field)
        let select = self.lower_field_list(query.field_list(), distinct, top, is_union);

        // 6. Lower WHERE clause
        let where_clause = query.where_clause().map(|w| self.lower_where_clause(&w));

        // 7. Lower GROUP BY clause
        let group_by = query.group_by_clause().map(|g| self.lower_group_by(&g));

        // 8. Lower ORDER BY clause
        let order_by = query.order_by_clause().map(|o| self.lower_order_by(&o));

        // 9. Lower INTO clause (temporary table)
        let into_table = self.lower_into_clause(query.syntax());

        let range = query.syntax().text_range();

        // Build HIR for this single query
        // NOTE: UNION queries are processed separately in the main loop
        let mut hir = SdblHir {
            select,
            into_table,
            from,
            joins,
            where_clause,
            group_by,
            having: None,
            order_by,
            unions: vec![], // UNION queries are now processed separately, not nested
            diagnostics: std::mem::take(&mut self.diagnostics),
            range,
        };

        // 10. Check JOINs for unprotected fields (after complete HIR built)
        self.check_joins_for_unprotected_fields(&hir);

        // 11. Check SELECT fields for missing AS keyword (after complete HIR built)
        self.check_alias_without_as_keyword(&hir, is_union);

        // 12. Check for nested field dereference by dot (N+1 query problem)
        self.check_nested_fields_by_dot(&hir);

        // Merge diagnostics collected during post-lowering checks
        hir.diagnostics.extend(std::mem::take(&mut self.diagnostics));

        hir
    }

    /// Lower INTO clause (for temporary tables).
    ///
    /// Extracts temporary table name from `INTO TemporaryTableName` clause.
    ///
    /// # Example
    /// ```sql
    /// SELECT Field1 INTO MyTempTable FROM Catalog.Products
    /// ```
    ///
    /// Returns: `Some("MyTempTable")`
    pub(super) fn lower_into_clause(
        &mut self,
        query_node: &syntax::SyntaxNode,
    ) -> Option<crate::hir::Name> {
        use syntax::SyntaxKind;

        // Find INTO_CLAUSE node
        let into_clause =
            query_node.children().find(|n| n.kind() == SyntaxKind::SDBL_INTO_CLAUSE)?;

        // Record INTO keyword
        self.record_keyword_by_text(
            &into_clause,
            "INTO",
            "ПОМЕСТИТЬ",
            crate::source_map::TokenCategory::ClauseKeyword,
        );

        // Find TEMP_TABLE_NAME node
        let temp_table_node =
            into_clause.children().find(|n| n.kind() == SyntaxKind::SDBL_TEMP_TABLE_NAME)?;

        // Extract table name from first identifier token
        let table_name = temp_table_node.children_with_tokens().find_map(|element| {
            element
                .as_token()
                .filter(|t| t.kind() == SyntaxKind::IDENT)
                .map(|t| t.text().to_string())
        })?;

        tracing::debug!(table_name = %table_name, "Extracted INTO clause temporary table name");

        Some(crate::hir::Name::from(table_name))
    }
}
