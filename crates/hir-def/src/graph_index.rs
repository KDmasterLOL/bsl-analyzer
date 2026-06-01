//! Resident, body-free "Pass-A" index for building the whole-config call graph
//! with bounded RAM, plus an index-backed projection that mirrors the Salsa
//! [`workspace_call_graph_query`](crate::queries::workspace_call_graph_query)
//! without lowering every module's bodies into one database at once.
//!
//! Resolving a qualified or manager call needs only the target module's method
//! table (lowercased name → first `{local_id, is_export}`); the rest — config
//! visibility and the path-based [`ModuleIndex`](crate::module_index::ModuleIndex)
//! — is already cheap and path-only. [`GraphIndex`] holds that method table for
//! every module so a streaming, batched build can resolve cross-batch targets
//! without keeping other modules' Salsa symbol trees resident.
//!
//! The index-backed resolution reuses the resolver's `locate_*` prefixes (config
//! visibility + path index, identical to the Salsa path) and swaps only the final
//! method lookup for a [`GraphIndex`] read. A golden-equivalence test
//! (`ide-db`) asserts the result is identical to the Salsa fold.

use rustc_hash::{FxHashMap, FxHashSet};

use bsl_metadata::MdoType;

use crate::{
    call_graph::{
        EdgeKind, EdgeProvenance, GraphMethodEntry, GraphNode, MethodDispatch, ResolvedCallEdge,
        ResolvedModuleSummary, ResolvedTarget, WorkspaceCallGraph,
    },
    configs::ConfigsDatabase,
    name::Name,
    resolver::Resolver,
    MethodId, ModuleId,
};

/// A module's methods as seen from the item tree alone (no body lowering).
struct ModuleMethods {
    /// Lowercased name → first declaration, mirroring `SymbolTree::find_method`.
    by_name: FxHashMap<String, MethodRef>,
    /// All entries in declaration order — for the dispatch table.
    all: Vec<GraphMethodEntry>,
    /// Module-level dispatch from the execution context (common modules); `None`
    /// falls back to each method's annotation dispatch.
    module_dispatch: Option<MethodDispatch>,
}

#[derive(Clone, Copy)]
struct MethodRef {
    local_id: u32,
    is_export: bool,
}

/// The compact, resident method index over a set of modules.
pub struct GraphIndex {
    methods: FxHashMap<ModuleId, ModuleMethods>,
}

impl GraphIndex {
    /// Build the index for `modules` from item trees + module metadata only — no
    /// body lowering. The heavy `item_tree` is transient; only the compact tables
    /// are retained, so this stays cheap over a whole configuration.
    ///
    /// `modules` must cover **every** module that could be a resolution target,
    /// not just the ones whose edges are projected: a qualified/manager call into a
    /// module absent from the index falls into the method-absent arm (→ Unresolved
    /// / Mdo) instead of resolving. A batched build therefore indexes the whole
    /// configuration here, even though it lowers bodies one batch at a time later.
    pub fn build(db: &dyn ConfigsDatabase, modules: &[ModuleId]) -> Self {
        let mut methods = FxHashMap::default();
        for &module in modules {
            let item_tree = db.item_tree(module.file_id);
            let all = crate::call_graph::extract_graph_methods(&item_tree);
            // First-wins lowercased map, matching `SymbolTree::find_method`.
            let mut by_name = FxHashMap::default();
            for entry in &all {
                by_name
                    .entry(entry.name.as_str().to_lowercase())
                    .or_insert(MethodRef { local_id: entry.local_id, is_export: entry.is_export });
            }
            let module_dispatch = db
                .module_metadata(module)
                .execution_context
                .and_then(MethodDispatch::from_execution_context);
            methods.insert(module, ModuleMethods { by_name, all, module_dispatch });
        }
        Self { methods }
    }

    /// Method lookup mirroring `SymbolTree::find_method` (lowercased, first-wins).
    /// Returns the same `{local_id, is_export}` the Salsa symbol tree would, so the
    /// reconstructed `MethodId` is identical.
    fn find_method(&self, target: ModuleId, name: &Name) -> Option<MethodRef> {
        self.methods.get(&target)?.by_name.get(&name.as_str().to_lowercase()).copied()
    }

    /// Populate `graph`'s per-method dispatch table exactly as the fold's Pass 1:
    /// module execution context wins, else the method's annotation dispatch.
    fn seed_dispatch(&self, graph: &mut WorkspaceCallGraph) {
        for (&module, mm) in &self.methods {
            for entry in &mm.all {
                graph.set_dispatch(
                    GraphNode::Method(MethodId { module, local_id: entry.local_id }),
                    mm.module_dispatch.unwrap_or(entry.dispatch),
                );
            }
        }
    }
}

/// Resolve a module's raw call edges against `index`, producing the same
/// [`ResolvedModuleSummary`] the Salsa `resolved_module_summary_query` would — but
/// with method lookup served from the resident index rather than the target
/// modules' Salsa symbol trees. Forces only this module's
/// [`module_call_summary`](crate::call_graph::extract_call_summary) (its own
/// bodies); cross-module targets are resolved through `index`.
pub fn resolve_module_summary_via_index(
    db: &dyn ConfigsDatabase,
    module: ModuleId,
    index: &GraphIndex,
) -> ResolvedModuleSummary {
    use crate::call_graph::CallTarget;

    let summary = db.module_call_summary(module);
    let resolver = Resolver::with_workspace_scope(module);

    let mut edges = Vec::with_capacity(summary.call_edges.len());
    for edge in &summary.call_edges {
        let (target, provenance, kind) = match &edge.target {
            CallTarget::Local { callee_local_id } => (
                ResolvedTarget::Method(MethodId { module, local_id: *callee_local_id }),
                EdgeProvenance::Resolved,
                edge.kind,
            ),
            CallTarget::QualifiedModule { module_name, method_name } => {
                match resolver.locate_common_module(db, module_name) {
                    Ok(target_module) => match index.find_method(target_module, method_name) {
                        Some(m) if m.is_export => (
                            ResolvedTarget::Method(MethodId {
                                module: target_module,
                                local_id: m.local_id,
                            }),
                            EdgeProvenance::Resolved,
                            edge.kind,
                        ),
                        // Found but not exported → visible-but-unreachable.
                        Some(_) => (
                            ResolvedTarget::Unresolved(edge.target.clone()),
                            EdgeProvenance::VisibilityBlocked,
                            edge.kind,
                        ),
                        // Module located but method absent.
                        None => (
                            ResolvedTarget::Unresolved(edge.target.clone()),
                            EdgeProvenance::Unresolved,
                            edge.kind,
                        ),
                    },
                    // Not visible / module not found.
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
            } => {
                let to_mdo = || ResolvedTarget::Mdo {
                    mdo_type: manager_type.to_mdo_type(),
                    object_name: object_name.clone(),
                };
                match resolver.locate_manager_module(db, *manager_type, object_name) {
                    Ok(target_module) => match index.find_method(target_module, method_name) {
                        // A user manager-module method: the edge is about the method.
                        Some(m) if m.is_export => (
                            ResolvedTarget::Method(MethodId {
                                module: target_module,
                                local_id: m.local_id,
                            }),
                            EdgeProvenance::Inferred,
                            edge.kind,
                        ),
                        Some(_) => (
                            ResolvedTarget::Unresolved(edge.target.clone()),
                            EdgeProvenance::VisibilityBlocked,
                            edge.kind,
                        ),
                        // No user method → a platform manager method touching the object.
                        None => (
                            to_mdo(),
                            EdgeProvenance::Inferred,
                            crate::queries::manager_edge_kind(method_name.as_str()),
                        ),
                    },
                    // No manager module → a platform manager method.
                    Err(_) => (
                        to_mdo(),
                        EdgeProvenance::Inferred,
                        crate::queries::manager_edge_kind(method_name.as_str()),
                    ),
                }
            }
            CallTarget::ManagerAccess { manager_type, object_name, method_name: None } => (
                ResolvedTarget::Mdo {
                    mdo_type: manager_type.to_mdo_type(),
                    object_name: object_name.clone(),
                },
                EdgeProvenance::Inferred,
                EdgeKind::ManagerAccess,
            ),
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

    ResolvedModuleSummary { module, edges }
}

/// Build the whole-config call graph over `modules` using the resident `index`
/// for resolution instead of the monolithic Salsa fold. Mirrors
/// `workspace_call_graph_query` pass-for-pass; the golden-equivalence test
/// guarantees an identical result. Each module's own bodies/SDBL are still
/// lowered (Pass 2/3), so a batched build that loads only a window of texts can
/// drive this over its slice.
pub fn workspace_call_graph_via_index(
    db: &dyn ConfigsDatabase,
    modules: &[ModuleId],
    index: &GraphIndex,
) -> WorkspaceCallGraph {
    let mut graph = WorkspaceCallGraph::default();
    let mut mdo_canonical = crate::queries::MdoCanonical::default();

    // Pass 1: dispatch table (cross-module endpoints need it for the boundary flag).
    index.seed_dispatch(&mut graph);

    // Pass 2: resolved call/manager edges via the index.
    for &module in modules {
        let summary = resolve_module_summary_via_index(db, module, index);
        let edges = {
            let dispatch = |node: &GraphNode| graph.dispatch(node);
            crate::queries::project_module_call_edges(&summary, &dispatch, &mut mdo_canonical)
        };
        for edge in edges {
            graph.insert(edge);
        }
    }

    // Pass 3: SDBL query_ref edges (config/metadata-resolved — no symbol trees;
    // identical to the Salsa path).
    let mut seen_query_ref: FxHashSet<(GraphNode, MdoType, String)> = FxHashSet::default();
    let mut seen_query_attr: FxHashSet<(GraphNode, MdoType, String, String)> = FxHashSet::default();
    for &module in modules {
        let edges = crate::queries::project_module_query_edges(
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

    graph
}
