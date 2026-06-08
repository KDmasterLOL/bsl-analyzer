use crate::enums::CodeSeries;
use crate::error::{MetadataError, Result};
use crate::metadata_object::{Attribute, MdoType, MetadataObject};
use crate::tabular_section::{TabularSection, TabularSectionAttribute};
use std::str::FromStr;

use super::helpers::{child_text, find_child, find_mdo_element, parse_uuid, parse_xml};
use super::standard_attributes::{
    add_business_process_standard_attributes, add_catalog_standard_attributes,
    add_chart_of_accounts_standard_attributes,
    add_chart_of_characteristic_types_standard_attributes, add_document_standard_attributes,
    add_exchange_plan_standard_attributes, add_information_register_standard_attributes_as_attrs,
    add_task_standard_attributes, MdoProperties,
};
use super::type_parser::parse_type_xml;

pub fn parse_catalog_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_catalog_xml").entered();
    parse_metadata_object_xml(xml, MdoType::Catalog)
}

pub fn parse_document_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_document_xml").entered();
    parse_metadata_object_xml(xml, MdoType::Document)
}

pub fn parse_business_process_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_business_process_xml").entered();
    parse_metadata_object_xml(xml, MdoType::BusinessProcess)
}

pub fn parse_chart_of_characteristic_types_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_chart_of_characteristic_types_xml").entered();
    parse_metadata_object_xml(xml, MdoType::ChartOfCharacteristicTypes)
}

pub fn parse_task_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_task_xml").entered();
    parse_metadata_object_xml(xml, MdoType::Task)
}

pub fn parse_exchange_plan_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_exchange_plan_xml").entered();
    parse_metadata_object_xml(xml, MdoType::ExchangePlan)
}

pub fn parse_chart_of_accounts_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_chart_of_accounts_xml").entered();
    parse_metadata_object_xml(xml, MdoType::ChartOfAccounts)
}

pub fn parse_data_processor_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_data_processor_xml").entered();
    parse_metadata_object_xml(xml, MdoType::DataProcessor)
}

pub fn parse_report_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_report_xml").entered();
    parse_metadata_object_xml(xml, MdoType::Report)
}

fn parse_metadata_object_xml(xml: &str, mdo_type: MdoType) -> Result<MetadataObject> {
    let doc = parse_xml(xml)?;

    let mdo_node = find_mdo_element(&doc)
        .ok_or_else(|| MetadataError::InvalidFormat("No MDO element found".to_string()))?;

    let props_node = find_child(mdo_node, "Properties")
        .ok_or_else(|| MetadataError::InvalidFormat("No Properties element found".to_string()))?;

    let properties = MdoProperties::from_node(props_node);

    let mut attributes = Vec::new();
    let mut tabular_sections = Vec::new();

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

    if let Some(child_objects) = find_child(mdo_node, "ChildObjects") {
        for child in child_objects.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                // `AddressingAttribute` is a Task's addressing requisite
                // (Исполнитель, РольИсполнителя, …) — same `Properties>Name>Type`
                // shape as a regular attribute and a real member of the task
                // object, so resolve it like one.
                "Attribute" | "Resource" | "Dimension" | "AddressingAttribute" => {
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
    if let Some(uuid_str) = mdo_node.attribute("uuid") {
        match parse_uuid(uuid_str, "MDO root") {
            Ok(uuid) => mdo.set_uuid(uuid),
            Err(err) => tracing::warn!(
                ?mdo_type,
                name = %mdo.name,
                uuid_raw = %uuid_str,
                %err,
                "ignored malformed MDO root UUID"
            ),
        }
    }
    if mdo_type == MdoType::Document {
        mdo.set_register_records(parse_register_records(props_node));
    }
    for attr in attributes {
        mdo.add_attribute(attr);
    }
    for ts in tabular_sections {
        mdo.add_tabular_section(ts);
    }

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

fn parse_register_records(props_node: roxmltree::Node<'_, '_>) -> Vec<(MdoType, String)> {
    let Some(records_node) = find_child(props_node, "RegisterRecords") else {
        return Vec::new();
    };

    records_node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Item")
        .filter_map(|item| {
            let raw = item.text()?.trim();
            let (prefix, name) = raw.split_once('.')?;
            if name.is_empty() {
                return None;
            }
            let mdo_type = MdoType::from_str(prefix).ok()?;
            match mdo_type {
                MdoType::InformationRegister
                | MdoType::AccumulationRegister
                | MdoType::AccountingRegister
                | MdoType::CalculationRegister => Some((mdo_type, name.to_string())),
                _ => None,
            }
        })
        .collect()
}

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

fn parse_tabular_section_node(node: roxmltree::Node<'_, '_>) -> Result<TabularSection> {
    let uuid_str = node.attribute("uuid").unwrap_or("");
    let uuid = parse_uuid(uuid_str, "tabular section")?;

    let props = find_child(node, "Properties").ok_or_else(|| {
        MetadataError::InvalidFormat("TabularSection missing Properties".to_string())
    })?;

    let name = child_text(props, "Name").unwrap_or("").to_string();
    let mut tabular_section = TabularSection::new(uuid, name);

    if let Some(synonym_node) = find_child(props, "Synonym") {
        tabular_section.set_synonym(synonym_node.text().map(|s| s.to_string()));
    }

    let use_mode = child_text(props, "Use").map(|s| s.to_string());
    tabular_section.set_use_mode(use_mode);

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

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.20">
    <Catalog uuid="d11b89e1-90a2-47e7-b43f-7f231ec64b2f">
        <Properties>
            <Name>Валюты</Name>
        </Properties>
    </Catalog>
</MetaDataObject>"#;

    #[test]
    fn parse_catalog_xml_reads_root_uuid() {
        let mdo = parse_catalog_xml(CATALOG_XML).unwrap();
        assert_eq!(mdo.name, "Валюты");
        assert_eq!(mdo.mdo_type, MdoType::Catalog);
        assert_eq!(
            mdo.uuid().map(|u| u.to_string()),
            Some("d11b89e1-90a2-47e7-b43f-7f231ec64b2f".to_string())
        );
    }

    #[test]
    fn parse_document_xml_reads_root_uuid() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.20">
    <Document uuid="9de71b46-e9bf-4b0b-8f3c-4abcd6a385dd">
        <Properties><Name>АвансовыйОтчет</Name></Properties>
    </Document>
</MetaDataObject>"#;
        let mdo = parse_document_xml(xml).unwrap();
        assert_eq!(
            mdo.uuid().map(|u| u.to_string()),
            Some("9de71b46-e9bf-4b0b-8f3c-4abcd6a385dd".to_string())
        );
    }

    #[test]
    fn parse_task_xml_reads_addressing_attributes_as_attributes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" version="2.20">
    <Task uuid="0408002c-9255-43a2-8022-82f02ee0e8f4">
        <Properties><Name>ЗадачаИсполнителя</Name></Properties>
        <ChildObjects>
            <AddressingAttribute uuid="11111111-1111-1111-1111-111111111111">
                <Properties>
                    <Name>Исполнитель</Name>
                    <Type><v8:Type>cfg:CatalogRef.Пользователи</v8:Type></Type>
                </Properties>
            </AddressingAttribute>
            <Attribute uuid="22222222-2222-2222-2222-222222222222">
                <Properties>
                    <Name>Описание</Name>
                    <Type><v8:Type>xs:string</v8:Type></Type>
                </Properties>
            </Attribute>
        </ChildObjects>
    </Task>
</MetaDataObject>"#;
        let mdo = parse_task_xml(xml).unwrap();
        assert_eq!(mdo.mdo_type, MdoType::Task);
        let names: Vec<&str> = mdo.attributes.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"Исполнитель"), "addressing attribute must resolve: {names:?}");
        assert!(names.contains(&"Описание"), "regular attribute still present: {names:?}");
    }

    #[test]
    fn parse_xml_without_uuid_attribute_degrades_to_none() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.20">
    <Catalog>
        <Properties><Name>NoUuid</Name></Properties>
    </Catalog>
</MetaDataObject>"#;
        let mdo = parse_catalog_xml(xml).unwrap();
        assert_eq!(mdo.uuid(), None);
    }

    #[test]
    fn malformed_uuid_attribute_degrades_to_none() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.20">
    <Catalog uuid="not-a-uuid">
        <Properties><Name>BadUuid</Name></Properties>
    </Catalog>
</MetaDataObject>"#;
        let mdo = parse_catalog_xml(xml).unwrap();
        assert_eq!(mdo.uuid(), None);
    }
}
