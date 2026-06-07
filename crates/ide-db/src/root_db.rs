use std::sync::Arc;

use base_db::{FileIdInput, RootQueryDb, SourceDatabase};
use hir::DefDatabase;
use vfs::FileId;

use crate::SdblHirEntries;

#[salsa::db]
pub trait RootDatabase:
    SourceDatabase + RootQueryDb + DefDatabase + hir::HirDatabase + crate::metadata::MetadataDb
{
    fn get_configuration(&self, file_id: FileId) -> Option<Arc<bsl_metadata::Configuration>>;

    fn get_all_configurations(
        &self,
        file_id: FileId,
    ) -> Vec<(Option<String>, Arc<bsl_metadata::Configuration>)>;

    fn all_config_paths(&self) -> Vec<(Option<String>, std::path::PathBuf)>;

    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>>;

    fn sdbl_hir_in_file(&self, file_id: FileId) -> SdblHirEntries;

    fn module_cfgs(&self, file_id_input: FileIdInput) -> Arc<hir::cfg::ModuleCfgs>;

    fn module_reaching_definitions(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs>;

    fn module_path_terminates(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<hir::dataflow::path_terminates::ModulePathTerminates>;

    fn module_liveness_analysis(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<hir::dataflow::liveness::ModuleLiveness>;

    fn reaching_definitions(
        &self,
        method_id: hir::MethodId,
    ) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>>;

    fn liveness_analysis(
        &self,
        method_id: hir::MethodId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>>;

    fn method_cfg(&self, method_id: hir::MethodId) -> Arc<hir::cfg::ControlFlowGraph>;

    fn module_level_cfg(&self, module_id: hir::ModuleId) -> Arc<hir::cfg::ControlFlowGraph>;

    fn module_level_liveness_analysis(
        &self,
        module_id: hir::ModuleId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>>;

    fn line_index(&self, file_id_input: base_db::FileIdInput) -> Arc<line_index::LineIndex>;

    fn as_any(&self) -> &dyn std::any::Any;

    /// The Salsa-tracked config revision token for the root owning `path` (see
    /// [`RootDatabaseImpl::config_root_revision_for_path`](crate::RootDatabaseImpl::config_root_revision_for_path)).
    fn config_root_revision_for_path(&self, path: &std::path::Path) -> u32;
}
