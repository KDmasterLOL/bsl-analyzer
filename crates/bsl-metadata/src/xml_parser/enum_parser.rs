//! Enum XML parser

use crate::error::Result;
use crate::metadata_object::{EnumValue, MdoType, MetadataObject};

use super::serde_types::EnumRoot;

/// Parse Enum XML from Designer format
///
/// # Arguments
///
/// * `xml` - XML content as string
///
/// # Returns
///
/// Parsed `MetadataObject` structure with enum_values populated
///
/// # Example
///
/// ```no_run
/// # use bsl_metadata::xml_parser::parse_enum_xml;
/// let xml = std::fs::read_to_string("Enums/Статусы.xml")?;
/// let enum_obj = parse_enum_xml(&xml)?;
/// # Ok::<(), bsl_metadata::MetadataError>(())
/// ```
pub fn parse_enum_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_enum_xml").entered();

    let root: EnumRoot = quick_xml::de::from_str(xml)?;
    let enum_xml = root.enum_xml;

    let mut enum_values = Vec::new();

    // Parse EnumValue elements if present
    if let Some(child_objects) = enum_xml.child_objects {
        for ev_xml in child_objects.enum_values {
            enum_values.push(EnumValue {
                name: ev_xml.properties.name,
                name_en: None, // MVP: only Russian names
                uuid: ev_xml.uuid,
            });
        }
    }

    let mut mdo = MetadataObject::new(MdoType::Enum, enum_xml.properties.name);
    mdo.enum_values = enum_values;

    tracing::debug!(
        enum_name = %mdo.name,
        enum_values_count = mdo.enum_values.len(),
        "parsed enum"
    );

    Ok(mdo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_enum_xml_with_values() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.20">
    <Enum uuid="379167c7-29f4-479f-8803-914fd95e350f">
        <Properties>
            <Name>Статусы</Name>
        </Properties>
        <ChildObjects>
            <EnumValue uuid="f32053d4-6092-498a-9107-b042289e65ae">
                <Properties>
                    <Name>Активный</Name>
                </Properties>
            </EnumValue>
            <EnumValue uuid="7af3bc2d-f14b-4e93-a8db-0d3fece96257">
                <Properties>
                    <Name>Неактивный</Name>
                </Properties>
            </EnumValue>
            <EnumValue uuid="ff25e312-3fc6-49f2-8382-e5ed6f9746bb">
                <Properties>
                    <Name>Завершенный</Name>
                </Properties>
            </EnumValue>
        </ChildObjects>
    </Enum>
</MetaDataObject>"#;

        let enum_obj = parse_enum_xml(xml).unwrap();

        assert_eq!(enum_obj.name, "Статусы");
        assert_eq!(enum_obj.mdo_type, MdoType::Enum);
        assert_eq!(enum_obj.enum_values.len(), 3);

        // Check enum values
        assert_eq!(enum_obj.enum_values[0].name, "Активный");
        assert_eq!(enum_obj.enum_values[0].uuid, "f32053d4-6092-498a-9107-b042289e65ae");

        assert_eq!(enum_obj.enum_values[1].name, "Неактивный");
        assert_eq!(enum_obj.enum_values[1].uuid, "7af3bc2d-f14b-4e93-a8db-0d3fece96257");

        assert_eq!(enum_obj.enum_values[2].name, "Завершенный");
        assert_eq!(enum_obj.enum_values[2].uuid, "ff25e312-3fc6-49f2-8382-e5ed6f9746bb");

        // Test find_enum_value method (case-insensitive)
        assert!(enum_obj.find_enum_value("Активный").is_some());
        assert!(enum_obj.find_enum_value("активный").is_some());
        assert!(enum_obj.find_enum_value("АКТИВНЫЙ").is_some());
        assert!(enum_obj.find_enum_value("Несуществующий").is_none());
    }

    #[test]
    fn test_parse_enum_xml_no_values() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.20">
    <Enum uuid="12345678-1234-1234-1234-123456789012">
        <Properties>
            <Name>ПустоеПеречисление</Name>
        </Properties>
    </Enum>
</MetaDataObject>"#;

        let enum_obj = parse_enum_xml(xml).unwrap();

        assert_eq!(enum_obj.name, "ПустоеПеречисление");
        assert_eq!(enum_obj.mdo_type, MdoType::Enum);
        assert_eq!(enum_obj.enum_values.len(), 0);
    }
}
