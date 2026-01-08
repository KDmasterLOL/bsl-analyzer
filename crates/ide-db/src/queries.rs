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

use std::sync::Arc;

use base_db::FileIdInput;
use hir_def::ModuleId;

use crate::{metadata::ConfigurationPathInput, RootDatabase, SdblHirEntries};

// Re-export query from metadata module
pub use crate::metadata::load_configuration;

// Helper types for internal use
type SdblInFile = Vec<(hir_def::ExprId, syntax::SdblQueryInfo)>;

/// Get metadata for a module (type and execution context).
///
/// This is the actual implementation of module_metadata that has access to VFS.
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
) -> Arc<hir_def::ModuleMetadata> {
    let _span = tracing::info_span!("module_metadata", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);

    // Get file path using VFS access
    let file_path = match crate::get_file_path_for_metadata(db, file_id) {
        Some(path) => path,
        None => {
            tracing::debug!("Could not determine file path for metadata");
            return Arc::new(hir_def::ModuleMetadata {
                module_type: bsl_metadata::ModuleType::CommonModule,
                execution_context: None,
                common_module: None,
                mdo: None,
            });
        }
    };

    // Determine module type from file URI
    let module_type = {
        let uri = file_path.to_string_lossy().to_string();
        crate::metadata::get_module_type_from_uri(&uri)
            .unwrap_or(bsl_metadata::ModuleType::CommonModule)
    };

    // Load metadata if this is a CommonModule
    let (execution_context, common_module) =
        if matches!(module_type, bsl_metadata::ModuleType::CommonModule) {
            // Find configuration root by searching for Configuration.xml
            match crate::find_configuration_root_for_metadata(db, &file_path) {
                Some(config_root) => {
                    let config_path_str = config_root.to_string_lossy().to_string();
                    tracing::debug!(?config_path_str, "Loading configuration for metadata");

                    // Load configuration via Salsa query (already tracked!)
                    let path_input = ConfigurationPathInput::new(db, config_path_str);
                    let configuration = load_configuration(db, path_input);

                    // Find CommonModule for this file
                    if let Some(common_module) =
                        crate::find_common_module_by_uri(&configuration, &file_path)
                    {
                        let execution_context = hir_def::compute_execution_context(&common_module);
                        (Some(execution_context), Some(Arc::new(common_module)))
                    } else {
                        tracing::debug!("CommonModule not found in configuration");
                        (None, None)
                    }
                }
                None => {
                    tracing::debug!("Configuration root not found");
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

    Arc::new(hir_def::ModuleMetadata { module_type, execution_context, common_module, mdo: None })
}

/// Get all SDBL queries in a file with their ExprId in BSL HIR.
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
/// - Memory: ~100 bytes per SDBL query (ExprId + SdblQueryInfo)
///
/// # Returns
/// Vec of (ExprId, SdblQueryInfo) sorted by position in source file
#[salsa::tracked(lru = 128)]
pub fn all_sdbl_in_file_query<'db>(
    db: &'db dyn hir_def::DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<SdblInFile> {
    let _span = tracing::info_span!("all_sdbl_in_file", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);

    // Get module bodies (Salsa dependency tracked automatically)
    let module_bodies = db.module_bodies(module_id);
    let mut result = Vec::new();

    // Collect from all method bodies (procedures and functions)
    for (_local_id, body) in module_bodies.iter_bodies() {
        for (expr_id, query_info) in body.sdbl_exprs() {
            result.push((*expr_id, query_info.clone()));
        }
    }

    // Collect from module-level code (statements outside methods)
    if let Some(module_code) = module_bodies.module_code() {
        for (expr_id, query_info) in module_code.sdbl_exprs() {
            result.push((*expr_id, query_info.clone()));
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
/// Vec of (ExprId, Arc<SdblHir>) - one entry per successfully parsed SDBL query
#[salsa::tracked(lru = 64)]
pub fn sdbl_hir_in_file_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
) -> SdblHirEntries {
    let _span = tracing::info_span!("sdbl_hir_in_file", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);

    // Get SDBL queries from BSL HIR (Salsa dependency tracked)
    let sdbl_queries = all_sdbl_in_file_query(db, file_id_input);

    // Try to load configuration for metadata-based type inference
    let configuration = crate::get_file_path_for_sdbl(db, file_id).and_then(|file_path| {
        crate::find_configuration_root_for_sdbl(db, &file_path).map(|config_root| {
            let config_path_str = config_root.to_string_lossy().to_string();
            let path_input = ConfigurationPathInput::new(db, config_path_str);
            // Salsa dependency tracked automatically!
            load_configuration(db, path_input)
        })
    });

    // Lower each SDBL query to HIR
    let config_ref = configuration.as_deref();
    let mut result = Vec::with_capacity(sdbl_queries.len());
    for (expr_id, query_info) in sdbl_queries.iter() {
        // Only lower if we have a parsed AST
        if let Some(ref sdbl_ast) = query_info.query_ast {
            let sdbl_hir = sdbl_hir::lower_sdbl_to_hir(sdbl_ast, config_ref);
            result.push((*expr_id, Arc::new(sdbl_hir)));
        }
    }

    tracing::debug!(count = result.len(), "Lowered SDBL to HIR");
    Arc::new(result)
}

/// Salsa tracked query for Control Flow Graph (CFG) construction.
///
/// Builds CFG from HIR Body for a single method. The CFG represents the flow of
/// execution through the method, with nodes for basic blocks and control structures.
///
/// # Performance
/// - LRU: 256 methods (CFG construction is relatively cheap)
/// - Depends on: module_bodies (via MethodIdInput)
/// - Invalidation: Automatic when method body changes
/// - Construction time: ~1-2ms for typical 100-line method
///
/// # Caching Strategy
/// CFG is cached separately from dataflow results to enable reuse across multiple
/// analyses (reaching definitions, liveness, constant propagation, etc.).
#[salsa::tracked(lru = 256)]
pub fn method_cfg_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir_def::MethodIdInput<'db>,
) -> Arc<cfg::ControlFlowGraph> {
    let _span = tracing::info_span!("method_cfg", ?method_id_input).entered();
    let method_id = method_id_input.method_id(db);
    let module_id = hir_def::ModuleId::new(method_id.module.file_id);

    // Get module bodies (cached)
    let module_bodies = db.module_bodies(module_id);

    // Get body for this method
    let body = match module_bodies.body(method_id.local_id) {
        Some(body) => body,
        None => {
            // Method has no body (forward declaration or error)
            tracing::debug!("Method has no body: {:?}", method_id);
            return Arc::new(cfg::ControlFlowGraph::new());
        }
    };

    // Build CFG from HIR Body (Phase 6.2 - HIR-based CFG)
    let cfg = cfg::CfgBuilder::new().build_graph_from_hir(&body.body_stmts, body, None);
    Arc::new(cfg)
}

/// Compute reaching definitions for a method using dataflow analysis.
///
/// This Salsa tracked query performs reaching definitions analysis - tracking which
/// variable definitions reach each program point.
///
/// # Salsa caching
/// - LRU: 256 (per-method dataflow analysis)
/// - Invalidation: Automatic when module_bodies changes (method code changed)
/// - Memory: ~2-5 KB per method (definition sets at each CFG node)
///
/// # Dependencies tracked by Salsa
/// - module_bodies (via DefDatabase) - gets method HIR body
/// - Automatically invalidates when method code changes
///
/// # Algorithm
/// 1. Build CFG from HIR body (Phase 6.2 HIR-based CFG)
/// 2. Initialize with parameter definitions
/// 3. Run dataflow analysis (Kildall's worklist algorithm)
/// 4. Return ReachingDefsResult with in/out sets for each CFG node
///
/// # Performance
/// - First call: ~5-20ms (CFG build + dataflow solve)
/// - Cached: < 1ms
/// - Convergence: Usually 3-10 iterations for typical BSL methods
///
/// # Returns
/// None if analysis fails (malformed CFG, no convergence), Some(result) otherwise
#[salsa::tracked(lru = 256)]
pub fn reaching_definitions_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir_def::MethodIdInput<'db>,
) -> Option<Arc<dataflow::reaching_defs::ReachingDefsResult>> {
    let _span = tracing::info_span!("reaching_definitions", ?method_id_input).entered();
    let method_id = method_id_input.method_id(db);

    // Get module bodies (Salsa dependency tracked automatically)
    let module_id = hir_def::ModuleId::new(method_id.module.file_id);
    let module_bodies = db.module_bodies(module_id);

    // Get body for this method
    let body = module_bodies.body(method_id.local_id)?;

    // Get cached CFG (Phase 6.6 - CFG caching via Salsa)
    // This replaces direct CFG construction, enabling reuse across multiple analyses
    let cfg = db.method_cfg(method_id);

    // Initialize reaching definitions with parameters
    let mut initial_defs = dataflow::reaching_defs::ReachingDefs::new();
    for &param_id in body.params.iter() {
        let binding = body.binding(param_id);
        let def = dataflow::reaching_defs::Definition::parameter(&binding.name, param_id);
        initial_defs.insert(def);
    }

    // Run dataflow analysis
    let transfer = dataflow::reaching_defs::ReachingDefsTransfer;
    let mut solver = dataflow::DataflowSolver::new(cfg, body.clone(), transfer);

    // Configure solver
    solver.set_max_iterations(100); // Reasonable limit for BSL methods
    solver.set_initial_state(initial_defs);

    // Solve dataflow equations
    let dataflow_result = solver.solve()?;

    // Wrap in high-level API
    let result = Arc::new(dataflow::reaching_defs::ReachingDefsResult::new(dataflow_result));

    tracing::debug!("Dataflow analysis converged");
    Some(result)
}

/// Compute liveness analysis for a method using backward dataflow.
///
/// This Salsa tracked query performs liveness analysis - tracking which variables
/// are "live" (may be read in the future) at each program point. Used to detect
/// unused local variables.
///
/// # Salsa caching
/// - LRU: 256 (per-method dataflow analysis)
/// - Invalidation: Automatic when module_bodies changes (method code changed)
/// - Memory: ~1-3 KB per method (live variable sets at each CFG node)
///
/// # Dependencies tracked by Salsa
/// - method_cfg (via method_cfg_query) - gets cached CFG
/// - module_bodies (via DefDatabase) - gets method HIR body
/// - Automatically invalidates when method code changes
///
/// # Algorithm
/// 1. Get cached CFG from method_cfg_query
/// 2. Create Liveness lattice (set of live variables)
/// 3. Run backward dataflow analysis (Kildall's algorithm)
///    - Start from exit with empty set (no variables live after exit)
///    - OUT[B] = join of IN[S] for all successors
///    - IN[B] = USE[B] ∪ (OUT[B] - DEF[B])
/// 4. Return DataflowResult<Liveness> with in/out sets for each CFG node
///
/// # Performance
/// - First call: ~2-10ms (backward dataflow solve)
/// - Cached: < 1ms
/// - Convergence: Usually 2-8 iterations for typical BSL methods
///
/// # Returns
/// None if analysis fails (malformed CFG, no convergence), Some(result) otherwise
#[salsa::tracked(lru = 256)]
pub fn liveness_analysis_query<'db>(
    db: &'db dyn RootDatabase,
    method_id_input: hir_def::MethodIdInput<'db>,
) -> Option<Arc<dataflow::DataflowResult<dataflow::liveness::Liveness>>> {
    let _span = tracing::info_span!("liveness_analysis", ?method_id_input).entered();
    let method_id = method_id_input.method_id(db);

    // Get module bodies (Salsa dependency tracked automatically)
    let module_id = hir_def::ModuleId::new(method_id.module.file_id);
    let module_bodies = db.module_bodies(module_id);

    // Get body for this method
    let body = module_bodies.body(method_id.local_id)?;

    // Get cached CFG (reuse across multiple analyses)
    let cfg = db.method_cfg(method_id);

    // Run backward dataflow analysis for liveness
    let transfer = dataflow::liveness::LivenessTransfer;
    let mut solver = dataflow::DataflowSolver::new(cfg, body.clone(), transfer);

    // Configure solver for backward analysis
    solver.set_max_iterations(100); // Reasonable limit for BSL methods
    solver.set_direction(dataflow::Direction::Backward);
    // No initial state needed - backward analysis starts from bottom (empty set)

    // Solve dataflow equations
    let dataflow_result = solver.solve()?;

    tracing::debug!("Liveness analysis converged");
    Some(Arc::new(dataflow_result))
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
) -> Arc<cfg::ControlFlowGraph> {
    let _span = tracing::info_span!("module_level_cfg", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = hir_def::ModuleId::new(file_id);

    // Get module bodies (cached)
    let module_bodies = db.module_bodies(module_id);

    // Get module-level code body
    let body = match module_bodies.module_code() {
        Some(body) => body,
        None => {
            // No module-level code
            tracing::debug!("No module-level code in module: {:?}", module_id);
            return Arc::new(cfg::ControlFlowGraph::new());
        }
    };

    // Build CFG from HIR body
    let cfg = cfg::CfgBuilder::new().build_graph_from_hir(&body.body_stmts, body, None);
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
) -> Option<Arc<dataflow::DataflowResult<dataflow::liveness::Liveness>>> {
    let _span = tracing::info_span!("module_level_liveness", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = hir_def::ModuleId::new(file_id);

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

    // Run backward dataflow analysis for liveness
    let transfer = dataflow::liveness::LivenessTransfer;
    let mut solver = dataflow::DataflowSolver::new(cfg, body.clone(), transfer);

    // Configure solver for backward analysis
    solver.set_max_iterations(100);
    solver.set_direction(dataflow::Direction::Backward);

    // Solve dataflow equations
    let dataflow_result = solver.solve()?;

    tracing::debug!("Module-level liveness analysis converged");
    Some(Arc::new(dataflow_result))
}
