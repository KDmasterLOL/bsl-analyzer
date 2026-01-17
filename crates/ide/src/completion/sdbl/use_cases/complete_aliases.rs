//! Use case: Complete table aliases.

use crate::completion::sdbl::domain::SdblCompletionItem;
use crate::completion::CompletionItemKind;
use sdbl_hir::Scope;

/// Use case for completing table aliases.
///
/// Handles two scenarios:
/// 1. List available table aliases from query scope (for ON/ПО clauses)
/// 2. Suggest alias name after AS/КАК keyword
pub struct CompleteAliasesUseCase;

impl CompleteAliasesUseCase {
    /// Execute the use case: get available table aliases from scope.
    ///
    /// Returns all table aliases defined in the query (FROM and JOIN clauses),
    /// filtered by prefix.
    ///
    /// # Arguments
    /// - `scope`: SDBL scope with table information
    /// - `prefix`: Filter prefix (case-insensitive)
    ///
    /// # Returns
    /// List of table alias completion items
    pub fn execute_table_aliases(scope: &Scope, prefix: &str) -> Vec<SdblCompletionItem> {
        let prefix_lower = prefix.to_lowercase();

        // Get all tables with aliases from scope
        scope
            .all_tables()
            .filter_map(|table| {
                if let Some(ref alias) = table.alias {
                    // Filter by prefix
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

    /// Execute the use case: suggest alias name after AS/КАК keyword.
    ///
    /// Returns a single completion item with the suggested alias name,
    /// extracted from context (field name or table name).
    ///
    /// # Arguments
    /// - `suggestion`: Optional suggested alias name (e.g., "Код", "Номенклатура")
    ///
    /// # Returns
    /// Vec with 0-1 completion item containing the suggested alias
    pub fn execute_alias_suggestion(suggestion: Option<String>) -> Vec<SdblCompletionItem> {
        if let Some(alias) = suggestion {
            vec![SdblCompletionItem::new(&alias, CompletionItemKind::Keyword)
                .with_detail("Предлагаемый псевдоним")
                .with_documentation("Псевдоним на основе имени поля или таблицы")]
        } else {
            // No suggestion available - return empty
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
    use syntax::TextRange;

    fn make_test_scope_with_aliases() -> Scope {
        let mut scope = Scope::new();

        // Add table with alias "Т"
        let table1 = TableRef {
            parts: vec![SmolStr::from("Справочник"), SmolStr::from("Валюты")],
            full_name: "Справочник.Валюты".to_string(),
            alias: Some(SmolStr::from("Т")),
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Catalog,
                name: "Валюты".to_string(),
                fields: Vec::new(),
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            range: TextRange::empty(0.into()),
            subquery: None,
        };

        // Add table with alias "Т1"
        let table2 = TableRef {
            parts: vec![SmolStr::from("Справочник"), SmolStr::from("Контрагенты")],
            full_name: "Справочник.Контрагенты".to_string(),
            alias: Some(SmolStr::from("Т1")),
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Catalog,
                name: "Контрагенты".to_string(),
                fields: Vec::new(),
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            range: TextRange::empty(0.into()),
            subquery: None,
        };

        // Add table WITHOUT alias
        let table3 = TableRef {
            parts: vec![SmolStr::from("Документ"), SmolStr::from("Продажа")],
            full_name: "Документ.Продажа".to_string(),
            alias: None,
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Document,
                name: "Продажа".to_string(),
                fields: Vec::new(),
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            range: TextRange::empty(0.into()),
            subquery: None,
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

        // Should return both aliases (Т and Т1), but not the table without alias
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "Т"));
        assert!(items.iter().any(|i| i.label == "Т1"));
    }

    #[test]
    fn test_complete_table_aliases_with_prefix() {
        let scope = make_test_scope_with_aliases();
        let items = CompleteAliasesUseCase::execute_table_aliases(&scope, "Т1");

        // Should return only Т1
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Т1");
    }

    #[test]
    fn test_complete_table_aliases_case_insensitive() {
        let scope = make_test_scope_with_aliases();
        let items_upper = CompleteAliasesUseCase::execute_table_aliases(&scope, "Т");
        let items_lower = CompleteAliasesUseCase::execute_table_aliases(&scope, "т");

        // Should return same results (both Т and Т1 start with Т/т)
        assert_eq!(items_upper.len(), items_lower.len());
        assert_eq!(items_upper.len(), 2);
    }

    #[test]
    fn test_complete_table_aliases_detail_format() {
        let scope = make_test_scope_with_aliases();
        let items = CompleteAliasesUseCase::execute_table_aliases(&scope, "Т");

        // Find alias "Т"
        let item = items.iter().find(|i| i.label == "Т").unwrap();

        // Should have detail and documentation with table full name
        assert!(item.detail.as_ref().unwrap().contains("Справочник.Валюты"));
        assert!(item.documentation.as_ref().unwrap().contains("Справочник.Валюты"));
    }

    #[test]
    fn test_complete_table_aliases_no_match() {
        let scope = make_test_scope_with_aliases();
        let items = CompleteAliasesUseCase::execute_table_aliases(&scope, "ЗЗЗ");

        // Should return empty for non-matching prefix
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

        // Should return empty
        assert_eq!(items.len(), 0);
    }
}
