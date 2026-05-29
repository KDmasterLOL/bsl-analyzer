use std::sync::Arc;

use base_db::FileIdInput;

use vfs::FileId;

use crate::{
    body::ExternalRef, module_index::ModuleIndex, DefDatabase, ModuleBodies, ModuleData, ModuleId,
    WorkspaceSymbols,
};

pub use crate::conditional_tree::conditional_tree_query;
pub use crate::item_tree::item_tree_query;
pub use crate::region_tree::region_tree_query;
pub use crate::symbol_tree::symbol_tree_query;
pub use crate::workspace_index::workspace_index_query;

#[salsa::tracked(lru = 512)]
pub fn module_data_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleData> {
    let _span = tracing::info_span!("module_data", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let tree = db.item_tree(file_id);
    let module_id = ModuleId::new(file_id);
    Arc::new(ModuleData::from_item_tree(module_id, tree))
}

#[salsa::tracked(lru = 128)]
pub fn module_bodies_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleBodies> {
    let _span = tracing::info_span!("module_bodies", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);

    let result = crate::lower_module_bodies(db, module_id);

    Arc::new(result)
}

#[salsa::tracked(lru = 16)]
pub fn workspace_symbols_query(
    db: &dyn DefDatabase,
    source_root_input: base_db::SourceRootInput,
) -> Arc<WorkspaceSymbols> {
    let source_root = source_root_input.root(db);
    let file_set = source_root.file_set();
    let files: Vec<_> = source_root
        .iter()
        .filter(|&file_id| crate::workspace::is_bsl_source(file_set, file_id))
        .collect();
    let _span = tracing::info_span!("workspace_symbols", file_count = files.len()).entered();
    Arc::new(crate::workspace::workspace_symbols(db, &files))
}

#[salsa::tracked(lru = 256)]
pub fn module_call_summary_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<crate::call_graph::ModuleCallSummary> {
    let _span = tracing::info_span!("module_call_summary", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);

    let item_tree = db.item_tree(file_id);
    let module_bodies = db.module_bodies(module_id);
    let module_metadata = db.module_metadata(module_id);

    let form_handlers: &[bsl_metadata::FormEventHandler] =
        module_metadata.form.as_ref().map(|f| f.event_handlers.as_slice()).unwrap_or(&[]);

    Arc::new(crate::call_graph::extract_call_summary(&item_tree, &module_bodies, form_handlers))
}

#[salsa::tracked(lru = 512)]
pub fn file_external_refs_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<Vec<ExternalRef>> {
    let _span = tracing::info_span!("file_external_refs", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);

    tracing::debug!(file_id = file_id.0, "file_external_refs: calling module_bodies");
    let bodies = db.module_bodies(module_id);

    let method_count = bodies.iter_lower_results().count();
    tracing::debug!(file_id = file_id.0, method_count, "file_external_refs: got module_bodies");

    let mut refs = Vec::new();
    for (method_id, lower_result) in bodies.iter_lower_results() {
        let ref_count = lower_result.external_refs.len();
        if ref_count > 0 {
            tracing::debug!(
                file_id = file_id.0,
                method_id = ?method_id,
                ref_count,
                "file_external_refs: found refs in method"
            );
        }
        refs.extend(lower_result.external_refs.iter().cloned());
    }

    if let Some(module_code) = bodies.module_code_result() {
        let ref_count = module_code.external_refs.len();
        if ref_count > 0 {
            tracing::debug!(
                file_id = file_id.0,
                ref_count,
                "file_external_refs: found refs in module code"
            );
        }
        refs.extend(module_code.external_refs.iter().cloned());
    }

    tracing::debug!(file_id = file_id.0, total_refs = refs.len(), "file_external_refs: done");
    Arc::new(refs)
}

#[salsa::tracked(lru = 16)]
pub fn module_index_query(
    _db: &dyn DefDatabase,
    source_root_input: base_db::SourceRootInput,
) -> Arc<ModuleIndex> {
    let source_root = source_root_input.root(_db);
    let _span =
        tracing::info_span!("module_index", file_count = source_root.iter().count()).entered();

    let file_set = source_root.file_set();
    let paths: Vec<(FileId, String)> = source_root
        .iter()
        .filter_map(|file_id| {
            let vfs_path = file_set.path_for_file(&file_id)?;
            let path = vfs_path.as_path();
            let path_str = path.to_str()?;
            Some((file_id, path_str.to_string()))
        })
        .collect();

    let index = ModuleIndex::build_from_paths(paths.iter().map(|(id, p)| (*id, p.as_str())));

    Arc::new(index)
}

#[salsa::tracked(lru = 512)]
pub fn file_dependencies_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<Vec<FileId>> {
    let _span = tracing::info_span!("file_dependencies", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);

    let source_root_id = db.file_source_root_input(file_id).source_root_id(db);
    let source_root_input = db.source_root_input(source_root_id);

    let index = module_index_query(db, source_root_input);
    tracing::debug!(
        file_id = file_id.0,
        common_modules = index.common_module_count(),
        managers = index.manager_count(),
        "file_dependencies: got module_index"
    );

    let file_id_input = base_db::FileIdInput::new(db, file_id);
    let refs = file_external_refs_query(db, file_id_input);
    tracing::debug!(
        file_id = file_id.0,
        refs_count = refs.len(),
        "file_dependencies: got external_refs"
    );

    let mut deps: Vec<FileId> = refs.iter().filter_map(|r| index.resolve(r)).collect();
    tracing::debug!(
        file_id = file_id.0,
        resolved = deps.len(),
        unresolved = refs.len() - deps.len(),
        "file_dependencies: resolved refs"
    );

    deps.sort_by_key(|f| f.index());
    deps.dedup();

    Arc::new(deps)
}
