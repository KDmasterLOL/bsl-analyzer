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
//! - `is_nullable` — `Null` / `Undefined` are separate `Ty` variants,
//!   not a modifier on other types. No dedicated method needed.

use bsl_metadata::{AttributeType, MdoType};
use bsl_platform::{PlatformData, PlatformMethod};
use hir_def::configs::ConfigsDatabase;
use hir_def::ty::{MetadataKind, Ty};
use hir_def::type_ref::TypeRef;
use hir_def::Name;
use hir_ty::lower::TyLoweringContext;
use hir_ty::{
    coerce_this_object_to_metadata_ref, is_assignable, is_ref_ty, lookup_field, lookup_method,
};
use std::collections::HashSet;
use vfs::FileId;

/// Lightweight DTO for a method exposed by a [`Type`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    /// Russian method name.
    pub name: Name,
    /// English method name.
    pub english_name: Name,
    /// `None` for procedures.
    pub return_ty: Option<Ty>,
    /// Method parameters in declaration order.
    pub params: Vec<MethodParam>,
}

/// Lightweight DTO for a method parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodParam {
    pub name: Name,
    pub ty: Option<Ty>,
    pub optional: bool,
}

/// Lightweight DTO for a field (MDO attribute, tabular section, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// Field name as declared in the metadata (Russian canonical form
    /// from `Attribute::name` / `TabularSection::name`).
    pub name: Name,
    /// English name from `name_en`, falling back to `name` when the
    /// metadata does not declare a separate English alias.
    pub english_name: Name,
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
        is_ref_ty(&self.ty)
    }

    /// Structural assignability: is `self` usable where `other` is expected?
    ///
    /// Task 7 rules (M4_PLAN.md):
    ///
    /// - Reflexivity: `A ≤ A`.
    /// - Gradual top/bottom: `A ≤ Unknown` and `Unknown ≤ A` — neither
    ///   side constraints the check when we don't know one of the types.
    ///   This keeps `TypeMismatch` silent on code paths we can't type.
    /// - `Null ≤ ref-type` (any `MetadataRef { kind: *Ref, .. }`).
    /// - `A ≤ Union(…, X, …)` iff `A ≤ X` for some `X` in the union.
    /// - `Union(A, B) ≤ T` iff `A ≤ T ∧ B ≤ T` (distributes on the left).
    /// - `ThisObject{(k, n)} ≤ MetadataRef{*Object matching k, n}`: a
    ///   **one-way** coercion — `ЭтотОбъект` passes where
    ///   `CatalogObject.Товары` is expected, but the reverse is
    ///   rejected so the `ThisObject` variant's "explicitly
    ///   self-referential" provenance signal (used by
    ///   `BodyDiagnostic::RedundantAccessToObject` and future rename /
    ///   refactor features) stays meaningful.
    ///
    /// ## Narrowing
    ///
    /// The method takes a plain [`Type`] — not a syntax node — so it
    /// compares whatever [`Ty`] the caller already narrowed. Callers
    /// that want the narrowed type at a specific expression should
    /// build the [`Type`] from [`Semantics::type_of_expr`], which
    /// already overlays the [`NarrowState`] produced by
    /// [`HirDatabase::narrow`] (Task 6.6). Calling `is_assignable_to`
    /// on the base (pre-narrow) [`Ty`] is legal but less precise.
    ///
    /// [`Semantics::type_of_expr`]: crate::Semantics::type_of_expr
    /// [`NarrowState`]: hir_ty::narrow::NarrowState
    /// [`HirDatabase::narrow`]: hir_ty::HirDatabase::narrow
    pub fn is_assignable_to(&self, other: &Self) -> bool {
        is_assignable(&self.ty, &other.ty)
    }

    /// The corresponding manager type — e.g. `CatalogRef.Товары` →
    /// `CatalogManager.Товары` (`Ty::ObjectManager { Catalog, "Товары" }`).
    ///
    /// Operates as a coarse `MetadataKind → MdoType` family projection:
    /// both `*Ref` and `*Object` variants resolve to the same manager
    /// because a catalog object's manager hop is the same catalog manager
    /// as the ref's. Returns `None` only for types without an MDO family
    /// (primitives, collections, unions, platform objects, tabular
    /// sections / rows, register record-set / record-manager receivers).
    pub fn manager(&self) -> Option<Self> {
        let (mdo_type, name) = match &self.ty {
            Ty::MetadataRef { kind, name } => {
                let mdo = match kind {
                    MetadataKind::CatalogRef | MetadataKind::CatalogObject => MdoType::Catalog,
                    MetadataKind::DocumentRef | MetadataKind::DocumentObject => MdoType::Document,
                    MetadataKind::EnumRef => MdoType::Enum,
                    MetadataKind::TaskRef => MdoType::Task,
                    MetadataKind::BusinessProcessRef => MdoType::BusinessProcess,
                    MetadataKind::ExchangePlanRef | MetadataKind::ExchangePlanObject => {
                        MdoType::ExchangePlan
                    }
                    MetadataKind::ChartOfAccountsRef | MetadataKind::ChartOfAccountsObject => {
                        MdoType::ChartOfAccounts
                    }
                    MetadataKind::InformationRegisterRef => MdoType::InformationRegister,
                    MetadataKind::AccumulationRegisterRef => MdoType::AccumulationRegister,
                    MetadataKind::AccountingRegisterRef => MdoType::AccountingRegister,
                    MetadataKind::CalculationRegisterRef => MdoType::CalculationRegister,
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
        // `Ty::ThisObject` is coerced to its matching `*Object`
        // `MetadataRef` so IDE callers that hover / complete on
        // `ЭтотОбъект.` see the same attribute / tabular-section list
        // as on the explicit object reference. Non-coercible owner
        // kinds (forms, record sets, …) fall through to the empty
        // default below.
        let coerced = coerce_this_object_to_metadata_ref(&self.ty);
        let ty = coerced.as_ref().unwrap_or(&self.ty);
        match ty {
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
                    let mut seen_names = HashSet::with_capacity(out.capacity() * 2);
                    for attr in &mdo.attributes {
                        push_unique_field(
                            &mut out,
                            &mut seen_names,
                            Field {
                                name: Name::new(&attr.name),
                                english_name: metadata_english_name(
                                    attr.name_en.as_deref(),
                                    attr.name.as_str(),
                                ),
                                ty: lower_attribute_type(&attr.attr_type, &ctx),
                            },
                        );
                    }
                    for ts in &mdo.tabular_sections {
                        let qualified = Name::new(&format!("{}.{}", mdo_name.as_str(), ts.name()));
                        push_unique_field(
                            &mut out,
                            &mut seen_names,
                            Field {
                                name: Name::new(ts.name()),
                                english_name: metadata_english_name(ts.name_en(), ts.name()),
                                ty: Ty::MetadataRef {
                                    kind: MetadataKind::TabularSection { parent: mdo_type },
                                    name: qualified,
                                },
                            },
                        );
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
                                english_name: metadata_english_name(attr.name_en(), attr.name()),
                                ty: lower_attribute_type(attr.attr_type(), &ctx),
                            })
                            .collect();
                    }
                    return Vec::new();
                }
            }
        }

        if let Some(parent) = register_parent_for_kind(kind) {
            // Same extensions-override-main iteration order as the MDO
            // branch; register parts are kept flat in the returned vec
            // (dimensions + resources + attributes) so completion sees
            // one unified field list, matching how `FieldLookup` walks
            // them in order.
            for cfg in configs.iter().rev() {
                if let Some(register) =
                    cfg.configuration.find_register_by_type_and_name(parent, mdo_name.as_str())
                {
                    let mut out = Vec::with_capacity(
                        register.dimensions().len()
                            + register.resources().len()
                            + register.attributes().len(),
                    );
                    let mut seen_names = HashSet::with_capacity(out.capacity() * 2);
                    for dim in register.dimensions() {
                        push_unique_field(
                            &mut out,
                            &mut seen_names,
                            Field {
                                name: Name::new(dim.name()),
                                english_name: Name::new(dim.name()),
                                ty: register_part_ty_for_facade(
                                    dim.attr_type(),
                                    MetadataKind::RegisterDimension { parent },
                                    mdo_name,
                                    dim.name(),
                                    &ctx,
                                ),
                            },
                        );
                    }
                    for res in register.resources() {
                        push_unique_field(
                            &mut out,
                            &mut seen_names,
                            Field {
                                name: Name::new(res.name()),
                                english_name: metadata_english_name(res.name_en(), res.name()),
                                ty: register_part_ty_for_facade(
                                    res.attr_type(),
                                    MetadataKind::RegisterResource { parent },
                                    mdo_name,
                                    res.name(),
                                    &ctx,
                                ),
                            },
                        );
                    }
                    for attr in register.attributes() {
                        push_unique_field(
                            &mut out,
                            &mut seen_names,
                            Field {
                                name: Name::new(attr.name()),
                                english_name: metadata_english_name(attr.name_en(), attr.name()),
                                ty: register_part_ty_for_facade(
                                    attr.attr_type(),
                                    MetadataKind::RegisterAttribute { parent },
                                    mdo_name,
                                    attr.name(),
                                    &ctx,
                                ),
                            },
                        );
                    }
                    return out;
                }
            }
            return Vec::new();
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
    let params = method
        .parameters
        .iter()
        .map(|param| MethodParam {
            name: Name::new(param.name.as_str()),
            ty: param.param_type.as_ref().map(|ty| resolve_platform_type_name(ty)),
            optional: param.is_optional,
        })
        .collect();
    Method {
        name: Name::new(method.name.as_str()),
        english_name: fallback_name(method.english_name.as_str(), method.name.as_str()),
        return_ty: method.return_type.as_ref().map(|ret| resolve_platform_type_name(ret)),
        params,
    }
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
        MetadataKind::ExchangePlanRef | MetadataKind::ExchangePlanObject => {
            Some(MdoType::ExchangePlan)
        }
        MetadataKind::ChartOfAccountsRef | MetadataKind::ChartOfAccountsObject => {
            Some(MdoType::ChartOfAccounts)
        }
        MetadataKind::InformationRegisterRecordManager
        | MetadataKind::InformationRegisterRef
        | MetadataKind::AccumulationRegisterRecordSet
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::CalculationRegisterRef
        | MetadataKind::RegisterDimension { .. }
        | MetadataKind::RegisterResource { .. }
        | MetadataKind::RegisterAttribute { .. }
        | MetadataKind::TabularSection { .. }
        | MetadataKind::TabularSectionRow { .. } => None,
    }
}

fn lower_attribute_type(attr_type: &AttributeType, ctx: &TyLoweringContext) -> Ty {
    ctx.lower_type_ref(&TypeRef::from_attribute_type(attr_type))
}

/// Parity helper with `field_lookup::register_parent_for_kind` — maps the
/// six register receiver kinds (RecordManager / RecordSet / *Ref) to
/// their register flavour [`MdoType`]. Leaf part kinds
/// (`RegisterDimension` / `RegisterResource` / `RegisterAttribute`) are
/// deliberately excluded: they already carry their `parent` explicitly
/// and have no field surface to enumerate.
fn register_parent_for_kind(kind: MetadataKind) -> Option<MdoType> {
    match kind {
        MetadataKind::InformationRegisterRecordManager | MetadataKind::InformationRegisterRef => {
            Some(MdoType::InformationRegister)
        }
        MetadataKind::AccumulationRegisterRecordSet | MetadataKind::AccumulationRegisterRef => {
            Some(MdoType::AccumulationRegister)
        }
        MetadataKind::AccountingRegisterRef => Some(MdoType::AccountingRegister),
        MetadataKind::CalculationRegisterRef => Some(MdoType::CalculationRegister),
        _ => None,
    }
}

/// Parity with `field_lookup::register_part_ty`: lower a register-part
/// type or fall back to the symbolic `Register{Dimension,Resource,
/// Attribute}` variant when `attr_type` is absent.
fn register_part_ty_for_facade(
    attr_type: Option<&AttributeType>,
    fallback_kind: MetadataKind,
    register_name: &Name,
    part_name: &str,
    ctx: &TyLoweringContext,
) -> Ty {
    match attr_type {
        Some(at) => lower_attribute_type(at, ctx),
        None => Ty::MetadataRef {
            kind: fallback_kind,
            name: Name::new(&format!("{}.{}", register_name.as_str(), part_name)),
        },
    }
}

fn split_parent_section(name: &str) -> Option<(&str, &str)> {
    let (parent, section) = name.split_once('.')?;
    if parent.is_empty() || section.is_empty() {
        return None;
    }
    Some((parent, section))
}

fn metadata_english_name(english_name: Option<&str>, fallback: &str) -> Name {
    fallback_name(english_name.unwrap_or_default(), fallback)
}

fn fallback_name(name: &str, fallback: &str) -> Name {
    if name.is_empty() {
        Name::new(fallback)
    } else {
        Name::new(name)
    }
}

fn push_unique_field(out: &mut Vec<Field>, seen_names: &mut HashSet<Name>, field: Field) {
    if seen_names.contains(&field.name) || seen_names.contains(&field.english_name) {
        return;
    }
    seen_names.insert(field.name.clone());
    seen_names.insert(field.english_name.clone());
    out.push(field);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn designer_fixture_path() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
    }

    fn db_with_configuration(config_path: PathBuf) -> (RootDatabaseImpl, FileId) {
        let (mut db, file_id) = empty_db();
        db.set_all_config_paths(vec![(None, config_path)]);
        (db, file_id)
    }

    fn copy_dir_all(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).expect("create temp fixture dir");
        for entry in fs::read_dir(src).expect("read fixture dir") {
            let entry = entry.expect("read fixture entry");
            let ty = entry.file_type().expect("fixture entry type");
            let dst_path = dst.join(entry.file_name());
            if ty.is_dir() {
                copy_dir_all(&entry.path(), &dst_path);
            } else {
                fs::copy(entry.path(), dst_path).expect("copy fixture file");
            }
        }
    }

    /// RAII fixture that clones the designer tree into a per-test temp
    /// directory, tweaks the XML to create a name collision between a
    /// custom attribute and a tabular section, and removes the tree on
    /// drop. Without the `Drop` impl the previous version leaked
    /// `/tmp/bsl-analyzer-type-facade-*` directories across runs,
    /// especially under CI where test harnesses don't clear `TMPDIR`.
    struct TempFixture {
        path: PathBuf,
    }

    impl TempFixture {
        fn duplicated_field() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("bsl-analyzer-type-facade-{}-{unique}", std::process::id()));
            copy_dir_all(&designer_fixture_path(), &path);

            let catalog_path = path.join("Catalogs/Справочник1.xml");
            let xml = fs::read_to_string(&catalog_path).expect("read copied catalog xml");
            let xml = xml.replacen("<Name>ТабличнаяЧасть1</Name>", "<Name>Реквизит2</Name>", 1);
            fs::write(&catalog_path, xml).expect("write copied catalog xml");

            Self { path }
        }

        fn path(&self) -> PathBuf {
            self.path.clone()
        }
    }

    impl Drop for TempFixture {
        fn drop(&mut self) {
            // Best-effort cleanup — an error here only means a leaked
            // `/tmp` entry, not a test failure, so we intentionally
            // ignore `remove_dir_all`'s `Result`.
            let _ = fs::remove_dir_all(&self.path);
        }
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
        // Non-ref receivers return None. Register refs now map to
        // register managers, so keep one real non-ref and one
        // collection receiver to pin the fall-through branch.
        let (db, file_id) = empty_db();
        assert!(Type::new(&db, file_id, Ty::Number).manager().is_none());
        assert!(Type::new(&db, file_id, Ty::Array).manager().is_none());
    }

    #[test]
    fn manager_from_register_ref_types() {
        let (db, file_id) = empty_db();
        let reg = Type::new(
            &db,
            file_id,
            Ty::MetadataRef { kind: MetadataKind::AccumulationRegisterRef, name: Name::new("X") },
        );
        let manager = reg.manager().expect("register ref has a manager form");
        match manager.ty() {
            Ty::ObjectManager { kind, name } => {
                assert_eq!(*kind, MdoType::AccumulationRegister);
                assert_eq!(name.as_str(), "X");
            }
            other => panic!("expected ObjectManager, got {other:?}"),
        }
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

    #[test]
    fn fields_include_custom_attributes_from_configuration() {
        let (db, file_id) = db_with_configuration(designer_fixture_path());
        let cat = Type::new(
            &db,
            file_id,
            Ty::MetadataRef {
                kind: MetadataKind::CatalogRef, name: Name::new("Справочник1")
            },
        );

        let fields = cat.fields();
        let attr = fields
            .iter()
            .find(|field| field.name == Name::new("Реквизит2"))
            .expect("custom attribute must be present");
        assert_eq!(attr.english_name, Name::new("Реквизит2"));
        assert_eq!(attr.ty, Ty::Number);
    }

    #[test]
    fn fields_include_tabular_sections_from_configuration() {
        let (db, file_id) = db_with_configuration(designer_fixture_path());
        let cat = Type::new(
            &db,
            file_id,
            Ty::MetadataRef {
                kind: MetadataKind::CatalogRef, name: Name::new("Справочник1")
            },
        );

        let fields = cat.fields();
        let section = fields
            .iter()
            .find(|field| field.name == Name::new("ТабличнаяЧасть1"))
            .expect("tabular section must be present");
        assert_eq!(section.english_name, Name::new("ТабличнаяЧасть1"));
        assert_eq!(
            section.ty,
            Ty::MetadataRef {
                kind: MetadataKind::TabularSection { parent: MdoType::Catalog },
                name: Name::new("Справочник1.ТабличнаяЧасть1"),
            }
        );
    }

    #[test]
    fn fields_include_register_parts_from_configuration() {
        // Pins the M4 Task 2 register branch in `enumerate_metadata_ref_fields`.
        // The designer fixture's `РегистрСведений1` is an InformationRegister
        // with one dimension `Справочник1: CatalogRef.Справочник1`. The
        // facade's `.fields()` must surface that dimension from
        // `Configuration.registers` with its lowered concrete Ty — mirrors
        // how `FieldLookup::lookup_on_register` resolves the same part
        // but through the completion-surface API instead.
        let (db, file_id) = db_with_configuration(designer_fixture_path());
        let reg = Type::new(
            &db,
            file_id,
            Ty::MetadataRef {
                kind: MetadataKind::InformationRegisterRef,
                name: Name::new("РегистрСведений1"),
            },
        );

        let fields = reg.fields();
        let dim = fields
            .iter()
            .find(|field| field.name == Name::new("Справочник1"))
            .expect("register dimension must appear in .fields()");
        assert_eq!(
            dim.ty,
            Ty::MetadataRef {
                kind: MetadataKind::CatalogRef, name: Name::new("Справочник1")
            },
            "typed dimension must lower through TyLoweringContext, not fall back to symbolic",
        );
    }

    // --- is_assignable_to (Task 7) --------------------------------

    fn t(db: &RootDatabaseImpl, file_id: FileId, ty: Ty) -> Type<'_, RootDatabaseImpl> {
        Type::new(db, file_id, ty)
    }

    #[test]
    fn is_assignable_reflexive_on_primitives() {
        // `A ≤ A` — the most basic rule. Pins that reflexivity works
        // for primitives where `Ty` implements `PartialEq` trivially.
        let (db, file_id) = empty_db();
        assert!(t(&db, file_id, Ty::Number).is_assignable_to(&t(&db, file_id, Ty::Number)));
        assert!(t(&db, file_id, Ty::String).is_assignable_to(&t(&db, file_id, Ty::String)));
        assert!(t(&db, file_id, Ty::Boolean).is_assignable_to(&t(&db, file_id, Ty::Boolean)));
        assert!(!t(&db, file_id, Ty::Number).is_assignable_to(&t(&db, file_id, Ty::String)));
    }

    #[test]
    fn is_assignable_reflexive_on_metadata_ref() {
        // `MetadataRef{kind, name} ≤ MetadataRef{kind, name}` — guards
        // the Name-equality path of the structural comparison. A ref
        // to a different catalog must *not* be assignable.
        let (db, file_id) = empty_db();
        let cat_x = Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("X") };
        let cat_y = Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("Y") };
        assert!(t(&db, file_id, cat_x.clone()).is_assignable_to(&t(&db, file_id, cat_x.clone())));
        assert!(!t(&db, file_id, cat_x).is_assignable_to(&t(&db, file_id, cat_y)));
    }

    #[test]
    fn is_assignable_unknown_is_top_and_bottom() {
        // `A ≤ Unknown` (spec) *and* `Unknown ≤ A` (gradual-typing
        // extension — prevents false `TypeMismatch`es on inferences
        // that bailed out to `Unknown`).
        let (db, file_id) = empty_db();
        assert!(t(&db, file_id, Ty::Number).is_assignable_to(&t(&db, file_id, Ty::Unknown)));
        assert!(t(&db, file_id, Ty::Unknown).is_assignable_to(&t(&db, file_id, Ty::Number)));
        assert!(t(&db, file_id, Ty::Unknown).is_assignable_to(&t(&db, file_id, Ty::Unknown)));
    }

    #[test]
    fn is_assignable_null_to_ref_types_only() {
        // `Null ≤ ref-type` for every MDO ref variant. Must **not**
        // hold for `CatalogObject` (object, not ref) or for non-MDO
        // primitives.
        let (db, file_id) = empty_db();
        let null = t(&db, file_id, Ty::Null);
        for kind in [
            MetadataKind::CatalogRef,
            MetadataKind::DocumentRef,
            MetadataKind::EnumRef,
            MetadataKind::TaskRef,
            MetadataKind::BusinessProcessRef,
            MetadataKind::ExchangePlanRef,
            MetadataKind::ChartOfAccountsRef,
            MetadataKind::InformationRegisterRef,
            MetadataKind::AccumulationRegisterRef,
            MetadataKind::AccountingRegisterRef,
            MetadataKind::CalculationRegisterRef,
        ] {
            let target = t(&db, file_id, Ty::MetadataRef { kind, name: Name::new("X") });
            assert!(null.is_assignable_to(&target), "Null should be assignable to {kind:?}");
        }
        // Objects are not refs — must reject.
        let cat_obj = t(
            &db,
            file_id,
            Ty::MetadataRef { kind: MetadataKind::CatalogObject, name: Name::new("X") },
        );
        assert!(!null.is_assignable_to(&cat_obj));
        assert!(!null.is_assignable_to(&t(&db, file_id, Ty::Number)));
    }

    #[test]
    fn is_assignable_element_to_union_right() {
        // `A ≤ Union(…, A, …)` — the union-right rule. Element lives
        // in the union → assignable; element does not → rejected.
        let (db, file_id) = empty_db();
        let number_or_string = Ty::union(vec![Ty::Number, Ty::String]);
        assert!(t(&db, file_id, Ty::Number).is_assignable_to(&t(
            &db,
            file_id,
            number_or_string.clone()
        )));
        assert!(t(&db, file_id, Ty::String).is_assignable_to(&t(
            &db,
            file_id,
            number_or_string.clone()
        )));
        assert!(!t(&db, file_id, Ty::Boolean).is_assignable_to(&t(&db, file_id, number_or_string)));
    }

    #[test]
    fn is_assignable_union_left_distributes() {
        // `Union(A, B) ≤ T ↔ A ≤ T ∧ B ≤ T`. `Union(Number, String) ≤
        // Number` must fail because String is not a Number; but
        // `Union(Number, Number)` collapses to `Number` (smart
        // constructor), so test with two genuinely distinct types.
        let (db, file_id) = empty_db();
        let ns = Ty::union(vec![Ty::Number, Ty::String]);
        // Neither `Number` nor `String` alone covers the whole union.
        assert!(!t(&db, file_id, ns.clone()).is_assignable_to(&t(&db, file_id, Ty::Number)));
        assert!(!t(&db, file_id, ns.clone()).is_assignable_to(&t(&db, file_id, Ty::String)));

        // `Union(A, B) ≤ Union(A, B)` (reflexivity after `Ty::union`
        // normalisation) — and `Union(Number, String) ≤
        // Union(Number, String, Boolean)` via every component matching
        // some component of the target.
        let nsb = Ty::union(vec![Ty::Number, Ty::String, Ty::Boolean]);
        assert!(t(&db, file_id, ns.clone()).is_assignable_to(&t(&db, file_id, ns.clone())));
        assert!(t(&db, file_id, ns).is_assignable_to(&t(&db, file_id, nsb)));
    }

    #[test]
    fn is_assignable_this_object_coerces_to_metadata_ref() {
        // `ЭтотОбъект` in a catalog module must pass where
        // `CatalogObject.Товары` is expected. The **reverse** direction
        // is deliberately rejected — `Ty::ThisObject` is a provenance-
        // preserving variant (used by `BodyDiagnostic::RedundantAccessToObject`
        // etc.), so an arbitrary `CatalogObject.X` must not satisfy a
        // `ЭтотОбъект` slot.
        let (db, file_id) = empty_db();
        let this_cat = Ty::ThisObject { owner: (MdoType::Catalog, Name::new("Товары")) };
        let cat_object =
            Ty::MetadataRef { kind: MetadataKind::CatalogObject, name: Name::new("Товары") };
        assert!(t(&db, file_id, this_cat.clone()).is_assignable_to(&t(
            &db,
            file_id,
            cat_object.clone()
        )));
        assert!(
            !t(&db, file_id, cat_object).is_assignable_to(&t(&db, file_id, this_cat.clone())),
            "reverse *Object → ThisObject direction must be rejected — preserves provenance"
        );

        // Mismatched owner must still fail even in the accepted direction.
        let cat_other = Ty::MetadataRef {
            kind: MetadataKind::CatalogObject,
            name: Name::new("Номенклатура"),
        };
        assert!(!t(&db, file_id, this_cat).is_assignable_to(&t(&db, file_id, cat_other)));
    }

    #[test]
    fn is_assignable_concrete_ref_to_union_of_refs() {
        // Composition: union-right on two distinct concrete refs.
        // `CatalogRef.Товары ≤ Union(CatalogRef.Товары, DocumentRef.Заказ)`
        // must hold; a third ref absent from the union must fail.
        let (db, file_id) = empty_db();
        let cat_t =
            Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("Товары") };
        let doc_z =
            Ty::MetadataRef { kind: MetadataKind::DocumentRef, name: Name::new("Заказ") };
        let cat_o = Ty::MetadataRef {
            kind: MetadataKind::CatalogRef,
            name: Name::new("Номенклатура"),
        };
        let target = Ty::union(vec![cat_t.clone(), doc_z.clone()]);

        assert!(t(&db, file_id, cat_t).is_assignable_to(&t(&db, file_id, target.clone())));
        assert!(t(&db, file_id, doc_z).is_assignable_to(&t(&db, file_id, target.clone())));
        assert!(
            !t(&db, file_id, cat_o).is_assignable_to(&t(&db, file_id, target)),
            "concrete ref not present in union must be rejected"
        );
    }

    #[test]
    fn is_assignable_null_to_union_containing_ref() {
        // Composition: `Null` + union-right. `Null ≤ CatalogRef.X` and
        // union-right accepts the `Null`-compatible member.
        let (db, file_id) = empty_db();
        let cat_x = Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("X") };
        let doc_y = Ty::MetadataRef { kind: MetadataKind::DocumentRef, name: Name::new("Y") };
        let target = Ty::union(vec![cat_x, doc_y]);
        assert!(t(&db, file_id, Ty::Null).is_assignable_to(&t(&db, file_id, target)));

        // Null into a union with no ref members must fail.
        let ns = Ty::union(vec![Ty::Number, Ty::String]);
        assert!(!t(&db, file_id, Ty::Null).is_assignable_to(&t(&db, file_id, ns)));
    }

    #[test]
    fn is_assignable_union_with_null_left_distributes() {
        // Composition: `Union(Null, CatalogRef.X) ≤ CatalogRef.X` —
        // every component must fit individually. `Null ≤ ref` passes,
        // reflexivity passes → true.
        let (db, file_id) = empty_db();
        let cat_x = Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("X") };
        let nullable_cat = Ty::union(vec![Ty::Null, cat_x.clone()]);
        assert!(t(&db, file_id, nullable_cat).is_assignable_to(&t(&db, file_id, cat_x.clone())));

        // And the key negative composition case Codex flagged:
        // `Union(Null, String) ≤ CatalogRef.X` — `Null ≤ ref` true,
        // `String ≤ CatalogRef.X` false → whole union rejected.
        let null_or_string = Ty::union(vec![Ty::Null, Ty::String]);
        assert!(
            !t(&db, file_id, null_or_string).is_assignable_to(&t(&db, file_id, cat_x)),
            "union-left must reject when any component fails"
        );
    }

    #[test]
    fn is_assignable_unknown_inside_union_is_permissive() {
        // Documents the **gradual-typing** composition: `Ty::union`
        // preserves `Unknown` members (`hir-def/src/ty.rs` smart
        // constructor does not absorb `Unknown`), so any union that
        // acquired an `Unknown` through failed inference will pass all
        // assignment checks. Intentional at M4 Task 7 — revisit when
        // `TypeMismatch` gets a live emitter (see FIXME in
        // `is_assignable`).
        let (db, file_id) = empty_db();
        let unknown_or_string = Ty::union(vec![Ty::Unknown, Ty::String]);
        // Union-left: `Unknown ≤ String` (gradual bottom) + `String ≤
        // String` (reflex) → true.
        assert!(t(&db, file_id, unknown_or_string).is_assignable_to(&t(&db, file_id, Ty::String)));
        // Union-right: `String ≤ Unknown` (gradual top) short-circuits
        // inside the `any`.
        let number_or_unknown = Ty::union(vec![Ty::Number, Ty::Unknown]);
        assert!(t(&db, file_id, Ty::String).is_assignable_to(&t(&db, file_id, number_or_unknown)));
    }

    #[test]
    fn is_assignable_function_reflexive_and_disjoint_primitives_fail() {
        // Facade-level pin for the subtype algorithm's function branch.
        // Reflexive identity → true (covers the common "same signature
        // on both sides" case that a variance implementation must not
        // regress); disjoint primitive params / returns → false because
        // neither variance axis (`String ≤ Number`) holds. Variance
        // itself (contravariant params, covariant return against
        // unions) is unit-tested in `hir_ty::subtype::tests`; this
        // stays narrow so the facade catches any accidental
        // short-circuit before the branch runs.
        let (db, file_id) = empty_db();
        let f_num_to_str =
            Ty::Function { params: vec![Ty::Number].into(), ret: Box::new(Ty::String) };
        let f_num_to_str_2 =
            Ty::Function { params: vec![Ty::Number].into(), ret: Box::new(Ty::String) };
        let f_str_to_str =
            Ty::Function { params: vec![Ty::String].into(), ret: Box::new(Ty::String) };
        let f_num_to_num =
            Ty::Function { params: vec![Ty::Number].into(), ret: Box::new(Ty::Number) };

        assert!(t(&db, file_id, f_num_to_str.clone()).is_assignable_to(&t(
            &db,
            file_id,
            f_num_to_str_2
        )));
        assert!(!t(&db, file_id, f_num_to_str.clone()).is_assignable_to(&t(
            &db,
            file_id,
            f_str_to_str
        )));
        assert!(!t(&db, file_id, f_num_to_str).is_assignable_to(&t(&db, file_id, f_num_to_num)));
    }

    #[test]
    fn is_assignable_function_variance_surfaces_through_facade() {
        // One high-level pin that the variance branch is reachable
        // through `hir::Type::is_assignable_to` (not just the raw
        // `hir_ty::subtype::is_assignable`). Covers the minimum diff a
        // future refactor of `Type::is_assignable_to`'s plumbing could
        // break: contravariant param widening + covariant return
        // narrowing, both in a single function signature. Detailed
        // axis-by-axis assertions live in `hir_ty::subtype::tests`.
        let (db, file_id) = empty_db();
        let from = Ty::Function {
            params: vec![Ty::union(vec![Ty::Number, Ty::String])].into(),
            ret: Box::new(Ty::Number),
        };
        let to = Ty::Function {
            params: vec![Ty::Number].into(),
            ret: Box::new(Ty::union(vec![Ty::Number, Ty::String])),
        };
        assert!(t(&db, file_id, from.clone()).is_assignable_to(&t(&db, file_id, to.clone())));
        assert!(!t(&db, file_id, to).is_assignable_to(&t(&db, file_id, from)));
    }

    #[test]
    fn fields_deduplicate_duplicate_names_preferring_attributes() {
        let fixture = TempFixture::duplicated_field();
        let (db, file_id) = db_with_configuration(fixture.path());
        let cat = Type::new(
            &db,
            file_id,
            Ty::MetadataRef {
                kind: MetadataKind::CatalogRef, name: Name::new("Справочник1")
            },
        );

        let fields = cat.fields();
        let matches: Vec<_> =
            fields.iter().filter(|field| field.name == Name::new("Реквизит2")).collect();
        assert_eq!(matches.len(), 1, "duplicate Russian names must be deduplicated");
        assert_eq!(matches[0].ty, Ty::Number, "attribute must win over tabular section");
    }
}
