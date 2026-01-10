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

    // For InformationRegister
    #[serde(rename = "InformationRegisterPeriodicity", default)]
    periodicity: Option<String>,

    #[serde(rename = "EnableTotalsSliceFirst", default)]
    enable_totals_slice_first: BoolValue,

    #[serde(rename = "EnableTotalsSliceLast", default)]
    enable_totals_slice_last: BoolValue,

    // For AccumulationRegister
    #[serde(rename = "RegisterType", default)]
    register_type: Option<String>,
}

/// ChildObjects container for dimensions, resources, and attributes
#[derive(Debug, Deserialize)]
struct ChildObjects {
    #[serde(rename = "Dimension", default)]
    dimensions: Vec<DimensionXml>,

    #[serde(rename = "Resource", default)]
    resources: Vec<ResourceXml>,

    #[serde(rename = "Attribute", default)]
    attributes: Vec<RegisterAttributeXml>,
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

/// Resource XML structure
#[derive(Debug, Deserialize)]
struct ResourceXml {
    #[serde(rename = "@uuid")]
    uuid: String,

    #[serde(rename = "Properties")]
    properties: ResourceProperties,
}

/// Resource Properties
#[derive(Debug, Deserialize)]
struct ResourceProperties {
    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "Type")]
    resource_type: TypeXml,
}

/// RegisterAttribute XML structure
#[derive(Debug, Deserialize)]
struct RegisterAttributeXml {
    #[serde(rename = "@uuid")]
    uuid: String,

    #[serde(rename = "Properties")]
    properties: RegisterAttributeProperties,
}

/// RegisterAttribute Properties
#[derive(Debug, Deserialize)]
struct RegisterAttributeProperties {
    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "Type")]
    attr_type: TypeXml,
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
    let mut resources = Vec::new();
    let mut attributes = Vec::new();

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

        for resource_xml in child_objects.resources {
            let resource_uuid = resource_xml.uuid.parse::<Uuid>().map_err(|e| {
                crate::error::MetadataError::InvalidFormat(format!("Invalid resource UUID: {}", e))
            })?;

            let mut resource =
                crate::register::RegisterResource::new(resource_uuid, resource_xml.properties.name);
            let resource_type = parse_type_xml(&resource_xml.properties.resource_type)?;
            resource.set_type_str(format!("{:?}", resource_type));
            resource.set_attr_type(resource_type);

            resources.push(resource);
        }

        for attr_xml in child_objects.attributes {
            let attr_uuid = attr_xml.uuid.parse::<Uuid>().map_err(|e| {
                crate::error::MetadataError::InvalidFormat(format!("Invalid attribute UUID: {}", e))
            })?;

            let mut attribute =
                crate::register::RegisterAttribute::new(attr_uuid, attr_xml.properties.name);
            let attr_type = parse_type_xml(&attr_xml.properties.attr_type)?;
            attribute.set_type_str(format!("{:?}", attr_type));
            attribute.set_attr_type(attr_type);

            attributes.push(attribute);
        }
    }

    // Parse periodicity (for InformationRegister)
    let periodicity = if mdo_type == MdoType::InformationRegister {
        root.register.properties.periodicity.and_then(|p| match p.as_str() {
            "Nonperiodical" => Some(crate::register::RegisterPeriodicity::Nonperiodical),
            "Second" => Some(crate::register::RegisterPeriodicity::Second),
            "Day" => Some(crate::register::RegisterPeriodicity::Day),
            "Month" => Some(crate::register::RegisterPeriodicity::Month),
            "RecorderPosition" => Some(crate::register::RegisterPeriodicity::RecorderPosition),
            _ => None,
        })
    } else {
        None
    };

    // Parse register type (for AccumulationRegister)
    let register_type = if mdo_type == MdoType::AccumulationRegister {
        root.register.properties.register_type.and_then(|rt| match rt.as_str() {
            "Balance" => Some(crate::register::AccumulationRegisterType::Balance),
            "Turnovers" => Some(crate::register::AccumulationRegisterType::Turnovers),
            "BalanceAndTurnovers" => {
                Some(crate::register::AccumulationRegisterType::BalanceAndTurnovers)
            }
            _ => None,
        })
    } else {
        None
    };

    let register = Register::builder()
        .uuid(uuid)
        .name(root.register.properties.name)
        .mdo_type(mdo_type)
        .dimensions(dimensions)
        .resources(resources)
        .attributes(attributes)
        .periodicity(periodicity)
        .register_type(register_type)
        .enable_totals_slice_first(root.register.properties.enable_totals_slice_first.into())
        .enable_totals_slice_last(root.register.properties.enable_totals_slice_last.into())
        .build();

    tracing::debug!(
        register_name = %register.name(),
        uuid = %register.uuid(),
        mdo_type = ?register.mdo_type(),
        dimensions = register.dimensions().len(),
        resources = register.resources().len(),
        attributes = register.attributes().len(),
        periodicity = ?register.periodicity(),
        register_type = ?register.register_type(),
        enable_totals_slice_first = register.enable_totals_slice_first(),
        enable_totals_slice_last = register.enable_totals_slice_last(),
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

/// Root XML structure for Catalog
///
/// Designer format structure:
/// ```xml
/// <MetaDataObject xmlns="...">
///   <Catalog uuid="...">
///     <Properties>
///       <Name>...</Name>
///     </Properties>
///     <ChildObjects>
///       <Attribute uuid="...">
///         <Properties>
///           <Name>...</Name>
///           <Type>...</Type>
///         </Properties>
///       </Attribute>
///     </ChildObjects>
///   </Catalog>
/// </MetaDataObject>
/// ```
#[derive(Debug, Deserialize)]
struct CatalogRoot {
    #[serde(rename = "Catalog")]
    catalog: MetadataObjectXml,
}

/// Root XML structure for Document
#[derive(Debug, Deserialize)]
struct DocumentRoot {
    #[serde(rename = "Document")]
    document: MetadataObjectXml,
}

/// Generic metadata object XML structure (Catalog, Document, etc.)
#[derive(Debug, Deserialize)]
struct MetadataObjectXml {
    #[serde(rename = "@uuid")]
    _uuid: String,

    #[serde(rename = "Properties")]
    properties: MetadataObjectProperties,

    #[serde(rename = "ChildObjects", default)]
    child_objects: Option<MetadataChildObjects>,
}

/// Metadata object properties
#[derive(Debug, Deserialize)]
struct MetadataObjectProperties {
    #[serde(rename = "Name")]
    name: String,
}

/// Child objects container (attributes, tabular sections, etc.)
#[derive(Debug, Deserialize)]
struct MetadataChildObjects {
    #[serde(rename = "Attribute", default)]
    attributes: Vec<AttributeXml>,

    // For InformationRegisters
    #[serde(rename = "Resource", default)]
    resources: Vec<AttributeXml>,

    // For InformationRegisters
    #[serde(rename = "Dimension", default)]
    dimensions_as_attributes: Vec<AttributeXml>,

    // Tabular sections (for Catalog, Document, ChartOfCharacteristicTypes, etc.)
    #[serde(rename = "TabularSection", default)]
    tabular_sections: Vec<TabularSectionXml>,
}

/// Attribute XML structure
#[derive(Debug, Deserialize)]
struct AttributeXml {
    #[serde(rename = "@uuid")]
    _uuid: String,

    #[serde(rename = "Properties")]
    properties: AttributeProperties,
}

/// Attribute properties
#[derive(Debug, Deserialize)]
struct AttributeProperties {
    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "Type")]
    attr_type: TypeXml,
}

/// Type XML structure
///
/// Handles multiple type variants:
/// - `<v8:Type>xs:boolean</v8:Type>`
/// - `<v8:Type>xs:string</v8:Type><v8:StringQualifiers><v8:Length>100</v8:Length>...`
/// - `<v8:Type>xs:decimal</v8:Type><v8:NumberQualifiers><v8:Digits>10</v8:Digits>...`
/// - `<v8:Type>cfg:CatalogRef.Name</v8:Type>`
#[derive(Debug, Deserialize)]
struct TypeXml {
    /// Type value (can be multiple)
    #[serde(rename = "Type", default)]
    types: Vec<String>,

    /// String qualifiers (for xs:string)
    #[serde(rename = "StringQualifiers", default)]
    string_qualifiers: Option<StringQualifiers>,

    /// Number qualifiers (for xs:decimal)
    #[serde(rename = "NumberQualifiers", default)]
    number_qualifiers: Option<NumberQualifiers>,

    /// Date qualifiers (for xs:dateTime)
    #[serde(rename = "DateQualifiers", default)]
    date_qualifiers: Option<DateQualifiers>,
}

/// String qualifiers
#[derive(Debug, Deserialize)]
struct StringQualifiers {
    #[serde(rename = "Length", default)]
    length: Option<u32>,
}

/// Number qualifiers
#[derive(Debug, Deserialize)]
struct NumberQualifiers {
    #[serde(rename = "Digits", default)]
    digits: Option<u8>,

    #[serde(rename = "FractionDigits", default)]
    fraction_digits: Option<u8>,
}

/// Date qualifiers
#[derive(Debug, Deserialize)]
struct DateQualifiers {
    #[serde(rename = "DateFractions", default)]
    date_fractions: Option<String>,
}

/// Tabular Section XML structure
#[derive(Debug, Deserialize)]
struct TabularSectionXml {
    #[serde(rename = "@uuid")]
    uuid: String,

    #[serde(rename = "Properties")]
    properties: TabularSectionProperties,

    #[serde(rename = "ChildObjects", default)]
    child_objects: Option<TabularSectionChildObjects>,
}

/// Tabular Section properties
#[derive(Debug, Deserialize)]
struct TabularSectionProperties {
    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "Synonym", default)]
    synonym: Option<SynonymXml>,

    #[serde(rename = "Use", default)]
    use_mode: Option<String>,
}

/// Synonym XML structure (wraps the actual text value)
#[derive(Debug, Deserialize)]
struct SynonymXml {
    #[serde(rename = "$text", default)]
    value: Option<String>,
}

/// Child objects of a tabular section (attributes)
#[derive(Debug, Deserialize)]
struct TabularSectionChildObjects {
    #[serde(rename = "Attribute", default)]
    attributes: Vec<AttributeXml>,
}

/// Parse Catalog XML from Designer format
///
/// # Arguments
///
/// * `xml` - XML content as string
///
/// # Returns
///
/// Parsed `MetadataObject` structure with attributes
///
/// # Example
///
/// ```no_run
/// # use bsl_metadata::xml_parser::parse_catalog_xml;
/// let xml = std::fs::read_to_string("Catalogs/Валюты.xml")?;
/// let catalog = parse_catalog_xml(&xml)?;
/// # Ok::<(), bsl_metadata::MetadataError>(())
/// ```
pub fn parse_catalog_xml(xml: &str) -> Result<crate::metadata_object::MetadataObject> {
    let _span = tracing::debug_span!("parse_catalog_xml").entered();

    let root: CatalogRoot = quick_xml::de::from_str(xml)?;

    parse_metadata_object(root.catalog, MdoType::Catalog)
}

/// Parse Document XML from Designer format
pub fn parse_document_xml(xml: &str) -> Result<crate::metadata_object::MetadataObject> {
    let _span = tracing::debug_span!("parse_document_xml").entered();

    let root: DocumentRoot = quick_xml::de::from_str(xml)?;

    parse_metadata_object(root.document, MdoType::Document)
}

/// Internal helper to parse metadata object XML
fn parse_metadata_object(
    obj_xml: MetadataObjectXml,
    mdo_type: MdoType,
) -> Result<crate::metadata_object::MetadataObject> {
    use crate::metadata_object::MetadataObject;

    let mut attributes = Vec::new();
    let mut tabular_sections = Vec::new();

    if let Some(child_objects) = obj_xml.child_objects {
        // Parse regular Attributes (for Catalog, Document)
        for attr_xml in child_objects.attributes {
            let attr = parse_attribute(attr_xml)?;
            attributes.push(attr);
        }

        // Parse Resources (for InformationRegister - treated as attributes)
        for resource_xml in child_objects.resources {
            let attr = parse_attribute(resource_xml)?;
            attributes.push(attr);
        }

        // Parse Dimensions (for InformationRegister - treated as attributes)
        for dim_xml in child_objects.dimensions_as_attributes {
            let attr = parse_attribute(dim_xml)?;
            attributes.push(attr);
        }

        // Parse Tabular Sections (for Catalog, Document, ChartOfCharacteristicTypes, etc.)
        for ts_xml in child_objects.tabular_sections {
            let tabular_section = parse_tabular_section(ts_xml)?;
            tabular_sections.push(tabular_section);
        }
    }

    let mut mdo = MetadataObject::new(mdo_type, obj_xml.properties.name);
    for attr in attributes {
        mdo.add_attribute(attr);
    }
    for ts in tabular_sections {
        mdo.add_tabular_section(ts);
    }

    tracing::debug!(
        mdo_name = %mdo.name,
        mdo_type = ?mdo.mdo_type,
        attributes = mdo.attributes.len(),
        tabular_sections = mdo.tabular_sections.len(),
        "parsed metadata object"
    );

    Ok(mdo)
}

/// Parse single attribute from XML
fn parse_attribute(attr_xml: AttributeXml) -> Result<crate::metadata_object::Attribute> {
    use crate::metadata_object::Attribute;

    let attr_type = parse_type_xml(&attr_xml.properties.attr_type)?;

    Ok(Attribute { name: attr_xml.properties.name, name_en: None, attr_type })
}

/// Parse TabularSection XML into TabularSection
fn parse_tabular_section(
    ts_xml: TabularSectionXml,
) -> Result<crate::tabular_section::TabularSection> {
    use crate::tabular_section::{TabularSection, TabularSectionAttribute};

    let uuid = ts_xml
        .uuid
        .parse::<Uuid>()
        .map_err(|e| crate::error::MetadataError::InvalidFormat(format!("Invalid UUID: {}", e)))?;

    let name = ts_xml.properties.name;
    let mut tabular_section = TabularSection::new(uuid, name);

    // Set synonym if present
    if let Some(synonym_xml) = ts_xml.properties.synonym {
        if let Some(synonym_value) = synonym_xml.value {
            tabular_section.set_synonym(Some(synonym_value));
        }
    }

    // Set use mode if present
    tabular_section.set_use_mode(ts_xml.properties.use_mode);

    // Parse attributes of the tabular section
    if let Some(child_objects) = ts_xml.child_objects {
        let mut ts_attributes = Vec::new();

        for attr_xml in child_objects.attributes {
            let attr_uuid = attr_xml._uuid.parse::<Uuid>().map_err(|e| {
                crate::error::MetadataError::InvalidFormat(format!("Invalid UUID: {}", e))
            })?;

            let attr_type = parse_type_xml(&attr_xml.properties.attr_type)?;
            let type_str = format!("{:?}", attr_type); // Convert AttributeType to string representation

            let ts_attr =
                TabularSectionAttribute::new(attr_uuid, attr_xml.properties.name, type_str);

            ts_attributes.push(ts_attr);
        }

        tabular_section.set_attributes(ts_attributes);
    }

    Ok(tabular_section)
}

/// Parse Type XML into AttributeType
fn parse_type_xml(type_xml: &TypeXml) -> Result<crate::metadata_object::AttributeType> {
    use crate::metadata_object::AttributeType;

    // If no types specified, return Unknown
    if type_xml.types.is_empty() {
        return Ok(AttributeType::Unknown);
    }

    // Take first type (1C can have multiple, we'll use first for now)
    let type_str = &type_xml.types[0];

    // Parse type string
    match type_str.as_str() {
        "xs:boolean" => Ok(AttributeType::Boolean),

        "xs:string" => {
            let length = type_xml.string_qualifiers.as_ref().and_then(|q| q.length);
            Ok(AttributeType::String { length })
        }

        "xs:decimal" => {
            let precision =
                type_xml.number_qualifiers.as_ref().and_then(|q| q.digits).unwrap_or(10);
            let scale =
                type_xml.number_qualifiers.as_ref().and_then(|q| q.fraction_digits).unwrap_or(0);
            Ok(AttributeType::Number { precision, scale })
        }

        "xs:dateTime" => {
            // Check if DateTime or Date
            let is_datetime = type_xml
                .date_qualifiers
                .as_ref()
                .and_then(|q| q.date_fractions.as_deref())
                .map(|df| df.eq_ignore_ascii_case("DateTime"))
                .unwrap_or(false);

            if is_datetime {
                Ok(AttributeType::DateTime)
            } else {
                Ok(AttributeType::Date)
            }
        }

        // Reference types: "cfg:CatalogRef.Name", "cfg:DocumentRef.Name"
        type_str if type_str.starts_with("cfg:") => parse_reference_type(type_str),

        // UUID, ValueStorage, etc.
        "v8:UUID" | "v8:ValueStorage" => Ok(AttributeType::Unknown),

        _ => {
            tracing::warn!(type_str = %type_str, "unknown type");
            Ok(AttributeType::Unknown)
        }
    }
}

/// Parse reference type string like "cfg:CatalogRef.Валюты"
fn parse_reference_type(type_str: &str) -> Result<crate::metadata_object::AttributeType> {
    use crate::metadata_object::AttributeType;

    // Format: "cfg:CatalogRef.Name" or "cfg:DocumentRef.Name" or "cfg:EnumRef.Name"
    let parts: Vec<&str> = type_str.split('.').collect();
    if parts.len() != 2 {
        tracing::warn!(type_str = %type_str, "invalid reference type format");
        return Ok(AttributeType::Unknown);
    }

    let ref_type = parts[0];
    let name = parts[1].to_string();

    let mdo_type = match ref_type {
        "cfg:CatalogRef" => MdoType::Catalog,
        "cfg:DocumentRef" => MdoType::Document,
        "cfg:InformationRegisterRef" => MdoType::InformationRegister,
        "cfg:AccumulationRegisterRef" => MdoType::AccumulationRegister,
        "cfg:AccountingRegisterRef" => MdoType::AccountingRegister,
        "cfg:CalculationRegisterRef" => MdoType::CalculationRegister,
        "cfg:EnumRef" => MdoType::Enum,
        "cfg:TaskRef" => MdoType::Task,
        "cfg:ExchangePlanRef" => MdoType::ExchangePlan,
        "cfg:BusinessProcessRef" => MdoType::BusinessProcess,
        "cfg:ChartOfCharacteristicTypesRef" => MdoType::ChartOfCharacteristicTypes,
        "cfg:ChartOfAccountsRef" => MdoType::ChartOfAccounts,
        "cfg:ChartOfCalculationTypesRef" => MdoType::ChartOfCalculationTypes,
        _ => {
            tracing::warn!(ref_type = %ref_type, "unsupported reference type");
            return Ok(AttributeType::Unknown);
        }
    };

    Ok(AttributeType::Ref { mdo_type, name })
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

    #[test]
    fn test_parse_catalog_xml_with_attributes() {
        use crate::metadata_object::AttributeType;

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:cfg="http://v8.1c.ru/8.1/data/enterprise/current-config" version="2.10">
    <Catalog uuid="1d6b8425-360c-4ab1-9bab-cc9a3b590bb2">
        <Properties>
            <Name>Валюты</Name>
        </Properties>
        <ChildObjects>
            <Attribute uuid="9f67d228-79aa-44e6-8dc7-fae4fbdfef2a">
                <Properties>
                    <Name>ЗагружаетсяИзИнтернета</Name>
                    <Type>
                        <v8:Type>xs:boolean</v8:Type>
                    </Type>
                </Properties>
            </Attribute>
            <Attribute uuid="231d3950-f363-4e63-83cd-8ddb81507c27">
                <Properties>
                    <Name>НаименованиеПолное</Name>
                    <Type>
                        <v8:Type>xs:string</v8:Type>
                        <v8:StringQualifiers>
                            <v8:Length>50</v8:Length>
                        </v8:StringQualifiers>
                    </Type>
                </Properties>
            </Attribute>
            <Attribute uuid="87429f11-bf95-4013-bf13-da904570f88d">
                <Properties>
                    <Name>Наценка</Name>
                    <Type>
                        <v8:Type>xs:decimal</v8:Type>
                        <v8:NumberQualifiers>
                            <v8:Digits>10</v8:Digits>
                            <v8:FractionDigits>2</v8:FractionDigits>
                        </v8:NumberQualifiers>
                    </Type>
                </Properties>
            </Attribute>
            <Attribute uuid="6173cab2-e0f5-40c1-8e74-4f41fc8bd68f">
                <Properties>
                    <Name>ОсновнаяВалюта</Name>
                    <Type>
                        <v8:Type>cfg:CatalogRef.Валюты</v8:Type>
                    </Type>
                </Properties>
            </Attribute>
        </ChildObjects>
    </Catalog>
</MetaDataObject>"#;

        let catalog = parse_catalog_xml(xml).unwrap();

        assert_eq!(catalog.name, "Валюты");
        assert_eq!(catalog.attributes.len(), 4);

        // Check Boolean attribute
        let attr1 = catalog.find_attribute("ЗагружаетсяИзИнтернета").unwrap();
        assert_eq!(attr1.name, "ЗагружаетсяИзИнтернета");
        assert_eq!(attr1.attr_type, AttributeType::Boolean);

        // Check String attribute with length
        let attr2 = catalog.find_attribute("НаименованиеПолное").unwrap();
        assert_eq!(attr2.name, "НаименованиеПолное");
        assert_eq!(attr2.attr_type, AttributeType::String { length: Some(50) });

        // Check Number attribute
        let attr3 = catalog.find_attribute("Наценка").unwrap();
        assert_eq!(attr3.name, "Наценка");
        assert_eq!(attr3.attr_type, AttributeType::Number { precision: 10, scale: 2 });

        // Check Reference attribute
        let attr4 = catalog.find_attribute("ОсновнаяВалюта").unwrap();
        assert_eq!(attr4.name, "ОсновнаяВалюта");
        assert_eq!(
            attr4.attr_type,
            AttributeType::Ref { mdo_type: MdoType::Catalog, name: "Валюты".to_string() }
        );
    }

    #[test]
    fn test_parse_catalog_xml_no_attributes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa">
        <Properties>
            <Name>ПростойСправочник</Name>
        </Properties>
    </Catalog>
</MetaDataObject>"#;

        let catalog = parse_catalog_xml(xml).unwrap();

        assert_eq!(catalog.name, "ПростойСправочник");
        assert_eq!(catalog.attributes.len(), 0);
    }

    #[test]
    fn test_parse_document_xml_with_attributes() {
        use crate::metadata_object::AttributeType;

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:cfg="http://v8.1c.ru/8.1/data/enterprise/current-config" version="2.10">
    <Document uuid="bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb">
        <Properties>
            <Name>ЗаказПокупателя</Name>
        </Properties>
        <ChildObjects>
            <Attribute uuid="cccccccc-cccc-cccc-cccc-cccccccccccc">
                <Properties>
                    <Name>Контрагент</Name>
                    <Type>
                        <v8:Type>cfg:CatalogRef.Контрагенты</v8:Type>
                    </Type>
                </Properties>
            </Attribute>
            <Attribute uuid="dddddddd-dddd-dddd-dddd-dddddddddddd">
                <Properties>
                    <Name>Дата</Name>
                    <Type>
                        <v8:Type>xs:dateTime</v8:Type>
                        <v8:DateQualifiers>
                            <v8:DateFractions>DateTime</v8:DateFractions>
                        </v8:DateQualifiers>
                    </Type>
                </Properties>
            </Attribute>
        </ChildObjects>
    </Document>
</MetaDataObject>"#;

        let document = parse_document_xml(xml).unwrap();

        assert_eq!(document.name, "ЗаказПокупателя");
        assert_eq!(document.attributes.len(), 2);

        // Check Reference attribute
        let attr1 = document.find_attribute("Контрагент").unwrap();
        assert_eq!(attr1.name, "Контрагент");
        assert_eq!(
            attr1.attr_type,
            AttributeType::Ref {
                mdo_type: MdoType::Catalog, name: "Контрагенты".to_string()
            }
        );

        // Check DateTime attribute
        let attr2 = document.find_attribute("Дата").unwrap();
        assert_eq!(attr2.name, "Дата");
        assert_eq!(attr2.attr_type, AttributeType::DateTime);
    }

    #[test]
    fn test_parse_type_xml_date() {
        use crate::metadata_object::AttributeType;

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" version="2.10">
    <Catalog uuid="eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee">
        <Properties>
            <Name>Тест</Name>
        </Properties>
        <ChildObjects>
            <Attribute uuid="ffffffff-ffff-ffff-ffff-ffffffffffff">
                <Properties>
                    <Name>ДатаБезВремени</Name>
                    <Type>
                        <v8:Type>xs:dateTime</v8:Type>
                        <v8:DateQualifiers>
                            <v8:DateFractions>Date</v8:DateFractions>
                        </v8:DateQualifiers>
                    </Type>
                </Properties>
            </Attribute>
        </ChildObjects>
    </Catalog>
</MetaDataObject>"#;

        let catalog = parse_catalog_xml(xml).unwrap();

        let attr = catalog.find_attribute("ДатаБезВремени").unwrap();
        assert_eq!(attr.attr_type, AttributeType::Date);
    }

    #[test]
    fn test_parse_type_xml_unknown_types() {
        use crate::metadata_object::AttributeType;

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:cfg="http://v8.1c.ru/8.1/data/enterprise/current-config" version="2.10">
    <Catalog uuid="aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa">
        <Properties>
            <Name>Тест</Name>
        </Properties>
        <ChildObjects>
            <Attribute uuid="bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb">
                <Properties>
                    <Name>УникальныйИдентификатор</Name>
                    <Type>
                        <v8:Type>v8:UUID</v8:Type>
                    </Type>
                </Properties>
            </Attribute>
            <Attribute uuid="cccccccc-cccc-cccc-cccc-cccccccccccc">
                <Properties>
                    <Name>Хранилище</Name>
                    <Type>
                        <v8:Type>v8:ValueStorage</v8:Type>
                    </Type>
                </Properties>
            </Attribute>
            <Attribute uuid="dddddddd-dddd-dddd-dddd-dddddddddddd">
                <Properties>
                    <Name>Перечисление</Name>
                    <Type>
                        <v8:Type>cfg:EnumRef.Статусы</v8:Type>
                    </Type>
                </Properties>
            </Attribute>
        </ChildObjects>
    </Catalog>
</MetaDataObject>"#;

        let catalog = parse_catalog_xml(xml).unwrap();

        // UUID -> Unknown
        let attr1 = catalog.find_attribute("УникальныйИдентификатор").unwrap();
        assert_eq!(attr1.attr_type, AttributeType::Unknown);

        // ValueStorage -> Unknown
        let attr2 = catalog.find_attribute("Хранилище").unwrap();
        assert_eq!(attr2.attr_type, AttributeType::Unknown);

        // EnumRef -> now supported
        let attr3 = catalog.find_attribute("Перечисление").unwrap();
        assert_eq!(
            attr3.attr_type,
            AttributeType::Ref { mdo_type: MdoType::Enum, name: "Статусы".to_string() }
        );
    }

    #[test]
    #[ignore] // Only run when doc3 project is available
    fn test_parse_real_catalog_from_doc3() {
        use crate::metadata_object::AttributeType;

        // This test uses a real Catalog from the doc3 benchmark project
        let xml_path = concat!(env!("HOME"), "/src/doc3/src/cf/Catalogs/Валюты.xml");

        // Skip if file doesn't exist
        if !std::path::Path::new(xml_path).exists() {
            eprintln!("Skipping test: doc3 project not found at {}", xml_path);
            return;
        }

        let xml = std::fs::read_to_string(xml_path).expect("Failed to read XML file");
        let catalog = parse_catalog_xml(&xml).expect("Failed to parse catalog XML");

        // Verify basic properties
        assert_eq!(catalog.name, "Валюты");

        // Should have multiple attributes (from doc3)
        assert!(
            catalog.attributes.len() >= 6,
            "Expected at least 6 attributes, got {}",
            catalog.attributes.len()
        );

        // Verify specific attributes we know exist in doc3
        let attr_bool = catalog.find_attribute("ЗагружаетсяИзИнтернета");
        assert!(attr_bool.is_some(), "Expected ЗагружаетсяИзИнтернета attribute");
        assert_eq!(attr_bool.unwrap().attr_type, AttributeType::Boolean);

        let attr_string = catalog.find_attribute("НаименованиеПолное");
        assert!(attr_string.is_some(), "Expected НаименованиеПолное attribute");
        assert!(matches!(attr_string.unwrap().attr_type, AttributeType::String { .. }));

        let attr_number = catalog.find_attribute("Наценка");
        assert!(attr_number.is_some(), "Expected Наценка attribute");
        assert!(matches!(attr_number.unwrap().attr_type, AttributeType::Number { .. }));

        let attr_ref = catalog.find_attribute("ОсновнаяВалюта");
        assert!(attr_ref.is_some(), "Expected ОсновнаяВалюта attribute");
        assert!(matches!(attr_ref.unwrap().attr_type, AttributeType::Ref { .. }));
    }

    #[test]
    #[ignore] // Only run when doc3 project is available
    fn test_load_from_directory_with_attributes() {
        use crate::loader::load_from_directory;

        // This test loads the full doc3 configuration and verifies attributes are loaded
        let config_path = concat!(env!("HOME"), "/src/doc3/src/cf");

        // Skip if directory doesn't exist
        if !std::path::Path::new(config_path).exists() {
            eprintln!("Skipping test: doc3 project not found at {}", config_path);
            return;
        }

        let config = load_from_directory(config_path).expect("Failed to load configuration");

        // Should have loaded catalogs with attributes
        assert!(!config.metadata_objects().is_empty(), "Expected some metadata objects");

        // Find Валюты catalog
        let currency_catalog = config.metadata_objects().iter().find(|obj| obj.name == "Валюты");

        if let Some(catalog) = currency_catalog {
            // Should have attributes loaded
            assert!(
                !catalog.attributes.is_empty(),
                "Expected Валюты catalog to have attributes loaded"
            );
            assert!(
                catalog.attributes.len() >= 6,
                "Expected at least 6 attributes in Валюты, got {}",
                catalog.attributes.len()
            );

            tracing::info!(
                catalog = %catalog.name,
                attributes = catalog.attributes.len(),
                "Successfully loaded catalog with attributes from doc3"
            );
        } else {
            panic!("Валюты catalog not found in metadata objects");
        }
    }

    #[test]
    fn test_parse_information_register_with_resources_and_attributes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:cfg="http://v8.1c.ru/8.1/data/enterprise/current-config" version="2.10">
    <InformationRegister uuid="12345678-1234-5678-1234-123456789012">
        <Properties>
            <Name>ЦеныНоменклатуры</Name>
            <InformationRegisterPeriodicity>Day</InformationRegisterPeriodicity>
            <EnableTotalsSliceFirst>true</EnableTotalsSliceFirst>
            <EnableTotalsSliceLast>true</EnableTotalsSliceLast>
        </Properties>
        <ChildObjects>
            <Dimension uuid="11111111-1111-1111-1111-111111111111">
                <Properties>
                    <Name>Номенклатура</Name>
                    <Type><v8:Type>cfg:CatalogRef.Номенклатура</v8:Type></Type>
                    <Master>false</Master>
                    <DenyIncompleteValues>true</DenyIncompleteValues>
                    <Indexing>Index</Indexing>
                </Properties>
            </Dimension>
            <Resource uuid="22222222-2222-2222-2222-222222222222">
                <Properties>
                    <Name>Цена</Name>
                    <Type>
                        <v8:Type>xs:decimal</v8:Type>
                        <v8:NumberQualifiers>
                            <v8:Digits>15</v8:Digits>
                            <v8:FractionDigits>2</v8:FractionDigits>
                        </v8:NumberQualifiers>
                    </Type>
                </Properties>
            </Resource>
            <Attribute uuid="33333333-3333-3333-3333-333333333333">
                <Properties>
                    <Name>Валюта</Name>
                    <Type><v8:Type>cfg:CatalogRef.Валюты</v8:Type></Type>
                </Properties>
            </Attribute>
        </ChildObjects>
    </InformationRegister>
</MetaDataObject>"#;

        let register = parse_information_register_xml(xml).unwrap();

        assert_eq!(register.name(), "ЦеныНоменклатуры");
        assert!(register.is_information_register());
        assert_eq!(register.dimensions().len(), 1);
        assert_eq!(register.resources().len(), 1);
        assert_eq!(register.attributes().len(), 1);

        assert_eq!(register.dimensions()[0].name(), "Номенклатура");
        assert_eq!(register.resources()[0].name(), "Цена");
        assert_eq!(register.attributes()[0].name(), "Валюта");
    }

    #[test]
    fn test_parse_accumulation_register_with_resources() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:cfg="http://v8.1c.ru/8.1/data/enterprise/current-config" version="2.10">
    <AccumulationRegister uuid="aaaaaaaa-1111-1111-1111-111111111111">
        <Properties>
            <Name>ТоварыНаСкладах</Name>
            <RegisterType>Balance</RegisterType>
        </Properties>
        <ChildObjects>
            <Dimension uuid="11111111-1111-1111-1111-111111111111">
                <Properties>
                    <Name>Номенклатура</Name>
                    <Type><v8:Type>cfg:CatalogRef.Номенклатура</v8:Type></Type>
                    <Master>false</Master>
                    <DenyIncompleteValues>true</DenyIncompleteValues>
                    <Indexing>Index</Indexing>
                </Properties>
            </Dimension>
            <Resource uuid="22222222-1111-1111-1111-111111111111">
                <Properties>
                    <Name>Количество</Name>
                    <Type>
                        <v8:Type>xs:decimal</v8:Type>
                        <v8:NumberQualifiers>
                            <v8:Digits>15</v8:Digits>
                            <v8:FractionDigits>3</v8:FractionDigits>
                        </v8:NumberQualifiers>
                    </Type>
                </Properties>
            </Resource>
        </ChildObjects>
    </AccumulationRegister>
</MetaDataObject>"#;

        let register = parse_accumulation_register_xml(xml).unwrap();

        assert_eq!(register.name(), "ТоварыНаСкладах");
        assert!(register.is_accumulation_register());
        assert_eq!(register.dimensions().len(), 1);
        assert_eq!(register.resources().len(), 1);

        assert_eq!(register.resources()[0].name(), "Количество");
    }

    #[test]
    fn test_parse_accumulation_register_with_resources_and_attributes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:cfg="http://v8.1c.ru/8.1/data/enterprise/current-config" version="2.10">
    <AccumulationRegister uuid="bbbbbbbb-2222-2222-2222-222222222222">
        <Properties>
            <Name>РабочееВремяСотрудников</Name>
            <RegisterType>Turnovers</RegisterType>
        </Properties>
        <ChildObjects>
            <Dimension uuid="11111111-2222-2222-2222-111111111111">
                <Properties>
                    <Name>Сотрудник</Name>
                    <Type><v8:Type>cfg:CatalogRef.Сотрудники</v8:Type></Type>
                    <Master>false</Master>
                    <DenyIncompleteValues>true</DenyIncompleteValues>
                    <Indexing>Index</Indexing>
                </Properties>
            </Dimension>
            <Resource uuid="22222222-2222-2222-2222-111111111111">
                <Properties>
                    <Name>ОтработаноЧасов</Name>
                    <Type>
                        <v8:Type>xs:decimal</v8:Type>
                        <v8:NumberQualifiers>
                            <v8:Digits>7</v8:Digits>
                            <v8:FractionDigits>2</v8:FractionDigits>
                        </v8:NumberQualifiers>
                    </Type>
                </Properties>
            </Resource>
            <Attribute uuid="33333333-1111-1111-1111-111111111111">
                <Properties>
                    <Name>Комментарий</Name>
                    <Type>
                        <v8:Type>xs:string</v8:Type>
                        <v8:StringQualifiers>
                            <v8:Length>100</v8:Length>
                        </v8:StringQualifiers>
                    </Type>
                </Properties>
            </Attribute>
        </ChildObjects>
    </AccumulationRegister>
</MetaDataObject>"#;

        let register = parse_accumulation_register_xml(xml).unwrap();

        assert_eq!(register.name(), "РабочееВремяСотрудников");
        assert!(register.is_accumulation_register());
        assert_eq!(register.dimensions().len(), 1);
        assert_eq!(register.resources().len(), 1);
        assert_eq!(register.attributes().len(), 1);

        assert_eq!(register.dimensions()[0].name(), "Сотрудник");
        assert_eq!(register.resources()[0].name(), "ОтработаноЧасов");
        assert_eq!(register.attributes()[0].name(), "Комментарий");
    }
}
