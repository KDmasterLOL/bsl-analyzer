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

use crate::{
    ty::infer::InferenceResult, DefDatabase, ModuleBodies, ModuleData, ModuleId, ModuleMetadata,
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
