//! Constant XML parser

use crate::error::Result;
use crate::metadata_object::{MdoType, MetadataObject};

use super::serde_types::ConstantRoot;

/// Parse Constant XML from Designer format
pub fn parse_constant_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_constant_xml").entered();

    let root: ConstantRoot = quick_xml::de::from_str(xml)?;
    let mdo = MetadataObject::new(MdoType::Constant, root.constant.properties.name);

    Ok(mdo)
}
