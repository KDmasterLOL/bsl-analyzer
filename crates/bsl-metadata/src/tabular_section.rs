//! Tabular sections for metadata objects.
//!
//! Tabular sections (табличные части) are child collections of metadata objects.
//! They represent one-to-many relationships, similar to foreign key tables.
//!
//! ## Example
//!
//! Catalog "Номенклатура" may have tabular sections:
//! - "Штрихкоды" (barcodes)
//! - "Характеристики" (characteristics)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Tabular section (табличная часть) of a metadata object.
///
/// Represents a child collection of a metadata object, containing multiple attributes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabularSection {
    /// UUID
    uuid: Uuid,

    /// Russian name (e.g., "Штрихкоды")
    name: String,

    /// English name (optional)
    #[serde(default)]
    name_en: Option<String>,

    /// Synonym for display
    #[serde(default)]
    synonym: Option<String>,

    /// Attributes of the tabular section (columns)
    #[serde(default)]
    attributes: Vec<TabularSectionAttribute>,

    /// Use mode (for Catalog/ChartOfCharacteristicTypes)
    /// Values: ForItem, ForFolder, DontUse
    #[serde(default)]
    use_mode: Option<String>,
}

/// Attribute (column) in a tabular section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabularSectionAttribute {
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
}

impl TabularSection {
    /// Create a new tabular section with the given UUID and name.
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

    /// Get the UUID of the tabular section.
    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    /// Get the Russian name of the tabular section.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the English name of the tabular section.
    pub fn name_en(&self) -> Option<&str> {
        self.name_en.as_deref()
    }

    /// Get the synonym for display.
    pub fn synonym(&self) -> Option<&str> {
        self.synonym.as_deref()
    }

    /// Get the attributes (columns) of the tabular section.
    pub fn attributes(&self) -> &[TabularSectionAttribute] {
        &self.attributes
    }

    /// Get the use mode.
    pub fn use_mode(&self) -> Option<&str> {
        self.use_mode.as_deref()
    }

    /// Set the English name.
    pub fn set_name_en(&mut self, name_en: Option<String>) {
        self.name_en = name_en;
    }

    /// Set the synonym.
    pub fn set_synonym(&mut self, synonym: Option<String>) {
        self.synonym = synonym;
    }

    /// Set the attributes.
    pub fn set_attributes(&mut self, attributes: Vec<TabularSectionAttribute>) {
        self.attributes = attributes;
    }

    /// Set the use mode.
    pub fn set_use_mode(&mut self, use_mode: Option<String>) {
        self.use_mode = use_mode;
    }
}

impl TabularSectionAttribute {
    /// Create a new tabular section attribute.
    pub fn new(uuid: Uuid, name: impl Into<String>, type_str: impl Into<String>) -> Self {
        Self { uuid, name: name.into(), name_en: None, type_str: type_str.into() }
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

    /// Set the English name.
    pub fn set_name_en(&mut self, name_en: Option<String>) {
        self.name_en = name_en;
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
        let attr = TabularSectionAttribute::new(attr_uuid, "Штрихкод", "String(13)");

        ts.set_attributes(vec![attr]);

        assert_eq!(ts.attributes().len(), 1);
        assert_eq!(ts.attributes()[0].name(), "Штрихкод");
        assert_eq!(ts.attributes()[0].type_str(), "String(13)");
    }

    #[test]
    fn test_tabular_section_with_synonym() {
        let uuid = Uuid::new_v4();
        let mut ts = TabularSection::new(uuid, "Штрихкоды");

        ts.set_synonym(Some("Коды товара".to_string()));

        assert_eq!(ts.synonym(), Some("Коды товара"));
    }
}
