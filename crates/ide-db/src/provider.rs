//! Analysis data provider abstraction.
//!
//! This module defines the `AnalysisProvider` trait which abstracts over
//! the source of analysis data, enabling two implementations:
//! - [`SalsaProvider`](crate::SalsaProvider): Full caching via RootDatabase (LSP mode)
//! - `StreamingProvider`: On-the-fly computation (analyze mode, future)

use std::sync::Arc;

use base_db::SourceRootId;
use bsl_metadata::Configuration;
use hir::{
    ItemTree, MethodDocs, MethodId, ModuleBodies, ModuleId, ModuleIndex, ModuleMetadata, SymbolTree,
};
use syntax::{Parse, SyntaxNode};
use vfs::{FileId, VfsPath};

use crate::SdblHirEntries;

/// Abstraction over analysis data sources.
///
/// Two implementations:
/// - `SalsaProvider`: Uses RootDatabase with full caching (LSP mode)
/// - `StreamingProvider`: Computes on-the-fly, releases after use (analyze mode)
///
/// # Global vs Per-file Data
///
/// Methods are organized into categories:
/// - **Global**: Shared across all files, kept in memory for the entire analysis
/// - **Per-file**: Can be computed on-demand and released after use
/// - **Dataflow**: Complex analyses built on top of CFG
pub trait AnalysisProvider {
    // ========================================================================
    // Global Data (shared across all files)
    // ========================================================================

    /// Get 1C Configuration metadata.
    ///
    /// Contains CommonModules, MetadataObjects, Registers, etc.
    /// Loaded once and reused for all files.
    fn configuration(&self) -> Option<Arc<Configuration>>;

    /// Get workspace symbols index for cross-module resolution.
    ///
    /// Maps CommonModule names to their exported methods.
    /// Used for qualified name resolution: `CommonModule.Method()`.
    fn workspace_symbols(&self, source_root_id: SourceRootId) -> Arc<hir::WorkspaceSymbols>;

    /// Get module index (name -> FileId mapping).
    fn module_index(&self, source_root_id: SourceRootId) -> Arc<ModuleIndex>;

    // ========================================================================
    // Per-file Data
    // ========================================================================

    /// Parse file to AST.
    fn parse(&self, file_id: FileId) -> Parse<SyntaxNode>;

    /// Get file text.
    fn file_text(&self, file_id: FileId) -> String;

    /// Get ItemTree (method signatures).
    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree>;

    /// Get SymbolTree for a module (case-insensitive lookup).
    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree>;

    /// Get lowered HIR bodies for all methods in module.
    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies>;

    /// Get module metadata (type, execution context).
    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata>;

    /// Get line index for byte offset -> line/column conversion.
    fn line_index(&self, file_id: FileId) -> Arc<line_index::LineIndex>;

    /// Get file path as string (for metadata lookups).
    fn file_path(&self, file_id: FileId) -> Option<String>;

    /// Get source root ID for a file.
    fn file_source_root_id(&self, file_id: FileId) -> SourceRootId;

    // ========================================================================
    // Regions (for code style diagnostics)
    // ========================================================================

    /// Get region tree for file (module structure with regions).
    fn region_tree(&self, file_id: FileId) -> Arc<hir::RegionTree>;

    /// Get module-level regions (top-level regions in file).
    fn module_level_regions(&self, file_id: FileId) -> Arc<Vec<base_db::RegionInfo>>;

    // ========================================================================
    // SDBL Queries (for query diagnostics)
    // ========================================================================

    /// Get SDBL HIR for all queries in file (lowered + type-inferred).
    ///
    /// Returns Vec<(ExprId in BSL HIR, lowered SDBL HIR)>.
    fn sdbl_hir_in_file(&self, file_id: FileId) -> SdblHirEntries;

    /// Get all SDBL queries (parsed AST) in file.
    ///
    /// Returns Vec<(SdblExprId, SDBL query info with source position)>.
    /// SdblExprId uniquely identifies SDBL expression across all bodies in file.
    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>>;

    // ========================================================================
    // Module Information
    // ========================================================================

    /// Get module data (name, type, etc.) for a module.
    fn module_data(&self, module_id: ModuleId) -> Arc<hir::ModuleData>;

    /// Get parsed documentation for a method.
    ///
    /// Extracts and parses leading comments (lines starting with //)
    /// before a procedure or function definition.
    fn method_docs(&self, method_id: MethodId) -> Option<Arc<MethodDocs>>;

    // ========================================================================
    // Dataflow Analysis (for complex diagnostics)
    // ========================================================================

    /// Get CFGs for all methods in module (batch).
    fn module_cfgs(&self, file_id: FileId) -> Arc<hir::cfg::ModuleCfgs>;

    /// Get liveness analysis for all methods (batch).
    fn module_liveness_analysis(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::liveness::ModuleLiveness>;

    /// Get reaching definitions for all methods (batch).
    fn module_reaching_definitions(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs>;

    // ========================================================================
    // Per-Method Dataflow (for specific diagnostics)
    // ========================================================================

    /// Get reaching definitions for a specific method.
    ///
    /// Returns `None` if analysis doesn't converge (malformed CFG, infinite loop).
    fn reaching_definitions(
        &self,
        method_id: MethodId,
    ) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>>;

    // ========================================================================
    // Cross-module References
    // ========================================================================

    /// Get external references (qualified calls) from a module.
    ///
    /// Returns list of cross-module references like `CommonModule.Method()`.
    fn file_external_refs(&self, module_id: ModuleId) -> Arc<Vec<hir::ExternalRef>>;

    /// Get liveness analysis for module-level code.
    ///
    /// Returns `None` if no module-level code or analysis doesn't converge.
    fn module_level_liveness_analysis(
        &self,
        module_id: ModuleId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>>;

    // ========================================================================
    // VFS Resolution (for metadata lookups)
    // ========================================================================

    /// Resolve VfsPath to FileId within a SourceRoot.
    ///
    /// Used for finding metadata files (CommonModules, EventSubscriptions, etc.)
    /// given their URI from Configuration.
    fn resolve_vfs_path(&self, source_root_id: SourceRootId, vfs_path: &VfsPath) -> Option<FileId>;

    /// Resolve a relative module URI to FileId.
    ///
    /// Builds absolute path from workspace root + relative URI, then resolves
    /// through file_set (fast path) or VFS. Used for cross-module diagnostics
    /// that need to find CommonModule/ManagerModule files.
    ///
    /// Returns `None` if workspace root is not available or file not found.
    fn resolve_module_file(&self, relative_uri: &str) -> Option<FileId>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{salsa_provider::SalsaProvider, RootDatabaseImpl};
    use base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use vfs::{file_set::FileSet, VfsPath};

    fn setup_db() -> RootDatabaseImpl {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

        db
    }

    #[test]
    fn test_salsa_provider_parse() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let parse = provider.parse(FileId(0));
        assert!(!parse.has_errors());
    }

    #[test]
    fn test_salsa_provider_file_text() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let text = provider.file_text(FileId(0));
        assert_eq!(text, "Процедура Тест() КонецПроцедуры");
    }

    #[test]
    fn test_salsa_provider_item_tree() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let item_tree = provider.item_tree(FileId(0));
        assert_eq!(item_tree.top_level_items().len(), 1);
    }

    #[test]
    fn test_salsa_provider_module_bodies() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let module_id = ModuleId::new(FileId(0));
        let bodies = provider.module_bodies(module_id);

        // Should have one method body
        assert_eq!(bodies.iter_bodies().count(), 1);
    }

    #[test]
    fn test_salsa_provider_symbol_tree() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let module_id = ModuleId::new(FileId(0));
        let symbols = provider.symbol_tree(module_id);

        // Should find the "Тест" procedure
        assert!(symbols.find_method(&hir::Name::new("Тест")).is_some());
    }

    #[test]
    fn test_salsa_provider_line_index() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let line_index = provider.line_index(FileId(0));
        // File has one line
        assert_eq!(line_index.line_col(0.into()).line, 0);
    }

    #[test]
    fn test_salsa_provider_source_root_id() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let source_root_id = provider.file_source_root_id(FileId(0));
        assert_eq!(source_root_id, SourceRootId(0));
    }

    #[test]
    fn test_salsa_provider_configuration_none() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        // No configuration path provided
        assert!(provider.configuration().is_none());
    }
}
