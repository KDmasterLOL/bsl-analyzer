//! SameMetadataObjectAndChildNames diagnostic.
//!
//! Detects when a child metadata object (attribute, dimension, resource, tabular section)
//! has the same name as its parent metadata object.
//!
//! Java equivalent: `SameMetadataObjectAndChildNamesDiagnostic.java`

use crate::{Diagnostic, DiagnosticCode, DiagnosticsConfig, Severity};
use hir_def::ModuleMetadata;
use ide_db::TextRange;

/// Supported module types for this diagnostic.
const SUPPORTED_MODULE_TYPES: &[bsl_metadata::ModuleType] = &[
    bsl_metadata::ModuleType::ManagerModule,
    bsl_metadata::ModuleType::ObjectModule,
    // Note: SessionModule is not yet supported in our infrastructure
];

/// Check if module type is supported.
fn is_supported_module_type(module_type: bsl_metadata::ModuleType) -> bool {
    SUPPORTED_MODULE_TYPES.contains(&module_type)
}

/// Collect diagnostics from module metadata.
///
/// Checks for child objects that have the same name as their parent:
/// 1. Attributes of object → name should not match object name
/// 2. Tabular sections → name should not match object name
/// 3. Attributes of tabular sections → name should not match tabular section name
/// 4. Register dimensions → name should not match register name
/// 5. Register resources → name should not match register name
/// 6. Register attributes → name should not match register name
pub fn from_metadata(metadata: &ModuleMetadata, _config: &DiagnosticsConfig) -> Vec<Diagnostic> {
    if !is_supported_module_type(metadata.module_type) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    // Check MetadataObject (Catalog, Document, etc.)
    if let Some(ref mdo) = metadata.mdo {
        collect_mdo_diagnostics(&mdo.name, mdo.as_ref(), &mut diagnostics);
    }

    // Check Register
    if let Some(ref register) = metadata.register {
        collect_register_diagnostics(register.name(), register.as_ref(), &mut diagnostics);
    }

    diagnostics
}

/// Collect diagnostics for a MetadataObject.
fn collect_mdo_diagnostics(
    parent_name: &str,
    mdo: &bsl_metadata::MetadataObject,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let parent_name_lower = parent_name.to_lowercase();

    // Check attributes
    for attr in &mdo.attributes {
        if attr.name.to_lowercase() == parent_name_lower {
            diagnostics.push(create_diagnostic(&attr.name, parent_name));
        }
    }

    // Check tabular sections
    for ts in &mdo.tabular_sections {
        let ts_name = ts.name();
        let ts_name_lower = ts_name.to_lowercase();

        // Tabular section name should not match parent name
        if ts_name_lower == parent_name_lower {
            diagnostics.push(create_diagnostic(ts_name, parent_name));
        }

        // Tabular section attributes should not match tabular section name
        for ts_attr in ts.attributes() {
            if ts_attr.name().to_lowercase() == ts_name_lower {
                diagnostics.push(create_diagnostic(ts_attr.name(), ts_name));
            }
        }
    }
}

/// Collect diagnostics for a Register.
fn collect_register_diagnostics(
    parent_name: &str,
    register: &bsl_metadata::Register,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let parent_name_lower = parent_name.to_lowercase();

    // Check dimensions
    for dim in register.dimensions() {
        if dim.name().to_lowercase() == parent_name_lower {
            diagnostics.push(create_diagnostic(dim.name(), parent_name));
        }
    }

    // Check resources
    for resource in register.resources() {
        if resource.name().to_lowercase() == parent_name_lower {
            diagnostics.push(create_diagnostic(resource.name(), parent_name));
        }
    }

    // Check attributes
    for attr in register.attributes() {
        if attr.name().to_lowercase() == parent_name_lower {
            diagnostics.push(create_diagnostic(attr.name(), parent_name));
        }
    }
}

/// Create a diagnostic for a name conflict.
fn create_diagnostic(child_name: &str, parent_name: &str) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::SameMetadataObjectAndChildNames,
        message: format!(
            "Измените имя '{}', чтобы оно не совпадало с родительским '{}'",
            child_name, parent_name
        ),
        range: TextRange::default(),
        severity: Severity::Critical,
        tags: Vec::new(),
        fixes: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use uuid::Uuid;

    fn make_metadata_with_mdo(mdo: bsl_metadata::MetadataObject) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::ManagerModule,
            execution_context: None,
            common_module: None,
            mdo: Some(Arc::new(mdo)),
            register: None,
        }
    }

    fn make_metadata_with_register(register: bsl_metadata::Register) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::ManagerModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: Some(Arc::new(register)),
        }
    }

    fn default_config() -> DiagnosticsConfig {
        DiagnosticsConfig::default()
    }

    #[test]
    fn test_catalog_with_matching_attribute() {
        let mut catalog =
            bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Catalog, "Номенклатура");
        catalog.add_attribute(bsl_metadata::Attribute {
            name: "Номенклатура".to_string(),
            name_en: None,
            attr_type: bsl_metadata::AttributeType::String { length: Some(100) },
        });

        let metadata = make_metadata_with_mdo(catalog);
        let diagnostics = from_metadata(&metadata, &default_config());

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Номенклатура"));
    }

    #[test]
    fn test_catalog_with_non_matching_attribute() {
        let mut catalog =
            bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Catalog, "Номенклатура");
        catalog.add_attribute(bsl_metadata::Attribute {
            name: "Наименование".to_string(),
            name_en: None,
            attr_type: bsl_metadata::AttributeType::String { length: Some(100) },
        });

        let metadata = make_metadata_with_mdo(catalog);
        let diagnostics = from_metadata(&metadata, &default_config());

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_catalog_with_matching_tabular_section() {
        let mut catalog =
            bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Catalog, "Номенклатура");

        let ts = bsl_metadata::TabularSection::new(Uuid::new_v4(), "Номенклатура");
        catalog.add_tabular_section(ts);

        let metadata = make_metadata_with_mdo(catalog);
        let diagnostics = from_metadata(&metadata, &default_config());

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Номенклатура"));
    }

    #[test]
    fn test_tabular_section_with_matching_attribute() {
        let mut catalog =
            bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Catalog, "Номенклатура");

        let mut ts = bsl_metadata::TabularSection::new(Uuid::new_v4(), "Штрихкоды");
        ts.set_attributes(vec![bsl_metadata::TabularSectionAttribute::new(
            Uuid::new_v4(),
            "Штрихкоды",
            bsl_metadata::AttributeType::String { length: Some(13) },
        )]);
        catalog.add_tabular_section(ts);

        let metadata = make_metadata_with_mdo(catalog);
        let diagnostics = from_metadata(&metadata, &default_config());

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Штрихкоды"));
    }

    #[test]
    fn test_register_with_matching_dimension() {
        let register = bsl_metadata::Register::builder()
            .name("Товары")
            .mdo_type(bsl_metadata::MdoType::InformationRegister)
            .add_dimension(bsl_metadata::Dimension::builder().name("Товары").build())
            .build();

        let metadata = make_metadata_with_register(register);
        let diagnostics = from_metadata(&metadata, &default_config());

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Товары"));
    }

    #[test]
    fn test_register_with_matching_resource() {
        let register = bsl_metadata::Register::builder()
            .name("Остатки")
            .mdo_type(bsl_metadata::MdoType::AccumulationRegister)
            .add_resource(bsl_metadata::RegisterResource::new(Uuid::new_v4(), "Остатки"))
            .build();

        let metadata = make_metadata_with_register(register);
        let diagnostics = from_metadata(&metadata, &default_config());

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Остатки"));
    }

    #[test]
    fn test_register_with_non_matching_children() {
        let register = bsl_metadata::Register::builder()
            .name("РегистрСведений")
            .mdo_type(bsl_metadata::MdoType::InformationRegister)
            .add_dimension(bsl_metadata::Dimension::builder().name("Период").build())
            .add_attribute(bsl_metadata::RegisterAttribute::new(Uuid::new_v4(), "Значение"))
            .build();

        let metadata = make_metadata_with_register(register);
        let diagnostics = from_metadata(&metadata, &default_config());

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_case_insensitive_matching() {
        let mut catalog =
            bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Catalog, "Номенклатура");
        catalog.add_attribute(bsl_metadata::Attribute {
            name: "НОМЕНКЛАТУРА".to_string(),
            name_en: None,
            attr_type: bsl_metadata::AttributeType::String { length: Some(100) },
        });

        let metadata = make_metadata_with_mdo(catalog);
        let diagnostics = from_metadata(&metadata, &default_config());

        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_unsupported_module_type() {
        let catalog =
            bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Catalog, "Номенклатура");

        let metadata = ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: None,
            mdo: Some(Arc::new(catalog)),
            register: None,
        };

        let diagnostics = from_metadata(&metadata, &default_config());

        assert!(diagnostics.is_empty());
    }
}
