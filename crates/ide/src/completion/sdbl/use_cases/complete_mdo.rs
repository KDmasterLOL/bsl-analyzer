//! Use case: Complete MDO types and objects.

use crate::completion::sdbl::domain::{MetadataProvider, SdblCompletionItem};
use crate::completion::CompletionItemKind;
use bsl_metadata::MdoType;

/// Use case for completing MDO types and objects.
///
/// Handles two scenarios:
/// 1. MDO type names (Справочник, Catalog, Документ, Document, etc.)
/// 2. MDO object names for a specific type (Валюты, Контрагенты, etc.)
pub struct CompleteMdoUseCase;

impl CompleteMdoUseCase {
    /// Execute the use case: get MDO type completions.
    ///
    /// Returns all available MDO type names in both Russian and English.
    ///
    /// # Arguments
    /// - `prefix`: Filter prefix (case-insensitive)
    ///
    /// # Returns
    /// List of MDO type completion items
    pub fn execute_types(prefix: &str) -> Vec<SdblCompletionItem> {
        let prefix_lower = prefix.to_lowercase();

        let mut items = Vec::new();

        for &mdo_type in MdoType::all() {
            let russian_name = mdo_type.russian_name();
            let english_name = mdo_type.english_name();

            // Add Russian variant if matches
            if prefix.is_empty() || russian_name.to_lowercase().starts_with(&prefix_lower) {
                items.push(SdblCompletionItem::new(russian_name, CompletionItemKind::MdoType));
            }

            // Add English variant if matches
            if prefix.is_empty() || english_name.to_lowercase().starts_with(&prefix_lower) {
                items.push(SdblCompletionItem::new(english_name, CompletionItemKind::MdoType));
            }
        }

        tracing::debug!(
            prefix = %prefix,
            count = items.len(),
            "CompleteMdoUseCase::execute_types: generated MDO type completions"
        );

        items
    }

    /// Execute the use case: get MDO object completions for a specific type.
    ///
    /// Returns metadata objects filtered by type and prefix.
    ///
    /// # Arguments
    /// - `metadata`: Metadata provider for accessing configuration
    /// - `mdo_type`: Type of metadata objects to complete
    /// - `prefix`: Filter prefix (case-insensitive)
    ///
    /// # Returns
    /// List of MDO object completion items
    pub fn execute_objects(
        metadata: &dyn MetadataProvider,
        mdo_type: MdoType,
        prefix: &str,
    ) -> Vec<SdblCompletionItem> {
        // Get configuration
        let Some(config) = metadata.get_configuration() else {
            tracing::debug!(
                ?mdo_type,
                "CompleteMdoUseCase::execute_objects: no configuration available"
            );
            return Vec::new();
        };

        let prefix_lower = prefix.to_lowercase();

        // Get metadata objects by type
        let objects = Self::get_objects_by_type(&config, mdo_type);

        // Filter and map to completion items
        let items: Vec<SdblCompletionItem> = objects
            .iter()
            .filter(|obj| {
                if prefix.is_empty() {
                    true
                } else {
                    // Match by Russian name or English name
                    obj.name.to_lowercase().starts_with(&prefix_lower)
                        || obj
                            .name_en
                            .as_ref()
                            .is_some_and(|en| en.to_lowercase().starts_with(&prefix_lower))
                }
            })
            .map(|obj| {
                // Show full path in detail: "Справочник.Валюты"
                let detail = format!("{}.{}", mdo_type.russian_name(), obj.name);

                SdblCompletionItem::new(&obj.name, CompletionItemKind::MdoObject)
                    .with_detail(detail)
            })
            .collect();

        tracing::debug!(
            count = items.len(),
            total = objects.len(),
            ?mdo_type,
            prefix = %prefix,
            "CompleteMdoUseCase::execute_objects: generated MDO object completions"
        );

        items
    }

    /// Get metadata objects of a specific type from configuration.
    ///
    /// Handles both regular objects (Catalogs, Documents) and Registers.
    fn get_objects_by_type(
        config: &bsl_metadata::Configuration,
        mdo_type: MdoType,
    ) -> Vec<bsl_metadata::MetadataObject> {
        // Check if this is a register type - registers are stored separately
        if matches!(
            mdo_type,
            MdoType::InformationRegister
                | MdoType::AccumulationRegister
                | MdoType::AccountingRegister
                | MdoType::CalculationRegister
        ) {
            // Convert Register objects to MetadataObject
            return config
                .registers()
                .iter()
                .filter(|reg| reg.mdo_type() == mdo_type)
                .map(|reg| bsl_metadata::MetadataObject::new(mdo_type, reg.name()))
                .collect();
        }

        // Filter metadata_objects by type (for Catalogs, Documents, etc.)
        config.metadata_objects().iter().filter(|obj| obj.mdo_type == mdo_type).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::Configuration;
    use std::sync::Arc;

    // Mock MetadataProvider for testing
    struct MockMetadataProvider {
        config: Option<Arc<Configuration>>,
    }

    impl MetadataProvider for MockMetadataProvider {
        fn get_configuration(&self) -> Option<Arc<Configuration>> {
            self.config.clone()
        }
    }

    fn make_test_configuration() -> Configuration {
        let mut config = Configuration::new("TestConfig");

        // Add test catalogs
        config.add_metadata_object(bsl_metadata::MetadataObject::with_details(
            MdoType::Catalog,
            "Валюты",
            Some("Currencies".to_string()),
            Vec::new(),
        ));
        config.add_metadata_object(bsl_metadata::MetadataObject::with_details(
            MdoType::Catalog,
            "Контрагенты",
            Some("Counterparties".to_string()),
            Vec::new(),
        ));

        // Add test documents
        config.add_metadata_object(bsl_metadata::MetadataObject::with_details(
            MdoType::Document,
            "ПриходнаяНакладная",
            Some("GoodsReceipt".to_string()),
            Vec::new(),
        ));

        config
    }

    #[test]
    fn test_complete_mdo_types_no_prefix() {
        let items = CompleteMdoUseCase::execute_types("");

        // Should return all MDO types (both Russian and English)
        // MdoType::all() returns ~24 types, so ~48 items total
        assert!(items.len() >= 40);

        // Check some specific types
        assert!(items.iter().any(|i| i.label == "Справочник"));
        assert!(items.iter().any(|i| i.label == "Catalog"));
        assert!(items.iter().any(|i| i.label == "Документ"));
        assert!(items.iter().any(|i| i.label == "Document"));
    }

    #[test]
    fn test_complete_mdo_types_with_prefix_russian() {
        let items = CompleteMdoUseCase::execute_types("Спр");

        // Should return only types starting with "Спр"
        assert!(items.iter().any(|i| i.label == "Справочник"));
        assert!(!items.iter().any(|i| i.label == "Документ"));
        assert!(!items.iter().any(|i| i.label == "Catalog"));
    }

    #[test]
    fn test_complete_mdo_types_with_prefix_english() {
        let items = CompleteMdoUseCase::execute_types("Cat");

        // Should return only types starting with "Cat"
        assert!(items.iter().any(|i| i.label == "Catalog"));
        assert!(!items.iter().any(|i| i.label == "Справочник"));
        assert!(!items.iter().any(|i| i.label == "Document"));
    }

    #[test]
    fn test_complete_mdo_types_case_insensitive() {
        let items_upper = CompleteMdoUseCase::execute_types("СПРА");
        let items_lower = CompleteMdoUseCase::execute_types("спра");

        // Should return same results
        assert_eq!(items_upper.len(), items_lower.len());
        assert!(!items_upper.is_empty());
    }

    #[test]
    fn test_complete_mdo_objects_no_metadata() {
        let provider = MockMetadataProvider { config: None };
        let items = CompleteMdoUseCase::execute_objects(&provider, MdoType::Catalog, "");

        // Should return empty when no configuration
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_complete_mdo_objects_catalogs_no_prefix() {
        let config = make_test_configuration();
        let provider = MockMetadataProvider { config: Some(Arc::new(config)) };

        let items = CompleteMdoUseCase::execute_objects(&provider, MdoType::Catalog, "");

        // Should return all catalogs
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "Валюты"));
        assert!(items.iter().any(|i| i.label == "Контрагенты"));
    }

    #[test]
    fn test_complete_mdo_objects_with_prefix_russian() {
        let config = make_test_configuration();
        let provider = MockMetadataProvider { config: Some(Arc::new(config)) };

        let items = CompleteMdoUseCase::execute_objects(&provider, MdoType::Catalog, "Вал");

        // Should return only "Валюты"
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Валюты");
    }

    #[test]
    fn test_complete_mdo_objects_with_prefix_english() {
        let config = make_test_configuration();
        let provider = MockMetadataProvider { config: Some(Arc::new(config)) };

        let items = CompleteMdoUseCase::execute_objects(&provider, MdoType::Catalog, "Curr");

        // Should match by English name
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Валюты");
    }

    #[test]
    fn test_complete_mdo_objects_detail_format() {
        let config = make_test_configuration();
        let provider = MockMetadataProvider { config: Some(Arc::new(config)) };

        let items = CompleteMdoUseCase::execute_objects(&provider, MdoType::Catalog, "Валюты");

        assert_eq!(items.len(), 1);
        let item = &items[0];

        // Should have detail with full path
        assert_eq!(item.detail, Some("Справочник.Валюты".to_string()));
        assert_eq!(item.kind, CompletionItemKind::MdoObject);
    }

    #[test]
    fn test_complete_mdo_objects_filter_by_type() {
        let config = make_test_configuration();
        let provider = MockMetadataProvider { config: Some(Arc::new(config)) };

        // Request Documents - should not return Catalogs
        let items = CompleteMdoUseCase::execute_objects(&provider, MdoType::Document, "");

        assert_eq!(items.len(), 1);
        assert!(items.iter().any(|i| i.label == "ПриходнаяНакладная"));
        assert!(!items.iter().any(|i| i.label == "Валюты"));
    }
}
