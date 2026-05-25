//! `hir::Type` facade.
//!
//! Unified IDE entry point for asking semantic questions about a type:
//! "what methods are callable on this?", "what's the type of this
//! field?", "is this a reference type?". Wraps the type kernel
//! (`TypeId` / `TypeKind`) and
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

use bsl_metadata::MdoType;
use bsl_platform::{PlatformData, PlatformMethod};
use bsl_types::builders::Builders;
use bsl_types::display::{display_name as kernel_display, Locale as KernelLocale, PlainDisplayCtx};
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{Projection, TypeId, TypeKind};
use hir_def::configs::ConfigsDatabase;
use hir_def::ty::MetadataKind;
use hir_def::Name;
use hir_ty::lower::type_string::{lower_param_type_string_typeid, lower_return_type_string_typeid};
use hir_ty::method_lookup::platform_type_key_id;
use hir_ty::{
    enumerate_fields, is_assignable, is_ref_ty, lookup_field, lookup_method, FieldInfo, FieldOrigin,
};
use std::sync::Arc;
use vfs::FileId;

/// Lightweight DTO for a method exposed by a [`Type`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    /// Russian method name.
    pub name: Name,
    /// English method name.
    pub english_name: Name,
    /// `None` for procedures.
    pub return_ty: Option<TypeId>,
    /// Method parameters in declaration order.
    pub params: Vec<MethodParam>,
}

/// Lightweight DTO for a method parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodParam {
    pub name: Name,
    pub ty: Option<TypeId>,
    pub optional: bool,
}

/// Where a field came from — mirrors [`hir_ty::FieldOrigin`].
///
/// Re-exported here so IDE callers (`ide/src/completion/`, hover, etc.) do
/// not need to depend directly on `hir-ty` for this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirFieldOrigin {
    StandardAttribute,
    UserAttribute,
    FormAttribute,
    MainFormAttribute,
    TabularSection,
    TabularSectionRowColumn,
    RegisterDimension,
    RegisterResource,
    RegisterAttribute,
    PlatformProperty,
}

impl From<FieldOrigin> for HirFieldOrigin {
    fn from(o: FieldOrigin) -> Self {
        match o {
            FieldOrigin::StandardAttribute => Self::StandardAttribute,
            FieldOrigin::UserAttribute => Self::UserAttribute,
            FieldOrigin::FormAttribute => Self::FormAttribute,
            FieldOrigin::MainFormAttribute => Self::MainFormAttribute,
            FieldOrigin::TabularSection => Self::TabularSection,
            FieldOrigin::TabularSectionRowColumn => Self::TabularSectionRowColumn,
            FieldOrigin::RegisterDimension => Self::RegisterDimension,
            FieldOrigin::RegisterResource => Self::RegisterResource,
            FieldOrigin::RegisterAttribute => Self::RegisterAttribute,
            FieldOrigin::PlatformProperty => Self::PlatformProperty,
        }
    }
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
    pub ty: TypeId,
    /// Domain value type wrapped by a synthetic/platform accessor.
    pub value_ty: Option<TypeId>,
    /// Whether this field is read-only.
    pub is_readonly: bool,
    /// Where this field originated from.
    pub origin: HirFieldOrigin,
}

/// Semantic type handle with IDE-facing queries.
///
/// Pairs a `TypeId` with the database + file context so enumerators that
/// need visible configurations (MDO attribute list) and the platform
/// index (method list) don't require extra parameters at every call
/// site.
#[derive(Debug)]
pub struct Type<'db, DB> {
    db: &'db DB,
    file_id: FileId,
    id: TypeId,
}

impl<'db, DB: ConfigsDatabase + TypeKernelDb> Type<'db, DB> {
    /// Wrap an interned [`TypeId`] in the facade (kernel-native entry).
    pub fn from_id(db: &'db DB, file_id: FileId, id: TypeId) -> Self {
        Self { db, file_id, id }
    }

    /// The interned type id backing this facade.
    pub fn id(&self) -> TypeId {
        self.id
    }

    /// Borrow the kernel [`TypeKind`] backing this facade.
    pub fn kind(&self) -> &TypeKind {
        self.db.lookup_type(self.id)
    }

    /// Short human-readable name in the given locale, e.g.
    /// `"Number"` / `"Число"`, `"Number | String"` / `"Число | Строка"` for
    /// a union. Phase 3 §4.G.5d: rendered through the type kernel
    /// ([`bsl_types::display`]) — the single source of display truth.
    pub fn display_name(&self, locale: base_db::Locale) -> String {
        kernel_type_label(self.db, self.id, locale, false)
    }

    /// English type label; equivalent to `display_name(Locale::En)`. Use in
    /// tests, logs, and any other context where the English rendering is
    /// intentional rather than locale-dependent.
    ///
    /// Phase 3 §4.G.5d: rendered through the type kernel
    /// ([`bsl_types::display`]); manager-shaped types may differ from the
    /// legacy `Ty::canonical_name` platform machine-names until the Phase 4
    /// manager-display polish lands.
    pub fn canonical_name(&self) -> String {
        kernel_type_label(self.db, self.id, base_db::Locale::En, false)
    }

    /// `true` for types that carry an MDO reference — `CatalogRef`,
    /// `DocumentRef`, register refs, etc.
    ///
    /// Does **not** return `true` for `ObjectManager`, `ManagerCollection`,
    /// or `TabularSection`/`TabularSectionRow`; those are manager-side
    /// or container-side abstractions, not first-class references.
    pub fn is_ref_type(&self) -> bool {
        is_ref_ty(self.db, self.id)
    }

    /// Structural assignability: is `self` usable where `other` is expected?
    ///
    /// Rules:
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
    /// compares whatever `TypeId` the caller already narrowed. Callers
    /// that want the narrowed type at a specific expression should
    /// build the [`Type`] from [`Semantics::type_of_expr`], which
    /// already overlays the [`NarrowState`] produced by
    /// [`HirDatabase::narrow`] (Task 6.6). Calling `is_assignable_to`
    /// on the base (pre-narrow) `TypeId` is legal but less precise.
    ///
    /// [`Semantics::type_of_expr`]: crate::Semantics::type_of_expr
    /// [`NarrowState`]: hir_ty::narrow::NarrowState
    /// [`HirDatabase::narrow`]: hir_ty::HirDatabase::narrow
    pub fn is_assignable_to(&self, other: &Self) -> bool {
        // `TypeId` is db-local, so `self` and `other` must share the same
        // kernel db — every caller builds both facades from one
        // `Semantics`/db, so their ids are directly comparable.
        is_assignable(self.db, self.id, other.id)
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
        // Extract owned facet data before the `object_manager_with_config`
        // builder call — the `&TypeKind` borrow from `kind()` cannot be
        // held across a `db` method that re-borrows the kernel.
        //
        // Both `MetadataRef` and `MetadataObject` carry the same
        // `kind/name/config_id` and must route here: the legacy `Ty` path
        // collapsed `MetadataObject` into `Ty::MetadataRef` before the
        // manager arms, so a native `MetadataObject` receiver reached them.
        let (kind, name, config_id) = match self.kind() {
            TypeKind::MetadataRef(facet) => {
                (facet.kind, facet.name.clone(), facet.config_id.clone())
            }
            TypeKind::MetadataObject(facet) => {
                (facet.kind, facet.name.clone(), facet.config_id.clone())
            }
            _ => return None,
        };
        let mdo_type = match kind {
            MetadataKind::CatalogRef | MetadataKind::CatalogObject => MdoType::Catalog,
            MetadataKind::DocumentRef | MetadataKind::DocumentObject => MdoType::Document,
            MetadataKind::EnumRef => MdoType::Enum,
            // *Object companions of MDOs reach the same Manager
            // (`Обработки.X`, `Отчёты.X`, `БизнесПроцессы.X`,
            // `Задачи.X`) — the form-attribute projection lands
            // on `MetadataRef{*Object}` and the user may then
            // navigate `Объект.Manager`-style paths through the
            // facade. Mapping to MDO here is symmetric with the
            // *Ref / *Object union arms for Catalog / Document.
            MetadataKind::TaskRef | MetadataKind::TaskObject => MdoType::Task,
            MetadataKind::BusinessProcessRef | MetadataKind::BusinessProcessObject => {
                MdoType::BusinessProcess
            }
            MetadataKind::DataProcessorObject => MdoType::DataProcessor,
            MetadataKind::ReportObject => MdoType::Report,
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
            // No-manager kinds: enumerated explicitly (no wildcard)
            // so a new `MetadataKind` variant becomes a compile
            // error here instead of silently returning None.
            // Tabular sections, register parts, and the Filter
            // synthetic don't have a Manager surface; record-set
            // and record-manager kinds reach managers through
            // their parent register, not the kind directly.
            MetadataKind::InformationRegisterRecordManager
            | MetadataKind::InformationRegisterRecordSet
            | MetadataKind::InformationRegisterRecord
            | MetadataKind::AccumulationRegisterRecordSet
            | MetadataKind::AccumulationRegisterRecord
            | MetadataKind::AccountingRegisterRecordSet
            | MetadataKind::AccountingRegisterRecord
            | MetadataKind::CalculationRegisterRecordSet
            | MetadataKind::CalculationRegisterRecord
            | MetadataKind::RegisterDimension { .. }
            | MetadataKind::RegisterResource { .. }
            | MetadataKind::RegisterAttribute { .. }
            | MetadataKind::RegisterFilter { .. }
            | MetadataKind::TabularSection { .. }
            | MetadataKind::TabularSectionRow { .. } => return None,
        };

        let id = self.db.object_manager_with_config(mdo_type, name, config_id);
        Some(Self::from_id(self.db, self.file_id, id))
    }

    /// Resolve a method call `x.method_name(...)` to its return type.
    ///
    /// Thin bridge over [`lookup_method`] — adds no cache, so Salsa's
    /// `PlatformData::instance` (used by the adapter) controls caching
    /// at the platform-data layer.
    pub fn method_return_type(&self, method_name: &Name) -> Self {
        // Phase 3 §4.F: facade + `lookup_method` are both kernel-native,
        // so the receiver id and the returned id flow through without a
        // bridge.
        let id = lookup_method(self.db, self.id, method_name)
            .map(|info| info.return_ty)
            .unwrap_or_else(|| self.db.unknown());
        Self::from_id(self.db, self.file_id, id)
    }

    /// Resolve a field access `x.field_name` to its type.
    ///
    /// Reads `db.configurations(file_id)` through the Salsa graph, so
    /// hover / completion on attributes correctly invalidate when the
    /// MDO's XML changes.
    pub fn field_type(&self, field_name: &Name) -> Self {
        // `lookup_field` takes and returns kernel ids, so the receiver and
        // the resolved `FieldInfo.ty` flow through without bridging.
        let configs = self.db.configurations(self.file_id);
        let id = lookup_field(self.db, &configs, self.id, field_name)
            .map(|info| info.ty)
            .unwrap_or_else(|| self.db.unknown());
        Self::from_id(self.db, self.file_id, id)
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
        // Form-control receivers carry an ordered platform-type chain
        // `[base, extension?]` — merge methods across the chain so
        // extension members (`<UsualGroup>.Скрыть`, …) appear next to
        // shared base methods. Single-entry chains reduce to one
        // platform-data lookup, identical to the pre-chain shape.
        if let TypeKind::FormControl { kind, .. } = self.kind() {
            let chain = hir_def::ty::form_control_platform_type_chain(*kind);
            let mut methods: Vec<Method> = Vec::new();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            // Walk reversed so extension members win on collision —
            // matches `lookup_method` precedence.
            for type_name in chain.iter().rev() {
                for m in PlatformData::instance().get_type_methods(type_name) {
                    let dto = method_dto_from_platform(self.db, m);
                    if seen.insert(dto.name.as_str().to_lowercase()) {
                        methods.push(dto);
                    }
                }
            }
            return methods;
        }

        let Some(type_key) = platform_type_key_id(self.db, self.id) else {
            return Vec::new();
        };
        PlatformData::instance()
            .get_type_methods(&type_key)
            .into_iter()
            .map(|m| method_dto_from_platform(self.db, m))
            .collect()
    }

    /// Enumerate fields on the receiver — MDO attributes + tabular
    /// sections for `MetadataRef`, nothing for other types at M3.
    ///
    /// Tabular-row receivers return their section's columns; tabular
    /// sections as a whole return an empty vec (a section's "fields"
    /// are actually row-level accesses — use `.Строки[0].X` or the
    /// promoted `TabularSectionRow` receiver).
    ///
    /// `Ty::ThisObject` coercion is handled inside [`enumerate_fields`],
    /// so callers do not need to prepare the receiver.
    pub fn fields(&self) -> Vec<Field> {
        let configs = self.db.configurations(self.file_id);
        enumerate_fields(self.db, &configs, self.id)
            .into_iter()
            .map(|info| Field {
                name: info.name.clone(),
                english_name: info.name_en.clone().unwrap_or_else(|| info.name.clone()),
                ty: info.ty,
                value_ty: info.value_ty,
                is_readonly: info.is_readonly,
                origin: HirFieldOrigin::from(info.origin),
            })
            .collect()
    }

    /// Whether this type is a `Ty::QueryResultSelection` carrying a
    /// resolved SDBL projection (a non-`None` payload).
    ///
    /// IDE consumers use this as a cheap discriminator before pulling
    /// the field list — e.g. hover routes through the projection
    /// pretty-printer instead of the generic platform docs only when
    /// this returns `true`.
    pub fn is_query_projection(&self) -> bool {
        self.projection().is_some()
    }

    /// The SDBL projection carried by a `QueryResultSelection` /
    /// `ValueTable` / `ValueTableRow` receiver, if any. Cloned (cheap `Arc`)
    /// so the `kind()` borrow is released for downstream `db` calls.
    fn projection(&self) -> Option<Arc<Projection>> {
        match self.kind() {
            TypeKind::QueryResultSelection(facet) => facet.projection.clone(),
            TypeKind::ValueTable(facet) | TypeKind::ValueTableRow(facet) => {
                facet.projection.clone()
            }
            _ => None,
        }
    }

    /// Per-column `(name, type)` pairs from the SDBL projection.
    ///
    /// `Some(vec)` when [`Type::is_query_projection`] returns `true`;
    /// `None` otherwise. Phase 3 §4.G.5c: each column type is the interned
    /// kernel [`TypeId`] (the SDBL projection stores the interned handles
    /// directly). Callers that need the IDE facade `Type` for a column can
    /// wrap the id via [`Type::from_id`] with the same `db` / `file_id`.
    pub fn projection_fields(&self) -> Option<Vec<(Name, TypeId)>> {
        let p = self.projection()?;
        Some(p.fields.iter().map(|f| (Name::new(f.name.as_str()), f.ty)).collect())
    }

    /// Per-column SDBL display labels (`"Число(15,2)"`, `"Строка(50)"`)
    /// indexed in lock-step with [`Type::projection_fields`].
    ///
    /// `None` when the receiver isn't a projection-typed selection OR
    /// when the projection's `raw_sdbl_types` shadow was not captured
    /// (some bridge entry points skip it). Hover uses these to render
    /// SDBL precision/scale that the bridged `Ty` drops. Each entry is the
    /// pre-rendered SDBL type label (`"Число(15,2)"`, `"Строка(50)"`).
    pub fn projection_field_displays(&self) -> Option<Vec<String>> {
        let p = self.projection()?;
        p.raw_sdbl_types.as_ref().map(|shadows| shadows.iter().map(|s| s.display.clone()).collect())
    }
}

/// Kernel-display label for `id` (Phase 3 §4.G.5d).
///
/// Renders through the type kernel's locale-aware
/// [`bsl_types::display::display_name`] — the single source of display
/// truth. `precision = true` → hover-style (precision / scale / length and
/// projection column shapes shown); `false` → completion-style bare name.
/// IDE features call this at the display boundary instead of the legacy
/// `Ty::display` / `Ty::display_name`.
pub fn kernel_type_label(
    db: &dyn TypeKernelDb,
    id: TypeId,
    locale: base_db::Locale,
    precision: bool,
) -> String {
    let kernel_locale = match locale {
        base_db::Locale::Ru => KernelLocale::Ru,
        base_db::Locale::En => KernelLocale::En,
    };
    let ctx = PlainDisplayCtx { locale: kernel_locale, precision_visible: precision };
    kernel_display(db.lookup_type(id), &ctx, db)
}

/// Project a kernel [`FieldInfo`] into the hir [`Field`] DTO.
/// §4.G.5c: the DTO is kernel-native (`TypeId`), so `ty` / `value_ty`
/// pass through without bridging.
fn field_from_info(info: FieldInfo) -> Field {
    Field {
        name: info.name.clone(),
        english_name: info.name_en.unwrap_or_else(|| info.name.clone()),
        ty: info.ty,
        value_ty: info.value_ty,
        is_readonly: info.is_readonly,
        origin: HirFieldOrigin::from(info.origin),
    }
}

pub fn module_implicit_fields<DB: hir_ty::db::HirDatabase>(db: &DB, file_id: FileId) -> Vec<Field> {
    hir_ty::module_implicit_fields(db, file_id).into_iter().map(field_from_info).collect()
}

/// Convert a `PlatformMethod` into the facade's `Method` DTO.
///
/// Param / return types lower through the unified
/// [`hir_ty::lower::type_string`] pipeline so the DTO stays consistent
/// with what `lookup_method` produces (param-asymmetric gradual typing,
/// `;`-separator-aware unions, `Произвольный` collapse).
fn method_dto_from_platform(db: &dyn TypeKernelDb, method: &PlatformMethod) -> Method {
    let params = method
        .parameters
        .iter()
        .map(|param| MethodParam {
            name: Name::new(param.name.as_str()),
            // Phase 3 §4.A.4: the DTO is kernel-native and so is the lowering —
            // mint the param type directly through the native type-string path.
            ty: param.param_type.as_ref().map(|ty| lower_param_type_string_typeid(db, ty)),
            optional: param.is_optional,
        })
        .collect();
    Method {
        name: Name::new(method.name.as_str()),
        english_name: fallback_name(method.english_name.as_str(), method.name.as_str()),
        return_ty: method.return_type.as_ref().map(|ret| lower_return_type_string_typeid(db, ret)),
        params,
    }
}

fn fallback_name(name: &str, fallback: &str) -> Name {
    if name.is_empty() {
        Name::new(fallback)
    } else {
        Name::new(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_config::ConfigId;
    use bsl_types::facet::{
        ArgArity, FunctionFacet, FunctionOrigin, MdoRefFacet, ParamPassing, ParamSpec,
    };
    use bsl_types::testing::RootConfigCtx;
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

    fn db_at_with_configuration(
        module_path: PathBuf,
        config_path: PathBuf,
    ) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);
        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new(module_path.to_string_lossy().to_string()));
        db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, "");
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

        fn information_register_with_resource() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("bsl-analyzer-type-facade-{}-{unique}", std::process::id()));
            copy_dir_all(&designer_fixture_path(), &path);

            let register_path = path.join("InformationRegisters/РегистрСведений1.xml");
            let xml = fs::read_to_string(&register_path).expect("read copied register xml");
            let resource = r#"
			<Resource uuid="11111111-2222-3333-4444-555555555555">
				<Properties>
					<Name>Количество</Name>
					<Type>
						<v8:Type>xs:decimal</v8:Type>
						<v8:NumberQualifiers>
							<v8:Digits>15</v8:Digits>
							<v8:FractionDigits>3</v8:FractionDigits>
						</v8:NumberQualifiers>
					</Type>
				</Properties>
			</Resource>"#;
            let xml =
                xml.replacen("</ChildObjects>", &format!("{resource}\n\t\t</ChildObjects>"), 1);
            fs::write(&register_path, xml).expect("write copied register xml");

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
        // Also pins the locale propagation: `display_name(Ru)` on the
        // facade must agree with `Ty::display_name(Ru)`.
        use base_db::Locale;
        let (db, file_id) = empty_db();
        let t = t(&db, file_id, db.number(None, None));
        assert_eq!(t.display_name(Locale::En), "Number");
        assert_eq!(t.display_name(Locale::Ru), "Число");
        assert_eq!(t.canonical_name(), "Number");
    }

    #[test]
    fn is_ref_type_true_for_metadata_refs() {
        // Every `MetadataKind::*Ref` variant that carries an MDO ref
        // must report as a ref type; non-ref MetadataKinds
        // (`CatalogObject`, `TabularSection`) must not.
        let (db, file_id) = empty_db();
        let catalog = t(&db, file_id, metadata_ref(&db, MetadataKind::CatalogRef, "X"));
        assert!(catalog.is_ref_type());

        let catalog_obj = t(&db, file_id, metadata_ref(&db, MetadataKind::CatalogObject, "X"));
        assert!(!catalog_obj.is_ref_type(), "CatalogObject is not a ref type (it is an object)");

        let row = t(
            &db,
            file_id,
            metadata_ref(
                &db,
                MetadataKind::TabularSectionRow { parent: MdoType::Document },
                "X.Section",
            ),
        );
        assert!(!row.is_ref_type());

        assert!(!t(&db, file_id, db.number(None, None)).is_ref_type());
    }

    #[test]
    fn manager_from_ref_types() {
        // CatalogRef.X → ObjectManager(Catalog, "X"). Proves the
        // kind-to-MdoType translation and the name carry-over.
        let (db, file_id) = empty_db();
        let cat = t(&db, file_id, metadata_ref(&db, MetadataKind::CatalogRef, "Номенклатура"));
        let manager = cat.manager().expect("CatalogRef has a manager form");
        match db.lookup_type(manager.id()) {
            TypeKind::ObjectManager(facet) => {
                assert_eq!(facet.mdo, MdoType::Catalog);
                assert_eq!(facet.name.as_str(), "Номенклатура");
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
        assert!(t(&db, file_id, db.number(None, None)).manager().is_none());
        assert!(t(&db, file_id, db.array(None)).manager().is_none());
    }

    #[test]
    fn manager_from_register_ref_types() {
        let (db, file_id) = empty_db();
        let reg = t(&db, file_id, metadata_ref(&db, MetadataKind::AccumulationRegisterRef, "X"));
        let manager = reg.manager().expect("register ref has a manager form");
        match db.lookup_type(manager.id()) {
            TypeKind::ObjectManager(facet) => {
                assert_eq!(facet.mdo, MdoType::AccumulationRegister);
                assert_eq!(facet.name.as_str(), "X");
            }
            other => panic!("expected ObjectManager, got {other:?}"),
        }
    }

    #[test]
    fn manager_from_metadata_object_receiver() {
        // A native kernel `MetadataObject` receiver (not reachable through
        // the `Ty` bridge, which collapses it to `MetadataRef`) must still
        // resolve its manager. Regression pin for the §4.E.5a `.kind()`
        // flip, which originally matched only `MetadataRef`.
        use bsl_types::testing::RootConfigCtx;
        let (db, file_id) = empty_db();
        let id = db.metadata_object(
            MetadataKind::CatalogObject,
            "Номенклатура".to_string(),
            &RootConfigCtx,
        );
        let manager = Type::from_id(&db, file_id, id)
            .manager()
            .expect("MetadataObject receiver has a manager form");
        match db.lookup_type(manager.id()) {
            TypeKind::ObjectManager(facet) => {
                assert_eq!(facet.mdo, MdoType::Catalog);
                assert_eq!(facet.name.as_str(), "Номенклатура");
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
        let arr = t(&db, file_id, db.array(None));
        let ret = arr.method_return_type(&Name::new("Добавить"));
        // `Добавить` is a procedure — `lookup_method` returns
        // `Ty::Undefined`.
        assert_eq!(ret.id(), db.undefined());
    }

    #[test]
    fn method_return_type_unknown_for_missing() {
        // Missing method → Unknown (no fabrication of a non-existent
        // return type).
        let (db, file_id) = empty_db();
        let arr = t(&db, file_id, db.array(None));
        let ret = arr.method_return_type(&Name::new("НеСуществует"));
        assert_eq!(ret.id(), db.unknown());
    }

    #[test]
    fn methods_lists_platform_methods_for_array() {
        // `Array` must expose at least `Добавить` in its method list.
        // Wrapped with `.iter().any(...)` so the test doesn't need to
        // know the exact count — platform data may grow.
        let (db, file_id) = empty_db();
        let arr = t(&db, file_id, db.array(None));
        let methods = arr.methods();
        assert!(!methods.is_empty(), "Array must expose at least one platform method");
        assert!(
            methods.iter().any(|m| m.name.as_str() == "Добавить"),
            "Array methods must include Добавить — got {:?}",
            methods.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn methods_empty_for_ref_types_at_m3() {
        // Ref / register / tabular types return an empty method list at
        // M3 — manager methods are the M4 adapter's job.
        let (db, file_id) = empty_db();
        let cat = t(&db, file_id, metadata_ref(&db, MetadataKind::CatalogRef, "X"));
        assert!(cat.methods().is_empty());
        assert!(t(&db, file_id, db.number(None, None)).methods().is_empty());
    }

    #[test]
    fn fields_empty_without_configuration() {
        // Without a registered Configuration, `db.configurations()`
        // returns an empty Vec (or a single empty Configuration),
        // so `fields()` returns an empty list rather than panicking.
        // Pins the deferred-gap contract.
        let (db, file_id) = empty_db();
        let cat = t(&db, file_id, metadata_ref(&db, MetadataKind::CatalogRef, "X"));
        assert!(cat.fields().is_empty());
    }

    #[test]
    fn fields_include_custom_attributes_from_configuration() {
        let (db, file_id) = db_with_configuration(designer_fixture_path());
        let cat = t(&db, file_id, metadata_ref(&db, MetadataKind::CatalogRef, "Справочник1"));

        let fields = cat.fields();
        let attr = fields
            .iter()
            .find(|field| field.name == Name::new("Реквизит2"))
            .expect("custom attribute must be present");
        assert_eq!(attr.english_name, Name::new("Реквизит2"));
        assert_eq!(attr.ty, db.number(None, None));
    }

    #[test]
    fn fields_on_metadata_object_receiver_surface_attributes() {
        // Regression pin (§4.E.5c-completion): a native kernel
        // `MetadataObject` receiver must enumerate the same MDO fields as
        // its `MetadataRef` companion. Completion routes such receivers
        // through `HirType::from_id(...).fields()`; before the
        // `enumerate_fields_inner` MetadataObject arm they came back empty.
        use bsl_types::testing::RootConfigCtx;
        let (db, file_id) = db_with_configuration(designer_fixture_path());
        let id = db.metadata_object(
            MetadataKind::CatalogObject,
            "Справочник1".to_string(),
            &RootConfigCtx,
        );
        let fields = Type::from_id(&db, file_id, id).fields();
        assert!(
            fields.iter().any(|f| f.name == Name::new("Реквизит2")),
            "MetadataObject receiver must surface MDO custom attributes; got {:?}",
            fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn fields_include_tabular_sections_from_configuration() {
        let (db, file_id) = db_with_configuration(designer_fixture_path());
        let cat = t(&db, file_id, metadata_ref(&db, MetadataKind::CatalogRef, "Справочник1"));

        let fields = cat.fields();
        let section = fields
            .iter()
            .find(|field| field.name == Name::new("ТабличнаяЧасть1"))
            .expect("tabular section must be present");
        assert_eq!(section.english_name, Name::new("ТабличнаяЧасть1"));
        assert_eq!(
            section.ty,
            metadata_ref(
                &db,
                MetadataKind::TabularSection { parent: MdoType::Catalog },
                "Справочник1.ТабличнаяЧасть1",
            )
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
        let reg = t(
            &db,
            file_id,
            metadata_ref(&db, MetadataKind::InformationRegisterRef, "РегистрСведений1"),
        );

        let fields = reg.fields();
        let dim = fields
            .iter()
            .find(|field| field.name == Name::new("Справочник1"))
            .expect("register dimension must appear in .fields()");
        assert_eq!(
            dim.ty,
            metadata_ref(&db, MetadataKind::CatalogRef, "Справочник1"),
            "typed dimension must lower through TyLoweringContext, not fall back to symbolic",
        );
    }

    #[test]
    fn module_implicit_fields_object_module_yields_mdo_attributes() {
        let config_path = designer_fixture_path();
        let module_path = config_path.join("DataProcessors/ТестоваяОбработка/Ext/ObjectModule.bsl");
        let (db, file_id) = db_at_with_configuration(module_path, config_path);

        let fields = module_implicit_fields(&db, file_id);
        let attr = fields
            .iter()
            .find(|field| field.name == Name::new("АдресСайта"))
            .expect("object module must expose owner MDO attributes as bare identifiers");

        assert_eq!(attr.ty, db.string(None, false));
    }

    #[test]
    fn module_implicit_fields_record_set_module_yields_dimensions_and_resources() {
        let fixture = TempFixture::information_register_with_resource();
        let config_path = fixture.path();
        let module_path =
            config_path.join("InformationRegisters/РегистрСведений1/Ext/RecordSetModule.bsl");
        let (db, file_id) = db_at_with_configuration(module_path, config_path);

        let fields = module_implicit_fields(&db, file_id);

        assert!(
            fields.iter().any(|field| {
                field.name == Name::new("Справочник1")
                    && field.origin == HirFieldOrigin::RegisterDimension
            }),
            "record-set module must expose register dimensions",
        );
        assert!(
            fields.iter().any(|field| {
                field.name == Name::new("Количество")
                    && field.origin == HirFieldOrigin::RegisterResource
            }),
            "record-set module must expose register resources",
        );
    }

    #[test]
    fn module_implicit_fields_managed_form_yields_form_attributes_with_origin() {
        let config_path = designer_fixture_path();
        let main_module_path =
            config_path.join("DataProcessors/ТестоваяОбработка/Forms/Форма/Ext/Form/Module.bsl");
        let regular_module_path =
            config_path.join("Catalogs/рдт_Рецептура/Forms/ФормаЭлемента/Ext/Form/Module.bsl");

        let (db, file_id) = db_at_with_configuration(main_module_path, config_path.clone());
        let fields = module_implicit_fields(&db, file_id);
        let main = fields
            .iter()
            .find(|field| field.name == Name::new("Объект"))
            .expect("managed form must expose main form attribute");
        assert_eq!(main.origin, HirFieldOrigin::MainFormAttribute);

        let (db, file_id) = db_at_with_configuration(regular_module_path, config_path);
        let fields = module_implicit_fields(&db, file_id);
        let plain = fields
            .iter()
            .find(|field| field.name == Name::new("Пересчитать"))
            .expect("managed form must expose regular form attribute");
        assert_eq!(plain.origin, HirFieldOrigin::FormAttribute);
    }

    #[test]
    fn module_implicit_fields_manager_module_yields_empty() {
        let config_path = designer_fixture_path();
        let module_path = config_path.join("Catalogs/Справочник1/Ext/ManagerModule.bsl");
        let (db, file_id) = db_at_with_configuration(module_path, config_path);

        assert!(module_implicit_fields(&db, file_id).is_empty());
    }

    // --- is_assignable_to (Task 7) --------------------------------

    fn t(db: &RootDatabaseImpl, file_id: FileId, id: TypeId) -> Type<'_, RootDatabaseImpl> {
        Type::from_id(db, file_id, id)
    }

    fn metadata_ref(db: &RootDatabaseImpl, kind: MetadataKind, name: &str) -> TypeId {
        db.metadata_ref(kind, name.to_string(), &RootConfigCtx)
    }

    fn fixed_function(db: &RootDatabaseImpl, params: Vec<TypeId>, returns: TypeId) -> TypeId {
        let params: Arc<[ParamSpec]> = params
            .into_iter()
            .enumerate()
            .map(|(idx, ty)| ParamSpec::new(format!("p{idx}"), ty, ParamPassing::ByRef, false))
            .collect();
        let arity = u16::try_from(params.len()).expect("test function arity fits u16");
        let defaults = vec![None; params.len()].into();
        db.function(FunctionFacet::new(
            params,
            defaults,
            arity,
            ArgArity::Fixed(arity),
            returns,
            FunctionOrigin::Unknown,
        ))
    }

    #[test]
    fn is_assignable_reflexive_on_primitives() {
        // `A ≤ A` — the most basic rule. Pins that reflexivity works
        // for primitives where `Ty` implements `PartialEq` trivially.
        let (db, file_id) = empty_db();
        assert!(t(&db, file_id, db.number(None, None)).is_assignable_to(&t(
            &db,
            file_id,
            db.number(None, None)
        )));
        assert!(t(&db, file_id, db.string(None, false)).is_assignable_to(&t(
            &db,
            file_id,
            db.string(None, false)
        )));
        assert!(t(&db, file_id, db.boolean()).is_assignable_to(&t(&db, file_id, db.boolean())));
        assert!(!t(&db, file_id, db.number(None, None)).is_assignable_to(&t(
            &db,
            file_id,
            db.string(None, false)
        )));
    }

    #[test]
    fn is_assignable_reflexive_on_metadata_ref() {
        // `MetadataRef{kind, name} ≤ MetadataRef{kind, name}` — guards
        // the Name-equality path of the structural comparison. A ref
        // to a different catalog must *not* be assignable.
        let (db, file_id) = empty_db();
        let cat_x = metadata_ref(&db, MetadataKind::CatalogRef, "X");
        let cat_y = metadata_ref(&db, MetadataKind::CatalogRef, "Y");
        assert!(t(&db, file_id, cat_x).is_assignable_to(&t(&db, file_id, cat_x)));
        assert!(!t(&db, file_id, cat_x).is_assignable_to(&t(&db, file_id, cat_y)));
    }

    #[test]
    fn is_assignable_unknown_is_top_and_bottom() {
        // `A ≤ Unknown` (spec) *and* `Unknown ≤ A` (gradual-typing
        // extension — prevents false `TypeMismatch`es on inferences
        // that bailed out to `Unknown`).
        let (db, file_id) = empty_db();
        assert!(t(&db, file_id, db.number(None, None)).is_assignable_to(&t(
            &db,
            file_id,
            db.unknown()
        )));
        assert!(t(&db, file_id, db.unknown()).is_assignable_to(&t(
            &db,
            file_id,
            db.number(None, None)
        )));
        assert!(t(&db, file_id, db.unknown()).is_assignable_to(&t(&db, file_id, db.unknown())));
    }

    #[test]
    fn is_assignable_null_to_ref_types_only() {
        // `Null ≤ ref-type` for every MDO ref variant. Must **not**
        // hold for `CatalogObject` (object, not ref) or for non-MDO
        // primitives.
        let (db, file_id) = empty_db();
        let null = t(&db, file_id, db.null());
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
            let target = t(&db, file_id, metadata_ref(&db, kind, "X"));
            assert!(null.is_assignable_to(&target), "Null should be assignable to {kind:?}");
        }
        // Objects are not refs — must reject.
        let cat_obj = t(&db, file_id, metadata_ref(&db, MetadataKind::CatalogObject, "X"));
        assert!(!null.is_assignable_to(&cat_obj));
        assert!(!null.is_assignable_to(&t(&db, file_id, db.number(None, None))));
    }

    #[test]
    fn is_assignable_element_to_union_right() {
        // `A ≤ Union(…, A, …)` — the union-right rule. Element lives
        // in the union → assignable; element does not → rejected.
        let (db, file_id) = empty_db();
        let number_or_string = db.union(vec![db.number(None, None), db.string(None, false)]);
        assert!(t(&db, file_id, db.number(None, None)).is_assignable_to(&t(
            &db,
            file_id,
            number_or_string
        )));
        assert!(t(&db, file_id, db.string(None, false)).is_assignable_to(&t(
            &db,
            file_id,
            number_or_string
        )));
        assert!(!t(&db, file_id, db.boolean()).is_assignable_to(&t(
            &db,
            file_id,
            number_or_string
        )));
    }

    #[test]
    fn is_assignable_union_left_distributes() {
        // `Union(A, B) ≤ T ↔ A ≤ T ∧ B ≤ T`. `Union(Number, String) ≤
        // Number` must fail because String is not a Number; but
        // `Union(Number, Number)` collapses to `Number` (smart
        // constructor), so test with two genuinely distinct types.
        let (db, file_id) = empty_db();
        let ns = db.union(vec![db.number(None, None), db.string(None, false)]);
        // Neither `Number` nor `String` alone covers the whole union.
        assert!(!t(&db, file_id, ns).is_assignable_to(&t(&db, file_id, db.number(None, None))));
        assert!(!t(&db, file_id, ns).is_assignable_to(&t(&db, file_id, db.string(None, false))));

        // `Union(A, B) ≤ Union(A, B)` (reflexivity after `Ty::union`
        // normalisation) — and `Union(Number, String) ≤
        // Union(Number, String, Boolean)` via every component matching
        // some component of the target.
        let nsb = db.union(vec![db.number(None, None), db.string(None, false), db.boolean()]);
        assert!(t(&db, file_id, ns).is_assignable_to(&t(&db, file_id, ns)));
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
        let this_cat = db.mk_this_object(
            ConfigId::Root,
            MdoRefFacet::new(MdoType::Catalog, "Товары".to_string()),
        );
        let cat_object = metadata_ref(&db, MetadataKind::CatalogObject, "Товары");
        assert!(t(&db, file_id, this_cat).is_assignable_to(&t(&db, file_id, cat_object)));
        assert!(
            !t(&db, file_id, cat_object).is_assignable_to(&t(&db, file_id, this_cat)),
            "reverse *Object → ThisObject direction must be rejected — preserves provenance"
        );

        // Mismatched owner must still fail even in the accepted direction.
        let cat_other = metadata_ref(&db, MetadataKind::CatalogObject, "Номенклатура");
        assert!(!t(&db, file_id, this_cat).is_assignable_to(&t(&db, file_id, cat_other)));
    }

    #[test]
    fn is_assignable_concrete_ref_to_union_of_refs() {
        // Composition: union-right on two distinct concrete refs.
        // `CatalogRef.Товары ≤ Union(CatalogRef.Товары, DocumentRef.Заказ)`
        // must hold; a third ref absent from the union must fail.
        let (db, file_id) = empty_db();
        let cat_t = metadata_ref(&db, MetadataKind::CatalogRef, "Товары");
        let doc_z = metadata_ref(&db, MetadataKind::DocumentRef, "Заказ");
        let cat_o = metadata_ref(&db, MetadataKind::CatalogRef, "Номенклатура");
        let target = db.union(vec![cat_t, doc_z]);

        assert!(t(&db, file_id, cat_t).is_assignable_to(&t(&db, file_id, target)));
        assert!(t(&db, file_id, doc_z).is_assignable_to(&t(&db, file_id, target)));
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
        let cat_x = metadata_ref(&db, MetadataKind::CatalogRef, "X");
        let doc_y = metadata_ref(&db, MetadataKind::DocumentRef, "Y");
        let target = db.union(vec![cat_x, doc_y]);
        assert!(t(&db, file_id, db.null()).is_assignable_to(&t(&db, file_id, target)));

        // Null into a union with no ref members must fail.
        let ns = db.union(vec![db.number(None, None), db.string(None, false)]);
        assert!(!t(&db, file_id, db.null()).is_assignable_to(&t(&db, file_id, ns)));
    }

    #[test]
    fn is_assignable_union_with_null_left_distributes() {
        // Composition: `Union(Null, CatalogRef.X) ≤ CatalogRef.X` —
        // every component must fit individually. `Null ≤ ref` passes,
        // reflexivity passes → true.
        let (db, file_id) = empty_db();
        let cat_x = metadata_ref(&db, MetadataKind::CatalogRef, "X");
        let nullable_cat = db.union(vec![db.null(), cat_x]);
        assert!(t(&db, file_id, nullable_cat).is_assignable_to(&t(&db, file_id, cat_x)));

        // And the key negative composition case Codex flagged:
        // `Union(Null, String) ≤ CatalogRef.X` — `Null ≤ ref` true,
        // `String ≤ CatalogRef.X` false → whole union rejected.
        let null_or_string = db.union(vec![db.null(), db.string(None, false)]);
        assert!(
            !t(&db, file_id, null_or_string).is_assignable_to(&t(&db, file_id, cat_x)),
            "union-left must reject when any component fails"
        );
    }

    #[test]
    fn is_assignable_unknown_inside_union_collapses_to_concrete_arm() {
        // Phase 3 §4.E: the kernel's `canonicalise_union` ABSORBS
        // `Unknown` once a concrete arm remains (`bsl-types`
        // `intern.rs` step 4, plan §1.D rule 4). This DIVERGES from the
        // legacy `hir-def` `Ty::union` smart constructor, which
        // preserved `Unknown` members. Since assignability now runs on
        // interned `TypeId`s, a union built with an `Unknown` arm no
        // longer carries that arm — it collapses to the concrete
        // member(s).
        //
        // Net behavioural shift (intentional, single-source-of-truth):
        // the old "an Unknown arm makes the whole union permissive"
        // gradual quirk is GONE. It was explicitly flagged as a
        // revisit-point at M4 Task 7; the kernel resolves it in favour
        // of the concrete arm.
        let (db, file_id) = empty_db();

        // `Union(Unknown, String)` canonicalises to `String`, so
        // `String ≤ String` (reflexivity) → still true.
        let unknown_or_string = db.union(vec![db.unknown(), db.string(None, false)]);
        assert_eq!(unknown_or_string, db.string(None, false));
        assert!(t(&db, file_id, unknown_or_string).is_assignable_to(&t(
            &db,
            file_id,
            db.string(None, false)
        )));

        // `Union(Number, Unknown)` canonicalises to `Number`, so the
        // target is just `Number`. `String ≤ Number` → FALSE. Under the
        // legacy union-preserves-Unknown rule this returned true via the
        // gradual `String ≤ Unknown` arm; the kernel drops that arm.
        let number_or_unknown = db.union(vec![db.number(None, None), db.unknown()]);
        assert_eq!(number_or_unknown, db.number(None, None));
        assert!(
            !t(&db, file_id, db.string(None, false))
                .is_assignable_to(&t(&db, file_id, number_or_unknown)),
            "kernel absorbs Unknown: Number|Unknown collapses to Number, so String is not assignable"
        );
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
        let f_num_to_str = fixed_function(&db, vec![db.number(None, None)], db.string(None, false));
        let f_num_to_str_2 =
            fixed_function(&db, vec![db.number(None, None)], db.string(None, false));
        let f_str_to_str =
            fixed_function(&db, vec![db.string(None, false)], db.string(None, false));
        let f_num_to_num = fixed_function(&db, vec![db.number(None, None)], db.number(None, None));

        assert!(t(&db, file_id, f_num_to_str).is_assignable_to(&t(&db, file_id, f_num_to_str_2)));
        assert!(!t(&db, file_id, f_num_to_str).is_assignable_to(&t(&db, file_id, f_str_to_str)));
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
        let number_or_string = db.union(vec![db.number(None, None), db.string(None, false)]);
        let from = fixed_function(&db, vec![number_or_string], db.number(None, None));
        let to = fixed_function(&db, vec![db.number(None, None)], number_or_string);
        assert!(t(&db, file_id, from).is_assignable_to(&t(&db, file_id, to)));
        assert!(!t(&db, file_id, to).is_assignable_to(&t(&db, file_id, from)));
    }

    #[test]
    fn fields_deduplicate_duplicate_names_preferring_attributes() {
        let fixture = TempFixture::duplicated_field();
        let (db, file_id) = db_with_configuration(fixture.path());
        let cat = t(&db, file_id, metadata_ref(&db, MetadataKind::CatalogRef, "Справочник1"));

        let fields = cat.fields();
        let matches: Vec<_> =
            fields.iter().filter(|field| field.name == Name::new("Реквизит2")).collect();
        assert_eq!(matches.len(), 1, "duplicate Russian names must be deduplicated");
        assert_eq!(matches[0].ty, db.number(None, None), "attribute must win over tabular section");
    }
}
