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

    /// Function or procedure type.
    ///
    /// In BSL, functions and procedures are first-class values.
    /// params: parameter types, ret: return type (Undefined for procedures).
    Function { params: Box<[Ty]>, ret: Box<Ty> },

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
    /// Accumulation register record set (РегистрНакопленияНаборЗаписей).
    AccumulationRegisterRecordSet,
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
            // methods under `InformationRegisterRecordManager.<Имя>`
            // and `AccumulationRegisterRecordSet.<Имя>` composite
            // typenames. Wired here as part of Phase C so platform
            // calls (`Записать`, `Прочитать`, …) on register-record
            // receivers stay resolvable now that
            // `map_generic_metadata_return_type` rebinds those return
            // types to concrete `Ty::MetadataRef` shapes.
            Self::InformationRegisterRecordManager => Some("InformationRegisterRecordManager"),
            Self::AccumulationRegisterRecordSet => Some("AccumulationRegisterRecordSet"),
            Self::InformationRegisterRef
            | Self::AccumulationRegisterRef
            | Self::AccountingRegisterRef
            | Self::CalculationRegisterRef
            | Self::RegisterDimension { .. }
            | Self::RegisterResource { .. }
            | Self::RegisterAttribute { .. }
            | Self::TabularSection { .. }
            | Self::TabularSectionRow { .. } => None,
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

    /// Get a human-readable display name for this type.
    pub fn display_name(&self) -> &str {
        match self {
            Ty::Unknown => "Unknown",
            Ty::Number => "Number",
            Ty::String => "String",
            Ty::Boolean => "Boolean",
            Ty::Date => "Date",
            Ty::Undefined => "Undefined",
            Ty::Null => "Null",
            Ty::Array => "Array",
            Ty::Structure => "Structure",
            Ty::Map => "Map",
            Ty::Type => "Type",
            Ty::ValueTable => "ValueTable",
            Ty::ValueList => "ValueList",
            Ty::MetadataRef { .. } => "MetadataRef",
            Ty::ManagerCollection(kind) => {
                // `manager_type_prefix` is the canonical display
                // ("DocumentManager", …). The [`Ty::manager_collection`]
                // factory rejects `MdoType`s without a manager form, so the
                // `None` branch is only reachable if a caller bypassed the
                // factory — surface a generic label rather than panic to
                // keep the type-system layer robust in the face of a
                // lowering bug.
                kind.manager_type_prefix().unwrap_or("ManagerCollection")
            }
            Ty::ObjectManager { .. } => "ObjectManager",
            Ty::ThisObject { .. } => "ThisObject",
            Ty::Function { .. } => "Function",
            Ty::PlatformObject(name) => name.as_str(),
            // Coarse label mirrors `MetadataRef` / `ObjectManager`: the
            // member-by-member rendering lives on `impl fmt::Display` so
            // callers that need "Число | Строка" go through `to_string()`
            // while APIs that only need a `&str` tag stay cheap.
            Ty::Union(_) => "Union",
        }
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
            // Unions have no single platform type by construction — a
            // caller that wants methods on `Ty::Union([Number, String])`
            // must narrow first (M4) or intersect method tables explicitly.
            Ty::Union(_) => None,
            Ty::PlatformObject(name) => Some(name.as_str()),
            _ => Some(self.display_name()),
        }
    }
}

/// Human-readable type rendering.
///
/// Simple variants delegate to [`Ty::display_name`]. [`Ty::Union`] is the only
/// variant whose rendering depends on its payload — the smart constructor
/// imposes a stable order, so the output is deterministic.
impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Union(types) => {
                let mut it = types.iter();
                if let Some(first) = it.next() {
                    write!(f, "{first}")?;
                    for t in it {
                        write!(f, " | {t}")?;
                    }
                }
                Ok(())
            }
            other => f.write_str(other.display_name()),
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

    /// Return type (`Undefined` for procedures).
    pub ret: Box<Ty>,
}

impl FunctionSignature {
    /// Create a new function signature.
    pub fn new(params: Vec<Ty>, ret: Ty) -> Self {
        Self { params: params.into_boxed_slice(), ret: Box::new(ret) }
    }

    /// Create a procedure signature (returns Undefined).
    pub fn procedure(params: Vec<Ty>) -> Self {
        Self::new(params, Ty::Undefined)
    }

    /// Create a function signature with known return type.
    pub fn function(params: Vec<Ty>, ret: Ty) -> Self {
        Self::new(params, ret)
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
        assert_eq!(Ty::Number.display_name(), "Number");
        assert_eq!(Ty::String.display_name(), "String");
        assert_eq!(Ty::Boolean.display_name(), "Boolean");
        assert_eq!(Ty::Unknown.display_name(), "Unknown");
        assert_eq!(Ty::Array.display_name(), "Array");
        assert_eq!(
            Ty::Function { params: Box::new([]), ret: Box::new(Ty::Undefined) }.display_name(),
            "Function"
        );
    }

    #[test]
    fn test_is_unknown() {
        assert!(Ty::Unknown.is_unknown());
        assert!(!Ty::Number.is_unknown());
        assert!(!Ty::String.is_unknown());
    }

    #[test]
    fn test_is_function() {
        assert!(Ty::Function { params: Box::new([]), ret: Box::new(Ty::Undefined) }.is_function());
        assert!(!Ty::Number.is_function());
        assert!(!Ty::Unknown.is_function());
    }

    #[test]
    fn test_default() {
        assert_eq!(Ty::default(), Ty::Unknown);
    }

    #[test]
    fn ty_display_manager_collection() {
        // Manager-collection display name matches the canonical manager
        // prefix in `bsl-metadata::MdoType::manager_type_prefix`, so hover
        // and completion pick up a name consumers already recognise.
        let doc = Ty::manager_collection(MdoType::Document).expect("Document has a manager");
        assert_eq!(doc.display_name(), "DocumentManager");
        let cat = Ty::manager_collection(MdoType::Catalog).expect("Catalog has a manager");
        assert_eq!(cat.display_name(), "CatalogManager");
        let enm = Ty::manager_collection(MdoType::Enum).expect("Enum has a manager");
        assert_eq!(enm.display_name(), "EnumManager");
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
        assert_eq!(ty.display_name(), "ObjectManager");
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
        // `impl Display` renders union members in the smart-constructor
        // order (deterministic via the `Debug`-keyed BTreeMap). Test pins the
        // shape so hover / completion output is stable across runs.
        let u = Ty::union(vec![Ty::String, Ty::Number]);
        let rendered = u.to_string();
        assert!(rendered.contains(" | "), "union render must join with ` | `, got {rendered:?}");
        // Simple variants appear verbatim.
        assert!(rendered.contains("Number"));
        assert!(rendered.contains("String"));
    }

    #[test]
    fn ty_union_display_name_is_coarse_label() {
        // `display_name()` stays as a cheap `&str` tag; nuanced rendering is
        // `Display`'s job. Matches the MetadataRef / ObjectManager pattern.
        assert_eq!(Ty::union(vec![Ty::Number, Ty::String]).display_name(), "Union");
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
