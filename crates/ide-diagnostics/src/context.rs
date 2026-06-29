use crate::DiagnosticsConfig;
use std::sync::Arc;
use vfs::FileId;

pub struct DiagnosticsContext<'a> {
    pub config: &'a DiagnosticsConfig,
    pub file_id: FileId,
    provider: &'a dyn ide_db::AnalysisProvider,
}

impl<'a> DiagnosticsContext<'a> {
    pub fn new(
        config: &'a DiagnosticsConfig,
        file_id: FileId,
        provider: &'a dyn ide_db::AnalysisProvider,
    ) -> Self {
        Self { config, file_id, provider }
    }

    pub fn locale(&self) -> base_db::Locale {
        self.config.locale
    }

    pub fn kernel_type_display(&self, id: hir::TypeId, locale: base_db::Locale) -> String {
        self.provider.kernel_type_display(id, locale)
    }

    pub fn main_configuration(&self) -> Option<Arc<bsl_metadata::Configuration>> {
        self.visible_configurations()
            .into_iter()
            .find(|vc| vc.config.name.is_none())
            .map(|vc| vc.config.configuration)
    }

    pub fn visible_configurations(&self) -> Vec<ide_db::provider::VisibleConfigWithRoot> {
        self.provider.visible_configurations(self.file_id)
    }

    /// The `Ext/Module.bsl` body file id(s) of the common module `name` visible to
    /// this file — base + its own extension. For diagnostics that read the module
    /// body (handler method existence/export, required parameters). Scoped
    /// extension-private through the per-common-module substrate.
    pub fn common_module_body_files(&self, name: &str) -> Vec<vfs::FileId> {
        self.provider.resolve_common_module_files(self.file_id, name)
    }

    /// The common module `name` visible to this file (base + its own extension),
    /// at per-common-module Salsa granularity. Replaces the former all-configs
    /// `find_common_module_anywhere`/`is_common_module_anywhere` scans, which were
    /// over-permissive (a sibling extension's module was visible) and depended on
    /// the whole configuration.
    pub fn resolve_common_module(&self, name: &str) -> Option<Arc<bsl_metadata::CommonModule>> {
        self.provider.resolve_common_module(self.file_id, name)
    }

    /// Resolve one EventSubscription visible to this file through the provider's
    /// single-name API. Salsa providers route this through the typed substrate.
    pub fn resolve_event_subscription(
        &self,
        name: &str,
    ) -> Option<Arc<bsl_metadata::EventSubscription>> {
        self.provider.resolve_event_subscription(self.file_id, name)
    }

    /// Main-configuration EventSubscription enumeration for diagnostics that need
    /// to scan declared subscriptions while preserving previous main-only behavior.
    pub fn main_event_subscriptions(&self) -> Vec<Arc<bsl_metadata::EventSubscription>> {
        self.provider.main_event_subscriptions(self.file_id)
    }

    pub fn is_common_module_anywhere(&self, name: &str) -> bool {
        self.resolve_common_module(name).is_some()
    }

    pub fn assignment_target_kind(&self, name: &str) -> hir::AssignmentResolution {
        self.provider.assignment_target_kind(self.file_id, name)
    }

    pub fn file_path(&self) -> Option<String> {
        self.provider.file_path(self.file_id)
    }

    /// The common module this file is the body of (its `Ext/Module.bsl`), if any.
    /// Routes through the per-common-module reverse index when the substrate is
    /// populated, so it depends on just that module rather than the whole config.
    pub fn common_module_for_file(&self) -> Option<Arc<bsl_metadata::CommonModule>> {
        self.provider.common_module_for_file(self.file_id)
    }

    /// The metadata object `(mdo_type, name)` visible to this file, resolved
    /// per-MDO (base + the file's own extension) so it depends on just that object.
    pub fn resolve_metadata_object(
        &self,
        mdo_type: bsl_metadata::MdoType,
        name: &str,
    ) -> Option<Arc<bsl_metadata::MetadataObject>> {
        self.provider.resolve_metadata_object(self.file_id, mdo_type, name)
    }

    fn query<T>(&self, f: impl FnOnce(&dyn ide_db::AnalysisProvider) -> T) -> T {
        f(self.provider)
    }

    pub fn parse(&self) -> syntax::Parse<syntax::SyntaxNode> {
        self.query(|p| p.parse(self.file_id))
    }

    pub fn module_bodies(&self) -> Arc<hir::ModuleBodies> {
        let module_id = hir::ModuleId::new(self.file_id);
        self.query(|p| p.module_bodies(module_id))
    }

    pub fn infer(&self) -> Arc<hir::InferenceResult> {
        self.query(|p| p.infer(self.file_id))
    }

    pub fn arg_diagnostics(&self) -> Arc<Vec<(hir::DefWithBodyId, hir::InferenceDiagnostic)>> {
        self.query(|p| p.arg_diagnostics(self.file_id))
    }

    pub fn module_metadata(&self) -> Arc<hir::ModuleMetadata> {
        let module_id = hir::ModuleId::new(self.file_id);
        self.query(|p| p.module_metadata(module_id))
    }

    pub fn module_implicit_field_names(&self) -> Vec<String> {
        self.query(|p| p.module_implicit_field_names(self.file_id))
    }

    pub fn symbol_tree(&self) -> Arc<hir::SymbolTree> {
        let module_id = hir::ModuleId::new(self.file_id);
        self.symbol_tree_for(module_id)
    }

    pub fn symbol_tree_for(&self, module_id: hir::ModuleId) -> Arc<hir::SymbolTree> {
        self.query(|p| p.symbol_tree(module_id))
    }

    pub fn call_summary(&self, module_id: hir::ModuleId) -> Arc<hir::ModuleCallSummary> {
        self.query(|p| p.call_summary(module_id))
    }

    pub fn item_tree(&self) -> Arc<hir::ItemTree> {
        self.query(|p| p.item_tree(self.file_id))
    }

    pub fn file_text(&self) -> Arc<String> {
        Arc::new(self.query(|p| p.file_text(self.file_id)))
    }

    pub fn line_index(&self) -> Arc<line_index::LineIndex> {
        self.query(|p| p.line_index(self.file_id))
    }

    pub fn source_root_id(&self) -> base_db::SourceRootId {
        self.query(|p| p.file_source_root_id(self.file_id))
    }

    pub fn module_index(&self) -> Arc<hir::ModuleIndex> {
        let source_root_id = self.source_root_id();
        self.query(|p| p.module_index(source_root_id))
    }

    pub fn module_cfgs(&self) -> Arc<hir::cfg::ModuleCfgs> {
        self.query(|p| p.module_cfgs(self.file_id))
    }

    pub fn module_liveness(&self) -> Arc<hir::dataflow::liveness::ModuleLiveness> {
        self.query(|p| p.module_liveness_analysis(self.file_id))
    }

    pub fn module_security_state(&self) -> Arc<ide_db::effects::ModuleSecurityState> {
        self.query(|p| p.module_security_state(self.file_id))
    }

    pub fn method_effect_summary(
        &self,
        method: hir::MethodId,
    ) -> std::sync::Arc<hir::dataflow::effect_summary::EffectSummary> {
        self.query(|p| p.method_effect_summary(method))
    }

    pub fn module_hir_metrics(&self) -> Arc<ide_db::queries::ModuleHirMetrics> {
        self.query(|p| p.module_hir_metrics(self.file_id))
    }

    pub fn module_cyclomatic(&self) -> Arc<ide_db::queries::ModuleCyclomatic> {
        self.query(|p| p.module_cyclomatic(self.file_id))
    }

    pub fn module_reaching_defs(&self) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs> {
        self.query(|p| p.module_reaching_definitions(self.file_id))
    }

    pub fn module_path_terminates(
        &self,
    ) -> Arc<hir::dataflow::path_terminates::ModulePathTerminates> {
        self.query(|p| p.module_path_terminates(self.file_id))
    }

    pub fn region_tree(&self) -> Arc<hir::RegionTree> {
        self.query(|p| p.region_tree(self.file_id))
    }

    pub fn sdbl_hir_in_file(&self) -> ide_db::SdblHirEntries {
        self.query(|p| p.sdbl_hir_in_file(self.file_id))
    }

    pub fn all_sdbl_in_file(&self) -> Arc<Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>> {
        self.query(|p| p.all_sdbl_in_file(self.file_id))
    }

    pub fn module_data(&self) -> Arc<hir::ModuleData> {
        let module_id = hir::ModuleId::new(self.file_id);
        self.query(|p| p.module_data(module_id))
    }

    pub fn method_docs(&self, method_id: hir::MethodId) -> Option<Arc<hir::MethodDocs>> {
        self.query(|p| p.method_docs(method_id))
    }

    pub fn variable_docs(&self, variable_id: hir::VariableId) -> Option<Arc<hir::VariableDocs>> {
        self.query(|p| p.variable_docs(variable_id))
    }

    pub fn file_external_refs(&self) -> std::sync::Arc<Vec<hir::ExternalRef>> {
        let module_id = hir::ModuleId::new(self.file_id);
        self.query(|p| p.file_external_refs(module_id))
    }

    pub fn module_level_liveness_analysis(
        &self,
    ) -> Option<std::sync::Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>>
    {
        let module_id = hir::ModuleId::new(self.file_id);
        self.query(|p| p.module_level_liveness_analysis(module_id))
    }

    pub fn module_bodies_for(&self, module_id: hir::ModuleId) -> std::sync::Arc<hir::ModuleBodies> {
        self.query(|p| p.module_bodies(module_id))
    }

    pub fn reaching_definitions(
        &self,
        method_id: hir::MethodId,
    ) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>> {
        self.query(|p| p.reaching_definitions(method_id))
    }

    pub fn resolve_vfs_path(
        &self,
        source_root_id: base_db::SourceRootId,
        vfs_path: &vfs::VfsPath,
    ) -> Option<vfs::FileId> {
        self.query(|p| p.resolve_vfs_path(source_root_id, vfs_path))
    }

    pub fn resolve_qualified_path(
        &self,
        module_name: &hir::Name,
        method_name: &hir::Name,
    ) -> hir::PathResolution {
        let module_index = self.module_index();

        let target_file_id = match module_index.resolve_common_module(module_name) {
            Some(id) => id,
            None => {
                return hir::PathResolution::Unresolved(hir::QualifiedName::from_segments([
                    module_name.clone(),
                    method_name.clone(),
                ]));
            }
        };

        let target_module_id = hir::ModuleId::new(target_file_id);
        let symbol_tree = self.symbol_tree_for(target_module_id);

        if let Some(method_symbol) = symbol_tree.find_method(method_name) {
            if method_symbol.is_export {
                return hir::PathResolution::Method(method_symbol.id);
            }
        }

        hir::PathResolution::Unresolved(hir::QualifiedName::from_segments([
            module_name.clone(),
            method_name.clone(),
        ]))
    }

    pub fn resolve_platform_global_member(
        &self,
        module_name: &hir::Name,
        method_name: &hir::Name,
    ) -> Option<bsl_platform::PlatformMethod> {
        let data = bsl_platform::PlatformDataInner::instance();
        data.resolve_global_member(module_name.as_str(), method_name.as_str()).cloned()
    }

    pub fn find_common_module_file(
        &self,
        common_module: &bsl_metadata::CommonModule,
    ) -> Option<vfs::FileId> {
        use bsl_metadata::traits::{MdObject, Module};

        let uri = common_module.uri()?;
        let file_id = self.resolve_module_file(uri);

        if file_id.is_none() {
            tracing::warn!(
                module = %common_module.name(),
                uri = %uri,
                "CommonModule file not found in VFS"
            );
        }

        file_id
    }

    pub fn resolve_module_file(&self, relative_uri: &str) -> Option<vfs::FileId> {
        self.query(|p| p.resolve_module_file(relative_uri))
    }

    pub fn config_int(&self, code: crate::DiagnosticCode, param: &str, default: i64) -> i64 {
        self.config.get_int(code, param).unwrap_or(default)
    }

    pub fn config_bool(&self, code: crate::DiagnosticCode, param: &str, default: bool) -> bool {
        self.config.get_bool(code, param).unwrap_or(default)
    }

    pub fn config_string(&self, code: crate::DiagnosticCode, param: &str, default: &str) -> String {
        self.config.get_string(code, param).unwrap_or(default).to_string()
    }

    pub fn severity(&self, code: crate::DiagnosticCode) -> crate::Severity {
        self.config
            .get_effective_metadata(code)
            .map(|m| m.severity_value())
            .unwrap_or(crate::Severity::Warning)
    }

    pub fn tags(&self, code: crate::DiagnosticCode) -> Vec<crate::DiagnosticTag> {
        self.config
            .get_effective_metadata(code)
            .map(|m| {
                let tags = m.tags();
                let mut result = Vec::new();
                for tag in tags {
                    match tag {
                        crate::metadata::MetadataTag::Unused => {
                            result.push(crate::DiagnosticTag::Unnecessary);
                        }
                        crate::metadata::MetadataTag::Deprecated => {
                            result.push(crate::DiagnosticTag::Deprecated);
                        }
                        _ => {}
                    }
                }
                result
            })
            .unwrap_or_default()
    }

    pub fn is_disabled_with_metadata(&self, code: crate::DiagnosticCode) -> bool {
        self.config.is_disabled(code)
    }
}
