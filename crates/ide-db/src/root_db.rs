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

    /// The configurations VISIBLE to `file_id` as separate entries (no merge):
    /// the base first, then the file's dependency chain in order (own extension
    /// last). A base-config file sees only the base; an extension file sees the
    /// base, its declared transitive dependencies and itself — never an
    /// unrelated sibling.
    fn get_all_configurations(
        &self,
        file_id: FileId,
    ) -> Vec<(Option<String>, Arc<bsl_metadata::Configuration>)>;

    /// EVERY configured root (base + all extensions), regardless of any file's
    /// visibility — the inventory view for index/graph builders that need the
    /// whole workspace. Semantic per-file resolution must use
    /// [`Self::get_all_configurations`] instead: this union deliberately ignores
    /// dependency-scoped visibility.
    fn all_configurations_inventory(
        &self,
    ) -> Vec<(Option<String>, Arc<bsl_metadata::Configuration>)>;

    fn all_config_paths(&self) -> Vec<(Option<String>, std::path::PathBuf)>;

    /// Topological rank and extension label of the root owning `file_id`, using
    /// the same configured/canonical longest-prefix rule as visibility.
    fn config_root_rank_and_label(&self, file_id: FileId) -> Option<(usize, Option<String>)>;

    /// Topological ranks of roots visible to `file_id`, or `None` when the
    /// workspace has no configured root visibility.
    fn visible_config_root_ranks(&self, file_id: FileId) -> Option<Vec<usize>>;

    /// The common module that owns the `Ext/Module.bsl` whose id is `module_file_id`
    /// (typically the file being analysed), or `None` if it is not a common module's
    /// body. Per-common-module Salsa granularity when the substrate is populated.
    fn common_module_for_file_id(
        &self,
        module_file_id: FileId,
    ) -> Option<Arc<bsl_metadata::CommonModule>>;

    fn http_service_for_file_id(
        &self,
        module_file_id: FileId,
    ) -> Option<Arc<bsl_metadata::HTTPService>>;

    fn web_service_for_file_id(
        &self,
        module_file_id: FileId,
    ) -> Option<Arc<bsl_metadata::WebService>>;

    fn integration_service_for_file_id(
        &self,
        module_file_id: FileId,
    ) -> Option<Arc<bsl_metadata::IntegrationService>>;

    /// The `Ext/Module.bsl` bodies of the common module `name` visible to `file_id`
    /// (base + the file's own extension), each carrying whether its bytes could be
    /// read. For method/parameter validation that must read the module body, scoped
    /// extension-private like the metadata.
    ///
    /// The composition rather than a plain list of files, because a consumer that
    /// cannot see an unread body decides "the module has no such method" from bodies
    /// that were never entitled to answer.
    ///
    /// This is the MERGED surface, ordered extension-first — not priority order. Walk
    /// it with [`hir::CommonModuleBodies::search_merged_surface`]; `search` would stop
    /// at "the first" unread body, and in this order the unread one can be last.
    fn resolve_common_module_files(&self, file_id: FileId, name: &str) -> hir::CommonModuleBodies;

    /// Uncached application-host path lookup used only behind the tracked
    /// `application_module_files_query` aggregation.
    fn resolve_application_module_files_uncached(
        &self,
        file_id: FileId,
        kind: hir::ApplicationModuleKind,
    ) -> Option<hir::CommonModuleBodies>;

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

    /// The config roots visible to `file_id`: the base plus the file's
    /// dependency-ordered extension chain (own extension last). `None` when no
    /// roots are registered or the file's path is unknown.
    fn visible_roots_for_file(&self, file_id: FileId) -> Option<crate::VisibleRoots>;
}
