//! Standard attributes handling for different MDO types

use crate::metadata_object::{Attribute, AttributeType, MdoType, StandardAttributeKind};
use crate::register::RegisterAttribute;

use super::helpers::{
    child_bool, child_text, child_u32, create_register_standard_attribute,
    create_standard_attribute, find_child,
};

// ============================================================================
// MdoProperties - extracted properties from MDO XML
// ============================================================================

/// Extracted properties from MDO XML, used for standard attribute generation
pub(crate) struct MdoProperties {
    pub name: String,
    pub code_length: Option<u32>,
    pub number_length: Option<u32>,
    pub description_length: Option<u32>,
    pub hierarchical: bool,
    pub owners: Vec<String>,
    pub periodicity: Option<String>,
    pub check_unique: bool,
    pub code_series: Option<String>,
}

impl MdoProperties {
    /// Extract properties from a `<Properties>` roxmltree node
    pub(crate) fn from_node(props_node: roxmltree::Node<'_, '_>) -> Self {
        let name = child_text(props_node, "Name").unwrap_or("").to_string();
        let code_length = child_u32(props_node, "CodeLength");
        let number_length = child_u32(props_node, "NumberLength");
        let description_length = child_u32(props_node, "DescriptionLength");
        let hierarchical = child_bool(props_node, "Hierarchical");
        let check_unique = child_bool(props_node, "CheckUnique");
        let periodicity =
            child_text(props_node, "InformationRegisterPeriodicity").map(|s| s.to_string());
        let code_series = child_text(props_node, "CodeSeries").map(|s| s.to_string());

        // Collect <Owners><Item>text</Item></Owners>
        let owners = find_child(props_node, "Owners")
            .map(|owners_node| {
                owners_node
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "Item")
                    .filter_map(|n| n.text())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        MdoProperties {
            name,
            code_length,
            number_length,
            description_length,
            hierarchical,
            owners,
            periodicity,
            check_unique,
            code_series,
        }
    }
}

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
    properties: &MdoProperties,
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
    if let Some(length) = properties.code_length.filter(|&l| l > 0) {
        let kind = StandardAttributeKind::Code { length };
        attributes.push(create_standard_attribute(&kind, mdo_type, object_name));
    }

    // Description - only if DescriptionLength > 0
    if let Some(length) = properties.description_length.filter(|&l| l > 0) {
        let kind = StandardAttributeKind::Description { length };
        attributes.push(create_standard_attribute(&kind, mdo_type, object_name));
    }

    // IsFolder/Parent - only if Hierarchical
    if properties.hierarchical {
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

    // Owner - only if owners is not empty
    if !properties.owners.is_empty() {
        let owner_types = parse_owner_types(&properties.owners);
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

/// Add standard attributes for Document objects
///
/// Documents have: Ref, DeletionMark, Number (if NumberLength > 0), Date, Posted
pub(crate) fn add_document_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
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

    // Number - only if NumberLength > 0
    if let Some(length) = properties.number_length.filter(|&l| l > 0) {
        let kind = StandardAttributeKind::Number { length };
        attributes.push(create_standard_attribute(&kind, mdo_type, object_name));
    }

    // Date - always present
    attributes.push(create_standard_attribute(&StandardAttributeKind::Date, mdo_type, object_name));

    // Posted - always present for Document
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::Posted,
        mdo_type,
        object_name,
    ));
}

/// Add standard attributes for BusinessProcess objects
///
/// BusinessProcesses have: Ref, DeletionMark, Number (if NumberLength > 0), Date,
/// Started, Completed, HeadTask
pub(crate) fn add_business_process_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
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

    // Number - only if NumberLength > 0
    if let Some(length) = properties.number_length.filter(|&l| l > 0) {
        let kind = StandardAttributeKind::Number { length };
        attributes.push(create_standard_attribute(&kind, mdo_type, object_name));
    }

    // Date - always present
    attributes.push(create_standard_attribute(&StandardAttributeKind::Date, mdo_type, object_name));

    // Started - always present for BusinessProcess
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::Started,
        mdo_type,
        object_name,
    ));

    // Completed - always present for BusinessProcess
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::Completed,
        mdo_type,
        object_name,
    ));

    // HeadTask - always present for BusinessProcess
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::HeadTask,
        mdo_type,
        object_name,
    ));
}

/// Add standard attributes for Task objects
///
/// Tasks have: Ref, DeletionMark, Number (if NumberLength > 0), Date,
/// Executed, TaskBusinessProcess, RoutePoint
pub(crate) fn add_task_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
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

    // Number - only if NumberLength > 0
    if let Some(length) = properties.number_length.filter(|&l| l > 0) {
        let kind = StandardAttributeKind::Number { length };
        attributes.push(create_standard_attribute(&kind, mdo_type, object_name));
    }

    // Date - always present
    attributes.push(create_standard_attribute(&StandardAttributeKind::Date, mdo_type, object_name));

    // Executed - always present for Task
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::Executed,
        mdo_type,
        object_name,
    ));

    // TaskBusinessProcess - always present for Task
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::TaskBusinessProcess,
        mdo_type,
        object_name,
    ));

    // RoutePoint - always present for Task
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::RoutePoint,
        mdo_type,
        object_name,
    ));
}

/// Add standard attributes for ExchangePlan objects
///
/// ExchangePlans have: same as Catalog + ThisNode
pub(crate) fn add_exchange_plan_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
    mdo_type: MdoType,
) {
    add_catalog_standard_attributes(attributes, properties, mdo_type);

    let object_name = &properties.name;
    // ThisNode - always present for ExchangePlan
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::ThisNode,
        mdo_type,
        object_name,
    ));
}

/// Add standard attributes for ChartOfCharacteristicTypes objects
///
/// ChartOfCharacteristicTypes have: same as Catalog + ValueType
pub(crate) fn add_chart_of_characteristic_types_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
    mdo_type: MdoType,
) {
    add_catalog_standard_attributes(attributes, properties, mdo_type);

    let object_name = &properties.name;
    // ValueType - always present for ChartOfCharacteristicTypes
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::ValueType,
        mdo_type,
        object_name,
    ));
}

/// Add standard attributes for ChartOfAccounts objects
///
/// ChartOfAccounts have: same as Catalog + Order
pub(crate) fn add_chart_of_accounts_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
    mdo_type: MdoType,
) {
    add_catalog_standard_attributes(attributes, properties, mdo_type);

    let object_name = &properties.name;
    // Order - always present for ChartOfAccounts
    attributes.push(create_standard_attribute(
        &StandardAttributeKind::Order,
        mdo_type,
        object_name,
    ));
}

/// Add standard attributes for InformationRegister (Attribute variant)
pub(crate) fn add_information_register_standard_attributes_as_attrs(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
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

/// Parse owner types from owner string values
fn parse_owner_types(items: &[String]) -> Vec<AttributeType> {
    items
        .iter()
        .filter_map(|item| {
            // Parse format: "Catalog.Контрагенты" or "ChartOfCharacteristicTypes.Свойства"
            let parts: Vec<&str> = item.split('.').collect();
            if parts.len() != 2 {
                tracing::warn!(value = %item, "Invalid owner format, expected Type.Name");
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
