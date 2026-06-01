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

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use bsl_metadata::MdoType;
use vfs::FileId;

use crate::{
    call_graph::{
        EdgeKind, EdgeProvenance, GraphMethodEntry, GraphNode, MethodDispatch, ResolvedCallEdge,
        ResolvedModuleSummary, ResolvedTarget, WorkspaceCallEdge, WorkspaceCallGraph,
    },
    configs::ConfigsDatabase,
    module_index::{module_key_for_path, ModuleKey},
    name::Name,
    resolver::Resolver,
    MethodId, ModuleId,
};

/// A module's methods as seen from the item tree alone (no body lowering).
/// `by_name` serves resolution; `all` carries the declaration facts (name, export,
/// ranges) that node materialisation needs. Dispatch lives in
/// [`GraphIndex::node_dispatch`].
struct ModuleMethods {
    /// Lowercased name → first declaration, mirroring `SymbolTree::find_method`.
    by_name: FxHashMap<String, MethodRef>,
    /// Every method in declaration order (the index is the `local_id`).
    all: Vec<GraphMethodEntry>,
}

#[derive(Clone, Copy)]
struct MethodRef {
    local_id: u32,
    is_export: bool,
}

/// The compact, resident method index over a set of modules, plus the per-method
/// client/server dispatch table (the fold's Pass-1 data).
#[derive(Default)]
pub struct GraphIndex {
    methods: FxHashMap<ModuleId, ModuleMethods>,
    /// Per-method dispatch (module execution context wins, else annotation),
    /// resident so a batched build can flag client→server edges without rebuilding
    /// the whole graph's dispatch table per batch.
    node_dispatch: FxHashMap<MethodId, MethodDispatch>,
}

impl GraphIndex {
    /// An empty index; populate with [`Self::add_module`] (e.g. one batch's modules
    /// at a time in a fresh database) to build the whole-config index without ever
    /// holding every module's item tree resident.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the index for `modules` in one pass. See [`Self::add_module`]; this is
    /// the convenience path when every module's text is already in `db`.
    ///
    /// `modules` must cover **every** module that could be a resolution target,
    /// not just the ones whose edges are projected: a qualified/manager call into a
    /// module absent from the index falls into the method-absent arm (→ Unresolved
    /// / Mdo) instead of resolving. A batched build therefore indexes the whole
    /// configuration, even though it lowers bodies one batch at a time later.
    pub fn build(db: &dyn ConfigsDatabase, modules: &[ModuleId]) -> Self {
        let mut index = Self::new();
        for &module in modules {
            index.add_module(db, module);
        }
        index
    }

    /// Add one module's compact method table + dispatch from the item tree and
    /// module metadata only — no body lowering. The heavy `item_tree` is transient,
    /// so building the whole index batch-by-batch in fresh databases keeps peak RAM
    /// bounded.
    pub fn add_module(&mut self, db: &dyn ConfigsDatabase, module: ModuleId) {
        let item_tree = db.item_tree(module.file_id);
        let all = crate::call_graph::extract_graph_methods(&item_tree);
        let module_dispatch = db
            .module_metadata(module)
            .execution_context
            .and_then(MethodDispatch::from_execution_context);

        // First-wins lowercased map, matching `SymbolTree::find_method`.
        let mut by_name = FxHashMap::default();
        for entry in &all {
            by_name
                .entry(entry.name.as_str().to_lowercase())
                .or_insert(MethodRef { local_id: entry.local_id, is_export: entry.is_export });
            // Pass-1 dispatch rule: module execution context wins, else annotation.
            self.node_dispatch.insert(
                MethodId { module, local_id: entry.local_id },
                module_dispatch.unwrap_or(entry.dispatch),
            );
        }
        self.methods.insert(module, ModuleMethods { by_name, all });
    }

    /// The declaration facts (name, export, dispatch, ranges) for a method, for
    /// node materialisation. `None` if the module/method is not indexed.
    pub fn method_entry(&self, method: MethodId) -> Option<&GraphMethodEntry> {
        self.methods.get(&method.module)?.all.iter().find(|e| e.local_id == method.local_id)
    }

    /// Every indexed method node. This is exactly the fold's Pass-1 dispatch-seeded
    /// set (`node_dispatch.keys`), so a streaming build that materialises a node for
    /// each yields the same isolated (call-free) method nodes the in-memory graph's
    /// [`nodes`](WorkspaceCallGraph::nodes) exposes, not only edge endpoints.
    pub fn method_nodes(&self) -> impl Iterator<Item = MethodId> + '_ {
        self.node_dispatch.keys().copied()
    }

    /// Method lookup mirroring `SymbolTree::find_method` (lowercased, first-wins).
    /// Returns the same `{local_id, is_export}` the Salsa symbol tree would, so the
    /// reconstructed `MethodId` is identical.
    fn find_method(&self, target: ModuleId, name: &Name) -> Option<MethodRef> {
        self.methods.get(&target)?.by_name.get(&name.as_str().to_lowercase()).copied()
    }

    /// Resident per-node dispatch — the same value the fold's seeded graph returns
    /// (`None` for non-method nodes), so the client→server boundary flag matches.
    pub fn dispatch(&self, node: &GraphNode) -> Option<MethodDispatch> {
        match node {
            GraphNode::Method(method_id) => self.node_dispatch.get(method_id).copied(),
            _ => None,
        }
    }

    /// Populate `graph`'s per-method dispatch table (the fold's Pass 1).
    fn seed_dispatch(&self, graph: &mut WorkspaceCallGraph) {
        for (&method_id, &dispatch) in &self.node_dispatch {
            graph.set_dispatch(GraphNode::Method(method_id), dispatch);
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

/// Workspace-wide state threaded across batches: MDO spelling canonicalization and
/// query-ref dedup. Reuse ONE instance for the whole build — recreating it per
/// batch would split an object's `Mdo` node across spellings and re-emit duplicate
/// query_ref edges.
#[derive(Default)]
pub struct GraphBuildState {
    mdo_canonical: crate::queries::MdoCanonical,
    seen_query_ref: FxHashSet<(GraphNode, MdoType, String)>,
    seen_query_attr: FxHashSet<(GraphNode, MdoType, String, String)>,
}

impl GraphBuildState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Project the call/manager and SDBL query_ref edges for one `batch` of modules,
/// resolving cross-module targets and the client→server boundary flag through the
/// resident `index`. The batch database therefore needs only its own modules'
/// texts (plus the configuration metadata) — the foundation of the RAM-bounded
/// streaming build.
///
/// `state` carries the workspace-wide canonicalization/dedup across batches and
/// MUST be reused.
///
/// This projects a batch's call/manager edges **and** its query edges together. To
/// reproduce the fold's `Mdo`/`Attribute` node spelling byte-for-byte, a streaming
/// build must instead run [`project_batch_call_edges`] across ALL batches before
/// [`project_batch_query_edges`] across all batches (the fold's Pass-2-then-Pass-3
/// order): an object referenced only in code vs. only in a query would otherwise
/// get a different first-seen spelling. This combined helper is for callers that
/// process one batch in isolation and accept that per-batch ordering.
pub fn project_batch_edges(
    db: &dyn ConfigsDatabase,
    batch: &[ModuleId],
    index: &GraphIndex,
    state: &mut GraphBuildState,
) -> Vec<WorkspaceCallEdge> {
    let mut edges = project_batch_call_edges(db, batch, index, state);
    edges.extend(project_batch_query_edges(db, batch, state));
    edges
}

/// A batch's resolved call/manager edges, resolving cross-module targets through
/// the resident `index` (so the batch database needs only its own texts). Run this
/// across every batch before [`project_batch_query_edges`] to match the fold's
/// global Pass-2-then-Pass-3 canonicalization order. `state.mdo_canonical` is
/// shared and updated as new metadata-object spellings are first seen.
pub fn project_batch_call_edges(
    db: &dyn ConfigsDatabase,
    batch: &[ModuleId],
    index: &GraphIndex,
    state: &mut GraphBuildState,
) -> Vec<WorkspaceCallEdge> {
    let mut edges = Vec::new();
    for &module in batch {
        let summary = resolve_module_summary_via_index(db, module, index);
        let dispatch = |node: &GraphNode| index.dispatch(node);
        edges.extend(crate::queries::project_module_call_edges(
            &summary,
            &dispatch,
            &mut state.mdo_canonical,
        ));
    }
    edges
}

/// A batch's SDBL `query_ref` edges. Run across every batch only after
/// [`project_batch_call_edges`] has run across all of them, sharing the same
/// `state`, so query-only metadata objects inherit the spelling the fold's Pass 3
/// would give them (call sites win first, exactly as in the fold).
pub fn project_batch_query_edges(
    db: &dyn ConfigsDatabase,
    batch: &[ModuleId],
    state: &mut GraphBuildState,
) -> Vec<WorkspaceCallEdge> {
    let mut edges = Vec::new();
    for &module in batch {
        edges.extend(crate::queries::project_module_query_edges(
            db,
            module,
            &mut state.mdo_canonical,
            &mut state.seen_query_ref,
            &mut state.seen_query_attr,
        ));
    }
    edges
}

// ---- build-time durable-id encoding + row projection -----------------------

/// Encode a module key to the durable id scope segment. Shared with `ide::graph`'s
/// serving path so build-time ids and serve-time ids agree.
pub fn encode_scope(key: &ModuleKey) -> String {
    match key {
        ModuleKey::Common { name } => format!("common/{name}"),
        ModuleKey::Manager { mdo_type, name } => {
            format!("manager/{}/{name}", mdo_type.english_name())
        }
        ModuleKey::Object { mdo_type, name } => {
            format!("object/{}/{name}", mdo_type.english_name())
        }
        ModuleKey::RecordSet { mdo_type, name } => {
            format!("recordset/{}/{name}", mdo_type.english_name())
        }
    }
}

/// The human-facing qualified scope for a module key (e.g. `ОбщийМодуль.X`).
pub fn display_scope(key: &ModuleKey) -> String {
    match key {
        ModuleKey::Common { name } => format!("ОбщийМодуль.{name}"),
        ModuleKey::Manager { mdo_type, name } => {
            format!("{}.{name}.МодульМенеджера", mdo_type.russian_name())
        }
        ModuleKey::Object { mdo_type, name } => {
            format!("{}.{name}.МодульОбъекта", mdo_type.russian_name())
        }
        ModuleKey::RecordSet { mdo_type, name } => {
            format!("{}.{name}.МодульНабораЗаписей", mdo_type.russian_name())
        }
    }
}

fn basename(path: &str) -> Option<&str> {
    path.rsplit('/').next()
}

fn dispatch_labels(d: MethodDispatch) -> Vec<&'static str> {
    let mut labels = Vec::new();
    if d.can_run_on_client {
        labels.push("client");
    }
    if d.can_run_on_server {
        labels.push("server");
    }
    labels
}

fn edge_kind_label(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::DirectLocal | EdgeKind::DirectQualifiedModule => "call",
        EdgeKind::ManagerCreates => "manager_creates",
        EdgeKind::ManagerAccess => "manager_access",
        EdgeKind::QueryRef => "query_ref",
    }
}

fn provenance_label(p: EdgeProvenance) -> &'static str {
    match p {
        EdgeProvenance::Resolved => "resolved",
        EdgeProvenance::Inferred => "inferred",
        EdgeProvenance::VisibilityBlocked => "visibility_blocked",
        EdgeProvenance::Unresolved => "unresolved",
    }
}

/// A graph node projected for storage and serving. Source text is read on demand
/// from `file` + the ranges, never stored inline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRow {
    pub id: String,
    pub kind: &'static str,
    pub name: String,
    pub qualified: String,
    pub module: Option<String>,
    /// Workspace path for source-on-demand (method/module nodes only).
    pub file: Option<String>,
    /// Byte offset of the declaration name token, to reconstruct the signature line.
    pub name_offset: Option<u32>,
    /// Method source byte range (method nodes only).
    pub src_start: Option<u32>,
    pub src_end: Option<u32>,
    pub dispatch: Vec<&'static str>,
    pub is_export: Option<bool>,
    /// Whether the id round-trips back to a node on its own.
    pub addressable: bool,
}

/// A resolved edge projected for storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeRow {
    pub from_id: String,
    pub to_id: String,
    pub kind: &'static str,
    pub provenance: &'static str,
    pub crosses: bool,
}

/// Encodes graph nodes/edges to durable rows at build time — method names/ranges
/// from the resident [`GraphIndex`], paths from the file set, no database. Produces
/// the SAME durable id strings as `ide::graph` (a parity test in `ide-db` guards
/// this), so ids an agent holds survive the in-memory → SQLite switch.
pub struct GraphRowEncoder<'a> {
    index: &'a GraphIndex,
    paths: &'a FxHashMap<FileId, String>,
    workspace_root: Option<&'a Path>,
}

impl<'a> GraphRowEncoder<'a> {
    pub fn new(
        index: &'a GraphIndex,
        paths: &'a FxHashMap<FileId, String>,
        workspace_root: Option<&'a Path>,
    ) -> Self {
        Self { index, paths, workspace_root }
    }

    fn path_for(&self, file: FileId) -> Option<String> {
        self.paths.get(&file).map(|p| p.replace('\\', "/"))
    }

    fn rel_path(&self, abs: &str) -> Option<String> {
        let root = self.workspace_root?;
        let root_str = root.to_str()?.replace('\\', "/");
        let stripped = abs.strip_prefix(&root_str)?;
        Some(stripped.trim_start_matches('/').to_string())
    }

    fn method_name(&self, method: MethodId) -> String {
        self.index.method_entry(method).map(|e| e.name.as_str().to_string()).unwrap_or_default()
    }

    fn module_display(&self, module: ModuleId) -> Option<String> {
        let path = self.path_for(module.file_id)?;
        match module_key_for_path(&path) {
            Some(key) => Some(display_scope(&key)),
            None => self.rel_path(&path).or_else(|| basename(&path).map(str::to_string)),
        }
    }

    /// The durable id and whether it round-trips on its own.
    pub fn encode(&self, node: &GraphNode) -> (String, bool) {
        match node {
            GraphNode::Method(method) => {
                let name = self.method_name(*method);
                let path = self.path_for(method.module.file_id);
                if let Some(key) = path.as_deref().and_then(module_key_for_path) {
                    (format!("method/{}/{name}", encode_scope(&key)), true)
                } else if let Some(rel) = path.as_deref().and_then(|p| self.rel_path(p)) {
                    (format!("method/file/{rel}::{name}"), true)
                } else {
                    let base = path.as_deref().and_then(basename).unwrap_or("?");
                    (format!("method/file/{base}::{name}"), false)
                }
            }
            GraphNode::ModuleCode(module) => {
                let path = self.path_for(module.file_id);
                if let Some(key) = path.as_deref().and_then(module_key_for_path) {
                    (format!("module/{}", encode_scope(&key)), true)
                } else if let Some(rel) = path.as_deref().and_then(|p| self.rel_path(p)) {
                    (format!("module/file/{rel}"), true)
                } else {
                    let base = path.as_deref().and_then(basename).unwrap_or("?");
                    (format!("module/file/{base}"), false)
                }
            }
            GraphNode::Mdo { mdo_type, object_name } => {
                (format!("mdo/{}/{}", mdo_type.english_name(), object_name.as_str()), true)
            }
            GraphNode::Attribute { mdo_type, object_name, attr_name } => (
                format!(
                    "attribute/{}/{}/{}",
                    mdo_type.english_name(),
                    object_name.as_str(),
                    attr_name.as_str()
                ),
                true,
            ),
        }
    }

    /// Project a node to its storage row.
    pub fn node_row(&self, node: &GraphNode) -> NodeRow {
        let (id, addressable) = self.encode(node);
        match node {
            GraphNode::Method(method) => {
                let entry = self.index.method_entry(*method);
                let name = entry.map(|e| e.name.as_str().to_string()).unwrap_or_default();
                let module = self.module_display(method.module);
                let qualified = match &module {
                    Some(scope) => format!("{scope}.{name}"),
                    None => name.clone(),
                };
                NodeRow {
                    id,
                    kind: "method",
                    name,
                    qualified,
                    module,
                    file: self.path_for(method.module.file_id),
                    name_offset: entry.map(|e| e.name_range.start().into()),
                    src_start: entry.map(|e| e.source_range.start().into()),
                    src_end: entry.map(|e| e.source_range.end().into()),
                    dispatch: self.index.dispatch(node).map(dispatch_labels).unwrap_or_default(),
                    is_export: entry.map(|e| e.is_export),
                    addressable,
                }
            }
            GraphNode::ModuleCode(module) => {
                let display = self.module_display(*module);
                let name = display.clone().unwrap_or_else(|| "<модуль>".to_string());
                NodeRow {
                    id,
                    kind: "module",
                    name: name.clone(),
                    qualified: name,
                    module: display,
                    file: self.path_for(module.file_id),
                    name_offset: None,
                    src_start: None,
                    src_end: None,
                    dispatch: self.index.dispatch(node).map(dispatch_labels).unwrap_or_default(),
                    is_export: None,
                    addressable,
                }
            }
            GraphNode::Mdo { mdo_type, object_name } => NodeRow {
                id,
                kind: "mdo",
                name: object_name.as_str().to_string(),
                qualified: format!("{}.{}", mdo_type.russian_name(), object_name.as_str()),
                module: None,
                file: None,
                name_offset: None,
                src_start: None,
                src_end: None,
                dispatch: Vec::new(),
                is_export: None,
                addressable,
            },
            GraphNode::Attribute { mdo_type, object_name, attr_name } => NodeRow {
                id,
                kind: "attribute",
                name: attr_name.as_str().to_string(),
                qualified: format!(
                    "{}.{}.{}",
                    mdo_type.russian_name(),
                    object_name.as_str(),
                    attr_name.as_str()
                ),
                module: None,
                file: None,
                name_offset: None,
                src_start: None,
                src_end: None,
                dispatch: Vec::new(),
                is_export: None,
                addressable,
            },
        }
    }

    /// Project a resolved edge to its storage row.
    pub fn edge_row(&self, edge: &WorkspaceCallEdge) -> EdgeRow {
        EdgeRow {
            from_id: self.encode(&edge.from).0,
            to_id: self.encode(&edge.to).0,
            kind: edge_kind_label(edge.kind),
            provenance: provenance_label(edge.provenance),
            crosses: edge.crosses_client_to_server,
        }
    }
}
