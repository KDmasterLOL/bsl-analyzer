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
    fn get_scope(
        &self,
        file_id: FileId,
        bsl_literal_range: syntax::TextRange,
        sdbl_offset: TextSize,
    ) -> Option<Scope> {
        let _span = tracing::info_span!(
            "DbScopeProvider::get_scope",
            ?file_id,
            bsl_literal_range = ?bsl_literal_range,
            sdbl_offset = u32::from(sdbl_offset)
        )
        .entered();

        // 1. Get all SDBL queries from HIR to find ExprId by BSL literal range
        let all_sdbl = self.db.all_sdbl_in_file(file_id);

        tracing::info!(
            total_queries = all_sdbl.len(),
            sdbl_offset = u32::from(sdbl_offset),
            "searching for query containing cursor"
        );

        // 2. Find the SdblExprId and QueryInfo by matching BSL literal range
        let (target_sdbl_expr_id, _query_info) =
            all_sdbl.iter().find_map(|(sdbl_expr_id, qinfo)| {
                let matches = qinfo.bsl_literal_range == bsl_literal_range;
                let range_len = u32::from(qinfo.bsl_literal_range.end())
                    - u32::from(qinfo.bsl_literal_range.start());
                tracing::info!(
                    sdbl_expr_id = ?sdbl_expr_id,
                    bsl_range_start = u32::from(qinfo.bsl_literal_range.start()),
                    bsl_range_end = u32::from(qinfo.bsl_literal_range.end()),
                    range_len = range_len,
                    matches = matches,
                    "DIAGNOSTIC: checking if query BSL range matches"
                );
                if matches {
                    Some((*sdbl_expr_id, qinfo.clone()))
                } else {
                    None
                }
            })?;

        tracing::info!(
            target_sdbl_expr_id = ?target_sdbl_expr_id,
            "found target query by BSL offset"
        );

        // 3. Get lowered HIR through Salsa query (CACHED!)
        let sdbl_hirs = self.db.sdbl_hir_in_file(file_id);

        tracing::info!(
            sdbl_hirs_count = sdbl_hirs.len(),
            "DIAGNOSTIC: retrieved SDBL HIRs from cache"
        );

        // Log all SdblExprIds and their package query counts
        for (sdbl_expr_id, package) in sdbl_hirs.iter() {
            tracing::info!(
                sdbl_expr_id = ?sdbl_expr_id,
                package_queries_count = package.queries().len(),
                is_target = *sdbl_expr_id == target_sdbl_expr_id,
                "DIAGNOSTIC: SDBL HIR package from cache"
            );
        }

        // 4. Find the package for the target SdblExprId
        let (_sdbl_expr_id, sdbl_package) =
            sdbl_hirs.iter().find(|(sdbl_expr_id, _)| *sdbl_expr_id == target_sdbl_expr_id)?;

        // 5. Find query containing cursor using SDBL offset (already converted from BSL)
        tracing::info!(
            sdbl_offset = u32::from(sdbl_offset),
            package_queries_count = sdbl_package.queries().len(),
            "DIAGNOSTIC: about to call query_at_offset"
        );

        // Log all query ranges in package
        for (idx, query) in sdbl_package.queries().iter().enumerate() {
            let in_range = query.range.start() <= sdbl_offset && sdbl_offset <= query.range.end();
            tracing::info!(
                query_idx = idx,
                query_range_start = u32::from(query.range.start()),
                query_range_end = u32::from(query.range.end()),
                query_range_len = u32::from(query.range.end()) - u32::from(query.range.start()),
                sdbl_offset = u32::from(sdbl_offset),
                in_range = in_range,
                "DIAGNOSTIC: query range in package"
            );
        }

        let target_query = sdbl_package.query_at_offset(sdbl_offset)?;

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
