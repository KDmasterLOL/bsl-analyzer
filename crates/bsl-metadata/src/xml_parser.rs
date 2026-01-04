//! XML parser for Designer format metadata
//!
//! Parses 1C:Enterprise metadata files in Designer format using quick-xml + serde.

use crate::common_module::CommonModule;
use crate::dimension::Dimension;
use crate::enums::ReturnValueReuse;
use crate::error::Result;
use crate::event_subscription::EventSubscription;
use crate::metadata_object::MdoType;
use crate::register::Register;
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

/// Root XML structure for Register (all 4 types)
///
/// Designer format structure:
/// ```xml
/// <MetaDataObject xmlns="...">
///   <InformationRegister uuid="...">  <!-- or AccumulationRegister, AccountingRegister, CalculationRegister -->
///     <Properties>
///       <Name>...</Name>
///     </Properties>
///     <ChildObjects>
///       <Dimension uuid="...">
///         <Properties>...</Properties>
///       </Dimension>
///     </ChildObjects>
///   </InformationRegister>
/// </MetaDataObject>
/// ```
#[derive(Debug, Deserialize)]
struct RegisterRoot {
    #[serde(
        alias = "InformationRegister",
        alias = "AccumulationRegister",
        alias = "AccountingRegister",
        alias = "CalculationRegister"
    )]
    register: RegisterXml,
}

/// Register XML structure (generic for all 4 types)
#[derive(Debug, Deserialize)]
struct RegisterXml {
    #[serde(rename = "@uuid")]
    uuid: String,

    #[serde(rename = "Properties")]
    properties: RegisterProperties,

    #[serde(rename = "ChildObjects", default)]
    child_objects: Option<ChildObjects>,
}

/// Register Properties
#[derive(Debug, Deserialize)]
struct RegisterProperties {
    #[serde(rename = "Name")]
    name: String,
}

/// ChildObjects container for dimensions
#[derive(Debug, Deserialize)]
struct ChildObjects {
    #[serde(rename = "Dimension", default)]
    dimensions: Vec<DimensionXml>,
}

/// Dimension XML structure
#[derive(Debug, Deserialize)]
struct DimensionXml {
    #[serde(rename = "@uuid")]
    uuid: String,

    #[serde(rename = "Properties")]
    properties: DimensionProperties,
}

/// Dimension Properties
#[derive(Debug, Deserialize)]
struct DimensionProperties {
    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "DenyIncompleteValues", default)]
    deny_incomplete_values: BoolValue,

    #[serde(rename = "Master", default)]
    master: BoolValue,

    #[serde(rename = "Indexing", default)]
    indexing: String,
}

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

    let uuid =
        root.register.uuid.parse::<Uuid>().map_err(|e| {
            crate::error::MetadataError::InvalidFormat(format!("Invalid UUID: {}", e))
        })?;

    let mut dimensions = Vec::new();
    if let Some(child_objects) = root.register.child_objects {
        for dim_xml in child_objects.dimensions {
            let dim_uuid = dim_xml.uuid.parse::<Uuid>().map_err(|e| {
                crate::error::MetadataError::InvalidFormat(format!("Invalid dimension UUID: {}", e))
            })?;

            let dimension = Dimension::builder()
                .uuid(dim_uuid)
                .name(dim_xml.properties.name)
                .deny_incomplete_values(dim_xml.properties.deny_incomplete_values.into())
                .master(dim_xml.properties.master.into())
                .indexing(dim_xml.properties.indexing)
                .build();

            dimensions.push(dimension);
        }
    }

    let register = Register::builder()
        .uuid(uuid)
        .name(root.register.properties.name)
        .mdo_type(mdo_type)
        .dimensions(dimensions)
        .build();

    tracing::debug!(
        register_name = %register.name(),
        uuid = %register.uuid(),
        mdo_type = ?register.mdo_type(),
        dimensions = register.dimensions().len(),
        "parsed register"
    );

    Ok(register)
}

/// Root XML structure for EventSubscription
///
/// Designer format structure:
/// ```xml
/// <MetaDataObject xmlns="...">
///   <EventSubscription uuid="...">
///     <Properties>
///       <Name>...</Name>
///       <Source><v8:Type>...</v8:Type></Source>  <!-- or v8:TypeSet -->
///       <Event>OnWrite</Event>
///       <Handler>CommonModule.Module.Method</Handler>
///     </Properties>
///   </EventSubscription>
/// </MetaDataObject>
/// ```
#[derive(Debug, Deserialize)]
struct EventSubscriptionRoot {
    #[serde(rename = "EventSubscription")]
    event_subscription: EventSubscriptionXml,
}

/// EventSubscription XML structure
#[derive(Debug, Deserialize)]
struct EventSubscriptionXml {
    /// UUID attribute
    #[serde(rename = "@uuid")]
    uuid: String,

    /// Properties block
    #[serde(rename = "Properties")]
    properties: EventSubscriptionProperties,
}

/// EventSubscription Properties
#[derive(Debug, Deserialize)]
struct EventSubscriptionProperties {
    /// Subscription name
    #[serde(rename = "Name")]
    name: String,

    /// Comment (optional)
    #[serde(rename = "Comment", default)]
    comment: Option<String>,

    /// Source - can contain either v8:Type or v8:TypeSet
    #[serde(rename = "Source")]
    source: EventSource,

    /// Event type (e.g., "OnWrite", "BeforeWrite")
    #[serde(rename = "Event")]
    event: String,

    /// Handler path (can be empty)
    #[serde(rename = "Handler", default)]
    handler: String,
}

/// Event source - handles both v8:Type and v8:TypeSet variants
/// Can contain multiple Type or TypeSet elements
#[derive(Debug, Deserialize)]
struct EventSource {
    /// Type elements (v8:Type)
    #[serde(rename = "Type", default)]
    types: Vec<String>,

    /// TypeSet elements (v8:TypeSet)
    #[serde(rename = "TypeSet", default)]
    type_sets: Vec<String>,
}

impl EventSource {
    fn as_string(&self) -> String {
        // Combine all types into a single string, separated by semicolons
        let mut all_types: Vec<&str> = Vec::new();
        all_types.extend(self.types.iter().map(|s| s.as_str()));
        all_types.extend(self.type_sets.iter().map(|s| s.as_str()));
        all_types.join(";")
    }
}

/// Parse EventSubscription XML from Designer format
///
/// # Arguments
///
/// * `xml` - XML content as string
///
/// # Returns
///
/// Parsed `EventSubscription` structure
///
/// # Example
///
/// ```no_run
/// # use bsl_metadata::xml_parser::parse_event_subscription_xml;
/// let xml = std::fs::read_to_string("EventSubscriptions/MySubscription.xml")?;
/// let subscription = parse_event_subscription_xml(&xml)?;
/// # Ok::<(), bsl_metadata::MetadataError>(())
/// ```
pub fn parse_event_subscription_xml(xml: &str) -> Result<EventSubscription> {
    let _span = tracing::debug_span!("parse_event_subscription_xml").entered();

    // Parse XML
    let root: EventSubscriptionRoot = quick_xml::de::from_str(xml)?;

    // Extract UUID
    let uuid =
        root.event_subscription.uuid.parse::<Uuid>().map_err(|e| {
            crate::error::MetadataError::InvalidFormat(format!("Invalid UUID: {}", e))
        })?;

    // Build EventSubscription using manual construction (no builder pattern needed)
    let subscription = EventSubscription {
        uuid,
        name: root.event_subscription.properties.name,
        comment: root.event_subscription.properties.comment,
        source: root.event_subscription.properties.source.as_string(),
        event: root.event_subscription.properties.event,
        handler: root.event_subscription.properties.handler,
    };

    tracing::debug!(
        subscription_name = %subscription.name(),
        uuid = %subscription.uuid,
        handler = %subscription.handler_string(),
        "parsed event subscription"
    );

    Ok(subscription)
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

    #[test]
    fn test_parse_information_register_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <InformationRegister uuid="59f8d329-f39c-4999-b470-ae9fc74511ac">
        <Properties>
            <Name>РегистрСведений1</Name>
        </Properties>
        <ChildObjects>
            <Dimension uuid="532f2a7f-4c1e-4a49-8281-3c21232da2d7">
                <Properties>
                    <Name>Справочник1</Name>
                    <Master>false</Master>
                    <DenyIncompleteValues>false</DenyIncompleteValues>
                    <Indexing>DontIndex</Indexing>
                </Properties>
            </Dimension>
        </ChildObjects>
    </InformationRegister>
</MetaDataObject>"#;

        let register = parse_information_register_xml(xml).unwrap();

        assert_eq!(register.name(), "РегистрСведений1");
        assert_eq!(register.uuid().to_string(), "59f8d329-f39c-4999-b470-ae9fc74511ac");
        assert!(register.is_information_register());
        assert_eq!(register.dimensions().len(), 1);

        let dimension = &register.dimensions()[0];
        assert_eq!(dimension.name(), "Справочник1");
        assert_eq!(dimension.uuid().to_string(), "532f2a7f-4c1e-4a49-8281-3c21232da2d7");
        assert!(!dimension.is_deny_incomplete_values());
        assert!(!dimension.is_master());
        assert_eq!(dimension.indexing(), "DontIndex");
    }

    #[test]
    fn test_parse_accumulation_register_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <AccumulationRegister uuid="11111111-1111-1111-1111-111111111111">
        <Properties>
            <Name>РегистрНакопления1</Name>
        </Properties>
        <ChildObjects>
            <Dimension uuid="22222222-2222-2222-2222-222222222222">
                <Properties>
                    <Name>Измерение1</Name>
                    <Master>true</Master>
                    <DenyIncompleteValues>true</DenyIncompleteValues>
                    <Indexing>Index</Indexing>
                </Properties>
            </Dimension>
        </ChildObjects>
    </AccumulationRegister>
</MetaDataObject>"#;

        let register = parse_accumulation_register_xml(xml).unwrap();

        assert_eq!(register.name(), "РегистрНакопления1");
        assert!(register.is_accumulation_register());
        assert_eq!(register.dimensions().len(), 1);

        let dimension = &register.dimensions()[0];
        assert_eq!(dimension.name(), "Измерение1");
        assert!(dimension.is_deny_incomplete_values());
        assert!(dimension.is_master());
        assert_eq!(dimension.indexing(), "Index");
    }

    #[test]
    fn test_parse_register_with_multiple_dimensions() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <InformationRegister uuid="33333333-3333-3333-3333-333333333333">
        <Properties>
            <Name>МногоИзмерений</Name>
        </Properties>
        <ChildObjects>
            <Dimension uuid="44444444-4444-4444-4444-444444444444">
                <Properties>
                    <Name>Измерение1</Name>
                    <DenyIncompleteValues>true</DenyIncompleteValues>
                    <Master>false</Master>
                    <Indexing>Index</Indexing>
                </Properties>
            </Dimension>
            <Dimension uuid="55555555-5555-5555-5555-555555555555">
                <Properties>
                    <Name>Измерение2</Name>
                    <DenyIncompleteValues>false</DenyIncompleteValues>
                    <Master>true</Master>
                    <Indexing>DontIndex</Indexing>
                </Properties>
            </Dimension>
        </ChildObjects>
    </InformationRegister>
</MetaDataObject>"#;

        let register = parse_information_register_xml(xml).unwrap();

        assert_eq!(register.name(), "МногоИзмерений");
        assert_eq!(register.dimensions().len(), 2);

        assert_eq!(register.dimensions()[0].name(), "Измерение1");
        assert!(register.dimensions()[0].is_deny_incomplete_values());
        assert!(!register.dimensions()[0].is_master());

        assert_eq!(register.dimensions()[1].name(), "Измерение2");
        assert!(!register.dimensions()[1].is_deny_incomplete_values());
        assert!(register.dimensions()[1].is_master());
    }

    #[test]
    fn test_parse_register_without_dimensions() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <InformationRegister uuid="66666666-6666-6666-6666-666666666666">
        <Properties>
            <Name>БезИзмерений</Name>
        </Properties>
    </InformationRegister>
</MetaDataObject>"#;

        let register = parse_information_register_xml(xml).unwrap();

        assert_eq!(register.name(), "БезИзмерений");
        assert_eq!(register.dimensions().len(), 0);
    }

    #[test]
    fn test_parse_all_register_types() {
        // Test InformationRegister
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <InformationRegister uuid="77777777-7777-7777-7777-777777777777">
        <Properties>
            <Name>ТестРегистр</Name>
        </Properties>
    </InformationRegister>
</MetaDataObject>"#;
        let register = parse_information_register_xml(xml).unwrap();
        assert_eq!(register.name(), "ТестРегистр");
        assert_eq!(register.mdo_type(), MdoType::InformationRegister);

        // Test AccumulationRegister
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <AccumulationRegister uuid="77777777-7777-7777-7777-777777777777">
        <Properties>
            <Name>ТестРегистр</Name>
        </Properties>
    </AccumulationRegister>
</MetaDataObject>"#;
        let register = parse_accumulation_register_xml(xml).unwrap();
        assert_eq!(register.name(), "ТестРегистр");
        assert_eq!(register.mdo_type(), MdoType::AccumulationRegister);

        // Test AccountingRegister
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <AccountingRegister uuid="77777777-7777-7777-7777-777777777777">
        <Properties>
            <Name>ТестРегистр</Name>
        </Properties>
    </AccountingRegister>
</MetaDataObject>"#;
        let register = parse_accounting_register_xml(xml).unwrap();
        assert_eq!(register.name(), "ТестРегистр");
        assert_eq!(register.mdo_type(), MdoType::AccountingRegister);

        // Test CalculationRegister
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <CalculationRegister uuid="77777777-7777-7777-7777-777777777777">
        <Properties>
            <Name>ТестРегистр</Name>
        </Properties>
    </CalculationRegister>
</MetaDataObject>"#;
        let register = parse_calculation_register_xml(xml).unwrap();
        assert_eq!(register.name(), "ТестРегистр");
        assert_eq!(register.mdo_type(), MdoType::CalculationRegister);
    }

    #[test]
    fn test_parse_event_subscription_xml_with_handler() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" version="2.10">
    <EventSubscription uuid="e557865e-afb4-4f72-b89b-5e7cf98d2029">
        <Properties>
            <Name>ПриЗаписиСправочника</Name>
            <Comment></Comment>
            <Source>
                <v8:Type>cfg:CatalogObject.Справочник1</v8:Type>
            </Source>
            <Event>OnWrite</Event>
            <Handler>CommonModule.ОбщийПодпискиНаСобытия.ПриЗаписиСправочника</Handler>
        </Properties>
    </EventSubscription>
</MetaDataObject>"#;

        let subscription = parse_event_subscription_xml(xml).unwrap();

        assert_eq!(subscription.name(), "ПриЗаписиСправочника");
        assert_eq!(subscription.uuid.to_string(), "e557865e-afb4-4f72-b89b-5e7cf98d2029");
        assert_eq!(subscription.event(), "OnWrite");
        assert_eq!(
            subscription.handler_string(),
            "CommonModule.ОбщийПодпискиНаСобытия.ПриЗаписиСправочника"
        );

        // Test handler parsing
        let handler = subscription.parse_handler().unwrap();
        assert_eq!(handler.module_name, "ОбщийПодпискиНаСобытия");
        assert_eq!(handler.method_name, "ПриЗаписиСправочника");
    }

    #[test]
    fn test_parse_event_subscription_xml_empty_handler() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" version="2.10">
    <EventSubscription uuid="90047d26-54b2-4ea0-a566-a6adc71b4d15">
        <Properties>
            <Name>ПередЗаписьюКонстанты</Name>
            <Comment></Comment>
            <Source>
                <v8:TypeSet>cfg:ConstantValueManager</v8:TypeSet>
            </Source>
            <Event>BeforeWrite</Event>
            <Handler></Handler>
        </Properties>
    </EventSubscription>
</MetaDataObject>"#;

        let subscription = parse_event_subscription_xml(xml).unwrap();

        assert_eq!(subscription.name(), "ПередЗаписьюКонстанты");
        assert_eq!(subscription.event(), "BeforeWrite");
        assert_eq!(subscription.handler_string(), "");

        // Empty handler should return None
        assert!(subscription.parse_handler().is_none());
    }

    #[test]
    fn test_parse_event_subscription_xml_malformed_handler() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" version="2.10">
    <EventSubscription uuid="12345678-1234-1234-1234-123456789012">
        <Properties>
            <Name>TestSubscription</Name>
            <Source>
                <v8:Type>cfg:DocumentObject.Test</v8:Type>
            </Source>
            <Event>OnWrite</Event>
            <Handler>CommonModule.ОбщийПодпискиНаСобытия</Handler>
        </Properties>
    </EventSubscription>
</MetaDataObject>"#;

        let subscription = parse_event_subscription_xml(xml).unwrap();

        assert_eq!(subscription.name(), "TestSubscription");
        assert_eq!(subscription.handler_string(), "CommonModule.ОбщийПодпискиНаСобытия");

        // Malformed handler (missing method) should return Some with empty method_name
        let handler = subscription.parse_handler().unwrap();
        assert_eq!(handler.module_name, "ОбщийПодпискиНаСобытия");
        assert_eq!(handler.method_name, "");
    }
}
