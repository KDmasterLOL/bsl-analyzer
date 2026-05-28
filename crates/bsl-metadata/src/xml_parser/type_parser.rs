//! Type parsing functions for XML metadata

use crate::error::Result;
use crate::metadata_object::{AttributeType, MdoType, PlatformValueType};

/// Mapping from reference type prefix to MdoType
static REF_TYPE_MAP: &[(&str, MdoType)] = &[
    ("cfg:CatalogRef", MdoType::Catalog),
    ("cfg:CatalogObject", MdoType::Catalog),
    ("cfg:DocumentRef", MdoType::Document),
    ("cfg:DocumentObject", MdoType::Document),
    ("cfg:InformationRegisterRef", MdoType::InformationRegister),
    ("cfg:AccumulationRegisterRef", MdoType::AccumulationRegister),
    ("cfg:AccountingRegisterRef", MdoType::AccountingRegister),
    ("cfg:CalculationRegisterRef", MdoType::CalculationRegister),
    ("cfg:EnumRef", MdoType::Enum),
    ("cfg:TaskRef", MdoType::Task),
    ("cfg:TaskObject", MdoType::Task),
    ("cfg:ExchangePlanRef", MdoType::ExchangePlan),
    ("cfg:ExchangePlanObject", MdoType::ExchangePlan),
    ("cfg:BusinessProcessRef", MdoType::BusinessProcess),
    ("cfg:BusinessProcessObject", MdoType::BusinessProcess),
    ("cfg:BusinessProcessRoutePointRef", MdoType::BusinessProcess),
    ("cfg:ChartOfCharacteristicTypesRef", MdoType::ChartOfCharacteristicTypes),
    ("cfg:ChartOfCharacteristicTypesObject", MdoType::ChartOfCharacteristicTypes),
    ("cfg:ChartOfAccountsRef", MdoType::ChartOfAccounts),
    ("cfg:ChartOfAccountsObject", MdoType::ChartOfAccounts),
    ("cfg:ChartOfCalculationTypesRef", MdoType::ChartOfCalculationTypes),
    ("cfg:ChartOfCalculationTypesObject", MdoType::ChartOfCalculationTypes),
    ("cfg:DataProcessorObject", MdoType::DataProcessor),
    ("cfg:ReportObject", MdoType::Report),
    ("cfg:ConstantValueManager", MdoType::Constant),
    ("cfg:InformationRegisterRecordSet", MdoType::InformationRegister),
    ("cfg:AccumulationRegisterRecordSet", MdoType::AccumulationRegister),
    ("cfg:AccountingRegisterRecordSet", MdoType::AccountingRegister),
    ("cfg:CalculationRegisterRecordSet", MdoType::CalculationRegister),
];

/// Mapping from TypeSet string to AttributeType for "any object of type" patterns
static TYPE_SET_MAP: &[(&str, MdoType)] = &[
    ("cfg:CatalogRef", MdoType::Catalog),
    ("cfg:DocumentRef", MdoType::Document),
    ("cfg:BusinessProcessRef", MdoType::BusinessProcess),
    ("cfg:TaskRef", MdoType::Task),
    ("cfg:EnumRef", MdoType::Enum),
    ("cfg:InformationRegisterRef", MdoType::InformationRegister),
    ("cfg:AccumulationRegisterRef", MdoType::AccumulationRegister),
    ("cfg:AccountingRegisterRef", MdoType::AccountingRegister),
    ("cfg:CalculationRegisterRef", MdoType::CalculationRegister),
    ("cfg:ChartOfCharacteristicTypesRef", MdoType::ChartOfCharacteristicTypes),
    ("cfg:ChartOfAccountsRef", MdoType::ChartOfAccounts),
    ("cfg:ChartOfCalculationTypesRef", MdoType::ChartOfCalculationTypes),
    ("cfg:ExchangePlanRef", MdoType::ExchangePlan),
];

/// Mapping from object type (without name) to MdoType
static OBJECT_TYPE_MAP: &[(&str, MdoType)] = &[
    ("cfg:CatalogObject", MdoType::Catalog),
    ("cfg:DocumentObject", MdoType::Document),
    ("cfg:BusinessProcessObject", MdoType::BusinessProcess),
    ("cfg:TaskObject", MdoType::Task),
    ("cfg:DataProcessorObject", MdoType::DataProcessor),
    ("cfg:ReportObject", MdoType::Report),
    ("cfg:BusinessProcessRoutePointRef", MdoType::BusinessProcess),
];

/// Qualifiers extracted from a `<Type>` element
struct TypeQualifiers {
    string_length: Option<u32>,
    number_digits: Option<u8>,
    number_fraction_digits: Option<u8>,
    date_fractions: Option<String>,
}

impl TypeQualifiers {
    fn from_node(type_node: roxmltree::Node<'_, '_>) -> Self {
        let string_length = type_node
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "StringQualifiers")
            .and_then(|sq| {
                sq.children()
                    .find(|n| n.is_element() && n.tag_name().name() == "Length")
                    .and_then(|n| n.text())
                    .and_then(|s| s.parse().ok())
            });

        let (number_digits, number_fraction_digits) = type_node
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "NumberQualifiers")
            .map(|nq| {
                let digits = nq
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "Digits")
                    .and_then(|n| n.text())
                    .and_then(|s| s.parse().ok());
                let frac = nq
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "FractionDigits")
                    .and_then(|n| n.text())
                    .and_then(|s| s.parse().ok());
                (digits, frac)
            })
            .unwrap_or((None, None));

        let date_fractions = type_node
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "DateQualifiers")
            .and_then(|dq| {
                dq.children()
                    .find(|n| n.is_element() && n.tag_name().name() == "DateFractions")
                    .and_then(|n| n.text())
                    .map(|s| s.to_string())
            });

        TypeQualifiers { string_length, number_digits, number_fraction_digits, date_fractions }
    }
}

/// Parse a `<Type>` element node into `AttributeType`.
///
/// The node is a `<Type>` element containing:
/// - `<Type>xs:string</Type>` (may be multiple, ignoring namespace prefix)
/// - `<TypeSet>cfg:DefinedType.Name</TypeSet>` (may be multiple)
/// - `<StringQualifiers><Length>100</Length></StringQualifiers>`
/// - `<NumberQualifiers><Digits>10</Digits><FractionDigits>2</FractionDigits></NumberQualifiers>`
/// - `<DateQualifiers><DateFractions>DateTime</DateFractions></DateQualifiers>`
pub(crate) fn parse_type_xml(type_node: roxmltree::Node<'_, '_>) -> Result<AttributeType> {
    let qualifiers = TypeQualifiers::from_node(type_node);

    // Collect all <Type> children text values
    let type_strs: Vec<&str> = type_node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Type")
        .filter_map(|n| n.text())
        .collect();

    // Collect all <TypeSet> children text values
    let type_set_strs: Vec<&str> = type_node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "TypeSet")
        .filter_map(|n| n.text())
        .collect();

    let mut all_types = Vec::new();

    // Parse all concrete types from Type elements
    for type_str in &type_strs {
        let parsed_type = parse_single_type(type_str, &qualifiers)?;
        all_types.push(parsed_type);
    }

    // Check for TypeSet and add to list if recognized
    if let Some(type_set) = type_set_strs.first() {
        tracing::debug!(
            type_set = %type_set,
            concrete_types_count = type_strs.len(),
            "parse_type_xml: found TypeSet"
        );

        if let Some(parsed) = parse_type_set(type_set) {
            tracing::debug!(type_set = %type_set, "adding TypeSet to types list");
            all_types.push(parsed);
        }
    }

    // Return based on collected types
    match all_types.len() {
        0 => {
            tracing::warn!(
                types = ?type_strs,
                type_sets = ?type_set_strs,
                "parse_type_xml: no types collected, returning Unknown"
            );
            Ok(AttributeType::Unknown)
        }
        1 => {
            tracing::debug!("parse_type_xml: single type");
            Ok(all_types.into_iter().next().unwrap())
        }
        _ => {
            tracing::debug!(types_count = all_types.len(), "parse_type_xml: composite type");
            Ok(AttributeType::Composite { types: all_types })
        }
    }
}

/// Parse TypeSet string into AttributeType
fn parse_type_set(type_set: &str) -> Option<AttributeType> {
    // cfg:AnyIBRef -> AnyRef
    if type_set == "cfg:AnyIBRef" {
        return Some(AttributeType::AnyRef);
    }

    // cfg:BusinessProcessRoutePointRef -> any route point
    if type_set == "cfg:BusinessProcessRoutePointRef" {
        return Some(AttributeType::AnyObjectRef { mdo_type: MdoType::BusinessProcess });
    }

    // DefinedType reference: cfg:DefinedType.Name
    if let Some(name) = type_set.strip_prefix("cfg:DefinedType.") {
        return Some(AttributeType::DefinedType { name: name.to_string() });
    }

    // Characteristic reference: cfg:Characteristic.Name -> ChartOfCharacteristicTypes
    if let Some(name) = type_set.strip_prefix("cfg:Characteristic.") {
        return Some(AttributeType::Ref {
            mdo_type: MdoType::ChartOfCharacteristicTypes,
            name: name.to_string(),
        });
    }

    // Check TYPE_SET_MAP for "any object of specific type"
    if let Some((_, mdo_type)) = TYPE_SET_MAP.iter().find(|(k, _)| *k == type_set) {
        return Some(AttributeType::AnyObjectRef { mdo_type: *mdo_type });
    }

    tracing::debug!(type_set = %type_set, "ignoring unrecognized TypeSet");
    None
}

/// Parse a single type string
fn parse_single_type(type_str: &str, qualifiers: &TypeQualifiers) -> Result<AttributeType> {
    tracing::debug!(type_str = %type_str, "parse_single_type");

    match type_str {
        "xs:boolean" => Ok(AttributeType::Boolean),

        "xs:string" => Ok(AttributeType::String { length: qualifiers.string_length }),

        "xs:decimal" => {
            let precision = qualifiers.number_digits.unwrap_or(10);
            let scale = qualifiers.number_fraction_digits.unwrap_or(0);
            Ok(AttributeType::Number { precision, scale })
        }

        "xs:dateTime" => {
            let is_datetime = qualifiers
                .date_fractions
                .as_deref()
                .is_some_and(|df| df.eq_ignore_ascii_case("DateTime"));

            if is_datetime {
                Ok(AttributeType::DateTime)
            } else {
                Ok(AttributeType::Date)
            }
        }

        // Reference types: "cfg:CatalogRef.Name", "cfg:DocumentRef.Name"
        s if s.starts_with("cfg:") => parse_reference_type(s),

        // Special types
        "v8:UUID" => Ok(AttributeType::Uuid),
        "v8:ValueStorage" => Ok(AttributeType::ValueStorage),

        // Platform value types — fixed `v8:` XDTO vocabulary. Classify the
        // token; the kernel mapping is the HIR bridge's job. `v8:FillChecking`
        // is intentionally absent: it names a metadata enum-value domain, not
        // a form-attribute value type.
        _ => match parse_platform_value_type(type_str) {
            Some(pvt) => Ok(AttributeType::Platform(pvt)),
            None => {
                tracing::warn!(type_str = %type_str, "unknown type");
                Ok(AttributeType::Unknown)
            }
        },
    }
}

/// Classify a `v8:` platform value-type token.
///
/// Pure token → [`PlatformValueType`] classification over the fixed XDTO
/// vocabulary — no kernel/HIR knowledge. `v8:UUID` / `v8:ValueStorage` are
/// handled by their own dedicated [`AttributeType`] variants and never reach
/// here. `v8:FillChecking` is deliberately omitted (enum-value domain, not a
/// value type).
fn parse_platform_value_type(type_str: &str) -> Option<PlatformValueType> {
    let pvt = match type_str {
        "v8:ValueListType" => PlatformValueType::ValueList,
        "v8:ValueTable" => PlatformValueType::ValueTable,
        "v8:ValueTree" => PlatformValueType::ValueTree,
        "v8:StandardPeriod" => PlatformValueType::StandardPeriod,
        "v8:StandardBeginningDate" => PlatformValueType::StandardBeginningDate,
        "v8:TypeDescription" => PlatformValueType::TypeDescription,
        "v8:FixedStructure" => PlatformValueType::FixedStructure,
        "v8:FixedArray" => PlatformValueType::FixedArray,
        "v8:FixedMap" => PlatformValueType::FixedMap,
        "v8:Type" => PlatformValueType::Type,
        "v8:Null" => PlatformValueType::Null,
        _ => return None,
    };
    Some(pvt)
}

/// Parse reference type string like "cfg:CatalogRef.Валюты"
fn parse_reference_type(type_str: &str) -> Result<AttributeType> {
    // Check for object types without name (e.g., cfg:CatalogObject)
    if let Some((_, mdo_type)) = OBJECT_TYPE_MAP.iter().find(|(k, _)| *k == type_str) {
        tracing::info!(type_str = %type_str, "Matched object type special case");
        return Ok(AttributeType::AnyObjectRef { mdo_type: *mdo_type });
    }

    // Format: "cfg:CatalogRef.Name" or "cfg:DocumentRef.Name"
    let parts: Vec<&str> = type_str.split('.').collect();
    if parts.len() != 2 {
        tracing::warn!(type_str = %type_str, "invalid reference type format");
        return Ok(AttributeType::Unknown);
    }

    let ref_type = parts[0];
    let name = parts[1].to_string();

    // Lookup in REF_TYPE_MAP
    if let Some((_, mdo_type)) = REF_TYPE_MAP.iter().find(|(k, _)| *k == ref_type) {
        return Ok(AttributeType::Ref { mdo_type: *mdo_type, name });
    }

    tracing::warn!(
        ref_type = %ref_type,
        full_type_str = %type_str,
        "unsupported reference type"
    );
    Ok(AttributeType::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qualifiers() -> TypeQualifiers {
        TypeQualifiers {
            string_length: None,
            number_digits: None,
            number_fraction_digits: None,
            date_fractions: None,
        }
    }

    #[test]
    fn classifies_every_platform_value_token() {
        for (token, expected) in [
            ("v8:ValueListType", PlatformValueType::ValueList),
            ("v8:ValueTable", PlatformValueType::ValueTable),
            ("v8:ValueTree", PlatformValueType::ValueTree),
            ("v8:StandardPeriod", PlatformValueType::StandardPeriod),
            ("v8:StandardBeginningDate", PlatformValueType::StandardBeginningDate),
            ("v8:TypeDescription", PlatformValueType::TypeDescription),
            ("v8:FixedStructure", PlatformValueType::FixedStructure),
            ("v8:FixedArray", PlatformValueType::FixedArray),
            ("v8:FixedMap", PlatformValueType::FixedMap),
            ("v8:Type", PlatformValueType::Type),
            ("v8:Null", PlatformValueType::Null),
        ] {
            assert_eq!(parse_platform_value_type(token), Some(expected), "token {token}");
            assert_eq!(
                parse_single_type(token, &qualifiers()).unwrap(),
                AttributeType::Platform(expected),
                "single-type {token}",
            );
        }
    }

    #[test]
    fn uuid_and_value_storage_keep_dedicated_variants() {
        // These two `v8:` tokens have their own `AttributeType` variants and
        // must NOT be reclassified as `Platform(_)`.
        assert_eq!(parse_platform_value_type("v8:UUID"), None);
        assert_eq!(parse_platform_value_type("v8:ValueStorage"), None);
        assert_eq!(parse_single_type("v8:UUID", &qualifiers()).unwrap(), AttributeType::Uuid);
        assert_eq!(
            parse_single_type("v8:ValueStorage", &qualifiers()).unwrap(),
            AttributeType::ValueStorage
        );
    }

    #[test]
    fn fill_checking_and_unrelated_tokens_stay_unknown() {
        // `v8:FillChecking` names a metadata enum-value domain, not a value
        // type — it must stay Unknown rather than be mapped to a platform type.
        assert_eq!(parse_platform_value_type("v8:FillChecking"), None);
        assert_eq!(parse_platform_value_type("v8:Nonsense"), None);
        assert_eq!(
            parse_single_type("v8:FillChecking", &qualifiers()).unwrap(),
            AttributeType::Unknown
        );
    }
}
