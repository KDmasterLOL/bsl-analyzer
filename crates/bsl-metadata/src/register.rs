//! Register metadata objects (all 4 types)
//!
//! Supports InformationRegister, AccumulationRegister, AccountingRegister, CalculationRegister

use crate::dimension::Dimension;
use crate::enums::{ObjectBelonging, SupportVariant};
use crate::metadata_object::MdoType;
use crate::traits::MdObject;
use serde::{Deserialize, Serialize};
use std::any::Any;
use uuid::Uuid;

/// Resource (ресурс) in AccumulationRegister
///
/// Resources are the numeric values that are accumulated in the register.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterResource {
    /// UUID
    uuid: Uuid,

    /// Russian name
    name: String,

    /// English name (optional)
    #[serde(default)]
    name_en: Option<String>,

    /// Type as string (simplified for now)
    /// Example: "Number(15,2)", "String(100)"
    #[serde(default)]
    type_str: String,

    /// Parsed attribute type (for type inference in SDBL)
    #[serde(skip)]
    attr_type: Option<crate::metadata_object::AttributeType>,
}

impl RegisterResource {
    /// Create a new resource with the given UUID and name.
    pub fn new(uuid: Uuid, name: impl Into<String>) -> Self {
        Self { uuid, name: name.into(), name_en: None, type_str: String::new(), attr_type: None }
    }

    /// Get the UUID of the resource.
    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    /// Get the Russian name of the resource.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the English name of the resource.
    pub fn name_en(&self) -> Option<&str> {
        self.name_en.as_deref()
    }

    /// Get the type string of the resource.
    pub fn type_str(&self) -> &str {
        &self.type_str
    }

    /// Set the type string.
    pub fn set_type_str(&mut self, type_str: String) {
        self.type_str = type_str;
    }

    /// Get the parsed attribute type.
    pub fn attr_type(&self) -> Option<&crate::metadata_object::AttributeType> {
        self.attr_type.as_ref()
    }

    /// Set the attribute type.
    pub fn set_attr_type(&mut self, attr_type: crate::metadata_object::AttributeType) {
        self.attr_type = Some(attr_type);
    }
}

/// Attribute (реквизит) in InformationRegister
///
/// Attributes are additional data fields associated with register records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterAttribute {
    /// UUID
    uuid: Uuid,

    /// Russian name
    name: String,

    /// English name (optional)
    #[serde(default)]
    name_en: Option<String>,

    /// Type as string (simplified for now)
    /// Example: "String(100)", "CatalogRef.Валюты"
    #[serde(default)]
    type_str: String,

    /// Parsed attribute type (for type inference in SDBL)
    #[serde(skip)]
    attr_type: Option<crate::metadata_object::AttributeType>,
}

impl RegisterAttribute {
    /// Create a new attribute with the given UUID and name.
    pub fn new(uuid: Uuid, name: impl Into<String>) -> Self {
        Self { uuid, name: name.into(), name_en: None, type_str: String::new(), attr_type: None }
    }

    /// Get the UUID of the attribute.
    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    /// Get the Russian name of the attribute.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the English name of the attribute.
    pub fn name_en(&self) -> Option<&str> {
        self.name_en.as_deref()
    }

    /// Get the type string of the attribute.
    pub fn type_str(&self) -> &str {
        &self.type_str
    }

    /// Set the type string.
    pub fn set_type_str(&mut self, type_str: String) {
        self.type_str = type_str;
    }

    /// Get the parsed attribute type.
    pub fn attr_type(&self) -> Option<&crate::metadata_object::AttributeType> {
        self.attr_type.as_ref()
    }

    /// Set the attribute type.
    pub fn set_attr_type(&mut self, attr_type: crate::metadata_object::AttributeType) {
        self.attr_type = Some(attr_type);
    }
}

/// Periodicity of an InformationRegister
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegisterPeriodicity {
    /// Nonperiodical (Непериодический)
    Nonperiodical,
    /// Per second (В пределах секунды)
    Second,
    /// Per day (В пределах дня)
    Day,
    /// Per month (В пределах месяца)
    Month,
    /// By recorder position (По позиции регистратора)
    RecorderPosition,
}

/// Type of AccumulationRegister
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccumulationRegisterType {
    /// Balance (Остатки)
    Balance,
    /// Turnovers (Обороты)
    Turnovers,
    /// Balance and Turnovers (Остатки и обороты)
    BalanceAndTurnovers,
}

/// Register metadata object
///
/// Unified structure for all 4 register types:
/// - InformationRegister (Регистр сведений)
/// - AccumulationRegister (Регистр накопления)
/// - AccountingRegister (Регистр бухгалтерии)
/// - CalculationRegister (Регистр расчета)
///
/// All register types share the same dimension structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Register {
    /// Unique identifier
    #[serde(rename = "uuid")]
    uuid: Uuid,

    /// Register name
    #[serde(rename = "name")]
    name: String,

    /// Register type
    #[serde(rename = "mdoType")]
    mdo_type: MdoType,

    /// Dimensions (measurements)
    #[serde(rename = "dimensions", default)]
    dimensions: Vec<Dimension>,

    /// Resources (for AccumulationRegister)
    #[serde(rename = "resources", default)]
    resources: Vec<RegisterResource>,

    /// Attributes (for InformationRegister)
    #[serde(rename = "attributes", default)]
    attributes: Vec<RegisterAttribute>,

    /// Periodicity (for InformationRegister)
    #[serde(rename = "periodicity", default)]
    periodicity: Option<RegisterPeriodicity>,

    /// Register type (for AccumulationRegister)
    #[serde(rename = "registerType", default)]
    register_type: Option<AccumulationRegisterType>,

    /// Enable slice first (for InformationRegister)
    #[serde(rename = "enableTotalsSliceFirst", default)]
    enable_totals_slice_first: bool,

    /// Enable slice last (for InformationRegister)
    #[serde(rename = "enableTotalsSliceLast", default)]
    enable_totals_slice_last: bool,
}

impl Register {
    /// Create new Register builder
    pub fn builder() -> RegisterBuilder {
        RegisterBuilder::default()
    }

    /// Get register UUID
    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    /// Get register name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get register type
    pub fn mdo_type(&self) -> MdoType {
        self.mdo_type
    }

    /// Get dimensions
    pub fn dimensions(&self) -> &[Dimension] {
        &self.dimensions
    }

    /// Get resources
    pub fn resources(&self) -> &[RegisterResource] {
        &self.resources
    }

    /// Get attributes
    pub fn attributes(&self) -> &[RegisterAttribute] {
        &self.attributes
    }

    /// Check if this is an InformationRegister
    pub fn is_information_register(&self) -> bool {
        self.mdo_type == MdoType::InformationRegister
    }

    /// Check if this is an AccumulationRegister
    pub fn is_accumulation_register(&self) -> bool {
        self.mdo_type == MdoType::AccumulationRegister
    }

    /// Check if this is an AccountingRegister
    pub fn is_accounting_register(&self) -> bool {
        self.mdo_type == MdoType::AccountingRegister
    }

    /// Check if this is a CalculationRegister
    pub fn is_calculation_register(&self) -> bool {
        self.mdo_type == MdoType::CalculationRegister
    }

    /// Get periodicity (for InformationRegister)
    pub fn periodicity(&self) -> Option<RegisterPeriodicity> {
        self.periodicity
    }

    /// Get register type (for AccumulationRegister)
    pub fn register_type(&self) -> Option<AccumulationRegisterType> {
        self.register_type
    }

    /// Get enable totals slice first flag
    pub fn enable_totals_slice_first(&self) -> bool {
        self.enable_totals_slice_first
    }

    /// Get enable totals slice last flag
    pub fn enable_totals_slice_last(&self) -> bool {
        self.enable_totals_slice_last
    }

    /// Get virtual tables available for this register.
    ///
    /// Returns a list of virtual table names based on the register type and parameters.
    ///
    /// ## InformationRegister
    /// - If `periodicity` is not `Nonperiodical`: `СрезПервых` and `СрезПоследних`
    /// - Note: `enable_totals_slice_first/last` flags only control physical tables,
    ///   virtual tables are always available for periodic registers
    ///
    /// ## AccumulationRegister
    /// - If `register_type` is `Balance`: `Остатки`
    /// - If `register_type` is `Turnovers`: `Обороты`
    /// - If `register_type` is `BalanceAndTurnovers`: both `Остатки` and `Обороты`
    ///
    /// ## AccountingRegister and CalculationRegister
    /// - TODO: Complex virtual tables logic (deferred)
    pub fn virtual_tables(&self) -> Vec<&'static str> {
        match self.mdo_type {
            MdoType::InformationRegister => self.info_register_virtual_tables(),
            MdoType::AccumulationRegister => self.accum_register_virtual_tables(),
            MdoType::AccountingRegister => vec![],  // TODO
            MdoType::CalculationRegister => vec![], // TODO
            _ => vec![],
        }
    }

    /// Get virtual tables for InformationRegister.
    fn info_register_virtual_tables(&self) -> Vec<&'static str> {
        let mut tables = Vec::new();

        // Periodic registers always have slice virtual tables
        // (enable_totals_slice_first/last flags only control physical tables)
        if let Some(periodicity) = self.periodicity {
            if periodicity != RegisterPeriodicity::Nonperiodical {
                tables.push("СрезПервых");
                tables.push("СрезПоследних");
            }
        }

        tables
    }

    /// Get virtual tables for AccumulationRegister.
    fn accum_register_virtual_tables(&self) -> Vec<&'static str> {
        match self.register_type {
            Some(AccumulationRegisterType::Balance) => vec!["Остатки"],
            Some(AccumulationRegisterType::Turnovers) => vec!["Обороты"],
            Some(AccumulationRegisterType::BalanceAndTurnovers) => vec!["Остатки", "Обороты"],
            None => vec![],
        }
    }
}

impl MdObject for Register {
    fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn comment(&self) -> Option<&str> {
        None
    }

    fn object_belonging(&self) -> ObjectBelonging {
        ObjectBelonging::Own
    }

    fn support_variant(&self) -> SupportVariant {
        SupportVariant::Unknown
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Builder for Register
#[derive(Debug, Default)]
pub struct RegisterBuilder {
    uuid: Option<Uuid>,
    name: Option<String>,
    mdo_type: Option<MdoType>,
    dimensions: Vec<Dimension>,
    resources: Vec<RegisterResource>,
    attributes: Vec<RegisterAttribute>,
    periodicity: Option<RegisterPeriodicity>,
    register_type: Option<AccumulationRegisterType>,
    enable_totals_slice_first: bool,
    enable_totals_slice_last: bool,
}

impl RegisterBuilder {
    /// Set UUID
    pub fn uuid(mut self, uuid: Uuid) -> Self {
        self.uuid = Some(uuid);
        self
    }

    /// Set name
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set register type
    pub fn mdo_type(mut self, mdo_type: MdoType) -> Self {
        self.mdo_type = Some(mdo_type);
        self
    }

    /// Add dimension
    pub fn add_dimension(mut self, dimension: Dimension) -> Self {
        self.dimensions.push(dimension);
        self
    }

    /// Set all dimensions at once
    pub fn dimensions(mut self, dimensions: Vec<Dimension>) -> Self {
        self.dimensions = dimensions;
        self
    }

    /// Add resource (for AccumulationRegister)
    pub fn add_resource(mut self, resource: RegisterResource) -> Self {
        self.resources.push(resource);
        self
    }

    /// Set all resources at once (for AccumulationRegister)
    pub fn resources(mut self, resources: Vec<RegisterResource>) -> Self {
        self.resources = resources;
        self
    }

    /// Add attribute (for InformationRegister)
    pub fn add_attribute(mut self, attribute: RegisterAttribute) -> Self {
        self.attributes.push(attribute);
        self
    }

    /// Set all attributes at once (for InformationRegister)
    pub fn attributes(mut self, attributes: Vec<RegisterAttribute>) -> Self {
        self.attributes = attributes;
        self
    }

    /// Set periodicity (for InformationRegister)
    pub fn periodicity(mut self, periodicity: Option<RegisterPeriodicity>) -> Self {
        self.periodicity = periodicity;
        self
    }

    /// Set register type (for AccumulationRegister)
    pub fn register_type(mut self, register_type: Option<AccumulationRegisterType>) -> Self {
        self.register_type = register_type;
        self
    }

    /// Set enable totals slice first flag
    pub fn enable_totals_slice_first(mut self, value: bool) -> Self {
        self.enable_totals_slice_first = value;
        self
    }

    /// Set enable totals slice last flag
    pub fn enable_totals_slice_last(mut self, value: bool) -> Self {
        self.enable_totals_slice_last = value;
        self
    }

    /// Build the Register
    pub fn build(self) -> Register {
        Register {
            uuid: self.uuid.unwrap_or_else(Uuid::new_v4),
            name: self.name.unwrap_or_default(),
            mdo_type: self.mdo_type.unwrap_or(MdoType::InformationRegister),
            dimensions: self.dimensions,
            resources: self.resources,
            attributes: self.attributes,
            periodicity: self.periodicity,
            register_type: self.register_type,
            enable_totals_slice_first: self.enable_totals_slice_first,
            enable_totals_slice_last: self.enable_totals_slice_last,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dimension::Dimension;

    #[test]
    fn test_register_builder_information() {
        let register = Register::builder()
            .name("РегистрСведений1")
            .mdo_type(MdoType::InformationRegister)
            .build();

        assert_eq!(register.name(), "РегистрСведений1");
        assert!(register.is_information_register());
        assert!(!register.is_accumulation_register());
        assert_eq!(register.dimensions().len(), 0);
    }

    #[test]
    fn test_register_with_dimensions() {
        let dim1 = Dimension::builder().name("Справочник1").deny_incomplete_values(false).build();

        let dim2 = Dimension::builder().name("Справочник2").deny_incomplete_values(true).build();

        let register = Register::builder()
            .name("TestRegister")
            .mdo_type(MdoType::AccumulationRegister)
            .add_dimension(dim1)
            .add_dimension(dim2)
            .build();

        assert_eq!(register.name(), "TestRegister");
        assert!(register.is_accumulation_register());
        assert_eq!(register.dimensions().len(), 2);
        assert_eq!(register.dimensions()[0].name(), "Справочник1");
        assert!(!register.dimensions()[0].is_deny_incomplete_values());
        assert_eq!(register.dimensions()[1].name(), "Справочник2");
        assert!(register.dimensions()[1].is_deny_incomplete_values());
    }

    #[test]
    fn test_register_types() {
        let info_reg = Register::builder().mdo_type(MdoType::InformationRegister).build();
        assert!(info_reg.is_information_register());

        let accum_reg = Register::builder().mdo_type(MdoType::AccumulationRegister).build();
        assert!(accum_reg.is_accumulation_register());

        let accounting_reg = Register::builder().mdo_type(MdoType::AccountingRegister).build();
        assert!(accounting_reg.is_accounting_register());

        let calc_reg = Register::builder().mdo_type(MdoType::CalculationRegister).build();
        assert!(calc_reg.is_calculation_register());
    }

    #[test]
    fn test_register_partial_eq() {
        let uuid = Uuid::new_v4();

        let reg1 = Register::builder()
            .uuid(uuid)
            .name("Test")
            .mdo_type(MdoType::InformationRegister)
            .build();

        let reg2 = Register::builder()
            .uuid(uuid)
            .name("Test")
            .mdo_type(MdoType::InformationRegister)
            .build();

        assert_eq!(reg1, reg2);
    }

    #[test]
    fn test_register_mdo_object_trait() {
        let register =
            Register::builder().name("TestRegister").mdo_type(MdoType::InformationRegister).build();

        let mdo: &dyn MdObject = &register;
        assert_eq!(mdo.name(), "TestRegister");

        // Can downcast to concrete Register type
        let concrete = mdo.as_any().downcast_ref::<Register>();
        assert!(concrete.is_some());
        assert_eq!(concrete.unwrap().mdo_type(), MdoType::InformationRegister);
    }
}
