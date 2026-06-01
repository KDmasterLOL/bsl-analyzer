use std::sync::Arc;

use base_db::FileIdInput;
use bsl_metadata::MdoType;
use rustc_hash::{FxHashMap, FxHashSet};

use vfs::FileId;

use crate::{
    body::ExternalRef,
    call_graph::{GraphNode, MethodDispatch, WorkspaceCallEdge},
    module_index::ModuleIndex,
    DefDatabase, ModuleBodies, ModuleData, ModuleId, WorkspaceSymbols,
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
    use crate::call_graph::{
        CallTarget, EdgeKind, EdgeProvenance, ResolvedCallEdge, ResolvedTarget,
    };

    let _span = tracing::info_span!("resolved_module_summary", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);

    let summary = db.module_call_summary(module_id);
    let resolver = crate::resolver::Resolver::with_workspace_scope(module_id);

    let mut edges = Vec::with_capacity(summary.call_edges.len());
    for edge in &summary.call_edges {
        // Each branch yields the resolved target, its provenance, and the edge
        // kind. The kind defaults to the extraction-time kind, but manager
        // accesses that land on a metadata object override it with
        // `ManagerCreates`/`ManagerAccess` (create-vs-touch is a semantic call).
        let (target, provenance, kind) = match &edge.target {
            CallTarget::Local { callee_local_id } => (
                ResolvedTarget::Method(crate::MethodId {
                    module: module_id,
                    local_id: *callee_local_id,
                }),
                EdgeProvenance::Resolved,
                edge.kind,
            ),
            CallTarget::QualifiedModule { module_name, method_name } => {
                match resolver.resolve_qualified_method(db, module_name, method_name) {
                    Ok(r) if r.is_export => {
                        (ResolvedTarget::Method(r.method_id), EdgeProvenance::Resolved, edge.kind)
                    }
                    Ok(_) => (
                        ResolvedTarget::Unresolved(edge.target.clone()),
                        EdgeProvenance::VisibilityBlocked,
                        edge.kind,
                    ),
                    Err(_) => (
                        ResolvedTarget::Unresolved(edge.target.clone()),
                        EdgeProvenance::Unresolved,
                        edge.kind,
                    ),
                }
            }
            CallTarget::ManagerAccess {
                manager_type,
                object_name,
                method_name: Some(method_name),
            } => match resolver.resolve_manager_method(db, *manager_type, object_name, method_name)
            {
                // A user manager-module method: keep the edge about the method.
                Ok(r) if r.is_export => {
                    (ResolvedTarget::Method(r.method_id), EdgeProvenance::Inferred, edge.kind)
                }
                Ok(_) => (
                    ResolvedTarget::Unresolved(edge.target.clone()),
                    EdgeProvenance::VisibilityBlocked,
                    edge.kind,
                ),
                // A platform manager method (create/find/…): the edge is about
                // the metadata object it touches, not a user node.
                Err(_) => (
                    ResolvedTarget::Mdo {
                        mdo_type: manager_type.to_mdo_type(),
                        object_name: object_name.clone(),
                    },
                    EdgeProvenance::Inferred,
                    manager_edge_kind(method_name.as_str()),
                ),
            },
            // A bare `Справочники.X` reference (no method) touches the object.
            CallTarget::ManagerAccess { manager_type, object_name, method_name: None } => (
                ResolvedTarget::Mdo {
                    mdo_type: manager_type.to_mdo_type(),
                    object_name: object_name.clone(),
                },
                EdgeProvenance::Inferred,
                EdgeKind::ManagerAccess,
            ),
            // A `ЭтотОбъект` call that reached here is a platform object method
            // (local user methods were already resolved at extraction time).
            CallTarget::ThisObjectMethod { .. } | CallTarget::Unresolved => (
                ResolvedTarget::Unresolved(edge.target.clone()),
                EdgeProvenance::Unresolved,
                edge.kind,
            ),
        };

        edges.push(ResolvedCallEdge {
            caller: edge.caller,
            target,
            kind,
            range: edge.range,
            provenance,
        });
    }

    Arc::new(crate::call_graph::ResolvedModuleSummary { module: module_id, edges })
}

/// Classify a platform manager method into a metadata-object edge kind. Creation
/// methods (`СоздатьЭлемент`/`СоздатьГруппу`/… or English `Create…`) produce a
/// `ManagerCreates` edge; everything else (find/select/…) a `ManagerAccess` edge.
/// Only platform methods reach here — user manager-module methods resolve to a
/// `Method` node earlier — so the name prefix is a reliable creation signal.
fn manager_edge_kind(method_name: &str) -> crate::call_graph::EdgeKind {
    use crate::call_graph::EdgeKind;
    let lower = method_name.to_lowercase();
    if lower.starts_with("создать") || lower.starts_with("create") {
        EdgeKind::ManagerCreates
    } else {
        EdgeKind::ManagerAccess
    }
}

/// Canonical-spelling map for metadata objects. BSL identifiers are
/// case-insensitive, so different spellings of the same object across call sites
/// and query texts must collapse to a single `Mdo`/`Attribute` node. First-seen
/// spelling wins; shared between the call-edge and query-ref projections.
type MdoCanonical = FxHashMap<(MdoType, String), crate::name::Name>;

/// Project one module's resolved call/manager edges (its
/// `resolved_module_summary`) into workspace graph edges. `dispatch` supplies
/// per-node client/server capability — including callees in other modules — so
/// the client→server boundary flag can be set without the whole graph being
/// materialised. `mdo_canonical` is updated as new metadata-object spellings are
/// seen and is shared with [`project_module_query_edges`].
///
/// Forcing only `resolved_module_summary(module)` (which lowers exactly this
/// module's bodies) keeps the projection per-module: a streaming build can
/// project one module, write its edges, and evict before the next.
fn project_module_call_edges(
    db: &dyn crate::configs::ConfigsDatabase,
    module: ModuleId,
    dispatch: &dyn Fn(&GraphNode) -> Option<MethodDispatch>,
    mdo_canonical: &mut MdoCanonical,
) -> Vec<WorkspaceCallEdge> {
    use crate::call_graph::{CallerId, ResolvedTarget};

    let summary = db.resolved_module_summary(module);
    let mut edges = Vec::with_capacity(summary.edges.len());
    for edge in &summary.edges {
        let to = match &edge.target {
            ResolvedTarget::Method(method_id) => GraphNode::Method(*method_id),
            ResolvedTarget::Mdo { mdo_type, object_name } => {
                let canon = mdo_canonical
                    .entry((*mdo_type, object_name.as_str().to_lowercase()))
                    .or_insert_with(|| object_name.clone())
                    .clone();
                GraphNode::Mdo { mdo_type: *mdo_type, object_name: canon }
            }
            ResolvedTarget::Unresolved(_) => continue,
        };
        let from = match edge.caller {
            CallerId::Method(local_id) => GraphNode::Method(crate::MethodId { module, local_id }),
            CallerId::ModuleCode => GraphNode::ModuleCode(module),
        };
        // Mdo nodes have no dispatch, so the boundary flag falls out `false`.
        let crosses_client_to_server = dispatch(&from).is_some_and(|d| d.can_run_on_client)
            && dispatch(&to).is_some_and(|d| d.is_server_only());
        edges.push(WorkspaceCallEdge {
            from,
            to,
            kind: edge.kind,
            provenance: edge.provenance,
            crosses_client_to_server,
        });
    }
    edges
}

/// Project one module's SDBL query references into `query_ref` graph edges: a
/// method (or module body) that runs a query reading a metadata object links to
/// that object's `Mdo` node (coarse) and to each read attribute's `Attribute`
/// node (precise). `mdo_canonical` is shared with [`project_module_call_edges`]
/// so query- and call-derived `Mdo` nodes are the same node. The `seen_*` sets
/// dedup across the whole workspace ("this method reads Catalog X" once), so they
/// are threaded through every module's projection rather than reset per module.
fn project_module_query_edges(
    db: &dyn crate::configs::ConfigsDatabase,
    module: ModuleId,
    mdo_canonical: &mut MdoCanonical,
    seen_query_ref: &mut FxHashSet<(GraphNode, MdoType, String)>,
    seen_query_attr: &mut FxHashSet<(GraphNode, MdoType, String, String)>,
) -> Vec<WorkspaceCallEdge> {
    use crate::call_graph::{EdgeKind, EdgeProvenance};

    let file_id_input = base_db::FileIdInput::new(db, module.file_id);
    let sdbl_entries = crate::sdbl_cache::sdbl_hir_for_file_query(db, file_id_input);
    let mut edges = Vec::new();
    for (sdbl_expr_id, package) in sdbl_entries.iter() {
        let from = match sdbl_expr_id.owner {
            crate::DefWithBodyId::Method(local_id) => {
                GraphNode::Method(crate::MethodId { module, local_id })
            }
            crate::DefWithBodyId::ModuleCode => GraphNode::ModuleCode(module),
        };
        let mut resolved = Vec::new();
        let mut attrs = Vec::new();
        for query in package.queries() {
            query.hir.collect_resolved_tables(&mut resolved);
            query.hir.collect_resolved_attributes(&mut attrs);
        }
        // Coarse: the method reads object X (survives even when columns are
        // unresolved, e.g. `ВЫБРАТЬ *`).
        for table in resolved {
            let (mdo_type, name) = match table {
                sdbl_hir::ResolvedTable::Metadata { mdo_type, name, .. }
                | sdbl_hir::ResolvedTable::Register { mdo_type, name, .. } => (*mdo_type, name),
                sdbl_hir::ResolvedTable::TempTable { .. } => continue,
            };
            let name_lower = name.to_lowercase();
            if !seen_query_ref.insert((from.clone(), mdo_type, name_lower.clone())) {
                continue;
            }
            let canon = mdo_canonical
                .entry((mdo_type, name_lower))
                .or_insert_with(|| crate::name::Name::new(name))
                .clone();
            edges.push(WorkspaceCallEdge {
                from: from.clone(),
                to: GraphNode::Mdo { mdo_type, object_name: canon },
                kind: EdgeKind::QueryRef,
                provenance: EdgeProvenance::Inferred,
                crosses_client_to_server: false,
            });
        }
        // Precise: the method reads object X's attribute Y.
        for (mdo_type, object, attr) in attrs {
            let object_lower = object.to_lowercase();
            if !seen_query_attr.insert((
                from.clone(),
                mdo_type,
                object_lower.clone(),
                attr.to_lowercase(),
            )) {
                continue;
            }
            let canon = mdo_canonical
                .entry((mdo_type, object_lower))
                .or_insert_with(|| crate::name::Name::new(&object))
                .clone();
            edges.push(WorkspaceCallEdge {
                from: from.clone(),
                to: GraphNode::Attribute {
                    mdo_type,
                    object_name: canon,
                    attr_name: crate::name::Name::new(&attr),
                },
                kind: EdgeKind::QueryRef,
                provenance: EdgeProvenance::Inferred,
                crosses_client_to_server: false,
            });
        }
    }
    edges
}

#[salsa::tracked(lru = 16)]
pub fn workspace_call_graph_query(
    db: &dyn crate::configs::ConfigsDatabase,
    source_root_input: base_db::SourceRootInput,
) -> Arc<crate::call_graph::WorkspaceCallGraph> {
    use crate::call_graph::WorkspaceCallGraph;

    let source_root = source_root_input.root(db);
    let file_set = source_root.file_set();
    let modules: Vec<ModuleId> = source_root
        .iter()
        .filter(|&file_id| crate::workspace::is_bsl_source(file_set, file_id))
        .map(ModuleId::new)
        .collect();
    let _span = tracing::info_span!("workspace_call_graph", module_count = modules.len()).entered();

    let mut graph = WorkspaceCallGraph::default();
    let mut mdo_canonical: MdoCanonical = FxHashMap::default();

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

    // Pass 2: resolved call/manager edges, projected per module.
    for &module in &modules {
        let edges = {
            let dispatch = |node: &GraphNode| graph.dispatch(node);
            project_module_call_edges(db, module, &dispatch, &mut mdo_canonical)
        };
        for edge in edges {
            graph.insert(edge);
        }
    }

    // Pass 3: SDBL query_ref edges, projected per module. Built after the call
    // edges so it shares the populated `mdo_canonical`; the `seen_*` sets dedup
    // across the whole workspace.
    let mut seen_query_ref: FxHashSet<(GraphNode, MdoType, String)> = FxHashSet::default();
    let mut seen_query_attr: FxHashSet<(GraphNode, MdoType, String, String)> = FxHashSet::default();
    for &module in &modules {
        let edges = project_module_query_edges(
            db,
            module,
            &mut mdo_canonical,
            &mut seen_query_ref,
            &mut seen_query_attr,
        );
        for edge in edges {
            graph.insert(edge);
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
