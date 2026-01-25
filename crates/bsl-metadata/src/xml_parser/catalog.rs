//! Catalog, Document, BusinessProcess, ChartOfCharacteristicTypes, ChartOfAccounts XML parser

use crate::enums::CodeSeries;
use crate::error::Result;
use crate::metadata_object::{Attribute, MdoType, MetadataObject};
use crate::tabular_section::{TabularSection, TabularSectionAttribute};

use super::helpers::parse_uuid;
use super::serde_types::{
    AttributeXml, BusinessProcessRoot, CatalogRoot, ChartOfAccountsRoot,
    ChartOfCharacteristicTypesRoot, DocumentRoot, ExchangePlanRoot, MetadataObjectXml,
    TabularSectionXml, TaskRoot,
};
use super::standard_attributes::{
    add_catalog_standard_attributes, add_information_register_standard_attributes_as_attrs,
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

    let root: CatalogRoot = quick_xml::de::from_str(xml)?;
    parse_metadata_object(root.catalog, MdoType::Catalog)
}

/// Parse Document XML from Designer format
pub fn parse_document_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_document_xml").entered();

    let root: DocumentRoot = quick_xml::de::from_str(xml)?;
    parse_metadata_object(root.document, MdoType::Document)
}

/// Parse BusinessProcess XML from Designer format
pub fn parse_business_process_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_business_process_xml").entered();

    let root: BusinessProcessRoot = quick_xml::de::from_str(xml)?;
    parse_metadata_object(root.business_process, MdoType::BusinessProcess)
}

/// Parse ChartOfCharacteristicTypes XML from Designer format
pub fn parse_chart_of_characteristic_types_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_chart_of_characteristic_types_xml").entered();

    let root: ChartOfCharacteristicTypesRoot = quick_xml::de::from_str(xml)?;
    parse_metadata_object(root.chart_of_characteristic_types, MdoType::ChartOfCharacteristicTypes)
}

/// Parse Task XML from Designer format
pub fn parse_task_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_task_xml").entered();

    let root: TaskRoot = quick_xml::de::from_str(xml)?;
    parse_metadata_object(root.task, MdoType::Task)
}

/// Parse ExchangePlan XML from Designer format
pub fn parse_exchange_plan_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_exchange_plan_xml").entered();

    let root: ExchangePlanRoot = quick_xml::de::from_str(xml)?;
    parse_metadata_object(root.exchange_plan, MdoType::ExchangePlan)
}

/// Parse ChartOfAccounts XML from Designer format
pub fn parse_chart_of_accounts_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_chart_of_accounts_xml").entered();

    let root: ChartOfAccountsRoot = quick_xml::de::from_str(xml)?;
    parse_metadata_object(root.chart_of_accounts, MdoType::ChartOfAccounts)
}

/// Internal helper to parse metadata object XML
fn parse_metadata_object(obj_xml: MetadataObjectXml, mdo_type: MdoType) -> Result<MetadataObject> {
    let mut attributes = Vec::new();
    let mut tabular_sections = Vec::new();

    // Add standard attributes FIRST based on object type
    match mdo_type {
        MdoType::Catalog | MdoType::Document => {
            add_catalog_standard_attributes(&mut attributes, &obj_xml.properties, mdo_type);
        }
        MdoType::InformationRegister => {
            add_information_register_standard_attributes_as_attrs(
                &mut attributes,
                &obj_xml.properties,
                mdo_type,
            );
        }
        _ => {
            // Other types don't have standard attributes yet
        }
    }

    // Parse child objects if present
    if let Some(child_objects) = obj_xml.child_objects {
        // Parse regular Attributes (for Catalog, Document)
        for attr_xml in child_objects.attributes {
            attributes.push(parse_attribute(attr_xml)?);
        }

        // Parse Resources (for InformationRegister - treated as attributes)
        for resource_xml in child_objects.resources {
            attributes.push(parse_attribute(resource_xml)?);
        }

        // Parse Dimensions (for InformationRegister - treated as attributes)
        for dim_xml in child_objects.dimensions_as_attributes {
            attributes.push(parse_attribute(dim_xml)?);
        }

        // Parse Tabular Sections
        for ts_xml in child_objects.tabular_sections {
            tabular_sections.push(parse_tabular_section(ts_xml)?);
        }
    }

    let mut mdo = MetadataObject::new(mdo_type, obj_xml.properties.name.clone());
    for attr in attributes {
        mdo.add_attribute(attr);
    }
    for ts in tabular_sections {
        mdo.add_tabular_section(ts);
    }

    // Set CheckUnique and CodeSeries for relevant object types
    mdo.set_check_unique(obj_xml.properties.check_unique.into());
    if let Some(code_series_str) = &obj_xml.properties.code_series {
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

/// Parse single attribute from XML
fn parse_attribute(attr_xml: AttributeXml) -> Result<Attribute> {
    let _span =
        tracing::debug_span!("parse_attribute", attr_name = %attr_xml.properties.name).entered();
    let attr_type = parse_type_xml(&attr_xml.properties.attr_type)?;
    Ok(Attribute { name: attr_xml.properties.name, name_en: None, attr_type })
}

/// Parse TabularSection XML into TabularSection
fn parse_tabular_section(ts_xml: TabularSectionXml) -> Result<TabularSection> {
    let uuid = parse_uuid(&ts_xml.uuid, "tabular section")?;
    let mut tabular_section = TabularSection::new(uuid, ts_xml.properties.name);

    // Set synonym if present
    if let Some(synonym_xml) = ts_xml.properties.synonym {
        if let Some(synonym_value) = synonym_xml.value {
            tabular_section.set_synonym(Some(synonym_value));
        }
    }

    // Set use mode if present
    tabular_section.set_use_mode(ts_xml.properties.use_mode);

    // Parse attributes of the tabular section
    let Some(child_objects) = ts_xml.child_objects else {
        return Ok(tabular_section);
    };

    let mut ts_attributes = Vec::new();
    for attr_xml in child_objects.attributes {
        let _attr_span = tracing::debug_span!(
            "parse_ts_attribute",
            ts_name = %tabular_section.name(),
            attr_name = %attr_xml.properties.name
        )
        .entered();
        let attr_uuid = parse_uuid(&attr_xml._uuid, "tabular section attribute")?;
        let attr_type = parse_type_xml(&attr_xml.properties.attr_type)?;

        ts_attributes.push(TabularSectionAttribute::new(
            attr_uuid,
            attr_xml.properties.name,
            attr_type,
        ));
    }

    tabular_section.set_attributes(ts_attributes);
    Ok(tabular_section)
}
