use std::sync::Arc;

use bsl_metadata::{Configuration, MdoType, MetadataObject, Register};
use vfs::FileId;

use crate::DefDatabase;

use bsl_config::VisibleConfig;

#[salsa::db]
pub trait ConfigsDatabase: DefDatabase {
    fn configurations(&self, file_id: FileId) -> Vec<VisibleConfig>;

    fn merged_visible_configuration(&self, file_id: FileId) -> Option<Arc<Configuration>>;

    /// Resolve a single MetadataObject-family object (catalog, document, enum,
    /// constant, …) visible to `file_id` at per-MDO Salsa granularity: the base
    /// config composed with the file's own extension (extension priority), with no
    /// other extension visible. Depending on this records a dependency on just that
    /// MDO, so editing an unrelated MDO does not invalidate the caller — unlike
    /// reading the whole [`configurations`]/[`merged_visible_configuration`].
    ///
    /// Registers are a separate type/query; resolve them via [`resolve_register`].
    fn resolve_metadata_object(
        &self,
        file_id: FileId,
        mdo_type: MdoType,
        name: &str,
    ) -> Option<Arc<MetadataObject>>;

    /// The register counterpart of [`resolve_metadata_object`] (registers are a
    /// separate type). Same per-MDO granularity and base + own-extension scoping.
    fn resolve_register(
        &self,
        file_id: FileId,
        mdo_type: MdoType,
        name: &str,
    ) -> Option<Arc<Register>>;

    fn resolved_module_summary(
        &self,
        module_id: crate::ModuleId,
    ) -> Arc<crate::call_graph::ResolvedModuleSummary>;

    fn workspace_call_graph(
        &self,
        source_root_id: base_db::SourceRootId,
    ) -> Arc<crate::call_graph::WorkspaceCallGraph>;
}
