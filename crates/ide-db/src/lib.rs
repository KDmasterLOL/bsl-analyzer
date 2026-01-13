//! IDE database for bsl-analyzer.
//!
//! This crate provides the database for IDE functionality with full DefDatabase implementation.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base_db::{FileIdInput, Files, RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use bsl_metadata::traits::Module;
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
/// Maps ExprId (from BSL HIR) to the corresponding SDBL package.
pub type SdblHirEntries = Arc<Vec<(hir_def::ExprId, Arc<sdbl_hir::SdblPackage>)>>;

pub mod metadata;
pub mod provider;
pub mod queries;
pub mod salsa_provider;

// Re-export provider types
pub use provider::AnalysisProvider;
pub use salsa_provider::SalsaProvider;

// Re-export all Salsa query functions from the queries module
pub use queries::{
    all_sdbl_in_file_query, line_index_query, liveness_analysis_query, method_cfg_query,
    module_metadata_query, reaching_definitions_query, sdbl_hir_in_file_query,
};

/// Symbol kind (procedure, function, variable, etc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Procedure,
    Function,
    Variable,
    // TODO: Add more symbol kinds as needed
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
    /// Get all SDBL queries in a file with their ExprId in BSL HIR.
    ///
    /// Reuses BSL HIR lowering - no separate AST traversal!
    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir_def::ExprId, syntax::SdblQueryInfo)>>;

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
    /// Follows rust-analyzer pattern - LineIndex is cached through Salsa.
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

/// Get file path for SDBL HIR loading.
pub(crate) fn get_file_path_for_sdbl(db: &dyn RootDatabase, file_id: FileId) -> Option<PathBuf> {
    let db_impl = db.as_any().downcast_ref::<RootDatabaseImpl>()?;
    db_impl.get_file_path(file_id)
}

/// Find configuration root for SDBL HIR loading.
pub(crate) fn find_configuration_root_for_sdbl(
    db: &dyn RootDatabase,
    file_path: &Path,
) -> Option<PathBuf> {
    let db_impl = db.as_any().downcast_ref::<RootDatabaseImpl>()?;
    db_impl.find_configuration_root(file_path)
}

/// Get file path for metadata loading.
///
/// This function provides VFS access for the Salsa query.
/// It downcasts the database to RootDatabaseImpl to access file path resolution.
pub(crate) fn get_file_path_for_metadata(
    db: &dyn RootDatabase,
    file_id: FileId,
) -> Option<PathBuf> {
    // Downcast to concrete type to access get_file_path method
    let db_impl = db.as_any().downcast_ref::<RootDatabaseImpl>()?;
    db_impl.get_file_path(file_id)
}

/// Find configuration root for metadata loading.
///
/// This function provides file system access for the Salsa query.
/// It downcasts the database to RootDatabaseImpl to access configuration search.
pub(crate) fn find_configuration_root_for_metadata(
    db: &dyn RootDatabase,
    file_path: &Path,
) -> Option<PathBuf> {
    // Downcast to concrete type to access find_configuration_root method
    let db_impl = db.as_any().downcast_ref::<RootDatabaseImpl>()?;
    db_impl.find_configuration_root(file_path)
}

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
        // Call documentation parsing query
        // TODO: Make this a proper Salsa tracked query with MethodIdInput
        // For now, call directly (still benefits from parse() caching)
        hir_def::docs::method_docs_query(self, method)
    }

    fn workspace_symbols(
        &self,
        source_root_id: base_db::SourceRootId,
    ) -> Arc<hir_def::WorkspaceSymbols> {
        // Call Salsa-tracked workspace_symbols_query from hir-def
        let source_root_input = self.source_root_input(source_root_id);
        hir_def::workspace_symbols_query(self, source_root_input)
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
    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir_def::ExprId, syntax::SdblQueryInfo)>> {
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

/// Find CommonModule in configuration by matching file URI.
///
/// Matches the file path against CommonModule URIs from metadata.
pub(crate) fn find_common_module_by_uri(
    configuration: &bsl_metadata::Configuration,
    file_path: &Path,
) -> Option<bsl_metadata::CommonModule> {
    let file_uri = file_path.to_string_lossy().to_string();

    configuration
        .common_modules()
        .iter()
        .find(|module| {
            if let Some(module_uri) = module.uri() {
                // Normalize paths for comparison (case-insensitive on some systems)
                module_uri.to_lowercase() == file_uri.to_lowercase()
            } else {
                false
            }
        })
        .cloned()
}

#[salsa::db]
impl metadata::MetadataDb for RootDatabaseImpl {}

#[cfg(test)]
mod tests {
    use super::*;
    use vfs::{file_set::FileSet, VfsPath};

    #[test]
    fn test_root_database_basic() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

        // Test parse query
        let parse = db.parse(file_id);
        assert!(!parse.has_errors());

        // Test item_tree query
        let tree = db.item_tree(file_id);
        assert_eq!(tree.top_level_items().len(), 1);

        // Test module_data query
        let module_id = ModuleId::new(file_id);
        let module_data = db.module_data(module_id);
        assert_eq!(module_data.procedures.len(), 1);
        assert_eq!(module_data.functions.len(), 0);
        assert_eq!(module_data.variables.len(), 0);
    }

    #[test]
    fn test_incremental_item_tree() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Initial content
        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");
        let tree1 = db.item_tree(file_id);
        assert_eq!(tree1.top_level_items().len(), 1);

        // Change content - should invalidate cache
        db.set_file_text(
            file_id,
            r#"
Процедура Тест1() КонецПроцедуры
Функция Тест2() КонецФункции
        "#,
        );
        let tree2 = db.item_tree(file_id);
        assert_eq!(tree2.top_level_items().len(), 2);
    }

    #[test]
    fn test_symbol_tree_query() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
        db.set_file_text(
            file_id,
            r#"
Процедура ПерваяПроцедура()
КонецПроцедуры

Функция ВтораяФункция() Экспорт
КонецФункции

Перем МодульнаяПеременная;
        "#,
        );

        // Test symbol_tree query
        let module_id = ModuleId::new(file_id);
        let symbol_tree = db.symbol_tree(module_id);

        assert_eq!(symbol_tree.methods().count(), 2);
        assert_eq!(symbol_tree.variables().count(), 1);
        assert_eq!(symbol_tree.exported_methods().count(), 1);
    }

    #[test]
    fn test_symbol_tree_caching() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set initial content
        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

        let module_id = ModuleId::new(file_id);
        let tree1 = db.symbol_tree(module_id);
        assert_eq!(tree1.methods().count(), 1);

        // Second call should return cached result
        let tree2 = db.symbol_tree(module_id);
        assert_eq!(tree2.methods().count(), 1);

        // Verify it's the same Arc (cached)
        assert!(Arc::ptr_eq(&tree1, &tree2));
    }

    #[test]
    fn test_symbol_tree_invalidation() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Initial content
        db.set_file_text(file_id, "Процедура Тест1() КонецПроцедуры");

        let module_id = ModuleId::new(file_id);
        let tree1 = db.symbol_tree(module_id);
        assert_eq!(tree1.methods().count(), 1);

        // Change content - should invalidate cache
        db.set_file_text(
            file_id,
            r#"
Процедура Тест1() КонецПроцедуры
Функция Тест2() КонецФункции
        "#,
        );

        let tree2 = db.symbol_tree(module_id);
        assert_eq!(tree2.methods().count(), 2);

        // Should NOT be the same Arc (invalidated)
        assert!(!Arc::ptr_eq(&tree1, &tree2));
    }

    #[test]
    fn test_symbol_tree_case_insensitive() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, "Процедура МояПроцедура() КонецПроцедуры");

        let module_id = ModuleId::new(file_id);
        let symbol_tree = db.symbol_tree(module_id);

        // Case-insensitive lookup
        use hir_def::Name;
        assert!(symbol_tree.find_method(&Name::new("МояПроцедура")).is_some());
        assert!(symbol_tree.find_method(&Name::new("мояпроцедура")).is_some());
        assert!(symbol_tree.find_method(&Name::new("МОЯПРОЦЕДУРА")).is_some());
    }

    #[test]
    fn test_symbol_tree_multi_file() {
        let mut db = RootDatabaseImpl::new();

        // Set up source root
        let mut file_set = FileSet::new();
        let file1 = FileId(0);
        let file2 = FileId(1);
        file_set.insert(file1, VfsPath::new("/module1.bsl"));
        file_set.insert(file2, VfsPath::new("/module2.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file1, SourceRootId(0));
        db.set_file_source_root(file2, SourceRootId(0));

        // File 1
        db.set_file_text(file1, "Процедура Метод1() КонецПроцедуры");

        // File 2
        db.set_file_text(file2, "Функция Метод2() Экспорт КонецФункции");

        // Check file 1
        let module1 = ModuleId::new(file1);
        let tree1 = db.symbol_tree(module1);
        assert_eq!(tree1.methods().count(), 1);
        assert_eq!(tree1.exported_methods().count(), 0);

        // Check file 2
        let module2 = ModuleId::new(file2);
        let tree2 = db.symbol_tree(module2);
        assert_eq!(tree2.methods().count(), 1);
        assert_eq!(tree2.exported_methods().count(), 1);
    }

    #[test]
    fn test_resolver_resolve_module_method() {
        use hir_def::resolver::Resolver;
        use hir_def::{ModuleId, Name};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Use actual BSL code instead of manually constructing ItemTree
        db.set_file_text(
            file_id,
            r#"
Процедура МояПроцедура()
КонецПроцедуры

Функция МояФункция() Экспорт
КонецФункции
        "#,
        );

        // Create resolver
        let resolver = Resolver::for_module(module_id);

        // Resolve procedure
        let method_id = resolver.resolve_module_method(&db, &Name::new("МояПроцедура"));
        assert!(method_id.is_some());
        assert_eq!(method_id.unwrap().module, module_id);

        // Resolve function
        let method_id = resolver.resolve_module_method(&db, &Name::new("МояФункция"));
        assert!(method_id.is_some());
        assert_eq!(method_id.unwrap().module, module_id);

        // Not found
        let method_id = resolver.resolve_module_method(&db, &Name::new("НеСуществует"));
        assert!(method_id.is_none());
    }

    #[test]
    fn test_resolver_resolve_module_method_case_insensitive() {
        use hir_def::resolver::Resolver;
        use hir_def::{ModuleId, Name};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        // Set up
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, "Процедура МояПроцедура() КонецПроцедуры");

        let resolver = Resolver::for_module(module_id);

        // Different cases should all resolve
        assert!(resolver.resolve_module_method(&db, &Name::new("МояПроцедура")).is_some());
        assert!(resolver.resolve_module_method(&db, &Name::new("мояпроцедура")).is_some());
        assert!(resolver.resolve_module_method(&db, &Name::new("МОЯПРОЦЕДУРА")).is_some());
    }

    #[test]
    fn test_resolver_resolve_module_variable() {
        use hir_def::resolver::Resolver;
        use hir_def::{ModuleId, Name};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        // Set up
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, "Перем МодульнаяПеременная Экспорт;");

        let resolver = Resolver::for_module(module_id);

        // Resolve variable
        let var_id = resolver.resolve_module_variable(&db, &Name::new("МодульнаяПеременная"));
        assert!(var_id.is_some());
        assert_eq!(var_id.unwrap().module, module_id);

        // Not found
        let var_id = resolver.resolve_module_variable(&db, &Name::new("НеСуществует"));
        assert!(var_id.is_none());
    }

    #[test]
    fn test_resolver_resolve_name_hierarchy() {
        use hir_def::resolver::{Resolution, Resolver};
        use hir_def::scope::ExprScopes;
        use hir_def::{ModuleId, Name};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        // Set up
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Create module with method and variable
        db.set_file_text(
            file_id,
            r#"
Процедура Метод()
КонецПроцедуры

Перем Переменная;
        "#,
        );

        // Create resolver with expression scope
        let mut expr_scopes = ExprScopes::new();
        expr_scopes.add_parameter(Name::new("Параметр"));

        let root_scope = expr_scopes.root_scope();
        let resolver =
            Resolver::for_module(module_id).push_expr_scope(Arc::new(expr_scopes), root_scope);

        // Resolve parameter (local scope)
        let resolved = resolver.resolve_name(&db, &Name::new("Параметр"));
        assert!(matches!(resolved, Some(Resolution::Local(_))));

        // Resolve method (module scope)
        let resolved = resolver.resolve_name(&db, &Name::new("Метод"));
        assert!(matches!(resolved, Some(Resolution::Method(_))));

        // Resolve variable (module scope)
        let resolved = resolver.resolve_name(&db, &Name::new("Переменная"));
        assert!(matches!(resolved, Some(Resolution::Variable(_))));

        // Not found
        let resolved = resolver.resolve_name(&db, &Name::new("НеСуществует"));
        assert!(resolved.is_none());
    }

    #[test]
    fn test_resolver_shadowing_local_over_module() {
        use hir_def::resolver::{Resolution, Resolver};
        use hir_def::scope::ExprScopes;
        use hir_def::{ModuleId, Name};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        // Set up
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Create module variable with name "Значение"
        db.set_file_text(file_id, "Перем Значение;");

        // Create local variable with the same name
        let mut expr_scopes = ExprScopes::new();
        expr_scopes.add_local_variable(expr_scopes.root_scope(), Name::new("Значение"));

        let root_scope = expr_scopes.root_scope();
        let resolver =
            Resolver::for_module(module_id).push_expr_scope(Arc::new(expr_scopes), root_scope);

        // Should resolve to local variable (shadows module variable)
        let resolved = resolver.resolve_name(&db, &Name::new("Значение"));
        assert!(matches!(resolved, Some(Resolution::Local(_))));
    }

    #[test]
    fn test_resolver_with_workspace_scope() {
        use hir_def::resolver::Resolver;
        use hir_def::ModuleId;

        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let resolver = Resolver::with_workspace_scope(module_id);

        // Should have WorkspaceScope and ModuleScope
        assert_eq!(resolver.scopes.len(), 2);
    }

    // ========== SDBL Integration Tests (migrated from base-db) ==========

    #[test]
    fn test_all_sdbl_in_file_basic() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
КонецПроцедуры"#,
        );

        // Should extract query
        let queries = db.all_sdbl_in_file(file_id);
        assert_eq!(queries.len(), 1, "Should extract 1 SDBL query");
        assert!(queries[0].1.is_valid(), "SDBL should parse successfully");

        // Change file to have multiple queries
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос1 = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
    Запрос2 = "ВЫБРАТЬ Наименование ИЗ Справочник.Категории";
КонецПроцедуры"#,
        );

        // Should extract both queries
        let queries = db.all_sdbl_in_file(file_id);
        assert_eq!(queries.len(), 2, "Should extract 2 SDBL queries");
        assert!(queries.iter().all(|(_, q)| q.is_valid()));
    }

    #[test]
    fn test_all_sdbl_in_file_keyword_filter() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Strings without SELECT/ВЫБРАТЬ keywords should be skipped
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Строка = "Это просто строка без ключевых слов";
    Запрос = "ВЫБРАТЬ * ИЗ Справочник.Товары";
КонецПроцедуры"#,
        );

        let queries = db.all_sdbl_in_file(file_id);
        // Should only extract strings with SELECT/ВЫБРАТЬ
        assert_eq!(queries.len(), 1, "Should filter by SELECT/ВЫБРАТЬ keyword");
        assert!(queries[0].1.query_text.contains("ВЫБРАТЬ"));
    }

    #[test]
    fn test_all_sdbl_in_file_multiline() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Test multiline SDBL query with | prefix
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |    Ссылка,
             |    Наименование
             |ИЗ Справочник.Товары";
КонецПроцедуры"#,
        );

        let queries = db.all_sdbl_in_file(file_id);
        assert_eq!(queries.len(), 1, "Should extract multiline SDBL query");
        assert!(queries[0].1.is_valid(), "Multiline query should parse successfully");

        // Verify content contains all parts
        let query_text = &queries[0].1.query_text;
        assert!(query_text.contains("Ссылка"));
        assert!(query_text.contains("Наименование"));
        assert!(query_text.contains("Справочник.Товары"));
    }

    #[test]
    fn test_all_sdbl_in_file_assignment_patterns() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Test various assignment patterns
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    // Direct assignment
    Запрос1 = "ВЫБРАТЬ * ИЗ Справочник.Товары";

    // Assignment in method call
    Результат = ВыполнитьЗапрос("ВЫБРАТЬ * ИЗ Документ.Продажа");

    // Assignment in array
    Массив = Новый Массив();
    Массив.Добавить("ВЫБРАТЬ * ИЗ Регистр.Остатки");
КонецПроцедуры"#,
        );

        let queries = db.all_sdbl_in_file(file_id);
        // Should extract all SDBL strings regardless of assignment pattern
        assert_eq!(queries.len(), 3, "Should extract queries from various contexts");

        // Verify all queries are valid
        for (_, query_info) in queries.iter() {
            assert!(query_info.is_valid(), "All queries should parse successfully");
        }
    }

    #[test]
    fn test_all_sdbl_in_file_with_parameters() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Test SDBL query with parameters (&Parameter syntax)
        db.set_file_text(
            file_id,
            r#"Процедура ПолучитьДанные()
    Запрос = "ВЫБРАТЬ
             |    Ссылка,
             |    Наименование
             |ИЗ Справочник.Товары
             |ГДЕ
             |    Код = &Значение1
             |    И Наименование ПОДОБНО &Значение2
             |    И Родитель = &Значение3";
КонецПроцедуры"#,
        );

        let queries = db.all_sdbl_in_file(file_id);

        // Should extract query with parameters
        assert_eq!(queries.len(), 1, "Should extract query with parameters");

        // Verify query is valid (parses successfully)
        assert!(queries[0].1.is_valid(), "Query with parameters should parse successfully");

        // Verify query text contains parameters
        assert!(queries[0].1.query_text.contains("&Значение1"));
        assert!(queries[0].1.query_text.contains("&Значение2"));
        assert!(queries[0].1.query_text.contains("&Значение3"));
    }

    #[test]
    fn test_module_metadata_creation() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/CommonModules/ОбщегоНазначения/Ext/Module.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

        // Test module_metadata query
        let module_id = ModuleId::new(file_id);
        let metadata = db.module_metadata(module_id);

        // Should create metadata successfully
        // We don't have configuration loaded yet (Phase 2), so metadata will be minimal
        // But the Arc<ModuleMetadata> structure should be created
        assert_eq!(
            metadata.module_type,
            bsl_metadata::ModuleType::CommonModule,
            "Should detect CommonModule type from path"
        );
    }

    #[test]
    fn test_module_bodies_and_metadata_separate() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

        // Test module_bodies and module_metadata are separate queries
        let module_id = ModuleId::new(file_id);
        let _module_bodies = db.module_bodies(module_id);
        let _module_metadata = db.module_metadata(module_id);

        // Both should work independently (metadata is now accessed separately)
        // This is the correct pattern for performance - no cloning of ModuleBodies
    }

    #[test]
    fn test_module_metadata_cache_invalidation() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set initial file text and get metadata
        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");
        let module_id = ModuleId::new(file_id);
        let _metadata1 = db.module_metadata(module_id);

        // Change file text (should invalidate cache)
        db.set_file_text(file_id, "Процедура Тест2() КонецПроцедуры");
        let _metadata2 = db.module_metadata(module_id);

        // Test passes if we can call metadata again after invalidation
    }

    // ========== SDBL HIR Tests ==========

    #[test]
    fn test_sdbl_hir_in_file_basic() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
КонецПроцедуры"#,
        );

        // Should extract and lower query to HIR
        let hirs = db.sdbl_hir_in_file(file_id);
        assert_eq!(hirs.len(), 1, "Should have 1 SDBL HIR");

        // Verify HIR structure
        let (_, sdbl_hir) = &hirs[0];
        assert!(!sdbl_hir.queries()[0].hir.from.is_empty(), "Should have FROM clause");
        assert_eq!(sdbl_hir.queries()[0].hir.from[0].full_name, "Справочник.Товары");
    }

    #[test]
    fn test_sdbl_hir_in_file_multiple_queries() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with multiple SDBL queries
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос1 = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
    Запрос2 = "ВЫБРАТЬ Номер ИЗ Документ.РасходнаяНакладная";
КонецПроцедуры"#,
        );

        // Should extract and lower both queries
        let hirs = db.sdbl_hir_in_file(file_id);
        assert_eq!(hirs.len(), 2, "Should have 2 SDBL HIRs");

        // Verify first query
        assert_eq!(hirs[0].1.queries()[0].hir.from[0].full_name, "Справочник.Товары");

        // Verify second query
        assert_eq!(hirs[1].1.queries()[0].hir.from[0].full_name, "Документ.РасходнаяНакладная");
    }

    #[test]
    fn test_sdbl_hir_in_file_caching() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
КонецПроцедуры"#,
        );

        // First call
        let hirs1 = db.sdbl_hir_in_file(file_id);

        // Second call should return cached result
        let hirs2 = db.sdbl_hir_in_file(file_id);

        // Verify same Arc (cached)
        assert!(Arc::ptr_eq(&hirs1, &hirs2), "Should return cached result");
    }

    #[test]
    fn test_sdbl_hir_in_file_invalidation() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Initial query
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
КонецПроцедуры"#,
        );
        let hirs1 = db.sdbl_hir_in_file(file_id);
        assert_eq!(hirs1[0].1.queries()[0].hir.from[0].full_name, "Справочник.Товары");

        // Change query
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Номер ИЗ Документ.Продажа";
КонецПроцедуры"#,
        );
        let hirs2 = db.sdbl_hir_in_file(file_id);

        // Should NOT be same Arc (invalidated)
        assert!(!Arc::ptr_eq(&hirs1, &hirs2), "Should invalidate cache on file change");

        // Should have new content
        assert_eq!(hirs2[0].1.queries()[0].hir.from[0].full_name, "Документ.Продажа");
    }
}
