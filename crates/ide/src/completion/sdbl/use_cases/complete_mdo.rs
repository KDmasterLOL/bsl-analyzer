use crate::completion::sdbl::domain::{MetadataProvider, SdblCompletionItem};
use crate::completion::CompletionItemKind;
use bsl_metadata::MdoType;
use stdx::case::CaseExt;

pub struct CompleteMdoUseCase;

impl CompleteMdoUseCase {
    pub fn execute_types(prefix: &str) -> Vec<SdblCompletionItem> {
        let prefix_lower = prefix.fold_lower();

        let mut items = Vec::new();

        for &mdo_type in MdoType::all() {
            let russian_name = mdo_type.russian_name();
            let english_name = mdo_type.english_name();

            if prefix.is_empty() || russian_name.fold_lower().starts_with(&prefix_lower) {
                items.push(SdblCompletionItem::new(russian_name, CompletionItemKind::MdoType));
            }

            if prefix.is_empty() || english_name.fold_lower().starts_with(&prefix_lower) {
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

    pub fn execute_objects(
        metadata: &dyn MetadataProvider,
        mdo_type: MdoType,
        prefix: &str,
    ) -> Vec<SdblCompletionItem> {
        let Some(config) = metadata.get_configuration() else {
            tracing::debug!(
                ?mdo_type,
                "CompleteMdoUseCase::execute_objects: no configuration available"
            );
            return Vec::new();
        };

        let prefix_lower = prefix.fold_lower();

        let objects = Self::get_objects_by_type(&config, mdo_type);

        let items: Vec<SdblCompletionItem> = objects
            .iter()
            .filter(|obj| {
                if prefix.is_empty() {
                    true
                } else {
                    obj.name.fold_lower().starts_with(&prefix_lower)
                        || obj
                            .name_en
                            .as_ref()
                            .is_some_and(|en| en.fold_lower().starts_with(&prefix_lower))
                }
            })
            .map(|obj| {
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

    fn get_objects_by_type(
        config: &bsl_metadata::Configuration,
        mdo_type: MdoType,
    ) -> Vec<bsl_metadata::MetadataObject> {
        if matches!(
            mdo_type,
            MdoType::InformationRegister
                | MdoType::AccumulationRegister
                | MdoType::AccountingRegister
                | MdoType::CalculationRegister
        ) {
            return config
                .registers()
                .iter()
                .filter(|reg| reg.mdo_type() == mdo_type)
                .map(|reg| bsl_metadata::MetadataObject::new(mdo_type, reg.name()))
                .collect();
        }

        config.metadata_objects().iter().filter(|obj| obj.mdo_type == mdo_type).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::Configuration;
    use std::sync::Arc;

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

        assert!(items.len() >= 40);

        assert!(items.iter().any(|i| i.label == "Справочник"));
        assert!(items.iter().any(|i| i.label == "Catalog"));
        assert!(items.iter().any(|i| i.label == "Документ"));
        assert!(items.iter().any(|i| i.label == "Document"));
    }

    #[test]
    fn test_complete_mdo_types_with_prefix_russian() {
        let items = CompleteMdoUseCase::execute_types("Спр");

        assert!(items.iter().any(|i| i.label == "Справочник"));
        assert!(!items.iter().any(|i| i.label == "Документ"));
        assert!(!items.iter().any(|i| i.label == "Catalog"));
    }

    #[test]
    fn test_complete_mdo_types_with_prefix_english() {
        let items = CompleteMdoUseCase::execute_types("Cat");

        assert!(items.iter().any(|i| i.label == "Catalog"));
        assert!(!items.iter().any(|i| i.label == "Справочник"));
        assert!(!items.iter().any(|i| i.label == "Document"));
    }

    #[test]
    fn test_complete_mdo_types_case_insensitive() {
        let items_upper = CompleteMdoUseCase::execute_types("СПРА");
        let items_lower = CompleteMdoUseCase::execute_types("спра");

        assert_eq!(items_upper.len(), items_lower.len());
        assert!(!items_upper.is_empty());
    }

    #[test]
    fn test_complete_mdo_objects_no_metadata() {
        let provider = MockMetadataProvider { config: None };
        let items = CompleteMdoUseCase::execute_objects(&provider, MdoType::Catalog, "");

        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_complete_mdo_objects_catalogs_no_prefix() {
        let config = make_test_configuration();
        let provider = MockMetadataProvider { config: Some(Arc::new(config)) };

        let items = CompleteMdoUseCase::execute_objects(&provider, MdoType::Catalog, "");

        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "Валюты"));
        assert!(items.iter().any(|i| i.label == "Контрагенты"));
    }

    #[test]
    fn test_complete_mdo_objects_with_prefix_russian() {
        let config = make_test_configuration();
        let provider = MockMetadataProvider { config: Some(Arc::new(config)) };

        let items = CompleteMdoUseCase::execute_objects(&provider, MdoType::Catalog, "Вал");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Валюты");
    }

    #[test]
    fn test_complete_mdo_objects_with_prefix_english() {
        let config = make_test_configuration();
        let provider = MockMetadataProvider { config: Some(Arc::new(config)) };

        let items = CompleteMdoUseCase::execute_objects(&provider, MdoType::Catalog, "Curr");

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

        assert_eq!(item.detail, Some("Справочник.Валюты".to_string()));
        assert_eq!(item.kind, CompletionItemKind::MdoObject);
    }

    #[test]
    fn test_complete_mdo_objects_filter_by_type() {
        let config = make_test_configuration();
        let provider = MockMetadataProvider { config: Some(Arc::new(config)) };

        let items = CompleteMdoUseCase::execute_objects(&provider, MdoType::Document, "");

        assert_eq!(items.len(), 1);
        assert!(items.iter().any(|i| i.label == "ПриходнаяНакладная"));
        assert!(!items.iter().any(|i| i.label == "Валюты"));
    }
}
