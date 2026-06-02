use std::path::{Path, PathBuf};
use std::sync::Arc;

use base_db::{FileIdInput, Files, RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use hir::{
    ConditionalTree, DefDatabase, ItemTree, ModuleBodies, ModuleData, ModuleId, RegionTree,
    SymbolTree,
};
use vfs::FileId;

use crate::features::FeaturesInput;
use crate::metadata::MetadataDb;
use crate::queries::{
    line_index_query, liveness_analysis_query, method_cfg_query, module_metadata_query,
    reaching_definitions_query,
};
use crate::type_kernel::{TypeKernelHandle, TypeKernelInner, TypeKernelInput};
use crate::{metadata, queries, vfs_helpers, RootDatabase, SdblHirEntries};
use hir::{all_sdbl_in_file_query, sdbl_hir_for_file_query};

#[salsa::db]
pub struct RootDatabaseImpl {
    storage: salsa::Storage<Self>,

    files: Files,

    workspace_configs_input: parking_lot::RwLock<Option<metadata::WorkspaceConfigsInput>>,

    features_input: parking_lot::RwLock<Option<FeaturesInput>>,

    type_kernel: Arc<TypeKernelInner>,

    type_kernel_input: parking_lot::RwLock<Option<TypeKernelInput>>,

    /// Optional process-side cache of loaded configurations, keyed by the interned
    /// config-root path string (stable and consistent across this build's batch
    /// databases — not necessarily filesystem-canonical, which does not matter since
    /// every batch interns the same root the same way). Set only by the batched
    /// graph build, which opens a fresh
    /// database per batch: without it each batch would reload the whole
    /// configuration via [`metadata::load_configuration`] (re-running
    /// `bsl_metadata::load_from_directory` over every metadata file). A single
    /// `Arc` is shared across all of one build's batch databases (and their per-job
    /// clones), so the load happens once. `None` for the long-lived LSP database,
    /// which already memoises `load_configuration` per revision — there the field is
    /// absent and loading is unchanged.
    graph_config_cache: Option<Arc<GraphConfigCache>>,
}

/// Build-scoped cache of loaded configurations by interned config-root path string.
/// A fresh instance per build keeps it a content snapshot — a later build (new
/// instance) never sees a stale entry — so no version key is needed.
pub type GraphConfigCache = dashmap::DashMap<PathBuf, Arc<bsl_metadata::Configuration>>;

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
            workspace_configs_input: parking_lot::RwLock::new(*self.workspace_configs_input.read()),
            features_input: parking_lot::RwLock::new(*self.features_input.read()),
            type_kernel: Arc::clone(&self.type_kernel),
            type_kernel_input: parking_lot::RwLock::new(*self.type_kernel_input.read()),
            // Share the same cache across clones so a per-job db clone sees configs
            // loaded by its siblings.
            graph_config_cache: self.graph_config_cache.clone(),
        }
    }
}

impl RootDatabaseImpl {
    pub fn new() -> Self {
        let type_kernel = Arc::new(TypeKernelInner::new());
        let db = Self {
            storage: salsa::Storage::default(),
            files: Files::new(),
            workspace_configs_input: parking_lot::RwLock::new(None),
            features_input: parking_lot::RwLock::new(None),
            type_kernel: Arc::clone(&type_kernel),
            type_kernel_input: parking_lot::RwLock::new(None),
            graph_config_cache: None,
        };
        let input = metadata::WorkspaceConfigsInput::new(&db, Vec::new(), 0);
        *db.workspace_configs_input.write() = Some(input);
        let defaults = project_model::FeaturesConfig::default();
        let features = FeaturesInput::new(&db, defaults.type_narrowing);
        *db.features_input.write() = Some(features);
        let type_kernel_input = TypeKernelInput::new(&db, TypeKernelHandle::new(type_kernel));
        *db.type_kernel_input.write() = Some(type_kernel_input);
        db
    }

    pub(crate) fn type_kernel_inner(&self) -> &Arc<TypeKernelInner> {
        &self.type_kernel
    }

    /// Attach a build-scoped configuration cache shared across this build's batch
    /// databases. See [`GraphConfigCache`] and the `graph_config_cache` field.
    pub fn set_graph_config_cache(&mut self, cache: Arc<GraphConfigCache>) {
        self.graph_config_cache = Some(cache);
    }

    fn workspace_configs(&self) -> metadata::WorkspaceConfigsInput {
        self.workspace_configs_input
            .read()
            .expect("workspace_configs_input is initialized in RootDatabaseImpl::new")
    }

    pub fn workspace_configs_input(&self) -> metadata::WorkspaceConfigsInput {
        self.workspace_configs()
    }

    pub fn metadata_version(&self) -> u32 {
        self.workspace_configs().version(self)
    }

    pub fn try_file_text(&self, file_id: vfs::FileId) -> Option<base_db::FileTextInput> {
        self.files.try_file_text(file_id)
    }

    pub fn bump_metadata_version(&mut self) {
        use salsa::Setter;
        let input = self.workspace_configs();
        let current = input.version(self);
        input.set_version(self).to(current + 1);
    }

    pub fn set_all_config_paths(&mut self, paths: Vec<(Option<String>, std::path::PathBuf)>) {
        use salsa::Setter;
        let input = self.workspace_configs();
        input.set_paths(self).to(paths);
    }

    pub fn all_config_paths(&self) -> Vec<(Option<String>, std::path::PathBuf)> {
        self.workspace_configs().paths(self)
    }

    fn features(&self) -> FeaturesInput {
        self.features_input.read().expect("features_input is initialized in RootDatabaseImpl::new")
    }

    pub fn type_narrowing_enabled(&self) -> bool {
        self.features().type_narrowing(self)
    }

    pub fn set_type_narrowing_enabled(&mut self, enabled: bool) {
        use salsa::Setter;
        let input = self.features();
        input.set_type_narrowing(self).to(enabled);
    }

    pub(crate) fn get_file_path(&self, file_id: FileId) -> Option<PathBuf> {
        let source_root_input = self.file_source_root_input(file_id);
        let source_root_id = source_root_input.source_root_id(self);
        let source_root_input = self.source_root_input(source_root_id);
        let source_root = source_root_input.root(self);
        let file_set = source_root.file_set();
        let vfs_path = file_set.path_for_file(&file_id)?;
        Some(PathBuf::from(vfs_path.as_path()))
    }

    pub(crate) fn find_configuration_root(&self, file_path: &Path) -> Option<PathBuf> {
        let mut current = file_path.parent()?;

        loop {
            let common_modules = current.join("CommonModules");
            if common_modules.is_dir() {
                tracing::debug!(?current, "Found configuration root via CommonModules/");
                return Some(current.to_path_buf());
            }

            let config_xml = current.join("Configuration.xml");
            if config_xml.is_file() {
                tracing::debug!(?current, "Found configuration root via Configuration.xml");
                return Some(current.to_path_buf());
            }

            current = match current.parent() {
                Some(parent) if parent != current => parent,
                _ => return None,
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

    fn merged_visible_configuration(
        &self,
        file_id: FileId,
    ) -> Option<Arc<bsl_metadata::Configuration>> {
        let file_path = vfs_helpers::get_file_path(self, file_id)?;
        let paths = RootDatabaseImpl::all_config_paths(self);

        let load_at = |path: &std::path::Path| -> Arc<bsl_metadata::Configuration> {
            let path_input = metadata::intern_configuration_path(
                self,
                &path.to_string_lossy(),
                self.metadata_version(),
            );
            self.load_configuration(path_input)
        };

        if paths.is_empty() {
            let config_root = vfs_helpers::find_configuration_root(self, &file_path)?;
            return Some(load_at(&config_root));
        }

        let main_path = paths.iter().find_map(|(name, path)| name.is_none().then_some(path));
        let extension_path = paths
            .iter()
            .filter(|(name, path)| name.is_some() && file_path.starts_with(path))
            .max_by_key(|(_, path)| path.as_os_str().len())
            .map(|(_, path)| path);

        match (main_path, extension_path) {
            (Some(main_path), Some(extension_path)) => {
                let main = load_at(main_path);
                let extension = load_at(extension_path);
                Some(Arc::new(main.merged_with_extension(&extension)))
            }
            (Some(main_path), None) => Some(load_at(main_path)),
            (None, Some(extension_path)) => Some(load_at(extension_path)),
            (None, None) => None,
        }
    }

    fn resolved_module_summary(
        &self,
        module_id: ModuleId,
    ) -> Arc<hir::call_graph::ResolvedModuleSummary> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::resolved_module_summary_query(self, file_id_input)
    }

    fn workspace_call_graph(
        &self,
        source_root_id: base_db::SourceRootId,
    ) -> Arc<hir::call_graph::WorkspaceCallGraph> {
        let source_root_input = self.source_root_input(source_root_id);
        hir::workspace_call_graph_query(self, source_root_input)
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
    ) -> hir::TypeId {
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

    fn module_reaching_definitions(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs> {
        let file_id_input = FileIdInput::new(self, file_id);
        queries::module_reaching_definitions_query(self, file_id_input)
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
        Some(self.load_configuration(path_input))
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
                let config = self.load_configuration(path_input);
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
        sdbl_hir_for_file_query(self, file_id_input)
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
impl metadata::MetadataDb for RootDatabaseImpl {
    /// Override the default loader to consult the build-scoped cache when one is
    /// attached, so the whole-config metadata load runs once per config root per
    /// build instead of once per fresh batch database. This is the single chokepoint
    /// every config read funnels through (the resolver's `find_*`, `module_metadata`,
    /// `configurations`/`merged_visible_configuration`), so caching here covers them
    /// all. With no cache attached (the LSP database) it is the plain salsa query.
    fn load_configuration<'db>(
        &'db self,
        path_input: metadata::ConfigurationPathInput<'db>,
    ) -> Arc<bsl_metadata::Configuration> {
        let Some(cache) = &self.graph_config_cache else {
            return metadata::load_configuration(self, path_input);
        };
        let key = PathBuf::from(path_input.path(self));
        if let Some(config) = cache.get(&key) {
            return Arc::clone(&config);
        }
        // Miss: load (the build warms each root sequentially before its parallel
        // region, so concurrent first-loads of the same root do not occur; a rare
        // duplicate load would only repeat pure work, never corrupt the result).
        let config = metadata::load_configuration(self, path_input);
        cache.insert(key, Arc::clone(&config));
        config
    }
}

#[cfg(test)]
#[path = "database_impl_tests.rs"]
mod database_impl_tests;
