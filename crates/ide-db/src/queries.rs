//! Salsa tracked queries for ide-db.
//!
//! This module provides a central registry of all top-level IDE queries.
//! These queries build on top of HIR to provide SDBL analysis, dataflow analysis,
//! and metadata loading.
//!
//! # Query Organization
//!
//! **Metadata:**
//! - [`load_configuration`] - Load 1C Configuration from disk (LRU: 16)
//! - [`module_metadata_query`] - Module type and execution context (LRU: 128)
//!
//! **SDBL:**
//! - [`all_sdbl_in_file_query`] - Extract SDBL queries from HIR (LRU: 128)
//! - [`sdbl_hir_in_file_query`] - Lower SDBL to HIR + type inference (LRU: 64)
//!
//! **Dataflow:**
//! - [`method_cfg_query`] - Control Flow Graph for method (LRU: 256)
//! - [`reaching_definitions_query`] - Reaching definitions analysis (LRU: 256)
//! - [`liveness_analysis_query`] - Liveness analysis for unused variables (LRU: 256)
//!
//! **Line Index:**
//! - [`line_index_query`] - Convert byte offsets to line/column positions (LRU: 256)

use std::{sync::Arc, time::Instant};

use base_db::FileIdInput;
use hir::ModuleId;

use crate::{
    metadata::{intern_configuration_path, ConfigurationPathInput},
    RootDatabase, SdblHirEntries,
};

// Re-export query from metadata module
pub use crate::metadata::load_configuration;

/// Get ConfigurationPathInput for a file (for provider construction).
///
/// Resolves the configuration root directory from the file's path,
/// returns None if the file path or configuration root cannot be found.
pub fn configuration_path_for_file<'db>(
    db: &'db dyn RootDatabase,
    file_id: vfs::FileId,
) -> Option<ConfigurationPathInput<'db>> {
    let file_path = crate::vfs_helpers::get_file_path(db, file_id)?;
    let config_root = crate::vfs_helpers::find_configuration_root(db, &file_path)?;
    Some(intern_configuration_path(db, &config_root.to_string_lossy(), db.metadata_version()))
}

// Helper types for internal use
type SdblInFile = Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>;

/// Get metadata for a module (type and execution context).
///
/// This Salsa tracked query delegates to `build_module_metadata` for the actual
/// metadata construction, ensuring a single source of truth.
///
/// Loads metadata from 1C Configuration if available, determines module type from file path,
/// and resolves execution context for CommonModules.
///
/// # Salsa caching
/// - LRU: 128 (metadata loading is I/O intensive)
/// - Invalidation: Automatic when file content changes
/// - Shared: load_configuration() is cached separately (LRU=16)
///
/// # Dependencies tracked by Salsa
/// - File content (implicit via file_id)
/// - Configuration (via load_configuration query)
/// - VFS file path resolution
///
/// # Performance
/// - First load: ~50-100ms (file path + configuration loading)
/// - Cached: < 1ms
/// - Configuration is shared across all modules in same project
#[salsa::tracked(lru = 128)]
pub fn module_metadata_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<hir::ModuleMetadata> {
    let _span = tracing::info_span!("module_metadata", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);

    // Get file path using VFS access
    let file_path = match crate::vfs_helpers::get_file_path(db, file_id) {
        Some(path) => path,
        None => {
            tracing::debug!("Could not determine file path for metadata");
            return Arc::new(hir::ModuleMetadata::unknown(bsl_metadata::ModuleType::Unknown));
        }
    };

    // Load configuration (Salsa-cached)
    let config_root = crate::vfs_helpers::find_configuration_root(db, &file_path);
    let configuration = config_root.map(|root| {
        let path_input =
            intern_configuration_path(db, &root.to_string_lossy(), db.metadata_version());
        load_configuration(db, path_input)
    });

    // Delegate to the single source of truth
    Arc::new(crate::metadata::build_module_metadata(&file_path, configuration.as_deref()))
}

/// Get all SDBL queries in a file with their SdblExprId.
///
/// This Salsa tracked query extracts SDBL queries from already-lowered BSL HIR bodies.
/// No separate AST traversal needed - reuses module_bodies query!
///
/// # Salsa caching
/// - LRU: 128 (lightweight extraction from module bodies)
/// - Invalidation: Automatic when module_bodies changes
/// - Sorted: Results sorted by source position for deterministic output
///
/// # Dependencies tracked by Salsa
/// - module_bodies (via DefDatabase)
/// - Automatically invalidates when file content changes
///
/// # Performance
/// - First call: ~1-5ms (iterates HIR bodies to find SDBL exprs)
/// - Cached: < 1ms
/// - Memory: ~100 bytes per SDBL query (SdblExprId + SdblQueryInfo)
///
/// # Returns
/// Vec of (SdblExprId, SdblQueryInfo) sorted by position in source file.
/// SdblExprId uniquely identifies SDBL expression across all bodies in file.
#[salsa::tracked(lru = 128)]
pub fn all_sdbl_in_file_query<'db>(
    db: &'db dyn hir::DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<SdblInFile> {
    let _span = tracing::debug_span!("all_sdbl_in_file", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);

    // Get module bodies (Salsa dependency tracked automatically)
    let module_bodies = db.module_bodies(module_id);
    let mut result = Vec::new();

    // Collect from all method bodies (procedures and functions)
    for (local_id, body) in module_bodies.iter_bodies() {
        for (expr_id, query_info) in body.sdbl_exprs() {
            let sdbl_expr_id = hir::SdblExprId::from_method(local_id, expr_id);
            result.push((sdbl_expr_id, query_info.clone()));
        }
    }

    // Collect from module-level code (statements outside methods)
    if let Some(module_code) = module_bodies.module_code() {
        for (expr_id, query_info) in module_code.sdbl_exprs() {
            let sdbl_expr_id = hir::SdblExprId::from_module_code(expr_id);
            result.push((sdbl_expr_id, query_info.clone()));
        }
    }

    // Sort by position in file for deterministic output
    result.sort_by_key(|(_, query_info)| query_info.bsl_literal_range.start());

    tracing::debug!(count = result.len(), "Collected SDBL from HIR");

    Arc::new(result)
}

/// Get SDBL HIR for all queries in a file.
///
/// This Salsa tracked query performs SDBL lowering to HIR with metadata-based type inference.
/// Depends on all_sdbl_in_file_query and load_configuration for automatic dependency tracking.
///
/// # Salsa caching
/// - LRU: 64 (heavy SDBL HIR lowering operation)
/// - Invalidation: Automatic when file content or configuration changes
/// - Dependencies: all_sdbl_in_file, load_configuration
///
/// # Performance
/// - First call: ~10-50ms (SDBL parsing + lowering + type inference)
/// - Cached: < 1ms
/// - Memory: ~1-5 KB per SDBL query (depends on query complexity)
///
/// # Semantic analysis performed
/// - Type inference from metadata (table types, field types)
/// - Name resolution (tables, fields, aliases)
/// - Semantic diagnostics (unknown tables, type mismatches, etc.)
///
/// # Returns
/// Vec of (SdblExprId, Arc<SdblPackage>) - one entry per successfully parsed SDBL query.
/// SdblExprId uniquely identifies SDBL expression across all bodies in file.
#[salsa::tracked(lru = 64)]
pub fn sdbl_hir_in_file_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> SdblHirEntries {
    let _span = tracing::debug_span!("sdbl_hir_in_file", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);

    // Get SDBL queries from BSL HIR (Salsa dependency tracked)
    let sdbl_queries = all_sdbl_in_file_query(db, file_id_input);

    if sdbl_queries.is_empty() {
        return Arc::new(Vec::new());
    }

    // Try to load configuration for metadata-based type inference
    let file_path_opt = crate::vfs_helpers::get_file_path(db, file_id);

    let configuration = file_path_opt.and_then(|file_path| {
        let config_root_opt = crate::vfs_helpers::find_configuration_root(db, &file_path);
        config_root_opt.map(|config_root| {
            let path_input = intern_configuration_path(
                db,
                &config_root.to_string_lossy(),
                db.metadata_version(),
            );
            // Salsa dependency tracked automatically!
            load_configuration(db, path_input)
        })
    });

    // Lower each SDBL query to HIR
    // Pass Arc<Configuration> directly to avoid cloning the large structure
    let mut result = Vec::with_capacity(sdbl_queries.len());
    for (expr_id, query_info) in sdbl_queries.iter() {
        // Only lower if we have a parsed AST
        if let Some(ref sdbl_ast) = query_info.query_ast {
            let sdbl_package = sdbl_hir::lower_sdbl_to_hir(sdbl_ast, configuration.clone());
            result.push((*expr_id, Arc::new(sdbl_package)));
        }
    }

    Arc::new(result)
}

// ============================================================================
// Module-Level Dataflow Queries (Batch Processing)
// ============================================================================

/// Build CFGs for all methods in a module at once (batch processing).
///
/// This query builds CFGs for ALL methods in the module in one pass,
/// which is much more efficient than calling method_cfg_query N times.
///
/// # Salsa caching
/// - LRU: 128 (per-module CFG collections)
/// - Invalidation: Automatic when module_bodies changes
/// - Memory: ~10-50 KB per module (depends on method count)
///
/// # Performance
/// - Build all CFGs in batch: ~1-5ms for typical module (10-50 methods)
/// - Much faster than N × method_cfg_query due to eliminated Salsa overhead
///
/// # Why module-level?
/// When any method changes, module_bodies invalidates the entire module,
/// which cascades to invalidate ALL per-method queries. Module-level
/// granularity matches the actual invalidation granularity, eliminating
/// wasted per-method Salsa overhead.
#[salsa::tracked(lru = 128)]
pub fn module_cfgs_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: base_db::FileIdInput<'db>,
) -> Arc<hir::cfg::ModuleCfgs> {
    let file_id = file_id_input.file_id(db);
    let module_id = hir::ModuleId::new(file_id);
    let _span = tracing::info_span!("module_cfgs", ?module_id).entered();

    // Get module bodies (Salsa dependency)
    let module_bodies = db.module_bodies(module_id);

    // Build CFG for each method
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

/// Get CFG for a single method (backward compatible accessor).
///
/// **Note:** This is now a thin wrapper around module_cfgs_query for backward compatibility.
/// Old code can continue using db.method_cfg(method_id), but under the hood it delegates
/// to the module-level batch query for efficiency.
///
/// # Performance
/// - LRU: 256 (per-method accessors, but delegates to module-level LRU=128)
/// - First call: Triggers module_cfgs_query (builds all CFGs for the module)
/// - Subsequent calls: Cheap HashMap lookup from cached module collection
/// - Expected speedup: 3-5x due to eliminated per-method Salsa overhead
///
/// # Migration
/// When a method changes, module_bodies invalidates, which cascades to module_cfgs.
/// All per-method accessors automatically get updated results from the module collection.
#[salsa::tracked(lru = 256)]
pub fn method_cfg_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> Arc<hir::cfg::ControlFlowGraph> {
    let _span = tracing::info_span!("method_cfg_accessor", ?method_id_input).entered();

    let method_id = method_id_input.method_id(db);
    let file_id = method_id.module.file_id;

    // Delegate to module-level query (Salsa caching!)
    let file_id_input = base_db::FileIdInput::new(db, file_id);
    let module_cfgs = db.module_cfgs(file_id_input);

    // HashMap lookup - cheap!
    module_cfgs
        .get(method_id.local_id)
        .cloned() // Clone Arc (cheap - just ref count bump)
        .unwrap_or_else(|| {
            tracing::debug!("No CFG found for method: {:?}", method_id);
            Arc::new(hir::cfg::ControlFlowGraph::new())
        })
}

/// Compute reaching definitions for all methods in a module (batch processing).
///
/// This query runs reaching definitions analysis for ALL methods in the module,
/// reusing CFGs from module_cfgs_query. Much more efficient than N separate queries.
///
/// # Salsa caching
/// - LRU: 128 (per-module collections)
/// - Invalidation: Automatic when module_bodies changes
/// - Dependencies: module_cfgs (shared CFGs!), module_bodies
///
/// # Performance
/// - Analyze all methods: ~5-20ms for typical module
/// - CFGs reused from module_cfgs_query (no rebuild overhead)
/// - Expected speedup: 3-5x vs per-method queries
///
/// # Max Iterations Fix
/// Uses 10000 iterations (not 100!) to ensure convergence for complex methods.
#[salsa::tracked(lru = 128)]
pub fn module_reaching_definitions_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: base_db::FileIdInput<'db>,
) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs> {
    let file_id = file_id_input.file_id(db);
    let module_id = hir::ModuleId::new(file_id);
    let _span = tracing::info_span!("module_reaching_definitions", ?module_id).entered();

    // Get shared CFGs (Salsa cached!)
    let module_cfgs = db.module_cfgs(file_id_input);
    let module_bodies = db.module_bodies(module_id);

    // Run reaching defs for each method (reusing CFGs)
    let mut results = rustc_hash::FxHashMap::default();

    for (local_id, body) in module_bodies.iter_bodies() {
        db.unwind_if_revision_cancelled();

        let cfg = match module_cfgs.get(local_id) {
            Some(cfg) => cfg.clone(), // Clone Arc (cheap)
            None => continue,
        };

        // Build definition index from body with parameters
        let params: Vec<_> = body
            .params()
            .map(|param_id| {
                let binding = body.binding(param_id);
                (binding.name.clone(), param_id)
            })
            .collect();
        let def_index =
            hir::dataflow::reaching_defs::DefinitionIndex::from_body_with_params(body, params);

        // Initialize entry state with parameters
        let mut initial_defs = hir::dataflow::reaching_defs::ReachingDefs::new(def_index.clone());
        for param_id in body.params() {
            let binding = body.binding(param_id);
            let def = hir::dataflow::reaching_defs::Definition::parameter(&binding.name, param_id);
            initial_defs.insert(&def);
        }

        // Run dataflow analysis
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

/// Compute path-terminates analysis for all methods in a module (batch processing).
///
/// Per Track 1 §1.5: backward dataflow on a 2-element `MayFallthrough`
/// lattice that answers "may execution starting at this block reach the
/// function's exit without an intervening `Return` / `Raise`?". Intended
/// consumer: the `AllFunctionPathMustHaveReturn` diagnostic
/// (Track 1 §1.6 — migrated in Step I, this query is the foundation).
///
/// # Salsa caching
/// - LRU: 128 (mirrors `module_cfgs` / `module_reaching_definitions` so
///   the cache cascade has matching granularity).
/// - Invalidation: cascades automatically via `module_bodies` →
///   `module_cfgs` → `module_path_terminates`.
///
/// # Performance
/// - Reuses CFGs from `module_cfgs` (zero rebuild overhead).
/// - The lattice has 2 elements → worklist converges in one full RPO pass
///   for any DAG and in O(loop-nest depth) on graphs with cycles, so the
///   per-method work is dominated by the basic-block sweep.
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

/// Get reaching definitions for a single method (backward compatible accessor).
///
/// **Note:** This is now a thin wrapper around module_reaching_definitions_query.
/// Old code can continue using db.reaching_definitions(method_id), but under the hood
/// it delegates to the module-level batch query for efficiency.
///
/// # Performance
/// - LRU: 256 (per-method accessors, but delegates to module-level LRU=128)
/// - First call: Triggers module_reaching_definitions_query (analyzes all methods in module with shared CFGs)
/// - Subsequent calls: Cheap HashMap lookup from cached module collection
/// - Expected speedup: 3-5x due to eliminated per-method Salsa overhead + max_iterations fix (10000 vs 100)
///
/// # Max Iterations Fix
/// The module-level query uses 10000 iterations (not 100!), ensuring convergence for complex methods.
///
/// # Returns
/// None if analysis fails (malformed CFG, no convergence), Some(result) otherwise
#[salsa::tracked(lru = 256)]
pub fn reaching_definitions_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>> {
    let _span = tracing::info_span!("reaching_definitions_accessor", ?method_id_input).entered();

    let method_id = method_id_input.method_id(db);
    let file_id = method_id.module.file_id;

    // Delegate to module-level query (Salsa caching!)
    let file_id_input = base_db::FileIdInput::new(db, file_id);
    let module_reaching_defs = db.module_reaching_definitions(file_id_input);

    // HashMap lookup - cheap!
    module_reaching_defs.get(method_id.local_id).cloned() // Clone Arc (cheap)
}

/// Compute liveness analysis for all methods in a module (batch processing).
///
/// This query runs liveness analysis for ALL methods in the module,
/// reusing CFGs from module_cfgs_query.
///
/// # Salsa caching
/// - LRU: 128 (per-module collections)
/// - Invalidation: Automatic when module_bodies changes
/// - Dependencies: module_cfgs (shared CFGs!), module_bodies
///
/// # Performance
/// - Analyze all methods: ~5-20ms for typical module
/// - CFGs reused from module_cfgs_query (no rebuild overhead)
/// - Expected speedup: 3-5x vs per-method queries (based on unused_local_variable optimization)
#[salsa::tracked(lru = 128)]
pub fn module_liveness_analysis_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: base_db::FileIdInput<'db>,
) -> Arc<hir::dataflow::liveness::ModuleLiveness> {
    let file_id = file_id_input.file_id(db);
    let module_id = hir::ModuleId::new(file_id);
    let _span = tracing::info_span!("module_liveness", ?module_id).entered();
    let total_start = Instant::now();

    // Get shared CFGs (Salsa cached!)
    let module_cfgs = db.module_cfgs(file_id_input);
    let module_bodies = db.module_bodies(module_id);

    // Run liveness for each method (reusing CFGs)
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

        // Build variable index
        let var_index = hir::dataflow::liveness::VariableIndex::from_body(body);

        // Run liveness analysis
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

/// Get liveness analysis for a single method (backward compatible accessor).
///
/// **Note:** This is now a thin wrapper around module_liveness_analysis_query.
/// Old code can continue using db.liveness_analysis(method_id), but under the hood
/// it delegates to the module-level batch query for efficiency.
///
/// # Performance
/// - LRU: 256 (per-method accessors, but delegates to module-level LRU=128)
/// - First call: Triggers module_liveness_analysis_query (analyzes all methods in module with shared CFGs)
/// - Subsequent calls: Cheap HashMap lookup from cached module collection
/// - Expected speedup: 3-5x due to eliminated per-method Salsa overhead (based on unused_local_variable optimization: 6.2x)
///
/// # Background
/// This optimization is based on the successful unused_local_variable optimization (commit 069d5a3)
/// which achieved 6.2x speedup by switching from per-method to batch processing.
///
/// # Returns
/// None if analysis fails (malformed CFG, no convergence), Some(result) otherwise
#[salsa::tracked(lru = 256)]
pub fn liveness_analysis_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir::MethodIdInput<'db>,
) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>> {
    let _span = tracing::info_span!("liveness_analysis_accessor", ?method_id_input).entered();

    let method_id = method_id_input.method_id(db);
    let file_id = method_id.module.file_id;

    // Delegate to module-level query (Salsa caching!)
    let file_id_input = base_db::FileIdInput::new(db, file_id);
    let module_liveness = db.module_liveness_analysis(file_id_input);

    // HashMap lookup - cheap!
    module_liveness.get(method_id.local_id).cloned() // Clone Arc (cheap)
}

/// Salsa tracked query for module-level code CFG construction.
///
/// Builds CFG from HIR Body for code outside procedures/functions.
/// Used for dataflow analyses on module initialization code.
///
/// # Performance
/// - LRU: 128 modules
/// - Depends on: module_bodies (via FileIdInput)
/// - Invalidation: Automatic when module body changes
///
/// # Returns
/// Empty CFG if no module-level code exists, otherwise CFG for module code.
#[salsa::tracked(lru = 128)]
pub fn module_level_cfg_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: base_db::FileIdInput<'db>,
) -> Arc<hir::cfg::ControlFlowGraph> {
    let _span = tracing::info_span!("module_level_cfg", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = hir::ModuleId::new(file_id);

    // Get module bodies (cached)
    let module_bodies = db.module_bodies(module_id);

    // Get module-level code body
    let body = match module_bodies.module_code() {
        Some(body) => body,
        None => {
            // No module-level code
            tracing::debug!("No module-level code in module: {:?}", module_id);
            return Arc::new(hir::cfg::ControlFlowGraph::new());
        }
    };

    // Build CFG from HIR body
    let cfg = hir::cfg::CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), body, None);
    tracing::debug!("Built module-level CFG: {} vertices", cfg.vertices().count());

    Arc::new(cfg)
}

/// Salsa tracked query for module-level liveness analysis.
///
/// Performs backward dataflow analysis on code outside procedures/functions.
/// Used to detect unused variables in module initialization code.
///
/// # Performance
/// - LRU: 128 modules
/// - Depends on: module_level_cfg, module_bodies
/// - Invalidation: Automatic when module body changes
///
/// # Returns
/// - `Some(DataflowResult<Liveness>)` if analysis succeeds
/// - `None` if no module-level code or analysis doesn't converge
#[salsa::tracked(lru = 128)]
pub fn module_level_liveness_analysis_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: base_db::FileIdInput<'db>,
) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>> {
    let _span = tracing::info_span!("module_level_liveness", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = hir::ModuleId::new(file_id);

    // Get module bodies (Salsa dependency tracked automatically)
    let module_bodies = db.module_bodies(module_id);

    // Get module-level code body
    let body = match module_bodies.module_code() {
        Some(body) => body,
        None => {
            // No module-level code
            tracing::debug!("No module-level code for liveness analysis: {:?}", module_id);
            return None;
        }
    };

    // Get cached CFG (reuse across multiple analyses)
    let cfg = db.module_level_cfg(module_id);

    // Create variable index for BitSet-based liveness (maps variable names to indices)
    let var_index = hir::dataflow::liveness::VariableIndex::from_body(body);

    // Run backward dataflow analysis for liveness
    let transfer = hir::dataflow::liveness::LivenessTransfer;
    let mut solver = hir::dataflow::DataflowSolver::new(cfg, body.clone(), transfer);

    // Configure solver for backward analysis
    solver.set_direction(hir::dataflow::Direction::Backward);

    // Initialize all blocks with BitSet-based bottom element (requires var_index)
    solver.set_bottom_factory(|| hir::dataflow::liveness::Liveness::new(var_index.clone()));

    // Max iterations: defaults to 1000 (sufficient for complex real-world methods)

    // Solve dataflow equations
    let dataflow_result = solver.solve()?;

    tracing::debug!("Module-level liveness analysis converged");
    Some(Arc::new(dataflow_result))
}

// ============================================================================
// Line Index Query
// ============================================================================

/// Get line index for a file (cached Salsa query).
///
/// LineIndex converts byte offsets (TextRange) to line/column positions.
/// This is needed for:
/// - Diagnostics that check multiline conditions
/// - LSP position conversions
/// - Line-based analysis (line length, empty lines, etc.)
///
/// # Architecture
/// LineIndex is cached through Salsa.
///
/// # Performance
/// - LRU: 256 files (most recently accessed)
/// - Construction: O(n) where n = file size
/// - Lookup: O(log n) binary search in line offsets
/// - Invalidation: Automatic when file_text changes
///
/// # Usage
/// ```ignore
/// let line_index = db.line_index(file_id);
/// let pos = line_index.line_col(range.start());
/// println!("Line: {}, Column: {}", pos.line, pos.col);
/// ```
#[salsa::tracked(lru = 256)]
pub fn line_index_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<line_index::LineIndex> {
    let file_id = file_id_input.file_id(db);
    let _span = tracing::info_span!("line_index", ?file_id).entered();

    let file_text_input = db.file_text_input(file_id);
    let file_text = file_text_input.text(db);

    Arc::new(line_index::LineIndex::new(file_text.as_ref()))
}

// ============================================================================
// Track 2 Phase B §6.3 — complexity-metric Salsa wrappers
// ============================================================================

/// Per-method [`hir::metrics::HirMethodMetrics`] results for one module.
///
/// Returned by [`module_hir_metrics_query`]. The §6.4 cognitive /
/// nested-statements / if-condition handlers iterate the per-method
/// shim [`method_hir_metrics_query`] which reads from this batch via a
/// cheap HashMap lookup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleHirMetrics {
    methods: rustc_hash::FxHashMap<u32, Arc<hir::metrics::HirMethodMetrics>>,
    /// Metrics for the module-level code body (statements outside any
    /// method). `None` when the module has no top-level code. Required
    /// by §6.4 `NestedStatements` whose legacy lowering-time emit ran
    /// for module-level Если/Пока/Для/Попытка the same way it ran for
    /// method bodies.
    module_code: Option<Arc<hir::metrics::HirMethodMetrics>>,
}

impl ModuleHirMetrics {
    /// Streaming-mode constructor (no Salsa). Mirrors
    /// [`crate::effects::ModuleSecurityState::from_methods_with_module_level`]
    /// — `pub(crate)` so the private fields stay opaque to downstream
    /// consumers.
    pub(crate) fn from_methods(
        methods: rustc_hash::FxHashMap<u32, Arc<hir::metrics::HirMethodMetrics>>,
        module_code: Option<Arc<hir::metrics::HirMethodMetrics>>,
    ) -> Self {
        Self { methods, module_code }
    }

    /// Look up a method's metrics. Returns `None` for methods absent
    /// from the module's body collection (synthetic / orphan ItemTree
    /// entries) — matches the contract of every other module-batch
    /// query.
    pub fn get(&self, local_id: u32) -> Option<Arc<hir::metrics::HirMethodMetrics>> {
        self.methods.get(&local_id).cloned()
    }

    /// Metrics for module-level code (top-level statements outside any
    /// method). `None` when the module has no top-level code. Returned
    /// as the same `HirMethodMetrics` shape so handlers can reuse the
    /// per-method analysis path with a synthetic "module body" entry.
    pub fn module_code(&self) -> Option<Arc<hir::metrics::HirMethodMetrics>> {
        self.module_code.clone()
    }

    /// Number of methods with computed metrics.
    pub fn len(&self) -> usize {
        self.methods.len()
    }

    /// `true` when no method or module-level code produced metrics
    /// (typically: an empty module).
    pub fn is_empty(&self) -> bool {
        self.methods.is_empty() && self.module_code.is_none()
    }
}

/// Per-method cyclomatic complexity values for one module.
///
/// Stored as `u32` directly (no `Arc` — values fit in a register).
/// Returned by [`module_cyclomatic_query`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleCyclomatic {
    methods: rustc_hash::FxHashMap<u32, u32>,
}

impl ModuleCyclomatic {
    /// Streaming-mode constructor (no Salsa).
    pub(crate) fn from_methods(methods: rustc_hash::FxHashMap<u32, u32>) -> Self {
        Self { methods }
    }

    /// Look up a method's cyclomatic complexity. Returns `1` (the
    /// conventional base) for methods absent from the module batch.
    pub fn get(&self, local_id: u32) -> u32 {
        self.methods.get(&local_id).copied().unwrap_or(1)
    }

    /// Number of methods with computed values.
    pub fn len(&self) -> usize {
        self.methods.len()
    }

    /// `true` when no method produced a value.
    pub fn is_empty(&self) -> bool {
        self.methods.is_empty()
    }
}

/// Compute HIR-structural metrics for every method in a module.
///
/// Per master plan §6.3: module-level batch matches the actual
/// invalidation granularity of `module_bodies` (any method change
/// invalidates the whole module), so per-method queries would burn
/// cache for no benefit. Each method's `HirMethodMetrics` is shared as
/// `Arc` so the per-method shim is a cheap pointer copy.
///
/// LRU = 128 (matches `module_cfgs_query`).
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
        // `size_lines` is a syntax-level metric (depends on
        // `LineIndex`, not the HIR `Body`), so it lives in the
        // `LowerResult` next to the body. Stamp it onto the metrics
        // here — the visitor cannot compute it.
        if let Some(lower_result) = module_bodies.lower_result(local_id) {
            metrics.size_lines = lower_result.size_lines;
        }
        methods.insert(local_id, Arc::new(metrics));
    }
    // Codex round-A fix: `ModuleBodies::module_code()` is always `Some`
    // after lowering, even for modules with zero top-level statements.
    // Drop the entry when its metrics are default (every counter at
    // zero, every list empty) so `ModuleHirMetrics::is_empty()` stays a
    // meaningful fast-path gate for handlers.
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

/// Per-method shim over [`module_hir_metrics_query`]. LRU = 256 to
/// match the `method_cfg_query` precedent — the hot path is a
/// per-method `HashMap` lookup against the module batch.
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

/// Compute cyclomatic complexity for every method in a module.
///
/// Reuses the CFGs already cached by `module_cfgs_query` — each
/// method's CFG is a single `Arc` clone, then a graph traversal via
/// [`hir::cfg::cyclomatic_complexity`].
///
/// Track 2 Phase B §6.5 alignment: the textbook McCabe formula
/// `V(G) = E - N + 2*P` only counts decision points that introduce
/// CFG edges. BSL short-circuit `И`/`ИЛИ` and ternary `?(...)` evaluate
/// inside basic blocks (no extra edges), so the textbook formula
/// under-counts compared to SonarQube cyclomatic. We add the
/// per-occurrence boolean and ternary counts (sourced from the §6.1
/// HIR visitor through `module_hir_metrics_query`) on top of the
/// graph value to match the SonarQube definition. The legacy
/// `hir-def::cyclomatic_complexity::calculate_complexity` (retired in
/// 71c22eab) also counted `Else` clauses as separate decisions; we
/// intentionally drop that — SonarQube doesn't, and `Else` doesn't
/// add a new outgoing edge in McCabe's textbook formulation.
///
/// LRU = 128.
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

/// Per-method shim over [`module_cyclomatic_query`]. LRU = 256.
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
