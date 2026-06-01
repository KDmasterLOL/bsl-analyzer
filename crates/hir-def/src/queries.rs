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

#[salsa::tracked(lru = 256)]
pub fn resolved_module_summary_query<'db>(
    db: &'db dyn crate::configs::ConfigsDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<crate::call_graph::ResolvedModuleSummary> {
    use crate::call_graph::{CallTarget, EdgeProvenance, ResolvedCallEdge, ResolvedTarget};

    let _span = tracing::info_span!("resolved_module_summary", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);

    let summary = db.module_call_summary(module_id);
    let resolver = crate::resolver::Resolver::with_workspace_scope(module_id);

    let mut edges = Vec::with_capacity(summary.call_edges.len());
    for edge in &summary.call_edges {
        let (target, provenance) = match &edge.target {
            CallTarget::Local { callee_local_id } => (
                ResolvedTarget::Method(crate::MethodId {
                    module: module_id,
                    local_id: *callee_local_id,
                }),
                EdgeProvenance::Resolved,
            ),
            CallTarget::QualifiedModule { module_name, method_name } => {
                match resolver.resolve_qualified_method(db, module_name, method_name) {
                    Ok(r) if r.is_export => {
                        (ResolvedTarget::Method(r.method_id), EdgeProvenance::Resolved)
                    }
                    Ok(_) => (
                        ResolvedTarget::Unresolved(edge.target.clone()),
                        EdgeProvenance::VisibilityBlocked,
                    ),
                    Err(_) => (
                        ResolvedTarget::Unresolved(edge.target.clone()),
                        EdgeProvenance::Unresolved,
                    ),
                }
            }
            CallTarget::ManagerAccess {
                manager_type,
                object_name,
                method_name: Some(method_name),
            } => match resolver.resolve_manager_method(db, *manager_type, object_name, method_name)
            {
                Ok(r) if r.is_export => {
                    (ResolvedTarget::Method(r.method_id), EdgeProvenance::Inferred)
                }
                Ok(_) => (
                    ResolvedTarget::Unresolved(edge.target.clone()),
                    EdgeProvenance::VisibilityBlocked,
                ),
                Err(_) => {
                    (ResolvedTarget::Unresolved(edge.target.clone()), EdgeProvenance::Unresolved)
                }
            },
            // ManagerAccess without a method is an object reference, and a
            // `ЭтотОбъект` call that reached here is a platform object method
            // (local user methods were already resolved at extraction time) —
            // neither maps to a user node yet.
            CallTarget::ManagerAccess { .. }
            | CallTarget::ThisObjectMethod { .. }
            | CallTarget::Unresolved => {
                (ResolvedTarget::Unresolved(edge.target.clone()), EdgeProvenance::Unresolved)
            }
        };

        edges.push(ResolvedCallEdge {
            caller: edge.caller,
            target,
            kind: edge.kind,
            range: edge.range,
            provenance,
        });
    }

    Arc::new(crate::call_graph::ResolvedModuleSummary { module: module_id, edges })
}

#[salsa::tracked(lru = 16)]
pub fn workspace_call_graph_query(
    db: &dyn crate::configs::ConfigsDatabase,
    source_root_input: base_db::SourceRootInput,
) -> Arc<crate::call_graph::WorkspaceCallGraph> {
    use crate::call_graph::{
        CallerId, GraphNode, MethodDispatch, ResolvedTarget, WorkspaceCallEdge, WorkspaceCallGraph,
    };

    let source_root = source_root_input.root(db);
    let file_set = source_root.file_set();
    let modules: Vec<ModuleId> = source_root
        .iter()
        .filter(|&file_id| crate::workspace::is_bsl_source(file_set, file_id))
        .map(ModuleId::new)
        .collect();
    let _span = tracing::info_span!("workspace_call_graph", module_count = modules.len()).entered();

    let mut graph = WorkspaceCallGraph::default();

    // Pass 1: per-method client/server dispatch, needed before edges so the
    // boundary flag can consult a callee that lives in another module. Common
    // modules dispatch at the module level (execution context); method-level
    // `&НаКлиенте`/`&НаСервере` annotations only apply where the module context
    // is unknown (form/command modules).
    for &module in &modules {
        let summary = db.module_call_summary(module);
        let module_dispatch = db
            .module_metadata(module)
            .execution_context
            .and_then(MethodDispatch::from_execution_context);
        for method in &summary.methods {
            graph.set_dispatch(
                GraphNode::Method(crate::MethodId { module, local_id: method.local_id }),
                module_dispatch.unwrap_or(method.dispatch),
            );
        }
    }

    // Pass 2: fold resolved edges into forward/reverse adjacency.
    for &module in &modules {
        let summary = db.resolved_module_summary(module);
        for edge in &summary.edges {
            let to_mid = match &edge.target {
                ResolvedTarget::Method(method_id) => *method_id,
                ResolvedTarget::Unresolved(_) => continue,
            };
            let from = match edge.caller {
                CallerId::Method(local_id) => {
                    GraphNode::Method(crate::MethodId { module, local_id })
                }
                CallerId::ModuleCode => GraphNode::ModuleCode(module),
            };
            let to = GraphNode::Method(to_mid);
            let crosses_client_to_server =
                graph.dispatch(from).is_some_and(|d| d.can_run_on_client)
                    && graph.dispatch(to).is_some_and(|d| d.is_server_only());
            graph.insert(WorkspaceCallEdge {
                from,
                to,
                kind: edge.kind,
                provenance: edge.provenance,
                crosses_client_to_server,
            });
        }
    }

    Arc::new(graph)
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
