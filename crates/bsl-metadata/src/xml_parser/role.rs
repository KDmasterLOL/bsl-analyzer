use std::str::FromStr;

use crate::error::{MetadataError, Result};
use crate::metadata_object::MdoType;
use crate::role::{Role, RoleData, RoleObjectRef};

use super::helpers::{child_bool, child_text, find_child, find_mdo_element, parse_uuid, parse_xml};

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

pub fn parse_rights_xml(xml: &str) -> Result<RoleData> {
    let _span = tracing::debug_span!("parse_rights_xml").entered();

    let doc = parse_xml(xml)?;
    let rights_node = doc.root_element();
    // A `Rights.xml` always has a `<Rights>` root; reject anything else so a misrouted file
    // never silently parses as rights (mirrors the subsystem parser's root-tag check).
    if rights_node.tag_name().name() != "Rights" {
        return Err(MetadataError::InvalidFormat(format!(
            "expected a Rights element, found {}",
            rights_node.tag_name().name()
        )));
    }

    let set_for_new_objects = child_bool(rights_node, "setForNewObjects");
    let set_for_attributes_by_default = child_bool(rights_node, "setForAttributesByDefault");
    let independent_rights_of_child_objects =
        child_bool(rights_node, "independentRightsOfChildObjects");

    let objects = parse_objects(rights_node);

    let data = RoleData::new(
        set_for_new_objects,
        set_for_attributes_by_default,
        independent_rights_of_child_objects,
    )
    .with_objects(objects);

    tracing::debug!(
        set_for_new_objects = data.set_for_new_objects(),
        set_for_attributes_by_default = data.set_for_attributes_by_default(),
        independent_rights_of_child_objects = data.independent_rights_of_child_objects(),
        object_count = data.objects().len(),
        "parsed rights"
    );

    Ok(data)
}

/// Parse the `<object>` rights entries: each `<name>` is a `Type.Object[.Attribute.X]`
/// MDObjectRef. The first segment is the English MDObjectRef type, the second the object
/// name; any attribute/tabular-section tail is dropped (the reference is at object grain).
/// The `Configuration.*` session/admin pseudo-object has no `MdoType` and is skipped, as is
/// any unknown-type or malformed ref. RLS `restrictionByCondition` condition texts on the
/// object's rights are gathered onto the object.
fn parse_objects<'a>(rights_node: roxmltree::Node<'a, 'a>) -> Vec<RoleObjectRef> {
    let mut objects = Vec::new();
    for obj in rights_node.children().filter(|n| n.is_element() && n.tag_name().name() == "object")
    {
        let Some(raw) = child_text(obj, "name") else { continue };
        let Some((mdo_type, name)) = parse_object_ref(raw.trim()) else { continue };

        let mut restrictions = Vec::new();
        for right in obj.children().filter(|n| n.is_element() && n.tag_name().name() == "right") {
            for rbc in right
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "restrictionByCondition")
            {
                if let Some(cond) = child_text(rbc, "condition") {
                    let cond = cond.trim();
                    if !cond.is_empty() {
                        restrictions.push(cond.to_string());
                    }
                }
            }
        }
        objects.push(RoleObjectRef { mdo_type, name, restrictions });
    }
    objects
}

/// Parse a `Type.Object[.Attribute.X]` MDObjectRef into `(MdoType, object_name)`. Unknown
/// type prefixes (e.g. the `Configuration` session pseudo-object) and refs without an object
/// segment are skipped so an unparseable entry never invents an edge.
fn parse_object_ref(raw: &str) -> Option<(MdoType, String)> {
    let mut segs = raw.split('.');
    let prefix = segs.next()?;
    let object = segs.next()?;
    if object.is_empty() {
        return None;
    }
    match MdoType::from_str(prefix) {
        Ok(mdo_type) => Some((mdo_type, object.to_string())),
        Err(_) => {
            tracing::debug!(raw, prefix, "role object ref has unknown type; skipped");
            None
        }
    }
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
    fn parses_object_rights_with_rls_and_skips_pseudo_and_unknown() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Rights xmlns="http://v8.1c.ru/8.2/roles" version="2.10">
    <setForNewObjects>false</setForNewObjects>
    <object>
        <name>Configuration.Конфигурация</name>
        <right><name>Administration</name><value>true</value></right>
    </object>
    <object>
        <name>Catalog.Контрагенты</name>
        <right>
            <name>Read</name>
            <value>true</value>
            <restrictionByCondition>
                <condition>Контрагенты.Организация В (ВЫБРАТЬ Ссылка ИЗ Справочник.Организации)</condition>
            </restrictionByCondition>
        </right>
    </object>
    <object>
        <name>Catalog.Контрагенты.Attribute.ИНН</name>
        <right><name>View</name><value>true</value></right>
    </object>
    <object>
        <name>Bogus.Unknown</name>
        <right><name>Read</name><value>true</value></right>
    </object>
</Rights>"#;

        let data = parse_rights_xml(xml).unwrap();
        let objects = data.objects();
        // Configuration.* pseudo-object and the unknown `Bogus` type are skipped; the
        // attribute-tailed ref collapses to its object grain (a second Контрагенты entry).
        assert_eq!(objects.len(), 2, "Configuration.* and unknown type are skipped");
        assert_eq!(objects[0].mdo_type, MdoType::Catalog);
        assert_eq!(objects[0].name, "Контрагенты");
        assert_eq!(
            objects[0].restrictions,
            vec!["Контрагенты.Организация В (ВЫБРАТЬ Ссылка ИЗ Справочник.Организации)".to_string()],
            "the RLS restrictionByCondition text is captured"
        );
        assert_eq!(
            objects[1].name, "Контрагенты",
            "attribute-tailed ref collapses to object grain"
        );
        assert!(objects[1].restrictions.is_empty());
    }

    #[test]
    fn rejects_a_non_rights_root() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject>
    <Role uuid="463de255-ab61-47be-8ea6-d6611313830a">
        <Properties><Name>Роль</Name></Properties>
    </Role>
</MetaDataObject>"#;
        assert!(parse_rights_xml(xml).is_err(), "a non-Rights root must be rejected");
    }

    #[test]
    fn rights_without_objects_yield_empty_list() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Rights xmlns="http://v8.1c.ru/8.2/roles" version="2.10">
    <setForNewObjects>true</setForNewObjects>
</Rights>"#;
        assert!(parse_rights_xml(xml).unwrap().objects().is_empty());
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
