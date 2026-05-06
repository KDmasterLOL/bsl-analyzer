//! Syntactic type references.
//!
//! `TypeRef` is the **syntactic** description of a BSL type — what the source
//! expressed before name resolution happened:
//!
//! - JSDoc: `// Ссылка - СправочникСсылка.Товары`
//! - `Новый Массив`, `Тип("Число")`
//! - XML metadata: `<Type>cfg:CatalogRef.Товары</Type>`
//! - `ОписаниеТипов("…")` literals
//!
//! To turn a `TypeRef` into a semantic [`crate::Ty`] the caller must run
//! `hir_ty::TyLoweringContext::lower_type_ref` — that adapter consults the
//! resolver and the workspace configuration to pick between `Ty::MetadataRef`,
//! `Ty::PlatformObject`, `Ty::ManagerCollection`, and so on.
//!
//! # Not to be confused with `symbol_info::domain::TypeRef`
//!
//! The `symbol-info` crate ships a *presentation-layer* type also called
//! `TypeRef`: it carries bilingual display strings for hover/completion and has
//! no relation to this HIR entity. Consumers that touch both must disambiguate:
//!
//! ```ignore
//! use hir_def::type_ref::TypeRef;                  // syntactic (this module)
//! use symbol_info::domain::TypeRef as SymTypeRef;  // presentation
//! ```

use bsl_metadata::{AttributeType, MdoType};

use crate::{path::QualifiedName, Name};

/// Syntactic description of a BSL type before name resolution.
///
/// Produced by XML, JSDoc, and expression parsers; consumed by
/// `hir_ty::TyLoweringContext`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeRef {
    /// Primitive builtin (Число, Строка, …).
    ///
    /// Consolidates the case-insensitive Ru/En table that `Ty::from_type_name`
    /// and `ty::doc_types::parse_type_name` historically duplicated.
    Builtin(BuiltinTypeRef),

    /// Qualified name as it appeared in source
    /// (`СправочникСсылка.Товары`, `Документы.ПКО`, `ОпределяемыйТип.Х`).
    ///
    /// Resolution of the head segment against the configuration and platform
    /// tables is `TyLoweringContext`'s job — this variant carries no semantic
    /// classification.
    Name(QualifiedName),

    /// `Массив` with an optional element type (`Массив из Число` in JSDoc).
    Array(Option<Box<TypeRef>>),

    /// `Соответствие` with optional `(Ключ, Значение)` types.
    Map(Option<(Box<TypeRef>, Box<TypeRef>)>),

    /// `ЛюбаяСсылка` — matches XML `cfg:AnyRef` without a concrete name.
    AnyRef,

    /// Union of types — from XML `AttributeType::Composite` and JSDoc
    /// `"Число, Строка"` (M3 Task 4 parser).
    ///
    /// `TyLoweringContext::lower_type_ref` feeds each component through the
    /// same lowering pipeline and then hands the results to [`crate::Ty::union`],
    /// which imposes the flatten/dedup/sort invariant. The caller must not
    /// pre-normalise — the smart constructor owns the shape.
    ///
    /// An empty `Vec` is legal: downstream it collapses to `Ty::Unknown`,
    /// matching the old "something stated but empty" behaviour.
    Union(Vec<TypeRef>),

    /// Source expressed a type that cannot yet be represented syntactically.
    /// Preserved as a marker so downstream diagnostics can tell "no type
    /// stated" from "stated but unsupported" (e.g. unresolved XML references).
    Unknown,
}

/// Primitive builtin, consolidated from the Ru/En lookup tables previously
/// duplicated in `Ty::from_type_name` and `doc_types::parse_type_name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinTypeRef {
    Number,
    String,
    Boolean,
    Date,
    Undefined,
    Null,
    Structure,
    ValueTable,
    ValueList,
    Type,
}

impl BuiltinTypeRef {
    /// Case-insensitive, bilingual (RU/EN) lookup. `None` when the name is not
    /// a primitive builtin — callers usually fall back to collection names
    /// (`Массив`/`Соответствие`) and then to `TypeRef::Name`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "число" | "number" => Some(Self::Number),
            "строка" | "string" => Some(Self::String),
            "булево" | "boolean" => Some(Self::Boolean),
            "дата" | "date" => Some(Self::Date),
            "неопределено" | "undefined" => Some(Self::Undefined),
            "null" => Some(Self::Null),
            "структура" | "structure" => Some(Self::Structure),
            "таблицазначений" | "valuetable" => Some(Self::ValueTable),
            "списокзначений" | "valuelist" => Some(Self::ValueList),
            "тип" | "type" => Some(Self::Type),
            _ => None,
        }
    }
}

impl TypeRef {
    /// Parse a bare, single-segment type name (`Массив`, `Число`, `Соответствие`).
    ///
    /// Covers the builtin table plus the two parameterised collection types.
    /// Returns `None` when the name refers to something that must go through
    /// the resolver (platform objects, user-defined types) — the caller then
    /// wraps the name into [`TypeRef::Name`].
    pub fn from_bare_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "массив" | "array" => Some(TypeRef::Array(None)),
            "соответствие" | "map" => Some(TypeRef::Map(None)),
            other => BuiltinTypeRef::from_name(other).map(TypeRef::Builtin),
        }
    }

    /// Bridge from the XML-parsed [`AttributeType`] (owned by `bsl-metadata`)
    /// into a `TypeRef`.
    ///
    /// Lives here — not in `bsl-metadata` — because `bsl-metadata` is
    /// purposefully ignorant of the HIR layer (reverse-dependency ban). M2
    /// limits the bridge to synactic rewriting; the `Ty` classification is
    /// `TyLoweringContext`'s responsibility.
    ///
    /// `Unknown` maps to [`TypeRef::Unknown`]; `Composite` builds a
    /// [`TypeRef::Union`] whose members are each recursively lowered by the
    /// same bridge, so a composite of composites flattens at the `Ty` layer
    /// when [`crate::Ty::union`] runs.
    pub fn from_attribute_type(attr: &AttributeType) -> Self {
        match attr {
            AttributeType::String { .. } => TypeRef::Builtin(BuiltinTypeRef::String),
            AttributeType::Number { .. } => TypeRef::Builtin(BuiltinTypeRef::Number),
            AttributeType::Boolean => TypeRef::Builtin(BuiltinTypeRef::Boolean),
            AttributeType::Date | AttributeType::DateTime => TypeRef::Builtin(BuiltinTypeRef::Date),
            AttributeType::Ref { mdo_type, name } => match mdo_ref_prefix(*mdo_type) {
                Some(prefix) => TypeRef::Name(QualifiedName::from_segments([
                    Name::new(prefix),
                    Name::new(name),
                ])),
                None => TypeRef::Unknown,
            },
            AttributeType::AnyRef => TypeRef::AnyRef,
            // `AnyObjectRef { mdo_type }` means "any ref of this kind" without
            // a concrete object name — semantically narrower than `AnyRef`
            // but M2 has no `Ty::AnyOf(kind)` variant yet, so we collapse it
            // into `TypeRef::AnyRef`. This deliberately avoids emitting a
            // 1-segment `TypeRef::Name([prefix])`, which downstream lowering
            // would misread as a bare platform object and turn into
            // `Ty::PlatformObject("CatalogRef")`. Promote to a dedicated
            // kind-scoped variant when `Ty` gains union / any-of support.
            AttributeType::AnyObjectRef { .. } => TypeRef::AnyRef,
            AttributeType::Uuid => {
                TypeRef::Name(QualifiedName::from_segments([Name::new("УникальныйИдентификатор")]))
            }
            AttributeType::ValueStorage => {
                TypeRef::Name(QualifiedName::from_segments([Name::new("ХранилищеЗначения")]))
            }
            AttributeType::DefinedType { name } => TypeRef::Name(QualifiedName::from_segments([
                Name::new("ОпределяемыйТип"),
                Name::new(name),
            ])),
            AttributeType::Composite { types } => {
                let members: Vec<TypeRef> = types.iter().map(Self::from_attribute_type).collect();
                TypeRef::Union(members)
            }
            AttributeType::Unknown => TypeRef::Unknown,
        }
    }
}

/// Synactic English prefix that `bsl-metadata::xml_parser` originally parsed
/// for an `AttributeType::Ref`.
///
/// Must **mirror the XML tokens** in `crates/bsl-metadata/src/xml_parser/type_parser.rs`
/// `REF_TYPE_MAP`: the bridge's job is round-tripping XML syntax, not inventing
/// new names. If `REF_TYPE_MAP` grows a new MdoType entry, update this table
/// and the tests that cover it.
///
/// `None` marks `MdoType`s that XML never parses into `Ref`/`AnyObjectRef` —
/// the bridge then falls back to [`TypeRef::Unknown`] rather than guessing a
/// wrong prefix.
fn mdo_ref_prefix(mdo: MdoType) -> Option<&'static str> {
    match mdo {
        MdoType::Catalog => Some("CatalogRef"),
        MdoType::Document => Some("DocumentRef"),
        // Registers are parsed from `cfg:...RegisterRef` tokens in XML — runtime
        // semantics (RecordKey vs RecordSet) belong to lowering, not to the
        // syntactic bridge.
        MdoType::InformationRegister => Some("InformationRegisterRef"),
        MdoType::AccumulationRegister => Some("AccumulationRegisterRef"),
        MdoType::AccountingRegister => Some("AccountingRegisterRef"),
        MdoType::CalculationRegister => Some("CalculationRegisterRef"),
        MdoType::ChartOfCharacteristicTypes => Some("ChartOfCharacteristicTypesRef"),
        MdoType::ChartOfAccounts => Some("ChartOfAccountsRef"),
        MdoType::ChartOfCalculationTypes => Some("ChartOfCalculationTypesRef"),
        MdoType::BusinessProcess => Some("BusinessProcessRef"),
        MdoType::Task => Some("TaskRef"),
        MdoType::Enum => Some("EnumRef"),
        MdoType::ExchangePlan => Some("ExchangePlanRef"),
        // Constants carry a value-manager form (`cfg:ConstantValueManager`) —
        // keep the XML-original token so lowering can match it.
        MdoType::Constant => Some("ConstantValueManager"),
        // DataProcessor and Report exist in XML only as `cfg:*Object` (no
        // ref form) — `REF_TYPE_MAP` carries those tokens so attribute
        // types referencing a data processor / report from a form's main
        // attribute round-trip through the same `Ref` shape.
        MdoType::DataProcessor => Some("DataProcessorObject"),
        MdoType::Report => Some("ReportObject"),
        // The remaining kinds have no reference form in XML; if `AttributeType`
        // ever carries them through `Ref`/`AnyObjectRef` we prefer Unknown over
        // an invented name that the resolver would fail on anyway.
        MdoType::ExternalDataSource
        | MdoType::Cube
        | MdoType::DimensionTable
        | MdoType::CommonModule => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_from_name_russian_and_english_case_insensitive() {
        assert_eq!(BuiltinTypeRef::from_name("Число"), Some(BuiltinTypeRef::Number));
        assert_eq!(BuiltinTypeRef::from_name("ЧИСЛО"), Some(BuiltinTypeRef::Number));
        assert_eq!(BuiltinTypeRef::from_name("Number"), Some(BuiltinTypeRef::Number));
        assert_eq!(BuiltinTypeRef::from_name("NUMBER"), Some(BuiltinTypeRef::Number));
        assert_eq!(BuiltinTypeRef::from_name("таблицаЗначений"), Some(BuiltinTypeRef::ValueTable));
    }

    #[test]
    fn builtin_from_name_rejects_non_primitives() {
        // Collection types are not primitives — they live on `TypeRef` itself
        // because they can carry element type parameters.
        assert_eq!(BuiltinTypeRef::from_name("Массив"), None);
        assert_eq!(BuiltinTypeRef::from_name("Соответствие"), None);
        // User-defined names must go through the resolver, not the builtin table.
        assert_eq!(BuiltinTypeRef::from_name("СправочникСсылка"), None);
        assert_eq!(BuiltinTypeRef::from_name(""), None);
    }

    #[test]
    fn type_ref_parses_primitive_names() {
        assert_eq!(
            TypeRef::from_bare_name("Число"),
            Some(TypeRef::Builtin(BuiltinTypeRef::Number))
        );
        assert_eq!(
            TypeRef::from_bare_name("boolean"),
            Some(TypeRef::Builtin(BuiltinTypeRef::Boolean))
        );
        assert_eq!(TypeRef::from_bare_name("Массив"), Some(TypeRef::Array(None)));
        assert_eq!(TypeRef::from_bare_name("MAP"), Some(TypeRef::Map(None)));
        assert_eq!(TypeRef::from_bare_name("Запрос"), None);
    }

    #[test]
    fn typeref_from_attribute_catalog_ref() {
        let attr =
            AttributeType::Ref { mdo_type: MdoType::Catalog, name: "Товары".to_string() };

        match TypeRef::from_attribute_type(&attr) {
            TypeRef::Name(qname) => {
                assert_eq!(qname.len(), 2);
                assert_eq!(qname.first().as_str(), "CatalogRef");
                assert_eq!(qname.last().as_str(), "Товары");
            }
            other => panic!("expected TypeRef::Name, got {other:?}"),
        }
    }

    #[test]
    fn typeref_from_attribute_primitives() {
        assert_eq!(
            TypeRef::from_attribute_type(&AttributeType::String { length: Some(64) }),
            TypeRef::Builtin(BuiltinTypeRef::String)
        );
        assert_eq!(
            TypeRef::from_attribute_type(&AttributeType::Number { precision: 15, scale: 2 }),
            TypeRef::Builtin(BuiltinTypeRef::Number)
        );
        assert_eq!(
            TypeRef::from_attribute_type(&AttributeType::Boolean),
            TypeRef::Builtin(BuiltinTypeRef::Boolean)
        );
        // Date and DateTime collapse to a single syntactic builtin — BSL does
        // not distinguish them on the type-system level.
        assert_eq!(
            TypeRef::from_attribute_type(&AttributeType::Date),
            TypeRef::Builtin(BuiltinTypeRef::Date)
        );
        assert_eq!(
            TypeRef::from_attribute_type(&AttributeType::DateTime),
            TypeRef::Builtin(BuiltinTypeRef::Date)
        );
    }

    #[test]
    fn typeref_from_attribute_any_ref_and_any_object_ref() {
        assert_eq!(TypeRef::from_attribute_type(&AttributeType::AnyRef), TypeRef::AnyRef);

        // `AnyObjectRef` loses the `mdo_type` discriminator in M2 because
        // there is no `Ty::AnyOf(kind)` — we map to `TypeRef::AnyRef` to
        // avoid producing a bogus 1-segment `TypeRef::Name(["DocumentRef"])`
        // that the lowering fallback would turn into
        // `Ty::PlatformObject("DocumentRef")`. When Ty grows union support,
        // flip this test to expect the kind-scoped variant.
        assert_eq!(
            TypeRef::from_attribute_type(&AttributeType::AnyObjectRef {
                mdo_type: MdoType::Document,
            }),
            TypeRef::AnyRef
        );
    }

    #[test]
    fn typeref_from_attribute_defined_type() {
        let attr =
            AttributeType::DefinedType { name: "ПоказательУчёта".to_string() };
        match TypeRef::from_attribute_type(&attr) {
            TypeRef::Name(qname) => {
                assert_eq!(qname.len(), 2);
                assert_eq!(qname.first().as_str(), "ОпределяемыйТип");
                assert_eq!(qname.last().as_str(), "ПоказательУчёта");
            }
            other => panic!("expected TypeRef::Name, got {other:?}"),
        }
    }

    #[test]
    fn typeref_from_attribute_composite_becomes_union() {
        // M3 wires `AttributeType::Composite` into `TypeRef::Union` so XML
        // `ОписаниеТипов` preserves every declared member type through
        // lowering. The bridge stays syntactic — member order reflects the
        // XML-parsed order; `TyLoweringContext` + `Ty::union` impose the
        // final flatten/dedup/sort.
        let attr = AttributeType::Composite {
            types: vec![AttributeType::Boolean, AttributeType::String { length: None }],
        };
        assert_eq!(
            TypeRef::from_attribute_type(&attr),
            TypeRef::Union(vec![
                TypeRef::Builtin(BuiltinTypeRef::Boolean),
                TypeRef::Builtin(BuiltinTypeRef::String),
            ])
        );
    }

    #[test]
    fn typeref_from_attribute_composite_recurses_on_members() {
        // Every member goes through the same bridge — a composite whose
        // members are themselves refs must come back with the right prefixes
        // so downstream lowering can pick a `MetadataKind` per branch.
        let attr = AttributeType::Composite {
            types: vec![
                AttributeType::Ref { mdo_type: MdoType::Catalog, name: "Товары".to_string() },
                AttributeType::Number { precision: 10, scale: 0 },
            ],
        };
        match TypeRef::from_attribute_type(&attr) {
            TypeRef::Union(members) => {
                assert_eq!(members.len(), 2);
                match &members[0] {
                    TypeRef::Name(q) => {
                        assert_eq!(q.first().as_str(), "CatalogRef");
                        assert_eq!(q.last().as_str(), "Товары");
                    }
                    other => panic!("expected TypeRef::Name for catalog ref, got {other:?}"),
                }
                assert_eq!(members[1], TypeRef::Builtin(BuiltinTypeRef::Number));
            }
            other => panic!("expected TypeRef::Union, got {other:?}"),
        }
    }

    #[test]
    fn typeref_from_attribute_unknown_passthrough() {
        assert_eq!(TypeRef::from_attribute_type(&AttributeType::Unknown), TypeRef::Unknown);
    }

    #[test]
    fn typeref_from_attribute_register_refs_use_xml_token() {
        // The XML parser (`xml_parser::type_parser::REF_TYPE_MAP`) parses
        // `cfg:InformationRegisterRef.X` into `Ref{InformationRegister, X}`.
        // The bridge must round-trip that token, not invent "RecordKey".
        let reg = AttributeType::Ref {
            mdo_type: MdoType::InformationRegister,
            name: "ЦеныНоменклатуры".to_string(),
        };
        match TypeRef::from_attribute_type(&reg) {
            TypeRef::Name(qname) => {
                assert_eq!(qname.first().as_str(), "InformationRegisterRef");
                assert_eq!(qname.last().as_str(), "ЦеныНоменклатуры");
            }
            other => panic!("expected TypeRef::Name, got {other:?}"),
        }

        for mdo in [
            MdoType::AccumulationRegister,
            MdoType::AccountingRegister,
            MdoType::CalculationRegister,
        ] {
            let attr = AttributeType::Ref { mdo_type: mdo, name: "Х".to_string() };
            match TypeRef::from_attribute_type(&attr) {
                TypeRef::Name(qname) => assert!(
                    qname.first().as_str().ends_with("RegisterRef"),
                    "expected `*RegisterRef` prefix for {mdo:?}, got {}",
                    qname.first().as_str()
                ),
                other => panic!("expected TypeRef::Name for {mdo:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn typeref_from_attribute_constant_uses_value_manager() {
        // `cfg:ConstantValueManager` is the only Constant form the XML parser
        // emits — the bridge mirrors the XML token so lowering can resolve it.
        let attr = AttributeType::Ref {
            mdo_type: MdoType::Constant,
            name: "ВалютаУчёта".to_string(),
        };
        match TypeRef::from_attribute_type(&attr) {
            TypeRef::Name(qname) => {
                assert_eq!(qname.first().as_str(), "ConstantValueManager");
                assert_eq!(qname.last().as_str(), "ВалютаУчёта");
            }
            other => panic!("expected TypeRef::Name, got {other:?}"),
        }
    }

    #[test]
    fn typeref_from_attribute_unreachable_mdo_kinds_fall_to_unknown() {
        // These MDO kinds have no XML reference token at all (parser would
        // never emit `Ref{ExternalDataSource,..}` etc.). If a future parser
        // extension produced one, Unknown is the safe default. DataProcessor
        // and Report are NOT in this list anymore — they round-trip through
        // `cfg:DataProcessorObject` / `cfg:ReportObject` (see
        // `mdo_ref_prefix` for the mapping).
        for mdo in [
            MdoType::ExternalDataSource,
            MdoType::Cube,
            MdoType::DimensionTable,
            MdoType::CommonModule,
        ] {
            let attr = AttributeType::Ref { mdo_type: mdo, name: "Х".to_string() };
            assert_eq!(
                TypeRef::from_attribute_type(&attr),
                TypeRef::Unknown,
                "expected Unknown for {mdo:?}"
            );
        }
    }

    #[test]
    fn typeref_from_attribute_data_processor_and_report_round_trip_via_object_token() {
        // `cfg:DataProcessorObject.X` → `Ref{DataProcessor, X}` → on the way
        // back through `mdo_ref_prefix` we emit `DataProcessorObject` so
        // lowering can find the resolver entry and `MetadataKind::object_kind_for`
        // can promote to a usable Object kind. Symmetric for Report.
        for (mdo, prefix) in
            [(MdoType::DataProcessor, "DataProcessorObject"), (MdoType::Report, "ReportObject")]
        {
            let attr = AttributeType::Ref { mdo_type: mdo, name: "Х".to_string() };
            match TypeRef::from_attribute_type(&attr) {
                TypeRef::Name(qname) => {
                    assert_eq!(qname.first().as_str(), prefix, "mdo_ref_prefix({mdo:?})");
                    assert_eq!(qname.last().as_str(), "Х");
                }
                other => panic!("expected TypeRef::Name({prefix}.Х) for {mdo:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn typeref_from_attribute_uuid_and_value_storage_single_segment() {
        match TypeRef::from_attribute_type(&AttributeType::Uuid) {
            TypeRef::Name(qname) => {
                assert_eq!(qname.len(), 1);
                assert_eq!(qname.first().as_str(), "УникальныйИдентификатор");
            }
            other => panic!("expected single-segment TypeRef::Name, got {other:?}"),
        }

        match TypeRef::from_attribute_type(&AttributeType::ValueStorage) {
            TypeRef::Name(qname) => {
                assert_eq!(qname.len(), 1);
                assert_eq!(qname.first().as_str(), "ХранилищеЗначения");
            }
            other => panic!("expected single-segment TypeRef::Name, got {other:?}"),
        }
    }
}
