use std::{sync::Arc, time::Instant};

use base_db::FileIdInput;
use hir::ModuleId;

use crate::{
    metadata::{intern_configuration_path, ConfigurationPathInput},
    RootDatabase,
};

pub use crate::metadata::load_configuration;

pub fn configuration_path_for_file<'db>(
    db: &'db dyn RootDatabase,
    file_id: vfs::FileId,
) -> Option<ConfigurationPathInput<'db>> {
    let file_path = crate::vfs_helpers::get_file_path(db, file_id)?;
    let config_root = crate::vfs_helpers::find_configuration_root(db, &file_path)?;
    Some(intern_configuration_path(
        db,
        &config_root.to_string_lossy(),
        db.config_root_revision_for_path(&file_path),
    ))
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
    let roots = db.all_config_paths();
    let base_path = hir::pair_base_module_path(&roots, &ext_path)?;

    let source_root = db.file_source_root_input(ext_file).source_root_id(db);
    let vfs_path = vfs::VfsPath::new(base_path.to_string_lossy().into_owned());
    let base_file = db.resolve_vfs_path(source_root, &vfs_path)?;

    let eid = hir::EffectiveModuleId::new(db, base_file, ext_file);
    hir::effective_module_text(db, eid)?;
    Some(eid)
}

#[salsa::tracked(lru = 128)]
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

    // Resolve via `get_configuration` (an object-safe `RootDatabase` method) rather
    // than the free `load_configuration` query, so the build-scoped config cache is
    // consulted and the configuration is not reloaded per batch database. The LSP
    // database has no cache attached and falls through to the salsa query.
    let configuration = db.get_configuration(file_id);

    Arc::new(crate::metadata::build_module_metadata(&file_path, configuration.as_deref()))
}

#[salsa::tracked(lru = 128)]
pub fn module_cfgs_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: base_db::FileIdInput<'db>,
) -> Arc<hir::cfg::ModuleCfgs> {
    let file_id = file_id_input.file_id(db);
    let module_id = hir::ModuleId::new(file_id);
    let _span = tracing::info_span!("module_cfgs", ?module_id).entered();

    let module_bodies = db.module_bodies(module_id);

    let mut cfgs = rustc_hash::FxHashMap::default();
    for (local_id, body) in module_bodies.iter_bodies() {
        let source_map = module_bodies.source_map(local_id);
        let cfg = hir::cfg::CfgBuilder::new().build_graph_from_hir(
            body.body_stmts_typed(),
            body,
            source_map,
        );
        cfgs.insert(local_id, Arc::new(cfg));
    }

    tracing::debug!(count = cfgs.len(), "Built module CFGs");
    Arc::new(hir::cfg::ModuleCfgs::new(cfgs))
}

#[salsa::tracked(lru = 256)]
pub fn method_cfg_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> Arc<hir::cfg::ControlFlowGraph> {
    let _span = tracing::info_span!("method_cfg_accessor", ?method_id_input).entered();

    let method_id = method_id_input.method_id(db);
    let file_id = method_id.module.file_id;

    let file_id_input = base_db::FileIdInput::new(db, file_id);
    let module_cfgs = db.module_cfgs(file_id_input);

    module_cfgs.get(method_id.local_id).cloned().unwrap_or_else(|| {
        tracing::debug!("No CFG found for method: {:?}", method_id);
        Arc::new(hir::cfg::ControlFlowGraph::new())
    })
}

#[salsa::tracked(lru = 128)]
pub fn module_reaching_definitions_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: base_db::FileIdInput<'db>,
) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs> {
    let file_id = file_id_input.file_id(db);
    let module_id = hir::ModuleId::new(file_id);
    let _span = tracing::info_span!("module_reaching_definitions", ?module_id).entered();

    let module_cfgs = db.module_cfgs(file_id_input);
    let module_bodies = db.module_bodies(module_id);

    let mut results = rustc_hash::FxHashMap::default();

    for (local_id, body) in module_bodies.iter_bodies() {
        db.unwind_if_revision_cancelled();

        let cfg = match module_cfgs.get(local_id) {
            Some(cfg) => cfg.clone(),
            None => continue,
        };

        let params: Vec<_> = body
            .params()
            .map(|param_id| {
                let binding = body.binding(param_id);
                (binding.name.clone(), param_id)
            })
            .collect();
        let def_index =
            hir::dataflow::reaching_defs::DefinitionIndex::from_body_with_params(body, params);

        let mut initial_defs = hir::dataflow::reaching_defs::ReachingDefs::new(def_index.clone());
        for param_id in body.params() {
            let binding = body.binding(param_id);
            let def = hir::dataflow::reaching_defs::Definition::parameter(&binding.name, param_id);
            initial_defs.insert(&def);
        }

        let transfer = hir::dataflow::reaching_defs::ReachingDefsTransfer;
        let mut solver = hir::dataflow::DataflowSolver::new(cfg, body.clone(), transfer);
        solver.set_max_iterations(hir::dataflow::DEFAULT_MAX_ITERATIONS);
        solver.set_bottom_factory(|| {
            hir::dataflow::reaching_defs::ReachingDefs::new(def_index.clone())
        });
        solver.set_initial_state(initial_defs);

        if let Some(dataflow_result) = solver.solve() {
            let result = hir::dataflow::reaching_defs::ReachingDefsResult::new(dataflow_result);
            results.insert(local_id, Arc::new(result));
        }
    }

    tracing::debug!(count = results.len(), "Analyzed reaching definitions");
    Arc::new(hir::dataflow::reaching_defs::ModuleReachingDefs::new(results))
}

#[salsa::tracked(lru = 128)]
pub fn module_path_terminates_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: base_db::FileIdInput<'db>,
) -> Arc<hir::dataflow::path_terminates::ModulePathTerminates> {
    let file_id = file_id_input.file_id(db);
    let module_id = hir::ModuleId::new(file_id);
    let _span = tracing::info_span!("module_path_terminates", ?module_id).entered();

    let module_cfgs = db.module_cfgs(file_id_input);
    let module_bodies = db.module_bodies(module_id);

    let mut results = rustc_hash::FxHashMap::default();

    for (local_id, body) in module_bodies.iter_bodies() {
        db.unwind_if_revision_cancelled();

        let cfg = match module_cfgs.get(local_id) {
            Some(cfg) => cfg,
            None => continue,
        };

        if let Some(result) = hir::dataflow::path_terminates::analyze_path_terminates(
            body,
            cfg,
            hir::dataflow::path_terminates::PathTerminatesConfig::default(),
            hir::dataflow::DEFAULT_MAX_ITERATIONS,
        ) {
            results.insert(local_id, Arc::new(result));
        }
    }

    tracing::debug!(count = results.len(), "Analyzed module path-terminates");
    Arc::new(hir::dataflow::path_terminates::ModulePathTerminates::new(results))
}

#[salsa::tracked(lru = 256)]
pub fn reaching_definitions_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>> {
    let _span = tracing::info_span!("reaching_definitions_accessor", ?method_id_input).entered();

    let method_id = method_id_input.method_id(db);
    let file_id = method_id.module.file_id;

    let file_id_input = base_db::FileIdInput::new(db, file_id);
    let module_reaching_defs =
        <dyn RootDatabase as RootDatabase>::module_reaching_definitions(db, file_id_input);

    module_reaching_defs.get(method_id.local_id).cloned()
}

#[salsa::tracked(lru = 128)]
pub fn module_liveness_analysis_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: base_db::FileIdInput<'db>,
) -> Arc<hir::dataflow::liveness::ModuleLiveness> {
    let file_id = file_id_input.file_id(db);
    let module_id = hir::ModuleId::new(file_id);
    let _span = tracing::info_span!("module_liveness", ?module_id).entered();
    let total_start = Instant::now();

    let module_cfgs = db.module_cfgs(file_id_input);
    let module_bodies = db.module_bodies(module_id);

    let mut results = rustc_hash::FxHashMap::default();

    for (local_id, body) in module_bodies.iter_bodies() {
        db.unwind_if_revision_cancelled();

        let cfg = match module_cfgs.get(local_id) {
            Some(cfg) => cfg,
            None => continue,
        };
        let method_start = Instant::now();
        let block_count = cfg.vertices().count();
        let stmt_count = body.stmt_count();

        let var_index = hir::dataflow::liveness::VariableIndex::from_body(body);

        if let Some(liveness_result) = hir::dataflow::liveness::liveness_analysis_direct(
            body,
            cfg,
            var_index,
            hir::dataflow::DEFAULT_MAX_ITERATIONS,
        ) {
            results.insert(local_id, Arc::new(liveness_result));
        }

        let elapsed_ms = method_start.elapsed().as_millis();
        if elapsed_ms >= 100 {
            tracing::info!(
                local_id,
                block_count,
                stmt_count,
                elapsed_ms,
                "Slow module liveness method"
            );
        }
    }

    tracing::info!(
        count = results.len(),
        elapsed_ms = total_start.elapsed().as_millis(),
        "Module liveness batch complete"
    );
    Arc::new(hir::dataflow::liveness::ModuleLiveness::new(results))
}

#[salsa::tracked(lru = 256)]
pub fn liveness_analysis_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>> {
    let _span = tracing::info_span!("liveness_analysis_accessor", ?method_id_input).entered();

    let method_id = method_id_input.method_id(db);
    let file_id = method_id.module.file_id;

    let file_id_input = base_db::FileIdInput::new(db, file_id);
    let module_liveness = db.module_liveness_analysis(file_id_input);

    module_liveness.get(method_id.local_id).cloned()
}

#[salsa::tracked(lru = 128)]
pub fn module_level_cfg_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: base_db::FileIdInput<'db>,
) -> Arc<hir::cfg::ControlFlowGraph> {
    let _span = tracing::info_span!("module_level_cfg", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = hir::ModuleId::new(file_id);

    let module_bodies = db.module_bodies(module_id);

    let body = match module_bodies.module_code() {
        Some(body) => body,
        None => {
            tracing::debug!("No module-level code in module: {:?}", module_id);
            return Arc::new(hir::cfg::ControlFlowGraph::new());
        }
    };

    let cfg = hir::cfg::CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), body, None);
    tracing::debug!("Built module-level CFG: {} vertices", cfg.vertices().count());

    Arc::new(cfg)
}

#[salsa::tracked(lru = 128)]
pub fn module_level_liveness_analysis_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: base_db::FileIdInput<'db>,
) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>> {
    let _span = tracing::info_span!("module_level_liveness", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = hir::ModuleId::new(file_id);

    let module_bodies = db.module_bodies(module_id);

    let body = match module_bodies.module_code() {
        Some(body) => body,
        None => {
            tracing::debug!("No module-level code for liveness analysis: {:?}", module_id);
            return None;
        }
    };

    let cfg = db.module_level_cfg(module_id);
    let var_index = hir::dataflow::liveness::VariableIndex::from_body(body);
    let transfer = hir::dataflow::liveness::LivenessTransfer;
    let mut solver = hir::dataflow::DataflowSolver::new(cfg, body.clone(), transfer);
    solver.set_direction(hir::dataflow::Direction::Backward);
    solver.set_bottom_factory(|| hir::dataflow::liveness::Liveness::new(var_index.clone()));
    let dataflow_result = solver.solve()?;

    tracing::debug!("Module-level liveness analysis converged");
    Some(Arc::new(dataflow_result))
}

#[salsa::tracked(lru = 256)]
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
    methods: rustc_hash::FxHashMap<u32, Arc<hir::metrics::HirMethodMetrics>>,
    module_code: Option<Arc<hir::metrics::HirMethodMetrics>>,
}

impl ModuleHirMetrics {
    pub(crate) fn from_methods(
        methods: rustc_hash::FxHashMap<u32, Arc<hir::metrics::HirMethodMetrics>>,
        module_code: Option<Arc<hir::metrics::HirMethodMetrics>>,
    ) -> Self {
        Self { methods, module_code }
    }

    pub fn get(&self, local_id: u32) -> Option<Arc<hir::metrics::HirMethodMetrics>> {
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
    methods: rustc_hash::FxHashMap<u32, u32>,
}

impl ModuleCyclomatic {
    pub(crate) fn from_methods(methods: rustc_hash::FxHashMap<u32, u32>) -> Self {
        Self { methods }
    }

    pub fn get(&self, local_id: u32) -> u32 {
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

#[salsa::tracked(lru = 128)]
pub fn module_hir_metrics_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleHirMetrics> {
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);
    let _span = tracing::info_span!("module_hir_metrics", ?module_id).entered();

    let module_bodies = db.module_bodies(module_id);
    let mut methods = rustc_hash::FxHashMap::default();
    for (local_id, body) in module_bodies.iter_bodies() {
        let mut metrics = hir::metrics::compute_hir_metrics(body);
        if let Some(lower_result) = module_bodies.lower_result(local_id) {
            metrics.size_lines = lower_result.size_lines;
        }
        methods.insert(local_id, Arc::new(metrics));
    }
    let module_code = module_bodies
        .module_code()
        .map(hir::metrics::compute_hir_metrics)
        .filter(|m| *m != hir::metrics::HirMethodMetrics::default())
        .map(Arc::new);
    tracing::debug!(
        count = methods.len(),
        has_module_code = module_code.is_some(),
        "Built module HIR metrics",
    );
    Arc::new(ModuleHirMetrics { methods, module_code })
}

#[salsa::tracked(lru = 256)]
pub fn method_hir_metrics_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> Arc<hir::metrics::HirMethodMetrics> {
    let _span = tracing::info_span!("method_hir_metrics", ?method_id_input).entered();
    let method_id = method_id_input.method_id(db);
    let file_id_input = FileIdInput::new(db, method_id.module.file_id);
    let module_metrics = module_hir_metrics_query(db, file_id_input);
    module_metrics
        .get(method_id.local_id)
        .unwrap_or_else(|| Arc::new(hir::metrics::HirMethodMetrics::default()))
}

#[salsa::tracked(lru = 128)]
pub fn module_cyclomatic_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleCyclomatic> {
    let file_id = file_id_input.file_id(db);
    let _span = tracing::info_span!("module_cyclomatic", ?file_id).entered();

    let module_cfgs = db.module_cfgs(file_id_input);
    let module_id = ModuleId::new(file_id);
    let module_bodies = db.module_bodies(module_id);
    let module_metrics = module_hir_metrics_query(db, file_id_input);
    let mut methods = rustc_hash::FxHashMap::default();
    for (local_id, _body) in module_bodies.iter_bodies() {
        let Some(cfg) = module_cfgs.get(local_id) else { continue };
        let base = hir::cfg::cyclomatic_complexity(cfg.as_ref());
        let extras = module_metrics
            .get(local_id)
            .map(|m| m.boolean_ops_count + m.ternary_count)
            .unwrap_or(0);
        methods.insert(local_id, base + extras);
    }
    tracing::debug!(count = methods.len(), "Built module cyclomatic metrics");
    Arc::new(ModuleCyclomatic { methods })
}

#[salsa::tracked(lru = 256)]
pub fn method_cyclomatic_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> u32 {
    let _span = tracing::info_span!("method_cyclomatic", ?method_id_input).entered();
    let method_id = method_id_input.method_id(db);
    let file_id_input = FileIdInput::new(db, method_id.module.file_id);
    let module_metrics = module_cyclomatic_query(db, file_id_input);
    module_metrics.get(method_id.local_id)
}
