use std::sync::Arc;

use bsl_metadata::{
    AttributeType, Configuration, EventSubscription, HTTPService, MdoType, MetadataObject,
    Register, Role, ScheduledJob, Subsystem, WebService,
};
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

    /// Resolve a register visible to `file_id` by NAME alone, when the caller does
    /// not know its kind (e.g. a `Движения.<Register>` movement touch). Same
    /// per-MDO granularity and base + own-extension scoping as [`resolve_register`].
    fn resolve_register_by_name(&self, file_id: FileId, name: &str) -> Option<Arc<Register>>;

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

    /// Resolve the event subscription `name` visible to `file_id` at
    /// per-event-subscription Salsa granularity: base + the file's own extension
    /// (extension priority). Depending on this records a dependency on just that
    /// subscription when the substrate is bootstrapped.
    fn resolve_event_subscription(
        &self,
        file_id: FileId,
        name: &str,
    ) -> Option<Arc<EventSubscription>>;

    /// Names of event subscriptions visible to `file_id`, for completion/member
    /// enumeration. File-scoped like [`resolve_event_subscription`].
    fn event_subscription_names(&self, file_id: FileId) -> Vec<String>;

    /// Explicit project/config enumeration for graph-style consumers that need the
    /// subscription handler metadata, not just a single hot lookup.
    fn enumerate_event_subscriptions(&self, file_id: FileId) -> Vec<Arc<EventSubscription>>;

    /// Resolve the role `name` visible to `file_id` at per-role Salsa granularity.
    /// This follows the same main-listing single-name parity as [`role_names`]:
    /// a single-name lookup resolves against the roles recorded for the file's
    /// main listing, while graph-style callers that need every root should use
    /// [`enumerate_roles`].
    fn resolve_role(&self, file_id: FileId, name: &str) -> Option<Arc<Role>>;

    /// Names of roles visible to `file_id`, for completion/member enumeration.
    /// File-scoped like [`resolve_role`], and likewise tied to the main listing.
    fn role_names(&self, file_id: FileId) -> Vec<String>;

    /// Explicit project/config enumeration for graph-style consumers that need
    /// every listed role object, not just main-listing names.
    fn enumerate_roles(&self, file_id: FileId) -> Vec<Arc<Role>>;

    /// Resolve the scheduled job `name` visible to `file_id` at per-scheduled-job
    /// Salsa granularity. File-scoped like [`resolve_event_subscription`], but
    /// Wave 2b resolves from the main listing only: `Configuration`'s extension
    /// overlay does not merge scheduled jobs today, so the bootstrapped path
    /// intentionally matches the merged whole-config lookup by ignoring the
    /// file's own extension root for the per-name resolution. The scheduled-job
    /// counterpart of [`resolve_event_subscription`].
    fn resolve_scheduled_job(&self, file_id: FileId, name: &str) -> Option<Arc<ScheduledJob>>;

    /// Names of scheduled jobs visible to `file_id`, for completion/member
    /// enumeration. File-scoped like [`resolve_scheduled_job`].
    fn scheduled_job_names(&self, file_id: FileId) -> Vec<String>;

    /// Resolve the HTTP service `name` visible to `file_id` at per-service Salsa
    /// granularity. Wave 2d keeps main-listing parity with the current whole-config
    /// service surface.
    fn resolve_http_service(&self, file_id: FileId, name: &str) -> Option<Arc<HTTPService>>;

    /// Names of HTTP services visible to `file_id`, for completion/member enumeration.
    fn http_service_names(&self, file_id: FileId) -> Vec<String>;

    /// Resolve the Web service `name` visible to `file_id` at per-service Salsa
    /// granularity. Wave 2d keeps main-listing parity with the current whole-config
    /// service surface.
    fn resolve_web_service(&self, file_id: FileId, name: &str) -> Option<Arc<WebService>>;

    /// Names of Web services visible to `file_id`, for completion/member enumeration.
    fn web_service_names(&self, file_id: FileId) -> Vec<String>;

    /// Resolve the subsystem `name` visible to `file_id` at per-subsystem Salsa
    /// granularity. The subsystem counterpart of [`resolve_scheduled_job`]; the
    /// bootstrapped path composes the main listing with the file's own extension
    /// listing, merging a same-name extension into the base via
    /// [`Subsystem::merge_from`] and resolving an extension-only subsystem from
    /// the extension listing. When the substrate is not bootstrapped, falls back
    /// to the merged whole-config subsystem lookup.
    fn resolve_subsystem(&self, file_id: FileId, name: &str) -> Option<Arc<Subsystem>>;

    /// Names of subsystems visible to `file_id`, for completion/member enumeration.
    /// File-scoped like [`resolve_subsystem`].
    fn subsystem_names(&self, file_id: FileId) -> Vec<String>;

    /// Explicit project/config enumeration for graph-style consumers that need
    /// every listed subsystem, not just a single hot lookup. The subsystem
    /// counterpart of [`enumerate_roles`]: prefers all configured listings when
    /// all are bootstrapped, merging same-name subsystems deterministically (base
    /// first, then extension order); falls back to the whole-config enumeration
    /// only behind this explicit enumeration API.
    fn enumerate_subsystems(&self, file_id: FileId) -> Vec<Arc<Subsystem>>;

    /// Whether `file_id` belongs to a configured project (has at least one visible
    /// config root). In the workspace path this reads only the config-paths input,
    /// so callers can gate on config presence — distinguishing "no config, defer to
    /// the module index" from "config present, name genuinely absent" — without
    /// taking a dependency on the whole loaded configuration.
    fn has_config_root(&self, file_id: FileId) -> bool;

    /// Whether `file_id` has a visible main configuration or an applicable
    /// extension configuration without loading the merged configuration.
    fn file_has_visible_config(&self, file_id: FileId) -> bool;

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
