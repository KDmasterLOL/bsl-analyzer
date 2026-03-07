//! Helper functions for XML parsing

use crate::error::{MetadataError, Result};
use crate::metadata_object::{Attribute, MdoType, StandardAttributeKind};
use crate::register::RegisterAttribute;
use uuid::Uuid;

/// Parse UUID from string with unified error handling
pub(crate) fn parse_uuid(s: &str, context: &str) -> Result<Uuid> {
    s.parse::<Uuid>()
        .map_err(|e| MetadataError::InvalidFormat(format!("Invalid {} UUID: {}", context, e)))
}

/// Create Attribute from StandardAttributeKind
pub(crate) fn create_standard_attribute(
    kind: &StandardAttributeKind,
    mdo_type: MdoType,
    object_name: &str,
) -> Attribute {
    Attribute {
        name: kind.russian_name().to_string(),
        name_en: Some(kind.english_name().to_string()),
        attr_type: kind.attribute_type(mdo_type, object_name),
    }
}

/// Create RegisterAttribute from StandardAttributeKind
pub(crate) fn create_register_standard_attribute(
    kind: &StandardAttributeKind,
    mdo_type: MdoType,
    object_name: &str,
) -> RegisterAttribute {
    let nil_uuid = Uuid::nil();
    let mut attr = RegisterAttribute::new(nil_uuid, kind.russian_name());
    attr.set_attr_type(kind.attribute_type(mdo_type, object_name));
    attr
}
