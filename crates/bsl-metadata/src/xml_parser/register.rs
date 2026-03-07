//! Register XML parser (InformationRegister, AccumulationRegister, etc.)

use crate::dimension::Dimension;
use crate::error::Result;
use crate::metadata_object::MdoType;
use crate::register::{
    AccumulationRegisterType, Register, RegisterAttribute, RegisterPeriodicity, RegisterResource,
};

use super::helpers::parse_uuid;
use super::serde_types::{RegisterChildObjects, RegisterRoot};
use super::standard_attributes::{
    add_accumulation_register_standard_attrs, add_information_register_standard_attrs,
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

    let root: RegisterRoot = quick_xml::de::from_str(xml)?;
    let uuid = parse_uuid(&root.register.uuid, "register")?;
    let object_name = &root.register.properties.name;

    // Add standard attributes first
    let mut attributes = Vec::new();
    add_standard_attrs_for_type(&mut attributes, object_name, mdo_type, &root);

    // Parse child objects (dimensions, resources, custom attributes)
    let (dimensions, resources) = if let Some(ref children) = root.register.child_objects {
        (parse_dimensions(children)?, parse_resources(children)?)
    } else {
        (Vec::new(), Vec::new())
    };

    // Parse custom attributes
    if let Some(ref children) = root.register.child_objects {
        parse_custom_attributes(children, &mut attributes)?;
    }

    // Parse periodicity and register type
    let periodicity = parse_periodicity(&root, mdo_type);
    let register_type = parse_register_type(&root, mdo_type);

    let register = Register::builder()
        .uuid(uuid)
        .name(root.register.properties.name)
        .mdo_type(mdo_type)
        .dimensions(dimensions)
        .resources(resources)
        .attributes(attributes)
        .periodicity(periodicity)
        .register_type(register_type)
        .enable_totals_slice_first(root.register.properties.enable_totals_slice_first.into())
        .enable_totals_slice_last(root.register.properties.enable_totals_slice_last.into())
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

/// Add standard attributes based on register type
fn add_standard_attrs_for_type(
    attributes: &mut Vec<RegisterAttribute>,
    object_name: &str,
    mdo_type: MdoType,
    root: &RegisterRoot,
) {
    match mdo_type {
        MdoType::InformationRegister => {
            let periodicity = root.register.properties.periodicity.as_deref();
            add_information_register_standard_attrs(attributes, object_name, periodicity);
        }
        MdoType::AccumulationRegister => {
            add_accumulation_register_standard_attrs(attributes, object_name);
        }
        _ => {
            // AccountingRegister and CalculationRegister - TODO: add standard attributes
        }
    }
}

/// Parse dimensions from child objects
fn parse_dimensions(children: &RegisterChildObjects) -> Result<Vec<Dimension>> {
    let mut dimensions = Vec::new();

    for dim_xml in &children.dimensions {
        let _span = tracing::debug_span!(
            "parse_dimension",
            dim_name = %dim_xml.properties.name
        )
        .entered();
        let dim_uuid = parse_uuid(&dim_xml.uuid, "dimension")?;

        let mut dimension = Dimension::builder()
            .uuid(dim_uuid)
            .name(dim_xml.properties.name.clone())
            .deny_incomplete_values(dim_xml.properties.deny_incomplete_values.clone().into())
            .master(dim_xml.properties.master.clone().into())
            .indexing(dim_xml.properties.indexing.clone())
            .build();

        if let Some(ref dim_type_xml) = dim_xml.properties.dim_type {
            let dim_type = parse_type_xml(dim_type_xml)?;
            dimension.set_type_str(format!("{}", dim_type));
            dimension.set_attr_type(dim_type);
        }

        dimensions.push(dimension);
    }

    Ok(dimensions)
}

/// Parse resources from child objects
fn parse_resources(children: &RegisterChildObjects) -> Result<Vec<RegisterResource>> {
    let mut resources = Vec::new();

    for resource_xml in &children.resources {
        let _span = tracing::debug_span!(
            "parse_resource",
            resource_name = %resource_xml.properties.name
        )
        .entered();
        let resource_uuid = parse_uuid(&resource_xml.uuid, "resource")?;

        let mut resource =
            RegisterResource::new(resource_uuid, resource_xml.properties.name.clone());
        let resource_type = parse_type_xml(&resource_xml.properties.resource_type)?;
        resource.set_type_str(format!("{}", resource_type));
        resource.set_attr_type(resource_type);

        resources.push(resource);
    }

    Ok(resources)
}

/// Parse custom attributes from child objects
fn parse_custom_attributes(
    children: &RegisterChildObjects,
    attributes: &mut Vec<RegisterAttribute>,
) -> Result<()> {
    for attr_xml in &children.attributes {
        let _span = tracing::debug_span!(
            "parse_register_attr",
            attr_name = %attr_xml.properties.name
        )
        .entered();
        let attr_uuid = parse_uuid(&attr_xml.uuid, "attribute")?;

        let mut attribute = RegisterAttribute::new(attr_uuid, attr_xml.properties.name.clone());
        let attr_type = parse_type_xml(&attr_xml.properties.attr_type)?;
        attribute.set_type_str(format!("{}", attr_type));
        attribute.set_attr_type(attr_type);

        attributes.push(attribute);
    }

    Ok(())
}

/// Parse periodicity for InformationRegister
fn parse_periodicity(root: &RegisterRoot, mdo_type: MdoType) -> Option<RegisterPeriodicity> {
    if mdo_type != MdoType::InformationRegister {
        return None;
    }

    root.register.properties.periodicity.as_ref().and_then(|p| match p.as_str() {
        "Nonperiodical" => Some(RegisterPeriodicity::Nonperiodical),
        "Second" => Some(RegisterPeriodicity::Second),
        "Day" => Some(RegisterPeriodicity::Day),
        "Month" => Some(RegisterPeriodicity::Month),
        "RecorderPosition" => Some(RegisterPeriodicity::RecorderPosition),
        _ => None,
    })
}

/// Parse register type for AccumulationRegister
fn parse_register_type(root: &RegisterRoot, mdo_type: MdoType) -> Option<AccumulationRegisterType> {
    if mdo_type != MdoType::AccumulationRegister {
        return None;
    }

    root.register.properties.register_type.as_ref().and_then(|rt| match rt.as_str() {
        "Balance" => Some(AccumulationRegisterType::Balance),
        "Turnovers" => Some(AccumulationRegisterType::Turnovers),
        "BalanceAndTurnovers" => Some(AccumulationRegisterType::BalanceAndTurnovers),
        _ => None,
    })
}
