//! Role XML parser

use crate::error::{MetadataError, Result};
use crate::role::{Role, RoleData};

use super::helpers::{child_bool, child_text, find_child, find_mdo_element, parse_uuid, parse_xml};

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

    let doc = parse_xml(xml)?;
    let mdo = find_mdo_element(&doc)
        .ok_or_else(|| MetadataError::InvalidFormat("No Role element found".to_string()))?;

    let uuid_str = mdo.attribute("uuid").unwrap_or("");
    let uuid = parse_uuid(uuid_str, "role")?;

    let props = find_child(mdo, "Properties")
        .ok_or_else(|| MetadataError::InvalidFormat("Role missing Properties".to_string()))?;

    let name = child_text(props, "Name").unwrap_or("").to_string();

    let role = Role::with_data(uuid, name.clone(), RoleData::default());

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

    let doc = parse_xml(xml)?;
    // The root element IS the <Rights> element
    let rights_node = doc.root_element();

    let set_for_new_objects = child_bool(rights_node, "setForNewObjects");
    let set_for_attributes_by_default = child_bool(rights_node, "setForAttributesByDefault");
    let independent_rights_of_child_objects =
        child_bool(rights_node, "independentRightsOfChildObjects");

    let data = RoleData::new(
        set_for_new_objects,
        set_for_attributes_by_default,
        independent_rights_of_child_objects,
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
