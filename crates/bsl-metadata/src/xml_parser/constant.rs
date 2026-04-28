//! Constant XML parser

use crate::error::{MetadataError, Result};
use crate::metadata_object::{MdoType, MetadataObject};

use super::helpers::{child_text, find_child, find_mdo_element, parse_xml};
use super::type_parser::parse_type_xml;

/// Parse Constant XML from Designer format.
///
/// Reads the `<Properties>/<Name>` and the optional `<Properties>/<Type>`
/// block. The type element follows the same canonical shape as register
/// dimensions and catalog attributes — `<v8:Type>` children plus
/// `<StringQualifiers>` / `<NumberQualifiers>` / `<DateQualifiers>` —
/// so it is parsed by the shared [`parse_type_xml`] helper.
///
/// Constants whose XML predates type extraction (or whose `<Type>` is
/// absent) leave `MetadataObject::constant_type = None`; downstream
/// consumers fall back to gradual typing.
pub fn parse_constant_xml(xml: &str) -> Result<MetadataObject> {
    let _span = tracing::debug_span!("parse_constant_xml").entered();

    let doc = parse_xml(xml)?;
    let mdo = find_mdo_element(&doc)
        .ok_or_else(|| MetadataError::InvalidFormat("No Constant element found".to_string()))?;

    let props = find_child(mdo, "Properties")
        .ok_or_else(|| MetadataError::InvalidFormat("Constant missing Properties".to_string()))?;

    let name = child_text(props, "Name").unwrap_or("").to_string();
    let mut mdo_obj = MetadataObject::new(MdoType::Constant, name);

    if let Some(type_node) = find_child(props, "Type") {
        let attr_type = parse_type_xml(type_node)?;
        mdo_obj.set_constant_type(attr_type);
    }

    Ok(mdo_obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata_object::AttributeType;

    fn wrap_constant(props: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:cfg="http://v8.1c.ru/8.1/data/enterprise/current-config">
  <Constant>
    <Properties>
      <Name>X</Name>
      {props}
    </Properties>
  </Constant>
</MetaDataObject>"#
        )
    }

    #[test]
    fn parses_string_constant_with_qualifier() {
        let xml = wrap_constant(
            r#"<Type>
                 <v8:Type>xs:string</v8:Type>
                 <StringQualifiers><Length>50</Length></StringQualifiers>
               </Type>"#,
        );
        let mdo = parse_constant_xml(&xml).expect("constant parses");
        assert_eq!(mdo.name, "X");
        match mdo.constant_type {
            Some(AttributeType::String { length }) => assert_eq!(length, Some(50)),
            other => panic!("expected String, got {other:?}"),
        }
    }

    #[test]
    fn parses_number_constant_with_qualifiers() {
        let xml = wrap_constant(
            r#"<Type>
                 <v8:Type>xs:decimal</v8:Type>
                 <NumberQualifiers><Digits>10</Digits><FractionDigits>2</FractionDigits></NumberQualifiers>
               </Type>"#,
        );
        let mdo = parse_constant_xml(&xml).expect("constant parses");
        assert!(matches!(mdo.constant_type, Some(AttributeType::Number { .. })));
    }

    #[test]
    fn parses_ref_constant() {
        let xml = wrap_constant(r#"<Type><v8:Type>cfg:CatalogRef.Справочник1</v8:Type></Type>"#);
        let mdo = parse_constant_xml(&xml).expect("constant parses");
        match mdo.constant_type {
            Some(AttributeType::Ref { mdo_type, name }) => {
                assert_eq!(mdo_type, MdoType::Catalog);
                assert_eq!(name, "Справочник1");
            }
            other => panic!("expected CatalogRef, got {other:?}"),
        }
    }

    #[test]
    fn parses_composite_constant() {
        let xml = wrap_constant(
            r#"<Type>
                 <v8:Type>xs:string</v8:Type>
                 <v8:Type>cfg:CatalogRef.Справочник1</v8:Type>
               </Type>"#,
        );
        let mdo = parse_constant_xml(&xml).expect("constant parses");
        match mdo.constant_type {
            Some(AttributeType::Composite { types }) => {
                assert_eq!(types.len(), 2);
                assert!(matches!(types[0], AttributeType::String { .. }));
                assert!(matches!(types[1], AttributeType::Ref { .. }));
            }
            other => panic!("expected Composite, got {other:?}"),
        }
    }

    #[test]
    fn missing_type_block_is_none() {
        let xml = wrap_constant("");
        let mdo = parse_constant_xml(&xml).expect("constant parses without <Type>");
        assert!(mdo.constant_type.is_none());
    }
}
