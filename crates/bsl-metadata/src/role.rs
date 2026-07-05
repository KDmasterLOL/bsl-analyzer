use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::metadata_object::MdoType;

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

    /// Metadata objects the role grants rights on, parsed from `Rights.xml`. Lets the call
    /// graph carry role → object reference edges for impact analysis ("which roles grant
    /// rights on this object"). Excludes the `Configuration.*` session/admin pseudo-object,
    /// which is not a real metadata object.
    #[serde(rename = "objects", default)]
    objects: Vec<RoleObjectRef>,
}

/// One metadata object referenced by a role's rights, plus the RLS restriction condition
/// texts attached to that object's rights (if any). The condition text is row-level-security
/// query text that may name further objects; it is resolved lazily by the graph build, not
/// here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleObjectRef {
    pub mdo_type: MdoType,
    pub name: String,
    /// RLS `restrictionByCondition` condition texts on this object's rights.
    #[serde(default)]
    pub restrictions: Vec<String>,
}

impl Role {
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

    /// Metadata objects this role grants rights on (excludes the `Configuration.*` pseudo-object).
    pub fn objects(&self) -> &[RoleObjectRef] {
        &self.data.objects
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
            objects: Vec::new(),
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

    pub fn objects(&self) -> &[RoleObjectRef] {
        &self.objects
    }

    pub fn with_objects(mut self, objects: Vec<RoleObjectRef>) -> Self {
        self.objects = objects;
        self
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
