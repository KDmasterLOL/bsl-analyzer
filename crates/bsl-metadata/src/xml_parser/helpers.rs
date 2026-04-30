//! Helper functions for XML parsing

use crate::error::{MetadataError, Result};
use crate::metadata_object::{MdoType, StandardAttributeKind};
use crate::register::RegisterAttribute;
use uuid::Uuid;

/// Parse UUID from string with unified error handling
pub(crate) fn parse_uuid(s: &str, context: &str) -> Result<Uuid> {
    s.parse::<Uuid>()
        .map_err(|e| MetadataError::InvalidFormat(format!("Invalid {} UUID: {}", context, e)))
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

// ============================================================================
// roxmltree DOM helpers
// ============================================================================

/// Find first child element with given tag name
pub(crate) fn find_child<'a>(
    node: roxmltree::Node<'a, 'a>,
    tag: &str,
) -> Option<roxmltree::Node<'a, 'a>> {
    node.children().find(|n| n.is_element() && n.tag_name().name() == tag)
}

/// Get text content of first child element with given tag name
pub(crate) fn child_text<'a>(node: roxmltree::Node<'a, 'a>, tag: &str) -> Option<&'a str> {
    find_child(node, tag).and_then(|n| n.text())
}

/// Get bool from child element text (true/false)
pub(crate) fn child_bool(node: roxmltree::Node<'_, '_>, tag: &str) -> bool {
    child_text(node, tag).is_some_and(|s| s.eq_ignore_ascii_case("true"))
}

/// Get optional u32 from child element text
pub(crate) fn child_u32(node: roxmltree::Node<'_, '_>, tag: &str) -> Option<u32> {
    child_text(node, tag).and_then(|s| s.parse().ok())
}

/// Parse MetaDataObject root: find the first child element of root
/// (e.g. `<Catalog>`, `<Document>`, `<CommonModule>`)
pub(crate) fn find_mdo_element<'a>(
    doc: &'a roxmltree::Document<'a>,
) -> Option<roxmltree::Node<'a, 'a>> {
    doc.root_element().children().find(|n| n.is_element())
}

/// Parse XML document from string
pub(crate) fn parse_xml(xml: &str) -> Result<roxmltree::Document<'_>> {
    roxmltree::Document::parse(xml)
        .map_err(|e| MetadataError::InvalidFormat(format!("XML parse error: {}", e)))
}
