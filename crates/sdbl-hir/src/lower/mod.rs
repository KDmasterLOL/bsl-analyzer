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

use syntax::ast::{AstNode, SdblQueryPackage, SdblSelectQuery};
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
    metadata: Option<std::sync::Arc<Configuration>>,
) -> crate::hir::SdblPackage {
    let _span = tracing::debug_span!("lower_sdbl_to_hir").entered();

    let root = sdbl_ast.syntax_node();

    // Try to cast root as query package
    let Some(package) = SdblQueryPackage::cast(root) else {
        tracing::debug!("Failed to cast root as SdblQueryPackage");
        return crate::hir::SdblPackage::empty();
    };

    // Create lowering context (Arc avoids cloning the large Configuration)
    let mut ctx = LoweringContext::new(metadata);

    let mut sdbl_queries = Vec::new();
    let mut select_index = 0;
    let mut has_query_item = false;

    for query_item in package.syntax().children() {
        match query_item.kind() {
            syntax::SyntaxKind::SDBL_SELECT_QUERY => {
                has_query_item = true;
            }
            syntax::SyntaxKind::SDBL_DROP_QUERY => {
                has_query_item = true;
                ctx.lower_drop_query(&query_item);
                continue;
            }
            _ => continue,
        }

        let Some(select_query) = SdblSelectQuery::cast(query_item) else {
            continue;
        };

        // Get subquery which contains main query + UNION queries
        let Some(subquery) = select_query.subquery() else {
            tracing::debug!(select_index, "No subquery in SdblSelectQuery");
            select_index += 1;
            continue;
        };

        // Determine if there are UNION siblings
        let has_union_siblings = subquery.union_clauses().next().is_some();

        // Lower main query first
        if let Some(main_query) = subquery.main_query() {
            // Push scope frame for main query (clears FROM/JOIN scope, keeps temp tables)
            ctx.scope.push_frame();

            let query_hir = ctx.lower_query(&main_query, has_union_siblings, true);
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
            } else {
                let union_range = union_clause
                    .syntax()
                    .children_with_tokens()
                    .filter_map(|it| it.into_token())
                    .find(|t| {
                        if t.kind() == syntax::SyntaxKind::IDENT {
                            let text = t.text();
                            text.eq_ignore_ascii_case("UNION")
                                || text.eq_ignore_ascii_case("ОБЪЕДИНИТЬ")
                        } else {
                            false
                        }
                    })
                    .map(|t| t.text_range())
                    .unwrap_or_else(|| union_clause.syntax().text_range());

                ctx.diagnostics.push(crate::diagnostics::SdblDiagnostic::UnionWithoutAll {
                    range: union_range,
                });
            }

            // Lower the UNION query
            if let Some(union_query) = union_clause.query() {
                // Push scope frame for UNION query (clears FROM/JOIN scope, keeps temp tables)
                ctx.scope.push_frame();

                // UNION queries always have union siblings (by definition)
                let query_hir = ctx.lower_query(&union_query, true, false);
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

        select_index += 1;
    }

    if !has_query_item {
        tracing::debug!("No queries in package");
        return crate::hir::SdblPackage::empty();
    }

    // Finalize source map (sort token lists)
    ctx.source_map.finalize();

    // Return package with all queries
    crate::hir::SdblPackage { queries: sdbl_queries, source_map: ctx.source_map }
}

impl LoweringContext {
    /// Lower a single SDBL query (main query or query from UNION).
    ///
    /// This method processes one query from a SELECT statement, which can be:
    /// - The main query before UNION
    /// - A query after UNION/UNION ALL
    ///
    /// # Arguments
    /// * `query` - The query AST node to lower
    /// * `has_union_siblings` - Whether this query has UNION siblings (for SelectTopWithoutOrderBy)
    /// * `check_field_aliases` - Whether result field aliases are semantically relevant
    pub(crate) fn lower_query(
        &mut self,
        query: &syntax::ast::SdblQuery,
        has_union_siblings: bool,
        check_field_aliases: bool,
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

        // 3. Lower JOINs (tables are added to scope inside lower_join_clause,
        //    before ON conditions are processed, so fields from joined tables resolve)
        let joins = self.lower_joins(query);

        // 4. Extract DISTINCT and TOP from limitations
        let (distinct, top, top_range) = self.extract_limitations(query.syntax());

        // 5. Lower SELECT clause (uses scope for name resolution).
        // Alias-without-AS diagnostics are emitted for result-shaping SELECTs.
        // In a UNION chain, result column names are defined by the first query,
        // so secondary UNION branches do not require field aliases.
        let select = self.lower_field_list(query.field_list(), distinct, top);

        // 6. Lower WHERE clause
        let where_clause = query.where_clause().map(|w| self.lower_where_clause(&w));

        // 7. Lower GROUP BY clause
        let group_by = query.group_by_clause().map(|g| self.lower_group_by(&g));

        // 8. Lower ORDER BY clause
        let order_by = query.order_by_clause().map(|o| self.lower_order_by(&o));

        // 9. Record INDEX BY clause tokens and selected output references.
        self.lower_index_by_clause(query.syntax(), &select);

        // 10. Lower INTO clause (temporary table)
        let into_table = self.lower_into_clause(query.syntax());

        // 11. Check SELECT TOP without ORDER BY
        if let (Some(top_value), Some(range)) = (top, top_range) {
            let has_order_by = order_by.is_some();
            let has_where = where_clause.is_some();

            // Always emit diagnostic if:
            // - This is part of UNION (TOP in UNION is always problematic)
            // - OR there's no ORDER BY clause
            if has_union_siblings || !has_order_by {
                self.diagnostics.push(
                    crate::diagnostics::SdblDiagnostic::SelectTopWithoutOrderBy {
                        top_value,
                        in_union: has_union_siblings,
                        has_where,
                        range,
                    },
                );
            }
        }

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

        // 12. Check JOINs for unprotected fields (after complete HIR built)
        self.check_joins_for_unprotected_fields(&hir);

        // 13. Check SELECT fields for missing AS keyword (after complete HIR built)
        if check_field_aliases {
            self.check_alias_without_as_keyword(&hir);
        }

        // 14. Check for nested field dereference by dot (N+1 query problem)
        self.check_nested_fields_by_dot(&hir);

        // 15. Check for redundant .Ссылка (Reference) field access
        self.check_ref_overuse(&hir);

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
