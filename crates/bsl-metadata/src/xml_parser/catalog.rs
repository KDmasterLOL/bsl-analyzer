//! Catalog, Document, BusinessProcess, ChartOfCharacteristicTypes, ChartOfAccounts XML parser

use crate::enums::CodeSeries;
use crate::error::{MetadataError, Result};
use crate::metadata_object::{Attribute, MdoType, MetadataObject};
use crate::tabular_section::{TabularSection, TabularSectionAttribute};

use super::helpers::{child_text, find_child, find_mdo_element, parse_uuid, parse_xml};
use super::standard_attributes::{
    add_business_process_standard_attributes, add_catalog_standard_attributes,
    add_chart_of_accounts_standard_attributes,
    add_chart_of_characteristic_types_standard_attributes, add_document_standard_attributes,
    add_exchange_plan_standard_attributes, add_information_register_standard_attributes_as_attrs,
    add_task_standard_attributes, MdoProperties,
};
use super::type_parser::parse_type_xml;

/// Parse Catalog XML from Designer format
///
/// # Arguments
///
/// * `xml` - XML content as string
///
/// # Returns
///
/// Parsed `MetadataObject` structure with attributes
///
/// # Example
///
/// ```no_run
/// # use bsl_metadata::xml_parser::parse_catalog_xml;
/// let xml = std::fs::read_to_string("Catalogs/Валюты.xml")?;
/// let catalog = parse_catalog_xml(&xml)?;
/// # Ok::<(), bsl_metadata::MetadataError>(())
/// ```
pub fn parse_catalog_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_catalog_xml").entered();
    parse_metadata_object_xml(xml, MdoType::Catalog)
}

/// Parse Document XML from Designer format
pub fn parse_document_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_document_xml").entered();
    parse_metadata_object_xml(xml, MdoType::Document)
}

/// Parse BusinessProcess XML from Designer format
pub fn parse_business_process_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_business_process_xml").entered();
    parse_metadata_object_xml(xml, MdoType::BusinessProcess)
}

/// Parse ChartOfCharacteristicTypes XML from Designer format
pub fn parse_chart_of_characteristic_types_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_chart_of_characteristic_types_xml").entered();
    parse_metadata_object_xml(xml, MdoType::ChartOfCharacteristicTypes)
}

/// Parse Task XML from Designer format
pub fn parse_task_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_task_xml").entered();
    parse_metadata_object_xml(xml, MdoType::Task)
}

/// Parse ExchangePlan XML from Designer format
pub fn parse_exchange_plan_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_exchange_plan_xml").entered();
    parse_metadata_object_xml(xml, MdoType::ExchangePlan)
}

/// Parse ChartOfAccounts XML from Designer format
pub fn parse_chart_of_accounts_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_chart_of_accounts_xml").entered();
    parse_metadata_object_xml(xml, MdoType::ChartOfAccounts)
}

/// Internal helper to parse metadata object XML using roxmltree
fn parse_metadata_object_xml(xml: &str, mdo_type: MdoType) -> Result<MetadataObject> {
    let doc = parse_xml(xml)?;

    let mdo_node = find_mdo_element(&doc)
        .ok_or_else(|| MetadataError::InvalidFormat("No MDO element found".to_string()))?;

    let props_node = find_child(mdo_node, "Properties")
        .ok_or_else(|| MetadataError::InvalidFormat("No Properties element found".to_string()))?;

    let properties = MdoProperties::from_node(props_node);

    let mut attributes = Vec::new();
    let mut tabular_sections = Vec::new();

    // Add standard attributes FIRST based on object type
    match mdo_type {
        MdoType::Catalog => {
            add_catalog_standard_attributes(&mut attributes, &properties, mdo_type);
        }
        MdoType::Document => {
            add_document_standard_attributes(&mut attributes, &properties, mdo_type);
        }
        MdoType::BusinessProcess => {
            add_business_process_standard_attributes(&mut attributes, &properties, mdo_type);
        }
        MdoType::Task => {
            add_task_standard_attributes(&mut attributes, &properties, mdo_type);
        }
        MdoType::ExchangePlan => {
            add_exchange_plan_standard_attributes(&mut attributes, &properties, mdo_type);
        }
        MdoType::ChartOfCharacteristicTypes => {
            add_chart_of_characteristic_types_standard_attributes(
                &mut attributes,
                &properties,
                mdo_type,
            );
        }
        MdoType::ChartOfAccounts => {
            add_chart_of_accounts_standard_attributes(&mut attributes, &properties, mdo_type);
        }
        MdoType::InformationRegister => {
            add_information_register_standard_attributes_as_attrs(
                &mut attributes,
                &properties,
                mdo_type,
            );
        }
        _ => {}
    }

    // Parse child objects if present
    if let Some(child_objects) = find_child(mdo_node, "ChildObjects") {
        for child in child_objects.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "Attribute" | "Resource" | "Dimension" => {
                    attributes.push(parse_attribute_node(child)?);
                }
                "TabularSection" => {
                    tabular_sections.push(parse_tabular_section_node(child)?);
                }
                _ => {}
            }
        }
    }

    let mut mdo = MetadataObject::new(mdo_type, properties.name.clone());
    for attr in attributes {
        mdo.add_attribute(attr);
    }
    for ts in tabular_sections {
        mdo.add_tabular_section(ts);
    }

    // Set CheckUnique and CodeSeries for relevant object types
    mdo.set_check_unique(properties.check_unique);
    if let Some(code_series_str) = &properties.code_series {
        mdo.set_code_series(parse_code_series(code_series_str));
    }

    tracing::debug!(
        mdo_name = %mdo.name,
        mdo_type = ?mdo.mdo_type,
        attributes = mdo.attributes.len(),
        tabular_sections = mdo.tabular_sections.len(),
        check_unique = mdo.check_unique,
        code_series = ?mdo.code_series,
        "parsed metadata object"
    );

    Ok(mdo)
}

/// Parse CodeSeries string from XML into enum
fn parse_code_series(s: &str) -> CodeSeries {
    match s {
        "WholeCatalog" | "WholeCharacteristicKind" | "WholeChartOfAccounts" => {
            CodeSeries::WholeCatalog
        }
        "WithinSubordination" => CodeSeries::WithinSubordination,
        "WithinOwnerSubordination" | "WithinOwner" => CodeSeries::WithinOwnerSubordination,
        _ => CodeSeries::Unknown,
    }
}

/// Parse single attribute from an `<Attribute>` (or `<Resource>` / `<Dimension>`) node
fn parse_attribute_node(node: roxmltree::Node<'_, '_>) -> Result<Attribute> {
    let props = find_child(node, "Properties")
        .ok_or_else(|| MetadataError::InvalidFormat("Attribute missing Properties".to_string()))?;
    let name = child_text(props, "Name").unwrap_or("").to_string();
    let _span = tracing::debug_span!("parse_attribute", attr_name = %name).entered();

    let type_node = find_child(props, "Type").ok_or_else(|| {
        MetadataError::InvalidFormat(format!("Attribute '{}' missing Type", name))
    })?;
    let attr_type = parse_type_xml(type_node)?;
    Ok(Attribute { name, name_en: None, attr_type })
}

/// Parse TabularSection node
fn parse_tabular_section_node(node: roxmltree::Node<'_, '_>) -> Result<TabularSection> {
    let uuid_str = node.attribute("uuid").unwrap_or("");
    let uuid = parse_uuid(uuid_str, "tabular section")?;

    let props = find_child(node, "Properties").ok_or_else(|| {
        MetadataError::InvalidFormat("TabularSection missing Properties".to_string())
    })?;

    let name = child_text(props, "Name").unwrap_or("").to_string();
    let mut tabular_section = TabularSection::new(uuid, name);

    // Set synonym if present — handle both text content and empty element
    if let Some(synonym_node) = find_child(props, "Synonym") {
        tabular_section.set_synonym(synonym_node.text().map(|s| s.to_string()));
    }

    // Set use mode if present
    let use_mode = child_text(props, "Use").map(|s| s.to_string());
    tabular_section.set_use_mode(use_mode);

    // Parse attributes of the tabular section
    let Some(child_objects) = find_child(node, "ChildObjects") else {
        return Ok(tabular_section);
    };

    let mut ts_attributes = Vec::new();
    for attr_node in
        child_objects.children().filter(|n| n.is_element() && n.tag_name().name() == "Attribute")
    {
        let attr_uuid_str = attr_node.attribute("uuid").unwrap_or("");
        let attr_uuid = parse_uuid(attr_uuid_str, "tabular section attribute")?;

        let attr_props = find_child(attr_node, "Properties").ok_or_else(|| {
            MetadataError::InvalidFormat("TS Attribute missing Properties".to_string())
        })?;
        let attr_name = child_text(attr_props, "Name").unwrap_or("").to_string();

        let _attr_span = tracing::debug_span!(
            "parse_ts_attribute",
            ts_name = %tabular_section.name(),
            attr_name = %attr_name
        )
        .entered();

        let type_node = find_child(attr_props, "Type").ok_or_else(|| {
            MetadataError::InvalidFormat(format!("TS Attribute '{}' missing Type", attr_name))
        })?;
        let attr_type = parse_type_xml(type_node)?;

        ts_attributes.push(TabularSectionAttribute::new(attr_uuid, attr_name, attr_type));
    }

    tabular_section.set_attributes(ts_attributes);
    Ok(tabular_section)
}
