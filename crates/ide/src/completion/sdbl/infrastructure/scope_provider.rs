//! Database-backed scope provider.

use crate::completion::sdbl::domain::ScopeProvider;
use ide_db::RootDatabase;
use sdbl_hir::Scope;
use syntax::TextSize;
use vfs::FileId;

/// Scope provider that builds SDBL Scope from RootDatabase.
pub struct DbScopeProvider<'a> {
    db: &'a dyn RootDatabase,
}

impl<'a> DbScopeProvider<'a> {
    /// Create a new provider for given database.
    pub fn new(db: &'a dyn RootDatabase) -> Self {
        Self { db }
    }
}

impl ScopeProvider for DbScopeProvider<'_> {
    fn get_scope(&self, file_id: FileId, bsl_offset: TextSize) -> Option<Scope> {
        let _span = tracing::info_span!(
            "DbScopeProvider::get_scope",
            ?file_id,
            bsl_offset = u32::from(bsl_offset)
        )
        .entered();

        // 1. Get all SDBL queries from HIR to find ExprId by BSL range
        let all_sdbl = self.db.all_sdbl_in_file(file_id);

        tracing::info!(
            total_queries = all_sdbl.len(),
            bsl_offset = u32::from(bsl_offset),
            "searching for query containing cursor"
        );

        // 2. Find the ExprId and QueryInfo of the query containing the cursor
        let (target_expr_id, query_info) = all_sdbl.iter().find_map(|(expr_id, qinfo)| {
            let contains = qinfo.bsl_literal_range.contains(bsl_offset);
            tracing::debug!(
                expr_id = ?expr_id,
                bsl_range_start = u32::from(qinfo.bsl_literal_range.start()),
                bsl_range_end = u32::from(qinfo.bsl_literal_range.end()),
                bsl_offset = u32::from(bsl_offset),
                contains = contains,
                "checking if query BSL range contains cursor offset"
            );
            if contains {
                Some((*expr_id, qinfo.clone()))
            } else {
                None
            }
        })?;

        tracing::info!(
            target_expr_id = ?target_expr_id,
            "found target query by BSL offset"
        );

        // 3. Get lowered HIR through Salsa query (CACHED!)
        let sdbl_hirs = self.db.sdbl_hir_in_file(file_id);

        tracing::debug!(sdbl_hirs_count = sdbl_hirs.len(), "retrieved SDBL HIRs from cache");

        // 4. Find the package for the target ExprId
        let (_expr_id, sdbl_package) =
            sdbl_hirs.iter().find(|(expr_id, _)| *expr_id == target_expr_id)?;

        // 5. Find query containing cursor using SDBL-space offset
        let offset_in_query = bsl_offset - query_info.bsl_literal_range.start();
        let target_query = sdbl_package.query_at_offset(offset_in_query)?;

        tracing::info!(
            query_range = ?target_query.range,
            from_tables_count = target_query.hir.from.len(),
            join_tables_count = target_query.hir.joins.len(),
            "found target query for completion"
        );

        // 6. Rebuild Scope from HIR
        let mut scope = Scope::new();
        let hir = &target_query.hir;

        // Add tables from FROM clause
        for table in &hir.from {
            scope.add_table(table.clone());
        }

        // Add tables from JOIN clauses
        for join in &hir.joins {
            scope.add_table(join.table.clone());
        }

        tracing::info!(
            from_tables = hir.from.len(),
            join_tables = hir.joins.len(),
            "built Scope from HIR"
        );

        Some(scope)
    }
}
