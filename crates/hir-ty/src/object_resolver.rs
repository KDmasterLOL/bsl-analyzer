use std::sync::Arc;
use stdx::case::CaseExt;

use bsl_config::VisibleConfig;
use bsl_metadata::{AttributeType, MdoType, MetadataObject, MetadataResolver, Register};
use bsl_types::kind::MetadataReferenceKind;
use hir_def::ConfigsDatabase;
use hir_def::Name;
use vfs::FileId;

/// Per-MDO metadata resolution surface threaded into the type-lookup helpers
/// (`manager_lookup`, `field_enum`, `field_lookup`).
///
/// Returning owned `Arc`s lets the production resolver hand back per-MDO objects
/// composed fresh from their own Salsa cells, so a lookup depends on just the
/// MDOs it touches instead of the whole `Configuration`. The configs-backed
/// resolver mirrors the same merged + file-scoped semantics for unit tests and
/// the cold call sites that still carry a `&[VisibleConfig]`.
pub trait ObjectResolver {
    fn resolve_metadata_object(&self, mdo_type: MdoType, name: &str)
        -> Option<Arc<MetadataObject>>;

    fn resolve_register(&self, mdo_type: MdoType, name: &str) -> Option<Arc<Register>>;

    fn resolve_metadata_reference(&self, kind: MetadataReferenceKind, name: &str) -> Option<Name>;

    fn metadata_reference_members(&self, kind: MetadataReferenceKind) -> Vec<Name>;

    /// The documents that record into the register `(parent, register_name)` — a
    /// cross-MDO reverse relation (every document writing to the register), so it
    /// cannot narrow to a single MDO. Aggregated across the visible configs.
    fn recorders_for_register(&self, parent: MdoType, register_name: &str) -> Vec<String>;
}

/// The full metadata-resolution surface a field/manager lookup needs: object and
/// register resolution ([`ObjectResolver`]) plus defined-type chain resolution
/// ([`MetadataResolver`]). Threaded as one `&dyn MetadataResolution` so a single
/// handle covers both; it upcasts to `&dyn MetadataResolver` for the type-lowering
/// layer. The blanket impl makes every type that provides both surfaces eligible.
pub trait MetadataResolution: ObjectResolver + MetadataResolver {}

impl<T: ObjectResolver + MetadataResolver + ?Sized> MetadataResolution for T {}

/// Production resolver: routes per-MDO resolution through the file-scoped,
/// overlay-aware [`ConfigsDatabase`] accessors. Generic over the database trait
/// object so `&dyn HirDatabase` (which is a `ConfigsDatabase`) can be passed
/// without trait upcasting.
pub struct DbObjectResolver<'a, D: ConfigsDatabase + ?Sized> {
    db: &'a D,
    file_id: FileId,
}

impl<'a, D: ConfigsDatabase + ?Sized> DbObjectResolver<'a, D> {
    pub fn new(db: &'a D, file_id: FileId) -> Self {
        Self { db, file_id }
    }
}

impl<D: ConfigsDatabase + ?Sized> std::fmt::Debug for DbObjectResolver<'_, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbObjectResolver").field("file_id", &self.file_id).finish()
    }
}

impl<D: ConfigsDatabase + ?Sized> ObjectResolver for DbObjectResolver<'_, D> {
    fn resolve_metadata_object(
        &self,
        mdo_type: MdoType,
        name: &str,
    ) -> Option<Arc<MetadataObject>> {
        self.db.resolve_metadata_object(self.file_id, mdo_type, name)
    }

    fn resolve_register(&self, mdo_type: MdoType, name: &str) -> Option<Arc<Register>> {
        self.db.resolve_register(self.file_id, mdo_type, name)
    }

    fn resolve_metadata_reference(&self, kind: MetadataReferenceKind, name: &str) -> Option<Name> {
        match kind {
            MetadataReferenceKind::Subsystem => {
                self.db.resolve_subsystem(self.file_id, name).map(|item| Name::new(item.name()))
            }
            MetadataReferenceKind::Role => {
                self.db.resolve_role(self.file_id, name).map(|role| Name::new(role.name()))
            }
            MetadataReferenceKind::EventSubscription => self
                .db
                .resolve_event_subscription(self.file_id, name)
                .map(|subscription| Name::new(subscription.name())),
            MetadataReferenceKind::ScheduledJob => {
                self.db.resolve_scheduled_job(self.file_id, name).map(|job| Name::new(job.name()))
            }
            MetadataReferenceKind::HttpService => self
                .db
                .resolve_http_service(self.file_id, name)
                .map(|service| Name::new(service.name())),
            MetadataReferenceKind::WebService => self
                .db
                .resolve_web_service(self.file_id, name)
                .map(|service| Name::new(service.name())),
        }
    }

    fn metadata_reference_members(&self, kind: MetadataReferenceKind) -> Vec<Name> {
        match kind {
            MetadataReferenceKind::Subsystem => self
                .db
                .subsystem_names(self.file_id)
                .into_iter()
                .map(|name| Name::new(&name))
                .collect(),
            MetadataReferenceKind::Role => {
                self.db.role_names(self.file_id).into_iter().map(|name| Name::new(&name)).collect()
            }
            MetadataReferenceKind::EventSubscription => self
                .db
                .event_subscription_names(self.file_id)
                .into_iter()
                .map(|name| Name::new(&name))
                .collect(),
            MetadataReferenceKind::ScheduledJob => self
                .db
                .scheduled_job_names(self.file_id)
                .into_iter()
                .map(|name| Name::new(&name))
                .collect(),
            MetadataReferenceKind::HttpService => self
                .db
                .http_service_names(self.file_id)
                .into_iter()
                .map(|name| Name::new(&name))
                .collect(),
            MetadataReferenceKind::WebService => self
                .db
                .web_service_names(self.file_id)
                .into_iter()
                .map(|name| Name::new(&name))
                .collect(),
        }
    }

    fn recorders_for_register(&self, parent: MdoType, register_name: &str) -> Vec<String> {
        self.db.recorders_for_register(self.file_id, parent, register_name)
    }
}

impl<D: ConfigsDatabase + ?Sized> MetadataResolver for DbObjectResolver<'_, D> {
    fn resolve_defined_type(&self, name: &str) -> Option<AttributeType> {
        self.db.resolve_defined_type(self.file_id, name)
    }
}

/// Configs-backed resolver: overlays every config in the slice in order so a
/// later one wins (`apply_extension_overlay`).
///
/// Unlike the production [`DbObjectResolver`] path, this does **not** itself pick
/// the file-applicable extension — it merges whatever slice it is handed. The
/// caller must pre-scope: pass `[base, applicable_extension]` to get the same
/// result as `ConfigsDatabase::resolve_metadata_object`. Feeding it a raw
/// `configurations(file_id)` (every root) would merge all extensions, not the
/// one that applies to the file. Used by unit tests, which pass that scoped pair
/// deliberately.
pub struct ConfigsObjectResolver<'a>(pub &'a [VisibleConfig]);

impl std::fmt::Debug for ConfigsObjectResolver<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigsObjectResolver").field("configs", &self.0.len()).finish()
    }
}

impl MetadataResolver for ConfigsObjectResolver<'_> {
    fn resolve_defined_type(&self, name: &str) -> Option<AttributeType> {
        self.0.iter().rev().find_map(|cfg| cfg.configuration.resolve_defined_type(name))
    }
}

impl ObjectResolver for ConfigsObjectResolver<'_> {
    fn resolve_metadata_object(
        &self,
        mdo_type: MdoType,
        name: &str,
    ) -> Option<Arc<MetadataObject>> {
        let mut merged: Option<MetadataObject> = None;
        for cfg in self.0 {
            if let Some(found) = cfg.configuration.find_metadata_object(mdo_type, name) {
                match &mut merged {
                    Some(base) => base.apply_extension_overlay(found),
                    None => merged = Some(found.clone()),
                }
            }
        }
        merged.map(Arc::new)
    }

    fn resolve_register(&self, mdo_type: MdoType, name: &str) -> Option<Arc<Register>> {
        let mut merged: Option<Register> = None;
        for cfg in self.0 {
            if let Some(found) = cfg.configuration.find_register_by_type_and_name(mdo_type, name) {
                match &mut merged {
                    Some(base) => base.apply_extension_overlay(found),
                    None => merged = Some(found.clone()),
                }
            }
        }
        merged.map(Arc::new)
    }

    fn resolve_metadata_reference(&self, kind: MetadataReferenceKind, name: &str) -> Option<Name> {
        self.0
            .iter()
            .rev()
            .find_map(|cfg| metadata_reference_name(cfg.configuration.as_ref(), kind, name))
    }

    fn metadata_reference_members(&self, kind: MetadataReferenceKind) -> Vec<Name> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for cfg in self.0 {
            for name in metadata_reference_names(cfg.configuration.as_ref(), kind) {
                if seen.insert(name.as_str().fold_lower()) {
                    out.push(name);
                }
            }
        }
        out
    }

    fn recorders_for_register(&self, parent: MdoType, register_name: &str) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for cfg in self.0 {
            for name in cfg.configuration.recorders_for_register(parent, register_name) {
                let name = name.as_str();
                if seen.insert(name.fold_lower()) {
                    out.push(name.to_string());
                }
            }
        }
        out
    }
}

fn metadata_reference_name(
    config: &bsl_metadata::Configuration,
    kind: MetadataReferenceKind,
    name: &str,
) -> Option<Name> {
    match kind {
        MetadataReferenceKind::Role => config.find_role(name).map(|item| Name::new(item.name())),
        MetadataReferenceKind::EventSubscription => {
            config.find_event_subscription(name).map(|item| Name::new(item.name()))
        }
        MetadataReferenceKind::ScheduledJob => {
            config.find_scheduled_job(name).map(|item| Name::new(item.name()))
        }
        MetadataReferenceKind::HttpService => {
            config.find_http_service(name).map(|item| Name::new(item.name()))
        }
        MetadataReferenceKind::WebService => {
            config.find_web_service(name).map(|item| Name::new(item.name()))
        }
        MetadataReferenceKind::Subsystem => {
            let name_lower = name.fold_lower();
            config
                .subsystems()
                .iter()
                .find(|item| item.name().fold_lower() == name_lower)
                .map(|item| Name::new(item.name()))
        }
    }
}

fn metadata_reference_names(
    config: &bsl_metadata::Configuration,
    kind: MetadataReferenceKind,
) -> Vec<Name> {
    match kind {
        MetadataReferenceKind::Role => {
            config.roles().iter().map(|item| Name::new(item.name())).collect()
        }
        MetadataReferenceKind::EventSubscription => {
            config.event_subscriptions().iter().map(|item| Name::new(item.name())).collect()
        }
        MetadataReferenceKind::ScheduledJob => {
            config.scheduled_jobs().iter().map(|item| Name::new(item.name())).collect()
        }
        MetadataReferenceKind::HttpService => {
            config.http_services().iter().map(|item| Name::new(item.name())).collect()
        }
        MetadataReferenceKind::WebService => {
            config.web_services().iter().map(|item| Name::new(item.name())).collect()
        }
        MetadataReferenceKind::Subsystem => {
            config.subsystems().iter().map(|item| Name::new(item.name())).collect()
        }
    }
}
