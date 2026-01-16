//! Use case: Complete nested elements (tabular sections, virtual tables).

use crate::completion::sdbl::domain::{MetadataProvider, SdblCompletionItem};
use crate::completion::CompletionItemKind;
use bsl_metadata::MdoType;

/// Use case for completing nested elements after MDO object name.
///
/// Returns:
/// - Tabular sections for regular objects (catalogs, documents, etc.)
/// - Virtual tables for registers
pub struct CompleteNestedElementsUseCase;

impl CompleteNestedElementsUseCase {
    /// Execute the use case: get nested elements (tabular sections or virtual tables).
    ///
    /// # Arguments
    /// - `metadata_provider`: Access to metadata
    /// - `mdo_type`: Type of the metadata object
    /// - `object_name`: Name of the object
    /// - `prefix`: Filter prefix (case-insensitive)
    ///
    /// # Returns
    /// List of matching nested elements
    pub fn execute(
        metadata_provider: &impl MetadataProvider,
        mdo_type: MdoType,
        object_name: &str,
        prefix: &str,
    ) -> Vec<SdblCompletionItem> {
        tracing::info!(
            ?mdo_type,
            object_name = %object_name,
            prefix = %prefix,
            "CompleteNestedElementsUseCase: executing"
        );

        // Distinguish between registers and other MDO types
        match mdo_type {
            MdoType::InformationRegister
            | MdoType::AccumulationRegister
            | MdoType::AccountingRegister
            | MdoType::CalculationRegister => {
                // Find register and return virtual tables
                Self::complete_virtual_tables(metadata_provider, mdo_type, object_name, prefix)
            }
            _ => {
                // Find MDO object and return tabular sections
                Self::complete_tabular_sections(metadata_provider, mdo_type, object_name, prefix)
            }
        }
    }

    /// Complete tabular sections for an MDO object.
    fn complete_tabular_sections(
        metadata_provider: &impl MetadataProvider,
        mdo_type: MdoType,
        object_name: &str,
        prefix: &str,
    ) -> Vec<SdblCompletionItem> {
        // Get configuration
        let Some(config) = metadata_provider.get_configuration() else {
            tracing::debug!("no configuration available");
            return Vec::new();
        };

        // Find metadata object by name
        let Some(object) = config.find_metadata_object(mdo_type, object_name) else {
            tracing::debug!(
                ?mdo_type,
                object_name = %object_name,
                "metadata object not found"
            );
            return Vec::new();
        };

        let prefix_lower = prefix.to_lowercase();

        // Filter tabular sections by prefix
        let items: Vec<SdblCompletionItem> = object
            .tabular_sections
            .iter()
            .filter(|ts| ts.name().to_lowercase().starts_with(&prefix_lower))
            .map(|ts| {
                let detail = format!("{}.{}.{}", mdo_type.russian_name(), object_name, ts.name());
                let documentation =
                    ts.synonym().map(|s| s.to_string()).unwrap_or_else(|| ts.name().to_string());

                SdblCompletionItem::new(ts.name(), CompletionItemKind::Field)
                    .with_detail(detail)
                    .with_documentation(documentation)
            })
            .collect();

        tracing::debug!(
            count = items.len(),
            total = object.tabular_sections.len(),
            ?mdo_type,
            object_name = %object_name,
            "generated tabular section completions"
        );

        items
    }

    /// Complete virtual tables for a register.
    fn complete_virtual_tables(
        metadata_provider: &impl MetadataProvider,
        mdo_type: MdoType,
        register_name: &str,
        prefix: &str,
    ) -> Vec<SdblCompletionItem> {
        // Get configuration
        let Some(config) = metadata_provider.get_configuration() else {
            tracing::debug!("no configuration available");
            return Vec::new();
        };

        // Find register by name
        let Some(register) = config.find_register(register_name) else {
            tracing::debug!(
                ?mdo_type,
                register_name = %register_name,
                "register not found"
            );
            return Vec::new();
        };

        // Verify that register type matches (since find_register only checks name)
        if register.mdo_type() != mdo_type {
            tracing::debug!(
                ?mdo_type,
                found_type = ?register.mdo_type(),
                register_name = %register_name,
                "register type mismatch"
            );
            return Vec::new();
        }

        // Get virtual tables based on register parameters
        let virtual_tables = register.virtual_tables();

        let prefix_lower = prefix.to_lowercase();

        // Filter virtual tables by prefix
        let items: Vec<SdblCompletionItem> = virtual_tables
            .into_iter()
            .filter(|vt_name| vt_name.to_lowercase().starts_with(&prefix_lower))
            .map(|vt_name| {
                let detail = format!("{}.{}.{}", mdo_type.russian_name(), register_name, vt_name);

                SdblCompletionItem::new(vt_name, CompletionItemKind::Field).with_detail(detail)
            })
            .collect();

        tracing::debug!(
            count = items.len(),
            ?mdo_type,
            register_name = %register_name,
            "generated virtual table completions"
        );

        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::{Configuration, MetadataObject, TabularSection};
    use std::sync::Arc;
    use uuid::Uuid;

    struct MockMetadataProvider {
        config: Arc<Configuration>,
    }

    impl MetadataProvider for MockMetadataProvider {
        fn get_configuration(&self) -> Option<Arc<Configuration>> {
            Some(self.config.clone())
        }
    }

    fn make_test_provider() -> MockMetadataProvider {
        let mut config = Configuration::new("TestConfig");

        // Add catalog with tabular sections
        let catalog = MetadataObject {
            mdo_type: MdoType::Catalog,
            name: "Номенклатура".to_string(),
            name_en: Some("Items".to_string()),
            attributes: vec![],
            tabular_sections: vec![
                TabularSection::new(Uuid::nil(), "Штрихкоды"),
                TabularSection::new(Uuid::nil(), "Изображения"),
            ],
            children: vec![],
        };
        config.add_metadata_object(catalog);

        MockMetadataProvider { config: Arc::new(config) }
    }

    #[test]
    fn test_complete_tabular_sections_no_prefix() {
        let provider = make_test_provider();
        let items =
            CompleteNestedElementsUseCase::execute(&provider, MdoType::Catalog, "Номенклатура", "");

        // Should return all tabular sections
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "Штрихкоды"));
        assert!(items.iter().any(|i| i.label == "Изображения"));
    }

    #[test]
    fn test_complete_tabular_sections_with_prefix() {
        let provider = make_test_provider();
        let items = CompleteNestedElementsUseCase::execute(
            &provider,
            MdoType::Catalog,
            "Номенклатура",
            "Штри",
        );

        // Should return only matching tabular section
        assert_eq!(items.len(), 1);
        assert!(items.iter().any(|i| i.label == "Штрихкоды"));
    }

    #[test]
    fn test_complete_tabular_sections_unknown_object() {
        let provider = make_test_provider();
        let items = CompleteNestedElementsUseCase::execute(
            &provider,
            MdoType::Catalog,
            "НесуществующийОбъект",
            "",
        );

        // Should return empty for unknown object
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_complete_tabular_sections_has_detail() {
        let provider = make_test_provider();
        let items = CompleteNestedElementsUseCase::execute(
            &provider,
            MdoType::Catalog,
            "Номенклатура",
            "Штри",
        );

        assert_eq!(items.len(), 1);
        let item = &items[0];

        // Check that detail has full path
        assert!(item.detail.is_some());
        assert!(item.detail.as_ref().unwrap().contains("Справочник.Номенклатура.Штрихкоды"));
    }
}
