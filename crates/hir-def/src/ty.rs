//! Type system for BSL.
//!
//! This module provides basic type information for BSL values and expressions.
//! Full type inference is planned for later iterations (12+).

pub mod doc_types;

use bsl_metadata::MdoType;
use syntax::ast::{self, AstNode};
use syntax::SyntaxKind;

/// BSL type representation.
///
/// Represents the type of a BSL value or expression.
/// For Iteration 8, we support basic literal types and Unknown for everything else.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
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
}

/// Metadata object kind.
///
/// Classifies the flavour of MDO (or MDO fragment) that a [`Ty::MetadataRef`]
/// carries. The `name` of the enclosing `MetadataRef` is the MDO identifier as
/// it appears in the configuration (`"ПКО"`, `"Номенклатура"`). For
/// [`Self::TabularSection`] / [`Self::TabularSectionRow`] the name conventionally
/// encodes `"Parent.Section"` (e.g. `"ПКО.Товары"`) — parent MDO first, tabular
/// section name second — so a single `MetadataRef` locates the section without
/// an extra field.
///
/// Adding a variant? Also extend:
/// - `metadata_kind_from_prefix` in `hir-ty/src/lower/mod.rs` (prefix → kind),
/// - `mdo_ref_prefix` in `hir-def/src/type_ref.rs` if a new `MdoType` prefix is
///   needed,
/// - JSDoc parser in `hir-def/src/ty/doc_types.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// Tabular section of a metadata object (`ТабличнаяЧасть`).
    ///
    /// Name carries `"Parent.Section"` (e.g. `"ПКО.Товары"`) — the parent MDO
    /// identifier and the section name, dot-joined. This lets field lookup
    /// (M3 Task 8) resolve `Документ.Товары` without threading a second
    /// parent identifier.
    TabularSection,
    /// A single row of a tabular section (`СтрокаТабличнойЧасти`).
    ///
    /// Name follows the same `"Parent.Section"` convention as
    /// [`Self::TabularSection`] — the enclosing tabular section is implied by
    /// the name path.
    TabularSectionRow,
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
            Ty::Function { .. } => "Function",
            Ty::PlatformObject(name) => name.as_str(),
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
            Ty::PlatformObject(name) => Some(name.as_str()),
            _ => Some(self.display_name()),
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
}
