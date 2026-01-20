//! XML parser for Form metadata.
//!
//! Parses `<FormType>Managed</FormType>` from form XML files.

use serde::Deserialize;

use crate::enums::FormType;
use crate::error::Result;
use crate::form::Form;

use super::helpers::parse_uuid;

/// Root element for form XML
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FormRoot {
    form: FormElement,
}

/// Form element
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FormElement {
    #[serde(rename = "@uuid")]
    uuid: String,
    properties: FormProperties,
    #[serde(default)]
    form_type: String,
}

/// Form properties
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FormProperties {
    name: String,
}

/// Parse form XML to extract FormType.
///
/// Parses the minimal form metadata needed for diagnostics:
/// - Name
/// - FormType (Managed/Ordinary)
/// - UUID
///
/// # Example XML
/// ```xml
/// <Form uuid="...">
///     <Properties>
///         <Name>ФормаЭлемента</Name>
///     </Properties>
///     <FormType>Managed</FormType>
/// </Form>
/// ```
pub fn parse_form_xml(xml: &str) -> Result<Form> {
    let _span = tracing::debug_span!("parse_form_xml").entered();

    let metadata: FormRoot = quick_xml::de::from_str(xml)?;
    let uuid = parse_uuid(&metadata.form.uuid, "form")?;

    let form_type = if metadata.form.form_type.is_empty() {
        FormType::Managed // Default
    } else {
        FormType::from_name(&metadata.form.form_type)
    };

    let form = Form::new(metadata.form.properties.name, form_type, uuid);

    tracing::debug!(
        form_name = %form.name(),
        form_type = ?form.form_type(),
        uuid = %form.uuid(),
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
}
