//! Dimension metadata object for registers
//!
//! Represents a dimension (measurement) in 1C:Enterprise registers.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Dimension (measurement) in a register
///
/// Dimensions define the key fields by which register records are indexed and queried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    /// Unique identifier
    #[serde(rename = "uuid")]
    uuid: Uuid,

    /// Dimension name
    #[serde(rename = "name")]
    name: String,

    /// Deny incomplete values flag
    ///
    /// When true, prevents storing records with empty/incomplete dimension values.
    /// This is a critical data integrity setting.
    #[serde(rename = "denyIncompleteValues", default)]
    deny_incomplete_values: bool,

    /// Master dimension flag
    #[serde(rename = "master", default)]
    master: bool,

    /// Indexing mode
    #[serde(rename = "indexing", default)]
    indexing: String,
}

impl Dimension {
    /// Create new Dimension builder
    pub fn builder() -> DimensionBuilder {
        DimensionBuilder::default()
    }

    /// Get dimension UUID
    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    /// Get dimension name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Check if deny incomplete values flag is enabled
    pub fn is_deny_incomplete_values(&self) -> bool {
        self.deny_incomplete_values
    }

    /// Check if this is a master dimension
    pub fn is_master(&self) -> bool {
        self.master
    }

    /// Get indexing mode
    pub fn indexing(&self) -> &str {
        &self.indexing
    }
}

/// Builder for Dimension
#[derive(Debug, Default)]
pub struct DimensionBuilder {
    uuid: Option<Uuid>,
    name: Option<String>,
    deny_incomplete_values: bool,
    master: bool,
    indexing: String,
}

impl DimensionBuilder {
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

    /// Set deny incomplete values flag
    pub fn deny_incomplete_values(mut self, value: bool) -> Self {
        self.deny_incomplete_values = value;
        self
    }

    /// Set master dimension flag
    pub fn master(mut self, value: bool) -> Self {
        self.master = value;
        self
    }

    /// Set indexing mode
    pub fn indexing(mut self, indexing: impl Into<String>) -> Self {
        self.indexing = indexing.into();
        self
    }

    /// Build the Dimension
    pub fn build(self) -> Dimension {
        Dimension {
            uuid: self.uuid.unwrap_or_else(Uuid::new_v4),
            name: self.name.unwrap_or_default(),
            deny_incomplete_values: self.deny_incomplete_values,
            master: self.master,
            indexing: self.indexing,
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

        // UUIDs will be different but names and flags are same
        // PartialEq compares ALL fields including UUID
        assert_ne!(dim1, dim2); // Different UUIDs

        let uuid = Uuid::new_v4();
        let dim3 =
            Dimension::builder().uuid(uuid).name("Dim1").deny_incomplete_values(true).build();

        let dim4 =
            Dimension::builder().uuid(uuid).name("Dim1").deny_incomplete_values(true).build();

        assert_eq!(dim3, dim4); // Same UUID and fields
    }
}
