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
/// `SdblLowerResult` containing HIR with resolved types, collected diagnostics,
/// and source mapping for semantic highlighting.
///
/// # Example
///
/// ```ignore
/// let sdbl_ast = parser::parse_sdbl("SELECT Код FROM Справочник.Валюты");
/// let metadata = load_configuration()?;
/// let result = lower_sdbl_to_hir(&sdbl_ast, Some(&metadata));
///
/// for diag in &result.hir.diagnostics {
///     println!("Error: {}", diag.message());
/// }
///
/// // Use source map for semantic highlighting
/// for (token, category) in result.source_map.all_tokens() {
///     println!("Token: {} at {:?}", token.text, token.range);
/// }
/// ```
pub fn lower_sdbl_to_hir(
    sdbl_ast: &Parse<syntax::SyntaxNode>,
    metadata: Option<&Configuration>,
) -> SdblLowerResult {
    let _span = tracing::info_span!("lower_sdbl_to_hir").entered();

    let root = sdbl_ast.syntax_node();

    // DEBUG: Log the actual text being lowered
    let text = root.text().to_string();
    let text_preview: String = text.chars().take(200).collect();
    tracing::info!(
        text_len = text.len(),
        text_preview = %text_preview,
        "lower_sdbl_to_hir: starting with AST text"
    );

    // Try to cast root as query package
    let Some(package) = SdblQueryPackage::cast(root) else {
        tracing::debug!("Failed to cast root as SdblQueryPackage");
        return SdblLowerResult { hir: SdblHir::empty(), source_map: SdblSourceMap::new() };
    };

    // Create lowering context
    let mut ctx = LoweringContext::new(metadata);

    // Lower ALL SELECT queries in the package
    let mut queries = package.queries();
    let Some(first_query) = queries.next() else {
        tracing::debug!("No queries in package");
        return SdblLowerResult { hir: SdblHir::empty(), source_map: SdblSourceMap::new() };
    };

    // Lower first query
    let mut hir = ctx.lower_select_query(&first_query);

    // Lower remaining queries and merge diagnostics
    for select_query in queries {
        let additional = ctx.lower_select_query(&select_query);
        hir.diagnostics.extend(additional.diagnostics);
    }

    // Finalize source map (sort token lists)
    ctx.source_map.finalize();

    SdblLowerResult { hir, source_map: ctx.source_map }
}

impl LoweringContext<'_> {
    /// Lower a SELECT query.
    pub(crate) fn lower_select_query(&mut self, query: &syntax::ast::SdblSelectQuery) -> SdblHir {
        let Some(subquery) = query.subquery() else {
            return SdblHir::empty();
        };

        let Some(main_query) = subquery.main_query() else {
            return SdblHir::empty();
        };

        // Record SELECT keyword
        self.record_keyword_by_text(
            main_query.syntax(),
            "SELECT",
            "ВЫБРАТЬ",
            crate::source_map::TokenCategory::ClauseKeyword,
        );

        // 1. Lower FROM clause first (establishes scope)
        let from = self.lower_from_clause(main_query.from_clause());

        // 2. Register tables in scope
        for table in &from {
            self.scope.add_table(table.clone());
        }

        // 3. Lower JOINs
        let joins = self.lower_joins(&main_query);
        for join in &joins {
            self.scope.add_table(join.table.clone());
        }

        // 4. Extract DISTINCT and TOP from limitations
        let (distinct, top) = self.extract_limitations(main_query.syntax());

        // 5. Lower SELECT clause (uses scope for name resolution)
        let select = self.lower_field_list(main_query.field_list(), distinct, top);

        // 6. Lower WHERE clause
        let where_clause = main_query.where_clause().map(|w| self.lower_where_clause(&w));

        // 7. Lower GROUP BY clause
        let group_by = main_query.group_by_clause().map(|g| self.lower_group_by(&g));

        // 8. Lower ORDER BY clause
        let order_by = main_query.order_by_clause().map(|o| self.lower_order_by(&o));

        // 9. Lower UNION queries
        let unions = self.lower_union_clauses(&subquery);

        // Collect diagnostics from UNION queries before moving them
        let mut union_diagnostics = Vec::new();
        for union_hir in &unions {
            union_diagnostics.extend(union_hir.query.diagnostics.clone());
        }

        let range = query.syntax().text_range();

        // Build HIR
        let mut hir = SdblHir {
            select,
            from,
            joins,
            where_clause,
            group_by,
            having: None,
            order_by,
            unions,
            diagnostics: std::mem::take(&mut self.diagnostics),
            range,
        };

        // 7. Merge diagnostics from UNION queries
        hir.diagnostics.extend(union_diagnostics);

        // 8. Check JOINs for unprotected fields (after complete HIR built)
        self.check_joins_for_unprotected_fields(&hir);

        // Merge diagnostics collected during JOIN checking
        hir.diagnostics.extend(std::mem::take(&mut self.diagnostics));

        hir
    }
}
