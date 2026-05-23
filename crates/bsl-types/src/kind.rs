//! `TypeKind` — the value-level taxonomy of 1С runtime types.
//!
//! Single source of truth for what kinds of values exist in BSL.
//! BSL, SDBL, form, and doc-comment elaborators all produce values in
//! this universe via [`crate::builders::Builders`] and
//! [`crate::intern::TypeKernelDb::intern_type`].
//!
//! See `.omc/plans/clean-slate-type-architecture.md` v5 §4.2 for the
//! authoritative variant list and `.omc/plans/type-kernel-phase-1-sandbox.md`
//! v7 §1.B for the Phase 1 implementation contract.

use std::sync::Arc;

use bsl_metadata::{MdoType, Name};

use crate::facet::{
    ArrayFacet, DateFacet, FunctionFacet, MapFacet, MetaObjFacet, MetaRefFacet, NumberFacet,
    PlatformObjectFacet, ProjectionFacet, StringFacet, StructureFacet, TableFacet,
};

/// Opaque numeric handle to an interned `TypeKind`.
///
/// `TypeId` carries no lifetime parameter — the handle is freely
/// `Copy`. Lifetime safety lives on the lookup side: methods that
/// return `&TypeKind` borrow from `&self` of the db, so references
/// cannot outlive the db borrow.
///
/// **Cross-db caveat:** `TypeId(u64)` carries no db-identity tag.
/// Callers must not mix `TypeId`s obtained from one db with
/// `lookup_type` on a different db — IDs are opaque indices into a
/// particular db's intern table. Workspace discipline relies on the
/// single global db invariant (e.g. one `RootDatabase`); the type
/// system cannot enforce it.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct TypeId(pub(crate) u64);

impl TypeId {
    /// Raw numeric value of the handle. For debug/test only — production
    /// code compares `TypeId` via `Eq`, not via the inner number.
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// Per-MDO configuration identity carried by [`MetaRefFacet`] and
/// [`MetaObjFacet`].
///
/// Required (never `None`) so that two `MetadataRef`s for the same
/// `(kind, name)` from different configurations don't collide on the
/// kernel level. Sandbox uses [`Self::Root`]; production resolves to
/// either [`Self::Resolved`] for known configurations or
/// [`Self::Unknown`] for unresolvable names (carries the name so
/// distinct unresolved names don't collide).
///
/// Documented limitation: two distinct configurations that both fail
/// to resolve the **same** name collide at the kernel layer (both
/// produce `ConfigId::Unknown(name)`). Diagnostics differentiate by
/// source location, not by kernel identity.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum ConfigId {
    /// Single-config workspaces or sandbox tests.
    Root,
    /// Multi-config workspace; index into the interned configuration
    /// table maintained by `bsl-config::VisibleConfig` (Phase 2+).
    Resolved(u32),
    /// MDO name couldn't be resolved against any known configuration.
    /// Carries the name itself so different unresolved names produce
    /// different `ConfigId` values.
    Unknown(Name),
}

/// Surface form of a metadata reference type.
///
/// Distinguishes the BSL runtime type that wraps a configuration MDO —
/// e.g. `CatalogRef` is `СправочникСсылка.X`, `CatalogObject` is
/// `СправочникОбъект.X`. The same `MdoType::Catalog` projects to
/// `CatalogRef` or `CatalogObject` depending on the syntactic
/// construct.
///
/// Today this enum lives also in `hir-def::ty::MetadataKind`; Phase 2
/// migration replaces that with a re-export from here. Variant set
/// matches `hir-def` 1:1 so the migration is mechanical.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum MetadataKind {
    // ── Reference forms ────────────────────────────────────────
    CatalogRef,
    DocumentRef,
    EnumRef,
    TaskRef,
    BusinessProcessRef,
    ExchangePlanRef,
    ChartOfAccountsRef,
    InformationRegisterRef,
    AccumulationRegisterRef,
    AccountingRegisterRef,
    CalculationRegisterRef,

    // ── Object forms ───────────────────────────────────────────
    CatalogObject,
    DocumentObject,
    TaskObject,
    BusinessProcessObject,
    DataProcessorObject,
    ReportObject,
    ExchangePlanObject,
    ChartOfAccountsObject,

    // ── Register record-manager / record-set / record forms ────
    InformationRegisterRecordManager,
    InformationRegisterRecordSet,
    AccumulationRegisterRecordSet,
    AccountingRegisterRecordSet,
    CalculationRegisterRecordSet,
    InformationRegisterRecord,
    AccumulationRegisterRecord,
    AccountingRegisterRecord,
    CalculationRegisterRecord,

    // ── Register inner shapes (parameterised by parent flavour) ─
    RegisterDimension { parent: MdoType },
    RegisterResource { parent: MdoType },
    RegisterAttribute { parent: MdoType },
    RegisterFilter { parent: MdoType },

    // ── Tabular sections (parameterised by owner MDO) ──────────
    TabularSection { parent: MdoType },
    TabularSectionRow { parent: MdoType },
}

/// Literal value embedded in a `DefaultValue::Literal`.
///
/// Covers BSL literal forms that survive in default expressions.
/// Anything more complex (function calls, named constants, arithmetic)
/// stays as `DefaultValue::DeferredExpr` or `NamedConstant`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LiteralValue {
    /// Numeric literal as written (e.g. `3.14`, `100`).
    Number(Box<str>),
    /// String literal contents (without surrounding quotes).
    String(Box<str>),
    Boolean(bool),
    /// `Неопределено` / `Undefined`.
    Undefined,
    /// `NULL`.
    Null,
    /// `Дата(2024, 1, 1)` etc. — kept as raw string until a date
    /// elaborator promotes it.
    Date(Box<str>),
}

/// Reference into a body HIR expression, for `DefaultValue::DeferredExpr`.
///
/// Phase 1 stub: opaque `u32`. Real `ExprId` lives in `hir-def` and is
/// wired in Phase 5 when the doc/form HIR migrations land. Sandbox
/// tests must NOT destructure this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprRef(pub(crate) u32);

/// SDBL / form / doc projection — an ordered list of typed fields.
///
/// Carried by `ProjectionFacet`, `TableFacet`, and `Query{,BatchResult}`
/// variants of [`TypeKind`]. Field order is **preserved** (not
/// dedup'd) — projections are ordered lists in BSL source semantics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Projection {
    pub fields: Arc<[ProjectionField]>,
    pub origin: ProjectionOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ProjectionField {
    pub name: Name,
    pub ty: TypeId,
    pub source: ProjectionFieldSource,
}

/// Where a projection came from. Provenance — not part of equality
/// (canonicalisation strips it before hashing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProjectionOrigin {
    /// SDBL `ВЫБРАТЬ`/`SELECT` projection.
    SdblQuery,
    /// Form attribute (whole-form projection).
    FormAttribute,
    /// Hand-built `Новый ТаблицаЗначений` with `.Колонки.Добавить`.
    ValueTableLiteral,
    /// Unknown / not yet annotated.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProjectionFieldSource {
    /// Plain column reference.
    Column,
    /// `ВЫРАЗИТЬ` / `CAST` precision-bearing field.
    Cast,
    /// Aggregate (`SUM`, `MAX`, …).
    Aggregate,
    /// `НоваяКолонка` / synthesised field.
    Derived,
    /// Unknown origin.
    Unknown,
}

/// Provenance tag attached to primitive facets (number, string, date)
/// and function facet for hover / diagnostics.
///
/// **Excluded from equality** (canonicalisation strips it). Adding new
/// variants is non-breaking because consumers shouldn't dispatch on
/// origin — it's display/debug data only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TypeOrigin {
    /// Plain BSL literal (`3.14`, `"abc"`).
    BslLiteral,
    /// SDBL CAST expression (`ВЫРАЗИТЬ(... КАК ...)`).
    SdblCast,
    /// Doc-comment type annotation (`// Возвращаемое значение: ...`).
    DocComment,
    /// Form attribute type.
    FormAttribute,
}

/// The 1С runtime type universe.
///
/// Every BSL value at runtime has a kind in here. Elaborators (BSL,
/// SDBL, form, doc) construct values in this universe via
/// [`crate::builders::Builders`]; canonical identity comes from
/// [`crate::intern::TypeKernelDb::intern_type`].
///
/// `TypeKind` is fully `pub` + `#[non_exhaustive]`: external crates
/// `match` freely. Construction discipline is enforced at the
/// interning gateway, not at the variant boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TypeKind {
    // ── Bottom / top ───────────────────────────────────────────
    /// Analysis incomplete — "we don't yet know".
    #[default]
    Unknown,
    /// Proven unreachable / type-error sink.
    ///
    /// Distinct from `Unknown`: `Never` is "this code can't run";
    /// `Unknown` is "we haven't figured it out yet".
    Never,
    /// Explicit `Произвольный` / `Any`. Top type — every value fits.
    Any,

    // ── Primitives (with facets) ──────────────────────────────
    Number(NumberFacet),
    String(StringFacet),
    Date(DateFacet),
    Boolean,
    Null,
    Undefined,
    /// `УникальныйИдентификатор` — value-level UUID wrapper.
    Uuid,

    // ── Built-in collections ──────────────────────────────────
    Array(ArrayFacet),
    Map(MapFacet),
    Structure(StructureFacet),
    /// `СписокЗначений` — list with optional element type.
    ValueList(Option<TypeId>),
    /// `ТаблицаЗначений` — with optional projection (from
    /// `Запрос.Выполнить().Выгрузить()` or `Новый ТаблицаЗначений`
    /// with `.Колонки.Добавить`).
    ValueTable(TableFacet),
    /// Row of a projected `ТаблицаЗначений`.
    ValueTableRow(TableFacet),

    // ── Metadata references ───────────────────────────────────
    /// Concrete metadata reference: `Справочники.X.НайтиПоКоду(…)`,
    /// `СправочникСсылка.X`, etc.
    MetadataRef(MetaRefFacet),
    /// `<Flavour>Ссылка` without a specific name —
    /// `ЛюбаяСсылка<Catalog>`. Coarser than `MetadataRef`.
    AnyMetadataRef {
        mdo_type: MdoType,
    },
    /// Concrete metadata object: `СправочникОбъект.X`, etc.
    MetadataObject(MetaObjFacet),
    /// Tabular section of an MDO. `parent` pins the owner MDO;
    /// `name` is the section name (e.g. `"Товары"`).
    TabularSection {
        parent: MetaRefFacet,
        name: Name,
    },
    /// A single row of a tabular section.
    TabularSectionRow {
        parent: MetaRefFacet,
        name: Name,
    },

    // ── Register-specific inner shapes ────────────────────────
    /// `Измерение` of a register.
    RegisterDimension {
        parent: MetaRefFacet,
        name: Name,
    },
    /// `Ресурс` of a register.
    RegisterResource {
        parent: MetaRefFacet,
        name: Name,
    },
    /// `Реквизит` of a register.
    RegisterAttribute {
        parent: MetaRefFacet,
        name: Name,
    },
    /// `Отбор` of a record set.
    RegisterFilter {
        parent: MetaRefFacet,
    },
    /// Bare attribute of an MDO (catalog/document attribute).
    Attribute {
        parent: MetaRefFacet,
        name: Name,
    },

    // ── Platform wrappers ─────────────────────────────────────
    /// Wrapped platform object (`Запрос`, `ТабличныйДокумент`, …).
    PlatformObject(PlatformObjectFacet),
    /// `ХранилищеЗначения`.
    ValueStorage,
    /// `Тип` reflection wrapper (returned by `ТипЗнч`).
    TypeDescriptor,

    // ── Polymorphism ──────────────────────────────────────────
    /// Union of types. Members must be sorted (by `TypeId`) and
    /// dedup'd; canonicalisation enforces this at intern time.
    Union(Arc<[TypeId]>),

    // ── Manager axis (values) ─────────────────────────────────
    /// `Справочники`, `Документы`, … — collective managers.
    ManagerCollection(MdoType),
    /// `Справочники.X` — concrete object manager.
    ObjectManager(MetaRefFacet),

    // ── Functions / callables ─────────────────────────────────
    Function(FunctionFacet),

    // ── Query results (SDBL provenance lives in facets) ───────
    /// `Запрос.Выполнить()` result.
    QueryResult(ProjectionFacet),
    /// `Результат.Выбрать()` cursor.
    QueryResultSelection(ProjectionFacet),
    /// `Запрос.ВыполнитьПакет()` batch result with per-query
    /// projections.
    QueryBatchResult {
        per_query: Arc<[Option<Arc<Projection>>]>,
    },
    /// `Запрос` value holding one or more sub-query projections
    /// (set via `.Текст = ...`).
    Query {
        projections: Arc<[Option<Arc<Projection>>]>,
    },
}
