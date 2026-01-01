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

    /// Build the Register
    pub fn build(self) -> Register {
        Register {
            uuid: self.uuid.unwrap_or_else(Uuid::new_v4),
            name: self.name.unwrap_or_default(),
            mdo_type: self.mdo_type.unwrap_or(MdoType::InformationRegister),
            dimensions: self.dimensions,
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
