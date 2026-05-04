//! Register XML parser (InformationRegister, AccumulationRegister, etc.)

use crate::dimension::Dimension;
use crate::error::{MetadataError, Result};
use crate::metadata_object::MdoType;
use crate::register::{
    AccumulationRegisterType, Register, RegisterAttribute, RegisterPeriodicity, RegisterResource,
};

use super::helpers::{child_bool, child_text, find_child, find_mdo_element, parse_uuid, parse_xml};
use super::standard_attributes::{
    add_accounting_register_standard_attrs, add_accumulation_register_standard_attrs,
    add_calculation_register_standard_attrs, add_information_register_standard_attrs,
};
use super::type_parser::parse_type_xml;

/// Parse InformationRegister XML from Designer format
pub fn parse_information_register_xml(xml: &str) -> Result<Register> {
    parse_register_xml(xml, MdoType::InformationRegister)
}

/// Parse AccumulationRegister XML from Designer format
pub fn parse_accumulation_register_xml(xml: &str) -> Result<Register> {
    parse_register_xml(xml, MdoType::AccumulationRegister)
}

/// Parse AccountingRegister XML from Designer format
pub fn parse_accounting_register_xml(xml: &str) -> Result<Register> {
    parse_register_xml(xml, MdoType::AccountingRegister)
}

/// Parse CalculationRegister XML from Designer format
pub fn parse_calculation_register_xml(xml: &str) -> Result<Register> {
    parse_register_xml(xml, MdoType::CalculationRegister)
}

/// Internal helper to parse register XML with specific type
fn parse_register_xml(xml: &str, mdo_type: MdoType) -> Result<Register> {
    let _span = tracing::debug_span!("parse_register_xml", ?mdo_type).entered();

    let doc = parse_xml(xml)?;

    // find_mdo_element gets the first child of root regardless of tag name —
    // handles InformationRegister/AccumulationRegister/AccountingRegister/CalculationRegister
    let mdo = find_mdo_element(&doc)
        .ok_or_else(|| MetadataError::InvalidFormat("No register element found".to_string()))?;

    let uuid_str = mdo.attribute("uuid").unwrap_or("");
    let uuid = parse_uuid(uuid_str, "register")?;

    let props = find_child(mdo, "Properties")
        .ok_or_else(|| MetadataError::InvalidFormat("Register missing Properties".to_string()))?;

    let object_name = child_text(props, "Name").unwrap_or("").to_string();
    let periodicity_str =
        child_text(props, "InformationRegisterPeriodicity").map(|s| s.to_string());
    let enable_totals_slice_first = child_bool(props, "EnableTotalsSliceFirst");
    let enable_totals_slice_last = child_bool(props, "EnableTotalsSliceLast");
    let register_type_str = child_text(props, "RegisterType").map(|s| s.to_string());

    // Add standard attributes first
    let mut attributes = Vec::new();
    match mdo_type {
        MdoType::InformationRegister => {
            add_information_register_standard_attrs(
                &mut attributes,
                &object_name,
                periodicity_str.as_deref(),
            );
        }
        MdoType::AccumulationRegister => {
            add_accumulation_register_standard_attrs(&mut attributes, &object_name);
        }
        MdoType::AccountingRegister => {
            add_accounting_register_standard_attrs(&mut attributes, &object_name);
        }
        MdoType::CalculationRegister => {
            add_calculation_register_standard_attrs(&mut attributes, &object_name);
        }
        _ => {}
    }

    let mut dimensions = Vec::new();
    let mut resources = Vec::new();

    // Parse child objects
    if let Some(child_objects) = find_child(mdo, "ChildObjects") {
        for child in child_objects.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "Dimension" => {
                    dimensions.push(parse_dimension_node(child)?);
                }
                "Resource" => {
                    resources.push(parse_resource_node(child)?);
                }
                "Attribute" => {
                    parse_register_attr_node(child, &mut attributes)?;
                }
                _ => {}
            }
        }
    }

    let periodicity = parse_periodicity(periodicity_str.as_deref(), mdo_type);
    let register_type = parse_register_type(register_type_str.as_deref(), mdo_type);

    let register = Register::builder()
        .uuid(uuid)
        .name(object_name)
        .mdo_type(mdo_type)
        .dimensions(dimensions)
        .resources(resources)
        .attributes(attributes)
        .periodicity(periodicity)
        .register_type(register_type)
        .enable_totals_slice_first(enable_totals_slice_first)
        .enable_totals_slice_last(enable_totals_slice_last)
        .build();

    tracing::debug!(
        register_name = %register.name(),
        uuid = %register.uuid(),
        mdo_type = ?register.mdo_type(),
        dimensions = register.dimensions().len(),
        resources = register.resources().len(),
        attributes = register.attributes().len(),
        "parsed register"
    );

    Ok(register)
}

/// Parse a `<Dimension>` node
fn parse_dimension_node(node: roxmltree::Node<'_, '_>) -> Result<Dimension> {
    let uuid_str = node.attribute("uuid").unwrap_or("");
    let dim_uuid = parse_uuid(uuid_str, "dimension")?;

    let props = find_child(node, "Properties")
        .ok_or_else(|| MetadataError::InvalidFormat("Dimension missing Properties".to_string()))?;

    let name = child_text(props, "Name").unwrap_or("").to_string();

    let _span = tracing::debug_span!("parse_dimension", dim_name = %name).entered();

    let deny_incomplete_values = child_bool(props, "DenyIncompleteValues");
    let master = child_bool(props, "Master");
    let indexing = child_text(props, "Indexing").unwrap_or("").to_string();

    let mut dimension = Dimension::builder()
        .uuid(dim_uuid)
        .name(name)
        .deny_incomplete_values(deny_incomplete_values)
        .master(master)
        .indexing(indexing)
        .build();

    if let Some(type_node) = find_child(props, "Type") {
        let dim_type = parse_type_xml(type_node)?;
        dimension.set_type_str(format!("{}", dim_type));
        dimension.set_attr_type(dim_type);
    }

    Ok(dimension)
}

/// Parse a `<Resource>` node
fn parse_resource_node(node: roxmltree::Node<'_, '_>) -> Result<RegisterResource> {
    let uuid_str = node.attribute("uuid").unwrap_or("");
    let resource_uuid = parse_uuid(uuid_str, "resource")?;

    let props = find_child(node, "Properties")
        .ok_or_else(|| MetadataError::InvalidFormat("Resource missing Properties".to_string()))?;

    let name = child_text(props, "Name").unwrap_or("").to_string();

    let _span = tracing::debug_span!("parse_resource", resource_name = %name).entered();

    let mut resource = RegisterResource::new(resource_uuid, name);

    let type_node = find_child(props, "Type")
        .ok_or_else(|| MetadataError::InvalidFormat("Resource missing Type".to_string()))?;
    let resource_type = parse_type_xml(type_node)?;
    resource.set_type_str(format!("{}", resource_type));
    resource.set_attr_type(resource_type);

    Ok(resource)
}

/// Parse a `<Attribute>` node into a RegisterAttribute
fn parse_register_attr_node(
    node: roxmltree::Node<'_, '_>,
    attributes: &mut Vec<RegisterAttribute>,
) -> Result<()> {
    let uuid_str = node.attribute("uuid").unwrap_or("");
    let attr_uuid = parse_uuid(uuid_str, "attribute")?;

    let props = find_child(node, "Properties").ok_or_else(|| {
        MetadataError::InvalidFormat("RegisterAttribute missing Properties".to_string())
    })?;

    let name = child_text(props, "Name").unwrap_or("").to_string();

    let _span = tracing::debug_span!("parse_register_attr", attr_name = %name).entered();

    let mut attribute = RegisterAttribute::new(attr_uuid, name);

    let type_node = find_child(props, "Type").ok_or_else(|| {
        MetadataError::InvalidFormat("RegisterAttribute missing Type".to_string())
    })?;
    let attr_type = parse_type_xml(type_node)?;
    attribute.set_type_str(format!("{}", attr_type));
    attribute.set_attr_type(attr_type);

    attributes.push(attribute);
    Ok(())
}

/// Parse periodicity for InformationRegister
fn parse_periodicity(periodicity: Option<&str>, mdo_type: MdoType) -> Option<RegisterPeriodicity> {
    if mdo_type != MdoType::InformationRegister {
        return None;
    }

    periodicity.and_then(|p| match p {
        "Nonperiodical" => Some(RegisterPeriodicity::Nonperiodical),
        "Second" => Some(RegisterPeriodicity::Second),
        "Day" => Some(RegisterPeriodicity::Day),
        "Month" => Some(RegisterPeriodicity::Month),
        "RecorderPosition" => Some(RegisterPeriodicity::RecorderPosition),
        _ => None,
    })
}

/// Parse register type for AccumulationRegister
fn parse_register_type(
    register_type: Option<&str>,
    mdo_type: MdoType,
) -> Option<AccumulationRegisterType> {
    if mdo_type != MdoType::AccumulationRegister {
        return None;
    }

    register_type.and_then(|rt| match rt {
        "Balance" => Some(AccumulationRegisterType::Balance),
        "Turnovers" => Some(AccumulationRegisterType::Turnovers),
        "BalanceAndTurnovers" => Some(AccumulationRegisterType::BalanceAndTurnovers),
        _ => None,
    })
}
