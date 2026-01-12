//! Salsa tracked queries for hir-def.
//!
//! This module provides a central registry of all HIR-level queries.
//! Queries are organized into logical groups based on their functionality.
//!
//! # Query Organization
//!
//! **Invalidation Barrier Queries (AST → HIR metadata):**
//! - [`item_tree_query`] - Method/variable signatures
//! - [`region_tree_query`] - Preprocessor region hierarchy
//! - [`conditional_tree_query`] - Preprocessor conditional hierarchy
//!
//! **Derived Queries (depend on ItemTree):**
//! - [`symbol_tree_query`] - Case-insensitive symbol lookup
//! - [`module_data_query`] - Module-level data
//!
//! **HIR Lowering (AST → HIR bodies):**
//! - [`module_bodies_query`] - Lower method bodies + diagnostics
//!
//! **Type Inference:**
//! - [`infer_types_query`] - Type inference for module
//!
//! **Metadata:**
//! - [`module_metadata_query`] - Module type and execution context

use std::sync::Arc;

use base_db::FileIdInput;

use vfs::FileId;

use crate::{
    body::ExternalRef, module_index::ModuleIndex, ty::infer::InferenceResult, DefDatabase,
    ModuleBodies, ModuleData, ModuleId, ModuleMetadata, WorkspaceSymbols,
};

// Re-export query functions from individual modules
pub use crate::conditional_tree::conditional_tree_query;
pub use crate::item_tree::item_tree_query;
pub use crate::region_tree::region_tree_query;
pub use crate::symbol_tree::symbol_tree_query;

/// Get module data (derived from ItemTree).
///
/// ModuleData is a simplified view of ItemTree containing lists of procedures,
/// functions, and variables with their IDs.
///
/// ## Salsa caching
/// - LRU: 512 (derived query, cheap to compute)
/// - Invalidation: Automatic when ItemTree changes
/// - Dependency: calls item_tree() internally
///
/// ## Performance
/// - Computation: ~1ms (just extracts data from ItemTree)
/// - Cached access: < 1ms
///
/// ## Usage
/// ```ignore
/// // In DefDatabase implementation:
/// fn module_data(&self, module_id: ModuleId) -> Arc<ModuleData> {
///     let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
///     hir_def::module_data_query(self, file_id_input)
/// }
/// ```
#[salsa::tracked(lru = 512)]
pub fn module_data_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleData> {
    let _span = tracing::info_span!("module_data", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let tree = db.item_tree(file_id);
    let module_id = ModuleId::new(file_id);
    Arc::new(ModuleData::from_item_tree(module_id, tree))
}

/// Lower all method bodies in a module and collect diagnostics.
///
/// This is the main query for body lowering. It:
/// 1. Lowers all procedure and function bodies to HIR
/// 2. Collects diagnostics during lowering (MissingReturn, UnreachableCode, etc.)
/// 3. Attaches module metadata for context-sensitive checks
///
/// ## Salsa caching
/// - LRU: 128 (heavy lowering operation)
/// - Invalidation: Automatic when file content changes
/// - Dependency: calls module_metadata_query internally
///
/// ## Performance
/// - Lowering: ~5-10ms for typical 1000-line module
/// - Cached access: < 1ms
///
/// ## Usage
/// ```ignore
/// // In DefDatabase implementation:
/// fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies> {
///     let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
///     hir_def::module_bodies_query(self, file_id_input)
/// }
/// ```
#[salsa::tracked(lru = 128)]
pub fn module_bodies_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleBodies> {
    let _span = tracing::info_span!("module_bodies", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);

    // Lower all method bodies
    let result = crate::lower_module_bodies(db, module_id);

    // Note: Metadata is NOT attached here - it's attached by the DefDatabase
    // implementation in ide-db where VFS access is available for loading Configuration.
    // This keeps hir-def independent of VFS.
    Arc::new(result)
}

/// Get metadata for a module (type and execution context).
///
/// Loads metadata from 1C Configuration if available. Used by:
/// - ModuleBodies query (attaches metadata to bodies)
/// - Metadata-based diagnostics (naming rules, API requirements)
///
/// ## Salsa caching
/// - LRU: 128 (metadata loading is I/O intensive)
/// - Invalidation: Automatic when file content or configuration changes
/// - Shared: load_configuration() is cached separately (LRU=16)
///
/// ## Implementation note
/// This query is implemented in ide-db (not hir-def) because it needs access to:
/// - VFS for file path resolution
/// - Configuration loading infrastructure
///
/// For now, this is a placeholder. Actual implementation is in RootDatabaseImpl.
///
/// ## Usage
/// ```ignore
/// // In DefDatabase implementation:
/// fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata> {
///     let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
///     // Actual query implemented in ide-db
///     self.module_metadata_impl(file_id_input)
/// }
/// ```
#[salsa::tracked(lru = 128)]
pub fn module_metadata_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleMetadata> {
    let _span = tracing::info_span!("module_metadata", ?file_id_input).entered();
    let _file_id = file_id_input.file_id(db);

    // TODO: This should be moved to ide-db where VFS access is available
    // For now, return unknown metadata
    Arc::new(ModuleMetadata::unknown(bsl_metadata::ModuleType::Unknown))
}

/// Salsa tracked query for type inference.
///
/// Performs type inference for all expressions, variables, and methods in a module.
///
/// ## Performance
/// - LRU: 256 files (type inference is moderately expensive)
/// - Depends on: ItemTree (via FileIdInput)
/// - Invalidation: Automatic when signatures change
///
/// ## Usage
/// ```ignore
/// // In DefDatabase implementation:
/// fn infer_types(&self, module_id: ModuleId) -> Arc<InferenceResult> {
///     let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
///     hir_def::infer_types_query(self, file_id_input)
/// }
/// ```
#[salsa::tracked(lru = 256)]
pub fn infer_types_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<InferenceResult> {
    let _span = tracing::info_span!("infer_types", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);
    Arc::new(crate::ty::infer::InferenceContext::infer_module(db, module_id))
}

/// Build workspace-wide symbol index for CommonModules.
///
/// This function creates a global index of all CommonModules in the source root,
/// enabling O(1) lookup for qualified name resolution (e.g., `ОбщийМодуль.Метод()`).
///
/// ## Performance
/// - **Computation:** O(n×m) where n = files, m = avg methods per file
/// - **Memory:** ~1-5 KB per module (signatures only)
/// - **Typical time:** ~100ms for 6,540 files (first call)
/// - **Caching:** Salsa-tracked via SourceRootInput, subsequent calls are O(1)
///
/// ## Salsa caching
/// - LRU: 16 (typically one source root per workspace)
/// - Invalidation: When SourceRoot changes (files added/removed)
/// - First call: builds full index
/// - Subsequent calls: returns cached result
///
/// ## Usage
/// ```ignore
/// // In DefDatabase implementation:
/// fn workspace_symbols(&self, source_root_id: SourceRootId) -> Arc<WorkspaceSymbols> {
///     let source_root_input = self.source_root_input(source_root_id);
///     workspace_symbols_query(self, source_root_input)
/// }
/// ```
#[salsa::tracked(lru = 16)]
pub fn workspace_symbols_query(
    db: &dyn DefDatabase,
    source_root_input: base_db::SourceRootInput,
) -> Arc<WorkspaceSymbols> {
    let source_root = source_root_input.root(db);
    let files: Vec<_> = source_root.iter().collect();
    let _span = tracing::info_span!("workspace_symbols", file_count = files.len()).entered();
    Arc::new(crate::workspace::workspace_symbols(db, &files))
}

/// Get external module references from a file.
///
/// Extracts ExternalRef from module bodies (collected during HIR lowering).
/// These references are used to build the module dependency graph.
///
/// ## Salsa caching
/// - LRU: 512 (frequently accessed for dependency resolution)
/// - Invalidation: Automatic when module bodies change
/// - Dependency: calls module_bodies() internally
///
/// ## Performance
/// - Computation: < 1ms (just extracts data from ModuleBodies)
/// - Cached access: < 1ms
#[salsa::tracked(lru = 512)]
pub fn file_external_refs_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<Vec<ExternalRef>> {
    let _span = tracing::info_span!("file_external_refs", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let module_id = ModuleId::new(file_id);

    tracing::debug!(file_id = file_id.0, "file_external_refs: calling module_bodies");
    let bodies = db.module_bodies(module_id);

    let method_count = bodies.iter_lower_results().count();
    tracing::debug!(file_id = file_id.0, method_count, "file_external_refs: got module_bodies");

    // Collect external refs from all method bodies
    let mut refs = Vec::new();
    for (method_id, lower_result) in bodies.iter_lower_results() {
        let ref_count = lower_result.external_refs.len();
        if ref_count > 0 {
            tracing::debug!(
                file_id = file_id.0,
                method_id = ?method_id,
                ref_count,
                "file_external_refs: found refs in method"
            );
        }
        refs.extend(lower_result.external_refs.iter().cloned());
    }

    // Also collect from module-level code
    if let Some(module_code) = bodies.module_code_result() {
        let ref_count = module_code.external_refs.len();
        if ref_count > 0 {
            tracing::debug!(
                file_id = file_id.0,
                ref_count,
                "file_external_refs: found refs in module code"
            );
        }
        refs.extend(module_code.external_refs.iter().cloned());
    }

    tracing::debug!(file_id = file_id.0, total_refs = refs.len(), "file_external_refs: done");
    Arc::new(refs)
}

/// Build module index from source root.
///
/// Creates a lightweight index mapping module names to FileIds based on
/// file paths (Designer format). No parsing is required.
///
/// ## Salsa caching
/// - LRU: 16 (typically one source root per workspace)
/// - Invalidation: When SourceRoot changes (files added/removed)
///
/// ## Performance
/// - Computation: ~10ms for 6,540 files (path analysis only)
/// - Cached access: < 1ms
#[salsa::tracked(lru = 16)]
pub fn module_index_query(
    _db: &dyn DefDatabase,
    source_root_input: base_db::SourceRootInput,
) -> Arc<ModuleIndex> {
    let source_root = source_root_input.root(_db);
    let _span =
        tracing::info_span!("module_index", file_count = source_root.iter().count()).entered();

    // Build index from file paths using FileSet
    let file_set = source_root.file_set();
    let paths: Vec<(FileId, String)> = source_root
        .iter()
        .filter_map(|file_id| {
            let vfs_path = file_set.path_for_file(&file_id)?;
            let path = vfs_path.as_path();
            let path_str = path.to_str()?;
            Some((file_id, path_str.to_string()))
        })
        .collect();

    let index = ModuleIndex::build_from_paths(paths.iter().map(|(id, p)| (*id, p.as_str())));

    Arc::new(index)
}

/// Get file dependencies for a module.
///
/// Resolves external references to actual FileIds using the module index.
/// Returns the list of files that this module depends on.
///
/// ## Salsa caching
/// - LRU: 512 (frequently accessed for preloading)
/// - Invalidation: Automatic when external refs or module index change
///
/// ## Performance
/// - Computation: < 1ms (lookup in module index)
/// - Cached access: < 1ms
#[salsa::tracked(lru = 512)]
pub fn file_dependencies_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<Vec<FileId>> {
    let _span = tracing::info_span!("file_dependencies", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);

    // Get source root for this file
    let source_root_id = db.file_source_root_input(file_id).source_root_id(db);
    let source_root_input = db.source_root_input(source_root_id);

    // Get module index and external refs
    let index = module_index_query(db, source_root_input);
    tracing::debug!(
        file_id = file_id.0,
        common_modules = index.common_module_count(),
        managers = index.manager_count(),
        "file_dependencies: got module_index"
    );

    let file_id_input = base_db::FileIdInput::new(db, file_id);
    let refs = file_external_refs_query(db, file_id_input);
    tracing::debug!(
        file_id = file_id.0,
        refs_count = refs.len(),
        "file_dependencies: got external_refs"
    );

    // Resolve each ref to a FileId
    let mut deps: Vec<FileId> = refs.iter().filter_map(|r| index.resolve(r)).collect();
    tracing::debug!(
        file_id = file_id.0,
        resolved = deps.len(),
        unresolved = refs.len() - deps.len(),
        "file_dependencies: resolved refs"
    );

    // Remove duplicates
    deps.sort_by_key(|f| f.index());
    deps.dedup();

    Arc::new(deps)
}
