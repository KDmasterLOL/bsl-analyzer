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

    let form = Form::with_elements(name, form_type, uuid, elements);

    tracing::debug!(
        form_name = %form.name(),
        form_type = ?form.form_type(),
        uuid = %form.uuid(),
        elements_count = form.elements().len(),
        "parsed form"
    );

    Ok(form)
}

/// Parse form XML from file path.
///
/// Given a BSL module path like:
/// `Catalogs/Справочник1/Forms/ФормаЭлемента/Ext/Form/Module.bsl`
///
/// Derives the form XML path:
/// `Catalogs/Справочник1/Forms/ФормаЭлемента.xml`
pub fn parse_form_from_bsl_path(bsl_path: &std::path::Path) -> Result<Form> {
    // Path: .../Forms/<FormName>/Ext/Form/Module.bsl
    // Need: .../Forms/<FormName>.xml

    let mut path = bsl_path.to_path_buf();

    // Go up: Module.bsl -> Form -> Ext -> FormName -> Forms
    for _ in 0..4 {
        if !path.pop() {
            return Err(crate::error::MetadataError::InvalidFormat(format!(
                "Invalid form module path: {}",
                bsl_path.display()
            )));
        }
    }

    // Now path points to Forms directory, get form name
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

    // Build XML path
    path.push(format!("{}.xml", form_name));

    // Read and parse XML
    let xml = std::fs::read_to_string(&path).map_err(|e| {
        crate::error::MetadataError::InvalidFormat(format!(
            "Cannot read form XML at {}: {}",
            path.display(),
            e
        ))
    })?;

    parse_form_xml(&xml)
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
    }
}
