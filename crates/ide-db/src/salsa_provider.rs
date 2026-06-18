use std::sync::Arc;

use base_db::{FileIdInput, SourceRootId};
use bsl_metadata::Configuration;
use hir::{
    AssignmentResolution, DefWithBodyId, HirDatabase, InferenceDiagnostic, InferenceResult,
    ItemTree, ModuleBodies, ModuleId, ModuleIndex, ModuleMetadata, Name, Resolver, SymbolTree,
};
use syntax::{Parse, SyntaxNode};
use vfs::FileId;

use crate::{
    metadata::{intern_configuration_path, load_configuration, ConfigurationPathInput},
    provider::{AnalysisProvider, VisibleConfigWithRoot},
    RootDatabase,
};

/// Routes the seven local per-module queries (`parse` / `file_text` / `item_tree` /
/// `symbol_tree` / `module_bodies` / `infer` / `line_index`) of the extension file to the
/// spliced `&ИзменениеИКонтроль` effective module, so the diagnostics inference pass sees a
/// coherent merged source map. Everything else stays on the base/extension context.
#[derive(Clone, Copy)]
struct EffectiveRoute<'db> {
    eid: hir::EffectiveModuleId<'db>,
    ext_file: FileId,
}

pub struct SalsaProvider<'db> {
    db: &'db dyn RootDatabase,
    configuration_path_input: Option<ConfigurationPathInput<'db>>,
    file_set: Option<&'db vfs::file_set::FileSet>,
    effective: Option<EffectiveRoute<'db>>,
}

impl<'db> SalsaProvider<'db> {
    pub fn new(
        db: &'db dyn RootDatabase,
        configuration_path_input: Option<ConfigurationPathInput<'db>>,
    ) -> Self {
        Self { db, configuration_path_input, file_set: None, effective: None }
    }

    pub fn with_file_set(
        db: &'db dyn RootDatabase,
        configuration_path_input: Option<ConfigurationPathInput<'db>>,
        file_set: Option<&'db vfs::file_set::FileSet>,
    ) -> Self {
        Self { db, configuration_path_input, file_set, effective: None }
    }

    /// Route the extension file's local queries to its effective `&ИзменениеИКонтроль`
    /// module. Used only by the diagnostics inference pass; all other consumers keep the
    /// default (no-op) provider so behaviour is byte-identical for ordinary modules.
    pub fn with_effective(mut self, eid: hir::EffectiveModuleId<'db>, ext_file: FileId) -> Self {
        self.effective = Some(EffectiveRoute { eid, ext_file });
        self
    }

    pub fn db(&self) -> &'db dyn RootDatabase {
        self.db
    }

    /// The effective id when `file_id` is the routed extension file.
    fn effective_for(&self, file_id: FileId) -> Option<hir::EffectiveModuleId<'db>> {
        self.effective.filter(|r| r.ext_file == file_id).map(|r| r.eid)
    }
}

impl AnalysisProvider for SalsaProvider<'_> {
    fn configuration(&self) -> Option<Arc<Configuration>> {
        let path_input = self.configuration_path_input?;
        Some(load_configuration(self.db, path_input))
    }

    fn visible_configurations(&self, _file_id: FileId) -> Vec<VisibleConfigWithRoot> {
        let paths = self.db.all_config_paths();
        if paths.is_empty() {
            return match self.configuration_path_input {
                Some(path_input) => {
                    let root = std::path::PathBuf::from(path_input.path(self.db));
                    vec![VisibleConfigWithRoot {
                        config: bsl_config::VisibleConfig {
                            name: None,
                            configuration: load_configuration(self.db, path_input),
                        },
                        root,
                    }]
                }
                None => Vec::new(),
            };
        }

        paths
            .into_iter()
            .map(|(name, path)| {
                let path_input = intern_configuration_path(
                    self.db,
                    &path.to_string_lossy(),
                    self.db.config_root_revision_for_path(&path),
                );
                let configuration = load_configuration(self.db, path_input);
                VisibleConfigWithRoot {
                    config: bsl_config::VisibleConfig { name, configuration },
                    root: path,
                }
            })
            .collect()
    }

    fn common_module_for_file(&self, file_id: FileId) -> Option<Arc<bsl_metadata::CommonModule>> {
        self.db.common_module_for_file_id(file_id)
    }

    fn resolve_metadata_object(
        &self,
        file_id: FileId,
        mdo_type: bsl_metadata::MdoType,
        name: &str,
    ) -> Option<Arc<bsl_metadata::MetadataObject>> {
        self.db.resolve_metadata_object(file_id, mdo_type, name)
    }

    fn resolve_common_module(
        &self,
        file_id: FileId,
        name: &str,
    ) -> Option<Arc<bsl_metadata::CommonModule>> {
        self.db.resolve_common_module(file_id, name)
    }

    fn resolve_common_module_files(&self, file_id: FileId, name: &str) -> Vec<FileId> {
        self.db.resolve_common_module_files(file_id, name)
    }

    fn workspace_symbols(&self, source_root_id: SourceRootId) -> Arc<hir::WorkspaceSymbols> {
        self.db.workspace_symbols(source_root_id)
    }

    fn module_index(&self, source_root_id: SourceRootId) -> Arc<ModuleIndex> {
        self.db.module_index(source_root_id)
    }

    fn assignment_target_kind(&self, file_id: FileId, name: &str) -> AssignmentResolution {
        let module_id = ModuleId::new(file_id);
        let resolver = Resolver::for_module(module_id);
        resolver.resolve_assignment_target(self.db, &Name::new(name))
    }

    fn kernel_type_display(&self, id: bsl_types::kind::TypeId, locale: base_db::Locale) -> String {
        hir::kernel_type_label(self.db, id, locale, false)
    }

    fn module_implicit_field_names(&self, file_id: FileId) -> Vec<String> {
        hir::module_implicit_field_names(self.db, file_id)
    }

    fn parse(&self, file_id: FileId) -> Parse<SyntaxNode> {
        match self.effective_for(file_id) {
            Some(eid) => hir::parse_effective(self.db, eid),
            None => self.db.parse(file_id),
        }
    }

    fn file_text(&self, file_id: FileId) -> String {
        match self.effective_for(file_id).and_then(|eid| hir::effective_module_text(self.db, eid)) {
            Some(em) => em.text.to_string(),
            None => self.db.file_text(file_id).to_string(),
        }
    }

    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree> {
        match self.effective_for(file_id) {
            Some(eid) => hir::item_tree_effective(self.db, eid),
            None => self.db.item_tree(file_id),
        }
    }

    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree> {
        match self.effective_for(module_id.file_id) {
            Some(eid) => hir::symbol_tree_effective(self.db, eid),
            None => self.db.symbol_tree(module_id),
        }
    }

    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies> {
        match self.effective_for(module_id.file_id) {
            Some(eid) => hir::module_bodies_effective(self.db, eid),
            None => self.db.module_bodies(module_id),
        }
    }

    fn infer(&self, file_id: FileId) -> Arc<InferenceResult> {
        match self.effective_for(file_id) {
            Some(eid) => hir::infer_effective(self.db, eid),
            None => HirDatabase::infer(self.db, file_id),
        }
    }

    fn arg_diagnostics(&self, file_id: FileId) -> Arc<Vec<(DefWithBodyId, InferenceDiagnostic)>> {
        HirDatabase::arg_diagnostics(self.db, file_id)
    }

    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata> {
        self.db.module_metadata(module_id)
    }

    fn call_summary(&self, module_id: ModuleId) -> Arc<hir::ModuleCallSummary> {
        self.db.module_call_summary(module_id)
    }

    fn line_index(&self, file_id: FileId) -> Arc<line_index::LineIndex> {
        if let Some(em) =
            self.effective_for(file_id).and_then(|eid| hir::effective_module_text(self.db, eid))
        {
            return Arc::new(line_index::LineIndex::new(&em.text));
        }
        let input = FileIdInput::new(self.db, file_id);
        self.db.line_index(input)
    }

    fn file_source_root_id(&self, file_id: FileId) -> SourceRootId {
        self.db.file_source_root_input(file_id).source_root_id(self.db)
    }

    fn module_cfgs(&self, file_id: FileId) -> Arc<hir::cfg::ModuleCfgs> {
        let input = FileIdInput::new(self.db, file_id);
        self.db.module_cfgs(input)
    }

    fn module_path_terminates(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::path_terminates::ModulePathTerminates> {
        let input = FileIdInput::new(self.db, file_id);
        self.db.module_path_terminates(input)
    }

    fn module_liveness_analysis(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::liveness::ModuleLiveness> {
        let input = FileIdInput::new(self.db, file_id);
        self.db.module_liveness_analysis(input)
    }

    fn module_reaching_definitions(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs> {
        let input = FileIdInput::new(self.db, file_id);
        <dyn RootDatabase as RootDatabase>::module_reaching_definitions(self.db, input)
    }

    fn region_tree(&self, file_id: FileId) -> Arc<hir::RegionTree> {
        self.db.region_tree(file_id)
    }

    fn sdbl_hir_in_file(&self, file_id: FileId) -> crate::SdblHirEntries {
        self.db.sdbl_hir_in_file(file_id)
    }

    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>> {
        self.db.all_sdbl_in_file(file_id)
    }

    fn module_data(&self, module_id: ModuleId) -> Arc<hir::ModuleData> {
        self.db.module_data(module_id)
    }

    fn method_docs(&self, method_id: hir::MethodId) -> Option<Arc<hir::MethodDocs>> {
        self.db.method_docs(method_id)
    }

    fn variable_docs(&self, variable_id: hir::VariableId) -> Option<Arc<hir::VariableDocs>> {
        self.db.variable_docs(variable_id)
    }

    fn reaching_definitions(
        &self,
        method_id: hir::MethodId,
    ) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>> {
        self.db.reaching_definitions(method_id)
    }

    fn file_external_refs(&self, module_id: ModuleId) -> std::sync::Arc<Vec<hir::ExternalRef>> {
        self.db.file_external_refs(module_id)
    }

    fn module_level_liveness_analysis(
        &self,
        module_id: ModuleId,
    ) -> Option<std::sync::Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>>
    {
        self.db.module_level_liveness_analysis(module_id)
    }

    fn resolve_vfs_path(
        &self,
        source_root_id: base_db::SourceRootId,
        vfs_path: &vfs::VfsPath,
    ) -> Option<FileId> {
        self.db.resolve_vfs_path(source_root_id, vfs_path)
    }

    fn resolve_module_file(&self, relative_uri: &str) -> Option<FileId> {
        let config_path_input = self.configuration_path_input?;
        let config_root = config_path_input.path(self.db);
        let full_path = std::path::PathBuf::from(&config_root).join(relative_uri);
        let vfs_path = vfs::VfsPath::new(full_path.to_string_lossy().into_owned());

        if let Some(file_set) = self.file_set {
            file_set.file_for_path(&vfs_path).copied()
        } else {
            self.db.resolve_vfs_path(SourceRootId(0), &vfs_path)
        }
    }

    fn file_path(&self, file_id: FileId) -> Option<String> {
        if let Some(file_set) = self.file_set {
            let vfs_path = file_set.path_for_file(&file_id)?;
            return Some(vfs_path.as_path().to_string_lossy().to_string());
        }
        crate::vfs_helpers::get_file_path(self.db, file_id).map(|p| p.to_string_lossy().to_string())
    }

    fn method_effect_summary(
        &self,
        method: hir::MethodId,
    ) -> Arc<hir::dataflow::effect_summary::EffectSummary> {
        let method_input = hir::MethodIdInput::new(self.db, method);
        crate::effects::method_effect_summary_query(self.db, method_input)
    }

    fn module_security_state(&self, file_id: FileId) -> Arc<crate::effects::ModuleSecurityState> {
        let input = FileIdInput::new(self.db, file_id);
        crate::effects::module_security_state_query(self.db, input)
    }

    fn method_hir_metrics(&self, method: hir::MethodId) -> Arc<hir::metrics::HirMethodMetrics> {
        let input = hir::MethodIdInput::new(self.db, method);
        crate::queries::method_hir_metrics_query(self.db, input)
    }

    fn module_hir_metrics(&self, file_id: FileId) -> Arc<crate::queries::ModuleHirMetrics> {
        let input = FileIdInput::new(self.db, file_id);
        crate::queries::module_hir_metrics_query(self.db, input)
    }

    fn method_cyclomatic(&self, method: hir::MethodId) -> u32 {
        let input = hir::MethodIdInput::new(self.db, method);
        crate::queries::method_cyclomatic_query(self.db, input)
    }

    fn module_cyclomatic(&self, file_id: FileId) -> Arc<crate::queries::ModuleCyclomatic> {
        let input = FileIdInput::new(self.db, file_id);
        crate::queries::module_cyclomatic_query(self.db, input)
    }
}
