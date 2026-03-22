//! DefinedType XML parser

use crate::defined_type::DefinedType;
use crate::error::{MetadataError, Result};

use super::helpers::{child_text, find_child, find_mdo_element, parse_uuid, parse_xml};
use super::type_parser::parse_type_xml;

/// Parse DefinedType XML from Designer format
pub fn parse_defined_type_xml(xml: &str) -> Result<DefinedType> {
    let doc = parse_xml(xml)?;
    let mdo = find_mdo_element(&doc)
        .ok_or_else(|| MetadataError::InvalidFormat("No DefinedType element found".to_string()))?;

    let uuid_str = mdo.attribute("uuid").unwrap_or("");
    let uuid = parse_uuid(uuid_str, "defined type")?;

    let props = find_child(mdo, "Properties").ok_or_else(|| {
        MetadataError::InvalidFormat("DefinedType missing Properties".to_string())
    })?;

    let name = child_text(props, "Name").unwrap_or("").to_string();

    let _span = tracing::debug_span!("parse_defined_type_xml", defined_type_name = %name).entered();

    let type_node = find_child(props, "Type").ok_or_else(|| {
        MetadataError::InvalidFormat("DefinedType missing Type element".to_string())
    })?;
    let underlying_type = parse_type_xml(type_node)?;

    let defined_type = DefinedType::builder()
        .uuid(uuid)
        .name(name.clone())
        .underlying_type(underlying_type.clone())
        .build();

    tracing::debug!(
        defined_type_name = %name,
        uuid = %uuid,
        underlying_type = ?underlying_type,
        "parsed defined type"
    );

    Ok(defined_type)
}
