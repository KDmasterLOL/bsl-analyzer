use std::sync::Arc;

use bsl_metadata::{AttributeType, Configuration, MdoType, MetadataObject, Register};
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

    /// Resolve the underlying type of the defined type `name` visible to
    /// `file_id`, at per-defined-type Salsa granularity. Base + the file's own
    /// extension (which replaces the underlying type wholesale). Returns the
    /// terminal type only after one hop; defined-type chains are unwound by
    /// [`bsl_metadata::resolve_defined_type_terminal`] in the type-lowering layer.
    fn resolve_defined_type(&self, file_id: FileId, name: &str) -> Option<AttributeType>;

    /// Resolve the common module `name` visible to `file_id` at per-common-module
    /// Salsa granularity: base + the file's own extension (which replaces the module
    /// wholesale). Used for by-name visibility/flag checks; the module's body is
    /// resolved separately through the symbol tree. Depending on this records a
    /// dependency on just that common module.
    fn resolve_common_module(
        &self,
        file_id: FileId,
        name: &str,
    ) -> Option<Arc<bsl_metadata::CommonModule>>;

    /// Whether `file_id` belongs to a configured project (has at least one visible
    /// config root). In the workspace path this reads only the config-paths input,
    /// so callers can gate on config presence — distinguishing "no config, defer to
    /// the module index" from "config present, name genuinely absent" — without
    /// taking a dependency on the whole loaded configuration.
    fn has_config_root(&self, file_id: FileId) -> bool;

    /// The documents recording into the register `(parent, register_name)` visible
    /// to `file_id` — a cross-MDO reverse relation aggregated across the base and
    /// the file's extension. Returns owned names (not a borrow) so a db-backed
    /// resolver can compose them. Not yet narrowed to a reverse index, so it still
    /// depends on the whole visible configuration.
    fn recorders_for_register(
        &self,
        file_id: FileId,
        parent: MdoType,
        register_name: &str,
    ) -> Vec<String>;

    fn resolved_module_summary(
        &self,
        module_id: crate::ModuleId,
    ) -> Arc<crate::call_graph::ResolvedModuleSummary>;

    fn workspace_call_graph(
        &self,
        source_root_id: base_db::SourceRootId,
    ) -> Arc<crate::call_graph::WorkspaceCallGraph>;
}
