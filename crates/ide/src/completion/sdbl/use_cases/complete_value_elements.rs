use crate::completion::sdbl::domain::{MetadataProvider, SdblCompletionItem};
use crate::completion::CompletionItemKind;
use bsl_metadata::MdoType;

pub struct CompleteValueElementsUseCase;

impl CompleteValueElementsUseCase {
    pub fn execute(
        metadata: &dyn MetadataProvider,
        mdo_type: MdoType,
        object_name: &str,
        prefix: &str,
        is_russian: bool,
    ) -> Vec<SdblCompletionItem> {
        let mut items = Vec::new();

        items.extend(Self::empty_ref_items(prefix, is_russian));

        let Some(mdo) = metadata.resolve_metadata_object(mdo_type, object_name) else {
            tracing::debug!(
                ?mdo_type,
                object_name = %object_name,
                "CompleteValueElementsUseCase::execute: metadata object not found"
            );
            return items;
        };

        match mdo_type {
            MdoType::Enum => {
                items.extend(Self::enum_value_items(&mdo, prefix));
            }
            MdoType::Catalog
            | MdoType::Document
            | MdoType::ChartOfCharacteristicTypes
            | MdoType::ChartOfCalculationTypes => {
                items.extend(Self::predefined_items(&mdo, prefix));
            }
            _ => {}
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

    fn empty_ref_items(prefix: &str, is_russian: bool) -> Vec<SdblCompletionItem> {
        let prefix_lower = prefix.to_lowercase();
        let mut items = Vec::new();

        if is_russian {
            if prefix.is_empty() || "пустаяссылка".starts_with(&prefix_lower) {
                items.push(
                    SdblCompletionItem::new("ПустаяСсылка", CompletionItemKind::Constant)
                        .with_detail("Пустая ссылка"),
                );
            }
        } else {
            if prefix.is_empty() || "emptyref".starts_with(&prefix_lower) {
                items.push(
                    SdblCompletionItem::new("EmptyRef", CompletionItemKind::Constant)
                        .with_detail("Empty reference"),
                );
            }
        }

        items
    }

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

        let items =
            CompleteValueElementsUseCase::execute(&provider, MdoType::Enum, "Статусы", "", true);

        assert!(items.len() >= 3);

        assert!(items.iter().any(|i| i.label == "Активный"));
        assert!(items.iter().any(|i| i.label == "Неактивный"));

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

        let items =
            CompleteValueElementsUseCase::execute(&provider, MdoType::Enum, "Статусы", "Акт", true);

        assert!(items.iter().any(|i| i.label == "Активный"));
        assert!(!items.iter().any(|i| i.label == "Неактивный"));
    }

    #[test]
    fn test_execute_no_configuration() {
        let provider = MockMetadataProvider { config: None };

        let items =
            CompleteValueElementsUseCase::execute(&provider, MdoType::Enum, "Статусы", "", true);

        assert_eq!(items.len(), 1);
        assert!(items.iter().any(|i| i.label == "ПустаяСсылка"));
        assert!(!items.iter().any(|i| i.label == "EmptyRef"));

        let items =
            CompleteValueElementsUseCase::execute(&provider, MdoType::Enum, "Статусы", "", false);

        assert_eq!(items.len(), 1);
        assert!(!items.iter().any(|i| i.label == "ПустаяСсылка"));
        assert!(items.iter().any(|i| i.label == "EmptyRef"));
    }

    #[test]
    fn test_empty_ref_prefix_filtering() {
        let items = CompleteValueElementsUseCase::empty_ref_items("Пуст", true);
        assert!(items.iter().any(|i| i.label == "ПустаяСсылка"));
        assert!(!items.iter().any(|i| i.label == "EmptyRef"));

        let items = CompleteValueElementsUseCase::empty_ref_items("Empty", false);
        assert!(!items.iter().any(|i| i.label == "ПустаяСсылка"));
        assert!(items.iter().any(|i| i.label == "EmptyRef"));

        let items = CompleteValueElementsUseCase::empty_ref_items("", true);
        assert_eq!(items.len(), 1);
        assert!(items.iter().any(|i| i.label == "ПустаяСсылка"));

        let items = CompleteValueElementsUseCase::empty_ref_items("", false);
        assert_eq!(items.len(), 1);
        assert!(items.iter().any(|i| i.label == "EmptyRef"));
    }
}
