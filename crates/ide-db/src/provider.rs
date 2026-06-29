use std::path::PathBuf;
use std::sync::Arc;
use stdx::case::CaseExt;

use base_db::SourceRootId;
use bsl_metadata::Configuration;
use hir::{
    dataflow::effect_summary::EffectSummary, AssignmentResolution, DefWithBodyId,
    InferenceDiagnostic, InferenceResult, ItemTree, MethodDocs, MethodId, ModuleBodies, ModuleId,
    ModuleIndex, ModuleMetadata, SymbolTree, VariableDocs, VariableId,
};
use syntax::{Parse, SyntaxNode};
use vfs::{FileId, VfsPath};

use crate::{
    effects::ModuleSecurityState,
    queries::{ModuleCyclomatic, ModuleHirMetrics},
    SdblHirEntries,
};

#[derive(Clone)]
pub struct VisibleConfigWithRoot {
    pub config: bsl_config::VisibleConfig,
    pub root: PathBuf,
}

pub trait AnalysisProvider {
    fn configuration(&self) -> Option<Arc<Configuration>>;

    fn visible_configurations(&self, _file_id: FileId) -> Vec<VisibleConfigWithRoot> {
        self.configuration()
            .map(|cfg| {
                vec![VisibleConfigWithRoot {
                    config: bsl_config::VisibleConfig { name: None, configuration: cfg },
                    root: PathBuf::new(),
                }]
            })
            .unwrap_or_default()
    }

    /// The common module whose `Ext/Module.bsl` is `file_id` (i.e. the file is the
    /// module's own body), if any — answering "is this file a common module, and
    /// which?". The default scans the file's visible configs by root-relative URI;
    /// the salsa-backed provider overrides it with a per-common-module reverse index.
    fn common_module_for_file(&self, file_id: FileId) -> Option<Arc<bsl_metadata::CommonModule>> {
        use bsl_metadata::traits::Module;

        let file_path = self.file_path(file_id)?;
        let file_path_lower = file_path.fold_lower();
        for visible in self.visible_configurations(file_id) {
            let found = visible.config.configuration.common_modules().iter().find(|m| {
                m.uri().is_some_and(|uri| {
                    if visible.root.as_os_str().is_empty() {
                        uri.fold_lower() == file_path_lower
                    } else {
                        visible.root.join(uri).to_string_lossy().fold_lower() == file_path_lower
                    }
                })
            });
            if let Some(m) = found {
                return Some(Arc::new(m.clone()));
            }
        }
        None
    }

    /// The metadata object `(mdo_type, name)` visible to `file_id`, merged across
    /// the file's visible configs (base + applicable extension) via
    /// `apply_extension_overlay`. The default folds whatever `visible_configurations`
    /// returns; the salsa-backed provider overrides it with the per-MDO accessor so
    /// it depends on just that object instead of the whole configuration.
    fn resolve_metadata_object(
        &self,
        file_id: FileId,
        mdo_type: bsl_metadata::MdoType,
        name: &str,
    ) -> Option<Arc<bsl_metadata::MetadataObject>> {
        let mut merged: Option<bsl_metadata::MetadataObject> = None;
        for visible in self.visible_configurations(file_id) {
            if let Some(found) = visible.config.configuration.find_metadata_object(mdo_type, name) {
                match &mut merged {
                    Some(base) => base.apply_extension_overlay(found),
                    None => merged = Some(found.clone()),
                }
            }
        }
        merged.map(Arc::new)
    }

    /// The common module `name` visible to `file_id` — base config plus the file's
    /// own extension (an extension's common module is visible only within that
    /// extension). The default scans the file's visible configs; the salsa-backed
    /// provider overrides it with the per-common-module accessor.
    fn resolve_common_module(
        &self,
        file_id: FileId,
        name: &str,
    ) -> Option<Arc<bsl_metadata::CommonModule>> {
        self.visible_configurations(file_id)
            .into_iter()
            .find_map(|visible| visible.config.configuration.find_common_module(name).cloned())
            .map(Arc::new)
    }

    /// The event subscription `name` visible to `file_id`. The default scans the
    /// provider's visible configurations; the salsa-backed provider overrides it
    /// with the per-event-subscription accessor.
    fn resolve_event_subscription(
        &self,
        file_id: FileId,
        name: &str,
    ) -> Option<Arc<bsl_metadata::EventSubscription>> {
        self.visible_configurations(file_id)
            .into_iter()
            .find_map(|visible| visible.config.configuration.find_event_subscription(name).cloned())
            .map(Arc::new)
    }

    /// Main-configuration EventSubscription enumeration for diagnostics that scan
    /// declared subscriptions while preserving the existing main-only behavior.
    fn main_event_subscriptions(
        &self,
        file_id: FileId,
    ) -> Vec<Arc<bsl_metadata::EventSubscription>> {
        self.visible_configurations(file_id)
            .into_iter()
            .find(|visible| visible.config.name.is_none())
            .map(|visible| {
                visible
                    .config
                    .configuration
                    .event_subscriptions()
                    .iter()
                    .cloned()
                    .map(Arc::new)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The `Ext/Module.bsl` body file id(s) of the common module `name` visible to
    /// `file_id`, for diagnostics that validate the module body (handler method
    /// existence/export, required parameters). The default resolves bodies by
    /// root-relative URI across the file's visible configs; the salsa-backed
    /// provider overrides it with the substrate, scoped base + the file's own
    /// extension.
    fn resolve_common_module_files(&self, file_id: FileId, name: &str) -> Vec<FileId> {
        use bsl_metadata::traits::Module;

        let mut out = Vec::new();
        for visible in self.visible_configurations(file_id) {
            let Some(uri) =
                visible.config.configuration.find_common_module(name).and_then(|m| m.uri())
            else {
                continue;
            };
            let resolved = if visible.root.as_os_str().is_empty() {
                self.resolve_module_file(uri)
            } else {
                let vfs_path = VfsPath::new(visible.root.join(uri).to_string_lossy().into_owned());
                self.resolve_vfs_path(SourceRootId(0), &vfs_path)
            };
            if let Some(fid) = resolved {
                if !out.contains(&fid) {
                    out.push(fid);
                }
            }
        }
        out
    }

    fn workspace_symbols(&self, source_root_id: SourceRootId) -> Arc<hir::WorkspaceSymbols>;

    fn module_index(&self, source_root_id: SourceRootId) -> Arc<ModuleIndex>;

    fn assignment_target_kind(&self, _file_id: FileId, _name: &str) -> AssignmentResolution {
        AssignmentResolution::Unknown
    }

    /// Lowercased ru/en names of the module's implicit context fields (object
    /// attributes, register dimensions/resources/attributes, form attributes,
    /// platform record-set properties such as БлокироватьДляИзменения). Empty
    /// for providers without type information.
    fn module_implicit_field_names(&self, _file_id: FileId) -> Vec<String> {
        Vec::new()
    }

    fn kernel_type_display(&self, id: bsl_types::kind::TypeId, locale: base_db::Locale) -> String;

    fn parse(&self, file_id: FileId) -> Parse<SyntaxNode>;

    fn file_text(&self, file_id: FileId) -> String;

    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree>;

    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree>;

    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies>;

    fn infer(&self, _file_id: FileId) -> Arc<InferenceResult> {
        Arc::new(InferenceResult::default())
    }

    fn arg_diagnostics(&self, _file_id: FileId) -> Arc<Vec<(DefWithBodyId, InferenceDiagnostic)>> {
        Arc::new(Vec::new())
    }

    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata>;

    fn call_summary(&self, module_id: ModuleId) -> Arc<hir::ModuleCallSummary>;

    fn line_index(&self, file_id: FileId) -> Arc<line_index::LineIndex>;

    fn file_path(&self, file_id: FileId) -> Option<String>;

    fn file_source_root_id(&self, file_id: FileId) -> SourceRootId;

    fn region_tree(&self, file_id: FileId) -> Arc<hir::RegionTree>;

    fn sdbl_hir_in_file(&self, file_id: FileId) -> SdblHirEntries;

    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>>;

    fn module_data(&self, module_id: ModuleId) -> Arc<hir::ModuleData>;

    fn method_docs(&self, method_id: MethodId) -> Option<Arc<MethodDocs>>;

    fn variable_docs(&self, variable_id: VariableId) -> Option<Arc<VariableDocs>>;

    fn module_cfgs(&self, file_id: FileId) -> Arc<hir::cfg::ModuleCfgs>;

    fn module_liveness_analysis(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::liveness::ModuleLiveness>;

    fn module_reaching_definitions(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs>;

    fn module_path_terminates(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::path_terminates::ModulePathTerminates>;

    fn reaching_definitions(
        &self,
        method_id: MethodId,
    ) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>>;

    fn file_external_refs(&self, module_id: ModuleId) -> Arc<Vec<hir::ExternalRef>>;

    fn module_level_liveness_analysis(
        &self,
        module_id: ModuleId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>>;

    fn resolve_vfs_path(&self, source_root_id: SourceRootId, vfs_path: &VfsPath) -> Option<FileId>;

    fn resolve_module_file(&self, relative_uri: &str) -> Option<FileId>;

    fn method_effect_summary(&self, _method: MethodId) -> Arc<EffectSummary> {
        Arc::new(EffectSummary::EMPTY)
    }

    fn module_security_state(&self, _file_id: FileId) -> Arc<ModuleSecurityState> {
        Arc::new(ModuleSecurityState::default())
    }

    fn method_hir_metrics(&self, _method: MethodId) -> Arc<hir::metrics::HirMethodMetrics> {
        Arc::new(hir::metrics::HirMethodMetrics::default())
    }

    fn module_hir_metrics(&self, _file_id: FileId) -> Arc<ModuleHirMetrics> {
        Arc::new(ModuleHirMetrics::default())
    }

    fn method_cyclomatic(&self, _method: MethodId) -> u32 {
        1
    }

    fn module_cyclomatic(&self, _file_id: FileId) -> Arc<ModuleCyclomatic> {
        Arc::new(ModuleCyclomatic::default())
    }
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

        assert_eq!(bodies.iter_bodies().count(), 1);
    }

    #[test]
    fn test_salsa_provider_symbol_tree() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let module_id = ModuleId::new(FileId(0));
        let symbols = provider.symbol_tree(module_id);

        assert!(symbols.find_method(&hir::Name::new("Тест")).is_some());
    }

    #[test]
    fn test_salsa_provider_line_index() {
        let db = setup_db();
        let provider = SalsaProvider::new(&db, None);

        let line_index = provider.line_index(FileId(0));
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

        assert!(provider.configuration().is_none());
    }
}
