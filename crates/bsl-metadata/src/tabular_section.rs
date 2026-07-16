use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabularSection {
    uuid: Uuid,

    name: String,

    #[serde(default)]
    name_en: Option<String>,

    #[serde(default)]
    synonym: Option<String>,

    #[serde(default)]
    attributes: Vec<TabularSectionAttribute>,

    #[serde(default)]
    use_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabularSectionAttribute {
    uuid: Uuid,
    name: String,
    #[serde(default)]
    name_en: Option<String>,
    attr_type: crate::AttributeType,
}

impl TabularSection {
    pub fn new(uuid: Uuid, name: impl Into<String>) -> Self {
        Self {
            uuid,
            name: name.into(),
            name_en: None,
            synonym: None,
            attributes: Vec::new(),
            use_mode: None,
        }
    }

    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn name_en(&self) -> Option<&str> {
        self.name_en.as_deref()
    }

    pub fn synonym(&self) -> Option<&str> {
        self.synonym.as_deref()
    }

    pub fn attributes(&self) -> &[TabularSectionAttribute] {
        &self.attributes
    }

    pub fn use_mode(&self) -> Option<&str> {
        self.use_mode.as_deref()
    }

    pub fn set_name_en(&mut self, name_en: Option<String>) {
        self.name_en = name_en;
    }

    pub fn set_synonym(&mut self, synonym: Option<String>) {
        self.synonym = synonym;
    }

    pub fn set_attributes(&mut self, attributes: Vec<TabularSectionAttribute>) {
        self.attributes = attributes;
    }

    pub fn set_use_mode(&mut self, use_mode: Option<String>) {
        self.use_mode = use_mode;
    }

    /// Heap bytes owned by this tabular section: its name strings plus the
    /// backing attribute vec and each attribute's own owned payload.
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity()
            + self.name_en.as_ref().map_or(0, String::capacity)
            + self.synonym.as_ref().map_or(0, String::capacity)
            + self.use_mode.as_ref().map_or(0, String::capacity)
            + stdx::heap::vec_bytes::<TabularSectionAttribute>(self.attributes.len())
            + self
                .attributes
                .iter()
                .map(TabularSectionAttribute::estimated_heap_size)
                .sum::<usize>()
    }
}

impl TabularSectionAttribute {
    pub fn new(uuid: Uuid, name: impl Into<String>, attr_type: crate::AttributeType) -> Self {
        Self { uuid, name: name.into(), name_en: None, attr_type }
    }

    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn name_en(&self) -> Option<&str> {
        self.name_en.as_deref()
    }

    pub fn attr_type(&self) -> &crate::AttributeType {
        &self.attr_type
    }

    pub fn set_name_en(&mut self, name_en: Option<String>) {
        self.name_en = name_en;
    }

    /// Heap bytes owned by this attribute: its name strings plus its type's own
    /// owned payload.
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity()
            + self.name_en.as_ref().map_or(0, String::capacity)
            + self.attr_type.estimated_heap_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tabular_section_creation() {
        let uuid = Uuid::new_v4();
        let ts = TabularSection::new(uuid, "Штрихкоды");

        assert_eq!(ts.name(), "Штрихкоды");
        assert_eq!(ts.uuid(), &uuid);
        assert_eq!(ts.name_en(), None);
        assert_eq!(ts.synonym(), None);
        assert_eq!(ts.attributes().len(), 0);
        assert_eq!(ts.use_mode(), None);
    }

    #[test]
    fn test_tabular_section_with_attributes() {
        let uuid = Uuid::new_v4();
        let mut ts = TabularSection::new(uuid, "Штрихкоды");

        let attr_uuid = Uuid::new_v4();
        let attr_type = crate::AttributeType::String { length: Some(13) };
        let attr = TabularSectionAttribute::new(attr_uuid, "Штрихкод", attr_type.clone());

        ts.set_attributes(vec![attr]);

        assert_eq!(ts.attributes().len(), 1);
        assert_eq!(ts.attributes()[0].name(), "Штрихкод");
        assert_eq!(ts.attributes()[0].attr_type(), &attr_type);
    }

    #[test]
    fn test_tabular_section_with_synonym() {
        let uuid = Uuid::new_v4();
        let mut ts = TabularSection::new(uuid, "Штрихкоды");

        ts.set_synonym(Some("Коды товара".to_string()));

        assert_eq!(ts.synonym(), Some("Коды товара"));
    }
}
