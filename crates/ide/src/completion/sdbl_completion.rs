//! SDBL query completion.
//!
//! Provides completion suggestions for SDBL queries based on:
//! - Position context (after FROM, inside MDO type, etc.)
//! - Metadata (available catalogs, documents, registers, etc.)

use super::{CompletionItem, CompletionItemKind, CompletionPosition};
use bsl_metadata::{Configuration, MdoType};
use ide_db::RootDatabase;
use sdbl_hir::{detect_context, detect_sdbl_at_position, SdblCompletionContext};

/// Main SDBL completion entry point.
///
/// Returns completion suggestions if cursor is inside an SDBL query string.
pub(super) fn sdbl_completions(
    db: &dyn RootDatabase,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    let file_id = position.file_id;
    let offset = position.offset;

    // Get parsed file
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    // Check if position is inside SDBL query string
    let query_info = detect_sdbl_at_position(&root, offset)?;

    tracing::debug!(
        query_len = query_info.query_text.len(),
        offset_in_query = u32::from(query_info.offset_in_query),
        "detected SDBL query"
    );

    // Determine completion context
    let context = detect_context(&query_info.query_text, query_info.offset_in_query);

    match context {
        SdblCompletionContext::AfterFromKeyword => {
            tracing::debug!("completion context: AfterFromKeyword");
            Some(complete_mdo_types())
        }
        SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
            tracing::debug!(
                ?mdo_type,
                prefix = %prefix,
                "completion context: InsideMdoType"
            );
            let config = get_configuration(db, position.workspace_root.as_deref());
            Some(complete_mdo_objects(&config, mdo_type, &prefix))
        }
        SdblCompletionContext::None => {
            tracing::trace!("no completion context detected");
            None
        }
    }
}

/// Complete MDO types (Справочник, Catalog, Документ, Document, etc.)
///
/// Returns all available MDO type names in both Russian and English.
fn complete_mdo_types() -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for &mdo_type in MdoType::all() {
        // Russian variant
        items.push(CompletionItem {
            label: mdo_type.russian_name().to_string(),
            detail: None,
            kind: CompletionItemKind::MdoType,
            insert_text: mdo_type.russian_name().to_string(),
            documentation: None,
        });

        // English variant
        items.push(CompletionItem {
            label: mdo_type.english_name().to_string(),
            detail: None,
            kind: CompletionItemKind::MdoType,
            insert_text: mdo_type.english_name().to_string(),
            documentation: None,
        });
    }

    tracing::debug!(count = items.len(), "generated MDO type completions");
    items
}

/// Complete MDO objects for a specific type.
///
/// Returns metadata objects filtered by:
/// - MDO type (Catalog, Document, etc.)
/// - Prefix (typed text after the dot)
///
/// # Arguments
///
/// * `config` - Configuration with metadata objects
/// * `mdo_type` - Metadata object type
/// * `prefix` - Filter prefix (case-insensitive)
fn complete_mdo_objects(
    config: &Configuration,
    mdo_type: MdoType,
    prefix: &str,
) -> Vec<CompletionItem> {
    // Get metadata objects of the specified type
    let objects = get_objects_by_type(config, mdo_type);

    let prefix_lower = prefix.to_lowercase();

    // Filter objects by prefix (case-insensitive)
    let items: Vec<CompletionItem> = objects
        .iter()
        .filter(|obj| {
            // Match by Russian name
            obj.name.to_lowercase().starts_with(&prefix_lower)
                // Or match by English name (if available)
                || obj
                    .name_en
                    .as_ref()
                    .is_some_and(|en| en.to_lowercase().starts_with(&prefix_lower))
        })
        .map(|obj| {
            CompletionItem {
                label: obj.name.clone(),
                // Show full path in detail: "Справочник.Валюты"
                detail: Some(format!("{}.{}", mdo_type.russian_name(), obj.name)),
                kind: CompletionItemKind::MdoObject,
                insert_text: obj.name.clone(),
                documentation: None,
            }
        })
        .collect();

    tracing::debug!(
        count = items.len(),
        total = objects.len(),
        ?mdo_type,
        "generated MDO object completions"
    );

    items
}

/// Get metadata objects of a specific type from configuration.
fn get_objects_by_type(
    config: &Configuration,
    mdo_type: MdoType,
) -> Vec<bsl_metadata::MetadataObject> {
    // Filter metadata_objects by type
    config.metadata_objects().iter().filter(|obj| obj.mdo_type == mdo_type).cloned().collect()
}

/// Get configuration from workspace.
///
/// Searches for 1C configuration using multiple strategies:
/// 1. Read .bsl-language-server.json or .bsl-analyzer.json (configurationRoot field)
/// 2. Search for Configuration.xml in workspace (max depth 2)
///
/// If no configuration is found, returns an empty Configuration (no completion suggestions).
///
/// # Arguments
///
/// * `_db` - Database (unused currently, reserved for future Salsa integration)
/// * `workspace_root` - Root directory of the workspace
fn get_configuration(
    _db: &dyn RootDatabase,
    workspace_root: Option<&std::path::Path>,
) -> std::sync::Arc<Configuration> {
    use std::sync::Arc;

    // If workspace_root provided, try to find configuration
    if let Some(root) = workspace_root {
        tracing::debug!(
            workspace_root = ?root,
            "searching for 1C configuration"
        );

        if let Some(config_path) = crate::config_finder::find_configuration_path(root) {
            match bsl_metadata::load_from_directory(&config_path) {
                Ok(config) => {
                    tracing::info!(
                        catalogs = config.metadata_objects().iter()
                            .filter(|obj| obj.mdo_type == bsl_metadata::MdoType::Catalog)
                            .count(),
                        documents = config.metadata_objects().iter()
                            .filter(|obj| obj.mdo_type == bsl_metadata::MdoType::Document)
                            .count(),
                        config_path = ?config_path,
                        "loaded metadata from workspace"
                    );
                    return Arc::new(config);
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        config_path = ?config_path,
                        "failed to load metadata from configuration path"
                    );
                }
            }
        }
    }

    // No configuration found - return empty configuration (no completion suggestions)
    tracing::warn!(
        workspace_root = ?workspace_root,
        "no metadata found, using empty configuration"
    );
    Arc::new(Configuration::new("EmptyConfiguration"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::MetadataObject;

    fn setup_test_configuration() -> Configuration {
        // Create test configuration with sample metadata
        let mut config = Configuration::new("TestConfig");

        // Add some test catalogs
        config.add_metadata_object(MetadataObject::new(MdoType::Catalog, "Валюты"));
        config.add_metadata_object(MetadataObject::new(MdoType::Catalog, "Контрагенты"));
        config.add_metadata_object(MetadataObject::new(MdoType::Catalog, "Номенклатура"));

        // Add some test documents
        config.add_metadata_object(MetadataObject::new(MdoType::Document, "ЗаказПокупателя"));
        config.add_metadata_object(MetadataObject::new(MdoType::Document, "Продажа"));

        config
    }

    #[test]
    fn test_complete_mdo_types() {
        let items = complete_mdo_types();

        // Should have both Russian and English for each MDO type
        // MdoType::all() returns 20 types, so we should have 40 items (20 * 2)
        assert_eq!(items.len(), 40);

        // Check for specific types
        assert!(items.iter().any(|i| i.label == "Справочник"));
        assert!(items.iter().any(|i| i.label == "Catalog"));
        assert!(items.iter().any(|i| i.label == "Документ"));
        assert!(items.iter().any(|i| i.label == "Document"));
        assert!(items.iter().any(|i| i.label == "РегистрСведений"));
        assert!(items.iter().any(|i| i.label == "InformationRegister"));
    }

    #[test]
    fn test_complete_mdo_objects_all() {
        let config = setup_test_configuration();

        let items = complete_mdo_objects(&config, MdoType::Catalog, "");

        // Should return all catalogs (no prefix filtering)
        assert_eq!(items.len(), 3);
        assert!(items.iter().any(|i| i.label == "Валюты"));
        assert!(items.iter().any(|i| i.label == "Контрагенты"));
        assert!(items.iter().any(|i| i.label == "Номенклатура"));

        // Check details
        let valuta_item = items.iter().find(|i| i.label == "Валюты").unwrap();
        assert_eq!(valuta_item.detail, Some("Справочник.Валюты".to_string()));
        assert_eq!(valuta_item.kind, CompletionItemKind::MdoObject);
    }

    #[test]
    fn test_complete_mdo_objects_with_prefix() {
        let config = setup_test_configuration();

        let items = complete_mdo_objects(&config, MdoType::Catalog, "Вал");

        // Should return only "Валюты" (starts with "Вал")
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Валюты");
    }

    #[test]
    fn test_complete_mdo_objects_case_insensitive() {
        let config = setup_test_configuration();

        let items = complete_mdo_objects(&config, MdoType::Catalog, "вал");

        // Case-insensitive matching
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Валюты");
    }

    #[test]
    fn test_complete_mdo_objects_no_match() {
        let config = setup_test_configuration();

        let items = complete_mdo_objects(&config, MdoType::Catalog, "Xyz");

        // No matches
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_complete_mdo_objects_documents() {
        let config = setup_test_configuration();

        let items = complete_mdo_objects(&config, MdoType::Document, "");

        // Should return all documents
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "ЗаказПокупателя"));
        assert!(items.iter().any(|i| i.label == "Продажа"));
    }

    #[test]
    fn test_complete_mdo_objects_empty_config() {
        let config = Configuration::new("Empty");

        let items = complete_mdo_objects(&config, MdoType::Catalog, "");

        // No metadata objects in empty config
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_load_fixtures_directly() {
        // Load fixtures using absolute path from workspace root
        // This simulates what get_configuration() should do in production
        let fixtures_path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let config =
            bsl_metadata::load_from_directory(fixtures_path).expect("Failed to load fixtures");

        let metadata_objects = config.metadata_objects();

        // Check that we have the expected objects from fixtures
        let catalogs: Vec<_> =
            metadata_objects.iter().filter(|obj| obj.mdo_type == MdoType::Catalog).collect();

        let documents: Vec<_> =
            metadata_objects.iter().filter(|obj| obj.mdo_type == MdoType::Document).collect();

        // fixtures/designer has Справочник1 and possibly СправочникСМенеджером
        assert!(
            !catalogs.is_empty(),
            "Expected at least 1 catalog from fixtures, got {}",
            catalogs.len()
        );

        // fixtures/designer has Документ1
        assert!(
            !documents.is_empty(),
            "Expected at least 1 document from fixtures, got {}",
            documents.len()
        );

        // Check specific objects exist
        assert!(
            metadata_objects.iter().any(|obj| obj.name == "Справочник1"),
            "Expected Справочник1 in fixtures"
        );

        assert!(
            metadata_objects.iter().any(|obj| obj.name == "Документ1"),
            "Expected Документ1 in fixtures"
        );
    }

    #[test]
    fn test_complete_mdo_objects_with_fixtures() {
        // Load real fixtures for integration testing
        let fixtures_path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let config =
            bsl_metadata::load_from_directory(fixtures_path).expect("Failed to load fixtures");

        // Test catalog completion
        let catalog_items = complete_mdo_objects(&config, MdoType::Catalog, "");

        // Should have at least Справочник1 from fixtures
        assert!(
            !catalog_items.is_empty(),
            "Expected at least 1 catalog completion item, got {}",
            catalog_items.len()
        );

        assert!(
            catalog_items.iter().any(|item| item.label == "Справочник1"),
            "Expected Справочник1 in completion items"
        );

        // Check item structure
        let справочник1 = catalog_items
            .iter()
            .find(|item| item.label == "Справочник1")
            .expect("Справочник1 not found");

        assert_eq!(справочник1.detail, Some("Справочник.Справочник1".to_string()));
        assert_eq!(справочник1.kind, CompletionItemKind::MdoObject);
        assert_eq!(справочник1.insert_text, "Справочник1");

        // Test document completion
        let document_items = complete_mdo_objects(&config, MdoType::Document, "");

        assert!(
            !document_items.is_empty(),
            "Expected at least 1 document completion item, got {}",
            document_items.len()
        );

        assert!(
            document_items.iter().any(|item| item.label == "Документ1"),
            "Expected Документ1 in completion items"
        );
    }
}
