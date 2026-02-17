//! Salsa-backed analysis provider.
//!
//! Wraps RootDatabase to implement AnalysisProvider trait.
//! All methods delegate to Salsa queries with full caching.

use std::sync::Arc;

use base_db::{FileIdInput, SourceRootId};
use bsl_metadata::Configuration;
use hir_def::{ItemTree, ModuleBodies, ModuleId, ModuleIndex, ModuleMetadata, SymbolTree};
use syntax::{Parse, SyntaxNode};
use vfs::FileId;

use crate::{
    metadata::{load_configuration, ConfigurationPathInput},
    provider::AnalysisProvider,
    RootDatabase,
};

/// Provider backed by Salsa RootDatabase.
///
/// All methods delegate to Salsa queries with full caching.
/// Used in LSP mode for maximum performance during editing.
pub struct SalsaProvider<'db> {
    db: &'db dyn RootDatabase,
    configuration_path_input: Option<ConfigurationPathInput<'db>>,
}

impl<'db> SalsaProvider<'db> {
    /// Create a new SalsaProvider.
    pub fn new(
        db: &'db dyn RootDatabase,
        configuration_path_input: Option<ConfigurationPathInput<'db>>,
    ) -> Self {
        Self { db, configuration_path_input }
    }

    /// Get the underlying database.
    pub fn db(&self) -> &'db dyn RootDatabase {
        self.db
    }
}

impl AnalysisProvider for SalsaProvider<'_> {
    fn configuration(&self) -> Option<Arc<Configuration>> {
        let path_input = self.configuration_path_input?;
        Some(load_configuration(self.db, path_input))
    }

    fn workspace_symbols(&self, source_root_id: SourceRootId) -> Arc<hir_def::WorkspaceSymbols> {
        self.db.workspace_symbols(source_root_id)
    }

    fn module_index(&self, source_root_id: SourceRootId) -> Arc<ModuleIndex> {
        self.db.module_index(source_root_id)
    }

    fn parse(&self, file_id: FileId) -> Parse<SyntaxNode> {
        self.db.parse(file_id)
    }

    fn file_text(&self, file_id: FileId) -> String {
        let input = self.db.file_text_input(file_id);
        input.text(self.db).clone()
    }

    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree> {
        self.db.item_tree(file_id)
    }

    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree> {
        self.db.symbol_tree(module_id)
    }

    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies> {
        self.db.module_bodies(module_id)
    }

    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata> {
        self.db.module_metadata(module_id)
    }

    fn line_index(&self, file_id: FileId) -> Arc<line_index::LineIndex> {
        let input = FileIdInput::new(self.db, file_id);
        self.db.line_index(input)
    }

    fn file_path(&self, file_id: FileId) -> Option<String> {
        crate::vfs_helpers::get_file_path_for_metadata(self.db, file_id).map(|p| p.to_string_lossy().to_string())
    }

    fn file_source_root_id(&self, file_id: FileId) -> SourceRootId {
        self.db.file_source_root_input(file_id).source_root_id(self.db)
    }

    fn module_cfgs(&self, file_id: FileId) -> Arc<cfg::ModuleCfgs> {
        let input = FileIdInput::new(self.db, file_id);
        self.db.module_cfgs(input)
    }

    fn module_liveness_analysis(&self, file_id: FileId) -> Arc<dataflow::liveness::ModuleLiveness> {
        let input = FileIdInput::new(self.db, file_id);
        self.db.module_liveness_analysis(input)
    }

    fn module_reaching_definitions(
        &self,
        file_id: FileId,
    ) -> Arc<dataflow::reaching_defs::ModuleReachingDefs> {
        let input = FileIdInput::new(self.db, file_id);
        self.db.module_reaching_definitions(input)
    }

    fn region_tree(&self, file_id: FileId) -> Arc<hir_def::RegionTree> {
        self.db.region_tree(file_id)
    }

    fn module_level_regions(&self, file_id: FileId) -> Arc<Vec<base_db::RegionInfo>> {
        self.db.module_level_regions(file_id)
    }

    fn sdbl_hir_in_file(&self, file_id: FileId) -> crate::SdblHirEntries {
        self.db.sdbl_hir_in_file(file_id)
    }

    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir_def::SdblExprId, syntax::SdblQueryInfo)>> {
        self.db.all_sdbl_in_file(file_id)
    }

    fn module_data(&self, module_id: ModuleId) -> Arc<hir_def::ModuleData> {
        self.db.module_data(module_id)
    }

    fn method_docs(&self, method_id: hir_def::MethodId) -> Option<Arc<hir_def::docs::MethodDocs>> {
        self.db.method_docs(method_id)
    }

    fn reaching_definitions(
        &self,
        method_id: hir_def::MethodId,
    ) -> Option<Arc<dataflow::reaching_defs::ReachingDefsResult>> {
        self.db.reaching_definitions(method_id)
    }

    fn resolve_vfs_path(
        &self,
        source_root_id: base_db::SourceRootId,
        vfs_path: &vfs::VfsPath,
    ) -> Option<FileId> {
        self.db.resolve_vfs_path(source_root_id, vfs_path)
    }
}
