use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dimension {
    #[serde(rename = "uuid")]
    uuid: Uuid,

    #[serde(rename = "name")]
    name: String,

    #[serde(rename = "denyIncompleteValues", default)]
    deny_incomplete_values: bool,

    #[serde(rename = "master", default)]
    master: bool,

    #[serde(rename = "indexing", default)]
    indexing: String,

    #[serde(default, skip)]
    type_str: String,

    #[serde(skip)]
    attr_type: Option<crate::metadata_object::AttributeType>,
}

impl Dimension {
    pub fn builder() -> DimensionBuilder {
        DimensionBuilder::default()
    }

    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_deny_incomplete_values(&self) -> bool {
        self.deny_incomplete_values
    }

    pub fn is_master(&self) -> bool {
        self.master
    }

    pub fn indexing(&self) -> &str {
        &self.indexing
    }

    pub fn type_str(&self) -> &str {
        &self.type_str
    }

    pub fn set_type_str(&mut self, type_str: String) {
        self.type_str = type_str;
    }

    pub fn attr_type(&self) -> Option<&crate::metadata_object::AttributeType> {
        self.attr_type.as_ref()
    }

    pub fn set_attr_type(&mut self, attr_type: crate::metadata_object::AttributeType) {
        self.attr_type = Some(attr_type);
    }

    /// Heap bytes owned by this dimension: its name/indexing/type-text strings
    /// plus its resolved type's own owned payload.
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity()
            + self.indexing.capacity()
            + self.type_str.capacity()
            + self
                .attr_type
                .as_ref()
                .map_or(0, crate::metadata_object::AttributeType::estimated_heap_size)
    }
}

#[derive(Debug, Default)]
pub struct DimensionBuilder {
    uuid: Option<Uuid>,
    name: Option<String>,
    deny_incomplete_values: bool,
    master: bool,
    indexing: String,
}

impl DimensionBuilder {
    pub fn uuid(mut self, uuid: Uuid) -> Self {
        self.uuid = Some(uuid);
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn deny_incomplete_values(mut self, value: bool) -> Self {
        self.deny_incomplete_values = value;
        self
    }

    pub fn master(mut self, value: bool) -> Self {
        self.master = value;
        self
    }

    pub fn indexing(mut self, indexing: impl Into<String>) -> Self {
        self.indexing = indexing.into();
        self
    }

    pub fn build(self) -> Dimension {
        Dimension {
            uuid: self.uuid.unwrap_or_else(Uuid::new_v4),
            name: self.name.unwrap_or_default(),
            deny_incomplete_values: self.deny_incomplete_values,
            master: self.master,
            indexing: self.indexing,
            type_str: String::new(),
            attr_type: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dimension_builder() {
        let dimension = Dimension::builder()
            .name("Справочник1")
            .deny_incomplete_values(false)
            .master(true)
            .indexing("Index")
            .build();

        assert_eq!(dimension.name(), "Справочник1");
        assert!(!dimension.is_deny_incomplete_values());
        assert!(dimension.is_master());
        assert_eq!(dimension.indexing(), "Index");
    }

    #[test]
    fn test_dimension_default_values() {
        let dimension = Dimension::builder().name("Test").build();

        assert_eq!(dimension.name(), "Test");
        assert!(!dimension.is_deny_incomplete_values());
        assert!(!dimension.is_master());
        assert_eq!(dimension.indexing(), "");
    }

    #[test]
    fn test_dimension_partial_eq() {
        let dim1 = Dimension::builder().name("Dim1").deny_incomplete_values(true).build();

        let dim2 = Dimension::builder().name("Dim1").deny_incomplete_values(true).build();

        assert_ne!(dim1, dim2);

        let uuid = Uuid::new_v4();
        let dim3 =
            Dimension::builder().uuid(uuid).name("Dim1").deny_incomplete_values(true).build();

        let dim4 =
            Dimension::builder().uuid(uuid).name("Dim1").deny_incomplete_values(true).build();

        assert_eq!(dim3, dim4);
    }
}
