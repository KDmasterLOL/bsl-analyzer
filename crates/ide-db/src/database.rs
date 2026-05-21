//! RootDatabaseImpl — Salsa-backed implementation of RootDatabase.
//!
//! This module contains the concrete database struct and all trait implementations
//! that wire Salsa queries to the trait methods defined in `root_db.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base_db::{FileIdInput, Files, RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use hir::{
    ConditionalTree, DefDatabase, ItemTree, ModuleBodies, ModuleData, ModuleId, RegionTree,
    SymbolTree,
};
use vfs::FileId;

use crate::features::FeaturesInput;
use crate::queries::{
    all_sdbl_in_file_query, line_index_query, liveness_analysis_query, method_cfg_query,
    module_metadata_query, reaching_definitions_query, sdbl_hir_in_file_query,
};
use crate::types::SdblHirEntries;
use crate::{metadata, queries, vfs_helpers, RootDatabase};

/// Default implementation of RootDatabase with Salsa integration.
///
/// All HIR queries are now managed by Salsa for automatic caching and invalidation!
/// No manual DashMap caches needed for HIR - Salsa handles everything.
#[salsa::db]
pub struct RootDatabaseImpl {
    /// Salsa storage for incremental computation
    storage: salsa::Storage<Self>,

    /// Base file storage
    files: Files,

    /// Salsa input carrying the workspace-wide set of configuration roots and
    /// the metadata generation counter.
    ///
    /// Lazily initialized on the first call to `set_all_config_paths` /
    /// `bump_metadata_version`. Queries read `paths` / `version` from this
    /// input, so mutations propagate through Salsa's invalidation graph.
    workspace_configs_input: parking_lot::RwLock<Option<metadata::WorkspaceConfigsInput>>,

    /// Salsa input carrying workspace-wide feature flags (Task 6.7).
    ///
    /// Eagerly created in [`RootDatabaseImpl::new`] with defaults matching
    /// [`project_model::FeaturesConfig::default`] (every flag on). Today's
    /// only consumer — `narrow_or_base` in `hir` — is a plain Rust helper,
    /// so flipping a flag through [`RootDatabaseImpl::set_type_narrowing_enabled`]
    /// takes effect on the next call. If a future `#[salsa::tracked]`
    /// query reads from this input, Salsa's standard revision tracking
    /// kicks in for that query.
    features_input: parking_lot::RwLock<Option<FeaturesInput>>,
}

impl Default for RootDatabaseImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for RootDatabaseImpl {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            files: self.files.clone(),
            // WorkspaceConfigsInput is Copy and tied to the shared storage,
            // so cloning the handle is safe.
            workspace_configs_input: parking_lot::RwLock::new(*self.workspace_configs_input.read()),
            features_input: parking_lot::RwLock::new(*self.features_input.read()),
        }
    }
}

impl RootDatabaseImpl {
    /// Create a new empty database.
    ///
    /// Eagerly creates the singleton [`metadata::WorkspaceConfigsInput`] so
    /// that tracked queries that read configuration metadata always observe
    /// a Salsa input and register a proper invalidation dependency — even
    /// before the LSP layer has published any configuration paths.
    pub fn new() -> Self {
        let db = Self {
            storage: salsa::Storage::default(),
            files: Files::new(),
            workspace_configs_input: parking_lot::RwLock::new(None),
            features_input: parking_lot::RwLock::new(None),
        };
        let input = metadata::WorkspaceConfigsInput::new(&db, Vec::new(), 0);
        *db.workspace_configs_input.write() = Some(input);
        // Defaults come from `project_model::FeaturesConfig::default` so
        // that a fresh database and a freshly-parsed project config agree
        // on the initial flag values — no risk of silent divergence if a
        // future default flips.
        let defaults = project_model::FeaturesConfig::default();
        let features = FeaturesInput::new(&db, defaults.type_narrowing);
        *db.features_input.write() = Some(features);
        db
    }

    /// Handle of the singleton workspace-configs Salsa input.
    ///
    /// Invariant: always `Some` after `new()` returns.
    fn workspace_configs(&self) -> metadata::WorkspaceConfigsInput {
        self.workspace_configs_input
            .read()
            .expect("workspace_configs_input is initialized in RootDatabaseImpl::new")
    }

    /// Expose the singleton handle (for consumers that need to read via
    /// Salsa getters).
    pub fn workspace_configs_input(&self) -> metadata::WorkspaceConfigsInput {
        self.workspace_configs()
    }

    /// Get the current metadata version (for configuration cache invalidation).
    pub fn metadata_version(&self) -> u32 {
        self.workspace_configs().version(self)
    }

    /// Non-panicking probe of the per-file `FileTextInput` cell.
    ///
    /// Returns `Some(FileTextInput)` if the cell exists, `None` if the `FileId`
    /// has never had a text input set. Safe to call **outside** tracked
    /// queries; inside a tracked query the regular `file_text_input(fid)` path
    /// is the right choice (panics on missing, but our total-VFS invariant
    /// guarantees presence for fids reachable through SourceRoot).
    pub fn try_file_text(&self, file_id: vfs::FileId) -> Option<base_db::FileTextInput> {
        self.files.try_file_text(file_id)
    }

    /// Bump metadata version to invalidate configuration cache.
    /// Call this when .xml metadata files change.
    pub fn bump_metadata_version(&mut self) {
        use salsa::Setter;
        let input = self.workspace_configs();
        let current = input.version(self);
        input.set_version(self).to(current + 1);
    }

    /// Set all configuration paths: main + extensions.
    /// Called from LSP when workspace root is set.
    pub fn set_all_config_paths(&mut self, paths: Vec<(Option<String>, std::path::PathBuf)>) {
        use salsa::Setter;
        let input = self.workspace_configs();
        input.set_paths(self).to(paths);
    }

    /// Get all registered configuration paths.
    pub fn all_config_paths(&self) -> Vec<(Option<String>, std::path::PathBuf)> {
        self.workspace_configs().paths(self)
    }

    /// Handle of the singleton features Salsa input.
    ///
    /// Invariant: always `Some` after `new()` returns.
    fn features(&self) -> FeaturesInput {
        self.features_input.read().expect("features_input is initialized in RootDatabaseImpl::new")
    }

    /// Whether the type narrowing overlay is enabled for this workspace.
    ///
    /// Proxy for the underlying Salsa reader, exposed on the impl so the
    /// LSP layer can read the flag without importing the trait.
    pub fn type_narrowing_enabled(&self) -> bool {
        self.features().type_narrowing(self)
    }

    /// Toggle the type-narrowing overlay feature flag.
    ///
    /// Called by the LSP layer after loading `bsl-analyzer.toml`. The
    /// next `narrow_or_base` call (or any future Salsa-tracked query that
    /// reads the same input) observes the new value. Taking a plain
    /// `bool` keeps this helper callable from contexts that don't want
    /// to construct a full `FeaturesConfig`.
    pub fn set_type_narrowing_enabled(&mut self, enabled: bool) {
        use salsa::Setter;
        let input = self.features();
        input.set_type_narrowing(self).to(enabled);
    }

    /// Get file path from FileId by traversing SourceRoot.
    ///
    /// Returns None if path cannot be resolved.
    pub(crate) fn get_file_path(&self, file_id: FileId) -> Option<PathBuf> {
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
    pub(crate) fn find_configuration_root(&self, file_path: &Path) -> Option<PathBuf> {
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
}

#[salsa::db]
impl DefDatabase for RootDatabaseImpl {
    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree> {
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        hir::item_tree_query(self, file_id_input)
    }

    fn region_tree(&self, file_id: FileId) -> Arc<RegionTree> {
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        hir::region_tree_query(self, file_id_input)
    }

    fn conditional_tree(&self, file_id: FileId) -> Arc<ConditionalTree> {
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        hir::conditional_tree_query(self, file_id_input)
    }

    fn module_data(&self, module_id: ModuleId) -> Arc<ModuleData> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::module_data_query(self, file_id_input)
    }

    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::symbol_tree_query(self, file_id_input)
    }

    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::module_bodies_query(self, file_id_input)
    }

    fn method_body(&self, method: hir::MethodIdInput<'_>) -> Arc<hir::Body> {
        hir::method_body_query(self, method)
    }

    fn method_body_with_source_map(
        &self,
        method: hir::MethodIdInput<'_>,
    ) -> Arc<(hir::Body, hir::BodySourceMap)> {
        hir::method_body_with_source_map_query(self, method)
    }

    fn module_metadata(&self, module_id: ModuleId) -> Arc<hir::ModuleMetadata> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        module_metadata_query(self, file_id_input)
    }

    fn module_call_summary(&self, module_id: ModuleId) -> Arc<hir::ModuleCallSummary> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::module_call_summary_query(self, file_id_input)
    }

    fn method_docs(&self, method: hir::MethodId) -> Option<Arc<hir::MethodDocs>> {
        let symbol_tree = self.symbol_tree(method.module);
        let method_symbol = symbol_tree.find_method_by_id(method)?;
        method_symbol.docs.clone()
    }

    fn variable_docs(&self, variable: hir::VariableId) -> Option<Arc<hir::VariableDocs>> {
        let symbol_tree = self.symbol_tree(variable.module);
        let variable_symbol = symbol_tree.find_variable_by_id(variable)?;
        variable_symbol.docs.clone()
    }

    fn workspace_symbols(
        &self,
        source_root_id: base_db::SourceRootId,
    ) -> Arc<hir::WorkspaceSymbols> {
        let source_root_input = self.source_root_input(source_root_id);
        hir::workspace_symbols_query(self, source_root_input)
    }

    fn workspace_index(&self, source_root_id: base_db::SourceRootId) -> Arc<hir::WorkspaceIndex> {
        let source_root_input = self.source_root_input(source_root_id);
        hir::workspace_index_query(self, source_root_input)
    }

    fn name_usage_index(
        &self,
        source_root_id: base_db::SourceRootId,
    ) -> Arc<hir::SourceRootNameUsage> {
        let source_root_input = self.source_root_input(source_root_id);
        hir::source_root_name_usage_query(self, source_root_input)
    }

    fn file_external_refs(&self, module_id: ModuleId) -> Arc<Vec<hir::ExternalRef>> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::file_external_refs_query(self, file_id_input)
    }

    fn module_index(&self, source_root_id: base_db::SourceRootId) -> Arc<hir::ModuleIndex> {
        let source_root_input = self.source_root_input(source_root_id);
        hir::module_index_query(self, source_root_input)
    }

    fn file_dependencies(&self, module_id: ModuleId) -> Arc<Vec<FileId>> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::file_dependencies_query(self, file_id_input)
    }
}

#[salsa::db]
impl hir::ConfigsDatabase for RootDatabaseImpl {
    fn configurations(&self, file_id: FileId) -> Vec<hir::VisibleConfig> {
        RootDatabase::get_all_configurations(self, file_id)
            .into_iter()
            .map(|(name, configuration)| hir::VisibleConfig { name, configuration })
            .collect()
    }
}

#[salsa::db]
impl hir::HirDatabase for RootDatabaseImpl {
    fn infer(&self, file_id: FileId) -> Arc<hir::InferenceResult> {
        let file_id_input = FileIdInput::new(self, file_id);
        hir::infer_query(self, file_id_input)
    }

    fn type_of_expr(
        &self,
        file_id: FileId,
        owner: hir::DefWithBodyId,
        expr: hir::ExprId,
    ) -> hir::Ty {
        hir::type_of_expr_query(self, file_id, owner, expr)
    }

    fn narrow(
        &self,
        file_id: FileId,
        owner: hir::DefWithBodyId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::NarrowState>>> {
        hir::narrow_query(self, file_id, owner)
    }

    fn arg_diagnostics(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir::DefWithBodyId, hir::InferenceDiagnostic)>> {
        hir::arg_diagnostics_query(self, file_id)
    }

    fn type_narrowing_enabled(&self) -> bool {
        RootDatabaseImpl::type_narrowing_enabled(self)
    }

    fn proc_signature(
        &self,
        method_input: hir::MethodIdInput<'_>,
    ) -> Arc<hir::proc_signature::ProcSignature> {
        hir::proc_signature::proc_signature_query(self, method_input)
    }

    fn infer_method(&self, method: hir::MethodIdInput<'_>) -> Arc<hir::BodyInferenceResult> {
        hir::infer_method_query(self, method)
    }

    fn infer_module_code(&self, file_id: FileId) -> Arc<hir::ModuleCodeInferenceResult> {
        let file_id_input = FileIdInput::new(self, file_id);
        hir::infer_module_code_query(self, file_id_input)
    }
}

#[salsa::db]
impl RootDatabase for RootDatabaseImpl {
    fn get_configuration(&self, file_id: FileId) -> Option<Arc<bsl_metadata::Configuration>> {
        let file_path = vfs_helpers::get_file_path(self, file_id)?;
        let config_root = vfs_helpers::find_configuration_root(self, &file_path)?;
        let path_input = metadata::intern_configuration_path(
            self,
            &config_root.to_string_lossy(),
            self.metadata_version(),
        );
        Some(metadata::load_configuration(self, path_input))
    }

    fn get_all_configurations(
        &self,
        file_id: FileId,
    ) -> Vec<(Option<String>, Arc<bsl_metadata::Configuration>)> {
        let version = self.metadata_version();
        let all_paths = RootDatabaseImpl::all_config_paths(self);

        if all_paths.is_empty() {
            return self.get_configuration(file_id).into_iter().map(|c| (None, c)).collect();
        }

        all_paths
            .into_iter()
            .map(|(name, path)| {
                let path_input =
                    metadata::intern_configuration_path(self, &path.to_string_lossy(), version);
                let config = metadata::load_configuration(self, path_input);
                (name, config)
            })
            .collect()
    }

    fn all_config_paths(&self) -> Vec<(Option<String>, std::path::PathBuf)> {
        RootDatabaseImpl::all_config_paths(self)
    }

    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>> {
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        all_sdbl_in_file_query(self, file_id_input)
    }

    fn sdbl_hir_in_file(&self, file_id: FileId) -> SdblHirEntries {
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        sdbl_hir_in_file_query(self, file_id_input)
    }

    fn module_cfgs(&self, file_id_input: FileIdInput) -> Arc<hir::cfg::ModuleCfgs> {
        queries::module_cfgs_query(self, file_id_input)
    }

    fn module_reaching_definitions(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs> {
        queries::module_reaching_definitions_query(self, file_id_input)
    }

    fn module_path_terminates(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<hir::dataflow::path_terminates::ModulePathTerminates> {
        queries::module_path_terminates_query(self, file_id_input)
    }

    fn module_liveness_analysis(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<hir::dataflow::liveness::ModuleLiveness> {
        queries::module_liveness_analysis_query(self, file_id_input)
    }

    fn reaching_definitions(
        &self,
        method_id: hir::MethodId,
    ) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>> {
        let method_id_input = hir::MethodIdInput::new(self, method_id);
        reaching_definitions_query(self, method_id_input)
    }

    fn liveness_analysis(
        &self,
        method_id: hir::MethodId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>> {
        let method_id_input = hir::MethodIdInput::new(self, method_id);
        liveness_analysis_query(self, method_id_input)
    }

    fn method_cfg(&self, method_id: hir::MethodId) -> Arc<hir::cfg::ControlFlowGraph> {
        let method_id_input = hir::MethodIdInput::new(self, method_id);
        method_cfg_query(self, method_id_input)
    }

    fn module_level_cfg(&self, module_id: hir::ModuleId) -> Arc<hir::cfg::ControlFlowGraph> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        queries::module_level_cfg_query(self, file_id_input)
    }

    fn module_level_liveness_analysis(
        &self,
        module_id: hir::ModuleId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        queries::module_level_liveness_analysis_query(self, file_id_input)
    }

    fn line_index(&self, file_id_input: base_db::FileIdInput) -> Arc<line_index::LineIndex> {
        line_index_query(self, file_id_input)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn metadata_version(&self) -> u32 {
        RootDatabaseImpl::metadata_version(self)
    }
}

#[salsa::db]
impl metadata::MetadataDb for RootDatabaseImpl {}

#[cfg(test)]
#[path = "database_impl_tests.rs"]
mod database_impl_tests;
