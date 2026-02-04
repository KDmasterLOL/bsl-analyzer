//! XML to BSL dependency resolution.
//!
//! Maps Designer format XML paths to their corresponding BSL module paths.

use std::path::{Path, PathBuf};

/// Resolve an XML file path to its corresponding BSL module path.
///
/// Designer format patterns:
/// - `*/Forms/<Name>/Ext/Form.xml` → `*/Forms/<Name>/Ext/Form/Module.bsl`
/// - `*/Forms/<Name>.xml` → `*/Forms/<Name>/Ext/Form/Module.bsl`
///
/// Returns None if the path doesn't match any known pattern.
pub fn resolve_xml_to_bsl(xml_path: &Path) -> Option<PathBuf> {
    let path_str = xml_path.to_string_lossy();
    let path_str = path_str.replace('\\', "/");

    // Pattern 1: Forms/<Name>/Ext/Form.xml → Forms/<Name>/Ext/Form/Module.bsl
    if path_str.ends_with("/Ext/Form.xml") {
        let base = path_str.strip_suffix("/Ext/Form.xml")?;
        // Check if this is a form (has Forms parent)
        if base.contains("/Forms/") || base.starts_with("Forms/") {
            return Some(PathBuf::from(format!("{}/Ext/Form/Module.bsl", base)));
        }
    }

    // Pattern 2: Forms/<Name>.xml → Forms/<Name>/Ext/Form/Module.bsl
    // This is the flat Designer format where Form.xml is at the form root
    if path_str.ends_with(".xml") {
        let base = path_str.strip_suffix(".xml")?;
        // Check if this looks like a form definition
        if base.contains("/Forms/") || base.starts_with("Forms/") {
            // Make sure we're not matching Form.xml from Pattern 1
            if !base.ends_with("/Ext/Form") {
                return Some(PathBuf::from(format!("{}/Ext/Form/Module.bsl", base)));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_form_xml_to_module() {
        // Pattern 1: Ext/Form.xml
        assert_eq!(
            resolve_xml_to_bsl(Path::new("Документы/Заказ/Forms/Форма/Ext/Form.xml")),
            Some(PathBuf::from("Документы/Заказ/Forms/Форма/Ext/Form/Module.bsl"))
        );

        assert_eq!(
            resolve_xml_to_bsl(Path::new("Forms/ФормаСписка/Ext/Form.xml")),
            Some(PathBuf::from("Forms/ФормаСписка/Ext/Form/Module.bsl"))
        );

        // With backslashes (Windows)
        assert_eq!(
            resolve_xml_to_bsl(Path::new("Forms\\Test\\Ext\\Form.xml")),
            Some(PathBuf::from("Forms/Test/Ext/Form/Module.bsl"))
        );
    }

    #[test]
    fn test_form_definition_xml() {
        // Pattern 2: Form definition at root
        assert_eq!(
            resolve_xml_to_bsl(Path::new("Документы/Заказ/Forms/Форма.xml")),
            Some(PathBuf::from("Документы/Заказ/Forms/Форма/Ext/Form/Module.bsl"))
        );

        assert_eq!(
            resolve_xml_to_bsl(Path::new("Forms/ФормаВыбора.xml")),
            Some(PathBuf::from("Forms/ФормаВыбора/Ext/Form/Module.bsl"))
        );
    }

    #[test]
    fn test_non_form_xml() {
        // Configuration XML - not a form
        assert_eq!(resolve_xml_to_bsl(Path::new("Configuration.xml")), None);

        // Other XML files
        assert_eq!(resolve_xml_to_bsl(Path::new("Ext/Module.xml")), None);
        assert_eq!(resolve_xml_to_bsl(Path::new("Templates/Макет.xml")), None);
    }

    #[test]
    fn test_nested_forms() {
        // Nested in document
        assert_eq!(
            resolve_xml_to_bsl(Path::new("Documents/Order/Forms/ItemForm/Ext/Form.xml")),
            Some(PathBuf::from("Documents/Order/Forms/ItemForm/Ext/Form/Module.bsl"))
        );

        // Nested in catalog
        assert_eq!(
            resolve_xml_to_bsl(Path::new("Catalogs/Products/Forms/SelectForm/Ext/Form.xml")),
            Some(PathBuf::from("Catalogs/Products/Forms/SelectForm/Ext/Form/Module.bsl"))
        );
    }
}
