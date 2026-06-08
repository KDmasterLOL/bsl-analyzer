use std::sync::Arc;

use bsl_config::VisibleConfig;
use bsl_metadata::{AttributeType, MdoType, MetadataObject, MetadataResolver, Register};
use hir_def::ConfigsDatabase;
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

    fn recorders_for_register(&self, parent: MdoType, register_name: &str) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for cfg in self.0 {
            for name in cfg.configuration.recorders_for_register(parent, register_name) {
                let name = name.as_str();
                if seen.insert(name.to_lowercase()) {
                    out.push(name.to_string());
                }
            }
        }
        out
    }
}
