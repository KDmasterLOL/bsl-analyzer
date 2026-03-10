//! XML parser for Form metadata.
//!
//! Parses Form.xml files including:
//! - Form type (Managed/Ordinary)
//! - ChildItems with DataPath bindings

use serde::Deserialize;

use crate::enums::FormType;
use crate::error::Result;
use crate::form::{Form, FormElement};

use super::helpers::parse_uuid;

/// Root element for form XML (FormRoot wrapper or direct Form)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FormRoot {
    #[serde(alias = "Form")]
    form: FormXmlElement,
}

/// Form XML element (renamed to avoid conflict with Form struct)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FormXmlElement {
    #[serde(rename = "@uuid", default)]
    uuid: String,
    #[serde(default)]
    properties: Option<FormProperties>,
    #[serde(default)]
    form_type: String,
    #[serde(default)]
    child_items: Option<ChildItems>,
    /// Form commands with actions
    #[serde(default)]
    commands: Option<FormCommands>,
}

/// Container for form commands
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct FormCommands {
    #[serde(default)]
    command: Vec<FormCommand>,
}

/// Single form command with action handler
#[allow(dead_code)] // name needed for XML deserialization
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FormCommand {
    /// Command name
    #[serde(rename = "@name", default)]
    name: String,
    /// Action handler method name
    #[serde(default)]
    action: String,
}

/// Form properties
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FormProperties {
    name: String,
}

/// Container for child items (form controls)
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct ChildItems {
    #[serde(default)]
    input_field: Vec<FormControl>,
    #[serde(default)]
    label_field: Vec<FormControl>,
    #[serde(default)]
    check_box_field: Vec<FormControl>,
    #[serde(default)]
    radio_button_field: Vec<FormControl>,
    #[serde(default)]
    text_document_field: Vec<FormControl>,
    #[serde(default)]
    spreadsheet_document_field: Vec<FormControl>,
    #[serde(default)]
    graphical_schema_field: Vec<FormControl>,
    #[serde(default)]
    formatted_document_field: Vec<FormControl>,
    #[serde(default)]
    picture_field: Vec<FormControl>,
    #[serde(default)]
    table: Vec<FormControlWithChildren>,
    #[serde(default)]
    usual_group: Vec<FormControlWithChildren>,
    #[serde(default)]
    pages: Vec<FormControlWithChildren>,
    #[serde(default)]
    page: Vec<FormControlWithChildren>,
}

/// Form control with name, id, and DataPath
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FormControl {
    #[serde(rename = "@name", default)]
    name: String,
    #[serde(rename = "@id", default)]
    id: u32,
    #[serde(default)]
    data_path: Option<String>,
}

/// Form control that can contain nested ChildItems
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FormControlWithChildren {
    #[serde(rename = "@name", default)]
    name: String,
    #[serde(rename = "@id", default)]
    id: u32,
    #[serde(default)]
    data_path: Option<String>,
    #[serde(default)]
    child_items: Option<ChildItems>,
}

impl ChildItems {
    /// Collect all form elements recursively.
    fn collect_elements(&self, elements: &mut Vec<FormElement>) {
        for ctrl in &self.input_field {
            elements.push(ctrl.to_form_element());
        }
        for ctrl in &self.label_field {
            elements.push(ctrl.to_form_element());
        }
        for ctrl in &self.check_box_field {
            elements.push(ctrl.to_form_element());
        }
        for ctrl in &self.radio_button_field {
            elements.push(ctrl.to_form_element());
        }
        for ctrl in &self.text_document_field {
            elements.push(ctrl.to_form_element());
        }
        for ctrl in &self.spreadsheet_document_field {
            elements.push(ctrl.to_form_element());
        }
        for ctrl in &self.graphical_schema_field {
            elements.push(ctrl.to_form_element());
        }
        for ctrl in &self.formatted_document_field {
            elements.push(ctrl.to_form_element());
        }
        for ctrl in &self.picture_field {
            elements.push(ctrl.to_form_element());
        }

        for ctrl in &self.table {
            elements.push(ctrl.to_form_element());
            if let Some(ref children) = ctrl.child_items {
                children.collect_elements(elements);
            }
        }
        for ctrl in &self.usual_group {
            elements.push(ctrl.to_form_element());
            if let Some(ref children) = ctrl.child_items {
                children.collect_elements(elements);
            }
        }
        for ctrl in &self.pages {
            elements.push(ctrl.to_form_element());
            if let Some(ref children) = ctrl.child_items {
                children.collect_elements(elements);
            }
        }
        for ctrl in &self.page {
            elements.push(ctrl.to_form_element());
            if let Some(ref children) = ctrl.child_items {
                children.collect_elements(elements);
            }
        }
    }
}

impl FormControl {
    fn to_form_element(&self) -> FormElement {
        FormElement { name: self.name.clone(), id: self.id, data_path: self.data_path.clone() }
    }
}

impl FormControlWithChildren {
    fn to_form_element(&self) -> FormElement {
        FormElement { name: self.name.clone(), id: self.id, data_path: self.data_path.clone() }
    }
}

/// Collect all `<Event>` handler names from the entire XML tree.
///
/// This uses a streaming XML reader to find every `<Event>` element regardless
/// of nesting depth. Catches both form-level events (OnCreateAtServer, OnOpen)
/// and element-level events (Table.OnActivateRow, InputField.OnChange, etc.).
fn collect_all_event_handlers(xml: &str) -> Vec<String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut handlers = Vec::new();
    let mut in_event = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) if e.local_name().as_ref() == b"Event" => {
                in_event = true;
            }
            Ok(Event::Text(ref e)) if in_event => {
                if let Ok(text) = std::str::from_utf8(e.as_ref()) {
                    let handler = text.trim();
                    if !handler.is_empty() {
                        handlers.push(handler.to_string());
                    }
                }
            }
            Ok(Event::End(ref e)) if e.local_name().as_ref() == b"Event" => {
                in_event = false;
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    handlers
}

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

    // Try parsing with FormRoot wrapper first, then try direct Form element
    let form_xml = if let Ok(root) = quick_xml::de::from_str::<FormRoot>(xml) {
        root.form
    } else {
        quick_xml::de::from_str::<FormXmlElement>(xml)?
    };

    let uuid = if form_xml.uuid.is_empty() {
        uuid::Uuid::nil()
    } else {
        parse_uuid(&form_xml.uuid, "form")?
    };

    let form_type = if form_xml.form_type.is_empty() {
        FormType::Managed // Default
    } else {
        FormType::from_name(&form_xml.form_type)
    };

    let name = form_xml.properties.as_ref().map(|p| p.name.clone()).unwrap_or_default();

    let mut elements = Vec::new();
    if let Some(ref child_items) = form_xml.child_items {
        child_items.collect_elements(&mut elements);
    }

    // Collect ALL event handlers (form-level and element-level) via XML reader.
    // Element-level events (e.g. Table/InputField/CheckBoxField events) are not
    // captured by serde since they can appear on any element type. A single pass
    // with quick_xml::Reader reliably collects every <Event> handler in the tree.
    let event_handlers = collect_all_event_handlers(xml);

    // Collect command handlers
    let command_handlers: Vec<String> = form_xml
        .commands
        .as_ref()
        .map(|commands| {
            commands
                .command
                .iter()
                .filter(|c| !c.action.is_empty())
                .map(|c| c.action.clone())
                .collect()
        })
        .unwrap_or_default();

    let form =
        Form::with_handlers(name, form_type, uuid, elements, event_handlers, command_handlers);

    tracing::debug!(
        form_name = %form.name(),
        form_type = ?form.form_type(),
        uuid = %form.uuid(),
        elements_count = form.elements().len(),
        event_handlers_count = form.event_handlers().len(),
        command_handlers_count = form.command_handlers().len(),
        "parsed form"
    );

    Ok(form)
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
    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct MetaDataObjectRoot {
        form: FormMetadataElement,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct FormMetadataElement {
        #[serde(rename = "@uuid", default)]
        uuid: String,
        #[serde(default)]
        properties: Option<FormMetadataProperties>,
        #[serde(default)]
        form_type: String,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct FormMetadataProperties {
        name: String,
    }

    let root: MetaDataObjectRoot = quick_xml::de::from_str(xml)?;

    let uuid = if root.form.uuid.is_empty() {
        uuid::Uuid::nil()
    } else {
        parse_uuid(&root.form.uuid, "form")?
    };

    let form_type = if root.form.form_type.is_empty() {
        FormType::Managed
    } else {
        FormType::from_name(&root.form.form_type)
    };

    let name = root.form.properties.map(|p| p.name).unwrap_or_default();

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
        assert_eq!(form.event_handlers().len(), 2);
        assert!(form.event_handlers().contains(&"ПриСозданииНаСервере".to_string()));
        assert!(form.event_handlers().contains(&"ПриОткрытии".to_string()));

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
        assert_eq!(form.event_handlers().len(), 4);
        assert!(form.is_handler("ПриСозданииНаСервере"));
        assert!(form.is_handler("СписокПриАктивизацииСтроки"));
        assert!(form.is_handler("Поле1ПриИзменении"));
        assert!(form.is_handler("ФлагПриИзменении"));
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
        assert_eq!(form.event_handlers().len(), 1);
        assert!(form.is_handler("ПриСозданииНаСервере"));
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
        assert_eq!(form.event_handlers().len(), 1);
        assert!(form.is_handler("ПриСозданииНаСервере"));
    }
}
