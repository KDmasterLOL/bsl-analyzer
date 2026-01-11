//! DefinedType XML parser

use crate::defined_type::DefinedType;
use crate::error::Result;

use super::helpers::parse_uuid;
use super::serde_types::DefinedTypeRoot;
use super::type_parser::parse_type_xml;

/// Parse DefinedType XML from Designer format
pub fn parse_defined_type_xml(xml: &str) -> Result<DefinedType> {
    let _span = tracing::debug_span!("parse_defined_type_xml").entered();

    let root: DefinedTypeRoot = quick_xml::de::from_str(xml)?;
    let uuid = parse_uuid(&root.defined_type.uuid, "defined type")?;

    let name = root.defined_type.properties.name;
    let underlying_type = parse_type_xml(&root.defined_type.properties.defined_type)?;

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
