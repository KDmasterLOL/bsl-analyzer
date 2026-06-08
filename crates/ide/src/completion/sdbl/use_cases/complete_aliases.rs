use crate::completion::sdbl::domain::SdblCompletionItem;
use crate::completion::CompletionItemKind;
use sdbl_hir::Scope;

pub struct CompleteAliasesUseCase;

impl CompleteAliasesUseCase {
    pub fn execute_table_aliases(scope: &Scope, prefix: &str) -> Vec<SdblCompletionItem> {
        let prefix_lower = prefix.to_lowercase();

        scope
            .all_tables()
            .filter_map(|table| {
                if let Some(ref alias) = table.alias {
                    if prefix.is_empty() || alias.to_lowercase().starts_with(&prefix_lower) {
                        let detail = format!("Псевдоним для {}", table.full_name);
                        let documentation = format!("Псевдоним таблицы {}", table.full_name);

                        Some(
                            SdblCompletionItem::new(alias.as_str(), CompletionItemKind::Keyword)
                                .with_detail(detail)
                                .with_documentation(documentation),
                        )
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn execute_alias_suggestion(suggestion: Option<String>) -> Vec<SdblCompletionItem> {
        if let Some(alias) = suggestion {
            vec![SdblCompletionItem::new(&alias, CompletionItemKind::Keyword)
                .with_detail("Предлагаемый псевдоним")
                .with_documentation("Псевдоним на основе имени поля или таблицы")]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::MdoType;
    use sdbl_hir::{ResolvedTable, TableRef};
    use smol_str::SmolStr;

    fn make_test_scope_with_aliases() -> Scope<'static> {
        let mut scope = Scope::new();

        let table1 = TableRef {
            parts: vec![SmolStr::from("Справочник"), SmolStr::from("Валюты")],
            full_name: "Справочник.Валюты".to_string(),
            alias: Some(SmolStr::from("Т")),
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Catalog,
                name: "Валюты".to_string(),
                fields: Vec::new(),
                field_model_complete: false,
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            range: syntax::MODULE_RANGE,
            subquery: Vec::new(),
        };

        let table2 = TableRef {
            parts: vec![SmolStr::from("Справочник"), SmolStr::from("Контрагенты")],
            full_name: "Справочник.Контрагенты".to_string(),
            alias: Some(SmolStr::from("Т1")),
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Catalog,
                name: "Контрагенты".to_string(),
                fields: Vec::new(),
                field_model_complete: false,
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            range: syntax::MODULE_RANGE,
            subquery: Vec::new(),
        };

        let table3 = TableRef {
            parts: vec![SmolStr::from("Документ"), SmolStr::from("Продажа")],
            full_name: "Документ.Продажа".to_string(),
            alias: None,
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Document,
                name: "Продажа".to_string(),
                fields: Vec::new(),
                field_model_complete: false,
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            range: syntax::MODULE_RANGE,
            subquery: Vec::new(),
        };

        scope.add_table(table1);
        scope.add_table(table2);
        scope.add_table(table3);

        scope
    }

    #[test]
    fn test_complete_table_aliases_no_prefix() {
        let scope = make_test_scope_with_aliases();
        let items = CompleteAliasesUseCase::execute_table_aliases(&scope, "");

        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "Т"));
        assert!(items.iter().any(|i| i.label == "Т1"));
    }

    #[test]
    fn test_complete_table_aliases_with_prefix() {
        let scope = make_test_scope_with_aliases();
        let items = CompleteAliasesUseCase::execute_table_aliases(&scope, "Т1");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Т1");
    }

    #[test]
    fn test_complete_table_aliases_case_insensitive() {
        let scope = make_test_scope_with_aliases();
        let items_upper = CompleteAliasesUseCase::execute_table_aliases(&scope, "Т");
        let items_lower = CompleteAliasesUseCase::execute_table_aliases(&scope, "т");

        assert_eq!(items_upper.len(), items_lower.len());
        assert_eq!(items_upper.len(), 2);
    }

    #[test]
    fn test_complete_table_aliases_detail_format() {
        let scope = make_test_scope_with_aliases();
        let items = CompleteAliasesUseCase::execute_table_aliases(&scope, "Т");

        let item = items.iter().find(|i| i.label == "Т").unwrap();

        assert!(item.detail.as_ref().unwrap().contains("Справочник.Валюты"));
        assert!(item.documentation.as_ref().unwrap().contains("Справочник.Валюты"));
    }

    #[test]
    fn test_complete_table_aliases_no_match() {
        let scope = make_test_scope_with_aliases();
        let items = CompleteAliasesUseCase::execute_table_aliases(&scope, "ЗЗЗ");

        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_complete_alias_suggestion_with_value() {
        let items = CompleteAliasesUseCase::execute_alias_suggestion(Some("Код".to_string()));

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Код");
        assert!(items[0].detail.is_some());
        assert!(items[0].documentation.is_some());
    }

    #[test]
    fn test_complete_alias_suggestion_none() {
        let items = CompleteAliasesUseCase::execute_alias_suggestion(None);

        assert_eq!(items.len(), 0);
    }
}
