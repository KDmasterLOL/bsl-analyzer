use std::sync::Arc;

use base_db::FileIdInput;
use hir::ModuleId;

use crate::{
    metadata::{intern_configuration_path, ConfigurationPathInput},
    RootDatabase,
};

pub use crate::metadata::load_configuration;

/// `heap_size` estimators wired into Salsa's `memory_usage` report. Each returns an
/// approximate live-heap byte count for the query's memoised output (hashbrown
/// table capacity derived from length at load factor 7/8, owned `Vec` payloads
/// summed). Accessor queries that return a clone of an `Arc` already owned by a
/// module-level query report zero, so the shared payload is counted exactly once.
pub(crate) mod heap_estimate {
    use std::sync::Arc;

    use super::{ModuleCyclomatic, ModuleHirMetrics};

    /// Heap of a `Vec` backing store holding `len` elements (spare capacity ignored).
    pub(crate) fn vec_bytes<T>(len: usize) -> usize {
        len * std::mem::size_of::<T>()
    }

    /// Approximate live bytes of an `FxHashMap`/hashbrown table with `len` entries
    /// of `(K, V)`: one control byte plus the `(K, V)` slot per bucket, bucket count
    /// grown to the next power of two above `len / (7/8)`.
    pub(crate) fn map_table_bytes<K, V>(len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        let cap = (len * 8 / 7 + 1).checked_next_power_of_two().unwrap_or(len);
        cap.saturating_mul(std::mem::size_of::<K>() + std::mem::size_of::<V>() + 1)
    }

    pub(super) fn cfg_heap(v: &Arc<hir::cfg::ControlFlowGraph>) -> usize {
        v.estimated_heap()
    }

    pub(super) fn module_level_cfg_heap(v: &Arc<hir::cfg::ControlFlowGraph>) -> usize {
        v.estimated_heap()
    }

    /// The result shares its `Body` with the lowering memo, and its own
    /// estimate leaves the body out.
    pub(super) fn reaching_defs_heap(
        v: &Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>>,
    ) -> usize {
        v.as_ref().map_or(0, |r| r.estimated_heap())
    }

    pub(super) fn path_terminates_heap(
        v: &Option<Arc<hir::dataflow::path_terminates::PathTerminatesResult>>,
    ) -> usize {
        v.as_ref().map_or(0, |r| r.estimated_heap())
    }

    pub(super) fn line_index_heap(v: &Arc<line_index::LineIndex>) -> usize {
        v.estimated_heap()
    }

    /// Per-method payloads are owned by `method_hir_metrics_query`; the file
    /// view counts its table and the module-code metrics it owns.
    pub(super) fn module_hir_metrics_heap(v: &Arc<ModuleHirMetrics>) -> usize {
        let mut bytes =
            map_table_bytes::<hir::MethodKey, Arc<hir::metrics::HirMethodMetrics>>(v.methods.len());
        if let Some(hm) = &v.module_code {
            bytes += std::mem::size_of::<hir::metrics::HirMethodMetrics>()
                + vec_bytes::<hir::metrics::ConditionMetrics>(hm.if_conditions.len())
                + vec_bytes::<hir::metrics::NestingLeafMetrics>(hm.nesting_leaves.len());
        }
        bytes
    }

    pub(super) fn hir_metrics_heap(v: &Arc<hir::metrics::HirMethodMetrics>) -> usize {
        std::mem::size_of::<hir::metrics::HirMethodMetrics>()
            + vec_bytes::<hir::metrics::ConditionMetrics>(v.if_conditions.len())
            + vec_bytes::<hir::metrics::NestingLeafMetrics>(v.nesting_leaves.len())
    }

    pub(super) fn recursive_methods_heap(v: &Arc<rustc_hash::FxHashSet<hir::MethodKey>>) -> usize {
        map_table_bytes::<hir::MethodKey, ()>(v.len())
    }

    pub(super) fn module_cyclomatic_heap(v: &Arc<ModuleCyclomatic>) -> usize {
        map_table_bytes::<hir::MethodKey, u32>(v.methods.len())
    }

    pub(super) fn module_metadata_heap(v: &Arc<hir::ModuleMetadata>) -> usize {
        std::mem::size_of::<hir::ModuleMetadata>() + v.estimated_heap_size()
    }
}

pub fn configuration_path_for_file<'db>(
    db: &'db dyn RootDatabase,
    file_id: vfs::FileId,
) -> Option<ConfigurationPathInput<'db>> {
    configuration_path_for_file_query(db, FileIdInput::new(db, file_id))
}

/// The configuration root of a file, found by walking its directory chain on
/// disk (`CommonModules/` or `Configuration.xml`). Memoised per file: the walk
/// lists directories — a common-modules folder of thousands of entries on a
/// large configuration — and the per-method diagnostics ask for it once per
/// body. The memo follows the file's source root and the configuration
/// revisions; a root that appears on disk without any registration is seen
/// on the next revision of those inputs, not on the next request.
#[salsa::tracked(heap_size = stdx::heap::zero, returns(copy))]
fn configuration_path_for_file_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Option<ConfigurationPathInput<'db>> {
    let file_id = file_id_input.file_id(db);
    let file_path = crate::vfs_helpers::get_file_path(db, file_id)?;
    let config_root = crate::vfs_helpers::find_configuration_root(db, &file_path)?;
    Some(intern_configuration_path(
        db,
        &config_root.to_string_lossy(),
        db.config_root_revision_for_path(&file_path),
    ))
}

/// Resolve the constructed base-module candidate: exactly first, then with the
/// module-path grammar's per-component case policy — the candidate carries the
/// EXTENSION's spelling of the conventional segments, while the base file may
/// spell them differently. Name positions stay exact (НУ-2).
fn resolve_pair_candidate(
    db: &dyn RootDatabase,
    source_root: base_db::SourceRootId,
    roots: &[(Option<String>, std::path::PathBuf)],
    base_path: &std::path::Path,
) -> Option<vfs::FileId> {
    let candidate = base_path.to_string_lossy().into_owned();
    let base_root = roots.iter().find_map(|(label, p)| label.is_none().then_some(p))?;
    let modes = base_path
        .strip_prefix(base_root)
        .ok()
        .and_then(|rel| hir::module_path_segment_modes(&rel.to_string_lossy()))
        .unwrap_or_default();
    base_db::resolve_vfs_path_ci_query(db, db.source_root_input(source_root), candidate, &modes)
}

/// The effective `&ИзменениеИКонтроль` module identity for an extension module file, or
/// `None` when the file is not an extension module, has no resolvable base counterpart, or
/// carries no usable change-and-validate splice (in which case ordinary analysis applies).
///
/// Pairing is path-structural via [`hir::pair_base_module_path`] over `all_config_paths`
/// (base root is the unlabelled entry); the candidate base path is resolved to a `FileId`
/// in the extension file's own source root. The final gate is
/// [`hir::effective_module_text`] being `Some`, so callers can construct an
/// `EffectiveModuleId` from the result and trust it routes to merged data rather than
/// silently re-deriving the base module under an effective key.
pub fn effective_target<'db>(
    db: &'db dyn RootDatabase,
    ext_file: vfs::FileId,
) -> Option<hir::EffectiveModuleId<'db>> {
    let ext_path = crate::vfs_helpers::get_file_path(db, ext_file)?;
    // Pairing is a relation between an extension and the base; an external
    // object's file never pairs, whatever relative path it happens to have.
    let roots = db.designer_config_paths();
    let base_path = hir::pair_base_module_path(&roots, &ext_path)?;

    let source_root = db.file_source_root_input(ext_file).source_root_id(db);
    let base_file = resolve_pair_candidate(db, source_root, &roots, &base_path)?;

    let eid = hir::EffectiveModuleId::new(db, base_file, ext_file);
    hir::effective_module_text(db, eid)?;
    Some(eid)
}

/// The *weaving* module identity for an extension module file, or `None` when the file
/// has no resolvable base counterpart. Unlike [`effective_target`] this does NOT require
/// an `&ИзменениеИКонтроль` splice: weaving (`&Вместо` / `&Перед` / `&После`) applies to
/// any extension module paired to an existing base, so the only gate is that a distinct
/// base file resolves.
///
/// Pairing is path-structural via [`hir::pair_base_module_path`] over `all_config_paths`
/// (base root is the unlabelled entry); the candidate base path is resolved to a `FileId`
/// in the extension file's own source root. Returns `None` when the resolved base file is
/// the extension file itself (no cross-module fallback to add).
pub fn weaving_target<'db>(
    db: &'db dyn RootDatabase,
    ext_file: vfs::FileId,
) -> Option<hir::WeavingModuleId<'db>> {
    let ext_path = crate::vfs_helpers::get_file_path(db, ext_file)?;
    let roots = db.designer_config_paths();
    let base_path = hir::pair_base_module_path(&roots, &ext_path)?;

    let source_root = db.file_source_root_input(ext_file).source_root_id(db);
    let base_file = resolve_pair_candidate(db, source_root, &roots, &base_path)?;

    if base_file == ext_file {
        return None;
    }

    Some(hir::WeavingModuleId::new(db, ext_file, base_file))
}

#[salsa::tracked(lru = 128, heap_size = heap_estimate::module_metadata_heap, returns(clone))]
pub fn module_metadata_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<hir::ModuleMetadata> {
    let _span = tracing::info_span!("module_metadata", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);

    let file_path = match crate::vfs_helpers::get_file_path(db, file_id) {
        Some(path) => path,
        None => {
            tracing::debug!("Could not determine file path for metadata");
            return Arc::new(hir::ModuleMetadata::unknown(bsl_metadata::ModuleType::Unknown));
        }
    };

    let mut metadata = crate::metadata::build_module_metadata(&file_path, None);

    match metadata.module_type {
        bsl_metadata::ModuleType::HTTPServiceModule => {
            if let Some(http_service) = db.http_service_for_file_id(file_id) {
                metadata.http_service = Some(http_service);
                return Arc::new(metadata);
            }
        }
        bsl_metadata::ModuleType::WebServiceModule => {
            if let Some(web_service) = db.web_service_for_file_id(file_id) {
                metadata.web_service = Some(web_service);
                return Arc::new(metadata);
            }
        }
        _ => {}
    }

    // Resolve via `get_configuration` (an object-safe `RootDatabase` method) rather
    // than the free `load_configuration` query, so the build-scoped config cache is
    // consulted and the configuration is not reloaded per batch database. The LSP
    // database has no cache attached and falls through to the salsa query.
    let configuration = db.get_configuration(file_id);

    metadata = crate::metadata::build_module_metadata(&file_path, configuration.as_deref());

    if let (Some(config), Some((root, canonical, kind))) =
        (configuration.as_deref(), db.external_root_of_path(&file_path))
    {
        crate::metadata::attach_external_owner(
            &mut metadata,
            &file_path,
            &root,
            &canonical,
            kind,
            config,
        );
    }

    if matches!(metadata.module_type, bsl_metadata::ModuleType::IntegrationServiceModule)
        && metadata.integration_service.is_none()
    {
        metadata.integration_service = db.integration_service_for_file_id(file_id);
    }

    Arc::new(metadata)
}

#[salsa::tracked(lru = 512, returns(clone))]
pub fn application_module_files_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: base_db::FileIdInput<'db>,
    kind: hir::ApplicationModuleKind,
) -> Option<hir::CommonModuleBodies> {
    db.resolve_application_module_files_uncached(file_id_input.file_id(db), kind)
}

// The per-method dataflow chain is computed from the method's own body and
// nothing file-wide, so a body edit re-runs it for the edited method only.
// Each link is retained at the cap of the inference that reads it (8192): a
// memo evicted below its reader has no old value to backdate against, and the
// reader above it re-runs — the LRU cascade of github#113. The sweep profile
// (`set_dataflow_lru_sweep_mode`) shrinks these caps together with the
// lowering chain.

#[salsa::tracked(lru = 8192, heap_size = heap_estimate::cfg_heap, returns(clone))]
pub fn method_cfg_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> Arc<hir::cfg::ControlFlowGraph> {
    let _span = tracing::info_span!("method_cfg", ?method_id_input).entered();
    let body = db.method_body_ref(method_id_input);
    Arc::new(hir::cfg::CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), body))
}

/// Reaching definitions over one body. Parameters seed the entry state only
/// for methods; module-level code has none.
fn solve_reaching_definitions(
    cfg: Arc<hir::cfg::ControlFlowGraph>,
    body_arc: &Arc<hir::Body>,
    seed_params: bool,
) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>> {
    use hir::dataflow::reaching_defs::{
        Definition, DefinitionIndex, ReachingDefs, ReachingDefsTransfer,
    };

    let body: &hir::Body = body_arc;
    let params: Vec<_> = if seed_params {
        body.params().map(|param_id| (body.binding(param_id).name.clone(), param_id)).collect()
    } else {
        Vec::new()
    };
    let def_index = DefinitionIndex::from_body_with_params(body, params);

    let mut initial_defs = ReachingDefs::new(def_index.clone());
    if seed_params {
        for param_id in body.params() {
            let binding = body.binding(param_id);
            initial_defs.insert(&Definition::parameter(&binding.name, param_id));
        }
    }

    let mut solver = hir::dataflow::DataflowSolver::new(cfg, body.clone(), ReachingDefsTransfer);
    solver.set_max_iterations(hir::dataflow::DEFAULT_MAX_ITERATIONS);
    solver.set_bottom_factory(|| ReachingDefs::new(def_index.clone()));
    solver.set_initial_state(initial_defs);

    solver.solve().map(|dataflow_result| {
        Arc::new(hir::dataflow::reaching_defs::ReachingDefsResult::new(
            dataflow_result,
            Arc::clone(body_arc),
        ))
    })
}

#[salsa::tracked(lru = 8192, heap_size = heap_estimate::reaching_defs_heap, returns(clone))]
pub fn reaching_definitions_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>> {
    let _span = tracing::info_span!("reaching_definitions", ?method_id_input).entered();
    let cfg = method_cfg_query(db, method_id_input);
    solve_reaching_definitions(cfg, db.method_body_ref(method_id_input), true)
}

#[salsa::tracked(lru = 128, heap_size = heap_estimate::reaching_defs_heap, returns(clone))]
pub fn module_code_reaching_definitions_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>> {
    let file_id = file_id_input.file_id(db);
    let _span = tracing::info_span!("module_code_reaching_definitions", ?file_id).entered();
    let module_bodies = db.module_bodies_ref(ModuleId::new(file_id));
    let body_arc = module_bodies.module_code_arc()?;
    let cfg = module_level_cfg_query(db, file_id_input);
    solve_reaching_definitions(cfg, body_arc, false)
}

#[salsa::tracked(lru = 8192, heap_size = heap_estimate::path_terminates_heap, returns(clone))]
pub fn method_path_terminates_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> Option<Arc<hir::dataflow::path_terminates::PathTerminatesResult>> {
    let _span = tracing::info_span!("method_path_terminates", ?method_id_input).entered();
    let cfg = method_cfg_query(db, method_id_input);
    let body = db.method_body_ref(method_id_input);
    hir::dataflow::path_terminates::analyze_path_terminates(
        body,
        &cfg,
        hir::dataflow::path_terminates::PathTerminatesConfig::default(),
        hir::dataflow::DEFAULT_MAX_ITERATIONS,
    )
    .map(Arc::new)
}

#[salsa::tracked(lru = 128, heap_size = heap_estimate::module_level_cfg_heap, returns(clone))]
pub fn module_level_cfg_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: base_db::FileIdInput<'db>,
) -> Arc<hir::cfg::ControlFlowGraph> {
    let _span = tracing::info_span!("module_level_cfg", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = hir::ModuleId::new(file_id);

    let module_bodies = db.module_bodies_ref(module_id);

    let body = match module_bodies.module_code() {
        Some(body) => body,
        None => {
            tracing::debug!("No module-level code in module: {:?}", module_id);
            return Arc::new(hir::cfg::ControlFlowGraph::new());
        }
    };

    let cfg = hir::cfg::CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), body);
    tracing::debug!("Built module-level CFG: {} vertices", cfg.vertices().count());

    Arc::new(cfg)
}

#[salsa::tracked(lru = 256, heap_size = heap_estimate::line_index_heap, returns(clone))]
pub fn line_index_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<line_index::LineIndex> {
    let file_id = file_id_input.file_id(db);
    let _span = tracing::info_span!("line_index", ?file_id).entered();

    let file_text = db.file_text(file_id);

    Arc::new(line_index::LineIndex::new(file_text.as_ref()))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleHirMetrics {
    methods: rustc_hash::FxHashMap<hir::MethodKey, Arc<hir::metrics::HirMethodMetrics>>,
    module_code: Option<Arc<hir::metrics::HirMethodMetrics>>,
}

impl ModuleHirMetrics {
    pub fn get(&self, local_id: hir::MethodKey) -> Option<Arc<hir::metrics::HirMethodMetrics>> {
        self.methods.get(&local_id).cloned()
    }

    pub fn module_code(&self) -> Option<Arc<hir::metrics::HirMethodMetrics>> {
        self.module_code.clone()
    }

    pub fn len(&self) -> usize {
        self.methods.len()
    }

    pub fn is_empty(&self) -> bool {
        self.methods.is_empty() && self.module_code.is_none()
    }

    /// File-level cognitive complexity: the sum of every method's raw HIR
    /// cognitive score. Module-level code is excluded (to stay symmetric with
    /// [`ModuleCyclomatic::total`]), as is the per-method recursion bonus the
    /// `CognitiveComplexity` diagnostic adds on top — this is the structural
    /// cognitive complexity only.
    pub fn total_cognitive(&self) -> u32 {
        self.methods.values().map(|m| m.cognitive).sum()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleCyclomatic {
    methods: rustc_hash::FxHashMap<hir::MethodKey, u32>,
}

impl ModuleCyclomatic {
    pub fn get(&self, local_id: hir::MethodKey) -> u32 {
        self.methods.get(&local_id).copied().unwrap_or(1)
    }

    pub fn len(&self) -> usize {
        self.methods.len()
    }

    pub fn is_empty(&self) -> bool {
        self.methods.is_empty()
    }

    /// File-level cyclomatic complexity: the sum of every method's complexity.
    pub fn total(&self) -> u32 {
        self.methods.values().copied().sum()
    }
}

#[salsa::tracked(lru = 8192, heap_size = heap_estimate::hir_metrics_heap, returns(clone))]
pub fn method_hir_metrics_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> Arc<hir::metrics::HirMethodMetrics> {
    let _span = tracing::info_span!("method_hir_metrics", ?method_id_input).entered();
    // The line count lives on the lowering, not on the body.
    let Some(lower) = db.method_lower(method_id_input) else {
        return Arc::new(hir::metrics::HirMethodMetrics::default());
    };
    let mut metrics = hir::metrics::compute_hir_metrics(&lower.body);
    metrics.size_lines = lower.size_lines;
    Arc::new(metrics)
}

/// File view of the per-method metrics, for the batch reporter's per-file
/// aggregates; diagnostics read the method-keyed query directly.
#[salsa::tracked(lru = 128, heap_size = heap_estimate::module_hir_metrics_heap, returns(clone))]
pub fn module_hir_metrics_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleHirMetrics> {
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);
    let _span = tracing::info_span!("module_hir_metrics", ?module_id).entered();

    let mut methods = rustc_hash::FxHashMap::default();
    for decl in db.module_interface_ref(module_id).methods() {
        db.unwind_if_revision_cancelled();
        let method_id_input = hir::MethodIdInput::new(db, decl.id);
        methods.insert(decl.id.local_id, method_hir_metrics_query(db, method_id_input));
    }
    let module_code = db
        .module_bodies_ref(module_id)
        .module_code()
        .map(hir::metrics::compute_hir_metrics)
        .filter(|m| *m != hir::metrics::HirMethodMetrics::default())
        .map(Arc::new);
    Arc::new(ModuleHirMetrics { methods, module_code })
}

#[salsa::tracked(lru = 8192, heap_size = stdx::heap::zero, returns(copy))]
pub fn method_cyclomatic_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> u32 {
    let _span = tracing::info_span!("method_cyclomatic", ?method_id_input).entered();
    let cfg = method_cfg_query(db, method_id_input);
    let metrics = method_hir_metrics_query(db, method_id_input);
    hir::cfg::cyclomatic_complexity(&cfg) + metrics.boolean_ops_count + metrics.ternary_count
}

/// File view of the per-method cyclomatic complexity, for the batch reporter's
/// per-file total; diagnostics read the method-keyed query directly.
#[salsa::tracked(lru = 128, heap_size = heap_estimate::module_cyclomatic_heap, returns(clone))]
pub fn module_cyclomatic_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleCyclomatic> {
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);
    let _span = tracing::info_span!("module_cyclomatic", ?file_id).entered();

    let mut methods = rustc_hash::FxHashMap::default();
    for decl in db.module_interface_ref(module_id).methods() {
        db.unwind_if_revision_cancelled();
        let method_id_input = hir::MethodIdInput::new(db, decl.id);
        methods.insert(decl.id.local_id, method_cyclomatic_query(db, method_id_input));
    }
    Arc::new(ModuleCyclomatic { methods })
}

/// Methods that call themselves, directly or through other methods of the
/// same module. Derived from the call summary, whose edges carry positions,
/// but position-free itself: an edit that moves calls without changing them
/// leaves this value equal, and its per-method readers valid.
#[salsa::tracked(lru = 128, heap_size = heap_estimate::recursive_methods_heap, returns(clone))]
pub fn module_recursive_methods_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<rustc_hash::FxHashSet<hir::MethodKey>> {
    use hir::call_graph::{CallTarget, CallerId, EdgeKind};

    let file_id = file_id_input.file_id(db);
    let summary = db.module_call_summary(ModuleId::new(file_id));
    let mut graph: rustc_hash::FxHashMap<hir::MethodKey, Vec<hir::MethodKey>> =
        rustc_hash::FxHashMap::default();
    for edge in &summary.call_edges {
        if !matches!(edge.kind, EdgeKind::DirectLocal) {
            continue;
        }
        let CallerId::Method(caller_id) = edge.caller else { continue };
        let CallTarget::Local { callee_local_id } = edge.target else { continue };
        graph.entry(caller_id).or_default().push(callee_local_id);
    }

    let mut recursive = rustc_hash::FxHashSet::default();
    for &start in graph.keys() {
        let mut stack = graph.get(&start).cloned().unwrap_or_default();
        let mut visited = rustc_hash::FxHashSet::default();
        while let Some(node) = stack.pop() {
            if node == start {
                recursive.insert(start);
                break;
            }
            if !visited.insert(node) {
                continue;
            }
            if let Some(next) = graph.get(&node) {
                stack.extend(next.iter().copied());
            }
        }
    }
    Arc::new(recursive)
}

/// Switch the per-method dataflow memos' LRU caps between the interactive
/// profile and the sweep profile, in step with the lowering chain
/// (`hir::set_lowering_lru_sweep_mode`): a whole-workspace sweep needs a
/// closed file's dataflow only while its chunk is analyzed. The interactive
/// values must stay equal to the `lru` literals on the queries.
pub fn set_dataflow_lru_sweep_mode(db: &mut dyn RootDatabase, sweep: bool) {
    const METHOD_INTERACTIVE: usize = 8192;
    const METHOD_SWEEP: usize = 2048;
    let cap = if sweep { METHOD_SWEEP } else { METHOD_INTERACTIVE };
    method_cfg_query::set_lru_capacity(db, cap);
    reaching_definitions_query::set_lru_capacity(db, cap);
    method_path_terminates_query::set_lru_capacity(db, cap);
    method_hir_metrics_query::set_lru_capacity(db, cap);
    method_cyclomatic_query::set_lru_capacity(db, cap);
    crate::effects::set_security_state_lru_capacity(db, cap);
    hir::set_arg_diagnostics_lru_capacity(db, cap);
}

#[cfg(test)]
mod salsa_backtrace_tests {
    use super::*;
    use crate::RootDatabaseImpl;
    use vfs::FileId;

    /// Exercises the condition the `main` panic hook relies on: inside a running
    /// query the database is attached to the thread, so `Backtrace::capture()`
    /// can resolve the query stack — impossible once `catch_unwind` has unwound.
    #[salsa::tracked(returns(clone))]
    fn capture_backtrace_in_query<'db>(
        db: &'db dyn RootDatabase,
        input: FileIdInput<'db>,
    ) -> Option<String> {
        let _ = input.file_id(db);
        salsa::Backtrace::capture().map(|bt| bt.to_string())
    }

    #[test]
    fn capture_inside_query_names_the_running_query() {
        let db = RootDatabaseImpl::new();
        let input = FileIdInput::new(&db, FileId(0));
        let rendered = capture_backtrace_in_query(&db, input)
            .expect("database is attached inside a tracked query");
        assert!(rendered.contains("query stacktrace"), "unexpected render: {rendered}");
        assert!(
            rendered.contains("capture_backtrace_in_query"),
            "backtrace should name the running query: {rendered}"
        );
    }

    #[test]
    fn capture_outside_any_query_is_none() {
        assert!(salsa::Backtrace::capture().is_none());
    }
}
