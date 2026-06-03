use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Role {
    #[serde(rename = "uuid")]
    pub(crate) uuid: Uuid,

    #[serde(rename = "name")]
    pub(crate) name: String,

    #[serde(rename = "data", default)]
    pub(crate) data: RoleData,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RoleData {
    #[serde(rename = "setForNewObjects", default)]
    set_for_new_objects: bool,

    #[serde(rename = "setForAttributesByDefault", default)]
    set_for_attributes_by_default: bool,

    #[serde(rename = "independentRightsOfChildObjects", default)]
    independent_rights_of_child_objects: bool,
}

impl Role {
    #[cfg(test)]
    pub fn new(name: impl Into<String>) -> Self {
        Self { uuid: Uuid::new_v4(), name: name.into(), data: RoleData::default() }
    }

    pub(crate) fn with_data(uuid: Uuid, name: String, data: RoleData) -> Self {
        Self { uuid, name, data }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    pub fn data(&self) -> &RoleData {
        &self.data
    }
}

impl RoleData {
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

    pub fn set_for_new_objects(&self) -> bool {
        self.set_for_new_objects
    }

    pub fn set_for_attributes_by_default(&self) -> bool {
        self.set_for_attributes_by_default
    }

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
