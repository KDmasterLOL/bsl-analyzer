//! RootDatabase trait — application-facing port for IDE operations.
//!
//! This module defines what queries the IDE layer can perform.
//! The concrete Salsa-backed implementation lives in `database.rs`.

use std::sync::Arc;

use base_db::{FileIdInput, RootQueryDb, SourceDatabase};
use hir::DefDatabase;
use vfs::FileId;

use crate::SdblHirEntries;

/// The root database for IDE operations.
///
/// This database extends SourceDatabase, RootQueryDb, DefDatabase, HirDatabase, and MetadataDb,
/// providing full HIR functionality, type inference, and metadata support with caching.
#[salsa::db]
pub trait RootDatabase:
    SourceDatabase + RootQueryDb + DefDatabase + hir::HirDatabase + crate::metadata::MetadataDb
{
    /// Get configuration for a file (Salsa-cached).
    ///
    /// This method loads configuration metadata for the project containing the file.
    /// The configuration is cached by Salsa for efficient reuse.
    ///
    /// # Returns
    /// - `Some(Arc<Configuration>)` if configuration found and loaded successfully
    /// - `None` if file path not found or configuration root not found
    fn get_configuration(&self, file_id: FileId) -> Option<Arc<bsl_metadata::Configuration>>;

    /// Get all configurations visible from a file: file's own config + all registered extensions.
    ///
    /// Returns pairs of (extension_name, configuration). The file's own configuration
    /// has `None` as name; extensions have `Some(name)`.
    fn get_all_configurations(
        &self,
        file_id: FileId,
    ) -> Vec<(Option<String>, Arc<bsl_metadata::Configuration>)>;

    /// Get all registered configuration roots: main + extensions.
    ///
    /// Returns pairs of (extension_name, root_path). Main configuration has
    /// `None` as name; extensions have `Some(name)`. Each root is the
    /// directory containing `Configuration.xml`.
    fn all_config_paths(&self) -> Vec<(Option<String>, std::path::PathBuf)>;

    /// Get all SDBL queries in a file with their SdblExprId.
    ///
    /// Reuses BSL HIR lowering - no separate AST traversal!
    /// SdblExprId uniquely identifies SDBL expression across all bodies in file.
    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>>;

    /// Get SDBL HIR for all queries in a file.
    ///
    /// Performs semantic analysis:
    /// - Type inference from metadata
    /// - Name resolution (tables, fields, aliases)
    /// - Semantic diagnostics collection
    ///
    /// ## Usage
    /// ```ignore
    /// let sdbl_hirs = db.sdbl_hir_in_file(file_id);
    /// for (expr_id, sdbl_hir) in sdbl_hirs.iter() {
    ///     // Check semantic diagnostics
    ///     for diag in &sdbl_hir.diagnostics {
    ///         println!("{}", diag.message());
    ///     }
    ///     // Access typed fields
    ///     for field in &sdbl_hir.select.fields {
    ///         println!("Field type: {:?}", field.ty);
    ///     }
    /// }
    /// ```
    fn sdbl_hir_in_file(&self, file_id: FileId) -> SdblHirEntries;

    // ========================================================================
    // Module-Level Dataflow Queries (Batch Processing)
    // ========================================================================

    /// Build CFGs for all methods in a module at once (batch processing).
    ///
    /// This query builds CFGs for ALL methods in the module in one pass,
    /// which is much more efficient than calling method_cfg N times.
    ///
    /// ## Performance
    /// - Build all CFGs in batch: ~1-5ms for typical module (10-50 methods)
    /// - Much faster than N × method_cfg due to eliminated per-method Salsa overhead
    /// - Cached per module (LRU=128)
    ///
    /// ## Why module-level?
    /// When any method changes, module_bodies invalidates the entire module,
    /// which cascades to invalidate ALL per-method queries. Module-level
    /// granularity matches the actual invalidation granularity.
    fn module_cfgs(&self, file_id_input: FileIdInput) -> Arc<hir::cfg::ModuleCfgs>;

    /// Compute reaching definitions for all methods in a module (batch processing).
    ///
    /// Runs reaching definitions analysis for ALL methods in the module,
    /// reusing CFGs from module_cfgs. Much more efficient than N separate queries.
    ///
    /// ## Performance
    /// - Analyze all methods: ~5-20ms for typical module
    /// - CFGs reused from module_cfgs (no rebuild overhead)
    /// - Expected speedup: 3-5x vs per-method queries
    /// - Cached per module (LRU=128)
    ///
    /// ## Max Iterations Fix
    /// Uses 10000 iterations (not 100!) to ensure convergence for complex methods.
    fn module_reaching_definitions(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs>;

    /// Compute path-terminates analysis for all methods in a module (batch).
    ///
    /// Backward dataflow that answers "may execution from this block reach
    /// the function's exit without crossing a `Return` / `Raise`?". The
    /// boundary fact `OUT[exit] = true` propagates back, killed at every
    /// `Return` / `Raise` statement and at every dead `AdjacentCode` edge.
    ///
    /// Intended consumer: the `AllFunctionPathMustHaveReturn` diagnostic
    /// (Track 1 §1.6 — migrated in Step I; this query is the
    /// foundation it will replace the legacy "inspect incoming edges of
    /// exit" walk with). Cached per-module (LRU=128).
    fn module_path_terminates(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<hir::dataflow::path_terminates::ModulePathTerminates>;

    /// Compute liveness analysis for all methods in a module (batch processing).
    ///
    /// Runs liveness analysis for ALL methods in the module,
    /// reusing CFGs from module_cfgs.
    ///
    /// ## Performance
    /// - Analyze all methods: ~5-20ms for typical module
    /// - CFGs reused from module_cfgs (no rebuild overhead)
    /// - Expected speedup: 3-5x vs per-method queries (based on unused_local_variable optimization: 6.2x)
    /// - Cached per module (LRU=128)
    fn module_liveness_analysis(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<hir::dataflow::liveness::ModuleLiveness>;

    // ========================================================================
    // Per-Method Dataflow Accessors (Backward Compatible)
    // ========================================================================
    //
    // Note: These are now thin wrappers around module-level queries.
    // They delegate to module_cfgs, module_reaching_definitions, and
    // module_liveness_analysis for efficiency.

    /// Compute reaching definitions for a method.
    ///
    /// Performs dataflow analysis to track which variable definitions reach each program point.
    /// Used by diagnostics that need to resolve variables to their definitions (e.g.,
    /// IncorrectUseOfStrTemplate, UnusedLocalVariable, RewriteMethodParameter).
    ///
    /// ## Algorithm
    ///
    /// Uses Kildall's worklist algorithm with:
    /// - **Lattice**: Set of definitions (union = join)
    /// - **Transfer**: Gen-kill for assignments, var decls, loop variables
    /// - **Convergence**: Typically 2-5 iterations for BSL methods
    ///
    /// ## Performance
    ///
    /// - Initial analysis: 5-10ms for typical 1000-line method
    /// - Cached per method
    /// - Invalidated when method body changes
    ///
    /// ## Returns
    ///
    /// - `Some(ReachingDefsResult)` if analysis succeeds
    /// - `None` if analysis doesn't converge (malformed CFG, infinite loop)
    fn reaching_definitions(
        &self,
        method_id: hir::MethodId,
    ) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>>;

    /// Compute liveness analysis for a method.
    ///
    /// Performs backward dataflow analysis to determine which variables are "live"
    /// (may be read in the future) at each program point. Used to detect unused variables.
    ///
    /// ## Performance
    ///
    /// - Initial analysis: 2-10ms for typical method
    /// - Cached per method (LRU=256)
    /// - Invalidated when method body changes
    ///
    /// ## Returns
    ///
    /// - `Some(DataflowResult<Liveness>)` if analysis succeeds
    /// - `None` if analysis doesn't converge (malformed CFG)
    fn liveness_analysis(
        &self,
        method_id: hir::MethodId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>>;

    /// Get Control Flow Graph (CFG) for a method.
    ///
    /// Constructs CFG from HIR Body, representing the flow of execution through the method.
    /// Used by dataflow analyses (reaching definitions, liveness, etc.) and flow-sensitive
    /// diagnostics.
    ///
    /// ## Performance
    /// - Cached per method (LRU=256)
    /// - Invalidated when method body changes
    /// - O(n) construction where n is number of statements
    /// - Construction time: ~1-2ms for typical 100-line method
    /// - Reused across multiple dataflow analyses
    ///
    /// ## Returns
    /// - `Arc<hir::cfg::ControlFlowGraph>` with basic blocks, control flow edges, entry/exit points
    fn method_cfg(&self, method_id: hir::MethodId) -> Arc<hir::cfg::ControlFlowGraph>;

    /// Get Control Flow Graph (CFG) for module-level code.
    ///
    /// Constructs CFG from HIR Body for code outside procedures/functions.
    /// Similar to method_cfg but for module initialization code.
    ///
    /// ## Performance
    /// - Cached per module (LRU=128)
    /// - Invalidated when module body changes
    ///
    /// ## Returns
    /// - `Arc<hir::cfg::ControlFlowGraph>` with CFG for module-level code, or empty CFG if none
    fn module_level_cfg(&self, module_id: hir::ModuleId) -> Arc<hir::cfg::ControlFlowGraph>;

    /// Compute liveness analysis for module-level code.
    ///
    /// Performs backward dataflow analysis on code outside procedures/functions
    /// to detect unused module-level variables.
    ///
    /// ## Performance
    /// - Cached per module (LRU=128)
    /// - Invalidated when module body changes
    ///
    /// ## Returns
    /// - `Some(DataflowResult<Liveness>)` if analysis succeeds
    /// - `None` if no module-level code or analysis doesn't converge
    fn module_level_liveness_analysis(
        &self,
        module_id: hir::ModuleId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>>;

    // ========================================================================
    // Line Index Query
    // ========================================================================

    /// Get line index for a file (converts byte offsets to line/column positions).
    ///
    /// LineIndex is used for:
    /// - Diagnostics that check multiline conditions (e.g., allowOneliner config)
    /// - LSP position conversions (TextRange → line/column)
    /// - Line-based analysis (line length, empty lines, etc.)
    ///
    /// ## Architecture
    /// LineIndex is cached through Salsa.
    ///
    /// ## Performance
    /// - LRU: 256 files (most recently accessed)
    /// - Construction: O(n) where n = file size (scans for newlines)
    /// - Lookup: O(log n) binary search in line offsets
    /// - Automatic invalidation when file_text changes
    ///
    /// ## Usage
    /// ```ignore
    /// let line_index = db.line_index(file_id_input);
    /// let pos = line_index.line_col(range.start());
    /// println!("Line: {}, Column: {}", pos.line, pos.col);
    /// ```
    fn line_index(&self, file_id_input: base_db::FileIdInput) -> Arc<line_index::LineIndex>;

    /// Downcast to `Any` for accessing implementation-specific methods.
    ///
    /// Used by helper functions to access VFS and file system operations
    /// that are not part of the trait interface.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Get metadata version for cache invalidation.
    ///
    /// Incremented each time VFS detects .xml file changes in the configuration
    /// directory. Passed as `version` to `ConfigurationPathInput::new` so Salsa
    /// creates a distinct interned key and re-runs `load_configuration`.
    fn metadata_version(&self) -> u32;
}
