//! XML parser for Form metadata.
//!
//! Parses Form.xml files including:
//! - Form type (Managed/Ordinary)
//! - ChildItems with DataPath bindings

use crate::enums::FormType;
use crate::error::{MetadataError, Result};
use crate::form::{Form, FormElement, FormEventHandler};

use super::helpers::parse_uuid;

/// Parse form XML to extract FormType and elements.
///
/// Parses form metadata needed for diagnostics:
/// - Name
/// - FormType (Managed/Ordinary)
/// - UUID
/// - ChildItems with DataPath bindings
///
/// # Example XML
/// ```xml
/// <Form uuid="...">
///     <Properties>
///         <Name>ФормаЭлемента</Name>
///     </Properties>
///     <FormType>Managed</FormType>
///     <ChildItems>
///         <InputField name="Код" id="1">
///             <DataPath>Объект.Code</DataPath>
///         </InputField>
///     </ChildItems>
/// </Form>
/// ```
pub fn parse_form_xml(xml: &str) -> Result<Form> {
    let _span = tracing::debug_span!("parse_form_xml").entered();

    let doc = roxmltree::Document::parse(xml)
        .map_err(|e| MetadataError::InvalidFormat(format!("Invalid form XML: {}", e)))?;

    let root = doc.root_element();

    // Support both <Form ...> root and <FormRoot><Form ...></FormRoot> wrapper
    let form_node = if root.tag_name().name() == "Form" {
        root
    } else {
        root.children()
            .find(|n| n.is_element() && n.tag_name().name() == "Form")
            .ok_or_else(|| MetadataError::InvalidFormat("No <Form> element found".to_string()))?
    };

    let uuid_str = form_node.attribute("uuid").unwrap_or("");
    let uuid = if uuid_str.is_empty() { uuid::Uuid::nil() } else { parse_uuid(uuid_str, "form")? };

    let name = form_node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Properties")
        .and_then(|props| {
            props.children().find(|n| n.is_element() && n.tag_name().name() == "Name")
        })
        .and_then(|n| n.text())
        .unwrap_or("")
        .to_string();

    let form_type_str = form_node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "FormType")
        .and_then(|n| n.text())
        .unwrap_or("");
    let form_type = if form_type_str.is_empty() {
        FormType::Managed
    } else {
        FormType::from_name(form_type_str)
    };

    let mut elements = Vec::new();
    if let Some(child_items) =
        form_node.children().find(|n| n.is_element() && n.tag_name().name() == "ChildItems")
    {
        collect_child_items(child_items, &mut elements);
    }

    let event_handlers = collect_all_events(form_node);

    let command_handlers: Vec<String> = form_node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Commands")
        .map(|commands| {
            commands
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "Command")
                .filter_map(|cmd| {
                    cmd.children()
                        .find(|n| n.is_element() && n.tag_name().name() == "Action")
                        .and_then(|n| n.text())
                        .filter(|t| !t.trim().is_empty())
                        .map(|t| t.trim().to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    let attributes: Vec<String> = form_node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Attributes")
        .map(|attrs| {
            attrs
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "Attribute")
                .filter_map(|attr| attr.attribute("name"))
                .filter(|name| !name.is_empty())
                .map(|name| name.to_string())
                .collect()
        })
        .unwrap_or_default();

    let mut form =
        Form::with_handlers(name, form_type, uuid, elements, event_handlers, command_handlers);
    form.attributes = attributes;

    tracing::debug!(
        form_name = %form.name(),
        form_type = ?form.form_type(),
        uuid = %form.uuid(),
        elements_count = form.elements().len(),
        event_handlers_count = form.event_handlers().len(),
        command_handlers_count = form.command_handlers().len(),
        attributes_count = form.attributes().len(),
        "parsed form"
    );

    Ok(form)
}

/// Recursively collect `<Event>` handlers from an element and all its descendants.
fn collect_all_events(node: roxmltree::Node<'_, '_>) -> Vec<FormEventHandler> {
    let mut handlers = Vec::new();
    collect_events_recursive(node, &mut handlers);
    handlers
}

fn collect_events_recursive(node: roxmltree::Node<'_, '_>, handlers: &mut Vec<FormEventHandler>) {
    for child in node.children().filter(|n| n.is_element()) {
        if child.tag_name().name() == "Event" {
            if let Some(text) = child.text() {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    let event_type = child.attribute("name").unwrap_or("").to_string();
                    handlers
                        .push(FormEventHandler { event_type, handler_name: trimmed.to_string() });
                }
            }
        } else {
            collect_events_recursive(child, handlers);
        }
    }
}

/// Recursively collect `FormElement`s from a `<ChildItems>` node.
///
/// Any element with both `name` and `id` attributes is collected.
/// Elements with a `<ChildItems>` child are recursed into.
fn collect_child_items(child_items: roxmltree::Node<'_, '_>, elements: &mut Vec<FormElement>) {
    for node in child_items.children().filter(|n| n.is_element()) {
        let name = match node.attribute("name") {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };
        let id: u32 = match node.attribute("id").and_then(|s| s.parse().ok()) {
            Some(v) => v,
            None => continue,
        };
        let data_path = node
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "DataPath")
            .and_then(|n| n.text())
            .map(|t| t.to_string());

        elements.push(FormElement { name, id, data_path });

        if let Some(nested) =
            node.children().find(|n| n.is_element() && n.tag_name().name() == "ChildItems")
        {
            collect_child_items(nested, elements);
        }
    }
}

/// Parse form XML from file path.
///
/// Given a BSL module path like:
/// `Catalogs/Справочник1/Forms/ФормаЭлемента/Ext/Form/Module.bsl`
///
/// Reads two XML files:
/// 1. `Catalogs/Справочник1/Forms/ФормаЭлемента.xml` - MetaDataObject with FormType, UUID, Name
/// 2. `Catalogs/Справочник1/Forms/ФормаЭлемента/Ext/Form.xml` - Form definition with Events, Commands
///
/// Combines information from both files.
pub fn parse_form_from_bsl_path(bsl_path: &std::path::Path) -> Result<Form> {
    // Path: .../Forms/<FormName>/Ext/Form/Module.bsl

    let mut forms_dir = bsl_path.to_path_buf();

    // Go up: Module.bsl -> Form -> Ext -> FormName -> Forms
    for _ in 0..4 {
        if !forms_dir.pop() {
            return Err(crate::error::MetadataError::InvalidFormat(format!(
                "Invalid form module path: {}",
                bsl_path.display()
            )));
        }
    }

    // Get form name
    let form_name = bsl_path
        .parent() // Module.bsl -> Form
        .and_then(|p| p.parent()) // Form -> Ext
        .and_then(|p| p.parent()) // Ext -> FormName
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            crate::error::MetadataError::InvalidFormat(format!(
                "Cannot extract form name from: {}",
                bsl_path.display()
            ))
        })?;

    // Path to Ext/Form.xml (contains Events, Commands, ChildItems)
    let ext_form_xml_path = bsl_path
        .parent() // Module.bsl -> Form
        .and_then(|p| p.parent()) // Form -> Ext
        .map(|p| p.join("Form.xml"))
        .ok_or_else(|| {
            crate::error::MetadataError::InvalidFormat(format!(
                "Cannot build Ext/Form.xml path from: {}",
                bsl_path.display()
            ))
        })?;

    // Path to Forms/<FormName>.xml (MetaDataObject with FormType)
    let metadata_xml_path = forms_dir.join(format!("{}.xml", form_name));

    // Read Ext/Form.xml (primary source for Events, Commands, ChildItems)
    let ext_form_xml = std::fs::read_to_string(&ext_form_xml_path).map_err(|e| {
        crate::error::MetadataError::InvalidFormat(format!(
            "Cannot read form XML at {}: {}",
            ext_form_xml_path.display(),
            e
        ))
    })?;

    // Parse Ext/Form.xml
    let mut form = parse_form_xml(&ext_form_xml)?;

    // Try to read MetaDataObject for FormType (optional, may not exist)
    if let Ok(metadata_xml) = std::fs::read_to_string(&metadata_xml_path) {
        if let Ok(metadata) = parse_form_metadata_xml(&metadata_xml) {
            // Update form with metadata info
            form.name = metadata.name;
            form.form_type = metadata.form_type;
            form.uuid = metadata.uuid;
        }
    }

    Ok(form)
}

/// Minimal form metadata from MetaDataObject XML.
struct FormMetadataInfo {
    name: String,
    form_type: FormType,
    uuid: uuid::Uuid,
}

/// Parse form MetaDataObject XML for FormType information.
///
/// MetaDataObject structure:
/// ```xml
/// <MetaDataObject>
///     <Form uuid="...">
///         <Properties>
///             <Name>ФормаЭлемента</Name>
///         </Properties>
///         <FormType>Managed</FormType>
///     </Form>
/// </MetaDataObject>
/// ```
fn parse_form_metadata_xml(xml: &str) -> Result<FormMetadataInfo> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|e| MetadataError::InvalidFormat(format!("Invalid form metadata XML: {}", e)))?;

    let root = doc.root_element();

    let form_node =
        root.children().find(|n| n.is_element() && n.tag_name().name() == "Form").ok_or_else(
            || MetadataError::InvalidFormat("No <Form> element in MetaDataObject".to_string()),
        )?;

    let uuid_str = form_node.attribute("uuid").unwrap_or("");
    let uuid = if uuid_str.is_empty() { uuid::Uuid::nil() } else { parse_uuid(uuid_str, "form")? };

    let form_type_str = form_node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "FormType")
        .and_then(|n| n.text())
        .unwrap_or("");
    let form_type = if form_type_str.is_empty() {
        FormType::Managed
    } else {
        FormType::from_name(form_type_str)
    };

    let name = form_node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Properties")
        .and_then(|props| {
            props.children().find(|n| n.is_element() && n.tag_name().name() == "Name")
        })
        .and_then(|n| n.text())
        .unwrap_or("")
        .to_string();

    Ok(FormMetadataInfo { name, form_type, uuid })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_managed_form() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<FormRoot xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Form uuid="12345678-1234-1234-1234-123456789012">
        <Properties>
            <Name>ФормаЭлемента</Name>
        </Properties>
        <FormType>Managed</FormType>
    </Form>
</FormRoot>"#;

        let form = parse_form_xml(xml).unwrap();
        assert_eq!(form.name(), "ФормаЭлемента");
        assert_eq!(form.form_type(), FormType::Managed);
        assert!(form.is_managed());
    }

    #[test]
    fn test_parse_ordinary_form() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<FormRoot xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Form uuid="12345678-1234-1234-1234-123456789012">
        <Properties>
            <Name>ОбычнаяФорма</Name>
        </Properties>
        <FormType>Ordinary</FormType>
    </Form>
</FormRoot>"#;

        let form = parse_form_xml(xml).unwrap();
        assert_eq!(form.name(), "ОбычнаяФорма");
        assert_eq!(form.form_type(), FormType::Ordinary);
        assert!(form.is_ordinary());
    }

    #[test]
    fn test_parse_form_default_type() {
        // When FormType is not specified, defaults to Managed
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<FormRoot xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Form uuid="12345678-1234-1234-1234-123456789012">
        <Properties>
            <Name>БезТипа</Name>
        </Properties>
    </Form>
</FormRoot>"#;

        let form = parse_form_xml(xml).unwrap();
        assert_eq!(form.form_type(), FormType::Managed);
    }

    #[test]
    fn test_parse_form_with_child_items() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" version="2.10">
    <ChildItems>
        <InputField name="Код" id="1">
            <DataPath>Объект.Code</DataPath>
        </InputField>
        <InputField name="Наименование" id="2">
            <DataPath>Объект.Description</DataPath>
        </InputField>
        <InputField name="НесуществующийРеквизит" id="3">
            <DataPath>~Объект.НесуществующийРеквизит</DataPath>
        </InputField>
    </ChildItems>
</Form>"#;

        let form = parse_form_xml(xml).unwrap();
        assert_eq!(form.elements().len(), 3);

        let wrong: Vec<_> = form.elements_with_wrong_data_path().collect();
        assert_eq!(wrong.len(), 1);
        assert_eq!(wrong[0].name, "НесуществующийРеквизит");
        assert_eq!(wrong[0].data_path.as_deref(), Some("~Объект.НесуществующийРеквизит"));
    }

    #[test]
    fn test_parse_form_with_nested_groups() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" version="2.10">
    <ChildItems>
        <UsualGroup name="Группа1" id="1">
            <ChildItems>
                <InputField name="ПолеВГруппе" id="2">
                    <DataPath>~Объект.УдаленноеПоле</DataPath>
                </InputField>
            </ChildItems>
        </UsualGroup>
    </ChildItems>
</Form>"#;

        let form = parse_form_xml(xml).unwrap();
        assert_eq!(form.elements().len(), 2); // Group + InputField

        let wrong: Vec<_> = form.elements_with_wrong_data_path().collect();
        assert_eq!(wrong.len(), 1);
        assert_eq!(wrong[0].name, "ПолеВГруппе");
    }

    #[test]
    fn test_parse_form_with_events_and_commands() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" version="2.20">
    <Events>
        <Event name="OnCreateAtServer">ПриСозданииНаСервере</Event>
        <Event name="OnOpen">ПриОткрытии</Event>
    </Events>
    <Commands>
        <Command name="Ок" id="1">
            <Action>Ок</Action>
        </Command>
        <Command name="Отмена" id="2">
            <Action>Отмена</Action>
        </Command>
    </Commands>
</Form>"#;

        let form = parse_form_xml(xml).unwrap();

        // Check event handlers
        let event_handler_names = form.event_handler_names();
        assert_eq!(event_handler_names.len(), 2);
        assert!(event_handler_names.contains(&"ПриСозданииНаСервере"));
        assert!(event_handler_names.contains(&"ПриОткрытии"));

        // Check command handlers
        assert_eq!(form.command_handlers().len(), 2);
        assert!(form.command_handlers().contains(&"Ок".to_string()));
        assert!(form.command_handlers().contains(&"Отмена".to_string()));

        // Check is_handler method
        assert!(form.is_handler("ПриСозданииНаСервере"));
        assert!(form.is_handler("присозданиинасервере")); // case-insensitive
        assert!(form.is_handler("Ок"));
        assert!(!form.is_handler("НеСуществующийОбработчик"));
    }

    #[test]
    fn test_parse_form_with_element_events() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" version="2.20">
    <Events>
        <Event name="OnCreateAtServer">ПриСозданииНаСервере</Event>
    </Events>
    <ChildItems>
        <Table name="Список" id="1">
            <Events>
                <Event name="OnActivateRow">СписокПриАктивизацииСтроки</Event>
            </Events>
        </Table>
        <InputField name="Поле1" id="2">
            <Events>
                <Event name="OnChange">Поле1ПриИзменении</Event>
            </Events>
        </InputField>
        <CheckBoxField name="Флаг" id="3">
            <Events>
                <Event name="OnChange">ФлагПриИзменении</Event>
            </Events>
        </CheckBoxField>
    </ChildItems>
</Form>"#;

        let form = parse_form_xml(xml).unwrap();

        // All event handlers (form-level + element-level) should be collected
        assert_eq!(form.event_handler_names().len(), 4);
        assert!(form.is_handler("ПриСозданииНаСервере"));
        assert!(form.is_handler("СписокПриАктивизацииСтроки"));
        assert!(form.is_handler("Поле1ПриИзменении"));
        assert!(form.is_handler("ФлагПриИзменении"));
    }

    #[test]
    fn test_parse_form_attributes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" xmlns:v8="http://v8.1c.ru/8.1/data/core" version="2.20">
    <ChildItems>
        <InputField name="Замечание" id="1">
            <DataPath>Замечание</DataPath>
        </InputField>
        <InputField name="ТекущееОписание" id="4">
            <DataPath>ТекущееОписание</DataPath>
        </InputField>
    </ChildItems>
    <Attributes>
        <Attribute name="Замечание" id="1">
            <Title>
                <v8:item>
                    <v8:lang>ru</v8:lang>
                    <v8:content>Замечание</v8:content>
                </v8:item>
            </Title>
        </Attribute>
        <Attribute name="ТекущееОписание" id="2">
            <Title>
                <v8:item>
                    <v8:lang>ru</v8:lang>
                    <v8:content>Текущее описание</v8:content>
                </v8:item>
            </Title>
        </Attribute>
        <Attribute name="ИсправленноеОписание" id="3">
            <Title>
                <v8:item>
                    <v8:lang>ru</v8:lang>
                    <v8:content>Исправленное описание</v8:content>
                </v8:item>
            </Title>
        </Attribute>
    </Attributes>
</Form>"#;

        let form = parse_form_xml(xml).unwrap();

        // Check attributes parsed
        assert_eq!(form.attributes().len(), 3);
        assert!(form.attributes().contains(&"Замечание".to_string()));
        assert!(form.attributes().contains(&"ТекущееОписание".to_string()));
        assert!(form.attributes().contains(&"ИсправленноеОписание".to_string()));

        // Check elements still parsed
        assert_eq!(form.elements().len(), 2);
    }

    #[test]
    fn test_parse_real_form_xml() {
        let xml = include_str!(
            "../../fixtures/designer/Catalogs/Справочник1/Forms/ФормаЭлемента/Ext/Form.xml"
        );
        let form = parse_form_xml(xml).unwrap();

        // Form has 3 InputFields: Код, Наименование, НесуществующийРеквизит
        assert_eq!(form.elements().len(), 3);

        let wrong: Vec<_> = form.elements_with_wrong_data_path().collect();
        assert_eq!(wrong.len(), 1);
        assert_eq!(wrong[0].name, "НесуществующийРеквизит");

        // Form has one event handler
        assert_eq!(form.event_handler_names().len(), 1);
        assert!(form.is_handler("ПриСозданииНаСервере"));
    }

    #[test]
    fn test_parse_form_with_interleaved_groups_and_attributes() {
        let xml = include_str!(
            "../../fixtures/designer/Catalogs/рдт_Рецептура/Forms/ФормаЭлемента/Ext/Form.xml"
        );
        let result = parse_form_xml(xml);
        assert!(result.is_ok(), "Form XML parsing failed: {:?}", result.err());

        let form = result.unwrap();

        // Interleaved UsualGroup/InputField elements must all be collected
        // Structure: UsualGroup(Поле1) → InputField(Родитель) → UsualGroup(Флаг1) →
        //            InputField(Наименование) → UsualGroup(Дата)
        assert_eq!(
            form.elements().len(),
            8,
            "Expected 8 elements (3 groups + 2 top-level inputs + 3 nested), got: {:?}",
            form.elements().iter().map(|e| &e.name).collect::<Vec<_>>()
        );

        // Verify form attributes parsed from <Attributes> section
        assert_eq!(form.attributes().len(), 4);
        let attr_names: Vec<String> = form.attributes().iter().map(|a| a.to_lowercase()).collect();
        assert!(attr_names.contains(&"объект".to_string()));
        assert!(attr_names.contains(&"новыйобъект".to_string()));
        assert!(attr_names.contains(&"пересчитать".to_string()));
        assert!(
            attr_names.contains(&"рольтекущегопользователявrnd".to_string()),
            "Must contain 'РольТекущегоПользователяВRnD' attribute, got: {:?}",
            form.attributes()
        );
    }

    #[test]
    fn test_parse_form_from_bsl_path() {
        // Get path to fixtures relative to this source file
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let bsl_path = manifest_dir
            .join("fixtures/designer/Catalogs/Справочник1/Forms/ФормаЭлемента/Ext/Form/Module.bsl");

        // Create a dummy Module.bsl if it doesn't exist (needed for path resolution)
        let module_dir = bsl_path.parent().unwrap();
        std::fs::create_dir_all(module_dir).ok();
        if !bsl_path.exists() {
            std::fs::write(&bsl_path, "// dummy").ok();
        }

        let form = parse_form_from_bsl_path(&bsl_path).unwrap();

        // Should have data from both MetaDataObject and Ext/Form.xml
        assert_eq!(form.name(), "ФормаЭлемента");
        assert_eq!(form.form_type(), FormType::Managed);

        // Event handlers from Ext/Form.xml
        assert_eq!(form.event_handler_names().len(), 1);
        assert!(form.is_handler("ПриСозданииНаСервере"));
    }
}
