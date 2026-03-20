//! CommonModule XML parser

use crate::common_module::CommonModule;
use crate::enums::ReturnValueReuse;
use crate::error::{MetadataError, Result};
use crate::traits::MdObject;

use super::helpers::{child_bool, child_text, find_child, find_mdo_element, parse_uuid, parse_xml};

/// Parse CommonModule XML from Designer format
///
/// # Arguments
///
/// * `xml` - XML content as string
///
/// # Returns
///
/// Parsed `CommonModule` structure
///
/// # Example
///
/// ```no_run
/// # use bsl_metadata::xml_parser::parse_common_module_xml;
/// let xml = std::fs::read_to_string("CommonModules/MyModule/MyModule.xml")?;
/// let module = parse_common_module_xml(&xml)?;
/// # Ok::<(), bsl_metadata::MetadataError>(())
/// ```
pub fn parse_common_module_xml(xml: &str) -> Result<CommonModule> {
    let _span = tracing::debug_span!("parse_common_module_xml").entered();

    let doc = parse_xml(xml)?;
    let mdo = find_mdo_element(&doc)
        .ok_or_else(|| MetadataError::InvalidFormat("No CommonModule element found".to_string()))?;

    let uuid_str = mdo.attribute("uuid").unwrap_or("");
    let uuid = parse_uuid(uuid_str, "common module")?;

    let props = find_child(mdo, "Properties").ok_or_else(|| {
        MetadataError::InvalidFormat("CommonModule missing Properties".to_string())
    })?;

    let name = child_text(props, "Name").unwrap_or("").to_string();
    let return_values_reuse_str = child_text(props, "ReturnValuesReuse").unwrap_or("");
    let return_values_reuse = ReturnValueReuse::from_name(return_values_reuse_str);

    let module = CommonModule::builder()
        .uuid(uuid)
        .name(name)
        .server(child_bool(props, "Server"))
        .global(child_bool(props, "Global"))
        .client_managed_application(child_bool(props, "ClientManagedApplication"))
        .client_ordinary_application(child_bool(props, "ClientOrdinaryApplication"))
        .external_connection(child_bool(props, "ExternalConnection"))
        .server_call(child_bool(props, "ServerCall"))
        .privileged(child_bool(props, "Privileged"))
        .return_values_reuse(return_values_reuse)
        .build();

    tracing::debug!(
        module_name = %module.name(),
        uuid = %module.uuid(),
        server = module.is_server(),
        global = module.is_global(),
        "parsed common module"
    );

    Ok(module)
}
