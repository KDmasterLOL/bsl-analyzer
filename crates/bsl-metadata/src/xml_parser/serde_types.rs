//! Serde deserialization structures for XML parsing
//!
//! All structures in this module are intermediate representations used to deserialize
//! Designer format XML into domain objects.

use serde::Deserialize;

// ============================================================================
// Common types
// ============================================================================

/// Helper type for deserializing bool values from XML text content
///
/// XML format: `<Server>true</Server>` or `<Server>false</Server>`
#[derive(Debug, Deserialize, Default, Clone)]
pub(crate) struct BoolValue {
    #[serde(rename = "$text", default)]
    pub value: Option<String>,
}

impl From<BoolValue> for bool {
    fn from(val: BoolValue) -> Self {
        val.value.as_ref().map(|s| s.eq_ignore_ascii_case("true")).unwrap_or(false)
    }
}

/// Helper type for deserializing integer values from XML text content
#[derive(Debug, Deserialize, Default, Clone)]
pub(crate) struct IntValue {
    #[serde(rename = "$text", default)]
    pub value: Option<u32>,
}

impl From<IntValue> for Option<u32> {
    fn from(val: IntValue) -> Self {
        val.value
    }
}

// ============================================================================
// CommonModule types
// ============================================================================

/// Root XML structure for CommonModule
#[derive(Debug, Deserialize)]
pub(crate) struct CommonModuleRoot {
    #[serde(rename = "CommonModule")]
    pub common_module: CommonModuleXml,
}

/// CommonModule XML structure
#[derive(Debug, Deserialize)]
pub(crate) struct CommonModuleXml {
    #[serde(rename = "@uuid")]
    pub uuid: String,

    #[serde(rename = "Properties")]
    pub properties: CommonModuleProperties,
}

/// CommonModule Properties
#[derive(Debug, Deserialize)]
pub(crate) struct CommonModuleProperties {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "Server", default)]
    pub server: BoolValue,

    #[serde(rename = "Global", default)]
    pub global: BoolValue,

    #[serde(rename = "ClientManagedApplication", default)]
    pub client_managed_application: BoolValue,

    #[serde(rename = "ClientOrdinaryApplication", default)]
    pub client_ordinary_application: BoolValue,

    #[serde(rename = "ExternalConnection", default)]
    pub external_connection: BoolValue,

    #[serde(rename = "ServerCall", default)]
    pub server_call: BoolValue,

    #[serde(rename = "Privileged", default)]
    pub privileged: BoolValue,

    #[serde(rename = "ReturnValuesReuse", default)]
    pub return_values_reuse: String,
}

// ============================================================================
// Register types
// ============================================================================

/// Root XML structure for Register (all 4 types)
#[derive(Debug, Deserialize)]
pub(crate) struct RegisterRoot {
    #[serde(
        alias = "InformationRegister",
        alias = "AccumulationRegister",
        alias = "AccountingRegister",
        alias = "CalculationRegister"
    )]
    pub register: RegisterXml,
}

/// Register XML structure (generic for all 4 types)
#[derive(Debug, Deserialize)]
pub(crate) struct RegisterXml {
    #[serde(rename = "@uuid")]
    pub uuid: String,

    #[serde(rename = "Properties")]
    pub properties: RegisterProperties,

    #[serde(rename = "ChildObjects", default)]
    pub child_objects: Option<RegisterChildObjects>,
}

/// Register Properties
#[derive(Debug, Deserialize)]
pub(crate) struct RegisterProperties {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "InformationRegisterPeriodicity", default)]
    pub periodicity: Option<String>,

    #[serde(rename = "EnableTotalsSliceFirst", default)]
    pub enable_totals_slice_first: BoolValue,

    #[serde(rename = "EnableTotalsSliceLast", default)]
    pub enable_totals_slice_last: BoolValue,

    #[serde(rename = "RegisterType", default)]
    pub register_type: Option<String>,
}

/// ChildObjects container for dimensions, resources, and attributes
#[derive(Debug, Deserialize)]
pub(crate) struct RegisterChildObjects {
    #[serde(rename = "Dimension", default)]
    pub dimensions: Vec<DimensionXml>,

    #[serde(rename = "Resource", default)]
    pub resources: Vec<ResourceXml>,

    #[serde(rename = "Attribute", default)]
    pub attributes: Vec<RegisterAttributeXml>,
}

/// Dimension XML structure
#[derive(Debug, Deserialize)]
pub(crate) struct DimensionXml {
    #[serde(rename = "@uuid")]
    pub uuid: String,

    #[serde(rename = "Properties")]
    pub properties: DimensionProperties,
}

/// Dimension Properties
#[derive(Debug, Deserialize)]
pub(crate) struct DimensionProperties {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "Type", default)]
    pub dim_type: Option<TypeXml>,

    #[serde(rename = "DenyIncompleteValues", default)]
    pub deny_incomplete_values: BoolValue,

    #[serde(rename = "Master", default)]
    pub master: BoolValue,

    #[serde(rename = "Indexing", default)]
    pub indexing: String,
}

/// Resource XML structure
#[derive(Debug, Deserialize)]
pub(crate) struct ResourceXml {
    #[serde(rename = "@uuid")]
    pub uuid: String,

    #[serde(rename = "Properties")]
    pub properties: ResourceProperties,
}

/// Resource Properties
#[derive(Debug, Deserialize)]
pub(crate) struct ResourceProperties {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "Type")]
    pub resource_type: TypeXml,
}

/// RegisterAttribute XML structure
#[derive(Debug, Deserialize)]
pub(crate) struct RegisterAttributeXml {
    #[serde(rename = "@uuid")]
    pub uuid: String,

    #[serde(rename = "Properties")]
    pub properties: RegisterAttributeProperties,
}

/// RegisterAttribute Properties
#[derive(Debug, Deserialize)]
pub(crate) struct RegisterAttributeProperties {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "Type")]
    pub attr_type: TypeXml,
}

// ============================================================================
// EventSubscription types
// ============================================================================

/// Root XML structure for EventSubscription
#[derive(Debug, Deserialize)]
pub(crate) struct EventSubscriptionRoot {
    #[serde(rename = "EventSubscription")]
    pub event_subscription: EventSubscriptionXml,
}

/// EventSubscription XML structure
#[derive(Debug, Deserialize)]
pub(crate) struct EventSubscriptionXml {
    #[serde(rename = "@uuid")]
    pub uuid: String,

    #[serde(rename = "Properties")]
    pub properties: EventSubscriptionProperties,
}

/// EventSubscription Properties
#[derive(Debug, Deserialize)]
pub(crate) struct EventSubscriptionProperties {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "Comment", default)]
    pub comment: Option<String>,

    #[serde(rename = "Source")]
    pub source: EventSource,

    #[serde(rename = "Event")]
    pub event: String,

    #[serde(rename = "Handler", default)]
    pub handler: String,
}

/// Event source - handles both v8:Type and v8:TypeSet variants
#[derive(Debug, Deserialize)]
pub(crate) struct EventSource {
    #[serde(rename = "Type", default)]
    pub types: Vec<String>,

    #[serde(rename = "TypeSet", default)]
    pub type_sets: Vec<String>,
}

impl EventSource {
    pub fn as_string(&self) -> String {
        let mut all_types: Vec<&str> = Vec::new();
        all_types.extend(self.types.iter().map(|s| s.as_str()));
        all_types.extend(self.type_sets.iter().map(|s| s.as_str()));
        all_types.join(";")
    }
}

// ============================================================================
// Catalog/Document/BusinessProcess types
// ============================================================================

/// Root XML structure for Catalog
#[derive(Debug, Deserialize)]
pub(crate) struct CatalogRoot {
    #[serde(rename = "Catalog")]
    pub catalog: MetadataObjectXml,
}

/// Root XML structure for Document
#[derive(Debug, Deserialize)]
pub(crate) struct DocumentRoot {
    #[serde(rename = "Document")]
    pub document: MetadataObjectXml,
}

/// Root XML structure for BusinessProcess
#[derive(Debug, Deserialize)]
pub(crate) struct BusinessProcessRoot {
    #[serde(rename = "BusinessProcess")]
    pub business_process: MetadataObjectXml,
}

/// Root XML structure for ChartOfCharacteristicTypes
#[derive(Debug, Deserialize)]
pub(crate) struct ChartOfCharacteristicTypesRoot {
    #[serde(rename = "ChartOfCharacteristicTypes")]
    pub chart_of_characteristic_types: MetadataObjectXml,
}

/// Root XML structure for Task
#[derive(Debug, Deserialize)]
pub(crate) struct TaskRoot {
    #[serde(rename = "Task")]
    pub task: MetadataObjectXml,
}

/// Root XML structure for ExchangePlan
#[derive(Debug, Deserialize)]
pub(crate) struct ExchangePlanRoot {
    #[serde(rename = "ExchangePlan")]
    pub exchange_plan: MetadataObjectXml,
}

/// Root XML structure for Enum
#[derive(Debug, Deserialize)]
pub(crate) struct EnumRoot {
    #[serde(rename = "Enum")]
    pub enum_xml: EnumXml,
}

/// Enum XML structure
#[derive(Debug, Deserialize)]
pub(crate) struct EnumXml {
    #[serde(rename = "@uuid")]
    pub _uuid: String,

    #[serde(rename = "Properties")]
    pub properties: EnumProperties,

    #[serde(rename = "ChildObjects", default)]
    pub child_objects: Option<EnumChildObjects>,
}

/// Enum properties
#[derive(Debug, Deserialize)]
pub(crate) struct EnumProperties {
    #[serde(rename = "Name")]
    pub name: String,
}

/// Child objects container for Enum (contains EnumValue elements)
#[derive(Debug, Deserialize)]
pub(crate) struct EnumChildObjects {
    #[serde(rename = "EnumValue", default)]
    pub enum_values: Vec<EnumValueXml>,
}

/// EnumValue XML structure
#[derive(Debug, Deserialize)]
pub(crate) struct EnumValueXml {
    #[serde(rename = "@uuid")]
    pub uuid: String,

    #[serde(rename = "Properties")]
    pub properties: EnumValueProperties,
}

/// EnumValue properties
#[derive(Debug, Deserialize)]
pub(crate) struct EnumValueProperties {
    #[serde(rename = "Name")]
    pub name: String,
}

/// Root XML structure for Constant
#[derive(Debug, Deserialize)]
pub(crate) struct ConstantRoot {
    #[serde(rename = "Constant")]
    pub constant: ConstantXml,
}

/// Constant XML structure
#[derive(Debug, Deserialize)]
pub(crate) struct ConstantXml {
    #[serde(rename = "@uuid")]
    pub _uuid: String,

    #[serde(rename = "Properties")]
    pub properties: ConstantProperties,
}

/// Constant properties
#[derive(Debug, Deserialize)]
pub(crate) struct ConstantProperties {
    #[serde(rename = "Name")]
    pub name: String,
}

/// Generic metadata object XML structure (Catalog, Document, etc.)
#[derive(Debug, Deserialize)]
pub(crate) struct MetadataObjectXml {
    #[serde(rename = "@uuid")]
    pub _uuid: String,

    #[serde(rename = "Properties")]
    pub properties: MetadataObjectProperties,

    #[serde(rename = "ChildObjects", default)]
    pub child_objects: Option<MetadataChildObjects>,
}

/// Metadata object properties
#[derive(Debug, Deserialize)]
pub(crate) struct MetadataObjectProperties {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "CodeLength", default)]
    pub code_length: Option<IntValue>,

    #[serde(rename = "DescriptionLength", default)]
    pub description_length: Option<IntValue>,

    #[serde(rename = "Hierarchical", default)]
    pub hierarchical: BoolValue,

    #[serde(rename = "Owners", default)]
    pub owners: Option<OwnersXml>,

    #[serde(rename = "InformationRegisterPeriodicity", default)]
    pub periodicity: Option<String>,
}

/// Owners XML structure
#[derive(Debug, Deserialize)]
pub(crate) struct OwnersXml {
    #[serde(rename = "Item", default)]
    pub items: Vec<OwnerItemXml>,
}

/// Owner item - reference to metadata object
#[derive(Debug, Deserialize)]
pub(crate) struct OwnerItemXml {
    #[serde(rename = "$text")]
    pub value: String,
}

/// Child objects container (attributes, tabular sections, etc.)
#[derive(Debug, Deserialize)]
pub(crate) struct MetadataChildObjects {
    #[serde(rename = "Attribute", default)]
    pub attributes: Vec<AttributeXml>,

    #[serde(rename = "Resource", default)]
    pub resources: Vec<AttributeXml>,

    #[serde(rename = "Dimension", default)]
    pub dimensions_as_attributes: Vec<AttributeXml>,

    #[serde(rename = "TabularSection", default)]
    pub tabular_sections: Vec<TabularSectionXml>,
}

/// Attribute XML structure
#[derive(Debug, Deserialize)]
pub(crate) struct AttributeXml {
    #[serde(rename = "@uuid")]
    pub _uuid: String,

    #[serde(rename = "Properties")]
    pub properties: AttributeProperties,
}

/// Attribute properties
#[derive(Debug, Deserialize)]
pub(crate) struct AttributeProperties {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "Type")]
    pub attr_type: TypeXml,
}

// ============================================================================
// Type parsing types
// ============================================================================

/// Type XML structure
///
/// Handles multiple type variants:
/// - `<v8:Type>xs:boolean</v8:Type>`
/// - `<v8:Type>xs:string</v8:Type><v8:StringQualifiers>...`
/// - `<v8:Type>cfg:CatalogRef.Name</v8:Type>`
/// - `<v8:TypeSet>cfg:DefinedType.Name</v8:TypeSet>`
#[derive(Debug, Deserialize, Default)]
pub(crate) struct TypeXml {
    #[serde(rename = "Type", default)]
    pub types: Vec<String>,

    #[serde(rename = "TypeSet", default)]
    pub type_sets: Vec<String>,

    #[serde(rename = "StringQualifiers", default)]
    pub string_qualifiers: Option<StringQualifiers>,

    #[serde(rename = "NumberQualifiers", default)]
    pub number_qualifiers: Option<NumberQualifiers>,

    #[serde(rename = "DateQualifiers", default)]
    pub date_qualifiers: Option<DateQualifiers>,
}

/// String qualifiers
#[derive(Debug, Deserialize)]
pub(crate) struct StringQualifiers {
    #[serde(rename = "Length", default)]
    pub length: Option<u32>,
}

/// Number qualifiers
#[derive(Debug, Deserialize)]
pub(crate) struct NumberQualifiers {
    #[serde(rename = "Digits", default)]
    pub digits: Option<u8>,

    #[serde(rename = "FractionDigits", default)]
    pub fraction_digits: Option<u8>,
}

/// Date qualifiers
#[derive(Debug, Deserialize)]
pub(crate) struct DateQualifiers {
    #[serde(rename = "DateFractions", default)]
    pub date_fractions: Option<String>,
}

// ============================================================================
// TabularSection types
// ============================================================================

/// Tabular Section XML structure
#[derive(Debug, Deserialize)]
pub(crate) struct TabularSectionXml {
    #[serde(rename = "@uuid")]
    pub uuid: String,

    #[serde(rename = "Properties")]
    pub properties: TabularSectionProperties,

    #[serde(rename = "ChildObjects", default)]
    pub child_objects: Option<TabularSectionChildObjects>,
}

/// Tabular Section properties
#[derive(Debug, Deserialize)]
pub(crate) struct TabularSectionProperties {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "Synonym", default)]
    pub synonym: Option<SynonymXml>,

    #[serde(rename = "Use", default)]
    pub use_mode: Option<String>,
}

/// Synonym XML structure
#[derive(Debug, Deserialize)]
pub(crate) struct SynonymXml {
    #[serde(rename = "$text", default)]
    pub value: Option<String>,
}

/// Child objects of a tabular section
#[derive(Debug, Deserialize)]
pub(crate) struct TabularSectionChildObjects {
    #[serde(rename = "Attribute", default)]
    pub attributes: Vec<AttributeXml>,
}

// ============================================================================
// DefinedType types
// ============================================================================

/// Root XML structure for DefinedType
#[derive(Debug, Deserialize)]
pub(crate) struct DefinedTypeRoot {
    #[serde(rename = "DefinedType")]
    pub defined_type: DefinedTypeXml,
}

/// DefinedType XML structure
#[derive(Debug, Deserialize)]
pub(crate) struct DefinedTypeXml {
    #[serde(rename = "@uuid")]
    pub uuid: String,

    #[serde(rename = "Properties")]
    pub properties: DefinedTypeProperties,
}

/// DefinedType properties
#[derive(Debug, Deserialize)]
pub(crate) struct DefinedTypeProperties {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "Type")]
    pub defined_type: TypeXml,
}
