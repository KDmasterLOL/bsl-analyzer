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
        let (all, module_dispatch) = Self::extract_module_data(db, module);
        self.insert_module_data(module, all, module_dispatch);
    }

    /// Add a whole batch's modules, lowering each module's item tree + metadata in
    /// parallel on `pool` (the per-module cost of [`Self::add_module`], repeated
    /// across the config), then folding the results into the index sequentially. The
    /// index is a set of per-module maps, so insertion order does not affect it —
    /// only the read-only extraction is parallelised.
    pub fn add_batch<DB: ConfigsDatabase + Clone + Send>(
        &mut self,
        pool: &rayon::ThreadPool,
        db: &DB,
        batch: &[ModuleId],
    ) {
        let extracted = parallel_per_module(pool, db, batch, |db, module| {
            (module, Self::extract_module_data(db, module))
        });
        for (module, (all, module_dispatch)) in extracted {
            self.insert_module_data(module, all, module_dispatch);
        }
    }

    /// The read-only half of [`Self::add_module`]: force a module's item tree and
    /// metadata and extract its method entries + module-level dispatch. Touches no
    /// shared index state, so it is safe to run for many modules concurrently.
    fn extract_module_data(
        db: &dyn ConfigsDatabase,
        module: ModuleId,
    ) -> (Vec<GraphMethodEntry>, Option<MethodDispatch>) {
        let item_tree = db.item_tree(module.file_id);
        let all = crate::call_graph::extract_graph_methods(&item_tree);
        let module_dispatch = db
            .module_metadata(module)
            .execution_context
            .and_then(MethodDispatch::from_execution_context);
        (all, module_dispatch)
    }

    /// The mutating half of [`Self::add_module`]: fold one module's extracted methods
    /// + dispatch into the resident index. Order-independent across modules.
    fn insert_module_data(
        &mut self,
        module: ModuleId,
        all: Vec<GraphMethodEntry>,
        module_dispatch: Option<MethodDispatch>,
    ) {
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

    /// A body-free signature hash of one module's methods: the ordered
    /// (original-spelling name, `is_export`, effective dispatch) of every method in
    /// declaration order. This is exactly the cross-module resolution + identity
    /// surface — `find_method` resolves on the name, callers' edges/boundary flags
    /// depend on `is_export` + effective dispatch, and the durable method id embeds
    /// the original name spelling. So if this hash is unchanged across an edit, no
    /// caller's resolved edge or stored node row can change and only this module's own
    /// rows need reprojecting. Deliberately excludes source ranges (they shift on any
    /// text edit) and bodies (a body edit not touching a signature keeps it stable).
    /// `None` if the module is not indexed.
    ///
    /// The hasher (std `DefaultHasher`) is stable within a build but not guaranteed
    /// across toolchain versions. That is safe by construction: a changed algorithm
    /// only makes the stored hashes mismatch on the next reload, which falls back to a
    /// full rebuild and re-persists fresh hashes — the same self-healing contract the
    /// workspace fingerprint already relies on for cache reuse.
    pub fn module_sig_hash(&self, module: ModuleId) -> Option<u64> {
        use std::hash::{Hash, Hasher};

        let methods = self.methods.get(&module)?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for entry in &methods.all {
            entry.name.as_str().hash(&mut hasher);
            entry.is_export.hash(&mut hasher);
            // The effective dispatch (module execution context wins, else annotation)
            // is what the stored node row and the client→server edge flag carry, so it
            // is the dispatch the parity surface sees — not the raw annotation.
            match self.node_dispatch.get(&MethodId { module, local_id: entry.local_id }) {
                Some(d) => {
                    true.hash(&mut hasher);
                    d.can_run_on_client.hash(&mut hasher);
                    d.can_run_on_server.hash(&mut hasher);
                    d.no_context.hash(&mut hasher);
                }
                None => false.hash(&mut hasher),
            }
        }
        Some(hasher.finish())
    }

    /// A module's methods as `(original-spelling name, is_export)` in declaration
    /// order, for the incremental caller-delta eligibility check (comparing the
    /// resolvable name surface across an edit). `None` if the module is not indexed.
    pub fn module_methods(&self, module: ModuleId) -> Option<Vec<(String, bool)>> {
        Some(
            self.methods
                .get(&module)?
                .all
                .iter()
                .map(|e| (e.name.as_str().to_string(), e.is_export))
                .collect(),
        )
    }

    /// Every indexed method node. This is exactly the fold's Pass-1 dispatch-seeded
    /// set (`node_dispatch.keys`), so a streaming build that materialises a node for
    /// each yields the same isolated (call-free) method nodes the in-memory graph's
    /// [`nodes`](WorkspaceCallGraph::nodes) exposes, not only edge endpoints.
    pub fn method_nodes(&self) -> impl Iterator<Item = MethodId> + '_ {
        self.node_dispatch.keys().copied()
    }

    /// One module's method declaration facts in declaration order, for a streaming
    /// consumer that needs each method's name/ranges/dispatch without re-scanning the
    /// whole-workspace node set per module. `None` if the module is not indexed.
    pub fn module_method_entries(&self, module: ModuleId) -> Option<&[GraphMethodEntry]> {
        self.methods.get(&module).map(|m| m.all.as_slice())
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
                        // A user manager-module method on a fully-literal
                        // `Коллекция.Объект.Метод()` path: the object name is a token and its
                        // manager module is uniquely determined, so locating the exported method
                        // is a direct lookup — as trustworthy as a qualified `Модуль.Метод()`
                        // call. The edge is about the method.
                        Some(m) if m.is_export => (
                            ResolvedTarget::Method(MethodId {
                                module: target_module,
                                local_id: m.local_id,
                            }),
                            EdgeProvenance::Resolved,
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
            // A `Движения.<Регистр>` movement touch: resolve the register name to its
            // metadata type from config, identical to the Salsa fold so the graphs match.
            CallTarget::RegisterMovement { register_name } => {
                match resolver.resolve_register_by_name(db, register_name) {
                    Some((mdo_type, object_name)) => (
                        ResolvedTarget::Mdo { mdo_type, object_name },
                        EdgeProvenance::Inferred,
                        EdgeKind::RegisterMovement,
                    ),
                    None => (
                        ResolvedTarget::Unresolved(edge.target.clone()),
                        EdgeProvenance::Unresolved,
                        edge.kind,
                    ),
                }
            }
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

    // Resolve string-dispatched callbacks through the resident index, mirroring the
    // qualified-call strategy above so the result is byte-identical to the Salsa fold.
    let find_local = |name: &crate::name::Name| {
        index.find_method(module, name).map(|m| MethodId { module, local_id: m.local_id })
    };
    let find_qualified =
        |module_name: &crate::name::Name, method_name: &crate::name::Name| match resolver
            .locate_common_module(db, module_name)
        {
            Ok(target_module) => match index.find_method(target_module, method_name) {
                Some(m) if m.is_export => crate::queries::QualifiedLookup::Resolved(MethodId {
                    module: target_module,
                    local_id: m.local_id,
                }),
                Some(_) => crate::queries::QualifiedLookup::VisibilityBlocked,
                None => crate::queries::QualifiedLookup::Absent,
            },
            Err(_) => crate::queries::QualifiedLookup::Absent,
        };
    let global_modules = resolver.global_common_module_names(db);
    edges.extend(crate::queries::resolve_callback_edges(
        &summary,
        find_local,
        find_qualified,
        &global_modules,
    ));

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

/// A qualified/manager call whose target MODULE resolved but whose METHOD did not
/// resolve to an exported method — the call sites a reverse index must remember so an
/// incremental rebuild can find the callers that would newly resolve if the target
/// gains (or exports) that method. Re-walks the Salsa-cached `module_call_summary`
/// (a memo hit in the build's batch db — no re-lowering) and re-runs only the cheap
/// locate+find resolution prefix, deliberately NOT touching the edge projection so
/// edge output stays byte-identical. The `Err` (module-not-found) cases are omitted:
/// a target module appearing requires a file/`.xml` add, which forces a full rebuild.
///
/// Returns `(target module, lowercased method name)`; the caller is `module`.
pub fn extract_unresolved_refs(
    db: &dyn ConfigsDatabase,
    module: ModuleId,
    index: &GraphIndex,
) -> Vec<(ModuleId, String)> {
    use crate::call_graph::CallTarget;

    let summary = db.module_call_summary(module);
    let resolver = Resolver::with_workspace_scope(module);
    let mut out = Vec::new();
    let unresolved = |m: Option<MethodRef>| !matches!(m, Some(r) if r.is_export);
    for edge in &summary.call_edges {
        match &edge.target {
            CallTarget::QualifiedModule { module_name, method_name } => {
                if let Ok(target) = resolver.locate_common_module(db, module_name) {
                    if unresolved(index.find_method(target, method_name)) {
                        out.push((target, method_name.as_str().to_lowercase()));
                    }
                }
            }
            CallTarget::ManagerAccess {
                manager_type,
                object_name,
                method_name: Some(method_name),
            } => {
                if let Ok(target) = resolver.locate_manager_module(db, *manager_type, object_name) {
                    if unresolved(index.find_method(target, method_name)) {
                        out.push((target, method_name.as_str().to_lowercase()));
                    }
                }
            }
            _ => {}
        }
    }
    // A `Новый ОписаниеОповещения("Метод", ОбщийМодуль)` whose handler is currently
    // missing or non-exported: record it so that exporting/adding the method later
    // triggers an incremental reproject of the callback edge. `ЭтотОбъект` and idle
    // handlers target the current module and are already covered by the module's own
    // `module_call_summary` invalidation.
    for reg in &summary.notify_regs {
        if let crate::call_graph::NotifyTarget::Module(module_name) = &reg.target {
            if let Ok(target) = resolver.locate_common_module(db, module_name) {
                if unresolved(index.find_method(target, &reg.callback_name)) {
                    out.push((target, reg.callback_name.as_str().to_lowercase()));
                }
            }
        }
    }
    out
}

/// A batch's call/manager edge projection plus the module-located-but-unresolved call
/// refs gathered in the same pass (caller, target module, lowercased method).
pub struct BatchCallProjection {
    pub edges: Vec<WorkspaceCallEdge>,
    pub unresolved: Vec<(ModuleId, ModuleId, String)>,
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
    /// Form data-binding intents gathered by the form pass, resolved against
    /// `catalog_index` and emitted as `DataBinding` edges in the final binding pass.
    form_bindings: Vec<FormBinding>,
    /// The metadata catalog indexed for binding resolution: per object, its canonical
    /// spelling and its declared (non-standard) fields / tabular-section columns with
    /// their metadata casing. Populated by the catalog pass as it emits nodes, so a
    /// binding's resolved to-id byte-matches the node the catalog emitted.
    catalog_index: CatalogIndex,
}

/// A form data-binding intent: a form node bound to a metadata object's structure.
/// `field_path` is empty for an object-level binding (`form_attribute → mdo`), `[field]`
/// for an object attribute, `[section, column]` for a tabular-section column.
struct FormBinding {
    from: GraphNode,
    target_mdo: MdoType,
    target_obj: String,
    field_path: Vec<String>,
}

type CatalogIndex = FxHashMap<(MdoType, String), CatalogEntry>;

/// One object's structure as the catalog emitted it: the canonical object spelling and
/// its fields / tabular sections keyed by lowercased name, each mapped to the
/// metadata-cased name used in the emitted node id.
struct CatalogEntry {
    object_name: crate::name::Name,
    attrs: FxHashMap<String, crate::name::Name>,
    sections: FxHashMap<String, CatalogSection>,
}

struct CatalogSection {
    section_name: crate::name::Name,
    cols: FxHashMap<String, crate::name::Name>,
}

impl GraphBuildState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Objects seen with more than one casing during the build, as
    /// `(EnglishType, lowercased object)`. An incremental rebuild refuses the
    /// body-only fast path for these — their cross-module first-seen ordering is not
    /// reconstructable from the canonicalised store alone.
    pub fn casing_variant_keys(&self) -> Vec<(&'static str, String)> {
        self.mdo_canonical
            .casing_variants()
            .map(|(ty, obj)| (ty.english_name(), obj.clone()))
            .collect()
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
pub fn project_batch_edges<DB: ConfigsDatabase + Clone + Send>(
    pool: &rayon::ThreadPool,
    db: &DB,
    batch: &[ModuleId],
    index: &GraphIndex,
    state: &mut GraphBuildState,
) -> Vec<WorkspaceCallEdge> {
    let mut edges = project_batch_call_edges(pool, db, batch, index, state).edges;
    edges.extend(project_batch_query_edges(pool, db, batch, state));
    edges
}

/// A batch's resolved call/manager edges, resolving cross-module targets through
/// the resident `index` (so the batch database needs only its own texts). Run this
/// across every batch before [`project_batch_query_edges`] to match the fold's
/// global Pass-2-then-Pass-3 canonicalization order. `state.mdo_canonical` is
/// shared and updated as new metadata-object spellings are first seen.
pub fn project_batch_call_edges<DB: ConfigsDatabase + Clone + Send>(
    pool: &rayon::ThreadPool,
    db: &DB,
    batch: &[ModuleId],
    index: &GraphIndex,
    state: &mut GraphBuildState,
) -> BatchCallProjection {
    // Resolve every module's summary in parallel, then project edges sequentially in
    // `batch` order so the shared `mdo_canonical` sees objects first-seen in the
    // exact order the fold does — parallelising only the read-only resolution keeps
    // the canonicalization deterministic. The unresolved-call refs are gathered in the
    // same parallel pass (a `module_call_summary` memo hit), so the index upkeep adds
    // no extra body lowering; edge projection is unchanged → edge output is identical.
    let results: Vec<_> = parallel_per_module(pool, db, batch, |db, module| {
        let summary = resolve_module_summary_via_index(db, module, index);
        let unresolved = extract_unresolved_refs(db, module, index);
        (summary, unresolved)
    });

    let mut edges = Vec::new();
    let mut unresolved = Vec::new();
    let dispatch = |node: &GraphNode| index.dispatch(node);
    for (summary, unres) in &results {
        edges.extend(crate::queries::project_module_call_edges(
            summary,
            &dispatch,
            &mut state.mdo_canonical,
        ));
        for (target, method_lower) in unres {
            unresolved.push((summary.module, *target, method_lower.clone()));
        }
    }
    BatchCallProjection { edges, unresolved }
}

/// Run `f` for every module in `batch` in parallel on `pool`, returning the results
/// in `batch` order. The database is `Send` but not `Sync` (a per-handle query
/// stack), so each rayon job works on its own cheap `db` clone — the clones share
/// the underlying memo storage. The work runs on the caller-supplied `pool`, never
/// the global one, so concurrent builds (each with its own pool and database) never
/// share a worker thread — Salsa attaches at most one database to any thread, and a
/// salsa query that itself parallelises (e.g. metadata loading) stays on this pool.
fn parallel_per_module<DB, R, F>(
    pool: &rayon::ThreadPool,
    db: &DB,
    batch: &[ModuleId],
    f: F,
) -> Vec<R>
where
    DB: ConfigsDatabase + Clone + Send,
    R: Send,
    F: Fn(&DB, ModuleId) -> R + Sync + Send,
{
    use rayon::prelude::*;

    // Warm the configuration loader ONCE on THIS thread before the parallel region.
    // `bsl_metadata::load_from_directory` (reached through the lru-cached
    // `load_configuration` query) fans out over its own `rayon::scope`; if it ran
    // inside a parallel job, a free worker could steal a sibling job — carrying a
    // different `db` clone — into that scope and attach a second database to a thread
    // mid-query, which Salsa forbids. `configurations` loads EVERY config root (it
    // iterates all config paths through the shared loader), so this single call
    // memoises the loader for all roots — base config plus extensions. The clones
    // share the `Zalsa`, so the per-module jobs below find the loader cached and
    // never open a nested scope; their own per-file metadata/visibility work runs in
    // the pool. It is the only internally parallel query the build reaches (no type
    // inference), so this one warm-up closes the window.
    if let Some(&first) = batch.first() {
        let _ = db.configurations(first.file_id);
        let _ = db.module_metadata(first);
    }

    let seed = db.clone();
    pool.install(move || batch.par_iter().map_with(seed, |db, &module| f(&*db, module)).collect())
}

/// A batch's SDBL `query_ref` edges. Run across every batch only after
/// [`project_batch_call_edges`] has run across all of them, sharing the same
/// `state`, so query-only metadata objects inherit the spelling the fold's Pass 3
/// would give them (call sites win first, exactly as in the fold).
pub fn project_batch_query_edges<DB: ConfigsDatabase + Clone + Send>(
    pool: &rayon::ThreadPool,
    db: &DB,
    batch: &[ModuleId],
    state: &mut GraphBuildState,
) -> Vec<WorkspaceCallEdge> {
    // Collect each module's query reads from its SDBL HIR in parallel (read-only),
    // then project edges sequentially in `batch` order so the shared
    // canonicalization/dedup matches the fold byte-for-byte.
    let collected: Vec<_> = parallel_per_module(pool, db, batch, |db, module| {
        crate::queries::collect_module_query_refs(db, module)
    });

    let mut edges = Vec::new();
    for refs in &collected {
        edges.extend(crate::queries::project_collected_query_edges(
            refs,
            &mut state.mdo_canonical,
            &mut state.seen_query_ref,
            &mut state.seen_query_attr,
        ));
    }
    edges
}

/// One form module's structural facts, gathered read-only for the form pass: its
/// path-derived key (owner + name) and the declared element names (deduped
/// case-insensitively, declaration order preserved).
struct CollectedForm {
    key: crate::module_index::FormKey,
    /// One per UI element, in declaration order: its name, its own element id, and
    /// its parent's element id (`None` for a root element).
    items: Vec<CollectedItem>,
    /// Declared form-attribute names (the form's data model), deduped
    /// case-insensitively in declaration order.
    attributes: Vec<crate::name::Name>,
    /// Data-binding intents: a Ref-typed form attribute (object-level) or a data-bound
    /// UI element (field-level). Resolved against the catalog in the binding pass.
    bindings: Vec<CollectedBinding>,
}

struct CollectedItem {
    name: crate::name::Name,
    id: u32,
    parent_id: Option<u32>,
}

/// The form-side endpoint of a data binding, before the canonical owner/form is known.
enum BindingFrom {
    /// A Ref-typed form attribute (object-level binding to its backing object).
    Attr(crate::name::Name),
    /// A data-bound UI element (field-level binding via its data path).
    Item(crate::name::Name),
}

struct CollectedBinding {
    from: BindingFrom,
    mdo: MdoType,
    obj: String,
    /// Empty for an object-level binding; the data-path segments after the form
    /// attribute for a field-level one.
    field_path: Vec<String>,
}

/// A batch's `contains` edges derived from form metadata: `mdo → form` (object forms
/// only) and `form → form_item` for each declared element. Run across every batch
/// only AFTER the call/query passes, sharing the same `state`, so a form's owner
/// object inherits the canonical spelling the call/query passes assigned (code sites
/// win first; a divergent form-directory casing is recorded as a casing variant just
/// like any other).
///
/// Emitted as edges only — the `Form`/`FormItem`/`Mdo` nodes fall out as edge
/// endpoints in the build driver, exactly as `Mdo`/`Attribute` nodes do. A common
/// form (no owner) with no elements therefore produces no edges and is not
/// represented; such a form carries no structural information.
///
/// This pass is run ONLY by the full build. The incremental reprojection leaves form
/// nodes/edges untouched (form structure comes from form XML, so any form-structure
/// change is a metadata drift that already forces a full rebuild).
pub fn project_batch_form_edges<DB: ConfigsDatabase + Clone + Send>(
    pool: &rayon::ThreadPool,
    db: &DB,
    batch: &[ModuleId],
    paths: &FxHashMap<FileId, String>,
    state: &mut GraphBuildState,
) -> Vec<WorkspaceCallEdge> {
    let collected: Vec<Option<CollectedForm>> =
        parallel_per_module(pool, db, batch, |db, module| {
            let path = paths.get(&module.file_id)?;
            let key = crate::module_index::parse_form_module_path(path)?;
            let metadata = db.module_metadata(module);
            let form = metadata.form.as_ref()?;
            // Keep every element (no dedup here) so the parent-id map below can
            // resolve any `parent_id`, even one pointing at a same-named sibling.
            let items = form
                .elements
                .iter()
                .map(|element| CollectedItem {
                    name: crate::name::Name::new(&element.name),
                    id: element.id,
                    parent_id: element.parent_id,
                })
                .collect();
            let mut attributes = Vec::new();
            let mut seen_attr = FxHashSet::default();
            let mut bindings = Vec::new();
            // `Ref`-typed form attributes back an object; index the survivor of each
            // name so a data path's first segment can resolve to its object, and emit
            // an object-level binding for it.
            let mut attr_backing: FxHashMap<String, (MdoType, String)> = FxHashMap::default();
            for attr in &form.attributes {
                let name = crate::name::Name::new(&attr.name);
                let lower = name.as_str().to_lowercase();
                if seen_attr.insert(lower.clone()) {
                    attributes.push(name.clone());
                    if let bsl_metadata::AttributeType::Ref { mdo_type, name: obj } =
                        &attr.attr_type
                    {
                        attr_backing.insert(lower, (*mdo_type, obj.clone()));
                        bindings.push(CollectedBinding {
                            from: BindingFrom::Attr(name),
                            mdo: *mdo_type,
                            obj: obj.clone(),
                            field_path: Vec::new(),
                        });
                    }
                }
            }
            // A UI element whose data path is `<реквизит>.<поле>[.<колонка>]` binds to
            // that field of the form attribute's backing object. A leading `~` marks a
            // broken path; bare `<реквизит>` has no field.
            for element in &form.elements {
                let Some(dp) = element.data_path.as_deref() else { continue };
                if dp.starts_with('~') {
                    continue;
                }
                let mut segs = dp.split('.');
                let Some(seg0) = segs.next() else { continue };
                let Some((mdo, obj)) = attr_backing.get(&seg0.to_lowercase()) else { continue };
                let field_path: Vec<String> = segs.map(str::to_string).collect();
                if field_path.is_empty() {
                    continue;
                }
                bindings.push(CollectedBinding {
                    from: BindingFrom::Item(crate::name::Name::new(&element.name)),
                    mdo: *mdo,
                    obj: obj.clone(),
                    field_path,
                });
            }
            Some(CollectedForm { key, items, attributes, bindings })
        });

    let mut edges = Vec::new();
    for form in collected.iter().flatten() {
        let form_name = crate::name::Name::new(&form.key.form_name);
        // Canonicalise the owner object so the form's `mdo` parent unifies with the
        // call/query-derived `Mdo` node for the same object.
        let owner = form.key.owner.as_ref().map(|(mdo_type, object)| {
            (*mdo_type, state.mdo_canonical.canonical(*mdo_type, object))
        });
        let form_node = GraphNode::Form { owner: owner.clone(), form_name: form_name.clone() };
        if let Some((mdo_type, object_name)) = &owner {
            edges.push(contains_edge(
                GraphNode::Mdo { mdo_type: *mdo_type, object_name: object_name.clone() },
                form_node.clone(),
            ));
        }
        let form_item = |item_name: crate::name::Name| GraphNode::FormItem {
            owner: owner.clone(),
            form_name: form_name.clone(),
            item_name,
        };
        // `form_item` nodes are keyed by name, so the surviving node for a name is
        // the first element declaring it. Map id → name over every element, and
        // record which id is that survivor per lowercased name. A `parent_id` is
        // only honoured when it points at the surviving element for its name — if the
        // real parent was a same-named element that collapsed into another node, its
        // name now denotes a different element, so the child hangs off the form root.
        let id_to_name: FxHashMap<u32, &crate::name::Name> =
            form.items.iter().map(|item| (item.id, &item.name)).collect();
        let mut survivor_id: FxHashMap<String, u32> = FxHashMap::default();
        // The surviving `form_item` node for a name is the first element declaring it;
        // its spelling is the one the node carries. A field-level binding must point at
        // that survivor spelling, not a later same-name element's own casing.
        let mut survivor_name: FxHashMap<String, &crate::name::Name> = FxHashMap::default();
        for item in &form.items {
            let lower = item.name.as_str().to_lowercase();
            survivor_id.entry(lower.clone()).or_insert(item.id);
            survivor_name.entry(lower).or_insert(&item.name);
        }
        let mut seen_item = FxHashSet::default();
        for item in &form.items {
            if !seen_item.insert(item.name.as_str().to_lowercase()) {
                continue;
            }
            let parent = item.parent_id.and_then(|pid| Some((pid, *id_to_name.get(&pid)?))).filter(
                |(pid, parent_name)| {
                    !parent_name.as_str().eq_ignore_ascii_case(item.name.as_str())
                        && survivor_id.get(&parent_name.as_str().to_lowercase()) == Some(pid)
                },
            );
            let parent_node = match parent {
                Some((_, parent_name)) => form_item(parent_name.clone()),
                None => form_node.clone(),
            };
            edges.push(contains_edge(parent_node, form_item(item.name.clone())));
        }
        for attr in &form.attributes {
            edges.push(contains_edge(
                form_node.clone(),
                GraphNode::FormAttribute {
                    owner: owner.clone(),
                    form_name: form_name.clone(),
                    attr_name: attr.clone(),
                },
            ));
        }
        // Stash data-binding intents with the from-node fully built; the binding pass
        // resolves their targets once the catalog index is complete.
        for binding in &form.bindings {
            let from = match &binding.from {
                BindingFrom::Attr(name) => GraphNode::FormAttribute {
                    owner: owner.clone(),
                    form_name: form_name.clone(),
                    attr_name: name.clone(),
                },
                BindingFrom::Item(name) => {
                    let survivor =
                        survivor_name.get(&name.as_str().to_lowercase()).copied().unwrap_or(name);
                    form_item(survivor.clone())
                }
            };
            state.form_bindings.push(FormBinding {
                from,
                target_mdo: binding.mdo,
                target_obj: binding.obj.clone(),
                field_path: binding.field_path.clone(),
            });
        }
    }
    edges
}

/// One metadata object's declared structure, gathered for the catalog pass.
struct CatalogObject {
    mdo_type: MdoType,
    name: String,
    /// Top-level attribute names (object attributes, or register
    /// dimensions/resources/attributes), declaration order.
    attrs: Vec<String>,
    /// Tabular sections, each with its column names. Empty for registers.
    sections: Vec<(String, Vec<String>)>,
}

/// The whole metadata catalog as `contains` edges: `mdo → attribute` (object
/// attributes + register dimensions/resources/attributes), `mdo → tabular_section`,
/// and `tabular_section → attribute` (the section column). Driven by the metadata
/// catalog — **every** object in every visible configuration, whether or not code
/// references it — so the structural node set is stable under body edits and an
/// incremental update (which never runs this pass) stays byte-identical to a full
/// rebuild. Any metadata/`.xml` change already forces a full rebuild.
///
/// Run ONCE on the driver thread after the call/query/form passes, sharing `state`
/// so an object's `mdo` node inherits the canonical spelling code sites assigned (a
/// metadata-only object is first-seen here). Objects are visited in a deterministic
/// `(english type, lowercased name)` order so first-seen canonicalisation and the
/// emitted edge set never depend on configuration load order. The union across base +
/// extension configurations is by node identity: a duplicated object/attribute/column
/// dedups, so an extension that adds attributes to a base object contributes only its
/// new ones.
pub fn project_workspace_catalog_edges<DB: ConfigsDatabase>(
    db: &DB,
    representative: FileId,
    state: &mut GraphBuildState,
) -> Vec<WorkspaceCallEdge> {
    // Platform standard attributes (Ссылка/Код/Наименование/…) are synthesised onto
    // every object and carry no configuration-specific structure; exclude them so the
    // catalog covers exactly the user-declared attributes (the same standard-field
    // exclusion the query-ref pass applies). `is_standard_attribute_name` is derived
    // from `StandardAttributeKind`, the enum the synthesiser builds them from.
    let is_standard = bsl_metadata::is_standard_attribute_name;

    let mut objects: Vec<CatalogObject> = Vec::new();
    for visible in db.configurations(representative) {
        let config = &visible.configuration;
        for mdo in config.metadata_objects() {
            objects.push(CatalogObject {
                mdo_type: mdo.mdo_type,
                name: mdo.name.clone(),
                attrs: mdo
                    .attributes
                    .iter()
                    .map(|a| a.name.clone())
                    .filter(|n| !is_standard(n))
                    .collect(),
                sections: mdo
                    .tabular_sections
                    .iter()
                    .map(|ts| {
                        (
                            ts.name().to_string(),
                            ts.attributes().iter().map(|c| c.name().to_string()).collect(),
                        )
                    })
                    .collect(),
            });
        }
        for reg in config.registers() {
            let mut attrs = Vec::new();
            // Dimensions and resources are always user-declared; only the register's
            // `attributes` bucket can hold synthesised standard fields (Период, …).
            attrs.extend(reg.dimensions().iter().map(|d| d.name().to_string()));
            attrs.extend(reg.resources().iter().map(|r| r.name().to_string()));
            attrs.extend(
                reg.attributes().iter().map(|a| a.name().to_string()).filter(|n| !is_standard(n)),
            );
            objects.push(CatalogObject {
                mdo_type: reg.mdo_type(),
                name: reg.name().to_string(),
                attrs,
                sections: Vec::new(),
            });
        }
    }
    // Deterministic visitation regardless of configuration load order. The original
    // spelling is the final tiebreaker so that two objects sharing a lowercased name
    // across configs (e.g. base + extension with different casing) always yield the
    // same first-seen canonical spelling, independent of base-vs-extension load order.
    objects.sort_by(|a, b| {
        a.mdo_type
            .english_name()
            .cmp(b.mdo_type.english_name())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut edges = Vec::new();
    let mut seen_attr: FxHashSet<(MdoType, String, String)> = FxHashSet::default();
    let mut seen_ts: FxHashSet<(MdoType, String, String)> = FxHashSet::default();
    let mut seen_ts_attr: FxHashSet<(MdoType, String, String, String)> = FxHashSet::default();
    for obj in &objects {
        let object_name = state.mdo_canonical.canonical(obj.mdo_type, &obj.name);
        let key = object_name.as_str().to_lowercase();
        let mdo = GraphNode::Mdo { mdo_type: obj.mdo_type, object_name: object_name.clone() };
        // Index this object for form data-binding resolution. First-wins on each
        // lowercased name mirrors the dedup below, so the indexed (metadata-cased) name
        // is the one the emitted node carries.
        let entry = state.catalog_index.entry((obj.mdo_type, key.clone())).or_insert_with(|| {
            CatalogEntry {
                object_name: object_name.clone(),
                attrs: FxHashMap::default(),
                sections: FxHashMap::default(),
            }
        });
        for attr in &obj.attrs {
            entry.attrs.entry(attr.to_lowercase()).or_insert_with(|| crate::name::Name::new(attr));
        }
        for (section, cols) in &obj.sections {
            let sec =
                entry.sections.entry(section.to_lowercase()).or_insert_with(|| CatalogSection {
                    section_name: crate::name::Name::new(section),
                    cols: FxHashMap::default(),
                });
            for col in cols {
                sec.cols.entry(col.to_lowercase()).or_insert_with(|| crate::name::Name::new(col));
            }
        }
        for attr in &obj.attrs {
            if seen_attr.insert((obj.mdo_type, key.clone(), attr.to_lowercase())) {
                edges.push(contains_edge(
                    mdo.clone(),
                    GraphNode::Attribute {
                        mdo_type: obj.mdo_type,
                        object_name: object_name.clone(),
                        attr_name: crate::name::Name::new(attr),
                    },
                ));
            }
        }
        for (section, cols) in &obj.sections {
            let section_lower = section.to_lowercase();
            let ts = GraphNode::TabularSection {
                mdo_type: obj.mdo_type,
                object_name: object_name.clone(),
                section_name: crate::name::Name::new(section),
            };
            if seen_ts.insert((obj.mdo_type, key.clone(), section_lower.clone())) {
                edges.push(contains_edge(mdo.clone(), ts.clone()));
            }
            for col in cols {
                if seen_ts_attr.insert((
                    obj.mdo_type,
                    key.clone(),
                    section_lower.clone(),
                    col.to_lowercase(),
                )) {
                    edges.push(contains_edge(
                        ts.clone(),
                        GraphNode::TabularSectionAttribute {
                            mdo_type: obj.mdo_type,
                            object_name: object_name.clone(),
                            section_name: crate::name::Name::new(section),
                            attr_name: crate::name::Name::new(col),
                        },
                    ));
                }
            }
        }
    }
    edges
}

/// Project event-subscription handler edges: each `ПодпискаНаСобытие` links its
/// subscription node (`Mdo{EventSubscription, name}`) to the exported common-module
/// method named in its handler. Config-level and full-build only, like the catalog
/// pass — the only edits that can invalidate such an edge (the handler method being
/// added, removed, renamed, or its `Экспорт` toggled) all change the handler module's
/// [`GraphIndex::module_sig_hash`], which fails the body-only precondition and forces a
/// full rebuild rather than a body-only reproject. A handler that does not resolve to an
/// exported method yields no edge (and hence no subscription node), mirroring every
/// other unresolved target.
pub fn project_workspace_subscription_edges<DB: ConfigsDatabase>(
    db: &DB,
    representative: FileId,
    index: &GraphIndex,
    state: &mut GraphBuildState,
) -> Vec<WorkspaceCallEdge> {
    // Resolve the handler's common module by name through the source-root module index,
    // not the visibility-gated resolver: a subscription's handler is named by
    // configuration metadata and is referenced regardless of code-visibility scoping,
    // matching the `MissingEventSubscriptionHandler` diagnostic's "anywhere" lookup.
    let source_root_id = db.file_source_root_input(representative).source_root_id(db);
    let module_index = db.module_index(source_root_id);

    // Collect (subscription, handler module, handler method) deterministically so the
    // shared canonicalization sees a load-order-independent first-seen spelling.
    let mut subs: Vec<(String, crate::name::Name, crate::name::Name)> = Vec::new();
    for visible in db.configurations(representative) {
        for sub in visible.configuration.event_subscriptions() {
            let Some(handler) = sub.parse_handler() else { continue };
            if handler.method_name.is_empty() {
                continue;
            }
            subs.push((
                sub.name().to_string(),
                crate::name::Name::new(&handler.module_name),
                crate::name::Name::new(&handler.method_name),
            ));
        }
    }
    subs.sort();
    subs.dedup();

    let mut edges = Vec::new();
    let mut seen: FxHashSet<(GraphNode, GraphNode)> = FxHashSet::default();
    for (sub_name, module_name, method_name) in &subs {
        let Some(handler_file) = module_index.resolve_common_module(module_name) else { continue };
        let module_id = ModuleId::new(handler_file);
        let Some(m) = index.find_method(module_id, method_name) else { continue };
        if !m.is_export {
            continue;
        }
        let object_name = state.mdo_canonical.canonical(MdoType::EventSubscription, sub_name);
        let from = GraphNode::Mdo { mdo_type: MdoType::EventSubscription, object_name };
        let to = GraphNode::Method(MethodId { module: module_id, local_id: m.local_id });
        if seen.insert((from.clone(), to.clone())) {
            edges.push(WorkspaceCallEdge {
                from,
                to,
                kind: EdgeKind::EventSubscriptionRef,
                provenance: EdgeProvenance::StringResolved,
                crosses_client_to_server: false,
            });
        }
    }
    edges
}

/// Project subsystem membership into edges: from each subsystem's `Mdo` node to every
/// member metadata object it contains and to every child subsystem. Pure config-driven
/// (like the catalog and subscription passes), no body lowering. Member/child node names
/// are canonicalized through the shared `mdo_canonical` so they coincide with the object's
/// own nodes from other passes.
pub fn project_workspace_subsystem_edges<DB: ConfigsDatabase>(
    db: &DB,
    representative: FileId,
    state: &mut GraphBuildState,
) -> Vec<WorkspaceCallEdge> {
    // Collect deterministically so canonicalization sees a load-order-independent
    // first-seen spelling.
    let mut members: Vec<(String, MdoType, String)> = Vec::new();
    let mut children: Vec<(String, String)> = Vec::new();
    for visible in db.configurations(representative) {
        for subsystem in visible.configuration.subsystems() {
            for (mdo_type, member_name) in subsystem.content() {
                members.push((subsystem.name().to_string(), *mdo_type, member_name.clone()));
            }
            for child in subsystem.child_subsystems() {
                children.push((subsystem.name().to_string(), child.clone()));
            }
        }
    }
    members.sort();
    members.dedup();
    children.sort();
    children.dedup();

    let mut edges = Vec::new();
    let mut seen: FxHashSet<(GraphNode, GraphNode)> = FxHashSet::default();

    for (sub_name, mdo_type, member_name) in &members {
        let from = GraphNode::Mdo {
            mdo_type: MdoType::Subsystem,
            object_name: state.mdo_canonical.canonical(MdoType::Subsystem, sub_name),
        };
        let to = GraphNode::Mdo {
            mdo_type: *mdo_type,
            object_name: state.mdo_canonical.canonical(*mdo_type, member_name),
        };
        if seen.insert((from.clone(), to.clone())) {
            edges.push(subsystem_membership_edge(from, to));
        }
    }
    for (sub_name, child_name) in &children {
        let from = GraphNode::Mdo {
            mdo_type: MdoType::Subsystem,
            object_name: state.mdo_canonical.canonical(MdoType::Subsystem, sub_name),
        };
        let to = GraphNode::Mdo {
            mdo_type: MdoType::Subsystem,
            object_name: state.mdo_canonical.canonical(MdoType::Subsystem, child_name),
        };
        if seen.insert((from.clone(), to.clone())) {
            edges.push(subsystem_membership_edge(from, to));
        }
    }
    edges
}

fn subsystem_membership_edge(from: GraphNode, to: GraphNode) -> WorkspaceCallEdge {
    WorkspaceCallEdge {
        from,
        to,
        kind: EdgeKind::SubsystemMembership,
        provenance: EdgeProvenance::Resolved,
        crosses_client_to_server: false,
    }
}

fn contains_edge(from: GraphNode, to: GraphNode) -> WorkspaceCallEdge {
    WorkspaceCallEdge {
        from,
        to,
        kind: EdgeKind::Contains,
        provenance: EdgeProvenance::Resolved,
        crosses_client_to_server: false,
    }
}

fn data_binding_edge(from: GraphNode, to: GraphNode) -> WorkspaceCallEdge {
    WorkspaceCallEdge {
        from,
        to,
        kind: EdgeKind::DataBinding,
        provenance: EdgeProvenance::Resolved,
        crosses_client_to_server: false,
    }
}

/// Resolve the form data-bindings gathered by the form pass against the catalog index
/// built by the catalog pass, emitting `DataBinding` edges. Pure (no database): run on
/// the driver thread AFTER the catalog pass so `state.catalog_index` is complete.
///
/// Each binding's target object is looked up in the catalog — only objects that the
/// catalog actually emitted (and, for a field/column binding, only declared non-standard
/// fields) produce an edge, so a `DataBinding` edge can never dangle. The catalog also
/// supplies the canonical object spelling and the metadata-cased field/section/column
/// names, so the to-id byte-matches the node the catalog pass emitted. Full-build only,
/// like the form and catalog passes it depends on.
pub fn project_form_binding_edges(state: &GraphBuildState) -> Vec<WorkspaceCallEdge> {
    let mut edges = Vec::new();
    let mut seen: FxHashSet<(GraphNode, GraphNode)> = FxHashSet::default();
    for binding in &state.form_bindings {
        let Some(entry) =
            state.catalog_index.get(&(binding.target_mdo, binding.target_obj.to_lowercase()))
        else {
            continue;
        };
        let to = match binding.field_path.as_slice() {
            // Ref-typed form attribute → the whole backing object.
            [] => GraphNode::Mdo {
                mdo_type: binding.target_mdo,
                object_name: entry.object_name.clone(),
            },
            // `Объект.<поле>` → an object attribute.
            [field] => {
                let Some(attr) = entry.attrs.get(&field.to_lowercase()) else { continue };
                GraphNode::Attribute {
                    mdo_type: binding.target_mdo,
                    object_name: entry.object_name.clone(),
                    attr_name: attr.clone(),
                }
            }
            // `Объект.<ТЧ>.<колонка>` → a tabular-section column.
            [section, column] => {
                let Some(sec) = entry.sections.get(&section.to_lowercase()) else { continue };
                let Some(col) = sec.cols.get(&column.to_lowercase()) else { continue };
                GraphNode::TabularSectionAttribute {
                    mdo_type: binding.target_mdo,
                    object_name: entry.object_name.clone(),
                    section_name: sec.section_name.clone(),
                    attr_name: col.clone(),
                }
            }
            // Deeper ref-chains are not resolved.
            _ => continue,
        };
        if seen.insert((binding.from.clone(), to.clone())) {
            edges.push(data_binding_edge(binding.from.clone(), to));
        }
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
        EdgeKind::Contains => "contains",
        EdgeKind::DataBinding => "data_binding",
        EdgeKind::NotifyRef => "notify_ref",
        EdgeKind::IdleHandler => "idle_handler",
        EdgeKind::EventSubscriptionRef => "event_subscription",
        EdgeKind::RegisterMovement => "register_movement",
        EdgeKind::SubsystemMembership => "subsystem_membership",
    }
}

/// The durable id scope segment for a form's owner: `<EnglishType>/<Object>` for an
/// object-owned form, or `common` for a common form. Shared with `ide::graph`'s
/// serving path so build-time ids and serve-time ids agree.
pub fn form_scope(owner: &Option<(MdoType, crate::name::Name)>) -> String {
    match owner {
        Some((mdo_type, object_name)) => {
            format!("{}/{}", mdo_type.english_name(), object_name.as_str())
        }
        None => "common".to_string(),
    }
}

/// The human-facing qualified-name prefix for a form's owner. Shared with
/// `ide::graph`'s serving path so build-time and serve-time `qualified` agree.
pub fn form_qualified_prefix(owner: &Option<(MdoType, crate::name::Name)>) -> String {
    match owner {
        Some((mdo_type, object_name)) => {
            format!("{}.{}", mdo_type.russian_name(), object_name.as_str())
        }
        None => "ОбщаяФорма".to_string(),
    }
}

fn provenance_label(p: EdgeProvenance) -> &'static str {
    match p {
        EdgeProvenance::Resolved => "resolved",
        EdgeProvenance::Inferred => "inferred",
        EdgeProvenance::VisibilityBlocked => "visibility_blocked",
        EdgeProvenance::Unresolved => "unresolved",
        EdgeProvenance::StringResolved => "string_resolved",
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
    /// Byte offset of the declaration name token — the start of the signature.
    pub name_offset: Option<u32>,
    /// Byte offset of the declaration header end (closing `)` or export keyword) —
    /// the end of the full, possibly multi-line, signature slice (method nodes only).
    pub sig_end: Option<u32>,
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
            GraphNode::Form { owner, form_name } => {
                (format!("form/{}/{}", form_scope(owner), form_name.as_str()), true)
            }
            GraphNode::FormItem { owner, form_name, item_name } => (
                format!(
                    "form_item/{}/{}/{}",
                    form_scope(owner),
                    form_name.as_str(),
                    item_name.as_str()
                ),
                true,
            ),
            GraphNode::FormAttribute { owner, form_name, attr_name } => (
                format!(
                    "form_attr/{}/{}/{}",
                    form_scope(owner),
                    form_name.as_str(),
                    attr_name.as_str()
                ),
                true,
            ),
            GraphNode::TabularSection { mdo_type, object_name, section_name } => (
                format!(
                    "tabular_section/{}/{}/{}",
                    mdo_type.english_name(),
                    object_name.as_str(),
                    section_name.as_str()
                ),
                true,
            ),
            GraphNode::TabularSectionAttribute {
                mdo_type,
                object_name,
                section_name,
                attr_name,
            } => (
                format!(
                    "ts_attr/{}/{}/{}/{}",
                    mdo_type.english_name(),
                    object_name.as_str(),
                    section_name.as_str(),
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
                    sig_end: entry.map(|e| e.sig_end.into()),
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
                    sig_end: None,
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
                sig_end: None,
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
                sig_end: None,
                src_start: None,
                src_end: None,
                dispatch: Vec::new(),
                is_export: None,
                addressable,
            },
            GraphNode::Form { owner, form_name } => NodeRow {
                id,
                kind: "form",
                name: form_name.as_str().to_string(),
                qualified: format!("{}.Форма.{}", form_qualified_prefix(owner), form_name.as_str()),
                module: None,
                file: None,
                name_offset: None,
                sig_end: None,
                src_start: None,
                src_end: None,
                dispatch: Vec::new(),
                is_export: None,
                addressable,
            },
            GraphNode::FormItem { owner, form_name, item_name } => NodeRow {
                id,
                kind: "form_item",
                name: item_name.as_str().to_string(),
                qualified: format!(
                    "{}.Форма.{}.{}",
                    form_qualified_prefix(owner),
                    form_name.as_str(),
                    item_name.as_str()
                ),
                module: None,
                file: None,
                name_offset: None,
                sig_end: None,
                src_start: None,
                src_end: None,
                dispatch: Vec::new(),
                is_export: None,
                addressable,
            },
            GraphNode::FormAttribute { owner, form_name, attr_name } => NodeRow {
                id,
                kind: "form_attribute",
                name: attr_name.as_str().to_string(),
                qualified: format!(
                    "{}.Форма.{}.Реквизит.{}",
                    form_qualified_prefix(owner),
                    form_name.as_str(),
                    attr_name.as_str()
                ),
                module: None,
                file: None,
                name_offset: None,
                sig_end: None,
                src_start: None,
                src_end: None,
                dispatch: Vec::new(),
                is_export: None,
                addressable,
            },
            GraphNode::TabularSection { mdo_type, object_name, section_name } => NodeRow {
                id,
                kind: "tabular_section",
                name: section_name.as_str().to_string(),
                qualified: format!(
                    "{}.{}.ТабличнаяЧасть.{}",
                    mdo_type.russian_name(),
                    object_name.as_str(),
                    section_name.as_str()
                ),
                module: None,
                file: None,
                name_offset: None,
                sig_end: None,
                src_start: None,
                src_end: None,
                dispatch: Vec::new(),
                is_export: None,
                addressable,
            },
            GraphNode::TabularSectionAttribute {
                mdo_type,
                object_name,
                section_name,
                attr_name,
            } => NodeRow {
                id,
                kind: "attribute",
                name: attr_name.as_str().to_string(),
                qualified: format!(
                    "{}.{}.{}.{}",
                    mdo_type.russian_name(),
                    object_name.as_str(),
                    section_name.as_str(),
                    attr_name.as_str()
                ),
                module: None,
                file: None,
                name_offset: None,
                sig_end: None,
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
