//! Role metadata object
//!
//! Represents 1C:Enterprise Role metadata.
//! Roles define user permissions for configuration objects.
//!
//! ## Structure
//!
//! - Name: Unique role name
//! - RoleData: Rights settings loaded from Ext/Rights.xml
//!
//! ## Note
//!
//! Roles have NO code files - only XML metadata.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Role metadata object
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Role {
    /// UUID
    #[serde(rename = "uuid")]
    pub(crate) uuid: Uuid,

    /// Role name
    #[serde(rename = "name")]
    pub(crate) name: String,

    /// Role rights data (from Ext/Rights.xml)
    #[serde(rename = "data", default)]
    pub(crate) data: RoleData,
}

/// Role rights data
///
/// Contains settings from Ext/Rights.xml file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RoleData {
    /// Set permissions for new objects flag
    ///
    /// If true, the role automatically gets permissions for newly created metadata objects.
    /// This is a security concern for non-admin roles.
    #[serde(rename = "setForNewObjects", default)]
    set_for_new_objects: bool,

    /// Set permissions for attributes by default flag
    #[serde(rename = "setForAttributesByDefault", default)]
    set_for_attributes_by_default: bool,

    /// Independent rights of child objects flag
    #[serde(rename = "independentRightsOfChildObjects", default)]
    independent_rights_of_child_objects: bool,
}

impl Role {
    /// Create new Role
    #[cfg(test)]
    pub fn new(name: impl Into<String>) -> Self {
        Self { uuid: Uuid::new_v4(), name: name.into(), data: RoleData::default() }
    }

    /// Create Role with specific settings
    pub(crate) fn with_data(uuid: Uuid, name: String, data: RoleData) -> Self {
        Self { uuid, name, data }
    }

    /// Get role name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get role UUID
    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    /// Get role data
    pub fn data(&self) -> &RoleData {
        &self.data
    }
}

impl RoleData {
    /// Create new RoleData
    pub(crate) fn new(
        set_for_new_objects: bool,
        set_for_attributes_by_default: bool,
        independent_rights_of_child_objects: bool,
    ) -> Self {
        Self {
            set_for_new_objects,
            set_for_attributes_by_default,
            independent_rights_of_child_objects,
        }
    }

    /// Check if role has "set for new objects" flag enabled
    ///
    /// This flag means the role automatically gets permissions for newly created
    /// metadata objects. This is a security vulnerability for non-admin roles.
    pub fn set_for_new_objects(&self) -> bool {
        self.set_for_new_objects
    }

    /// Check if role has "set for attributes by default" flag enabled
    pub fn set_for_attributes_by_default(&self) -> bool {
        self.set_for_attributes_by_default
    }

    /// Check if role has "independent rights of child objects" flag enabled
    pub fn independent_rights_of_child_objects(&self) -> bool {
        self.independent_rights_of_child_objects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_accessors() {
        let role = Role::new("ПолныеПрава");
        assert_eq!(role.name(), "ПолныеПрава");
        assert!(!role.data().set_for_new_objects());
    }

    #[test]
    fn test_role_with_data() {
        let data = RoleData::new(true, true, false);
        let role = Role::with_data(Uuid::new_v4(), "TestRole".to_string(), data);

        assert_eq!(role.name(), "TestRole");
        assert!(role.data().set_for_new_objects());
        assert!(role.data().set_for_attributes_by_default());
        assert!(!role.data().independent_rights_of_child_objects());
    }

    #[test]
    fn test_role_data_default() {
        let data = RoleData::default();
        assert!(!data.set_for_new_objects());
        assert!(!data.set_for_attributes_by_default());
        assert!(!data.independent_rights_of_child_objects());
    }
}
