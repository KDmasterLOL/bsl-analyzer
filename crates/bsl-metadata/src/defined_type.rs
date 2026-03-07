//! DefinedType metadata object

use crate::metadata_object::AttributeType;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// DefinedType metadata object (ОпределяемыйТип)
///
/// Represents a user-defined type that can hold multiple primitive/reference types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinedType {
    /// Unique identifier
    uuid: Uuid,

    /// Type name
    name: String,

    /// Underlying type
    underlying_type: AttributeType,
}

impl DefinedType {
    /// Create new DefinedType builder
    pub fn builder() -> DefinedTypeBuilder {
        DefinedTypeBuilder::default()
    }

    /// Get UUID
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Get name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get underlying type
    pub fn underlying_type(&self) -> &AttributeType {
        &self.underlying_type
    }
}

/// Builder for DefinedType
#[derive(Default)]
pub struct DefinedTypeBuilder {
    uuid: Option<Uuid>,
    name: Option<String>,
    underlying_type: Option<AttributeType>,
}

impl DefinedTypeBuilder {
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

    /// Set underlying type
    pub fn underlying_type(mut self, underlying_type: AttributeType) -> Self {
        self.underlying_type = Some(underlying_type);
        self
    }

    /// Build DefinedType
    pub fn build(self) -> DefinedType {
        DefinedType {
            uuid: self.uuid.expect("uuid is required"),
            name: self.name.expect("name is required"),
            underlying_type: self.underlying_type.expect("underlying_type is required"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata_object::AttributeType;

    #[test]
    fn test_defined_type_creation() {
        let uuid = Uuid::new_v4();
        let defined_type = DefinedType::builder()
            .uuid(uuid)
            .name("ОтметкаВремени")
            .underlying_type(AttributeType::String { length: Some(17) })
            .build();

        assert_eq!(defined_type.uuid(), uuid);
        assert_eq!(defined_type.name(), "ОтметкаВремени");
        assert_eq!(defined_type.underlying_type(), &AttributeType::String { length: Some(17) });
    }
}
