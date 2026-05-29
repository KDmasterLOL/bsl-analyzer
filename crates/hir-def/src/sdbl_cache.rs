use std::sync::Arc;

use base_db::FileIdInput;

use crate::configs::ConfigsDatabase;
use crate::{DefDatabase, ModuleId, SdblExprId};

pub type SdblInFile = Vec<(SdblExprId, syntax::SdblQueryInfo)>;

pub type SdblHirEntries = Arc<Vec<(SdblExprId, Arc<sdbl_hir::SdblPackage>)>>;

#[salsa::tracked(lru = 128)]
pub fn all_sdbl_in_file_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<SdblInFile> {
    let _span = tracing::debug_span!("all_sdbl_in_file", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);

    let module_bodies = db.module_bodies(module_id);
    let mut result = Vec::new();

    for (local_id, body) in module_bodies.iter_bodies() {
        for (expr_id, query_info) in body.sdbl_exprs() {
            let sdbl_expr_id = SdblExprId::from_method(local_id, expr_id);
            result.push((sdbl_expr_id, query_info.clone()));
        }
    }

    if let Some(module_code) = module_bodies.module_code() {
        for (expr_id, query_info) in module_code.sdbl_exprs() {
            let sdbl_expr_id = SdblExprId::from_module_code(expr_id);
            result.push((sdbl_expr_id, query_info.clone()));
        }
    }

    result.sort_by_key(|(_, query_info)| query_info.bsl_literal_range.start());

    tracing::debug!(count = result.len(), "Collected SDBL from HIR");
    Arc::new(result)
}

#[salsa::tracked(lru = 64)]
pub fn sdbl_hir_for_file_query<'db>(
    db: &'db dyn ConfigsDatabase,
    file_id_input: FileIdInput<'db>,
) -> SdblHirEntries {
    let _span = tracing::debug_span!("sdbl_hir_for_file", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);

    let sdbl_queries = all_sdbl_in_file_query(db, file_id_input);
    if sdbl_queries.is_empty() {
        return Arc::new(Vec::new());
    }

    let configuration = db.merged_visible_configuration(file_id);

    let mut result = Vec::with_capacity(sdbl_queries.len());
    for (expr_id, query_info) in sdbl_queries.iter() {
        if let Some(ref sdbl_ast) = query_info.query_ast {
            let sdbl_package = sdbl_hir::lower_sdbl_to_hir(sdbl_ast, configuration.clone());
            result.push((*expr_id, Arc::new(sdbl_package)));
        }
    }

    tracing::debug!(count = result.len(), "Lowered SDBL HIR for file");
    Arc::new(result)
}
