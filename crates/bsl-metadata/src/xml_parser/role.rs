//! Role XML parser

use crate::error::Result;
use crate::role::{Role, RoleData};

use super::helpers::parse_uuid;
use super::serde_types::{RightsXml, RoleRoot};

/// Parse Role XML from Designer format
///
/// # Arguments
///
/// * `xml` - XML content as string (from Roles/<Name>.xml)
///
/// # Returns
///
/// Parsed `Role` structure (without rights data)
pub fn parse_role_xml(xml: &str) -> Result<Role> {
    let _span = tracing::debug_span!("parse_role_xml").entered();

    let root: RoleRoot = quick_xml::de::from_str(xml)?;
    let uuid = parse_uuid(&root.role.uuid, "role")?;

    let role = Role::with_data(uuid, root.role.properties.name.clone(), RoleData::default());

    tracing::debug!(
        role_name = %role.name(),
        uuid = %role.uuid(),
        "parsed role"
    );

    Ok(role)
}

/// Parse Rights XML from Designer format
///
/// # Arguments
///
/// * `xml` - XML content as string (from Roles/<Name>/Ext/Rights.xml)
///
/// # Returns
///
/// Parsed `RoleData` structure
pub fn parse_rights_xml(xml: &str) -> Result<RoleData> {
    let _span = tracing::debug_span!("parse_rights_xml").entered();

    let rights: RightsXml = quick_xml::de::from_str(xml)?;

    let data = RoleData::new(
        rights.set_for_new_objects.into(),
        rights.set_for_attributes_by_default.into(),
        rights.independent_rights_of_child_objects.into(),
    );

    tracing::debug!(
        set_for_new_objects = data.set_for_new_objects(),
        set_for_attributes_by_default = data.set_for_attributes_by_default(),
        independent_rights_of_child_objects = data.independent_rights_of_child_objects(),
        "parsed rights"
    );

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_role_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Role uuid="463de255-ab61-47be-8ea6-d6611313830a">
        <Properties>
            <Name>ПолныеПрава</Name>
            <Synonym/>
            <Comment/>
        </Properties>
    </Role>
</MetaDataObject>"#;

        let role = parse_role_xml(xml).unwrap();

        assert_eq!(role.name(), "ПолныеПрава");
        assert_eq!(role.uuid().to_string(), "463de255-ab61-47be-8ea6-d6611313830a");
    }

    #[test]
    fn test_parse_rights_xml_all_true() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Rights xmlns="http://v8.1c.ru/8.2/roles" version="2.10">
    <setForNewObjects>true</setForNewObjects>
    <setForAttributesByDefault>true</setForAttributesByDefault>
    <independentRightsOfChildObjects>true</independentRightsOfChildObjects>
</Rights>"#;

        let data = parse_rights_xml(xml).unwrap();

        assert!(data.set_for_new_objects());
        assert!(data.set_for_attributes_by_default());
        assert!(data.independent_rights_of_child_objects());
    }

    #[test]
    fn test_parse_rights_xml_all_false() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Rights xmlns="http://v8.1c.ru/8.2/roles" version="2.10">
    <setForNewObjects>false</setForNewObjects>
    <setForAttributesByDefault>false</setForAttributesByDefault>
    <independentRightsOfChildObjects>false</independentRightsOfChildObjects>
</Rights>"#;

        let data = parse_rights_xml(xml).unwrap();

        assert!(!data.set_for_new_objects());
        assert!(!data.set_for_attributes_by_default());
        assert!(!data.independent_rights_of_child_objects());
    }

    #[test]
    fn test_parse_rights_xml_mixed() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Rights xmlns="http://v8.1c.ru/8.2/roles" version="2.10">
    <setForNewObjects>true</setForNewObjects>
    <setForAttributesByDefault>true</setForAttributesByDefault>
    <independentRightsOfChildObjects>false</independentRightsOfChildObjects>
</Rights>"#;

        let data = parse_rights_xml(xml).unwrap();

        assert!(data.set_for_new_objects());
        assert!(data.set_for_attributes_by_default());
        assert!(!data.independent_rights_of_child_objects());
    }
}
