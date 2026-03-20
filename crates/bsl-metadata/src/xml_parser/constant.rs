//! Constant XML parser

use crate::error::{MetadataError, Result};
use crate::metadata_object::{MdoType, MetadataObject};

use super::helpers::{child_text, find_child, find_mdo_element, parse_xml};

/// Parse Constant XML from Designer format
pub fn parse_constant_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_constant_xml").entered();

    let doc = parse_xml(xml)?;
    let mdo = find_mdo_element(&doc)
        .ok_or_else(|| MetadataError::InvalidFormat("No Constant element found".to_string()))?;

    let props = find_child(mdo, "Properties")
        .ok_or_else(|| MetadataError::InvalidFormat("Constant missing Properties".to_string()))?;

    let name = child_text(props, "Name").unwrap_or("").to_string();
    let mdo_obj = MetadataObject::new(MdoType::Constant, name);

    Ok(mdo_obj)
}
