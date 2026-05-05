//! Type system for BSL.
//!
//! This module provides basic type information for BSL values and expressions.
//! Full type inference is planned for later iterations (12+).

pub mod doc_types;

use std::sync::Arc;

use bsl_metadata::MdoType;
use syntax::ast::{self, AstNode};
use syntax::SyntaxKind;

/// BSL type representation.
///
/// Represents the type of a BSL value or expression.
/// For Iteration 8, we support basic literal types and Unknown for everything else.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Ty {
    /// Unknown type (default for complex expressions).
    #[default]
    Unknown,

    /// Number (Число).
    /// BSL doesn't distinguish between integers and floats.
    Number,

    /// String (Строка).
    String,

    /// Boolean (Булево).
    Boolean,

    /// Date (Дата).
    Date,

    /// Undefined (Неопределено).
    Undefined,

    /// Null (NULL).
    Null,

    /// Array (Массив).
    Array,

    /// Structure (Структура).
    Structure,

    /// Map (Соответствие).
    Map,

    /// Type descriptor (returned by ТипЗнч, used in type checks).
    Type,

    /// ValueTable (ТаблицаЗначений).
    ValueTable,

    /// ValueList (СписокЗначений).
    ValueList,

    /// Metadata object reference (Справочники.Товары, etc.).
    ///
    /// Represents a reference to a metadata object like Catalog, Document, Register.
    /// Only populated after type inference with metadata integration (Phase 5).
    MetadataRef { kind: MetadataKind, name: crate::Name },

    /// Collective manager global (`Документы`, `Справочники`, …).
    ///
    /// `Документы` resolves to `Ty::ManagerCollection(MdoType::Document)`. The
    /// variant is an intermediate step: chaining a single-segment member
    /// (`Документы.ПКО`) lowers to [`Ty::ObjectManager`], and a further method
    /// call (`Документы.ПКО.СоздатьДокумент()`) lowers to [`Ty::MetadataRef`].
    ///
    /// `MdoType::from_plural` in `bsl-metadata` is the canonical mapping from
    /// the surface name to this variant; do not duplicate that table.
    ManagerCollection(MdoType),

    /// Concrete manager bound to a metadata object name (`Документы.ПКО`).
    ///
    /// Produced when a [`Ty::ManagerCollection`] is indexed by a member name
    /// that resolves against the workspace configuration. `name` is the
    /// metadata object's identifier as it appears in the configuration (e.g.
    /// `Name::new("ПКО")`); the head [`MdoType`] disambiguates the manager
    /// family so downstream method lookup can find the right manager-module
    /// table.
    ObjectManager { kind: MdoType, name: crate::Name },

    /// Implicit receiver bound to `ЭтотОбъект` / `ThisObject`.
    ///
    /// `owner` pins the enclosing module's MDO — `(MdoType::Catalog,
    /// Name::new("Номенклатура"))` for an `ObjectModule` under
    /// `Catalogs/Номенклатура/`. The variant is deliberately distinct from
    /// [`Self::MetadataRef`]: keeping the `ThisObject` provenance preserves
    /// information diagnostics like `BodyDiagnostic::RedundantAccessToObject`
    /// and future rename/refactor features need — collapsing to
    /// `MetadataRef { CatalogObject, … }` at the Ty level would erase the
    /// "explicitly self-referential" signal.
    ///
    /// Downstream adapters ([`crate::ty::Ty`] is consumed by `hir-ty`'s
    /// `field_lookup` / `method_lookup`) coerce `ThisObject` to the right
    /// `MetadataRef { *Object, name }` at their entry point, so field and
    /// method lookup on `ЭтотОбъект` resolves transparently. Coercion is
    /// scoped to MDO kinds that have an `*Object` companion
    /// ([`MetadataKind::CatalogObject`], [`MetadataKind::DocumentObject`],
    /// [`MetadataKind::ExchangePlanObject`],
    /// [`MetadataKind::ChartOfAccountsObject`]); other module kinds (form
    /// modules, record-set modules, common modules) fall through to
    /// `Ty::Unknown` until follow-up PRs cover their receiver surfaces.
    ThisObject {
        /// `(kind, name)` of the MDO that owns the module in which
        /// `ЭтотОбъект` appears.
        owner: (MdoType, crate::Name),
    },

    /// Managed-form attribute receiver — the platform wrapper that exposes
    /// a form attribute (`Объект`, `Замечание`, `ТаблицаРасходов`) to
    /// FormModule code.
    ///
    /// Why a dedicated variant rather than reusing [`Self::ThisObject`] for
    /// the main attribute (`Объект`)? Method lookup on a `ThisObject` falls
    /// through to `MetadataRef { *Object, … }` via coercion in
    /// `hir-ty::this_object`. That coercion would expose `Записать()`,
    /// `ЗаблокироватьДанныеДляРедактирования()`, and other object-level
    /// methods that the platform deliberately blocks on the form-data
    /// wrapper — `Объект.Записать()` inside a managed form is a runtime
    /// error, not a valid call. Routing methods through
    /// `ДанныеФормыСтруктура` / `ДанныеФормыКоллекция` /
    /// `ДанныеФормыСтруктураСКоллекцией` (per [`FormDataKind`]) closes that
    /// gap.
    ///
    /// `underlying` carries the `(MdoType, Name)` that the form attribute
    /// projects when present (`<MainAttribute>true</...>` typed as
    /// `cfg:CatalogObject.X`). Field lookup on
    /// `Ty::FormData { Structure, underlying: Some((mdo, name)), .. }`
    /// enumerates the MDO's attributes — `Объект.Дата` resolves through the
    /// catalog/document attribute table just like `ЭтотОбъект.Дата` would
    /// in an object module. `None` covers `ValueTable`-typed attributes
    /// (the schema lives in `<Columns>`) and any future form attribute
    /// without a backing MDO.
    FormData {
        /// Platform wrapper kind — picks the right method table in
        /// `bsl-platform::PlatformDataInner`.
        kind: FormDataKind,
        /// Optional MDO behind a `Structure` / `StructureWithCollection`
        /// projection — `Some` for `<MainAttribute>` typed as
        /// `cfg:CatalogObject.X` / `cfg:DocumentObject.Y` / etc.
        underlying: Option<(MdoType, crate::Name)>,
    },

    /// Function or procedure type — used internally to carry a
    /// signature through call resolution. **BSL has no first-class
    /// function values**: a bare identifier without parentheses cannot
    /// evaluate to a function, so `infer_path_name` never produces this
    /// shape from a `Expr::Path`. The variant exists for the
    /// `Expr::Call` callee path (see the `Expr::Path` callee arm in
    /// `hir_ty::infer::infer_call`) and for tests that synthesise
    /// signatures directly.
    ///
    /// `params` are the declared parameter types (positional), `defaults` is
    /// a parallel mask where `true` at index `i` means parameter `i` has a
    /// default value (i.e. is optional at the call site), `max_args` is the
    /// hard upper bound on caller-supplied arguments (`Some(M)` caps at `M`,
    /// `None` means unbounded — true variadic), and `ret` is the return type
    /// (`Undefined` for procedures).
    ///
    /// `defaults.len() == params.len()` is an invariant on the constructor
    /// path; consumers may rely on it.
    Function { params: Box<[Ty]>, defaults: Box<[bool]>, max_args: Option<u32>, ret: Box<Ty> },

    /// Platform object type not covered by specific Ty variants.
    ///
    /// Represents platform types like Запрос, РезультатЗапроса, HTTPЗапрос, etc.
    /// The name is stored as-is from the constructor (e.g., `Новый Запрос` → "Запрос").
    /// Platform data lookup is case-insensitive and bilingual.
    PlatformObject(crate::Name),

    /// Union of types — `Ty::Union([A, B, …])`.
    ///
    /// Sources:
    /// - XML `AttributeType::Composite { types }` (describing `ОписаниеТипов`
    ///   attributes that can hold one of several types).
    /// - JSDoc `// Возвращаемое значение: Число, Строка` (M3 Task 4 parser).
    /// - Future narrowing results (M4).
    ///
    /// **Constructed only via [`Ty::union`]**: the smart constructor flattens
    /// nested unions, deduplicates by structural equality, and imposes a
    /// stable order so `Ty::union([A, B])` compares equal to
    /// `Ty::union([B, A])`. Bypassing it breaks `PartialEq` commutativity and
    /// Salsa's cache can then store "equal" unions under two different keys.
    ///
    /// Narrowing (`Если ТипЗнч(X) = Тип("Массив")`) is M4; today Unions are
    /// opaque at inference — `platform_type_name()` returns `None`, and
    /// `Expr::Field` / `Expr::MethodCall` on a union give `Ty::Unknown` until
    /// a narrowing step selects a concrete component.
    Union(Arc<[Ty]>),
}

/// Managed-form data wrapper flavour.
///
/// Picks the platform type whose method table backs a [`Ty::FormData`]
/// receiver:
///
/// | Variant | Platform type | When chosen |
/// |---------|---------------|-------------|
/// | [`Self::Structure`] | `ДанныеФормыСтруктура` / `FormDataStructure` | scalar / `<MainAttribute>` form attribute (e.g. `Объект` typed as `cfg:DocumentObject.X`) |
/// | [`Self::Collection`] | `ДанныеФормыКоллекция` / `FormDataCollection` | `ValueTable`-typed attribute with `<Columns>` (table inside the form) |
/// | [`Self::StructureWithCollection`] | `ДанныеФормыСтруктураСКоллекцией` / `FormDataStructureAndCollection` | object-typed attribute that also exposes table parts (covers `Объект` for documents/catalogs with tabular sections — the platform composite that exposes both fields and tabular collections) |
///
/// The platform type names live in `bsl-platform/data/platform_data.json`;
/// `Display::fmt` and `platform_type_name` map this enum to those names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FormDataKind {
    /// Plain form-data structure (no nested collections).
    Structure,
    /// Form-data collection (`ValueTable` attribute).
    Collection,
    /// Form-data structure that also has nested form-data collections.
    StructureWithCollection,
}

impl FormDataKind {
    /// Russian platform type name for method/property lookup.
    pub fn platform_type_name(self) -> &'static str {
        match self {
            Self::Structure => "ДанныеФормыСтруктура",
            Self::Collection => "ДанныеФормыКоллекция",
            Self::StructureWithCollection => "ДанныеФормыСтруктураСКоллекцией",
        }
    }
}

/// Metadata object kind.
///
/// Classifies the flavour of MDO (or MDO fragment) that a [`Ty::MetadataRef`]
/// carries. The `name` of the enclosing `MetadataRef` is the MDO identifier as
/// it appears in the configuration (`"ПКО"`, `"Номенклатура"`). For
/// [`Self::TabularSection`] / [`Self::TabularSectionRow`] the name encodes
/// `"Parent.Section"` (e.g. `"ПКО.Товары"`) — parent MDO first, tabular
/// section name second — and the variant **also** carries the parent
/// [`MdoType`], so callers never have to probe several candidates to
/// disambiguate `Catalog "X"` from `Document "X"` with an identically
/// named section.
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
    /// through this kind via [`record_set_kind_to_mdo`].
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
    /// Business process reference (БизнесПроцессСсылка).
    BusinessProcessRef,
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
    /// Used by [`crate::ty::Ty::MetadataRef`] as a fallback when the
    /// XML-parsed `attr_type` on a [`bsl_metadata::register::Dimension`]
    /// is absent and the field-lookup adapter has no concrete `Ty` to
    /// return. `parent` pins the register flavour
    /// (`InformationRegister`, `AccumulationRegister`,
    /// `AccountingRegister`, `CalculationRegister`) so downstream
    /// tooling can still surface "dimension of register X" even without
    /// a typed payload. The enclosing `MetadataRef`'s name carries
    /// `"Register.Dimension"` (mirrors the `TabularSection` convention)
    /// so the originating register is recoverable.
    ///
    /// Kept as a leaf receiver in M4 Task 2: further field access on
    /// this variant returns `None`. A follow-up that wires the
    /// `Движения.X.Добавить()` record surface can promote it to a
    /// receiver once `bsl-metadata` exposes the record shape.
    RegisterDimension {
        /// Register flavour that owns the dimension.
        parent: MdoType,
    },
    /// Resource (ресурс) of a register — opaque symbolic form.
    ///
    /// Same semantics as [`Self::RegisterDimension`]: fallback `Ty` when
    /// the XML-parsed `attr_type` is missing, `parent` pins the register
    /// flavour, name carries `"Register.Resource"`.
    RegisterResource {
        /// Register flavour that owns the resource.
        parent: MdoType,
    },
    /// Attribute (реквизит) of a register — opaque symbolic form.
    ///
    /// Same semantics as [`Self::RegisterDimension`]: fallback `Ty` when
    /// the XML-parsed `attr_type` is missing, `parent` pins the register
    /// flavour, name carries `"Register.Attribute"`.
    RegisterAttribute {
        /// Register flavour that owns the attribute.
        parent: MdoType,
    },
    /// Per-record-set `Отбор` (Filter) — synthetic receiver.
    ///
    /// 1С runtime exposes a `.Отбор` property on every register record
    /// set whose member NAMES are the owning register's dimensions and
    /// each member is a `ЭлементОтбора` (FilterItem). The HBK shipped
    /// in `bsl-platform/data/platform_data.json` does NOT declare this
    /// property on any RecordSet `type_name` (gap of the source
    /// archive, not a scraper bug), so we synthesize it here.
    ///
    /// `parent` pins the register flavour; the enclosing
    /// `Ty::MetadataRef.name` carries the bare register name (no
    /// `"Register.Filter"` composite — there is at most one `Отбор`
    /// per record-set so a second segment would be redundant).
    ///
    /// Method/property surface:
    /// - Members (dimensions) come from
    ///   [`crate::field_enum::enumerate_filter_fields`] in `hir-ty`.
    /// - The 10 platform `Filter` methods (`Сбросить`, `Получить`,
    ///   `Найти`, …) are wired through a scalar-key side channel
    ///   (`metadata_ref_scalar_key` returning `"Filter"`), NOT through
    ///   `platform_prefix` — `Filter` is a scalar `type_name`, not a
    ///   composite `Filter.<X>` prefix.
    RegisterFilter {
        /// Register flavour that owns the record set producing this
        /// `Отбор`.
        parent: MdoType,
    },
    /// Tabular section of a metadata object (`ТабличнаяЧасть`).
    ///
    /// `parent` identifies the MDO flavour that owns this section (Catalog,
    /// Document, BusinessProcess, Task, ChartOf*), and the name of the
    /// enclosing [`Ty::MetadataRef`] carries `"Parent.Section"` (e.g.
    /// `"ПКО.Товары"`). Together they pin one specific MDO: `Catalog "X"`
    /// and `Document "X"` with a shared tabular-section name resolve
    /// unambiguously because their [`Self::TabularSection`] receivers
    /// differ in `parent`.
    TabularSection {
        /// MDO flavour that owns the section (matches the parent's
        /// `mdo_type`).
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
    /// Single source of truth for the `ЭтотОбъект` coercion (Task 5)
    /// and any future callers that need to pick the right `*Object`
    /// `MetadataKind` given an [`MdoType`]. Keeping the mapping here
    /// rather than duplicating it at every call site lets the
    /// resolver gate `Ty::ThisObject { .. }` construction on the same
    /// set of flavours the field / method adapters actually coerce:
    /// producing a `ThisObject` for an MDO with no `*Object` surface
    /// would leave the receiver dangling (neither refusable by the
    /// resolver nor resolvable by the adapters).
    ///
    /// New `*Object` variants should update this method — the
    /// resolver's `resolve_this_object` and `hir-ty::this_object`
    /// adapter pick the result up automatically.
    pub fn object_kind_for(mdo_type: MdoType) -> Option<Self> {
        match mdo_type {
            MdoType::Catalog => Some(MetadataKind::CatalogObject),
            MdoType::Document => Some(MetadataKind::DocumentObject),
            MdoType::ExchangePlan => Some(MetadataKind::ExchangePlanObject),
            MdoType::ChartOfAccounts => Some(MetadataKind::ChartOfAccountsObject),
            _ => None,
        }
    }

    /// English prefix under which this kind's platform methods are
    /// indexed in `bsl-platform` (`CatalogObject`, `CatalogRef`, …).
    ///
    /// `None` for kinds without a platform surface today (register
    /// record-manager / record-set flavours, dimensions, resources,
    /// attributes, tabular sections). Single source of truth shared by:
    ///
    /// - `hir-ty::platform_manager_lookup::resolve_platform_metadata_ref_method`
    ///   (semantic method resolution);
    /// - `ide::completion::platform_completion` (dot-completion on
    ///   `Ty::MetadataRef` receivers).
    ///
    /// Keep in sync with the `MdoType` mapping inside
    /// `metadata_kind_to_prefix_and_mdo` — when a new kind grows a
    /// platform table, both places pick it up.
    pub fn platform_prefix(self) -> Option<&'static str> {
        match self {
            Self::CatalogObject => Some("CatalogObject"),
            Self::CatalogRef => Some("CatalogRef"),
            Self::DocumentObject => Some("DocumentObject"),
            Self::DocumentRef => Some("DocumentRef"),
            Self::EnumRef => Some("EnumRef"),
            Self::TaskRef => Some("TaskRef"),
            Self::BusinessProcessRef => Some("BusinessProcessRef"),
            Self::ExchangePlanRef => Some("ExchangePlanRef"),
            Self::ExchangePlanObject => Some("ExchangePlanObject"),
            Self::ChartOfAccountsRef => Some("ChartOfAccountsRef"),
            Self::ChartOfAccountsObject => Some("ChartOfAccountsObject"),
            // Register-record kinds: platform data indexes their
            // methods under `<Flavour>RecordManager.<Имя>` and
            // `<Flavour>RecordSet.<Имя>` composite typenames. Wired
            // here so platform calls (`Записать`, `Прочитать`,
            // `Загрузить`, …) on register-record receivers stay
            // resolvable now that `map_generic_metadata_return_type`
            // rebinds those return types to concrete `Ty::MetadataRef`
            // shapes.
            Self::InformationRegisterRecordManager => Some("InformationRegisterRecordManager"),
            Self::InformationRegisterRecordSet => Some("InformationRegisterRecordSet"),
            Self::AccumulationRegisterRecordSet => Some("AccumulationRegisterRecordSet"),
            Self::AccountingRegisterRecordSet => Some("AccountingRegisterRecordSet"),
            Self::CalculationRegisterRecordSet => Some("CalculationRegisterRecordSet"),
            // Per-record kinds: HBK indexes their methods under
            // `<Flavour>Record.<Имя>` (e.g. `InformationRegisterRecord`).
            // Yielded as the element type of `Для каждого … Из …` over
            // a register record-set; see `iteration_lookup` in hir-ty.
            Self::InformationRegisterRecord => Some("InformationRegisterRecord"),
            Self::AccumulationRegisterRecord => Some("AccumulationRegisterRecord"),
            Self::AccountingRegisterRecord => Some("AccountingRegisterRecord"),
            Self::CalculationRegisterRecord => Some("CalculationRegisterRecord"),
            // `RegisterFilter` is a synthetic Filter receiver. Its
            // platform methods (`Filter` scalar `type_name`) are routed
            // through a scalar-key side channel, not a composite
            // prefix, so `platform_prefix` returns `None` here.
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
    /// indexed in `bsl-platform` when the surface is a flat platform
    /// type (no `"<Prefix>.<MDO>"` composite).
    ///
    /// Companion to [`Self::platform_prefix`]: `platform_prefix` covers
    /// composite indexing (`"CatalogObject.<Имя>"`); this method covers
    /// scalar indexing (`"Filter"`). Used by:
    ///
    /// - `hir-ty::method_lookup::lookup_method` (after a
    ///   `resolve_platform_metadata_ref_method` miss);
    /// - `ide::completion::platform_completion` (dot-completion of
    ///   methods and properties on synthetic receivers).
    ///
    /// Returns `Some` only for synthetic kinds that wrap an existing
    /// scalar platform type. Today that is `RegisterFilter` → `"Filter"`
    /// (the 1С `Отбор` object on a register record-set, member-typed
    /// per-register but method-typed by the scalar `Filter` HBK row).
    pub fn scalar_platform_key(self) -> Option<&'static str> {
        match self {
            Self::RegisterFilter { .. } => Some("Filter"),
            _ => None,
        }
    }

    /// User-facing label for this kind in the chosen locale.
    ///
    /// Used by [`TyDisplay`] to render `Ty::MetadataRef { kind, name }` as
    /// a fully-qualified `"<label>.<name>"` (`"СправочникСсылка.Товары"` /
    /// `"CatalogRef.Товары"`). The English form matches
    /// [`Self::platform_prefix`] for kinds that have one, so hover and
    /// completion stay aligned with the canonical platform-data names that
    /// power method lookup.
    ///
    /// Parametric variants (`TabularSection { parent }`, `RegisterDimension { parent }`,
    /// …) carry the parent flavour in the value itself; the enclosing
    /// `Ty::MetadataRef`'s `name` already encodes the `"Parent.Section"`
    /// suffix, so the label stays focused on the kind tag and the full
    /// path is still visible as `{label}.{name}`.
    pub fn display_label(self, locale: base_db::Locale) -> &'static str {
        use base_db::Locale;
        match (self, locale) {
            (Self::CatalogRef, Locale::Ru) => "СправочникСсылка",
            (Self::CatalogRef, Locale::En) => "CatalogRef",
            (Self::CatalogObject, Locale::Ru) => "СправочникОбъект",
            (Self::CatalogObject, Locale::En) => "CatalogObject",
            (Self::DocumentRef, Locale::Ru) => "ДокументСсылка",
            (Self::DocumentRef, Locale::En) => "DocumentRef",
            (Self::DocumentObject, Locale::Ru) => "ДокументОбъект",
            (Self::DocumentObject, Locale::En) => "DocumentObject",
            (Self::EnumRef, Locale::Ru) => "ПеречислениеСсылка",
            (Self::EnumRef, Locale::En) => "EnumRef",
            (Self::TaskRef, Locale::Ru) => "ЗадачаСсылка",
            (Self::TaskRef, Locale::En) => "TaskRef",
            (Self::BusinessProcessRef, Locale::Ru) => "БизнесПроцессСсылка",
            (Self::BusinessProcessRef, Locale::En) => "BusinessProcessRef",
            (Self::ExchangePlanRef, Locale::Ru) => "ПланОбменаСсылка",
            (Self::ExchangePlanRef, Locale::En) => "ExchangePlanRef",
            (Self::ExchangePlanObject, Locale::Ru) => "ПланОбменаОбъект",
            (Self::ExchangePlanObject, Locale::En) => "ExchangePlanObject",
            (Self::ChartOfAccountsRef, Locale::Ru) => "ПланСчетовСсылка",
            (Self::ChartOfAccountsRef, Locale::En) => "ChartOfAccountsRef",
            (Self::ChartOfAccountsObject, Locale::Ru) => "ПланСчетовОбъект",
            (Self::ChartOfAccountsObject, Locale::En) => "ChartOfAccountsObject",
            (Self::InformationRegisterRef, Locale::Ru) => "РегистрСведенийКлючЗаписи",
            (Self::InformationRegisterRef, Locale::En) => "InformationRegisterRef",
            (Self::InformationRegisterRecordManager, Locale::Ru) => "РегистрСведенийМенеджерЗаписи",
            (Self::InformationRegisterRecordManager, Locale::En) => {
                "InformationRegisterRecordManager"
            }
            (Self::InformationRegisterRecordSet, Locale::Ru) => "РегистрСведенийНаборЗаписей",
            (Self::InformationRegisterRecordSet, Locale::En) => "InformationRegisterRecordSet",
            (Self::InformationRegisterRecord, Locale::Ru) => "РегистрСведенийЗапись",
            (Self::InformationRegisterRecord, Locale::En) => "InformationRegisterRecord",
            (Self::AccumulationRegisterRef, Locale::Ru) => "РегистрНакопленияКлючЗаписи",
            (Self::AccumulationRegisterRef, Locale::En) => "AccumulationRegisterRef",
            (Self::AccumulationRegisterRecordSet, Locale::Ru) => "РегистрНакопленияНаборЗаписей",
            (Self::AccumulationRegisterRecordSet, Locale::En) => "AccumulationRegisterRecordSet",
            (Self::AccumulationRegisterRecord, Locale::Ru) => "РегистрНакопленияЗапись",
            (Self::AccumulationRegisterRecord, Locale::En) => "AccumulationRegisterRecord",
            (Self::AccountingRegisterRef, Locale::Ru) => "РегистрБухгалтерииКлючЗаписи",
            (Self::AccountingRegisterRef, Locale::En) => "AccountingRegisterRef",
            (Self::AccountingRegisterRecordSet, Locale::Ru) => "РегистрБухгалтерииНаборЗаписей",
            (Self::AccountingRegisterRecordSet, Locale::En) => "AccountingRegisterRecordSet",
            (Self::AccountingRegisterRecord, Locale::Ru) => "РегистрБухгалтерииЗапись",
            (Self::AccountingRegisterRecord, Locale::En) => "AccountingRegisterRecord",
            (Self::CalculationRegisterRef, Locale::Ru) => "РегистрРасчётаКлючЗаписи",
            (Self::CalculationRegisterRef, Locale::En) => "CalculationRegisterRef",
            (Self::CalculationRegisterRecordSet, Locale::Ru) => "РегистрРасчетаНаборЗаписей",
            (Self::CalculationRegisterRecordSet, Locale::En) => "CalculationRegisterRecordSet",
            (Self::CalculationRegisterRecord, Locale::Ru) => "РегистрРасчетаЗапись",
            (Self::CalculationRegisterRecord, Locale::En) => "CalculationRegisterRecord",
            (Self::RegisterDimension { .. }, Locale::Ru) => "Измерение",
            (Self::RegisterDimension { .. }, Locale::En) => "Dimension",
            (Self::RegisterResource { .. }, Locale::Ru) => "Ресурс",
            (Self::RegisterResource { .. }, Locale::En) => "Resource",
            (Self::RegisterAttribute { .. }, Locale::Ru) => "Реквизит",
            (Self::RegisterAttribute { .. }, Locale::En) => "Attribute",
            (Self::RegisterFilter { .. }, Locale::Ru) => "Отбор",
            (Self::RegisterFilter { .. }, Locale::En) => "Filter",
            (Self::TabularSection { .. }, Locale::Ru) => "ТабличнаяЧасть",
            (Self::TabularSection { .. }, Locale::En) => "TabularSection",
            (Self::TabularSectionRow { .. }, Locale::Ru) => "СтрокаТабличнойЧасти",
            (Self::TabularSectionRow { .. }, Locale::En) => "TabularSectionRow",
        }
    }
}

impl Ty {
    /// Infer type from a literal AST node.
    ///
    /// Returns the type of the literal, or Unknown if inference fails.
    pub fn from_literal(literal: &ast::Literal) -> Self {
        // Extract the token from the literal node
        let token = literal.syntax().children_with_tokens().filter_map(|it| it.into_token()).next();

        if let Some(token) = token {
            match token.kind() {
                SyntaxKind::FLOAT | SyntaxKind::DECIMAL => Ty::Number,
                SyntaxKind::STRING => Ty::String,
                SyntaxKind::KW_TRUE | SyntaxKind::KW_FALSE => Ty::Boolean,
                SyntaxKind::DATE => Ty::Date,
                SyntaxKind::KW_UNDEFINED => Ty::Undefined,
                SyntaxKind::KW_NULL => Ty::Null,
                _ => Ty::Unknown,
            }
        } else {
            Ty::Unknown
        }
    }

    /// Infer type from a NewExpr (e.g., "Новый Массив").
    ///
    /// Returns the type based on the type name, or Unknown if not recognized.
    pub fn from_new_expr(new_expr: &ast::NewExpr) -> Self {
        // Find the type name token (IDENT after Новый/New)
        if let Some(type_name_token) = new_expr
            .syntax()
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|token| token.kind() == SyntaxKind::IDENT)
        {
            Self::from_type_name(type_name_token.text())
        } else {
            Ty::Unknown
        }
    }

    /// Infer type from a type name (e.g., "Массив", "Структура").
    ///
    /// Returns the corresponding type, or Unknown if not recognized.
    pub fn from_type_name(name: &str) -> Self {
        let name_lower = name.to_lowercase();
        match name_lower.as_str() {
            // Collection types
            "массив" | "array" => Ty::Array,
            "структура" | "structure" => Ty::Structure,
            "соответствие" | "map" => Ty::Map,

            // Primitive types
            "число" | "number" => Ty::Number,
            "строка" | "string" => Ty::String,
            "булево" | "boolean" => Ty::Boolean,
            "дата" | "date" => Ty::Date,

            // Platform types (NEW in Phase 1)
            "тип" | "type" => Ty::Type,
            "таблицазначений" | "valuetable" => Ty::ValueTable,
            "списокзначений" | "valuelist" => Ty::ValueList,

            // Bottom-ish markers that appear in platform return-type union
            // strings (e.g. "РезультатЗапроса, Неопределено"). Recognising
            // them as their canonical `Ty` lets `Ty::union(...)` dedup and
            // lets `lookup_method` strip them before chained dispatch.
            "неопределено" | "undefined" => Ty::Undefined,
            "null" => Ty::Null,

            _ => Ty::Unknown,
        }
    }

    /// Check if this type is Unknown.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Ty::Unknown)
    }

    /// Check if this type is a function or procedure.
    pub fn is_function(&self) -> bool {
        matches!(self, Ty::Function { .. })
    }

    /// Construct a [`Ty::ManagerCollection`] for a metadata kind.
    ///
    /// Returns `None` for `MdoType` values that have no manager form
    /// (`Cube`, `DimensionTable`, `CommonModule`) — lowering should never
    /// produce a collection for those, and the factory guards the invariant
    /// so downstream matches don't have to.
    pub fn manager_collection(kind: MdoType) -> Option<Self> {
        kind.manager_type_prefix().map(|_| Ty::ManagerCollection(kind))
    }

    /// Smart-constructor for [`Ty::Union`].
    ///
    /// Normalises the input to guarantee `PartialEq` commutativity on unions:
    ///
    /// 1. **Flatten** nested unions (`Union([Union([A, B]), C])` → `[A, B, C]`).
    /// 2. **Sort + dedup** by [`Ord`]. `Ty` derives a lexicographic total
    ///    order over its variants, so the result is a deterministic,
    ///    order-independent canonical form — `Ty::union([A, B])` and
    ///    `Ty::union([B, A])` compare equal under `PartialEq`.
    /// 3. **Collapse** singletons: `[]` → `Ty::Unknown`, `[x]` → `x`.
    ///
    /// `Ty::Unknown` inside a union is preserved — a union that mentions an
    /// unrecognised component is semantically different from one that doesn't.
    /// Narrowing (M4) may drop it after evaluating the `ТипЗнч` guard.
    pub fn union(types: Vec<Ty>) -> Ty {
        let mut flat: Vec<Ty> = Vec::with_capacity(types.len());
        for t in types {
            match t {
                Ty::Union(inner) => flat.extend(inner.iter().cloned()),
                other => flat.push(other),
            }
        }

        flat.sort();
        flat.dedup();

        match flat.len() {
            0 => Ty::Unknown,
            1 => flat.into_iter().next().unwrap(),
            _ => Ty::Union(flat.into()),
        }
    }

    /// Get a human-readable display name for this type in the given locale.
    ///
    /// Used by user-facing emitters (diagnostics, hover, completion details)
    /// to render primitive type names (`Число` / `Number`) and generic
    /// labels (`СсылкаМетаданных` / `MetadataRef`) in the user's
    /// language. For machine-internal callers (logs, tests, debug, platform
    /// method lookup) prefer [`Self::canonical_name`] which always returns
    /// the stable English label and lets the reader see at the call site
    /// that locale-independence is intentional.
    ///
    /// Lifetime is tied to `&self` rather than `'static` because the
    /// `Ty::PlatformObject` / `Ty::FormData` arms borrow from the type's
    /// own data — those carry their original BSL spelling (already either
    /// Russian or English depending on how the source / platform-data
    /// declared it) and don't switch on `locale`.
    pub fn display_name(&self, locale: base_db::Locale) -> &str {
        use base_db::Locale;
        match (self, locale) {
            (Ty::Unknown, _) => "Unknown",
            (Ty::Number, Locale::Ru) => "Число",
            (Ty::Number, Locale::En) => "Number",
            (Ty::String, Locale::Ru) => "Строка",
            (Ty::String, Locale::En) => "String",
            (Ty::Boolean, Locale::Ru) => "Булево",
            (Ty::Boolean, Locale::En) => "Boolean",
            (Ty::Date, Locale::Ru) => "Дата",
            (Ty::Date, Locale::En) => "Date",
            (Ty::Undefined, Locale::Ru) => "Неопределено",
            (Ty::Undefined, Locale::En) => "Undefined",
            (Ty::Null, _) => "Null",
            (Ty::Array, Locale::Ru) => "Массив",
            (Ty::Array, Locale::En) => "Array",
            (Ty::Structure, Locale::Ru) => "Структура",
            (Ty::Structure, Locale::En) => "Structure",
            (Ty::Map, Locale::Ru) => "Соответствие",
            (Ty::Map, Locale::En) => "Map",
            (Ty::Type, Locale::Ru) => "Тип",
            (Ty::Type, Locale::En) => "Type",
            (Ty::ValueTable, Locale::Ru) => "ТаблицаЗначений",
            (Ty::ValueTable, Locale::En) => "ValueTable",
            (Ty::ValueList, Locale::Ru) => "СписокЗначений",
            (Ty::ValueList, Locale::En) => "ValueList",
            (Ty::MetadataRef { .. }, Locale::Ru) => "СсылкаМетаданных",
            (Ty::MetadataRef { .. }, Locale::En) => "MetadataRef",
            (Ty::ManagerCollection(kind), Locale::Ru) => {
                kind.manager_type_prefix_ru().unwrap_or("МенеджерКоллекция")
            }
            (Ty::ManagerCollection(kind), Locale::En) => {
                // `manager_type_prefix` is the canonical display
                // ("DocumentManager", …). The [`Ty::manager_collection`]
                // factory rejects `MdoType`s without a manager form, so the
                // `None` branch is only reachable if a caller bypassed the
                // factory — surface a generic label rather than panic to
                // keep the type-system layer robust in the face of a
                // lowering bug.
                kind.manager_type_prefix().unwrap_or("ManagerCollection")
            }
            (Ty::ObjectManager { .. }, Locale::Ru) => "МенеджерОбъекта",
            (Ty::ObjectManager { .. }, Locale::En) => "ObjectManager",
            (Ty::ThisObject { .. }, Locale::Ru) => "ЭтотОбъект",
            (Ty::ThisObject { .. }, Locale::En) => "ThisObject",
            // FormData wrapper names live in `bsl-platform` (already locale-
            // agnostic; both `ДанныеФормыСтруктура` and the EN-named
            // `FormDataStructure` are valid platform identifiers, but
            // platform_data canonicalises to one form per type). Yielding
            // whatever the platform declared keeps method lookup happy.
            (Ty::FormData { kind, .. }, _) => kind.platform_type_name(),
            (Ty::Function { .. }, Locale::Ru) => "Функция",
            (Ty::Function { .. }, Locale::En) => "Function",
            (Ty::PlatformObject(name), _) => name.as_str(),
            // Coarse label mirrors `MetadataRef` / `ObjectManager`: the
            // member-by-member rendering lives on the [`TyDisplay`]
            // wrapper accessed via [`Self::display`], so callers that
            // need "Число | Строка" use `format!("{}", ty.display(locale))`
            // while APIs that only need a `&str` tag stay cheap.
            (Ty::Union(_), _) => "Union",
        }
    }

    /// Stable English machine-name for logs, tests, `Debug`, and platform
    /// method lookups.
    ///
    /// Equivalent to `display_name(Locale::En)`, but the dedicated name
    /// makes the EN-on-purpose intent visible at the call site — important
    /// in tests where mismatched assertions would otherwise look like a
    /// localization bug.
    pub fn canonical_name(&self) -> &str {
        self.display_name(base_db::Locale::En)
    }

    /// Locale-aware [`std::fmt::Display`]-able wrapper for `format!` /
    /// `write!` / `to_string()`.
    ///
    /// `Ty` itself does NOT implement `Display` (deliberate — see the
    /// migration in commit 2 of the bilingual-display refactor): a bare
    /// `format!("{}", ty)` cannot pick a locale and would silently leak
    /// English type names into Russian-IDE output. Callers must opt into a
    /// locale by going through `ty.display(locale)`, which makes the
    /// presentation choice explicit.
    pub fn display(&self, locale: base_db::Locale) -> TyDisplay<'_> {
        TyDisplay { ty: self, locale }
    }

    /// Get the platform type name for method/property lookup via bsl-platform.
    ///
    /// Returns a type name suitable for `PlatformDataInner::get_type_methods()`.
    /// The platform data accepts both Russian and English names, case-insensitive.
    /// Returns None for types without platform methods.
    pub fn platform_type_name(&self) -> Option<&str> {
        match self {
            Ty::Unknown | Ty::Undefined | Ty::Null | Ty::Function { .. } => None,
            // Manager globals (`Документы`, `Документы.ПКО`) are not platform
            // objects — method lookup goes through the MDO-specific tables in
            // `bsl-platform::get_manager_methods`, not through
            // `get_type_methods`. Returning `None` keeps the platform fallback
            // from surfacing spurious methods before M3 wires the dedicated
            // manager lookup.
            Ty::ManagerCollection(_) | Ty::ObjectManager { .. } => None,
            // `ThisObject` has no platform type on its own — it only
            // becomes a method receiver after adapters coerce it to the
            // matching `Ty::MetadataRef { *Object, name }`. Returning
            // `None` here keeps the platform fallback from surfacing
            // bogus methods before the coercion step runs.
            Ty::ThisObject { .. } => None,
            // Form data wraps the platform `ДанныеФормыСтруктура` /
            // `ДанныеФормыКоллекция` / `ДанныеФормыСтруктураСКоллекцией`
            // method tables. Method lookup goes through these names; field
            // lookup on a `Structure` / `StructureWithCollection` peels off
            // the wrapper through `underlying` (handled in `field_lookup`).
            Ty::FormData { kind, .. } => Some(kind.platform_type_name()),
            // Unions have no single platform type by construction — a
            // caller that wants methods on `Ty::Union([Number, String])`
            // must narrow first (M4) or intersect method tables explicitly.
            Ty::Union(_) => None,
            Ty::PlatformObject(name) => Some(name.as_str()),
            // Platform method lookup is keyed by canonical English type
            // names (`get_type_methods("Number")`), not localized ones —
            // platform_data.json stores both `name` (RU) and `english_name`
            // (EN) and the lookup index normalises both, so EN here is
            // a deliberate machine-name choice, not user-facing output.
            _ => Some(self.canonical_name()),
        }
    }
}

/// `Display`-able wrapper for [`Ty`] that knows the user-facing locale.
///
/// `Ty` deliberately does not implement [`std::fmt::Display`] directly so
/// that any `format!("{}", ty)` becomes a compile error and forces the
/// caller to specify a locale via [`Ty::display`]. That keeps presentation
/// choices explicit at the call site instead of silently leaking the
/// canonical English label into Russian-IDE output.
///
/// Constructed only via [`Ty::display`].
pub struct TyDisplay<'a> {
    ty: &'a Ty,
    locale: base_db::Locale,
}

impl std::fmt::Display for TyDisplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use base_db::Locale;
        match self.ty {
            // Render union members one by one so each component picks up the
            // same locale as the surrounding format. Member order is fixed
            // by `Ty::union`'s smart constructor (deterministic via `Ord`).
            Ty::Union(types) => {
                let mut it = types.iter();
                if let Some(first) = it.next() {
                    write!(f, "{}", first.display(self.locale))?;
                    for t in it {
                        write!(f, " | {}", t.display(self.locale))?;
                    }
                }
                Ok(())
            }
            // Fully-qualified MDO references — `СправочникСсылка.Товары` /
            // `CatalogRef.Товары`. The kind label switches per locale; the
            // `name` is whatever the source / metadata declared (typically
            // Russian) and stays as-is so it still matches the project's
            // identifier of record.
            Ty::MetadataRef { kind, name } => {
                write!(f, "{}.{}", kind.display_label(self.locale), name.as_str())
            }
            // Object manager — `Справочник.Товары` / `Catalog.Товары`. The
            // MDO flavour label uses `MdoType::russian_name` /
            // `MdoType::english_name`, mirroring the way `bsl-metadata`
            // already names the manager surface in either language.
            Ty::ObjectManager { kind, name } => {
                let label = match self.locale {
                    Locale::Ru => kind.russian_name(),
                    Locale::En => kind.english_name(),
                };
                write!(f, "{}.{}", label, name.as_str())
            }
            // Form-data wrapper — `ДанныеФормыСтруктура (ДокументОбъект.ПКО)`.
            // Wrapper name comes from `bsl-platform` (locale-agnostic
            // platform identifier); the parenthetical projects the
            // underlying MDO so the reader sees which catalog/document
            // fields are visible. The `*Object` flavour drops in via
            // `MetadataKind::object_kind_for`, identical to the surface a
            // manual `ЭтотОбъект` cast would expose.
            Ty::FormData { kind, underlying } => {
                let wrapper = kind.platform_type_name();
                match underlying
                    .as_ref()
                    .and_then(|(mdo, name)| MetadataKind::object_kind_for(*mdo).map(|k| (k, name)))
                {
                    Some((object_kind, name)) => write!(
                        f,
                        "{} ({}.{})",
                        wrapper,
                        object_kind.display_label(self.locale),
                        name.as_str(),
                    ),
                    None => f.write_str(wrapper),
                }
            }
            other => f.write_str(other.display_name(self.locale)),
        }
    }
}

/// Function or procedure signature.
///
/// Contains parameter types and return type. For procedures, the return type is `Undefined`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionSignature {
    /// Parameter types in declaration order.
    pub params: Box<[Ty]>,

    /// Per-parameter "has default value" flag (parallel to `params`).
    ///
    /// `true` at index `i` means parameter `i` is optional (declared with
    /// `= <expr>` in BSL source, or `IsOptional=true` in platform data).
    /// BSL does not require optional params to be trailing — a non-standard
    /// `Функция Foo(А, Б = ..., В)` is legal — so this is a per-index flag,
    /// not a single "first optional index".
    ///
    /// Length always equals `params.len()`. Defaults to all-`false`
    /// (everything required) for callers that don't know.
    pub defaults: Box<[bool]>,

    /// Return type (`Undefined` for procedures).
    pub ret: Box<Ty>,

    /// Maximum number of arguments the caller may supply.
    ///
    /// - `Some(M)` enforces a hard upper bound of `M` arguments. For a
    ///   fixed-arity signature this is `params.len()`; for a documented
    ///   variadic — `СтрШаблон(Шаблон, Значение1-Значение10)` — this is
    ///   the platform's documented cap (`1 + 10 = 11`).
    /// - `None` means no upper bound (truly unbounded variadic, e.g.
    ///   `Мин`/`Макс` or user helpers whose tail length the platform
    ///   never specified).
    ///
    /// The lower bound `args.len() >= required_count()` is unaffected.
    /// BSL has no syntactic `...`; the platform adapter recovers the cap
    /// from `MethodParam::is_variadic` (explicit flag, → `None`) or, as a
    /// fallback, the platform-help idiom of naming the tail slot
    /// `<name>1-<name>N` (→ `Some(N)` after offset).
    pub max_args: Option<u32>,
}

impl FunctionSignature {
    /// Create a new function signature with all parameters required and a
    /// fixed-arity upper bound (`max_args = Some(params.len())`).
    pub fn new(params: Vec<Ty>, ret: Ty) -> Self {
        let max_args = Some(params.len() as u32);
        let defaults = vec![false; params.len()].into_boxed_slice();
        Self { params: params.into_boxed_slice(), defaults, ret: Box::new(ret), max_args }
    }

    /// Create a new function signature with explicit per-parameter defaults
    /// and a fixed-arity upper bound (`max_args = Some(params.len())`).
    ///
    /// Panics in debug if `params.len() != defaults.len()`.
    pub fn new_with_defaults(params: Vec<Ty>, defaults: Vec<bool>, ret: Ty) -> Self {
        debug_assert_eq!(
            params.len(),
            defaults.len(),
            "FunctionSignature::new_with_defaults: params/defaults length mismatch"
        );
        let max_args = Some(params.len() as u32);
        Self {
            params: params.into_boxed_slice(),
            defaults: defaults.into_boxed_slice(),
            ret: Box::new(ret),
            max_args,
        }
    }

    /// Create a procedure signature (returns Undefined). All params required.
    pub fn procedure(params: Vec<Ty>) -> Self {
        Self::new(params, Ty::Undefined)
    }

    /// Create a function signature with known return type. All params required.
    pub fn function(params: Vec<Ty>, ret: Ty) -> Self {
        Self::new(params, ret)
    }

    /// Override the upper bound on argument count. `Some(M)` caps at `M`;
    /// `None` means unbounded (truly variadic with no documented limit).
    pub fn with_max_args(mut self, max_args: Option<u32>) -> Self {
        self.max_args = max_args;
        self
    }

    /// Number of arguments that the caller MUST supply.
    ///
    /// Computed as `last_required_index + 1` — the position of the last
    /// non-default parameter plus one. For all-required signatures this
    /// equals `params.len()`; for all-optional signatures it is `0`.
    /// Non-standard mixed orders (e.g. `(А, Б = ..., В)`) yield the
    /// strictly-correct `3`, not `1`.
    pub fn required_count(&self) -> usize {
        self.defaults.iter().rposition(|has_default| !has_default).map_or(0, |i| i + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_name_inference_russian() {
        assert_eq!(Ty::from_type_name("Массив"), Ty::Array);
        assert_eq!(Ty::from_type_name("Структура"), Ty::Structure);
        assert_eq!(Ty::from_type_name("Соответствие"), Ty::Map);
        assert_eq!(Ty::from_type_name("Число"), Ty::Number);
        assert_eq!(Ty::from_type_name("Строка"), Ty::String);
        assert_eq!(Ty::from_type_name("Булево"), Ty::Boolean);
        assert_eq!(Ty::from_type_name("Дата"), Ty::Date);
    }

    #[test]
    fn test_type_name_inference_english() {
        assert_eq!(Ty::from_type_name("Array"), Ty::Array);
        assert_eq!(Ty::from_type_name("Structure"), Ty::Structure);
        assert_eq!(Ty::from_type_name("Map"), Ty::Map);
        assert_eq!(Ty::from_type_name("Number"), Ty::Number);
        assert_eq!(Ty::from_type_name("String"), Ty::String);
        assert_eq!(Ty::from_type_name("Boolean"), Ty::Boolean);
        assert_eq!(Ty::from_type_name("Date"), Ty::Date);
    }

    #[test]
    fn test_type_name_case_insensitive() {
        assert_eq!(Ty::from_type_name("МАССИВ"), Ty::Array);
        assert_eq!(Ty::from_type_name("массив"), Ty::Array);
        assert_eq!(Ty::from_type_name("МаССиВ"), Ty::Array);
        assert_eq!(Ty::from_type_name("array"), Ty::Array);
        assert_eq!(Ty::from_type_name("ARRAY"), Ty::Array);
    }

    #[test]
    fn test_type_name_unknown() {
        assert_eq!(Ty::from_type_name("UnknownType"), Ty::Unknown);
        assert_eq!(Ty::from_type_name("НеизвестныйТип"), Ty::Unknown);
        assert_eq!(Ty::from_type_name(""), Ty::Unknown);
    }

    #[test]
    fn test_display_name() {
        // Stable English canonical labels — pinned by `canonical_name()`.
        assert_eq!(Ty::Number.canonical_name(), "Number");
        assert_eq!(Ty::String.canonical_name(), "String");
        assert_eq!(Ty::Boolean.canonical_name(), "Boolean");
        assert_eq!(Ty::Unknown.canonical_name(), "Unknown");
        assert_eq!(Ty::Array.canonical_name(), "Array");
        assert_eq!(
            Ty::Function {
                params: Box::new([]),
                defaults: Box::new([]),
                max_args: Some(0),
                ret: Box::new(Ty::Undefined),
            }
            .canonical_name(),
            "Function"
        );
    }

    #[test]
    fn display_name_localizes_primitives() {
        use base_db::Locale;
        assert_eq!(Ty::Number.display_name(Locale::Ru), "Число");
        assert_eq!(Ty::String.display_name(Locale::Ru), "Строка");
        assert_eq!(Ty::Boolean.display_name(Locale::Ru), "Булево");
        assert_eq!(Ty::Date.display_name(Locale::Ru), "Дата");
        assert_eq!(Ty::Type.display_name(Locale::Ru), "Тип");
        assert_eq!(Ty::Array.display_name(Locale::Ru), "Массив");
        assert_eq!(Ty::Structure.display_name(Locale::Ru), "Структура");
        assert_eq!(Ty::Map.display_name(Locale::Ru), "Соответствие");
        assert_eq!(Ty::Undefined.display_name(Locale::Ru), "Неопределено");
        assert_eq!(Ty::Null.display_name(Locale::Ru), "Null");
        assert_eq!(Ty::ValueTable.display_name(Locale::Ru), "ТаблицаЗначений");
        assert_eq!(Ty::ValueList.display_name(Locale::Ru), "СписокЗначений");

        // English side stays in lockstep with `canonical_name`.
        assert_eq!(Ty::Number.display_name(Locale::En), "Number");
        assert_eq!(Ty::Number.canonical_name(), Ty::Number.display_name(Locale::En));
    }

    #[test]
    fn display_name_localizes_manager_collection() {
        use base_db::Locale;
        let cat = Ty::manager_collection(MdoType::Catalog).expect("Catalog has a manager");
        assert_eq!(cat.display_name(Locale::En), "CatalogManager");
        assert_eq!(cat.display_name(Locale::Ru), "СправочникМенеджер");
    }

    #[test]
    fn ty_display_metadata_ref_localizes() {
        use crate::Name;
        use base_db::Locale;

        let ty =
            Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("Товары") };
        // RU: kind label switches; the source-declared `name` stays
        // verbatim so the IDE still pinpoints the catalog.
        assert_eq!(ty.display(Locale::Ru).to_string(), "СправочникСсылка.Товары");
        // EN: kind label flips to platform-prefix shape (`CatalogRef`).
        assert_eq!(ty.display(Locale::En).to_string(), "CatalogRef.Товары");
    }

    #[test]
    fn ty_display_object_manager_localizes() {
        use crate::Name;
        use base_db::Locale;

        let ty = Ty::ObjectManager { kind: MdoType::Catalog, name: Name::new("Товары") };
        assert_eq!(ty.display(Locale::Ru).to_string(), "Справочник.Товары");
        assert_eq!(ty.display(Locale::En).to_string(), "Catalog.Товары");
    }

    #[test]
    fn ty_display_union_localizes_each_arm() {
        use base_db::Locale;
        // Member order is fixed by `Ty::union` (deterministic via the derived
        // `Ord` on `Ty`): Number, String, Undefined under the current variant
        // declaration order. The localised label flips per locale, but the
        // ordering stays identical so callers (snapshots, hover) get a stable
        // string.
        let u = Ty::union(vec![Ty::Undefined, Ty::String, Ty::Number]);
        assert_eq!(u.display(Locale::Ru).to_string(), "Число | Строка | Неопределено");
        assert_eq!(u.display(Locale::En).to_string(), "Number | String | Undefined");
    }

    #[test]
    fn test_is_unknown() {
        assert!(Ty::Unknown.is_unknown());
        assert!(!Ty::Number.is_unknown());
        assert!(!Ty::String.is_unknown());
    }

    #[test]
    fn test_is_function() {
        assert!(Ty::Function {
            params: Box::new([]),
            defaults: Box::new([]),
            max_args: Some(0),
            ret: Box::new(Ty::Undefined),
        }
        .is_function());
        assert!(!Ty::Number.is_function());
        assert!(!Ty::Unknown.is_function());
    }

    #[test]
    fn test_default() {
        assert_eq!(Ty::default(), Ty::Unknown);
    }

    #[test]
    fn ty_display_manager_collection() {
        // Manager-collection canonical (English) name matches the platform
        // manager prefix in `bsl-metadata::MdoType::manager_type_prefix`, so
        // tests / logs see the same labels platform_data uses.
        let doc = Ty::manager_collection(MdoType::Document).expect("Document has a manager");
        assert_eq!(doc.canonical_name(), "DocumentManager");
        let cat = Ty::manager_collection(MdoType::Catalog).expect("Catalog has a manager");
        assert_eq!(cat.canonical_name(), "CatalogManager");
        let enm = Ty::manager_collection(MdoType::Enum).expect("Enum has a manager");
        assert_eq!(enm.canonical_name(), "EnumManager");
    }

    #[test]
    fn ty_manager_collection_factory_rejects_managerless_kinds() {
        // Invariant: MdoTypes without a manager form must never become a
        // `ManagerCollection`. The factory is the single construction site
        // through which lowering goes; direct enum construction is a smell
        // and should be rare.
        assert!(Ty::manager_collection(MdoType::CommonModule).is_none());
        assert!(Ty::manager_collection(MdoType::Cube).is_none());
        assert!(Ty::manager_collection(MdoType::DimensionTable).is_none());
    }

    #[test]
    fn ty_display_object_manager() {
        let ty = Ty::ObjectManager { kind: MdoType::Document, name: crate::Name::new("ПКО") };
        assert_eq!(ty.canonical_name(), "ObjectManager");
    }

    #[test]
    fn ty_manager_and_object_not_platform_objects() {
        // Manager globals deliberately bypass the platform-type-name fallback
        // so downstream IDE features route them through MDO-specific tables
        // once those land in M3.
        assert_eq!(Ty::ManagerCollection(MdoType::Document).platform_type_name(), None);
        let om =
            Ty::ObjectManager { kind: MdoType::Catalog, name: crate::Name::new("Товары") };
        assert_eq!(om.platform_type_name(), None);
    }

    #[test]
    fn ty_equality_object_manager_case_insensitive() {
        // `Name` equality is case-sensitive by design; two `ObjectManager`s
        // with differently-cased names must be distinct so the Salsa cache
        // and `expr_types` map don't fold `Документы.ПКО` with
        // `Документы.пко`. Case-insensitive lookup happens at resolver time,
        // not at Ty-level equality.
        let a = Ty::ObjectManager { kind: MdoType::Document, name: crate::Name::new("ПКО") };
        let b = Ty::ObjectManager { kind: MdoType::Document, name: crate::Name::new("пко") };
        assert_ne!(a, b);

        // Same name, different kind — also distinct.
        let c = Ty::ObjectManager { kind: MdoType::Catalog, name: crate::Name::new("ПКО") };
        assert_ne!(a, c);

        // Identical → equal.
        let d = Ty::ObjectManager { kind: MdoType::Document, name: crate::Name::new("ПКО") };
        assert_eq!(a, d);
    }

    #[test]
    fn ty_union_single_collapses() {
        // A one-element union is never built: the smart constructor unwraps
        // `[x]` to `x`. Guarantees `Ty::union(vec![Ty::Number])` compares
        // equal to `Ty::Number` itself.
        assert_eq!(Ty::union(vec![Ty::Number]), Ty::Number);
    }

    #[test]
    fn ty_union_empty_collapses_to_unknown() {
        // Empty union has no meaningful type — Unknown is the honest answer.
        assert_eq!(Ty::union(vec![]), Ty::Unknown);
    }

    #[test]
    fn ty_union_flatten_nested() {
        // Nested unions flatten so downstream consumers never have to recurse.
        let inner = Ty::union(vec![Ty::Number, Ty::String]);
        let outer = Ty::union(vec![inner, Ty::Boolean]);
        match outer {
            Ty::Union(ref parts) => assert_eq!(parts.len(), 3, "nested unions must flatten"),
            _ => panic!("expected Ty::Union, got {outer:?}"),
        }
    }

    #[test]
    fn ty_union_dedup() {
        // Duplicate components collapse so the same syntactic source doesn't
        // blow up the union size.
        let u = Ty::union(vec![Ty::Number, Ty::String, Ty::Number]);
        match u {
            Ty::Union(ref parts) => assert_eq!(parts.len(), 2, "dedup must collapse Number"),
            _ => panic!("expected Ty::Union, got {u:?}"),
        }
    }

    #[test]
    fn ty_union_equality_order_independent() {
        // PartialEq commutativity is the whole point of the smart constructor —
        // Salsa keys unions by `Eq`, so `[A, B]` and `[B, A]` must fold.
        let a = Ty::union(vec![Ty::Number, Ty::String]);
        let b = Ty::union(vec![Ty::String, Ty::Number]);
        assert_eq!(a, b);
    }

    #[test]
    fn ty_union_display_sorted() {
        // `TyDisplay` renders union members in the smart-constructor order
        // (deterministic via `Ord`). Pin the shape in English so the test is
        // language-agnostic; locale-specific component names are exercised
        // by `display_name_localizes_primitives` above.
        use base_db::Locale;
        let u = Ty::union(vec![Ty::String, Ty::Number]);
        let rendered = format!("{}", u.display(Locale::En));
        assert!(rendered.contains(" | "), "union render must join with ` | `, got {rendered:?}");
        assert!(rendered.contains("Number"));
        assert!(rendered.contains("String"));

        // Russian locale propagates into each component.
        let rendered_ru = format!("{}", u.display(Locale::Ru));
        assert!(rendered_ru.contains("Число"));
        assert!(rendered_ru.contains("Строка"));
    }

    #[test]
    fn ty_union_display_name_is_coarse_label() {
        // `canonical_name()` stays as a cheap `&str` tag; nuanced rendering
        // goes through `Ty::display(locale)`. Matches the MetadataRef /
        // ObjectManager pattern.
        assert_eq!(Ty::union(vec![Ty::Number, Ty::String]).canonical_name(), "Union");
    }

    #[test]
    fn ty_union_not_platform_object() {
        // No single platform type corresponds to a union — method lookup must
        // narrow first before consulting `bsl-platform`.
        assert_eq!(Ty::union(vec![Ty::Number, Ty::String]).platform_type_name(), None);
    }

    #[test]
    fn metadata_kind_object_kind_for_covers_ship_set() {
        // Ships the 4 `*Object` coercions used by `Ty::ThisObject`.
        // If one of these regresses, the resolver would stop promoting
        // `ЭтотОбъект` in that MDO's ObjectModule and hover / completion
        // would silently drop type information.
        assert_eq!(
            MetadataKind::object_kind_for(MdoType::Catalog),
            Some(MetadataKind::CatalogObject)
        );
        assert_eq!(
            MetadataKind::object_kind_for(MdoType::Document),
            Some(MetadataKind::DocumentObject)
        );
        assert_eq!(
            MetadataKind::object_kind_for(MdoType::ExchangePlan),
            Some(MetadataKind::ExchangePlanObject)
        );
        assert_eq!(
            MetadataKind::object_kind_for(MdoType::ChartOfAccounts),
            Some(MetadataKind::ChartOfAccountsObject)
        );
    }

    #[test]
    fn metadata_kind_object_kind_for_rejects_non_object_mdo_kinds() {
        // Register flavours, Task, BusinessProcess, and
        // ChartOfCharacteristicTypes have no `*Object` companion yet.
        // `resolve_this_object` keys off this to refuse promotion on
        // those kinds — so a regression that starts returning `Some`
        // here would let the resolver surface dangling `Ty::ThisObject`
        // receivers with no downstream coercion surface.
        assert!(MetadataKind::object_kind_for(MdoType::InformationRegister).is_none());
        assert!(MetadataKind::object_kind_for(MdoType::AccumulationRegister).is_none());
        assert!(MetadataKind::object_kind_for(MdoType::AccountingRegister).is_none());
        assert!(MetadataKind::object_kind_for(MdoType::CalculationRegister).is_none());
        assert!(MetadataKind::object_kind_for(MdoType::Task).is_none());
        assert!(MetadataKind::object_kind_for(MdoType::BusinessProcess).is_none());
        assert!(MetadataKind::object_kind_for(MdoType::Enum).is_none());
        assert!(MetadataKind::object_kind_for(MdoType::CommonModule).is_none());
    }
}
