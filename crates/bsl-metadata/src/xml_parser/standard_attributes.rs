//! Standard attributes handling for different MDO types

use crate::metadata_object::{Attribute, AttributeType, MdoType, StandardAttributeKind};
use crate::register::RegisterAttribute;

use super::helpers::{create_register_standard_attribute, create_standard_attribute};
use super::serde_types::{MetadataObjectProperties, OwnerItemXml};

// ============================================================================
// Register standard attributes
// ============================================================================

/// Add common standard attributes for all register types (Active, LineNumber, Recorder)
pub(crate) fn add_register_common_attrs(
    attributes: &mut Vec<RegisterAttribute>,
    object_name: &str,
    mdo_type: MdoType,
) {
    // Active - always present
    attributes.push(create_register_standard_attribute(
        &StandardAttributeKind::Active,
        mdo_type,
        object_name,
    ));

    // LineNumber - always present
    attributes.push(create_register_standard_attribute(
        &StandardAttributeKind::LineNumber,
        mdo_type,
        object_name,
    ));

    // Recorder - always present
    attributes.push(create_register_standard_attribute(
        &StandardAttributeKind::Recorder,
        mdo_type,
        object_name,
    ));
}

/// Add Period attribute for registers (conditional based on periodicity)
pub(crate) fn add_register_period_attr(
    attributes: &mut Vec<RegisterAttribute>,
    object_name: &str,
    mdo_type: MdoType,
    periodicity: Option<&str>,
) {
    let should_add_period = match mdo_type {
        // InformationRegister: add Period only if not Nonperiodical
        MdoType::InformationRegister => periodicity.unwrap_or("Nonperiodical") != "Nonperiodical",
        // AccumulationRegister: Period is ALWAYS present
        MdoType::AccumulationRegister => true,
        // Other register types: no Period handling yet
        _ => false,
    };

    if should_add_period {
        attributes.push(create_register_standard_attribute(
            &StandardAttributeKind::Period,
            mdo_type,
            object_name,
        ));
    }
}

/// Add standard attributes for InformationRegister
pub(crate) fn add_information_register_standard_attrs(
    attributes: &mut Vec<RegisterAttribute>,
    object_name: &str,
    periodicity: Option<&str>,
) {
    add_register_common_attrs(attributes, object_name, MdoType::InformationRegister);
    add_register_period_attr(attributes, object_name, MdoType::InformationRegister, periodicity);
}

/// Add standard attributes for AccumulationRegister
pub(crate) fn add_accumulation_register_standard_attrs(
    attributes: &mut Vec<RegisterAttribute>,
    object_name: &str,
) {
    add_register_common_attrs(attributes, object_name, MdoType::AccumulationRegister);
    // AccumulationRegister always has Period
    add_register_period_attr(attributes, object_name, MdoType::AccumulationRegister, None);
}

// ============================================================================
// Catalog/Document standard attributes
// ============================================================================

/// Add standard attributes for Catalog/Document objects
pub(crate) fn add_catalog_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MetadataObjectProperties,
    mdo_type: MdoType,
) {
    let object_name = &properties.name;

    // Ref - always present
    attributes.push(create_standard_attribute(&StandardAttributeKind::Ref, mdo_type, object_name));

    // DeletionMark - always present
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::DeletionMark,
        mdo_type,
        object_name,
    ));

    // Code - only if CodeLength > 0
    if let Some(length) = properties.code_length.as_ref().and_then(|v| v.value).filter(|&l| l > 0) {
        let kind = StandardAttributeKind::Code { length };
        attributes.push(create_standard_attribute(&kind, mdo_type, object_name));
    }

    // Description - only if DescriptionLength > 0
    if let Some(length) =
        properties.description_length.as_ref().and_then(|v| v.value).filter(|&l| l > 0)
    {
        let kind = StandardAttributeKind::Description { length };
        attributes.push(create_standard_attribute(&kind, mdo_type, object_name));
    }

    // IsFolder/Parent - only if Hierarchical
    if bool::from(properties.hierarchical.clone()) {
        attributes.push(create_standard_attribute(
            &StandardAttributeKind::IsFolder,
            mdo_type,
            object_name,
        ));

        attributes.push(create_standard_attribute(
            &StandardAttributeKind::Parent,
            mdo_type,
            object_name,
        ));
    }

    // Owner - only if Owners is not empty
    if let Some(owners_xml) = &properties.owners {
        if !owners_xml.items.is_empty() {
            let owner_types = parse_owner_types(&owners_xml.items);
            let owner_attr_type = match owner_types.len() {
                0 => AttributeType::Unknown,
                1 => owner_types.into_iter().next().unwrap(),
                _ => AttributeType::Composite { types: owner_types },
            };

            attributes.push(Attribute {
                name: StandardAttributeKind::Owner.russian_name().to_string(),
                name_en: Some(StandardAttributeKind::Owner.english_name().to_string()),
                attr_type: owner_attr_type,
            });
        }
    }

    // Predefined - always present
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::Predefined,
        mdo_type,
        object_name,
    ));

    // PredefinedDataName - always present
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::PredefinedDataName,
        mdo_type,
        object_name,
    ));
}

/// Add standard attributes for InformationRegister (Attribute variant)
pub(crate) fn add_information_register_standard_attributes_as_attrs(
    attributes: &mut Vec<Attribute>,
    properties: &MetadataObjectProperties,
    mdo_type: MdoType,
) {
    let object_name = &properties.name;

    // Active - always present
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::Active,
        mdo_type,
        object_name,
    ));

    // LineNumber - always present
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::LineNumber,
        mdo_type,
        object_name,
    ));

    // Recorder - always present
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::Recorder,
        mdo_type,
        object_name,
    ));

    // Period - only if periodic (not Nonperiodical)
    let periodicity = properties.periodicity.as_deref().unwrap_or("Nonperiodical");
    if periodicity != "Nonperiodical" {
        attributes.push(create_standard_attribute(
            &StandardAttributeKind::Period,
            mdo_type,
            object_name,
        ));
    }
}

/// Parse owner types from Owners XML items
fn parse_owner_types(items: &[OwnerItemXml]) -> Vec<AttributeType> {
    items
        .iter()
        .filter_map(|item| {
            // Parse format: "Catalog.Контрагенты" or "ChartOfCharacteristicTypes.Свойства"
            let parts: Vec<&str> = item.value.split('.').collect();
            if parts.len() != 2 {
                tracing::warn!(value = %item.value, "Invalid owner format, expected Type.Name");
                return None;
            }

            let type_str = parts[0];
            let name = parts[1].to_string();

            match type_str.parse::<MdoType>() {
                Ok(mdo_type) => Some(AttributeType::Ref { mdo_type, name }),
                Err(e) => {
                    tracing::warn!(type_str = %type_str, error = %e, "Unknown MDO type in owner");
                    None
                }
            }
        })
        .collect()
}
