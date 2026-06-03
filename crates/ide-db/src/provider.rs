use std::path::PathBuf;
use std::sync::Arc;

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

    fn workspace_symbols(&self, source_root_id: SourceRootId) -> Arc<hir::WorkspaceSymbols>;

    fn module_index(&self, source_root_id: SourceRootId) -> Arc<ModuleIndex>;

    fn assignment_target_kind(&self, _file_id: FileId, _name: &str) -> AssignmentResolution {
        AssignmentResolution::Unknown
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
