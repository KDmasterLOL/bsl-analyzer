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
    ArrayFacet, DateFacet, FormBindingFacet, FormDataFacet, FormElementFacet, FunctionFacet,
    ManagerFacet, MapFacet, MdoRefFacet, MetaObjFacet, MetaRefFacet, NumberFacet,
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

    /// Construct a TypeId from a raw storage index.
    ///
    /// Reserved for production [`TypeKernelDb`](crate::intern::TypeKernelDb)
    /// implementations that manage the underlying type table.
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

// `ConfigId` is now defined canonically in `bsl-config` (Layer 0.5);
// re-exported here so existing `bsl_types::kind::ConfigId` and
// `bsl_types::ConfigId` import paths continue to resolve (Phase 2.D).
pub use bsl_config::ConfigId;

/// Metadata object kind.
///
/// Classifies the flavour of MDO (or MDO fragment) that a metadata reference
/// carries. The reference `name` is the MDO identifier as it appears in the
/// configuration (`"ПКО"`, `"Номенклатура"`). For [`Self::TabularSection`] /
/// [`Self::TabularSectionRow`] the name encodes `"Parent.Section"` (e.g.
/// `"ПКО.Товары"`) — parent MDO first, tabular section name second — and the
/// variant **also** carries the parent [`MdoType`], so callers never have to
/// probe several candidates to disambiguate `Catalog "X"` from `Document "X"`
/// with an identically named section.
///
/// Adding a variant? Also extend:
/// - `metadata_kind_from_prefix` in `hir-ty/src/lower/mod.rs` (prefix → kind),
/// - `mdo_ref_prefix` in `hir-def/src/type_ref.rs` if a new `MdoType` prefix is
///   needed,
/// - JSDoc parser in `hir-def/src/ty/doc_types.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MetadataKind {
    /// Catalog reference (СправочникСсылка).
    CatalogRef,
    /// Document reference (ДокументСсылка).
    DocumentRef,
    /// Document object (ДокументОбъект).
    DocumentObject,
    /// Catalog object (СправочникОбъект).
    CatalogObject,
    /// Information register record manager (РегистрСведенийМенеджерЗаписи).
    InformationRegisterRecordManager,
    /// Information register record set (РегистрСведенийНаборЗаписей).
    ///
    /// Returned by `РегистрыСведений.X.СоздатьНаборЗаписей()`. Companion
    /// to [`Self::InformationRegisterRecordManager`]: that variant models
    /// a single-record manager, this one models the multi-record set
    /// receiver. Methods (`Записать`, `Загрузить`, `Очистить`, …) are
    /// indexed under composite `InformationRegisterRecordSet.<Имя>` in
    /// platform data; workspace `RecordSetModule.bsl` exports also flow
    /// through this kind via `record_set_kind_to_mdo`.
    InformationRegisterRecordSet,
    /// Accumulation register record set (РегистрНакопленияНаборЗаписей).
    AccumulationRegisterRecordSet,
    /// Accounting register record set (РегистрБухгалтерииНаборЗаписей).
    AccountingRegisterRecordSet,
    /// Calculation register record set (РегистрРасчетаНаборЗаписей).
    CalculationRegisterRecordSet,
    /// Information register record (РегистрСведенийЗапись).
    ///
    /// The element type yielded when iterating an
    /// [`Self::InformationRegisterRecordSet`] with `Для каждого … Из …`.
    /// HBK names this composite `РегистрСведенийЗапись.<Имя регистра
    /// сведений>` and indexes its methods (`Удалить`, `Установить…`,
    /// access to dimensions/resources/attributes) under
    /// `InformationRegisterRecord.<Имя>` in platform data. Today this
    /// kind is a leaf receiver in `hir-ty`: field-resolution onto register
    /// dimensions/resources is wired through `bsl-metadata` in a follow-up,
    /// the same way [`Self::RegisterDimension`] is staged.
    InformationRegisterRecord,
    /// Accumulation register record (РегистрНакопленияЗапись). Element of
    /// [`Self::AccumulationRegisterRecordSet`]; see
    /// [`Self::InformationRegisterRecord`] for the full rationale.
    AccumulationRegisterRecord,
    /// Accounting register record (РегистрБухгалтерииЗапись). Element of
    /// [`Self::AccountingRegisterRecordSet`]; see
    /// [`Self::InformationRegisterRecord`] for the full rationale.
    AccountingRegisterRecord,
    /// Calculation register record (РегистрРасчетаЗапись). Element of
    /// [`Self::CalculationRegisterRecordSet`]; see
    /// [`Self::InformationRegisterRecord`] for the full rationale.
    CalculationRegisterRecord,
    /// Enum reference (ПеречислениеСсылка).
    EnumRef,
    /// Task reference (ЗадачаСсылка).
    TaskRef,
    /// Task object (ЗадачаОбъект). Companion to [`Self::TaskRef`] —
    /// emitted from `<Type>cfg:TaskObject.X</Type>` and used as the
    /// underlying MDO behind a Task form's `Объект`.
    TaskObject,
    /// Business process reference (БизнесПроцессСсылка).
    BusinessProcessRef,
    /// Business process object (БизнесПроцессОбъект). Companion to
    /// [`Self::BusinessProcessRef`] — emitted from
    /// `<Type>cfg:BusinessProcessObject.X</Type>` and used as the
    /// underlying MDO behind a BusinessProcess form's `Объект`.
    BusinessProcessObject,
    /// DataProcessor object (ОбработкаОбъект). DataProcessors only have
    /// an Object form (no `*Ref` companion) — used as the underlying
    /// MDO behind a DataProcessor form's `Объект`.
    DataProcessorObject,
    /// Report object (ОтчётОбъект). Symmetric to
    /// [`Self::DataProcessorObject`] — Reports only have an Object form.
    ReportObject,
    /// Exchange plan reference (ПланОбменаСсылка).
    ExchangePlanRef,
    /// Exchange plan object (ПланОбменаОбъект).
    ExchangePlanObject,
    /// Chart of accounts reference (ПланСчетовСсылка).
    ChartOfAccountsRef,
    /// Chart of accounts object (ПланСчетовОбъект).
    ChartOfAccountsObject,
    /// Information register reference / record key form
    /// (РегистрСведенийКлючЗаписи / `InformationRegisterRef`).
    ///
    /// Companion to [`Self::InformationRegisterRecordManager`]: the manager
    /// variant models the runtime record-manager object, this one models the
    /// XML-emitted reference/key token as a value type.
    InformationRegisterRef,
    /// Accumulation register reference / record key form
    /// (РегистрНакопленияКлючЗаписи / `AccumulationRegisterRef`).
    AccumulationRegisterRef,
    /// Accounting register reference / record key form
    /// (РегистрБухгалтерииКлючЗаписи / `AccountingRegisterRef`).
    AccountingRegisterRef,
    /// Calculation register reference / record key form
    /// (РегистрРасчётаКлючЗаписи / `CalculationRegisterRef`).
    CalculationRegisterRef,
    /// Dimension (измерение) of a register — opaque symbolic form.
    ///
    /// Used as a fallback metadata reference when the XML-parsed
    /// `attr_type` on a `bsl_metadata::register::Dimension` is absent and
    /// the field-lookup adapter has no concrete type to return. `parent`
    /// pins the register flavour (`InformationRegister`,
    /// `AccumulationRegister`, `AccountingRegister`,
    /// `CalculationRegister`) so downstream tooling can still surface
    /// "dimension of register X" even without a typed payload. The enclosing
    /// metadata reference name carries `"Register.Dimension"` (mirrors the
    /// `TabularSection` convention) so the originating register is
    /// recoverable.
    ///
    /// Kept as a leaf receiver in M4 Task 2: further field access on this
    /// variant returns `None`. A follow-up that wires the
    /// `Движения.X.Добавить()` record surface can promote it to a receiver
    /// once `bsl-metadata` exposes the record shape.
    RegisterDimension {
        /// Register flavour that owns the dimension.
        parent: MdoType,
    },
    /// Resource (ресурс) of a register — opaque symbolic form.
    ///
    /// Same semantics as [`Self::RegisterDimension`]: fallback metadata
    /// reference when the XML-parsed `attr_type` is missing, `parent` pins
    /// the register flavour, name carries `"Register.Resource"`.
    RegisterResource {
        /// Register flavour that owns the resource.
        parent: MdoType,
    },
    /// Attribute (реквизит) of a register — opaque symbolic form.
    ///
    /// Same semantics as [`Self::RegisterDimension`]: fallback metadata
    /// reference when the XML-parsed `attr_type` is missing, `parent` pins
    /// the register flavour, name carries `"Register.Attribute"`.
    RegisterAttribute {
        /// Register flavour that owns the attribute.
        parent: MdoType,
    },
    /// Per-record-set `Отбор` (Filter) — synthetic receiver.
    ///
    /// 1С runtime exposes a `.Отбор` property on every register record set
    /// whose member names are the owning register's dimensions and each member
    /// is a `ЭлементОтбора` (FilterItem). The HBK shipped in
    /// `bsl-platform/data/platform_data.json` does not declare this property
    /// on any RecordSet `type_name` (gap of the source archive, not a scraper
    /// bug), so we synthesize it here.
    ///
    /// `parent` pins the register flavour; the enclosing metadata reference
    /// name carries the bare register name (no `"Register.Filter"` composite —
    /// there is at most one `Отбор` per record-set so a second segment would
    /// be redundant).
    ///
    /// Method/property surface:
    /// - Members (dimensions) come from `hir-ty` field enumeration.
    /// - The 10 platform `Filter` methods (`Сбросить`, `Получить`,
    ///   `Найти`, …) are wired through a scalar-key side channel
    ///   ([`Self::scalar_platform_key`] returning `"Filter"`), not through
    ///   [`Self::platform_prefix`] — `Filter` is a scalar `type_name`, not a
    ///   composite `Filter.<X>` prefix.
    RegisterFilter {
        /// Register flavour that owns the record set producing this `Отбор`.
        parent: MdoType,
    },
    /// Tabular section of a metadata object (`ТабличнаяЧасть`).
    ///
    /// `parent` identifies the MDO flavour that owns this section (Catalog,
    /// Document, BusinessProcess, Task, ChartOf*), and the name of the
    /// enclosing metadata reference carries `"Parent.Section"` (e.g.
    /// `"ПКО.Товары"`). Together they pin one specific MDO: `Catalog "X"`
    /// and `Document "X"` with a shared tabular-section name resolve
    /// unambiguously because their [`Self::TabularSection`] receivers differ
    /// in `parent`.
    TabularSection {
        /// MDO flavour that owns the section (matches the parent's `mdo_type`).
        parent: MdoType,
    },
    /// A single row of a tabular section (`СтрокаТабличнойЧасти`).
    ///
    /// Uses the same `parent` + `"Parent.Section"` name convention as
    /// [`Self::TabularSection`].
    TabularSectionRow {
        /// MDO flavour that owns the enclosing section.
        parent: MdoType,
    },
}

impl MetadataKind {
    /// The `*Object` variant for an MDO flavour, or `None` if the
    /// flavour does not carry an object form in [`MetadataKind`] today.
    ///
    /// Single source of truth for the `ЭтотОбъект` coercion and any future
    /// callers that need to pick the right `*Object` [`MetadataKind`] given an
    /// [`MdoType`]. Keeping the mapping here rather than duplicating it at
    /// every call site lets resolvers gate `ThisObject` construction on the
    /// same set of flavours the field / method adapters actually coerce:
    /// producing a `ThisObject` for an MDO with no `*Object` surface would
    /// leave the receiver dangling.
    ///
    /// New `*Object` variants should update this method.
    pub fn object_kind_for(mdo_type: MdoType) -> Option<Self> {
        match mdo_type {
            MdoType::Catalog => Some(MetadataKind::CatalogObject),
            MdoType::Document => Some(MetadataKind::DocumentObject),
            MdoType::ExchangePlan => Some(MetadataKind::ExchangePlanObject),
            MdoType::ChartOfAccounts => Some(MetadataKind::ChartOfAccountsObject),
            MdoType::Task => Some(MetadataKind::TaskObject),
            MdoType::BusinessProcess => Some(MetadataKind::BusinessProcessObject),
            MdoType::DataProcessor => Some(MetadataKind::DataProcessorObject),
            MdoType::Report => Some(MetadataKind::ReportObject),
            _ => None,
        }
    }

    /// Sibling of [`Self::object_kind_for`] for register record-set modules.
    ///
    /// Returns the `*RecordSet` companion kind for the four register
    /// [`MdoType`] flavours. Non-register flavours return `None`.
    pub fn record_set_kind_for(mdo_type: MdoType) -> Option<Self> {
        match mdo_type {
            MdoType::InformationRegister => Some(MetadataKind::InformationRegisterRecordSet),
            MdoType::AccumulationRegister => Some(MetadataKind::AccumulationRegisterRecordSet),
            MdoType::AccountingRegister => Some(MetadataKind::AccountingRegisterRecordSet),
            MdoType::CalculationRegister => Some(MetadataKind::CalculationRegisterRecordSet),
            _ => None,
        }
    }

    /// English prefix under which this kind's platform methods and properties
    /// are indexed in `bsl-platform` (`CatalogObject`, `CatalogRef`, …).
    ///
    /// `None` for kinds without a composite platform surface today
    /// (dimensions, resources, attributes, tabular sections, and scalar
    /// synthetic receivers). Single source of truth shared by semantic method
    /// resolution, property enumeration, and dot-completion on metadata
    /// receivers.
    pub fn platform_prefix(self) -> Option<&'static str> {
        match self {
            Self::CatalogObject => Some("CatalogObject"),
            Self::CatalogRef => Some("CatalogRef"),
            Self::DocumentObject => Some("DocumentObject"),
            Self::DocumentRef => Some("DocumentRef"),
            Self::EnumRef => Some("EnumRef"),
            Self::TaskRef => Some("TaskRef"),
            Self::TaskObject => Some("TaskObject"),
            Self::BusinessProcessRef => Some("BusinessProcessRef"),
            Self::BusinessProcessObject => Some("BusinessProcessObject"),
            Self::DataProcessorObject => Some("DataProcessorObject"),
            Self::ReportObject => Some("ReportObject"),
            Self::ExchangePlanRef => Some("ExchangePlanRef"),
            Self::ExchangePlanObject => Some("ExchangePlanObject"),
            Self::ChartOfAccountsRef => Some("ChartOfAccountsRef"),
            Self::ChartOfAccountsObject => Some("ChartOfAccountsObject"),
            Self::InformationRegisterRecordManager => Some("InformationRegisterRecordManager"),
            Self::InformationRegisterRecordSet => Some("InformationRegisterRecordSet"),
            Self::AccumulationRegisterRecordSet => Some("AccumulationRegisterRecordSet"),
            Self::AccountingRegisterRecordSet => Some("AccountingRegisterRecordSet"),
            Self::CalculationRegisterRecordSet => Some("CalculationRegisterRecordSet"),
            Self::InformationRegisterRecord => Some("InformationRegisterRecord"),
            Self::AccumulationRegisterRecord => Some("AccumulationRegisterRecord"),
            Self::AccountingRegisterRecord => Some("AccountingRegisterRecord"),
            Self::CalculationRegisterRecord => Some("CalculationRegisterRecord"),
            Self::InformationRegisterRef
            | Self::AccumulationRegisterRef
            | Self::AccountingRegisterRef
            | Self::CalculationRegisterRef
            | Self::RegisterDimension { .. }
            | Self::RegisterResource { .. }
            | Self::RegisterAttribute { .. }
            | Self::RegisterFilter { .. }
            | Self::TabularSection { .. }
            | Self::TabularSectionRow { .. } => None,
        }
    }

    /// Scalar `type_name` under which this kind's methods/properties are
    /// indexed in `bsl-platform` when the surface is a flat platform type (no
    /// `"<Prefix>.<MDO>"` composite).
    ///
    /// Companion to [`Self::platform_prefix`]: `platform_prefix` covers
    /// composite indexing (`"CatalogObject.<Имя>"`); this method covers
    /// scalar indexing (`"Filter"`).
    pub fn scalar_platform_key(self) -> Option<&'static str> {
        match self {
            Self::RegisterFilter { .. } => Some("Filter"),
            _ => None,
        }
    }

    /// User-facing label for this kind in the chosen locale.
    ///
    /// The current callers pass `base_db::Locale`; this layer intentionally
    /// avoids depending on `base-db`, so locale is accepted by debug label and
    /// `Debug == "En"` selects English. All other values use Russian, matching
    /// `base_db::Locale`'s default.
    pub fn display_label(self, locale: impl std::fmt::Debug) -> &'static str {
        let is_en = format!("{locale:?}") == "En";
        match (self, is_en) {
            (Self::CatalogRef, false) => "СправочникСсылка",
            (Self::CatalogRef, true) => "CatalogRef",
            (Self::CatalogObject, false) => "СправочникОбъект",
            (Self::CatalogObject, true) => "CatalogObject",
            (Self::DocumentRef, false) => "ДокументСсылка",
            (Self::DocumentRef, true) => "DocumentRef",
            (Self::DocumentObject, false) => "ДокументОбъект",
            (Self::DocumentObject, true) => "DocumentObject",
            (Self::EnumRef, false) => "ПеречислениеСсылка",
            (Self::EnumRef, true) => "EnumRef",
            (Self::TaskRef, false) => "ЗадачаСсылка",
            (Self::TaskRef, true) => "TaskRef",
            (Self::TaskObject, false) => "ЗадачаОбъект",
            (Self::TaskObject, true) => "TaskObject",
            (Self::BusinessProcessRef, false) => "БизнесПроцессСсылка",
            (Self::BusinessProcessRef, true) => "BusinessProcessRef",
            (Self::BusinessProcessObject, false) => "БизнесПроцессОбъект",
            (Self::BusinessProcessObject, true) => "BusinessProcessObject",
            (Self::DataProcessorObject, false) => "ОбработкаОбъект",
            (Self::DataProcessorObject, true) => "DataProcessorObject",
            (Self::ReportObject, false) => "ОтчётОбъект",
            (Self::ReportObject, true) => "ReportObject",
            (Self::ExchangePlanRef, false) => "ПланОбменаСсылка",
            (Self::ExchangePlanRef, true) => "ExchangePlanRef",
            (Self::ExchangePlanObject, false) => "ПланОбменаОбъект",
            (Self::ExchangePlanObject, true) => "ExchangePlanObject",
            (Self::ChartOfAccountsRef, false) => "ПланСчетовСсылка",
            (Self::ChartOfAccountsRef, true) => "ChartOfAccountsRef",
            (Self::ChartOfAccountsObject, false) => "ПланСчетовОбъект",
            (Self::ChartOfAccountsObject, true) => "ChartOfAccountsObject",
            (Self::InformationRegisterRef, false) => "РегистрСведенийКлючЗаписи",
            (Self::InformationRegisterRef, true) => "InformationRegisterRef",
            (Self::InformationRegisterRecordManager, false) => "РегистрСведенийМенеджерЗаписи",
            (Self::InformationRegisterRecordManager, true) => "InformationRegisterRecordManager",
            (Self::InformationRegisterRecordSet, false) => "РегистрСведенийНаборЗаписей",
            (Self::InformationRegisterRecordSet, true) => "InformationRegisterRecordSet",
            (Self::InformationRegisterRecord, false) => "РегистрСведенийЗапись",
            (Self::InformationRegisterRecord, true) => "InformationRegisterRecord",
            (Self::AccumulationRegisterRef, false) => "РегистрНакопленияКлючЗаписи",
            (Self::AccumulationRegisterRef, true) => "AccumulationRegisterRef",
            (Self::AccumulationRegisterRecordSet, false) => "РегистрНакопленияНаборЗаписей",
            (Self::AccumulationRegisterRecordSet, true) => "AccumulationRegisterRecordSet",
            (Self::AccumulationRegisterRecord, false) => "РегистрНакопленияЗапись",
            (Self::AccumulationRegisterRecord, true) => "AccumulationRegisterRecord",
            (Self::AccountingRegisterRef, false) => "РегистрБухгалтерииКлючЗаписи",
            (Self::AccountingRegisterRef, true) => "AccountingRegisterRef",
            (Self::AccountingRegisterRecordSet, false) => "РегистрБухгалтерииНаборЗаписей",
            (Self::AccountingRegisterRecordSet, true) => "AccountingRegisterRecordSet",
            (Self::AccountingRegisterRecord, false) => "РегистрБухгалтерииЗапись",
            (Self::AccountingRegisterRecord, true) => "AccountingRegisterRecord",
            (Self::CalculationRegisterRef, false) => "РегистрРасчётаКлючЗаписи",
            (Self::CalculationRegisterRef, true) => "CalculationRegisterRef",
            (Self::CalculationRegisterRecordSet, false) => "РегистрРасчетаНаборЗаписей",
            (Self::CalculationRegisterRecordSet, true) => "CalculationRegisterRecordSet",
            (Self::CalculationRegisterRecord, false) => "РегистрРасчетаЗапись",
            (Self::CalculationRegisterRecord, true) => "CalculationRegisterRecord",
            (Self::RegisterDimension { .. }, false) => "Измерение",
            (Self::RegisterDimension { .. }, true) => "Dimension",
            (Self::RegisterResource { .. }, false) => "Ресурс",
            (Self::RegisterResource { .. }, true) => "Resource",
            (Self::RegisterAttribute { .. }, false) => "Реквизит",
            (Self::RegisterAttribute { .. }, true) => "Attribute",
            (Self::RegisterFilter { .. }, false) => "Отбор",
            (Self::RegisterFilter { .. }, true) => "Filter",
            (Self::TabularSection { .. }, false) => "ТабличнаяЧасть",
            (Self::TabularSection { .. }, true) => "TabularSection",
            (Self::TabularSectionRow { .. }, false) => "СтрокаТабличнойЧасти",
            (Self::TabularSectionRow { .. }, true) => "TabularSectionRow",
        }
    }
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
    /// Optional display-only shadow with pre-rendered SDBL type labels,
    /// indexed parallel to `fields`.
    ///
    /// `None` when the originating bridge wasn't given an SDBL package
    /// (e.g. hand-built `ValueTable` literals, form-attribute
    /// projections). `Some(slice)` invariant: `slice.len() == fields.len()`.
    ///
    /// Phase 3 §4.D: introduced so the `Ty ↔ TypeId` bridge preserves
    /// hover-rendered precision (`Число(15,2)`, `Строка(50)`) across the
    /// inference storage migration.
    pub raw_sdbl_types: Option<Arc<[crate::facet::SdblTypeShadowFacet]>>,
}

impl Projection {
    /// Constructor for callers outside `bsl-types`. The struct is
    /// `#[non_exhaustive]` so this is the only forward-compatible
    /// construction surface (adding new fields stays additive).
    pub fn new(
        fields: Arc<[ProjectionField]>,
        origin: ProjectionOrigin,
        raw_sdbl_types: Option<Arc<[crate::facet::SdblTypeShadowFacet]>>,
    ) -> Self {
        Self { fields, origin, raw_sdbl_types }
    }
}

impl ProjectionField {
    /// Constructor — `ProjectionField` is `#[non_exhaustive]` so external
    /// callers (e.g. the `hir-ty` bridge) need this to construct values.
    pub fn new(name: Name, ty: TypeId, source: ProjectionFieldSource) -> Self {
        Self { name, ty, source }
    }
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

    // ── Form-specific shapes ──────────────────────────────────
    /// Form data wrapper (`ДанныеФормыСтруктура`, collection, or mixed).
    FormData {
        kind: FormDataFacet,
        underlying: Option<MdoRefFacet>,
    },
    /// Form control with optional resolved data binding.
    FormControl {
        kind: FormElementFacet,
        binding: Option<FormBindingFacet>,
    },
    /// Contextual `ЭтотОбъект` value inside an object module.
    ThisObject {
        config_id: ConfigId,
        owner: MdoRefFacet,
    },
    /// Contextual `ЭтотМенеджер` receiver inside a manager module.
    ThisManager {
        config_id: ConfigId,
        owner: MdoRefFacet,
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
    ObjectManager(ManagerFacet),

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
