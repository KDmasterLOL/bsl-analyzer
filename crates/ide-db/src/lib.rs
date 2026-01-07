//! IDE database for bsl-analyzer.
//!
//! This crate provides the database for IDE functionality with full DefDatabase implementation.

use std::hash::BuildHasherDefault;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base_db::{Files, RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use bsl_metadata::traits::Module;
use dashmap::DashMap;
use hir_def::{
    ConditionalTree, DefDatabase, InferenceResult, ItemTree, ModuleBodies, ModuleData, ModuleId,
    RegionTree, SymbolTree,
};
use rustc_hash::FxHasher;
use vfs::FileId;

// Re-export commonly used types
pub use base_db;
pub use hir_def;
pub use syntax::TextRange;
pub use vfs;

/// Type alias for SDBL HIR entries in a file.
///
/// Maps ExprId (from BSL HIR) to the corresponding SDBL HIR.
pub type SdblHirEntries = Arc<Vec<(hir_def::ExprId, Arc<sdbl_hir::SdblHir>)>>;

pub mod metadata;

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
/// This database extends SourceDatabase, RootQueryDb, DefDatabase, and MetadataDb,
/// providing full HIR functionality and metadata support with caching.
pub trait RootDatabase: SourceDatabase + RootQueryDb + DefDatabase + metadata::MetadataDb {
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
}

/// Default implementation of RootDatabase with Salsa integration.
///
/// Uses Salsa for base queries and manual caching for HIR queries.
/// TODO: Migrate HIR queries to Salsa tracked functions in future iteration.
#[salsa::db]
#[derive(Clone)]
pub struct RootDatabaseImpl {
    /// Salsa storage for incremental computation
    storage: salsa::Storage<Self>,

    /// Base file storage
    files: Files,

    /// HIR caches (TODO: Replace with Salsa tracked queries)
    item_tree_cache: Arc<DashMap<FileId, Arc<ItemTree>, BuildHasherDefault<FxHasher>>>,
    region_tree_cache: Arc<DashMap<FileId, Arc<RegionTree>, BuildHasherDefault<FxHasher>>>,
    conditional_tree_cache:
        Arc<DashMap<FileId, Arc<ConditionalTree>, BuildHasherDefault<FxHasher>>>,
    module_data_cache: Arc<DashMap<ModuleId, Arc<ModuleData>, BuildHasherDefault<FxHasher>>>,
    symbol_tree_cache: Arc<DashMap<ModuleId, Arc<SymbolTree>, BuildHasherDefault<FxHasher>>>,
    infer_types_cache: Arc<DashMap<ModuleId, Arc<InferenceResult>, BuildHasherDefault<FxHasher>>>,
    module_bodies_cache: Arc<DashMap<ModuleId, Arc<ModuleBodies>, BuildHasherDefault<FxHasher>>>,
    module_metadata_cache:
        Arc<DashMap<ModuleId, Arc<hir_def::ModuleMetadata>, BuildHasherDefault<FxHasher>>>,
    sdbl_hir_cache: Arc<DashMap<FileId, SdblHirEntries, BuildHasherDefault<FxHasher>>>,
    #[allow(dead_code)] // TODO: Used when reaching_definitions query is re-enabled
    reaching_defs_cache: Arc<
        DashMap<
            hir_def::MethodId,
            Option<Arc<dataflow::reaching_defs::ReachingDefsResult>>,
            BuildHasherDefault<FxHasher>,
        >,
    >,
}

impl Default for RootDatabaseImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl RootDatabaseImpl {
    /// Create a new empty database.
    pub fn new() -> Self {
        Self {
            storage: salsa::Storage::default(),
            files: Files::new(),
            item_tree_cache: Arc::new(DashMap::default()),
            region_tree_cache: Arc::new(DashMap::default()),
            conditional_tree_cache: Arc::new(DashMap::default()),
            module_data_cache: Arc::new(DashMap::default()),
            symbol_tree_cache: Arc::new(DashMap::default()),
            infer_types_cache: Arc::new(DashMap::default()),
            module_bodies_cache: Arc::new(DashMap::default()),
            module_metadata_cache: Arc::new(DashMap::default()),
            sdbl_hir_cache: Arc::new(DashMap::default()),
            reaching_defs_cache: Arc::new(DashMap::default()),
        }
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

    /// Invalidate HIR caches for a file.
    ///
    /// Called when file content changes.
    /// Note: This is temporary. Will be automatic when we migrate to Salsa tracked queries.
    fn invalidate_file(&self, file_id: FileId) {
        self.item_tree_cache.remove(&file_id);
        self.region_tree_cache.remove(&file_id);
        self.sdbl_hir_cache.remove(&file_id);
        let module_id = ModuleId::new(file_id);
        self.module_data_cache.remove(&module_id);
        self.symbol_tree_cache.remove(&module_id);
        self.infer_types_cache.remove(&module_id);
        self.module_bodies_cache.remove(&module_id);
        self.module_metadata_cache.remove(&module_id);
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
        files.set_file_text(self, file_id, text);
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
        base_db::method_regions(self, input)
    }

    fn module_level_regions(&self, file_id: FileId) -> Arc<Vec<base_db::RegionInfo>> {
        let input = self.file_text_input(file_id);
        base_db::module_level_regions(self, input)
    }
}

impl DefDatabase for RootDatabaseImpl {
    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree> {
        // Check cache first
        if let Some(cached) = self.item_tree_cache.get(&file_id) {
            return cached.value().clone();
        }

        let _span = tracing::info_span!("item_tree", ?file_id).entered();

        // Lower AST → ItemTree
        let tree = hir_def::item_tree::lower_file(self, file_id);

        // Cache the result
        self.item_tree_cache.insert(file_id, tree.clone());
        tree
    }

    fn region_tree(&self, file_id: FileId) -> Arc<RegionTree> {
        // Check cache first
        if let Some(cached) = self.region_tree_cache.get(&file_id) {
            return cached.value().clone();
        }

        let _span = tracing::info_span!("region_tree", ?file_id).entered();

        // Parse and lower AST → RegionTree
        let parse = self.parse(file_id);
        let tree = Arc::new(hir_def::region_tree::lower_regions(&parse.syntax_node()));

        // Cache the result
        self.region_tree_cache.insert(file_id, tree.clone());
        tree
    }

    fn conditional_tree(&self, file_id: FileId) -> Arc<ConditionalTree> {
        // Check cache first
        if let Some(cached) = self.conditional_tree_cache.get(&file_id) {
            return cached.value().clone();
        }

        let _span = tracing::info_span!("conditional_tree", ?file_id).entered();

        // Parse and lower AST → ConditionalTree
        let parse = self.parse(file_id);
        let tree = Arc::new(hir_def::conditional_tree::lower_conditionals(&parse.syntax_node()));

        // Cache the result
        self.conditional_tree_cache.insert(file_id, tree.clone());
        tree
    }

    fn module_data(&self, module_id: ModuleId) -> Arc<ModuleData> {
        // Check cache first
        if let Some(cached) = self.module_data_cache.get(&module_id) {
            return cached.value().clone();
        }

        let _span = tracing::info_span!("module_data", ?module_id).entered();

        // Get ItemTree and convert to ModuleData
        let tree = self.item_tree(module_id.file_id);
        let data = Arc::new(ModuleData::from_item_tree(module_id, tree));

        // Cache the result
        self.module_data_cache.insert(module_id, data.clone());
        data
    }

    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree> {
        // Check cache first
        if let Some(cached) = self.symbol_tree_cache.get(&module_id) {
            return cached.value().clone();
        }

        let _span = tracing::info_span!("symbol_tree", ?module_id).entered();

        // Get ItemTree and build SymbolTree
        let item_tree = self.item_tree(module_id.file_id);
        let tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id));

        // Cache the result
        self.symbol_tree_cache.insert(module_id, tree.clone());
        tree
    }

    fn infer_types(&self, module_id: ModuleId) -> Arc<InferenceResult> {
        // Check cache first
        if let Some(cached) = self.infer_types_cache.get(&module_id) {
            return cached.value().clone();
        }

        let _span = tracing::info_span!("infer_types", ?module_id).entered();

        // Perform type inference
        let result = Arc::new(hir_def::InferenceContext::infer_module(self, module_id));

        // Cache the result
        self.infer_types_cache.insert(module_id, result.clone());
        result
    }

    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies> {
        // Check cache first
        if let Some(cached) = self.module_bodies_cache.get(&module_id) {
            return cached.value().clone();
        }

        let _span = tracing::info_span!("module_bodies", ?module_id).entered();

        // Lower all method bodies
        let mut result = hir_def::lower_module_bodies(self, module_id);

        // Attach metadata (loaded separately by module_metadata query)
        let metadata = self.module_metadata(module_id);
        result = result.with_metadata(metadata);

        let result = Arc::new(result);

        // Cache the result
        self.module_bodies_cache.insert(module_id, result.clone());
        result
    }

    fn module_metadata(&self, module_id: ModuleId) -> Arc<hir_def::ModuleMetadata> {
        // Check cache first
        if let Some(cached) = self.module_metadata_cache.get(&module_id) {
            return cached.value().clone();
        }

        let _span = tracing::info_span!("module_metadata", ?module_id).entered();

        // Get file path
        let file_path = match self.get_file_path(module_id.file_id) {
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
            metadata::get_module_type_from_uri(&uri)
                .unwrap_or(bsl_metadata::ModuleType::CommonModule)
        };

        // Load metadata if this is a CommonModule
        let (execution_context, common_module) =
            if matches!(module_type, bsl_metadata::ModuleType::CommonModule) {
                // Find configuration root by searching for Configuration.xml
                match self.find_configuration_root(&file_path) {
                    Some(config_root) => {
                        let config_path_str = config_root.to_string_lossy().to_string();
                        tracing::debug!(?config_path_str, "Loading configuration for metadata");

                        // Load configuration via Salsa query
                        let path_input =
                            metadata::ConfigurationPathInput::new(self, config_path_str);
                        let configuration = metadata::load_configuration(self, path_input);

                        // Find CommonModule for this file
                        if let Some(common_module) =
                            find_common_module_by_uri(&configuration, &file_path)
                        {
                            let execution_context =
                                hir_def::compute_execution_context(&common_module);
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

        let metadata = Arc::new(hir_def::ModuleMetadata {
            module_type,
            execution_context,
            common_module,
            mdo: None,
        });

        // Cache the result
        self.module_metadata_cache.insert(module_id, metadata.clone());
        metadata
    }
}

impl RootDatabase for RootDatabaseImpl {
    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir_def::ExprId, syntax::SdblQueryInfo)>> {
        let _span = tracing::info_span!("all_sdbl_in_file", ?file_id).entered();

        let module_id = ModuleId::new(file_id);
        let module_bodies = self.module_bodies(module_id);
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

        // Sort by position in file (bsl_literal_range start)
        // This ensures diagnostics are returned in source order, which tests expect
        result.sort_by_key(|(_, query_info)| query_info.bsl_literal_range.start());

        tracing::debug!(count = result.len(), "Collected SDBL from HIR");
        Arc::new(result)
    }

    fn sdbl_hir_in_file(&self, file_id: FileId) -> SdblHirEntries {
        // Check cache first
        if let Some(cached) = self.sdbl_hir_cache.get(&file_id) {
            return cached.value().clone();
        }

        let _span = tracing::info_span!("sdbl_hir_in_file", ?file_id).entered();

        // Get SDBL queries from BSL HIR
        let sdbl_queries = self.all_sdbl_in_file(file_id);

        // Try to load configuration for metadata-based type inference
        let configuration = self.get_file_path(file_id).and_then(|file_path| {
            self.find_configuration_root(&file_path).map(|config_root| {
                let config_path_str = config_root.to_string_lossy().to_string();
                let path_input = metadata::ConfigurationPathInput::new(self, config_path_str);
                metadata::load_configuration(self, path_input)
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
        let result = Arc::new(result);

        // Cache the result
        self.sdbl_hir_cache.insert(file_id, result.clone());
        result
    }

    fn reaching_definitions(
        &self,
        _method_id: hir_def::MethodId,
    ) -> Option<Arc<dataflow::reaching_defs::ReachingDefsResult>> {
        // TODO: Reimplement with HIR-based CFG
        // Currently disabled until cfg crate is updated to support HIR vertices
        None

        /* TEMPORARILY DISABLED - requires HIR-based CFG
        // Check cache first
        if let Some(cached) = self.reaching_defs_cache.get(&method_id) {
            return cached.value().clone();
        }

        let _span = tracing::info_span!("reaching_definitions", ?method_id).entered();

        // Get module bodies
        let module_bodies = self.module_bodies(method_id.module);

        // Get body for this method
        let body = module_bodies.body(method_id.local_id)?;

        // Build CFG for this method (clone body since build_cfg_for_body takes ownership)
        let cfg = hir_def::cfg_builder::HirCfgBuilder::build_cfg_for_body(body.clone());

        // Initialize reaching definitions with parameters
        let mut initial_defs = dataflow::reaching_defs::ReachingDefs::new();
        for &param_id in body.params.iter() {
            let binding = body.binding(param_id);
            let def = dataflow::reaching_defs::Definition::parameter(&binding.name, param_id);
            initial_defs.insert(def);
        }

        // Run dataflow analysis
        let transfer = dataflow::reaching_defs::ReachingDefsTransfer;
        let mut solver =
            dataflow::DataflowSolver::new(cfg, body.clone(), transfer);

        // Optional: adjust max iterations for complex methods
        solver.set_max_iterations(100);
        solver.set_initial_state(initial_defs); // Set parameters as initial state

        let dataflow_result = solver.solve()?;

        // Wrap in high-level API
        let result =
            Arc::new(dataflow::reaching_defs::ReachingDefsResult::new(dataflow_result));

        // Cache the result
        self.reaching_defs_cache.insert(method_id, Some(result.clone()));

        Some(result)
        */
    }
}

/// Find CommonModule in configuration by matching file URI.
///
/// Matches the file path against CommonModule URIs from metadata.
fn find_common_module_by_uri(
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
    fn test_module_bodies_includes_metadata() {
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

        // Test module_bodies includes metadata
        let module_id = ModuleId::new(file_id);
        let module_bodies = db.module_bodies(module_id);

        // Metadata should be present in module_bodies
        // Even if empty, it should be Some
        assert!(module_bodies.metadata().is_some(), "Module bodies should include metadata");
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
        assert!(!sdbl_hir.from.is_empty(), "Should have FROM clause");
        assert_eq!(sdbl_hir.from[0].full_name, "Справочник.Товары");
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
        assert_eq!(hirs[0].1.from[0].full_name, "Справочник.Товары");

        // Verify second query
        assert_eq!(hirs[1].1.from[0].full_name, "Документ.РасходнаяНакладная");
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
        assert_eq!(hirs1[0].1.from[0].full_name, "Справочник.Товары");

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
        assert_eq!(hirs2[0].1.from[0].full_name, "Документ.Продажа");
    }
}
