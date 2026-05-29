use crate::metadata_object::AttributeType;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinedType {
    uuid: Uuid,
    name: String,
    underlying_type: AttributeType,
}

impl DefinedType {
    pub fn builder() -> DefinedTypeBuilder {
        DefinedTypeBuilder::default()
    }

    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn underlying_type(&self) -> &AttributeType {
        &self.underlying_type
    }
}

#[derive(Default)]
pub struct DefinedTypeBuilder {
    uuid: Option<Uuid>,
    name: Option<String>,
    underlying_type: Option<AttributeType>,
}

impl DefinedTypeBuilder {
    pub fn uuid(mut self, uuid: Uuid) -> Self {
        self.uuid = Some(uuid);
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn underlying_type(mut self, underlying_type: AttributeType) -> Self {
        self.underlying_type = Some(underlying_type);
        self
    }

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
