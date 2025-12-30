//! XML parser for Designer format metadata
//!
//! Parses 1C:Enterprise metadata files in Designer format using quick-xml + serde.

use crate::common_module::CommonModule;
use crate::enums::ReturnValueReuse;
use crate::error::Result;
use crate::traits::MdObject;
use serde::Deserialize;
use uuid::Uuid;

/// Root XML structure for CommonModule
///
/// Designer format structure:
/// ```xml
/// <MetaDataObject xmlns="...">
///   <CommonModule uuid="...">
///     <Properties>
///       <Name>...</Name>
///       ...
///     </Properties>
///   </CommonModule>
/// </MetaDataObject>
/// ```
#[derive(Debug, Deserialize)]
struct MetaDataObject {
    #[serde(rename = "CommonModule")]
    common_module: CommonModuleXml,
}

/// CommonModule XML structure
#[derive(Debug, Deserialize)]
struct CommonModuleXml {
    /// UUID attribute
    #[serde(rename = "@uuid")]
    uuid: String,

    /// Properties block
    #[serde(rename = "Properties")]
    properties: CommonModuleProperties,
}

/// CommonModule Properties
#[derive(Debug, Deserialize)]
struct CommonModuleProperties {
    /// Module name
    #[serde(rename = "Name")]
    name: String,

    /// Server execution context
    #[serde(rename = "Server", default)]
    server: BoolValue,

    /// Global scope
    #[serde(rename = "Global", default)]
    global: BoolValue,

    /// Client managed application context
    #[serde(rename = "ClientManagedApplication", default)]
    client_managed_application: BoolValue,

    /// Client ordinary application context
    #[serde(rename = "ClientOrdinaryApplication", default)]
    client_ordinary_application: BoolValue,

    /// External connection context
    #[serde(rename = "ExternalConnection", default)]
    external_connection: BoolValue,

    /// Server call capability
    #[serde(rename = "ServerCall", default)]
    server_call: BoolValue,

    /// Privileged mode
    #[serde(rename = "Privileged", default)]
    privileged: BoolValue,

    /// Return value reuse mode
    #[serde(rename = "ReturnValuesReuse", default)]
    return_values_reuse: String,
}

/// Helper type for deserializing bool values from XML text content
///
/// XML format: `<Server>true</Server>` or `<Server>false</Server>`
#[derive(Debug, Deserialize, Default)]
struct BoolValue {
    #[serde(rename = "$text", default)]
    value: Option<String>,
}

impl From<BoolValue> for bool {
    fn from(val: BoolValue) -> Self {
        val.value.as_ref().map(|s| s.eq_ignore_ascii_case("true")).unwrap_or(false)
    }
}

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

    // Parse XML
    let metadata: MetaDataObject = quick_xml::de::from_str(xml)?;

    // Extract UUID
    let uuid =
        metadata.common_module.uuid.parse::<Uuid>().map_err(|e| {
            crate::error::MetadataError::InvalidFormat(format!("Invalid UUID: {}", e))
        })?;

    // Parse ReturnValuesReuse
    let return_values_reuse =
        ReturnValueReuse::from_name(&metadata.common_module.properties.return_values_reuse);

    // Build CommonModule
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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <CommonModule uuid="42869cb5-6361-4e4e-aee7-d4098cfe964d">
        <Properties>
            <Name>ГлобальныйСерверныйМодуль</Name>
            <Global>true</Global>
            <Server>true</Server>
            <ClientManagedApplication>false</ClientManagedApplication>
            <ClientOrdinaryApplication>false</ClientOrdinaryApplication>
            <ExternalConnection>false</ExternalConnection>
            <ServerCall>false</ServerCall>
            <Privileged>false</Privileged>
            <ReturnValuesReuse>DontUse</ReturnValuesReuse>
        </Properties>
    </CommonModule>
</MetaDataObject>"#;

    #[test]
    fn test_parse_common_module_xml() {
        let module = parse_common_module_xml(SAMPLE_XML).unwrap();

        assert_eq!(module.name(), "ГлобальныйСерверныйМодуль");
        assert_eq!(module.uuid().to_string(), "42869cb5-6361-4e4e-aee7-d4098cfe964d");
        assert!(module.is_server());
        assert!(module.is_global());
        assert!(!module.is_client_managed_application());
        assert!(!module.is_client_ordinary_application());
        assert!(!module.is_external_connection());
        assert!(!module.is_server_call());
        assert!(!module.is_privileged());
        assert_eq!(module.return_values_reuse(), ReturnValueReuse::DontUse);
    }

    #[test]
    fn test_parse_client_module() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <CommonModule uuid="4f304035-6a04-4455-9ce5-a5203bcb3081">
        <Properties>
            <Name>КлиентскийОбщийМодуль</Name>
            <Global>false</Global>
            <ClientManagedApplication>true</ClientManagedApplication>
            <Server>false</Server>
            <ExternalConnection>false</ExternalConnection>
            <ClientOrdinaryApplication>true</ClientOrdinaryApplication>
            <ServerCall>false</ServerCall>
            <Privileged>false</Privileged>
            <ReturnValuesReuse>DontUse</ReturnValuesReuse>
        </Properties>
    </CommonModule>
</MetaDataObject>"#;

        let module = parse_common_module_xml(xml).unwrap();

        assert_eq!(module.name(), "КлиентскийОбщийМодуль");
        assert!(!module.is_server());
        assert!(!module.is_global());
        assert!(module.is_client_managed_application());
        assert!(module.is_client_ordinary_application());
    }
}
