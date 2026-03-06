//! IDE database for bsl-analyzer.
//!
//! This crate provides the database for IDE functionality with full DefDatabase implementation.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base_db::{FileIdInput, Files, RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use hir_def::{
    ConditionalTree, DefDatabase, InferenceResult, ItemTree, ModuleBodies, ModuleData, ModuleId,
    RegionTree, SymbolTree,
};
use vfs::FileId;

// Re-export commonly used types
pub use base_db;
pub use hir_def;
pub use syntax::TextRange;
pub use vfs;

/// Type alias for SDBL HIR entries in a file.
///
/// Maps SdblExprId (unique across all bodies in file) to the corresponding SDBL package.
pub type SdblHirEntries = Arc<Vec<(hir_def::SdblExprId, Arc<sdbl_hir::SdblPackage>)>>;

pub mod metadata;
pub mod provider;
pub mod queries;
pub mod salsa_provider;
pub mod streaming;
pub(crate) mod vfs_helpers;

// Re-export provider types
pub use provider::AnalysisProvider;
pub use salsa_provider::SalsaProvider;
pub use streaming::{
    ClaimResult, FileReader, FileStatus, GlobalContext, ProcessError, SharedState,
    StreamingProvider,
};

// Re-export all Salsa query functions from the queries module
pub use queries::{
    all_sdbl_in_file_query, line_index_query, liveness_analysis_query, method_cfg_query,
    module_metadata_query, reaching_definitions_query, sdbl_hir_in_file_query,
};

// Re-export build_module_metadata for external consumers
pub use metadata::build_module_metadata;

/// Symbol kind (procedure, function, variable, etc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Procedure,
    Function,
    Variable,
    Region,
}

/// Symbol information.
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    // TODO: Add more fields as needed
}

/// The root database for IDE operations.
///
/// This database extends SourceDatabase, RootQueryDb, DefDatabase, HirDatabase, and MetadataDb,
/// providing full HIR functionality, type inference, and metadata support with caching.
#[salsa::db]
pub trait RootDatabase:
    SourceDatabase + RootQueryDb + DefDatabase + hir_ty::db::HirDatabase + metadata::MetadataDb
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

    /// Get all SDBL queries in a file with their SdblExprId.
    ///
    /// Reuses BSL HIR lowering - no separate AST traversal!
    /// SdblExprId uniquely identifies SDBL expression across all bodies in file.
    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir_def::SdblExprId, syntax::SdblQueryInfo)>>;

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
    fn module_cfgs(&self, file_id_input: FileIdInput) -> Arc<cfg::ModuleCfgs>;

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
    ) -> Arc<dataflow::reaching_defs::ModuleReachingDefs>;

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
    ) -> Arc<dataflow::liveness::ModuleLiveness>;

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
        method_id: hir_def::MethodId,
    ) -> Option<Arc<dataflow::reaching_defs::ReachingDefsResult>>;

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
        method_id: hir_def::MethodId,
    ) -> Option<Arc<dataflow::DataflowResult<dataflow::liveness::Liveness>>>;

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
    /// - `Arc<cfg::ControlFlowGraph>` with basic blocks, control flow edges, entry/exit points
    fn method_cfg(&self, method_id: hir_def::MethodId) -> Arc<cfg::ControlFlowGraph>;

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
    /// - `Arc<cfg::ControlFlowGraph>` with CFG for module-level code, or empty CFG if none
    fn module_level_cfg(&self, module_id: hir_def::ModuleId) -> Arc<cfg::ControlFlowGraph>;

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
        module_id: hir_def::ModuleId,
    ) -> Option<Arc<dataflow::DataflowResult<dataflow::liveness::Liveness>>>;

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
}

// Note: All Salsa query implementations have been moved to the `queries` module.
// See `queries.rs` for the full list of IDE-level queries.

/// Default implementation of RootDatabase with Salsa integration.
///
/// All HIR queries are now managed by Salsa for automatic caching and invalidation!
/// No manual DashMap caches needed for HIR - Salsa handles everything.
#[salsa::db]
#[derive(Clone)]
pub struct RootDatabaseImpl {
    /// Salsa storage for incremental computation
    storage: salsa::Storage<Self>,

    /// Base file storage
    files: Files,
    // ✅ ALL HIR queries migrated to Salsa! (Phase 1-6 complete)
    // - item_tree, region_tree, conditional_tree, module_data, symbol_tree
    // - infer_types, module_bodies, module_metadata
    // - all_sdbl_in_file, sdbl_hir_in_file
    // - reaching_definitions (Phase 6.5)
    // No more manual DashMap caches!
}

impl Default for RootDatabaseImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl RootDatabaseImpl {
    /// Create a new empty database.
    pub fn new() -> Self {
        Self { storage: salsa::Storage::default(), files: Files::new() }
    }

    /// Get file path from FileId by traversing SourceRoot.
    ///
    /// Returns None if path cannot be resolved.
    fn get_file_path(&self, file_id: FileId) -> Option<PathBuf> {
        let source_root_input = self.file_source_root_input(file_id);
        let source_root_id = source_root_input.source_root_id(self);
        let source_root_input = self.source_root_input(source_root_id);
        let source_root = source_root_input.root(self);
        let file_set = source_root.file_set();
        let vfs_path = file_set.path_for_file(&file_id)?;
        Some(PathBuf::from(vfs_path.as_path()))
    }

    /// Find configuration root directory by searching for Configuration.xml.
    ///
    /// Algorithm:
    /// 1. Start from file's directory
    /// 2. Look for CommonModules/ subdirectory or Configuration.xml
    /// 3. Walk up parent directories until found or root reached
    ///
    /// Returns None if configuration cannot be found.
    fn find_configuration_root(&self, file_path: &Path) -> Option<PathBuf> {
        let mut current = file_path.parent()?;

        // Walk up the directory tree looking for Configuration markers
        loop {
            // Check if CommonModules directory exists (typical Designer format structure)
            let common_modules = current.join("CommonModules");
            if common_modules.is_dir() {
                tracing::debug!(?current, "Found configuration root via CommonModules/");
                return Some(current.to_path_buf());
            }

            // Check if Configuration.xml exists
            let config_xml = current.join("Configuration.xml");
            if config_xml.is_file() {
                tracing::debug!(?current, "Found configuration root via Configuration.xml");
                return Some(current.to_path_buf());
            }

            // Move to parent directory
            current = match current.parent() {
                Some(parent) if parent != current => parent,
                _ => return None, // Reached root without finding config
            };
        }
    }

    /// Invalidate manual HIR caches for a file.
    ///
    /// Called when file content changes.
    ///
    /// Note: All HIR queries are now Salsa-managed and invalidated automatically!
    /// This method is kept for potential future non-Salsa caches.
    fn invalidate_file(&self, _file_id: FileId) {
        // All HIR queries migrated to Salsa - no manual invalidation needed!
        // Salsa automatically invalidates:
        // - item_tree, region_tree, conditional_tree, module_data, symbol_tree
        // - infer_types, module_bodies, module_metadata
        // - all_sdbl_in_file, sdbl_hir_in_file
    }
}

#[salsa::db]
impl salsa::Database for RootDatabaseImpl {}

#[salsa::db]
impl SourceDatabase for RootDatabaseImpl {
    fn file_text_input(&self, file_id: FileId) -> base_db::FileTextInput {
        self.files.file_text(file_id)
    }

    fn source_root_input(&self, source_root_id: SourceRootId) -> base_db::SourceRootInput {
        self.files.source_root(source_root_id)
    }

    fn file_source_root_input(&self, file_id: FileId) -> base_db::FileSourceRootInput {
        self.files.file_source_root(file_id)
    }

    fn set_file_text(&mut self, file_id: FileId, text: &str) {
        let files = self.files.clone();
        // Use smart durability detection based on source root (library vs user code)
        // This ensures library files get HIGH durability, user files get LOW
        files.set_file_text_smart(self, file_id, text);
        // Salsa automatically invalidates parse query
        // But we need to manually invalidate HIR caches for now
        self.invalidate_file(file_id);
    }

    fn set_file_source_root(&mut self, file_id: FileId, source_root_id: SourceRootId) {
        let files = self.files.clone();
        files.set_file_source_root(self, file_id, source_root_id);
    }

    fn set_source_root(&mut self, source_root_id: SourceRootId, source_root: SourceRoot) {
        let files = self.files.clone();
        files.set_source_root(self, source_root_id, source_root);
    }

    fn resolve_vfs_path(
        &self,
        source_root_id: SourceRootId,
        vfs_path: &vfs::VfsPath,
    ) -> Option<FileId> {
        let source_root_input = self.source_root_input(source_root_id);
        let vfs_path_str = vfs_path.as_path().to_string_lossy().to_string();
        base_db::resolve_vfs_path_query(self, source_root_input, vfs_path_str)
    }
}

#[salsa::db]
impl RootQueryDb for RootDatabaseImpl {
    fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::SyntaxNode> {
        let input = self.file_text_input(file_id);
        base_db::parse_query(self, input)
    }

    fn method_regions(
        &self,
        file_id: FileId,
    ) -> Arc<std::collections::HashMap<syntax::TextRange, String>> {
        let input = self.file_text_input(file_id);
        base_db::method_regions_query(self, input)
    }

    fn module_level_regions(&self, file_id: FileId) -> Arc<Vec<base_db::RegionInfo>> {
        let input = self.file_text_input(file_id);
        base_db::module_level_regions_query(self, input)
    }
}

#[salsa::db]
impl DefDatabase for RootDatabaseImpl {
    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree> {
        // Use Salsa tracked query with FileIdInput
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        hir_def::item_tree_query(self, file_id_input)
    }

    fn region_tree(&self, file_id: FileId) -> Arc<RegionTree> {
        // Use Salsa tracked query with FileIdInput
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        hir_def::region_tree_query(self, file_id_input)
    }

    fn conditional_tree(&self, file_id: FileId) -> Arc<ConditionalTree> {
        // Use Salsa tracked query with FileIdInput
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        hir_def::conditional_tree_query(self, file_id_input)
    }

    fn module_data(&self, module_id: ModuleId) -> Arc<ModuleData> {
        // Use Salsa tracked query with FileIdInput
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir_def::module_data_query(self, file_id_input)
    }

    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree> {
        // Use Salsa tracked query with FileIdInput
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir_def::symbol_tree_query(self, file_id_input)
    }

    fn infer_types(&self, module_id: ModuleId) -> Arc<InferenceResult> {
        // Call Salsa tracked query with FileIdInput
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir_def::infer_types_query(self, file_id_input)
    }

    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies> {
        // Call Salsa tracked query to get lowered bodies
        // NOTE: Return Arc directly without cloning! Metadata is accessed separately
        // via module_metadata() query. This is critical for performance - cloning
        // ModuleBodies was causing massive memory overhead.
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir_def::module_bodies_query(self, file_id_input)
    }

    fn module_metadata(&self, module_id: ModuleId) -> Arc<hir_def::ModuleMetadata> {
        // Call Salsa tracked query (caching handled by Salsa)
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        module_metadata_query(self, file_id_input)
    }

    fn method_docs(&self, method: hir_def::MethodId) -> Option<Arc<hir_def::docs::MethodDocs>> {
        // Get docs from SymbolTree (parsed once during SymbolTree construction)
        let symbol_tree = self.symbol_tree(method.module);
        let method_symbol = symbol_tree.find_method_by_id(method)?;
        method_symbol.docs.clone()
    }

    fn workspace_symbols(
        &self,
        source_root_id: base_db::SourceRootId,
    ) -> Arc<hir_def::WorkspaceSymbols> {
        // Call Salsa-tracked workspace_symbols_query from hir-def
        let source_root_input = self.source_root_input(source_root_id);
        hir_def::workspace_symbols_query(self, source_root_input)
    }

    fn workspace_index(
        &self,
        source_root_id: base_db::SourceRootId,
    ) -> Arc<hir_def::WorkspaceIndex> {
        // Call Salsa-tracked workspace_index_query from hir-def
        let source_root_input = self.source_root_input(source_root_id);
        hir_def::workspace_index_query(self, source_root_input)
    }

    fn file_external_refs(&self, module_id: ModuleId) -> Arc<Vec<hir_def::ExternalRef>> {
        // Call Salsa tracked query with FileIdInput
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir_def::file_external_refs_query(self, file_id_input)
    }

    fn module_index(&self, source_root_id: base_db::SourceRootId) -> Arc<hir_def::ModuleIndex> {
        // Call Salsa-tracked module_index_query from hir-def
        let source_root_input = self.source_root_input(source_root_id);
        hir_def::module_index_query(self, source_root_input)
    }

    fn file_dependencies(&self, module_id: ModuleId) -> Arc<Vec<FileId>> {
        // Call Salsa tracked query with FileIdInput
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir_def::file_dependencies_query(self, file_id_input)
    }
}

#[salsa::db]
impl hir_ty::db::HirDatabase for RootDatabaseImpl {
    fn infer(&self, file_id: FileId) -> Arc<hir_ty::InferenceResult> {
        // Call hir-ty query function
        hir_ty::infer::infer_query(self, file_id)
    }

    fn type_of_expr(&self, file_id: FileId, expr: hir_def::ExprId) -> hir_ty::Ty {
        // Call hir-ty query function
        hir_ty::infer::type_of_expr_query(self, file_id, expr)
    }
}

#[salsa::db]
impl RootDatabase for RootDatabaseImpl {
    fn get_configuration(&self, file_id: FileId) -> Option<Arc<bsl_metadata::Configuration>> {
        // Reuse the same logic as sdbl_hir_in_file_query
        let file_path = vfs_helpers::get_file_path_for_sdbl(self, file_id)?;
        let config_root = vfs_helpers::find_configuration_root_for_sdbl(self, &file_path)?;
        let config_path_str = config_root.to_string_lossy().to_string();
        let path_input = metadata::ConfigurationPathInput::new(self, config_path_str);
        // Salsa-cached via load_configuration
        Some(metadata::load_configuration(self, path_input))
    }

    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir_def::SdblExprId, syntax::SdblQueryInfo)>> {
        // Call Salsa tracked query (caching handled by Salsa)
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        all_sdbl_in_file_query(self, file_id_input)
    }

    fn sdbl_hir_in_file(&self, file_id: FileId) -> SdblHirEntries {
        // Call Salsa tracked query (caching handled by Salsa)
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        sdbl_hir_in_file_query(self, file_id_input)
    }

    // Module-level dataflow queries (batch processing)

    fn module_cfgs(&self, file_id_input: FileIdInput) -> Arc<cfg::ModuleCfgs> {
        // Call Salsa tracked query - builds CFGs for all methods at once
        queries::module_cfgs_query(self, file_id_input)
    }

    fn module_reaching_definitions(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<dataflow::reaching_defs::ModuleReachingDefs> {
        // Call Salsa tracked query - analyzes all methods with shared CFGs
        queries::module_reaching_definitions_query(self, file_id_input)
    }

    fn module_liveness_analysis(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<dataflow::liveness::ModuleLiveness> {
        // Call Salsa tracked query - analyzes all methods with shared CFGs
        queries::module_liveness_analysis_query(self, file_id_input)
    }

    // Per-method dataflow accessors (backward compatible wrappers)

    fn reaching_definitions(
        &self,
        method_id: hir_def::MethodId,
    ) -> Option<Arc<dataflow::reaching_defs::ReachingDefsResult>> {
        // Call Salsa tracked query (Phase 6.5 - automatic caching & invalidation)
        let method_id_input = hir_def::MethodIdInput::new(self, method_id);
        reaching_definitions_query(self, method_id_input)
    }

    fn liveness_analysis(
        &self,
        method_id: hir_def::MethodId,
    ) -> Option<Arc<dataflow::DataflowResult<dataflow::liveness::Liveness>>> {
        // Call Salsa tracked query (backward dataflow analysis)
        let method_id_input = hir_def::MethodIdInput::new(self, method_id);
        liveness_analysis_query(self, method_id_input)
    }

    fn method_cfg(&self, method_id: hir_def::MethodId) -> Arc<cfg::ControlFlowGraph> {
        // Call Salsa tracked query (Phase 6.6 - CFG caching & reuse)
        let method_id_input = hir_def::MethodIdInput::new(self, method_id);
        method_cfg_query(self, method_id_input)
    }

    fn module_level_cfg(&self, module_id: hir_def::ModuleId) -> Arc<cfg::ControlFlowGraph> {
        // Call Salsa tracked query for module-level code CFG
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        queries::module_level_cfg_query(self, file_id_input)
    }

    fn module_level_liveness_analysis(
        &self,
        module_id: hir_def::ModuleId,
    ) -> Option<Arc<dataflow::DataflowResult<dataflow::liveness::Liveness>>> {
        // Call Salsa tracked query for module-level liveness analysis
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        queries::module_level_liveness_analysis_query(self, file_id_input)
    }

    fn line_index(&self, file_id_input: base_db::FileIdInput) -> Arc<line_index::LineIndex> {
        // Call Salsa tracked query - cached per file
        line_index_query(self, file_id_input)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[salsa::db]
impl metadata::MetadataDb for RootDatabaseImpl {}

#[cfg(test)]
mod database_impl_tests;
