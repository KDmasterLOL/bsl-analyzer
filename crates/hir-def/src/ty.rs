//! Type system for BSL.
//!
//! This module provides basic type information for BSL values and expressions.
//! Full type inference is planned for later iterations (12+).

pub mod doc_types;

use std::sync::Arc;

pub use bsl_metadata::FormElementKind;
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

    /// Array parameterised by its element type (`Массив из X`).
    ///
    /// Construction sites:
    /// - JSDoc `// Возвращаемое значение: Массив из Строка` lowers
    ///   `TypeRef::Array(Some(elem))` → `Ty::TypedArray(Box::new(<elem>))`
    ///   in `TyLoweringContext::lower_type_ref`.
    /// - Form-control refinement (`Элементы.X.ВыделенныеСтроки`)
    ///   produces `Ty::TypedArray(Box::new(row_ty))` so `.Количество()`
    ///   and iteration stay consistent across both surfaces.
    ///
    /// Method lookup keys this under `"Array"` (same as the legacy
    /// [`Self::Array`] variant), so `.Добавить()`, `.Количество()`, … all
    /// resolve through the bilingual `Массив` page. Iteration yields the
    /// inner element directly, bypassing the platform
    /// `iter_element_types` table whose only entry for `Массив` is
    /// `"Произвольный"` → `Ty::Unknown` — this is the whole point of
    /// carrying the element type.
    ///
    /// `Ty::Array` (unparameterised) stays as the lowering target for
    /// `Новый Массив(...)` literals and `TypeRef::Array(None)` forms,
    /// where no element type is recoverable. The two variants compare
    /// distinct under [`PartialEq`] / `Ord` so the smart constructor
    /// [`Self::union`] dedupes correctly.
    TypedArray(Box<Ty>),

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

    /// Managed-form UI control receiver — the platform wrapper that
    /// `Элементы.<имя>` resolves to inside FormModule code.
    ///
    /// `kind` is the coarse XML-tag taxonomy from `bsl-metadata`
    /// ([`FormElementKind`]); it picks the platform method table
    /// (`Table → ТаблицаФормы`, `Field → ПолеФормы`, `Button → КнопкаФормы`,
    /// `Group → ГруппаФормы`, `Decoration → ДекорацияФормы`,
    /// `Addition → ДополнениеЭлементаФормы`, `Other → нет таблицы`).
    ///
    /// `binding` carries the resolved `<DataPath>` provenance when the
    /// control's path traces back to a form attribute. Phase 5 uses it
    /// to refine `Элементы.Переприемка.ВыделенныеСтроки` into a
    /// [`Ty::TypedArray`] of the actual tabular-section row instead of
    /// the platform's bare `Массив`.
    /// `None` covers controls with no `<DataPath>`, with a deleted
    /// (`~`-prefixed) path, or whose first segment does not resolve to
    /// a form attribute. In that case method/property lookup still
    /// works through the platform `kind` table, but row-aware
    /// refinements degrade gracefully to `Unknown`.
    FormControl {
        /// Coarse XML-tag taxonomy — picks the platform method/property
        /// table for the control.
        kind: FormElementKind,
        /// Optional resolved DataPath provenance for row-aware refinement.
        binding: Option<FormDataBinding>,
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

/// Resolved DataPath provenance for a [`Ty::FormControl`] — the chain
/// of segments and what the chain's tail actually points at.
///
/// Constructed only by `hir-ty::form_items::resolve_data_path` after
/// walking `<DataPath>` through `Form::find_attribute` + `lookup_field`.
/// `path` is the original chain (case-preserving but case-insensitively
/// equal under [`Name`] folding); `target` is the lowering of the tail.
///
/// Carrying both lets hover render `«ТаблицаФормы (Объект.Переприемка)»`
/// without a second resolution pass, and lets Phase 5 row-aware lookup
/// distinguish `Объект.Переприемка` (TabularSection) from `Объект.Дата`
/// (scalar Attribute) without re-parsing the path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FormDataBinding {
    /// Segments of the original `<DataPath>`, in declaration order.
    /// **Invariant:** always non-empty. Enforced by the only
    /// constructor [`Self::new`]; the field is private so callers can
    /// not bypass it via struct-literal syntax.
    path: Box<[crate::Name]>,
    /// What the chain's tail resolves to.
    target: FormDataTarget,
}

impl FormDataBinding {
    /// Construct a binding, enforcing the non-empty-`path` invariant.
    /// Returns `None` for an empty path so callers must surface
    /// `binding: None` on the enclosing [`Ty::FormControl`] rather
    /// than carry a vacuous binding — `TyDisplay` would then render
    /// a bare `«ТаблицаФормы ()»` with no provenance.
    ///
    /// This is the **only** constructor: the struct's fields are
    /// private to keep the invariant enforced from every call site,
    /// including future ones in `hir-ty` and tests.
    pub fn new(path: Box<[crate::Name]>, target: FormDataTarget) -> Option<Self> {
        if path.is_empty() {
            None
        } else {
            Some(Self { path, target })
        }
    }

    /// Resolved DataPath segments, in declaration order. Always
    /// non-empty per the [`Self::new`] invariant.
    pub fn path(&self) -> &[crate::Name] {
        &self.path
    }

    /// What the path's tail resolves to (tabular section / scalar
    /// attribute Ty).
    pub fn target(&self) -> &FormDataTarget {
        &self.target
    }
}

/// What a [`FormDataBinding::path`] resolves to at its tail.
///
/// Discriminates the two row-aware cases Phase 5 cares about:
/// - [`Self::TabularSection`] — the path ends in a tabular section
///   (`Объект.Переприемка`); refined `.ВыделенныеСтроки` returns
///   `Ty::TypedArray(row)` where row is the section's row Ty.
/// - [`Self::Attribute`] — the path ends in a scalar attribute
///   (`Объект.Дата`, `Замечание`); the bound type is whatever
///   `lookup_field` produced for the path tail.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FormDataTarget {
    /// DataPath terminates at a tabular section of an MDO. `mdo_type` /
    /// `owner` identify the MDO carrying the section; `section` is the
    /// section name (e.g. `(Document, "ПКО", "Переприемка")`).
    TabularSection { mdo_type: MdoType, owner: crate::Name, section: crate::Name },
    /// DataPath terminates at a scalar attribute. `ty` is the resolved
    /// attribute type — `Ty::String`, `Ty::Number`, `Ty::MetadataRef{…}`,
    /// or any other Ty `lookup_field` produces.
    Attribute { ty: Box<Ty> },
}

/// Ordered chain of platform type names for the control's property and
/// method tables — `[base, extension?]`. Every consumer (field lookup,
/// method lookup, hover, completion, `hir::Type` facade) walks this
/// chain reversed (extension first, base second) so extension-only
/// members (`<Pages>.ТекущаяСтраница`, `<UsualGroup>.Скрыть`,
/// `<Page>` page-specific properties, …) reach the user.
///
/// `chain[0]` is the user-facing display key (the base wrapper name);
/// `chain.last()` is the most specific extension. `Other` returns the
/// empty slice — no platform table to query and method lookup falls
/// through to `Ty::Unknown` instead of mis-classifying.
///
/// The five group sub-kinds (`UsualGroup`, `Pages`, `Page`,
/// `CommandBar`, `ButtonGroup`) carry both the base `ГруппаФормы` and
/// their dedicated platform extension. The catch-all `Group` keeps a
/// single-element chain — extensions are optional.
///
/// Free function rather than inherent impl because [`FormElementKind`]
/// is defined in `bsl-metadata` (orphan rule).
pub fn form_control_platform_type_chain(kind: FormElementKind) -> &'static [&'static str] {
    match kind {
        FormElementKind::Table => &["ТаблицаФормы"],
        FormElementKind::Group => &["ГруппаФормы"],
        FormElementKind::UsualGroup => {
            &["ГруппаФормы", "Расширение группы формы для обычной группы"]
        }
        FormElementKind::Pages => &["ГруппаФормы", "Расширение группы формы для страниц"],
        FormElementKind::Page => &["ГруппаФормы", "Расширение группы формы для страницы"],
        FormElementKind::CommandBar => {
            &["ГруппаФормы", "Расширение группы формы для командной панели"]
        }
        FormElementKind::ButtonGroup => {
            &["ГруппаФормы", "Расширение группы формы для группы кнопок"]
        }
        FormElementKind::Field => &["ПолеФормы"],
        FormElementKind::Button => &["КнопкаФормы"],
        FormElementKind::Decoration => &["ДекорацияФормы"],
        FormElementKind::Addition => &["ДополнениеЭлементаФормы"],
        FormElementKind::Other => &[],
    }
}

/// Primary platform type name (the base wrapper, e.g. `ТаблицаФормы` /
/// `ГруппаФормы`) — kept as a thin convenience over
/// [`form_control_platform_type_chain`] so display callers (hover label,
/// `Ty::display_name`) don't allocate a slice walk for one entry.
///
/// `Other` returns `None`. All other kinds return `Some(chain[0])`.
pub fn form_control_platform_type_name(kind: FormElementKind) -> Option<&'static str> {
    form_control_platform_type_chain(kind).first().copied()
}

/// Human-facing label for a form-element kind, bilingual.
///
/// Single source of truth for completion item details, hover badges and
/// any other UI surface that needs to name a kind. Lives in `hir-def`
/// rather than `bsl-metadata` because `Locale` is an interface-adapter
/// concern (i18n) and the entity layer should not depend on it (Clean
/// Architecture decision in plan v3.1, table row #5).
pub fn form_element_kind_label(kind: FormElementKind, locale: base_db::Locale) -> &'static str {
    use base_db::Locale;
    match (kind, locale) {
        (FormElementKind::Table, Locale::Ru) => "Таблица",
        (FormElementKind::Table, Locale::En) => "Table",
        (FormElementKind::Group, Locale::Ru) => "Группа",
        (FormElementKind::Group, Locale::En) => "Group",
        (FormElementKind::UsualGroup, Locale::Ru) => "Обычная группа",
        (FormElementKind::UsualGroup, Locale::En) => "Usual group",
        (FormElementKind::Pages, Locale::Ru) => "Страницы",
        (FormElementKind::Pages, Locale::En) => "Pages",
        (FormElementKind::Page, Locale::Ru) => "Страница",
        (FormElementKind::Page, Locale::En) => "Page",
        (FormElementKind::CommandBar, Locale::Ru) => "Командная панель",
        (FormElementKind::CommandBar, Locale::En) => "Command bar",
        (FormElementKind::ButtonGroup, Locale::Ru) => "Группа кнопок",
        (FormElementKind::ButtonGroup, Locale::En) => "Button group",
        (FormElementKind::Field, Locale::Ru) => "Поле",
        (FormElementKind::Field, Locale::En) => "Field",
        (FormElementKind::Button, Locale::Ru) => "Кнопка",
        (FormElementKind::Button, Locale::En) => "Button",
        (FormElementKind::Decoration, Locale::Ru) => "Декорация",
        (FormElementKind::Decoration, Locale::En) => "Decoration",
        (FormElementKind::Addition, Locale::Ru) => "Дополнение",
        (FormElementKind::Addition, Locale::En) => "Addition",
        (FormElementKind::Other, _) => "Элемент формы",
    }
}

/// Sort band for completion popups. Tables (`10`) → groups (`20`) →
/// fields (`30`) → buttons (`40`) → decorations (`50`) → additions
/// (`60`) → other (`70`). Decoupled from `derive(Ord)` because the
/// append-only discriminant policy puts new variants AFTER `Other`,
/// which is the wrong UI order.
pub fn form_element_kind_sort_band(kind: FormElementKind) -> u8 {
    match kind {
        FormElementKind::Table => 10,
        FormElementKind::Group
        | FormElementKind::UsualGroup
        | FormElementKind::Pages
        | FormElementKind::Page
        | FormElementKind::CommandBar
        | FormElementKind::ButtonGroup => 20,
        FormElementKind::Field => 30,
        FormElementKind::Button => 40,
        FormElementKind::Decoration => 50,
        FormElementKind::Addition => 60,
        FormElementKind::Other => 70,
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
            MdoType::Task => Some(MetadataKind::TaskObject),
            MdoType::BusinessProcess => Some(MetadataKind::BusinessProcessObject),
            MdoType::DataProcessor => Some(MetadataKind::DataProcessorObject),
            MdoType::Report => Some(MetadataKind::ReportObject),
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
            Self::TaskObject => Some("TaskObject"),
            Self::BusinessProcessRef => Some("BusinessProcessRef"),
            Self::BusinessProcessObject => Some("BusinessProcessObject"),
            Self::DataProcessorObject => Some("DataProcessorObject"),
            Self::ReportObject => Some("ReportObject"),
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
            (Self::TaskObject, Locale::Ru) => "ЗадачаОбъект",
            (Self::TaskObject, Locale::En) => "TaskObject",
            (Self::BusinessProcessRef, Locale::Ru) => "БизнесПроцессСсылка",
            (Self::BusinessProcessRef, Locale::En) => "BusinessProcessRef",
            (Self::BusinessProcessObject, Locale::Ru) => "БизнесПроцессОбъект",
            (Self::BusinessProcessObject, Locale::En) => "BusinessProcessObject",
            (Self::DataProcessorObject, Locale::Ru) => "ОбработкаОбъект",
            (Self::DataProcessorObject, Locale::En) => "DataProcessorObject",
            (Self::ReportObject, Locale::Ru) => "ОтчётОбъект",
            (Self::ReportObject, Locale::En) => "ReportObject",
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
            // Coarse label only — the fully parameterised render
            // (`Массив из Строка`) lives on the [`TyDisplay`] wrapper so
            // the cheap `&str` API stays cheap. Method/property lookup
            // never goes through `display_name`; it uses
            // `platform_type_name` which returns `"Array"` (canonical
            // English) via the catch-all arm.
            (Ty::TypedArray(_), Locale::Ru) => "Массив",
            (Ty::TypedArray(_), Locale::En) => "Array",
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
            // Form-control kinds map to the Russian `*Формы` platform type
            // names — same locale-agnostic strategy as `FormData`. `Other`
            // has no backing platform table; surface a generic label so
            // diagnostics never panic on an unknown XML tag.
            (Ty::FormControl { kind, .. }, _) => match form_control_platform_type_name(*kind) {
                Some(name) => name,
                None => match locale {
                    Locale::Ru => "ЭлементФормы",
                    Locale::En => "FormElement",
                },
            },
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
            // Form controls (`Элементы.<имя>`) wrap one of the per-kind
            // platform tables (`ТаблицаФормы`, `ПолеФормы`, …). Method
            // and non-refined property lookup goes through these names;
            // refined lookup on `FormControl{Table, Some(binding)}` for
            // `.ВыделенныеСтроки` / `.ТекущаяСтрока` is layered on top in
            // `hir-ty::field_lookup`. `Other` has no platform table, so
            // method dispatch falls through to `Ty::Unknown`.
            Ty::FormControl { kind, .. } => form_control_platform_type_name(*kind),
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
            // Parameterised array — `Массив из Строка` / `Array of String`.
            // The element renders with the same locale as the outer
            // wrapper so a Russian hover stays in Russian end-to-end and
            // an English one stays in English. Composite shapes
            // (`Массив из Массив из Число`) recurse naturally through
            // `elem.display(locale)`.
            Ty::TypedArray(elem) => match self.locale {
                Locale::Ru => write!(f, "Массив из {}", elem.display(self.locale)),
                Locale::En => write!(f, "Array of {}", elem.display(self.locale)),
            },
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
            // Form control — `ТаблицаФормы (Объект.Переприемка)` /
            // `FormTable (Объект.Переприемка)` when a DataPath binding
            // is present, or just the wrapper name otherwise. The path
            // is rendered as the original chain joined by `.` so the
            // reader sees which form attribute the control is bound to.
            Ty::FormControl { binding, .. } => {
                let wrapper = self.ty.display_name(self.locale);
                match binding {
                    Some(b) => {
                        f.write_str(wrapper)?;
                        f.write_str(" (")?;
                        let mut it = b.path.iter();
                        if let Some(first) = it.next() {
                            f.write_str(first.as_str())?;
                            for seg in it {
                                f.write_str(".")?;
                                f.write_str(seg.as_str())?;
                            }
                        }
                        f.write_str(")")
                    }
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
    fn typed_array_canonical_name_matches_array() {
        // Method/property lookup keys this through `canonical_name`
        // (English machine-name). Both variants share the same platform
        // page, so the key must collide on `"Array"`.
        let ta = Ty::TypedArray(Box::new(Ty::String));
        assert_eq!(ta.canonical_name(), "Array");
        assert_eq!(ta.canonical_name(), Ty::Array.canonical_name());
    }

    #[test]
    fn typed_array_platform_type_name_resolves_via_array_page() {
        // Falls through the catch-all in `platform_type_name`, which
        // returns canonical English — matching the legacy `Ty::Array`
        // route into `bsl-platform`.
        let ta = Ty::TypedArray(Box::new(Ty::Number));
        assert_eq!(ta.platform_type_name(), Some("Array"));
    }

    #[test]
    fn typed_array_display_renders_element_type_bilingual() {
        use base_db::Locale;
        let ta = Ty::TypedArray(Box::new(Ty::String));
        assert_eq!(ta.display(Locale::Ru).to_string(), "Массив из Строка");
        assert_eq!(ta.display(Locale::En).to_string(), "Array of String");
    }

    #[test]
    fn typed_array_display_recurses_into_nested_element() {
        use base_db::Locale;
        // Nested parameterisation renders end-to-end without losing
        // locale: `Массив из Массив из Число`.
        let inner = Ty::TypedArray(Box::new(Ty::Number));
        let outer = Ty::TypedArray(Box::new(inner));
        assert_eq!(outer.display(Locale::Ru).to_string(), "Массив из Массив из Число");
        assert_eq!(outer.display(Locale::En).to_string(), "Array of Array of Number");
    }

    #[test]
    fn typed_array_display_name_is_coarse_label() {
        use base_db::Locale;
        // The `&str`-returning `display_name` API is a coarse label —
        // it must match `Ty::Array`'s label so existing callers that
        // only look at the variant tag don't see drift. Detailed
        // rendering (`Массив из X`) lives on the `TyDisplay` wrapper.
        let ta = Ty::TypedArray(Box::new(Ty::String));
        assert_eq!(ta.display_name(Locale::Ru), "Массив");
        assert_eq!(ta.display_name(Locale::En), "Array");
    }

    #[test]
    fn typed_array_union_dedups_by_element_type() {
        // Smart-constructor invariant: identical `TypedArray(T)` instances
        // dedup; `TypedArray(T)` and `TypedArray(U)` (different elements)
        // stay distinct so the union renders both arms.
        let a = Ty::TypedArray(Box::new(Ty::Number));
        let b = Ty::TypedArray(Box::new(Ty::Number));
        assert_eq!(Ty::union(vec![a.clone(), b]), a);

        let c = Ty::TypedArray(Box::new(Ty::String));
        let union = Ty::union(vec![a, c]);
        match union {
            Ty::Union(ref parts) => {
                assert_eq!(parts.len(), 2, "TypedArray with different elements must not dedup");
            }
            other => panic!("expected Ty::Union, got {other:?}"),
        }
    }

    #[test]
    fn typed_array_distinct_from_unparameterised_array() {
        // The two variants are deliberately distinct under `PartialEq`.
        // `Ty::union` must keep both alive so a callsite that observes
        // both shapes (`Массив` literal flowing alongside a typed JSDoc
        // result) doesn't lose the element refinement.
        let a = Ty::Array;
        let ta = Ty::TypedArray(Box::new(Ty::Number));
        assert_ne!(a, ta);
        match Ty::union(vec![a, ta]) {
            Ty::Union(ref parts) => assert_eq!(parts.len(), 2),
            other => panic!("expected Ty::Union, got {other:?}"),
        }
    }

    #[test]
    fn metadata_kind_object_kind_for_covers_ship_set() {
        // Ships every `*Object` coercion used by `Ty::ThisObject` and the
        // FormData main-attribute projection. If one of these regresses,
        // the resolver / form_attr lowering would stop producing the
        // matching `*Object` MetadataRef and hover / completion would
        // silently drop type information for `Объект.<attr>`.
        for (mdo, expected) in [
            (MdoType::Catalog, MetadataKind::CatalogObject),
            (MdoType::Document, MetadataKind::DocumentObject),
            (MdoType::ExchangePlan, MetadataKind::ExchangePlanObject),
            (MdoType::ChartOfAccounts, MetadataKind::ChartOfAccountsObject),
            (MdoType::Task, MetadataKind::TaskObject),
            (MdoType::BusinessProcess, MetadataKind::BusinessProcessObject),
            (MdoType::DataProcessor, MetadataKind::DataProcessorObject),
            (MdoType::Report, MetadataKind::ReportObject),
        ] {
            assert_eq!(
                MetadataKind::object_kind_for(mdo),
                Some(expected),
                "object_kind_for({mdo:?}) must yield {expected:?}"
            );
        }
    }

    #[test]
    fn metadata_kind_object_kind_for_rejects_non_object_mdo_kinds() {
        // Register flavours, Enum, ChartOfCharacteristicTypes, and
        // CommonModule have no `*Object` companion. `resolve_this_object`
        // and FormData main-attribute lowering both key off this to
        // refuse promotion — a regression that starts returning `Some`
        // here would let downstream code surface dangling MetadataRef
        // receivers with no field/method enumeration surface.
        assert!(MetadataKind::object_kind_for(MdoType::InformationRegister).is_none());
        assert!(MetadataKind::object_kind_for(MdoType::AccumulationRegister).is_none());
        assert!(MetadataKind::object_kind_for(MdoType::AccountingRegister).is_none());
        assert!(MetadataKind::object_kind_for(MdoType::CalculationRegister).is_none());
        assert!(MetadataKind::object_kind_for(MdoType::Enum).is_none());
        assert!(MetadataKind::object_kind_for(MdoType::CommonModule).is_none());
    }

    // ---- Phase 3: Ty::FormControl ----

    #[test]
    fn form_control_platform_type_name_per_kind() {
        // The kind→platform-type mapping is the contract that links the
        // XML taxonomy in `bsl-metadata` to the per-control method
        // tables in `bsl-platform`. A drift here silently breaks
        // method/property dispatch on `Элементы.<имя>`.
        assert_eq!(form_control_platform_type_name(FormElementKind::Table), Some("ТаблицаФормы"));
        assert_eq!(form_control_platform_type_name(FormElementKind::Group), Some("ГруппаФормы"));
        assert_eq!(form_control_platform_type_name(FormElementKind::Field), Some("ПолеФормы"));
        assert_eq!(form_control_platform_type_name(FormElementKind::Button), Some("КнопкаФормы"));
        assert_eq!(
            form_control_platform_type_name(FormElementKind::Decoration),
            Some("ДекорацияФормы")
        );
        assert_eq!(
            form_control_platform_type_name(FormElementKind::Addition),
            Some("ДополнениеЭлементаФормы")
        );
        assert_eq!(form_control_platform_type_name(FormElementKind::Other), None);

        // New variants from Phase 9 of v3.1: each carries its own
        // platform extension but the *primary* (display) key stays the
        // shared `ГруппаФормы` base — extension lives at chain[1].
        assert_eq!(
            form_control_platform_type_name(FormElementKind::UsualGroup),
            Some("ГруппаФормы")
        );
        assert_eq!(form_control_platform_type_name(FormElementKind::Pages), Some("ГруппаФормы"));
        assert_eq!(form_control_platform_type_name(FormElementKind::Page), Some("ГруппаФормы"));
        assert_eq!(
            form_control_platform_type_name(FormElementKind::CommandBar),
            Some("ГруппаФормы")
        );
        assert_eq!(
            form_control_platform_type_name(FormElementKind::ButtonGroup),
            Some("ГруппаФормы")
        );
    }

    #[test]
    fn form_control_platform_type_chain_carries_extension_for_group_subkinds() {
        // The chain returned by `form_control_platform_type_chain` is the
        // load-bearing API for property/method lookup. Reading order is
        // [base, extension] — but lookup walks reversed (extension first)
        // so e.g. `<Pages>.ТекущаяСтраница` resolves on the extension hit.
        // This test pins the slice contents per kind.
        assert_eq!(form_control_platform_type_chain(FormElementKind::Table), &["ТаблицаФормы"]);
        assert_eq!(form_control_platform_type_chain(FormElementKind::Group), &["ГруппаФормы"]);
        assert_eq!(
            form_control_platform_type_chain(FormElementKind::UsualGroup),
            &["ГруппаФормы", "Расширение группы формы для обычной группы"]
        );
        assert_eq!(
            form_control_platform_type_chain(FormElementKind::Pages),
            &["ГруппаФормы", "Расширение группы формы для страниц"]
        );
        assert_eq!(
            form_control_platform_type_chain(FormElementKind::Page),
            &["ГруппаФормы", "Расширение группы формы для страницы"]
        );
        assert_eq!(
            form_control_platform_type_chain(FormElementKind::CommandBar),
            &["ГруппаФормы", "Расширение группы формы для командной панели"]
        );
        assert_eq!(
            form_control_platform_type_chain(FormElementKind::ButtonGroup),
            &["ГруппаФормы", "Расширение группы формы для группы кнопок"]
        );
        assert_eq!(form_control_platform_type_chain(FormElementKind::Field), &["ПолеФормы"]);
        assert_eq!(form_control_platform_type_chain(FormElementKind::Button), &["КнопкаФормы"]);
        assert_eq!(
            form_control_platform_type_chain(FormElementKind::Decoration),
            &["ДекорацияФормы"]
        );
        assert_eq!(
            form_control_platform_type_chain(FormElementKind::Addition),
            &["ДополнениеЭлементаФормы"]
        );
        assert_eq!(
            form_control_platform_type_chain(FormElementKind::Other),
            &[] as &[&'static str]
        );
    }

    #[test]
    fn form_control_platform_type_name_equals_chain_first_for_all_kinds() {
        // Invariant: the scalar primary key is exactly chain[0]. If this
        // ever drifts, display labels and lookup primary key disagree —
        // the kind of bug that wastes hours.
        for kind in [
            FormElementKind::Table,
            FormElementKind::Group,
            FormElementKind::UsualGroup,
            FormElementKind::Pages,
            FormElementKind::Page,
            FormElementKind::CommandBar,
            FormElementKind::ButtonGroup,
            FormElementKind::Field,
            FormElementKind::Button,
            FormElementKind::Decoration,
            FormElementKind::Addition,
            FormElementKind::Other,
        ] {
            assert_eq!(
                form_control_platform_type_name(kind),
                form_control_platform_type_chain(kind).first().copied(),
                "scalar key must equal chain[0] for kind {:?}",
                kind
            );
        }
    }

    #[test]
    fn form_element_kind_label_is_bilingual_and_total() {
        // Every kind has both Ru and En labels (Other deliberately
        // shares a single label with no English glossary entry).
        use base_db::Locale;
        assert_eq!(form_element_kind_label(FormElementKind::Table, Locale::Ru), "Таблица");
        assert_eq!(form_element_kind_label(FormElementKind::Table, Locale::En), "Table");
        assert_eq!(form_element_kind_label(FormElementKind::Pages, Locale::Ru), "Страницы");
        assert_eq!(form_element_kind_label(FormElementKind::Pages, Locale::En), "Pages");
        assert_eq!(
            form_element_kind_label(FormElementKind::UsualGroup, Locale::Ru),
            "Обычная группа"
        );
        assert_eq!(form_element_kind_label(FormElementKind::UsualGroup, Locale::En), "Usual group");
        assert_eq!(form_element_kind_label(FormElementKind::Other, Locale::Ru), "Элемент формы");
        assert_eq!(form_element_kind_label(FormElementKind::Other, Locale::En), "Элемент формы");
    }

    #[test]
    fn form_element_kind_sort_band_groups_share_one_band() {
        // All group sub-kinds share band 20 — the popup orders groups
        // together regardless of which extension chain they carry.
        // Tables come before, fields after; Other lands at the bottom.
        assert_eq!(form_element_kind_sort_band(FormElementKind::Table), 10);
        assert_eq!(form_element_kind_sort_band(FormElementKind::Group), 20);
        assert_eq!(form_element_kind_sort_band(FormElementKind::UsualGroup), 20);
        assert_eq!(form_element_kind_sort_band(FormElementKind::Pages), 20);
        assert_eq!(form_element_kind_sort_band(FormElementKind::Page), 20);
        assert_eq!(form_element_kind_sort_band(FormElementKind::CommandBar), 20);
        assert_eq!(form_element_kind_sort_band(FormElementKind::ButtonGroup), 20);
        assert_eq!(form_element_kind_sort_band(FormElementKind::Field), 30);
        assert_eq!(form_element_kind_sort_band(FormElementKind::Button), 40);
        assert_eq!(form_element_kind_sort_band(FormElementKind::Decoration), 50);
        assert_eq!(form_element_kind_sort_band(FormElementKind::Addition), 60);
        assert_eq!(form_element_kind_sort_band(FormElementKind::Other), 70);
    }

    #[test]
    fn form_control_display_name_uses_platform_table_per_kind() {
        use base_db::Locale;
        let table = Ty::FormControl { kind: FormElementKind::Table, binding: None };
        let field = Ty::FormControl { kind: FormElementKind::Field, binding: None };
        let other = Ty::FormControl { kind: FormElementKind::Other, binding: None };

        // Per-kind labels are locale-agnostic Russian platform names —
        // identical strategy as `Ty::FormData`.
        assert_eq!(table.display_name(Locale::Ru), "ТаблицаФормы");
        assert_eq!(table.display_name(Locale::En), "ТаблицаФормы");
        assert_eq!(field.display_name(Locale::Ru), "ПолеФормы");

        // `Other` falls back to a localised generic — no platform table.
        assert_eq!(other.display_name(Locale::Ru), "ЭлементФормы");
        assert_eq!(other.display_name(Locale::En), "FormElement");
    }

    #[test]
    fn form_control_platform_type_name_routes_method_lookup_per_kind() {
        let table = Ty::FormControl { kind: FormElementKind::Table, binding: None };
        let button = Ty::FormControl { kind: FormElementKind::Button, binding: None };
        let other = Ty::FormControl { kind: FormElementKind::Other, binding: None };

        // The `&self` API (used by method/property lookup in `hir-ty`)
        // must agree with the free function — both come from the same
        // table.
        assert_eq!(table.platform_type_name(), Some("ТаблицаФормы"));
        assert_eq!(button.platform_type_name(), Some("КнопкаФормы"));
        // `Other` returns `None` so dispatch falls through to `Unknown`
        // rather than mis-classifying.
        assert_eq!(other.platform_type_name(), None);
    }

    #[test]
    fn form_control_display_with_binding_renders_path() {
        use base_db::Locale;
        // Binding renders as `«ТаблицаФормы (Объект.Переприемка)»` —
        // path joined by `.` so hover shows which form attribute the
        // control is bound to without a second resolution pass.
        let binding = FormDataBinding::new(
            Box::new([crate::Name::new("Объект"), crate::Name::new("Переприемка")]),
            FormDataTarget::TabularSection {
                mdo_type: MdoType::Document,
                owner: crate::Name::new("ПКО"),
                section: crate::Name::new("Переприемка"),
            },
        )
        .expect("path is non-empty");
        let ty = Ty::FormControl { kind: FormElementKind::Table, binding: Some(binding) };
        assert_eq!(ty.display(Locale::Ru).to_string(), "ТаблицаФормы (Объект.Переприемка)");
        assert_eq!(ty.display(Locale::En).to_string(), "ТаблицаФормы (Объект.Переприемка)");
    }

    #[test]
    fn form_control_display_without_binding_is_just_wrapper() {
        use base_db::Locale;
        let ty = Ty::FormControl { kind: FormElementKind::Field, binding: None };
        assert_eq!(ty.display(Locale::Ru).to_string(), "ПолеФормы");
        assert_eq!(ty.display(Locale::En).to_string(), "ПолеФормы");
    }

    #[test]
    fn form_control_union_dedups_identical_kind_and_binding() {
        // Smart-constructor invariant: identical `(kind, binding)` pairs
        // dedup; different `kind` or `binding` stay distinct so the
        // union still surfaces every Ty alternative.
        let a = Ty::FormControl { kind: FormElementKind::Table, binding: None };
        let b = Ty::FormControl { kind: FormElementKind::Table, binding: None };
        assert_eq!(Ty::union(vec![a.clone(), b]), a);

        let other_kind = Ty::FormControl { kind: FormElementKind::Field, binding: None };
        match Ty::union(vec![a, other_kind]) {
            Ty::Union(parts) => assert_eq!(parts.len(), 2),
            other => panic!("expected Ty::Union, got {other:?}"),
        }
    }

    #[test]
    fn form_control_ord_is_stable_by_kind_then_binding() {
        // `Ord` participates in `Ty::union`'s sort+dedup. The current
        // contract is "kind first, binding second" via derived Ord on
        // pinned `FormElementKind` discriminants — pin the observable
        // shape so a future refactor doesn't silently shuffle Salsa
        // cache keys.
        let a = Ty::FormControl { kind: FormElementKind::Table, binding: None };
        let b = Ty::FormControl { kind: FormElementKind::Field, binding: None };
        // Table = 0 < Field = 2 (per pinned discriminants in Phase 1).
        assert!(a < b);
    }

    #[test]
    fn form_data_binding_hash_stable_for_salsa_keys() {
        // `FormDataBinding` participates in Ty's `Hash` derivation.
        // Equal bindings must hash equally so Salsa-cached lookups on
        // `Ty::FormControl{kind, Some(binding)}` collide on a single
        // cache entry. A bug here surfaces as cache thrashing, not a
        // wrong answer — easy to miss without an explicit pin.
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        fn h(b: &FormDataBinding) -> u64 {
            let mut s = DefaultHasher::new();
            b.hash(&mut s);
            s.finish()
        }

        let mk = || {
            FormDataBinding::new(
                Box::new([crate::Name::new("Объект"), crate::Name::new("Переприемка")]),
                FormDataTarget::TabularSection {
                    mdo_type: MdoType::Document,
                    owner: crate::Name::new("ПКО"),
                    section: crate::Name::new("Переприемка"),
                },
            )
            .expect("path is non-empty")
        };

        let a = mk();
        let b = mk();
        assert_eq!(a, b);
        assert_eq!(h(&a), h(&b));
    }

    #[test]
    fn form_data_binding_new_rejects_empty_path() {
        // Empty path is meaningless — the enclosing `Ty::FormControl`
        // should carry `binding: None` instead. Pinning this guards
        // against Phase 4 accidentally producing vacuous `Some(...)`
        // bindings that render as `ТаблицаФормы ()` in hover.
        let target = FormDataTarget::Attribute { ty: Box::new(Ty::Unknown) };
        assert!(FormDataBinding::new(Box::new([]), target.clone()).is_none());
        let ok = FormDataBinding::new(Box::new([crate::Name::new("Объект")]), target);
        assert!(ok.is_some());
    }

    #[test]
    fn form_data_target_attribute_carries_resolved_ty() {
        // Scalar attribute branch — Phase 4 will produce these for
        // `Замечание` (String) or `Объект.Code` (String) via
        // `lookup_field` on each path segment.
        let target = FormDataTarget::Attribute { ty: Box::new(Ty::String) };
        match target {
            FormDataTarget::Attribute { ty } => assert_eq!(*ty, Ty::String),
            other => panic!("expected Attribute, got {other:?}"),
        }
    }
}
