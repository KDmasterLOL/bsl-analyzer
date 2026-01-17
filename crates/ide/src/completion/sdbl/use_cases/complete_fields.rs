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
            subquery: Vec::new(),
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

    #[test]
    fn test_complete_fields_for_temp_table() {
        use sdbl_hir::{FieldDef, ResolvedTable, Scope, SdblType, TableRef};
        use smol_str::SmolStr;
        use syntax::TextRange;

        // Create scope with temporary table
        let mut scope = Scope::new();

        let temp_fields = vec![
            FieldDef::new("Поле1", SdblType::string()),
            FieldDef::new("Поле2", SdblType::number()),
        ];

        scope.add_temp_table("ТемпТаблица".to_string(), temp_fields.clone());

        // Create TableRef pointing to temp table
        let table = TableRef {
            parts: vec![],
            full_name: "ТемпТаблица".to_string(),
            alias: Some(SmolStr::from("Т")),
            metadata: Some(ResolvedTable::TempTable {
                name: "ТемпТаблица".to_string(),
                fields: temp_fields,
            }),
            is_virtual_table: false,
            virtual_table_params: vec![],
            range: TextRange::default(),
            subquery: Vec::new(),
        };
        scope.add_table(table);

        // Test completion
        let items = CompleteFieldsUseCase::execute(&scope, "Т", "");

        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "Поле1"));
        assert!(items.iter().any(|i| i.label == "Поле2"));

        // Check that types are preserved
        let field1 = items.iter().find(|i| i.label == "Поле1").unwrap();
        assert!(field1.detail.is_some());
        assert!(field1.detail.as_ref().unwrap().contains("Строка"));
    }

    #[test]
    fn test_complete_fields_for_tabular_section_in_join() {
        use bsl_metadata::MdoType;

        // Создаём scope с табличной частью и основным документом
        let mut scope = Scope::new();

        // Поля табличной части: Документ.ЧекККМ.Товары
        // (Ссылка - стандартное поле + атрибуты табличной части)
        let tabular_fields = vec![
            FieldDef::standard("Ссылка", "Ref", SdblType::reference(MdoType::Document, "ЧекККМ")),
            FieldDef::new("Номенклатура", SdblType::reference(MdoType::Catalog, "Номенклатура")),
            FieldDef::new("Количество", SdblType::number()),
            FieldDef::new("Сумма", SdblType::number()),
        ];

        // Добавляем табличную часть как таблицу в scope
        // Используем Metadata вариант (как это делает текущий код)
        let tabular_table = TableRef {
            parts: vec![
                SmolStr::from("Документ"),
                SmolStr::from("ЧекККМ"),
                SmolStr::from("Товары"),
            ],
            full_name: "Документ.ЧекККМ.Товары".to_string(),
            alias: Some(SmolStr::from("ЧекККМТовары")),
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Document,
                name: "Товары".to_string(), // Имя табличной части
                fields: tabular_fields,
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            range: TextRange::default(),
            subquery: Vec::new(),
        };

        scope.add_table(tabular_table);

        // Основной документ: Документ.ЧекККМ
        let document_fields = vec![
            FieldDef::standard("Ссылка", "Ref", SdblType::reference(MdoType::Document, "ЧекККМ")),
            FieldDef::standard("Номер", "Number", SdblType::string()),
            FieldDef::standard("Дата", "Date", SdblType::Date),
            FieldDef::new("Партнер", SdblType::reference(MdoType::Catalog, "Контрагенты")),
            FieldDef::new("Склад", SdblType::reference(MdoType::Catalog, "Склады")),
            FieldDef::new("Проведен", SdblType::Boolean),
        ];

        let document_table = TableRef {
            parts: vec![SmolStr::from("Документ"), SmolStr::from("ЧекККМ")],
            full_name: "Документ.ЧекККМ".to_string(),
            alias: Some(SmolStr::from("ЧекККМ")),
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Document,
                name: "ЧекККМ".to_string(),
                fields: document_fields,
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            range: TextRange::default(),
            subquery: Vec::new(),
        };

        scope.add_table(document_table);

        // Проверяем автодополнение для табличной части (ЧекККМТовары.↓)
        let tabular_items = CompleteFieldsUseCase::execute(&scope, "ЧекККМТовары", "");
        assert!(tabular_items.len() >= 4, "Tabular section should have at least 4 fields");

        // Проверяем наличие полей табличной части
        assert!(
            tabular_items.iter().any(|i| i.label == "Ссылка"),
            "Tabular section should have Ссылка field"
        );
        assert!(
            tabular_items.iter().any(|i| i.label == "Номенклатура"),
            "Tabular section should have Номенклатура field"
        );
        assert!(
            tabular_items.iter().any(|i| i.label == "Количество"),
            "Tabular section should have Количество field"
        );
        assert!(
            tabular_items.iter().any(|i| i.label == "Сумма"),
            "Tabular section should have Сумма field"
        );

        // Проверяем автодополнение для основного документа (ЧекККМ.↓)
        let document_items = CompleteFieldsUseCase::execute(&scope, "ЧекККМ", "");
        assert!(document_items.len() >= 6, "Document should have at least 6 fields");

        // Проверяем наличие полей документа
        assert!(
            document_items.iter().any(|i| i.label == "Ссылка"),
            "Document should have Ссылка field"
        );
        assert!(
            document_items.iter().any(|i| i.label == "Номер"),
            "Document should have Номер field"
        );
        assert!(
            document_items.iter().any(|i| i.label == "Дата"),
            "Document should have Дата field"
        );
        assert!(
            document_items.iter().any(|i| i.label == "Партнер"),
            "Document should have Партнер field"
        );
        assert!(
            document_items.iter().any(|i| i.label == "Проведен"),
            "Document should have Проведен field"
        );

        // Проверяем фильтрацию с префиксом (как в ON clause: ЧекККМТовары.Ном↓)
        let filtered_items = CompleteFieldsUseCase::execute(&scope, "ЧекККМТовары", "Ном");
        assert_eq!(filtered_items.len(), 1, "Should find only 'Номенклатура' with prefix 'Ном'");
        assert_eq!(filtered_items[0].label, "Номенклатура");

        // Проверяем, что типы корректно отображаются
        let nomenclature_field = tabular_items.iter().find(|i| i.label == "Номенклатура").unwrap();
        assert!(nomenclature_field.detail.is_some(), "Nomenclature field should have detail");

        // Проверяем, что detail содержит информацию о типе
        let detail = nomenclature_field.detail.as_ref().unwrap();
        assert!(
            detail.contains("Catalog") || detail.contains("Справочник") || detail.contains("Ref"),
            "Nomenclature field should show reference type, got: {}",
            detail
        );
    }
}
