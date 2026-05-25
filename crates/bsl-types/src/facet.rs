//! Facet payload structs carried by `TypeKind` variants.
//!
//! Facets carry precision/scale/length/projection/etc. inline — they
//! are part of the type, not side-channel data. Provenance hints
//! (`*_origin`, `*_source`) are **excluded from equality**;
//! canonicalisation at [`crate::intern::TypeKernelDb::intern_type`]
//! strips them.
//!
//! See `.omc/plans/clean-slate-type-architecture.md` v5 §4.3 for the
//! full specification.

use std::sync::Arc;

use bsl_metadata::{MdoType, Name};

use crate::kind::{ConfigId, ExprRef, LiteralValue, MetadataKind, Projection, TypeId, TypeOrigin};

// ── Primitive facets ─────────────────────────────────────────

/// Number facet — precision + scale, plus provenance hint.
///
/// `Число(15, 2)` → `precision: Some(15), scale: Some(2)`.
/// `Число` (unsized) → both `None`.
/// `Число(15)` (precision-only) → `precision: Some(15), scale: None`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct NumberFacet {
    pub precision: Option<u8>,
    pub scale: Option<u8>,
    /// Provenance hint — out of equality.
    pub origin: Option<TypeOrigin>,
}

impl NumberFacet {
    /// Unsized `Число`.
    pub const fn unsized_() -> Self {
        Self { precision: None, scale: None, origin: None }
    }

    /// `Число(P, S)`.
    pub const fn with_scale(precision: u8, scale: u8) -> Self {
        Self { precision: Some(precision), scale: Some(scale), origin: None }
    }

    /// `Число(P)`.
    pub const fn with_precision(precision: u8) -> Self {
        Self { precision: Some(precision), scale: None, origin: None }
    }
}

/// String facet — optional length, optional `Фиксированная`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct StringFacet {
    pub length: Option<u32>,
    pub fixed: bool,
    pub origin: Option<TypeOrigin>,
}

impl StringFacet {
    pub const fn unsized_() -> Self {
        Self { length: None, fixed: false, origin: None }
    }

    pub const fn with_length(length: u32) -> Self {
        Self { length: Some(length), fixed: false, origin: None }
    }
}

/// Granularity of a date facet — date only, time only, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DateComponent {
    Date,
    Time,
    DateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct DateFacet {
    pub component: DateComponent,
    pub origin: Option<TypeOrigin>,
}

impl DateFacet {
    pub const fn datetime() -> Self {
        Self { component: DateComponent::DateTime, origin: None }
    }
}

// ── Metadata facets ──────────────────────────────────────────

/// Facet for `TypeKind::MetadataRef` — kind + name + per-config id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct MetaRefFacet {
    pub kind: MetadataKind,
    pub name: Name,
    /// Required (never `None`). Single-config workspaces use
    /// `ConfigId::Root`.
    pub config_id: ConfigId,
}

/// Facet for `TypeKind::MetadataObject`. Same shape as
/// [`MetaRefFacet`] but separated by type so `match` discriminates
/// without inspecting the `kind` field.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct MetaObjFacet {
    pub kind: MetadataKind,
    pub name: Name,
    pub config_id: ConfigId,
}

/// Facet for `TypeKind::ObjectManager` — `Справочники.X`, `Константы.X`, ….
///
/// Keyed by [`MdoType`] (the metadata-object family), **not** by
/// [`MetadataKind`]: a manager is identified by which kind of metadata
/// object it manages, whereas `MetadataKind` is a value/reference
/// taxonomy (Ref / Object / RecordSet). Several manager families
/// (`Constant`, `CommonModule`, `ChartOfCharacteristicTypes`,
/// `ChartOfCalculationTypes`, `ExternalDataSource`, `Cube`,
/// `DimensionTable`) have no `MetadataKind` value-companion at all, so
/// `MetaRefFacet` could not represent them losslessly.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ManagerFacet {
    pub mdo: MdoType,
    pub name: Name,
    /// Required (never `None`). Single-config workspaces use
    /// `ConfigId::Root`.
    pub config_id: ConfigId,
}

impl ManagerFacet {
    pub fn new(mdo: MdoType, name: Name, config_id: ConfigId) -> Self {
        Self { mdo, name, config_id }
    }
}

/// Form data shape exposed by `ДанныеФормы*` platform values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FormDataFacet {
    Structure,
    Collection,
    StructureWithCollection,
}

/// Coarse taxonomy of form controls.
pub use bsl_metadata::FormElementKind as FormElementFacet;

/// Minimal owned MDO reference used by form-specific type facets.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct MdoRefFacet {
    pub mdo_type: MdoType,
    pub name: Name,
}

impl MdoRefFacet {
    pub fn new(mdo_type: MdoType, name: Name) -> Self {
        Self { mdo_type, name }
    }
}

/// Data-path binding for a form control.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FormBindingFacet {
    pub path: Arc<[Name]>,
    pub target: FormBindingTargetFacet,
}

impl FormBindingFacet {
    pub fn new(path: Arc<[Name]>, target: FormBindingTargetFacet) -> Self {
        Self { path, target }
    }
}

/// Resolved target of a form binding path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FormBindingTargetFacet {
    TabularSection { mdo_ref: MdoRefFacet, section: Name },
    Attribute { ty: TypeId },
}

// Deterministic raw-id ordering only (NOT semantic), required while
// `hir_def::ty::Ty::FormControl` embeds `FormBindingFacet` and derives
// `Ord`. Removable once `Ty` is deleted (§4.E.6h) — the kernel orders
// types by interned `TypeId`, never structurally. Mirrors the
// transitional `impl Ord for Projection` in `kind.rs`.
impl Ord for MdoRefFacet {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.mdo_type.cmp(&other.mdo_type).then_with(|| self.name.cmp(&other.name))
    }
}
impl PartialOrd for MdoRefFacet {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for FormBindingTargetFacet {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        use FormBindingTargetFacet::*;
        match (self, other) {
            (
                TabularSection { mdo_ref: a, section: sa },
                TabularSection { mdo_ref: b, section: sb },
            ) => a.cmp(b).then_with(|| sa.cmp(sb)),
            (Attribute { ty: a }, Attribute { ty: b }) => a.raw().cmp(&b.raw()),
            (TabularSection { .. }, Attribute { .. }) => Ordering::Less,
            (Attribute { .. }, TabularSection { .. }) => Ordering::Greater,
        }
    }
}
impl PartialOrd for FormBindingTargetFacet {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for FormBindingFacet {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.path.cmp(&other.path).then_with(|| self.target.cmp(&other.target))
    }
}
impl PartialOrd for FormBindingFacet {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// ── Collection facets ────────────────────────────────────────

/// Source tag for a `ValueTable` projection — provenance only,
/// out of equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TableSource {
    /// `Запрос.Выполнить().Выгрузить()`.
    SdblUnload,
    /// `Новый ТаблицаЗначений` literal.
    NewValueTable,
    /// Form attribute that wraps a tabular section.
    FormAttribute,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct TableFacet {
    pub projection: Option<Arc<Projection>>,
    pub source: TableSource,
}

impl TableFacet {
    pub fn unprojected() -> Self {
        Self { projection: None, source: TableSource::Unknown }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ArrayFacet {
    /// Element type, or `None` for `Массив` without a known element
    /// type.
    pub element: Option<TypeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct MapFacet {
    pub key: Option<TypeId>,
    pub value: Option<TypeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct StructureFacet {
    /// Ordered key names. `None` when keys aren't known
    /// (`Новый Структура()` without arguments).
    pub keys: Option<Arc<[Name]>>,
}

// ── Projection facet ─────────────────────────────────────────

/// Source tag for a projection-bearing type — provenance only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProjectionSource {
    Sdbl,
    FormAttribute,
    ValueTableLiteral,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ProjectionFacet {
    pub projection: Option<Arc<Projection>>,
    pub source: ProjectionSource,
}

impl ProjectionFacet {
    pub fn empty(source: ProjectionSource) -> Self {
        Self { projection: None, source }
    }

    pub fn with(projection: Arc<Projection>, source: ProjectionSource) -> Self {
        Self { projection: Some(projection), source }
    }
}

/// Display-only shadow for an SDBL field type, carried alongside a
/// [`Projection`] when the IDE needs precision / scale / length info
/// the bridged `TypeId` cannot carry (e.g. `Число(15,2)`, `Строка(50)`).
///
/// Phase 3 §4.D: introduced to preserve hover-rendered precision through
/// the `Ty ↔ TypeId` bridge round-trip. Decoupled from `sdbl-hir` so the
/// kernel stays independent of SDBL HIR shape.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub struct SdblTypeShadowFacet {
    /// Pre-rendered SDBL type label (locale-stable).
    pub display: String,
}

impl SdblTypeShadowFacet {
    pub fn new(display: String) -> Self {
        Self { display }
    }
}

// ── Platform object facet ────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PlatformObjectFacet {
    /// Russian platform type name as written in BSL (e.g. `Запрос`,
    /// `ТабличныйДокумент`).
    pub name: Name,
}

// ── Function facet + supporting types ────────────────────────

/// How a parameter is passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParamPassing {
    /// `Знач` — by-value.
    ByVal,
    /// Default — by-reference.
    ByRef,
}

/// Argument arity — fixed count or variadic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ArgArity {
    /// Exactly this many positional args accepted.
    Fixed(u16),
    /// Last parameter accepts 0+ additional args.
    Variadic,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ParamSpec {
    pub name: Name,
    /// Declared type. `Unknown` if unannotated. Variadic params
    /// declare the *element* type here.
    pub ty: TypeId,
    pub passing: ParamPassing,
    /// Only the last param of a function may be variadic.
    pub variadic: bool,
}

impl ParamSpec {
    pub fn new(name: Name, ty: TypeId, passing: ParamPassing, variadic: bool) -> Self {
        Self { name, ty, passing, variadic }
    }
}

/// Default value for a parameter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DefaultValue {
    /// Literal default: `= 3.14`, `= "abc"`, `= Истина`,
    /// `= Неопределено`, `= NULL`, `= Дата(2024, 1, 1)`.
    Literal(LiteralValue),
    /// Reference to a module-level named constant. Resolution
    /// happens at the call-site, not at the function definition.
    NamedConstant(Name),
    /// Arbitrary expression default (e.g. `= НоваяОбработка()`).
    /// Pre-evaluation would re-enter `infer_module` from the
    /// signature query and break the leaf guarantee — kept lazy.
    DeferredExpr(ExprRef),
}

/// Provenance for a function type — out of equality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FunctionOrigin {
    /// User-defined `Процедура`/`Функция`.
    UserDefined,
    /// Platform global (`Сообщить`, `ВЫРАЗИТЬ`-like helpers, …).
    PlatformGlobal,
    /// Closure / anonymous function (future-proof; BSL doesn't have
    /// these today but doc-comments can describe them).
    Closure,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FunctionFacet {
    pub params: Arc<[ParamSpec]>,
    /// Per-param default; `None` means required, `Some` means
    /// optional with the given default.
    pub defaults: Arc<[Option<DefaultValue>]>,
    /// Minimum positional arguments accepted (count of leading
    /// required params).
    pub min_args: u16,
    pub max_args: ArgArity,
    pub returns: TypeId,
    pub origin: FunctionOrigin,
}

impl FunctionFacet {
    pub fn new(
        params: Arc<[ParamSpec]>,
        defaults: Arc<[Option<DefaultValue>]>,
        min_args: u16,
        max_args: ArgArity,
        returns: TypeId,
        origin: FunctionOrigin,
    ) -> Self {
        Self { params, defaults, min_args, max_args, returns, origin }
    }
}

/// `ProjectionFieldSource` lives in `crate::kind` next to `Projection`;
/// re-exported here so `use crate::facet::*` reaches it without
/// dipping into `kind`.
pub use crate::kind::ProjectionFieldSource;
