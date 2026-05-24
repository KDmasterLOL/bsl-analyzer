//! Type system for BSL.
//!
//! This module provides basic type information for BSL values and expressions.
//! Full type inference is planned for later iterations (12+).

pub mod doc_types;

use std::sync::Arc;

pub use bsl_metadata::FormElementKind;
use bsl_metadata::MdoType;
use bsl_types::kind::TypeId;
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
    ///
    /// `projection` carries an [`SdblProjection`] when the table value was
    /// derived from a refined query result via `.Выгрузить()` (Phase H).
    /// `None` means either non-SDBL origin (`Новый ТаблицаЗначений`, form
    /// attribute) or an SDBL chain that the bridge couldn't refine
    /// (dynamic text, unrecognised `КАК` alias, etc.). Platform method
    /// dispatch always ignores `projection`; only `field_lookup` /
    /// `iteration_lookup` / `field_enum` consult it.
    ValueTable { projection: Option<Arc<SdblProjection>> },

    /// Row of a projected [`Ty::ValueTable`] (Phase H).
    ///
    /// Produced by `Для Каждого <row> Из <Ty::ValueTable { Some(p) }>` so
    /// the row's `<col>` access can resolve against the projection's
    /// `fields` slice. Platform method / field dispatch falls back to
    /// `СтрокаТаблицыЗначений` for everything outside the projection,
    /// matching the runtime shape. `projection: None` mirrors the
    /// existing platform `СтрокаТаблицыЗначений` row (no projection
    /// enrichment).
    ValueTableRow { projection: Option<Arc<SdblProjection>> },

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
    /// [`MetadataKind::ChartOfAccountsObject`]); manager modules surface
    /// `ЭтотОбъект` through [`Self::ThisManager`], record-set modules
    /// through [`Self::MetadataRef`] with the matching `*RecordSet` kind
    /// (see [`MetadataKind::record_set_kind_for`]), and managed forms
    /// through `Ty::PlatformObject(ФормаКлиентскогоПриложения)`. Common
    /// / command modules remain `Ty::Unknown`.
    ThisObject {
        /// `(kind, name)` of the MDO that owns the module in which
        /// `ЭтотОбъект` appears.
        owner: (MdoType, crate::Name),
    },

    /// Implicit receiver bound to `ЭтотОбъект` / `ThisObject` **inside a
    /// ManagerModule** (`<Folder>/<Name>/Ext/ManagerModule.bsl`).
    ///
    /// Sibling of [`Self::ThisObject`] for the manager axis. Inside an
    /// ObjectModule `ЭтотОбъект` denotes the per-record object and
    /// coerces to `Ty::MetadataRef { *Object, name }`; inside a
    /// ManagerModule the same identifier denotes the *manager* itself
    /// (`Справочники.Номенклатура`) and must coerce to
    /// [`Self::ObjectManager`] so subsequent method dispatch lands on
    /// the workspace `ManagerModule.bsl` exports / `CatalogManager`
    /// platform table — **not** the per-record `*Object` table.
    ///
    /// A separate variant rather than a discriminator on
    /// [`Self::ThisObject`] is the safe choice: existing pattern matches
    /// like `Ty::ThisObject { .. } | Ty::MetadataRef { .. }` (the
    /// authoritative-receiver predicate in `unresolved_field`,
    /// `unresolved_method_call`, …) keep meaning exactly "ObjectModule
    /// receiver" and don't silently widen to also accept ManagerModule
    /// receivers — the compiler now flags every site that needs an
    /// explicit decision about manager handling.
    ///
    /// Coercion target is [`Self::ObjectManager { kind, name }`]
    /// (`Ty::MetadataRef { CatalogManager, … }` is *not* a thing in
    /// this codebase — manager dispatch goes through `ObjectManager`
    /// keyed on `MdoType` directly, see
    /// [`hir_ty::this_object::coerce_to_metadata_ref`]). The plan
    /// (`linear-tumbling-noodle.md` §2.5) had drift here.
    ThisManager {
        /// `(kind, name)` of the MDO whose `ManagerModule.bsl` encloses
        /// the inferred `ЭтотОбъект`.
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

    /// `Запрос` value — the platform `Query` object with per-sub-query
    /// SDBL projections.
    ///
    /// The slice mirrors `SdblPackage::queries()` index-aligned:
    /// `projections[i]` is the projection of the i-th sub-query when
    /// the SDBL bridge resolved it, `None` when that sub-query's
    /// projection couldn't be derived (asterisk-only against an
    /// unresolved table, parse error, etc.). Empty slice means the
    /// constructor argument wasn't a recognised string literal at all —
    /// downstream method/field dispatch then falls back to the
    /// behaviour of `Ty::PlatformObject("Запрос")`.
    ///
    /// The single-projection case (the `.Выполнить()` chain) reads
    /// `projections.first().cloned().flatten()`; the batch case
    /// (`.ВыполнитьПакет()`) feeds the whole slice through to
    /// [`Ty::QueryBatchResult::per_query`] verbatim.
    Query { projections: Arc<[Option<Arc<SdblProjection>>]> },

    /// Return of `.Выполнить()` on a Query whose projection we know.
    ///
    /// `projection = None` mirrors `Ty::PlatformObject("РезультатЗапроса")`
    /// behaviour. Phase 0 never constructs `Some(_)`.
    QueryResult { projection: Option<Arc<SdblProjection>> },

    /// Return of `.Выбрать()` on a query result — the iteration cursor.
    ///
    /// `projection = None` mirrors
    /// `Ty::PlatformObject("ВыборкаИзРезультатаЗапроса")`. Phase 0 never
    /// constructs `Some(_)`. Field lookup gains a projection-driven branch
    /// in Phase 1.
    QueryResultSelection { projection: Option<Arc<SdblProjection>> },

    /// Return of `.ВыполнитьПакет()` — array of per-query results.
    ///
    /// `per_query[i]` is the projection of the `i`-th sub-query in the
    /// batch, or `None` when unresolved. Phase 0 never constructs a
    /// non-empty per_query — variant exists so Phase 3 can attach the
    /// projection at `.ВыполнитьПакет()[i]` indexing.
    QueryBatchResult { per_query: Arc<[Option<Arc<SdblProjection>>]> },

    /// Coarse "some MDO of this kind, name unknown" reference.
    ///
    /// SDBL's `AnyObjectRef { Catalog }` (the cell type behind
    /// `ВЫРАЗИТЬ(... КАК Catalog)` and similar) bridges to this variant.
    /// **Distinct from [`Ty::ManagerCollection`]**: `ManagerCollection`
    /// models the global manager container (`Справочники.` — a global
    /// value), while `AnyMetadataRef` models a *value* of an unspecified
    /// instance of a given MDO family.
    ///
    /// Phase 0 never constructs this variant — Phase 1 bridge adds the
    /// only synthesis sites. Method/field dispatch in Phase 0 mirrors
    /// `Ty::ManagerCollection(mdo_type)` until Phase 1 wires the
    /// instance-shape semantics.
    AnyMetadataRef { mdo_type: MdoType },
}

/// SDBL projection — per-field type information bridged from `sdbl-hir`.
///
/// Lives in `hir-def` (not `sdbl-hir`) because [`Ty`] embeds it: keeping
/// `hir-def` independent of SDBL HIR internals while still surfacing
/// projection data through the type system. The bridge (`hir-ty` Phase 1)
/// fills it; `sdbl-hir` does not import `hir-def`.
///
/// `fields` is the **bridged** view: each entry's [`TypeId`] is an interned
/// BSL type handle, not a raw `SdblType`. This is the single source of truth
/// for field-lookup, completion, and inference on projection-typed receivers.
///
/// `raw_sdbl_types` is the optional **shadow** carrying display-relevant
/// SDBL attributes (precision/scale, length) that `Ty` deliberately drops.
/// `None` when the projection was constructed without the originating
/// package (e.g. a manual cache-bypass path).
///
/// Holds interned `TypeId`s rather than raw `sdbl_hir::SdblType`; ordering is
/// implemented manually below because `TypeId` intentionally has no semantic
/// [`Ord`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SdblProjection {
    /// Per-field interned `TypeId`s under the same-db invariant.
    pub fields: Arc<[(crate::Name, TypeId)]>,
    /// Optional shadow with pre-rendered SDBL-specific display attributes,
    /// indexed parallel to `fields`. `None` when the bridge wasn't given
    /// the originating package; `Some(slice)` invariant: `slice.len() ==
    /// fields.len()`.
    pub raw_sdbl_types: Option<Arc<[SdblTypeShadow]>>,
}

// Deterministic raw-id ordering only (NOT semantic); required while `Ty`
// embeds `Arc<SdblProjection>` and derives `Ord`, until §4.E deletes `Ty`.
impl Ord for SdblProjection {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.fields
            .iter()
            .map(|(n, t)| (n, t.raw()))
            .cmp(other.fields.iter().map(|(n, t)| (n, t.raw())))
            .then_with(|| self.raw_sdbl_types.cmp(&other.raw_sdbl_types))
    }
}

impl PartialOrd for SdblProjection {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Display-only shadow for an SDBL field type.
///
/// Carries the rendered SDBL type label (`"Число(15,2)"`, `"Строка(50)"`)
/// so hover can show precision/scale/length that the bridged [`Ty`]
/// drops. Decoupled from `sdbl_hir::SdblType` to keep `hir-def`
/// independent of SDBL HIR shape.
///
/// `display` is rendered eagerly at bridge time — no formatting in the
/// hover hot path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SdblTypeShadow {
    /// Pre-rendered SDBL type label.
    pub display: String,
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

/// Walk the platform-type chain for `kind` **in reverse** (most-specific
/// extension first, base last) and return the first `Some(_)` produced
/// by `lookup`.
///
/// Encapsulates the "extension overrides base" precedence rule shared
/// by [`hir_ty::method_lookup::lookup_method`] and
/// [`hir_ty::platform_property_lookup::lookup_platform_property`]: both
/// query `PlatformData` per chain segment and want the kind-specific
/// extension table (e.g. `"Расширение группы формы для обычной
/// группы"`) to win over the base `ГруппаФормы` table.
///
/// `Other` has an empty chain → immediate `None` without invoking
/// `lookup`. Single-entry chains (e.g. `Field`, `Button`, `Group`,
/// `Decoration`, `Addition`, `Table`) collapse to one `lookup` call,
/// identical to the pre-helper behaviour.
pub fn form_control_chain_first_hit<T, F>(kind: FormElementKind, mut lookup: F) -> Option<T>
where
    F: FnMut(&str) -> Option<T>,
{
    for type_name in form_control_platform_type_chain(kind).iter().rev() {
        if let Some(res) = lookup(type_name) {
            return Some(res);
        }
    }
    None
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

pub use bsl_types::kind::MetadataKind;

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
            (Ty::ValueTable { .. }, Locale::Ru) => "ТаблицаЗначений",
            (Ty::ValueTable { .. }, Locale::En) => "ValueTable",
            (Ty::ValueTableRow { .. }, Locale::Ru) => "СтрокаТаблицыЗначений",
            (Ty::ValueTableRow { .. }, Locale::En) => "ValueTableRow",
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
            // `ThisManager` renders identically to `ThisObject` from the
            // user's perspective — both surface as `ЭтотОбъект` in source.
            // The variants are distinct only for inference-side dispatch.
            (Ty::ThisManager { .. }, Locale::Ru) => "ЭтотОбъект",
            (Ty::ThisManager { .. }, Locale::En) => "ThisObject",
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
            // while APIs that only need a `&str` tag stay cheap. The
            // RU label `"Составной"` matches the 1С synth-help term
            // ("Составной тип"); EN stays as `"Union"` because
            // `canonical_name()` delegates here and is the stable
            // machine name for platform lookups, logs, and tests.
            (Ty::Union(_), Locale::Ru) => "Составной",
            (Ty::Union(_), Locale::En) => "Union",
            // Projection-typed receivers alias to their corresponding
            // platform-object names: hover, completion and method lookup
            // must reach the same `Запрос` / `РезультатЗапроса` /
            // `ВыборкаИзРезультатаЗапроса` platform tables they reach today
            // through `Ty::PlatformObject(...)`. Phase 0 carries no
            // projection payload — Phase 1's bridge attaches it and adds
            // projection-aware rendering in `TyDisplay`.
            (Ty::Query { .. }, _) => "Запрос",
            (Ty::QueryResult { .. }, _) => "РезультатЗапроса",
            (Ty::QueryResultSelection { .. }, _) => "ВыборкаИзРезультатаЗапроса",
            // Batch result is a `Массив` of `РезультатЗапроса` at runtime;
            // alias to `Массив` so iteration / `.Количество()` resolve
            // through the same table `Ty::Array` reaches.
            (Ty::QueryBatchResult { .. }, Locale::Ru) => "Массив",
            (Ty::QueryBatchResult { .. }, Locale::En) => "Array",
            // `AnyMetadataRef` mirrors `ManagerCollection` semantics in
            // Phase 0 — Phase 1 will refine to instance-shape dispatch.
            (Ty::AnyMetadataRef { mdo_type }, Locale::Ru) => {
                mdo_type.manager_type_prefix_ru().unwrap_or("МенеджерКоллекция")
            }
            (Ty::AnyMetadataRef { mdo_type }, Locale::En) => {
                mdo_type.manager_type_prefix().unwrap_or("ManagerCollection")
            }
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
            //
            // Same story for `ThisManager`: it coerces to
            // [`Ty::ObjectManager { kind, name }`] which is itself
            // platform-method-less (`Ty::ObjectManager` arm above already
            // returns `None`); the dispatch goes through
            // `bsl_platform::get_manager_methods` keyed on `MdoType`, not
            // through the platform-type table.
            Ty::ThisObject { .. } | Ty::ThisManager { .. } => None,
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
            // `AnyMetadataRef` is an instance-shape receiver; like
            // `ManagerCollection` / `ObjectManager` it has no flat platform
            // table (manager / instance dispatch routes through
            // `platform_manager_lookup`, not the scalar method index).
            // Explicit `None` rather than the canonical-name fallback so
            // Phase 1 synthesis can never silently mis-route through a
            // bogus `"ManagerCollection"` key lookup.
            Ty::AnyMetadataRef { .. } => None,
            // Platform method lookup is keyed by canonical English type
            // names (`get_type_methods("Number")`), not localized ones —
            // platform_data.json stores both `name` (RU) and `english_name`
            // (EN) and the lookup index normalises both, so EN here is
            // a deliberate machine-name choice, not user-facing output.
            //
            // Projection-typed `Ty::Query` / `QueryResult` /
            // `QueryResultSelection` / `QueryBatchResult` fall through here
            // — their `canonical_name()` returns `"Запрос"` /
            // `"РезультатЗапроса"` / `"ВыборкаИзРезультатаЗапроса"` /
            // `"Array"`, matching `method_lookup::platform_type_key` so
            // `.methods()` and `lookup_method` stay consistent.
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

/// Function or procedure signature in legacy `Ty` form.
///
/// Contains parameter types and return type. For procedures, the return type is `Undefined`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionSignatureTy {
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

impl FunctionSignatureTy {
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

/// Function or procedure signature in type-kernel form.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionSignature {
    /// Parameter types in declaration order.
    pub params: Box<[bsl_types::kind::TypeId]>,

    /// Per-parameter "has default value" flag (parallel to `params`).
    pub defaults: Box<[bool]>,

    /// Return type (`Undefined` for procedures).
    pub ret: bsl_types::kind::TypeId,

    /// Maximum number of arguments the caller may supply.
    pub max_args: Option<u32>,
}

impl FunctionSignature {
    /// Number of arguments that the caller MUST supply.
    pub fn required_count(&self) -> usize {
        self.defaults.iter().rposition(|has_default| !has_default).map_or(0, |i| i + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(Ty::ValueTable { projection: None }.display_name(Locale::Ru), "ТаблицаЗначений");
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
        use base_db::Locale;
        // `canonical_name()` stays as a cheap `&str` tag; nuanced rendering
        // goes through `Ty::display(locale)`. Matches the MetadataRef /
        // ObjectManager pattern.
        let u = Ty::union(vec![Ty::Number, Ty::String]);
        assert_eq!(u.canonical_name(), "Union");
        assert_eq!(
            u.display_name(Locale::Ru),
            "Составной",
            "Ru coarse label must match the 1С `Составной тип` term"
        );
        assert_eq!(
            u.display_name(Locale::En),
            "Union",
            "En label stays as the canonical machine name `Union` (delegated by canonical_name)"
        );
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

    #[test]
    fn metadata_kind_record_set_kind_for_covers_register_flavours() {
        use MdoType::*;
        for (mdo, expected) in [
            (InformationRegister, MetadataKind::InformationRegisterRecordSet),
            (AccumulationRegister, MetadataKind::AccumulationRegisterRecordSet),
            (AccountingRegister, MetadataKind::AccountingRegisterRecordSet),
            (CalculationRegister, MetadataKind::CalculationRegisterRecordSet),
        ] {
            assert_eq!(
                MetadataKind::record_set_kind_for(mdo),
                Some(expected),
                "record_set_kind_for({mdo:?}) must yield {expected:?}"
            );
        }
    }

    #[test]
    fn metadata_kind_record_set_kind_for_rejects_non_register_mdo_kinds() {
        use MdoType::*;
        assert!(MetadataKind::record_set_kind_for(Catalog).is_none());
        assert!(MetadataKind::record_set_kind_for(Document).is_none());
        assert!(MetadataKind::record_set_kind_for(Enum).is_none());
        assert!(MetadataKind::record_set_kind_for(CommonModule).is_none());
    }

    #[test]
    fn platform_prefix_for_record_kinds_matches_hbk_composite_keys() {
        assert_eq!(
            MetadataKind::InformationRegisterRecord.platform_prefix(),
            Some("InformationRegisterRecord")
        );
        assert_eq!(
            MetadataKind::AccumulationRegisterRecord.platform_prefix(),
            Some("AccumulationRegisterRecord")
        );
        assert_eq!(
            MetadataKind::AccountingRegisterRecord.platform_prefix(),
            Some("AccountingRegisterRecord")
        );
        assert_eq!(
            MetadataKind::CalculationRegisterRecord.platform_prefix(),
            Some("CalculationRegisterRecord")
        );
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
