//! Use case: Complete VALUE() function elements (enum values, predefined items, EmptyRef).

use crate::completion::sdbl::domain::{MetadataProvider, SdblCompletionItem};
use crate::completion::CompletionItemKind;
use bsl_metadata::MdoType;

/// Use case for completing elements inside VALUE() function.
///
/// Handles third level of VALUE completion:
/// - Enum values for Enum type
/// - Predefined items for Catalog, Document, etc.
/// - ПустаяСсылка/EmptyRef for all reference types
pub struct CompleteValueElementsUseCase;

impl CompleteValueElementsUseCase {
    /// Execute the use case: get VALUE element completions.
    ///
    /// Returns:
    /// - EnumValue names for Enum type
    /// - PredefinedItem names for Catalog/Document types
    /// - ПустаяСсылка (RU) or EmptyRef (EN) based on context language
    ///
    /// # Arguments
    /// - `metadata`: Metadata provider for accessing configuration
    /// - `mdo_type`: Type of metadata object
    /// - `object_name`: Name of the specific object (e.g., "Статусы")
    /// - `prefix`: Filter prefix (case-insensitive)
    /// - `is_russian`: True if Russian keywords used in context
    ///
    /// # Returns
    /// List of element completion items
    pub fn execute(
        metadata: &dyn MetadataProvider,
        mdo_type: MdoType,
        object_name: &str,
        prefix: &str,
        is_russian: bool,
    ) -> Vec<SdblCompletionItem> {
        let mut items = Vec::new();

        // Suggest ПустаяСсылка (RU) or EmptyRef (EN) based on context language
        items.extend(Self::empty_ref_items(prefix, is_russian));

        // Get configuration
        let Some(config) = metadata.get_configuration() else {
            tracing::debug!(
                ?mdo_type,
                object_name = %object_name,
                "CompleteValueElementsUseCase::execute: no configuration available"
            );
            return items;
        };

        // Find the metadata object
        let Some(mdo) = Self::find_metadata_object(&config, mdo_type, object_name) else {
            tracing::debug!(
                ?mdo_type,
                object_name = %object_name,
                "CompleteValueElementsUseCase::execute: metadata object not found"
            );
            return items;
        };

        // Add type-specific elements
        match mdo_type {
            MdoType::Enum => {
                items.extend(Self::enum_value_items(&mdo, prefix));
            }
            MdoType::Catalog | MdoType::Document | MdoType::ChartOfCharacteristicTypes => {
                items.extend(Self::predefined_items(&mdo, prefix));
            }
            _ => {
                // Other types: only EmptyRef
            }
        }

        tracing::debug!(
            ?mdo_type,
            object_name = %object_name,
            prefix = %prefix,
            count = items.len(),
            "CompleteValueElementsUseCase::execute: generated element completions"
        );

        items
    }

    /// Generate ПустаяСсылка (RU) or EmptyRef (EN) completion item.
    ///
    /// Returns only one variant based on context language.
    fn empty_ref_items(prefix: &str, is_russian: bool) -> Vec<SdblCompletionItem> {
        let prefix_lower = prefix.to_lowercase();
        let mut items = Vec::new();

        if is_russian {
            // ПустаяСсылка (Russian)
            if prefix.is_empty() || "пустаяссылка".starts_with(&prefix_lower) {
                items.push(
                    SdblCompletionItem::new("ПустаяСсылка", CompletionItemKind::Constant)
                        .with_detail("Пустая ссылка"),
                );
            }
        } else {
            // EmptyRef (English)
            if prefix.is_empty() || "emptyref".starts_with(&prefix_lower) {
                items.push(
                    SdblCompletionItem::new("EmptyRef", CompletionItemKind::Constant)
                        .with_detail("Empty reference"),
                );
            }
        }

        items
    }

    /// Generate enum value completion items.
    fn enum_value_items(
        mdo: &bsl_metadata::MetadataObject,
        prefix: &str,
    ) -> Vec<SdblCompletionItem> {
        let prefix_lower = prefix.to_lowercase();

        mdo.enum_values
            .iter()
            .filter(|ev| {
                prefix.is_empty()
                    || ev.name.to_lowercase().starts_with(&prefix_lower)
                    || ev
                        .name_en
                        .as_ref()
                        .is_some_and(|en| en.to_lowercase().starts_with(&prefix_lower))
            })
            .map(|ev| {
                SdblCompletionItem::new(&ev.name, CompletionItemKind::EnumMember)
                    .with_detail(format!("{}.{}", mdo.name, ev.name))
            })
            .collect()
    }

    /// Generate predefined item completion items.
    fn predefined_items(
        mdo: &bsl_metadata::MetadataObject,
        prefix: &str,
    ) -> Vec<SdblCompletionItem> {
        let prefix_lower = prefix.to_lowercase();

        mdo.predefined_items
            .iter()
            .filter(|pi| {
                prefix.is_empty()
                    || pi.name.to_lowercase().starts_with(&prefix_lower)
                    || pi
                        .name_en
                        .as_ref()
                        .is_some_and(|en| en.to_lowercase().starts_with(&prefix_lower))
            })
            .map(|pi| {
                SdblCompletionItem::new(&pi.name, CompletionItemKind::Constant).with_detail(
                    format!("{}.{}.{}", mdo.mdo_type.russian_name(), mdo.name, pi.name),
                )
            })
            .collect()
    }

    /// Find metadata object by type and name.
    fn find_metadata_object(
        config: &bsl_metadata::Configuration,
        mdo_type: MdoType,
        object_name: &str,
    ) -> Option<bsl_metadata::MetadataObject> {
        let name_lower = object_name.to_lowercase();

        config
            .metadata_objects()
            .iter()
            .find(|obj| obj.mdo_type == mdo_type && obj.name.to_lowercase() == name_lower)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::{metadata_object::EnumValue, Configuration, MetadataObject};

    struct MockMetadataProvider {
        config: Option<Configuration>,
    }

    impl MetadataProvider for MockMetadataProvider {
        fn get_configuration(&self) -> Option<std::sync::Arc<bsl_metadata::Configuration>> {
            self.config.clone().map(std::sync::Arc::new)
        }
    }

    #[test]
    fn test_execute_enum_values() {
        // Create mock configuration with enum
        let mut config = Configuration::new("Test");
        let mut enum_obj = MetadataObject::new(MdoType::Enum, "Статусы");
        enum_obj.enum_values = vec![
            EnumValue {
                name: "Активный".to_string(),
                name_en: Some("Active".to_string()),
                uuid: "uuid1".to_string(),
            },
            EnumValue {
                name: "Неактивный".to_string(),
                name_en: Some("Inactive".to_string()),
                uuid: "uuid2".to_string(),
            },
        ];
        config.add_metadata_object(enum_obj);

        let provider = MockMetadataProvider { config: Some(config) };

        // Test: get all enum values + ПустаяСсылка (Russian context)
        let items =
            CompleteValueElementsUseCase::execute(&provider, MdoType::Enum, "Статусы", "", true);

        // Should have: 2 enum values + 1 EmptyRef (Russian only)
        assert!(items.len() >= 3);

        // Check that enum values are present
        assert!(items.iter().any(|i| i.label == "Активный"));
        assert!(items.iter().any(|i| i.label == "Неактивный"));

        // Check that only Russian EmptyRef is present
        assert!(items.iter().any(|i| i.label == "ПустаяСсылка"));
        assert!(!items.iter().any(|i| i.label == "EmptyRef"));
    }

    #[test]
    fn test_execute_enum_values_with_prefix() {
        let mut config = Configuration::new("Test");
        let mut enum_obj = MetadataObject::new(MdoType::Enum, "Статусы");
        enum_obj.enum_values = vec![
            EnumValue {
                name: "Активный".to_string(), name_en: None, uuid: "uuid1".to_string()
            },
            EnumValue {
                name: "Неактивный".to_string(), name_en: None, uuid: "uuid2".to_string()
            },
        ];
        config.add_metadata_object(enum_obj);

        let provider = MockMetadataProvider { config: Some(config) };

        // Test: prefix "Акт" should match "Активный" (Russian context)
        let items =
            CompleteValueElementsUseCase::execute(&provider, MdoType::Enum, "Статусы", "Акт", true);

        assert!(items.iter().any(|i| i.label == "Активный"));
        assert!(!items.iter().any(|i| i.label == "Неактивный"));
    }

    #[test]
    fn test_execute_no_configuration() {
        let provider = MockMetadataProvider { config: None };

        // Russian context - should only have ПустаяСсылка
        let items =
            CompleteValueElementsUseCase::execute(&provider, MdoType::Enum, "Статусы", "", true);

        assert_eq!(items.len(), 1);
        assert!(items.iter().any(|i| i.label == "ПустаяСсылка"));
        assert!(!items.iter().any(|i| i.label == "EmptyRef"));

        // English context - should only have EmptyRef
        let items =
            CompleteValueElementsUseCase::execute(&provider, MdoType::Enum, "Статусы", "", false);

        assert_eq!(items.len(), 1);
        assert!(!items.iter().any(|i| i.label == "ПустаяСсылка"));
        assert!(items.iter().any(|i| i.label == "EmptyRef"));
    }

    #[test]
    fn test_empty_ref_prefix_filtering() {
        // Russian context with prefix
        let items = CompleteValueElementsUseCase::empty_ref_items("Пуст", true);
        assert!(items.iter().any(|i| i.label == "ПустаяСсылка"));
        assert!(!items.iter().any(|i| i.label == "EmptyRef"));

        // English context with prefix
        let items = CompleteValueElementsUseCase::empty_ref_items("Empty", false);
        assert!(!items.iter().any(|i| i.label == "ПустаяСсылка"));
        assert!(items.iter().any(|i| i.label == "EmptyRef"));

        // Russian context without prefix
        let items = CompleteValueElementsUseCase::empty_ref_items("", true);
        assert_eq!(items.len(), 1);
        assert!(items.iter().any(|i| i.label == "ПустаяСсылка"));

        // English context without prefix
        let items = CompleteValueElementsUseCase::empty_ref_items("", false);
        assert_eq!(items.len(), 1);
        assert!(items.iter().any(|i| i.label == "EmptyRef"));
    }
}
