//! `hir::Type` facade.
//!
//! Unified IDE entry point for asking semantic questions about a type:
//! "what methods are callable on this?", "what's the type of this
//! field?", "is this a reference type?". Wraps [`hir_ty::Ty`] and
//! plumbs through the M3 adapters — [`hir_ty::lookup_method`],
//! [`hir_ty::lookup_field`] — plus [`PlatformData`] / [`Configuration`]
//! enumeration for the list-shaped queries.
//!
//! ## Why this exists
//!
//! Before M3, IDE layers (`ide/src/completion/platform_completion.rs`,
//! `mdo_completion.rs`, `hover.rs`) each dug into
//! [`PlatformData::instance`] and [`Configuration`] directly. That
//! violated Invariant #3 ("one façade for type info") and spread the
//! same "receiver type → methods / fields" logic across five call
//! sites. The façade gives every IDE feature one entry point; the
//! adapters underneath remain a single source of truth.
//!
//! ## Not in scope
//!
//! - `is_assignable_to` — deferred to M4. Without narrowing or
//!   union-subtyping semantics a naïve implementation would lie
//!   (Codex Q4, M3 plan). Callers that need a compatibility check
//!   should wait for the M4 ADR.
//! - `is_nullable` — `Null` / `Undefined` are separate `Ty` variants,
//!   not a modifier on other types. No dedicated method needed.

use bsl_metadata::{AttributeType, MdoType};
use bsl_platform::{PlatformData, PlatformMethod};
use hir_def::configs::ConfigsDatabase;
use hir_def::ty::{MetadataKind, Ty};
use hir_def::type_ref::TypeRef;
use hir_def::Name;
use hir_ty::lower::TyLoweringContext;
use hir_ty::{lookup_field, lookup_method};
use vfs::FileId;

/// Lightweight DTO for a method exposed by a [`Type`].
///
/// Carries only the name and return type — parameter lists, docs, etc.
/// live on [`PlatformMethod`] / `MethodSymbol` and can be fetched
/// separately when needed (hover vs. completion have different
/// appetites). Kept as a plain struct rather than a `'db`-bound handle
/// because the owning data is `Arc<Configuration>` / `PlatformData` —
/// cloning a `Name` + `Ty` is cheaper than threading lifetimes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    /// Method name (in the BSL source language — Russian unless the
    /// platform data or MDO declares an English-only alias).
    pub name: Name,
    /// Return type after lowering through [`TyLoweringContext`].
    /// `Ty::Undefined` for procedures; `Ty::Unknown` for
    /// methods whose return type the platform index doesn't record.
    pub return_ty: Ty,
}

/// Lightweight DTO for a field (MDO attribute, tabular section, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// Field name as declared in the metadata (Russian canonical form
    /// from `Attribute::name` / `TabularSection::name`).
    pub name: Name,
    /// Field type after lowering through [`TyLoweringContext`].
    pub ty: Ty,
}

/// Semantic type handle with IDE-facing queries.
///
/// Pairs a [`Ty`] with the database + file context so enumerators that
/// need visible configurations (MDO attribute list) and the platform
/// index (method list) don't require extra parameters at every call
/// site.
#[derive(Debug)]
pub struct Type<'db, DB> {
    db: &'db DB,
    file_id: FileId,
    ty: Ty,
}

impl<'db, DB: ConfigsDatabase> Type<'db, DB> {
    /// Wrap a raw [`Ty`] in the facade.
    ///
    /// `file_id` anchors the lookup in a specific file's visible
    /// configurations — swapping it changes which extensions' MDOs the
    /// facade sees (matches the Salsa invalidation graph).
    pub fn new(db: &'db DB, file_id: FileId, ty: Ty) -> Self {
        Self { db, file_id, ty }
    }

    /// Underlying [`Ty`] — escape hatch for callers that need the raw
    /// variant (e.g. union narrowing in M4).
    pub fn ty(&self) -> &Ty {
        &self.ty
    }

    /// Short human-readable name, e.g. "Number", "CatalogRef.Номенклатура",
    /// "Number | String" for a union. Delegates to [`Ty::display_name`]
    /// and owns a fresh `String` so callers can format freely.
    pub fn display_name(&self) -> String {
        self.ty.display_name().to_string()
    }

    /// `true` for types that carry an MDO reference — `CatalogRef`,
    /// `DocumentRef`, register refs, etc.
    ///
    /// Does **not** return `true` for `ObjectManager`, `ManagerCollection`,
    /// or `TabularSection`/`TabularSectionRow`; those are manager-side
    /// or container-side abstractions, not first-class references.
    pub fn is_ref_type(&self) -> bool {
        matches!(
            self.ty,
            Ty::MetadataRef {
                kind: MetadataKind::CatalogRef
                    | MetadataKind::DocumentRef
                    | MetadataKind::EnumRef
                    | MetadataKind::TaskRef
                    | MetadataKind::BusinessProcessRef
                    | MetadataKind::InformationRegisterRef
                    | MetadataKind::AccumulationRegisterRef
                    | MetadataKind::AccountingRegisterRef
                    | MetadataKind::CalculationRegisterRef,
                ..
            }
        )
    }

    /// The corresponding manager type — e.g. `CatalogRef.Товары` →
    /// `CatalogManager.Товары` (`Ty::ObjectManager { Catalog, "Товары" }`).
    ///
    /// Returns `None` for:
    /// - non-ref types (primitives, collections, unions);
    /// - register ref variants (`AccumulationRegisterRef` etc.) — those
    ///   don't share the `ObjectManager` shape (their managers are
    ///   `CatalogManager`-style but carry register-specific state).
    ///   Deferred to M4 alongside register field-lookup.
    pub fn manager(&self) -> Option<Self> {
        let (mdo_type, name) = match &self.ty {
            Ty::MetadataRef { kind, name } => {
                let mdo = match kind {
                    MetadataKind::CatalogRef | MetadataKind::CatalogObject => MdoType::Catalog,
                    MetadataKind::DocumentRef | MetadataKind::DocumentObject => MdoType::Document,
                    MetadataKind::EnumRef => MdoType::Enum,
                    MetadataKind::TaskRef => MdoType::Task,
                    MetadataKind::BusinessProcessRef => MdoType::BusinessProcess,
                    // Register variants have their own manager shape; M3
                    // doesn't model them. TabularSection kinds carry
                    // "Parent.Section" names and don't map to a single
                    // MDO manager.
                    _ => return None,
                };
                (mdo, name.clone())
            }
            _ => return None,
        };

        Some(Self::new(self.db, self.file_id, Ty::ObjectManager { kind: mdo_type, name }))
    }

    /// Resolve a method call `x.method_name(...)` to its return type.
    ///
    /// Thin bridge over [`lookup_method`] — adds no cache, so Salsa's
    /// `PlatformData::instance` (used by the adapter) controls caching
    /// at the platform-data layer.
    pub fn method_return_type(&self, method_name: &Name) -> Self {
        let ty =
            lookup_method(&self.ty, method_name).map(|info| info.return_ty).unwrap_or(Ty::Unknown);
        Self::new(self.db, self.file_id, ty)
    }

    /// Resolve a field access `x.field_name` to its type.
    ///
    /// Reads `db.configurations(file_id)` through the Salsa graph, so
    /// hover / completion on attributes correctly invalidate when the
    /// MDO's XML changes.
    pub fn field_type(&self, field_name: &Name) -> Self {
        let configs = self.db.configurations(self.file_id);
        let ty =
            lookup_field(&configs, &self.ty, field_name).map(|info| info.ty).unwrap_or(Ty::Unknown);
        Self::new(self.db, self.file_id, ty)
    }

    /// Enumerate methods callable on the receiver.
    ///
    /// Returns `Vec` (not iterator — matches the `Module::procedures`
    /// style). Sources:
    ///
    /// - Value-type platform objects (`Array`, `ValueTable`, …) and
    ///   bare `Ty::PlatformObject(name)` receivers pull from
    ///   [`PlatformData::get_type_methods`].
    /// - Managers / refs / primitives return an empty vec at M3 —
    ///   their method tables (predefined / manager methods) are covered
    ///   by the M4 adapter.
    pub fn methods(&self) -> Vec<Method> {
        let Some(type_key) = platform_type_key(&self.ty) else {
            return Vec::new();
        };
        PlatformData::instance()
            .get_type_methods(type_key)
            .into_iter()
            .map(method_dto_from_platform)
            .collect()
    }

    /// Enumerate fields on the receiver — MDO attributes + tabular
    /// sections for `MetadataRef`, nothing for other types at M3.
    ///
    /// Tabular-row receivers return their section's columns; tabular
    /// sections as a whole return an empty vec (a section's "fields"
    /// are actually row-level accesses — use `.Строки[0].X` or the
    /// promoted `TabularSectionRow` receiver).
    pub fn fields(&self) -> Vec<Field> {
        match &self.ty {
            Ty::MetadataRef { kind, name } => self.enumerate_metadata_ref_fields(*kind, name),
            _ => Vec::new(),
        }
    }

    fn enumerate_metadata_ref_fields(&self, kind: MetadataKind, mdo_name: &Name) -> Vec<Field> {
        let configs = self.db.configurations(self.file_id);
        let ctx = TyLoweringContext::new();

        if let Some(mdo_type) = mdo_type_for_kind(kind) {
            // Attributes (custom + standard, since the XML loader folds
            // standard attrs into `mdo.attributes`) plus tabular-section
            // promotions. `configs` iterates main-first, extensions-last;
            // reverse so extensions override main on name collisions.
            for cfg in configs.iter().rev() {
                if let Some(mdo) =
                    cfg.configuration.find_metadata_object(mdo_type, mdo_name.as_str())
                {
                    let mut out =
                        Vec::with_capacity(mdo.attributes.len() + mdo.tabular_sections.len());
                    for attr in &mdo.attributes {
                        out.push(Field {
                            name: Name::new(&attr.name),
                            ty: lower_attribute_type(&attr.attr_type, &ctx),
                        });
                    }
                    for ts in &mdo.tabular_sections {
                        let qualified = Name::new(&format!("{}.{}", mdo_name.as_str(), ts.name()));
                        out.push(Field {
                            name: Name::new(ts.name()),
                            ty: Ty::MetadataRef {
                                kind: MetadataKind::TabularSection { parent: mdo_type },
                                name: qualified,
                            },
                        });
                    }
                    return out;
                }
            }
            return Vec::new();
        }

        if let MetadataKind::TabularSectionRow { parent } = kind {
            let Some((parent_name, section_name)) = split_parent_section(mdo_name.as_str()) else {
                return Vec::new();
            };
            for cfg in configs.iter().rev() {
                if let Some(mdo) = cfg.configuration.find_metadata_object(parent, parent_name) {
                    if let Some(ts) = mdo.find_tabular_section(section_name) {
                        return ts
                            .attributes()
                            .iter()
                            .map(|attr| Field {
                                name: Name::new(attr.name()),
                                ty: lower_attribute_type(attr.attr_type(), &ctx),
                            })
                            .collect();
                    }
                    return Vec::new();
                }
            }
        }

        Vec::new()
    }
}

/// Pick the `PlatformData` key for a receiver, matching
/// `method_lookup::platform_type_key`. Returning the same keys here
/// keeps `.methods()` and `.method_return_type()` consistent.
fn platform_type_key(ty: &Ty) -> Option<&str> {
    match ty {
        Ty::Array => Some("Array"),
        Ty::Structure => Some("Structure"),
        Ty::Map => Some("Map"),
        Ty::ValueTable => Some("ValueTable"),
        Ty::ValueList => Some("ValueList"),
        Ty::Type => Some("Type"),
        Ty::PlatformObject(name) => Some(name.as_str()),
        _ => None,
    }
}

/// Convert a `PlatformMethod` into the facade's `Method` DTO.
fn method_dto_from_platform(method: &PlatformMethod) -> Method {
    let return_ty = method
        .return_type
        .as_ref()
        .map(|ret| resolve_platform_type_name(ret))
        .unwrap_or(Ty::Undefined);
    Method { name: Name::new(method.name.as_str()), return_ty }
}

/// Same fallback logic `method_lookup::resolve_platform_type_name` uses
/// — primitives / collections via `Ty::from_type_name`, everything else
/// becomes `Ty::PlatformObject(name)` so fluent chains survive.
fn resolve_platform_type_name(name: &str) -> Ty {
    let ty = Ty::from_type_name(name);
    if ty.is_unknown() {
        Ty::PlatformObject(Name::new(name))
    } else {
        ty
    }
}

/// Parity helper with `field_lookup::mdo_type_for_kind`.
fn mdo_type_for_kind(kind: MetadataKind) -> Option<MdoType> {
    match kind {
        MetadataKind::CatalogRef | MetadataKind::CatalogObject => Some(MdoType::Catalog),
        MetadataKind::DocumentRef | MetadataKind::DocumentObject => Some(MdoType::Document),
        MetadataKind::EnumRef => Some(MdoType::Enum),
        MetadataKind::TaskRef => Some(MdoType::Task),
        MetadataKind::BusinessProcessRef => Some(MdoType::BusinessProcess),
        _ => None,
    }
}

fn lower_attribute_type(attr_type: &AttributeType, ctx: &TyLoweringContext) -> Ty {
    ctx.lower_type_ref(&TypeRef::from_attribute_type(attr_type))
}

fn split_parent_section(name: &str) -> Option<(&str, &str)> {
    let (parent, section) = name.split_once('.')?;
    if parent.is_empty() || section.is_empty() {
        return None;
    }
    Some((parent, section))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;

    fn empty_db() -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);
        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, "");
        (db, file_id)
    }

    #[test]
    fn display_name_matches_ty() {
        // Facade's `display_name` owns a String but must equal the
        // underlying `Ty::display_name` (`&str`) — pins the passthrough.
        let (db, file_id) = empty_db();
        let t = Type::new(&db, file_id, Ty::Number);
        assert_eq!(t.display_name(), Ty::Number.display_name());
    }

    #[test]
    fn is_ref_type_true_for_metadata_refs() {
        // Every `MetadataKind::*Ref` variant that carries an MDO ref
        // must report as a ref type; non-ref MetadataKinds
        // (`CatalogObject`, `TabularSection`) must not.
        let (db, file_id) = empty_db();
        let catalog = Type::new(
            &db,
            file_id,
            Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("X") },
        );
        assert!(catalog.is_ref_type());

        let catalog_obj = Type::new(
            &db,
            file_id,
            Ty::MetadataRef { kind: MetadataKind::CatalogObject, name: Name::new("X") },
        );
        assert!(!catalog_obj.is_ref_type(), "CatalogObject is not a ref type (it is an object)");

        let row = Type::new(
            &db,
            file_id,
            Ty::MetadataRef {
                kind: MetadataKind::TabularSectionRow { parent: MdoType::Document },
                name: Name::new("X.Section"),
            },
        );
        assert!(!row.is_ref_type());

        assert!(!Type::new(&db, file_id, Ty::Number).is_ref_type());
    }

    #[test]
    fn manager_from_ref_types() {
        // CatalogRef.X → ObjectManager(Catalog, "X"). Proves the
        // kind-to-MdoType translation and the name carry-over.
        let (db, file_id) = empty_db();
        let cat = Type::new(
            &db,
            file_id,
            Ty::MetadataRef {
                kind: MetadataKind::CatalogRef, name: Name::new("Номенклатура")
            },
        );
        let manager = cat.manager().expect("CatalogRef has a manager form");
        match manager.ty() {
            Ty::ObjectManager { kind, name } => {
                assert_eq!(*kind, MdoType::Catalog);
                assert_eq!(name.as_str(), "Номенклатура");
            }
            other => panic!("expected ObjectManager, got {other:?}"),
        }
    }

    #[test]
    fn manager_none_for_non_refs() {
        // Non-ref receivers (primitives, collections, registers) return
        // None. Pins the fall-through branch.
        let (db, file_id) = empty_db();
        assert!(Type::new(&db, file_id, Ty::Number).manager().is_none());
        assert!(Type::new(&db, file_id, Ty::Array).manager().is_none());
        let reg = Type::new(
            &db,
            file_id,
            Ty::MetadataRef { kind: MetadataKind::AccumulationRegisterRef, name: Name::new("X") },
        );
        assert!(reg.manager().is_none(), "register ref does not yet expose manager");
    }

    #[test]
    fn method_return_type_on_array_is_known() {
        // `Ty::Array.Добавить` is a well-known platform method that
        // lives in PlatformData under type_name "Array". Smoke-tests
        // the full pipeline `lookup_method → return_ty → Self::new`.
        let (db, file_id) = empty_db();
        let arr = Type::new(&db, file_id, Ty::Array);
        let ret = arr.method_return_type(&Name::new("Добавить"));
        // `Добавить` is a procedure — `lookup_method` returns
        // `Ty::Undefined`.
        assert_eq!(ret.ty(), &Ty::Undefined);
    }

    #[test]
    fn method_return_type_unknown_for_missing() {
        // Missing method → Unknown (no fabrication of a non-existent
        // return type).
        let (db, file_id) = empty_db();
        let arr = Type::new(&db, file_id, Ty::Array);
        let ret = arr.method_return_type(&Name::new("НеСуществует"));
        assert_eq!(ret.ty(), &Ty::Unknown);
    }

    #[test]
    fn methods_lists_platform_methods_for_array() {
        // `Array` must expose at least `Добавить` in its method list.
        // Wrapped with `.iter().any(...)` so the test doesn't need to
        // know the exact count — platform data may grow.
        let (db, file_id) = empty_db();
        let arr = Type::new(&db, file_id, Ty::Array);
        let methods = arr.methods();
        assert!(!methods.is_empty(), "Ty::Array must expose at least one platform method");
        assert!(
            methods.iter().any(|m| m.name.as_str() == "Добавить"),
            "Ty::Array methods must include Добавить — got {:?}",
            methods.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn methods_empty_for_ref_types_at_m3() {
        // Ref / register / tabular types return an empty method list at
        // M3 — manager methods are the M4 adapter's job.
        let (db, file_id) = empty_db();
        let cat = Type::new(
            &db,
            file_id,
            Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("X") },
        );
        assert!(cat.methods().is_empty());
        assert!(Type::new(&db, file_id, Ty::Number).methods().is_empty());
    }

    #[test]
    fn fields_empty_without_configuration() {
        // Without a registered Configuration, `db.configurations()`
        // returns an empty Vec (or a single empty Configuration),
        // so `fields()` returns an empty list rather than panicking.
        // Pins the deferred-gap contract.
        let (db, file_id) = empty_db();
        let cat = Type::new(
            &db,
            file_id,
            Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("X") },
        );
        assert!(cat.fields().is_empty());
    }
}
