//! CommonModule XML parser

use crate::common_module::CommonModule;
use crate::enums::ReturnValueReuse;
use crate::error::Result;
use crate::traits::MdObject;

use super::helpers::parse_uuid;
use super::serde_types::CommonModuleRoot;

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

    let metadata: CommonModuleRoot = quick_xml::de::from_str(xml)?;
    let uuid = parse_uuid(&metadata.common_module.uuid, "common module")?;

    let return_values_reuse =
        ReturnValueReuse::from_name(&metadata.common_module.properties.return_values_reuse);

    let module = CommonModule::builder()
        .uuid(uuid)
        .name(metadata.common_module.properties.name)
        .server(metadata.common_module.properties.server.into())
        .global(metadata.common_module.properties.global.into())
        .client_managed_application(
            metadata.common_module.properties.client_managed_application.into(),
        )
        .client_ordinary_application(
            metadata.common_module.properties.client_ordinary_application.into(),
        )
        .external_connection(metadata.common_module.properties.external_connection.into())
        .server_call(metadata.common_module.properties.server_call.into())
        .privileged(metadata.common_module.properties.privileged.into())
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
