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

        // НОВОЕ: Добавить временные таблицы из всех предыдущих queries в батче
        // Это нужно для completion во втором+ запросе батча, который использует
        // временную таблицу, созданную в предыдущем запросе.

        // DIAGNOSTIC: Log all queries before processing
        let all_queries = sdbl_package.queries();
        tracing::info!(
            total_queries_in_package = all_queries.len(),
            target_query_range = ?target_query.range,
            "DIAGNOSTIC: Starting temp table extraction from previous queries"
        );

        for (idx, prev_query) in all_queries.iter().enumerate() {
            tracing::info!(
                query_idx = idx,
                query_range = ?prev_query.range,
                has_into_table = prev_query.hir.into_table.is_some(),
                into_table_name = ?prev_query.hir.into_table.as_ref().map(|n| n.as_str()),
                select_fields_count = prev_query.hir.select.fields.len(),
                is_current_query = prev_query.range == target_query.range,
                "DIAGNOSTIC: Examining query in batch"
            );

            // Остановиться на текущем query (не включать его INTO clause)
            if prev_query.range == target_query.range {
                tracing::info!("DIAGNOSTIC: Reached current query, stopping temp table extraction");
                break;
            }

            // Если предыдущий query создал временную таблицу (INTO clause)
            if let Some(ref temp_name) = prev_query.hir.into_table {
                // DIAGNOSTIC: Log details of each SELECT field
                for (field_idx, field) in prev_query.hir.select.fields.iter().enumerate() {
                    tracing::info!(
                        field_idx = field_idx,
                        has_alias = field.alias.is_some(),
                        alias = ?field.alias.as_ref().map(|a| a.as_str()),
                        has_raw_name = field.raw_name.is_some(),
                        raw_name = ?field.raw_name.as_ref().map(|n| n.as_str()),
                        expr_variant = ?std::mem::discriminant(&field.expr),
                        column_name = ?field.expr.column_name().map(|n| n.as_str()),
                        alias_or_name = ?field.alias_or_name().map(|n| n.as_str()),
                        field_type = ?field.ty,
                        "DIAGNOSTIC: Examining SELECT field"
                    );
                }

                // Извлечь поля из SELECT clause предыдущего query
                let temp_fields: Vec<sdbl_hir::FieldDef> = prev_query
                    .hir
                    .select
                    .fields
                    .iter()
                    .filter_map(|f| {
                        f.alias_or_name()
                            .map(|name| sdbl_hir::FieldDef::new(name.as_str(), f.ty.clone()))
                    })
                    .collect();

                tracing::info!(
                    temp_table = %temp_name,
                    fields_count = temp_fields.len(),
                    field_names = ?temp_fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
                    field_types = ?temp_fields.iter().map(|f| format!("{:?}", f.ty)).collect::<Vec<_>>(),
                    "DIAGNOSTIC: adding temporary table from previous query in batch"
                );

                tracing::info!(
                    total_select_fields = prev_query.hir.select.fields.len(),
                    "DIAGNOSTIC: SELECT clause in previous query has {} fields",
                    prev_query.hir.select.fields.len()
                );

                scope.add_temp_table(temp_name.to_string(), temp_fields.clone());

                // IMPORTANT: Also add as TableRef for completion to find it
                // Completion uses find_table() which searches in 'tables', not 'temp_tables'
                let temp_table_ref = sdbl_hir::TableRef {
                    parts: vec![temp_name.clone()],
                    full_name: temp_name.to_string(),
                    alias: None,
                    metadata: Some(sdbl_hir::ResolvedTable::TempTable {
                        name: temp_name.to_string(),
                        fields: temp_fields,
                    }),
                    is_virtual_table: false,
                    virtual_table_params: Vec::new(),
                    range: syntax::TextRange::default(),
                };
                scope.add_table(temp_table_ref);
            }
        }

        // Добавить таблицы из текущего query (FROM + JOINs)
        let hir = &target_query.hir;

        for table in &hir.from {
            tracing::info!(
                table_name = %table.full_name,
                has_alias = table.alias.is_some(),
                alias = ?table.alias.as_ref().map(|a| a.as_str()),
                has_metadata = table.metadata.is_some(),
                is_temp_table = matches!(&table.metadata, Some(sdbl_hir::ResolvedTable::TempTable { .. })),
                metadata_fields_count = table.metadata.as_ref().map(|m| m.fields().len()).unwrap_or(0),
                "DIAGNOSTIC: adding table from FROM clause"
            );

            scope.add_table(table.clone());
        }

        for join in &hir.joins {
            tracing::info!(
                table_name = %join.table.full_name,
                "DIAGNOSTIC: adding table from JOIN clause"
            );

            scope.add_table(join.table.clone());
        }

        tracing::info!(
            from_tables = hir.from.len(),
            join_tables = hir.joins.len(),
            "built Scope from HIR with temp tables from previous queries"
        );

        Some(scope)
    }
}
