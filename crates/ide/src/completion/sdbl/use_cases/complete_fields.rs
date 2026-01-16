//! Use case: Complete table fields by alias.

use crate::completion::sdbl::domain::{FieldFormatter, SdblCompletionItem};
use crate::completion::CompletionItemKind;
use sdbl_hir::Scope;

/// Use case for completing table fields by alias.
///
/// Returns fields/columns for a table alias in SDBL query.
pub struct CompleteFieldsUseCase;

impl CompleteFieldsUseCase {
    /// Execute the use case: get fields for table alias.
    ///
    /// # Arguments
    /// - `scope`: SDBL scope with table information
    /// - `alias`: Table alias to get fields for
    /// - `prefix`: Filter prefix (case-insensitive)
    ///
    /// # Returns
    /// List of matching table fields
    pub fn execute(scope: &Scope, alias: &str, prefix: &str) -> Vec<SdblCompletionItem> {
        let prefix_lower = prefix.to_lowercase();

        // Get column completions from scope for the specified alias
        let columns = scope.column_completions(Some(alias));

        tracing::info!(
            alias = %alias,
            prefix = %prefix,
            columns_count = columns.len(),
            "CompleteFieldsUseCase: got columns from scope"
        );

        // Convert to SdblCompletionItem and filter by prefix
        columns
            .into_iter()
            .filter(|col| {
                if prefix.is_empty() {
                    true
                } else {
                    col.column_name.as_str().to_lowercase().starts_with(&prefix_lower)
                }
            })
            .map(|col| {
                let field_name = col.column_name.as_str();
                let table_name = col.table_name.as_str();

                // Format field type
                let (detail, documentation) =
                    FieldFormatter::format_field_type(&col.ty, table_name, col.is_standard);

                SdblCompletionItem::new(field_name, CompletionItemKind::Field)
                    .with_detail(detail)
                    .with_documentation(documentation)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::MdoType;
    use sdbl_hir::SdblType;
    use sdbl_hir::{FieldDef, ResolvedTable, Scope, TableRef};
    use smol_str::SmolStr;
    use syntax::TextRange;

    fn make_test_scope() -> Scope {
        let mut scope = Scope::new();

        // Add test table with some fields
        let table = TableRef {
            parts: vec![SmolStr::from("Справочник"), SmolStr::from("Валюты")],
            full_name: "Справочник.Валюты".to_string(),
            alias: Some(SmolStr::from("В")),
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Catalog,
                name: "Валюты".to_string(),
                fields: vec![
                    FieldDef::standard(
                        "Ссылка",
                        "Ref",
                        SdblType::reference(MdoType::Catalog, "Валюты"),
                    ),
                    FieldDef::standard("Код", "Code", SdblType::string()),
                    FieldDef::standard("Наименование", "Description", SdblType::string()),
                    FieldDef::new("ПолноеНаименование", SdblType::string()),
                ],
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            range: TextRange::empty(0.into()),
        };

        scope.add_table(table);
        scope
    }

    #[test]
    fn test_complete_fields_no_prefix() {
        let scope = make_test_scope();
        let items = CompleteFieldsUseCase::execute(&scope, "В", "");

        // Should return all fields
        assert_eq!(items.len(), 4);
        assert!(items.iter().any(|i| i.label == "Ссылка"));
        assert!(items.iter().any(|i| i.label == "Код"));
        assert!(items.iter().any(|i| i.label == "Наименование"));
        assert!(items.iter().any(|i| i.label == "ПолноеНаименование"));
    }

    #[test]
    fn test_complete_fields_with_prefix() {
        let scope = make_test_scope();
        let items = CompleteFieldsUseCase::execute(&scope, "В", "Наим");

        // Should return only fields starting with "Наим"
        assert_eq!(items.len(), 1);
        assert!(items.iter().any(|i| i.label == "Наименование"));
        assert!(!items.iter().any(|i| i.label == "Код"));
    }

    #[test]
    fn test_complete_fields_case_insensitive() {
        let scope = make_test_scope();
        let items_upper = CompleteFieldsUseCase::execute(&scope, "В", "КОД");
        let items_lower = CompleteFieldsUseCase::execute(&scope, "В", "код");

        // Should return same results
        assert_eq!(items_upper.len(), items_lower.len());
        assert_eq!(items_upper.len(), 1);
    }

    #[test]
    fn test_field_has_detail_and_documentation() {
        let scope = make_test_scope();
        let items = CompleteFieldsUseCase::execute(&scope, "В", "Код");

        assert_eq!(items.len(), 1);
        let item = &items[0];

        assert_eq!(item.label, "Код");
        assert!(item.detail.is_some());
        assert!(item.documentation.is_some());
        assert_eq!(item.kind, CompletionItemKind::Field);
    }

    #[test]
    fn test_standard_field_marker() {
        let scope = make_test_scope();
        let items = CompleteFieldsUseCase::execute(&scope, "В", "");

        // Find standard field
        let standard_field = items.iter().find(|i| i.label == "Код").unwrap();
        // Standard fields should have "(стандартный)" marker
        assert!(standard_field.detail.as_ref().unwrap().contains("стандартный"));

        // Find custom field
        let custom_field = items.iter().find(|i| i.label == "ПолноеНаименование").unwrap();
        // Custom fields should NOT have marker
        assert!(!custom_field.detail.as_ref().unwrap().contains("стандартный"));
    }

    #[test]
    fn test_no_results_for_unknown_alias() {
        let scope = make_test_scope();
        let items = CompleteFieldsUseCase::execute(&scope, "НеизвестныйАлиас", "");

        // Should return empty for unknown alias
        assert_eq!(items.len(), 0);
    }
}
