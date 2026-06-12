use crate::completion::sdbl::domain::{FieldFormatter, SdblCompletionItem};
use crate::completion::CompletionItemKind;
use sdbl_hir::Scope;
use stdx::case::CaseExt;

pub struct CompleteFieldsUseCase;

impl CompleteFieldsUseCase {
    pub fn execute(scope: &Scope, alias: &str, prefix: &str) -> Vec<SdblCompletionItem> {
        let prefix_lower = prefix.fold_lower();

        let columns = scope.column_completions(Some(alias));

        tracing::info!(
            alias = %alias,
            prefix = %prefix,
            columns_count = columns.len(),
            "CompleteFieldsUseCase: got columns from scope"
        );

        columns
            .into_iter()
            .filter(|col| {
                if prefix.is_empty() {
                    true
                } else {
                    col.column_name.as_str().fold_lower().starts_with(&prefix_lower)
                }
            })
            .map(|col| {
                let field_name = col.column_name.as_str();
                let table_name = col.table_name.as_str();

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

    fn make_test_scope() -> Scope<'static> {
        let mut scope = Scope::new();

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
                field_model_complete: false,
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            range: syntax::MODULE_RANGE,
            subquery: Vec::new(),
        };

        scope.add_table(table);
        scope
    }

    #[test]
    fn test_complete_fields_no_prefix() {
        let scope = make_test_scope();
        let items = CompleteFieldsUseCase::execute(&scope, "В", "");

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

        assert_eq!(items.len(), 1);
        assert!(items.iter().any(|i| i.label == "Наименование"));
        assert!(!items.iter().any(|i| i.label == "Код"));
    }

    #[test]
    fn test_complete_fields_case_insensitive() {
        let scope = make_test_scope();
        let items_upper = CompleteFieldsUseCase::execute(&scope, "В", "КОД");
        let items_lower = CompleteFieldsUseCase::execute(&scope, "В", "код");

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

        let standard_field = items.iter().find(|i| i.label == "Код").unwrap();
        assert!(standard_field.detail.as_ref().unwrap().contains("стандартный"));

        let custom_field = items.iter().find(|i| i.label == "ПолноеНаименование").unwrap();
        assert!(!custom_field.detail.as_ref().unwrap().contains("стандартный"));
    }

    #[test]
    fn test_no_results_for_unknown_alias() {
        let scope = make_test_scope();
        let items = CompleteFieldsUseCase::execute(&scope, "НеизвестныйАлиас", "");

        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_complete_fields_for_temp_table() {
        use sdbl_hir::{FieldDef, ResolvedTable, Scope, SdblType, TableRef};
        use smol_str::SmolStr;
        use syntax::TextRange;

        let mut scope = Scope::new();

        let temp_fields = vec![
            FieldDef::new("Поле1", SdblType::string()),
            FieldDef::new("Поле2", SdblType::number()),
        ];

        scope.add_temp_table("ТемпТаблица".to_string(), temp_fields.clone());

        let table = TableRef {
            parts: vec![],
            full_name: "ТемпТаблица".to_string(),
            alias: Some(SmolStr::from("Т")),
            metadata: Some(ResolvedTable::TempTable {
                name: "ТемпТаблица".to_string(),
                fields: temp_fields,
                field_model_complete: false,
            }),
            is_virtual_table: false,
            virtual_table_params: vec![],
            range: TextRange::default(),
            subquery: Vec::new(),
        };
        scope.add_table(table);

        let items = CompleteFieldsUseCase::execute(&scope, "Т", "");

        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "Поле1"));
        assert!(items.iter().any(|i| i.label == "Поле2"));

        let field1 = items.iter().find(|i| i.label == "Поле1").unwrap();
        assert!(field1.detail.is_some());
        assert!(field1.detail.as_ref().unwrap().contains("Строка"));
    }

    #[test]
    fn test_complete_fields_for_tabular_section_in_join() {
        use bsl_metadata::MdoType;

        let mut scope = Scope::new();

        let tabular_fields = vec![
            FieldDef::standard("Ссылка", "Ref", SdblType::reference(MdoType::Document, "ЧекККМ")),
            FieldDef::new("Номенклатура", SdblType::reference(MdoType::Catalog, "Номенклатура")),
            FieldDef::new("Количество", SdblType::number()),
            FieldDef::new("Сумма", SdblType::number()),
        ];

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
                name: "Товары".to_string(),
                fields: tabular_fields,
                field_model_complete: false,
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            range: TextRange::default(),
            subquery: Vec::new(),
        };

        scope.add_table(tabular_table);

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
                field_model_complete: false,
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            range: TextRange::default(),
            subquery: Vec::new(),
        };

        scope.add_table(document_table);

        let tabular_items = CompleteFieldsUseCase::execute(&scope, "ЧекККМТовары", "");
        assert!(tabular_items.len() >= 4, "Tabular section should have at least 4 fields");

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

        let document_items = CompleteFieldsUseCase::execute(&scope, "ЧекККМ", "");
        assert!(document_items.len() >= 6, "Document should have at least 6 fields");

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

        let filtered_items = CompleteFieldsUseCase::execute(&scope, "ЧекККМТовары", "Ном");
        assert_eq!(filtered_items.len(), 1, "Should find only 'Номенклатура' with prefix 'Ном'");
        assert_eq!(filtered_items[0].label, "Номенклатура");

        let nomenclature_field = tabular_items.iter().find(|i| i.label == "Номенклатура").unwrap();
        assert!(nomenclature_field.detail.is_some(), "Nomenclature field should have detail");

        let detail = nomenclature_field.detail.as_ref().unwrap();
        assert!(
            detail.contains("Catalog") || detail.contains("Справочник") || detail.contains("Ref"),
            "Nomenclature field should show reference type, got: {}",
            detail
        );
    }

    #[test]
    fn test_complete_fields_for_temp_table_from_nested_union() {
        use bsl_metadata::MdoType;
        use sdbl_hir::{FieldDef, ResolvedTable, Scope, SdblType, TableRef};
        use smol_str::SmolStr;
        use syntax::TextRange;

        let mut scope = Scope::new();

        let temp_fields = vec![
            FieldDef::new("Наименование", SdblType::string()),
            FieldDef::new("Контрагент", SdblType::reference(MdoType::Catalog, "Контрагенты")),
        ];

        scope.add_temp_table("ВТ_Данные".to_string(), temp_fields.clone());

        let temp_table = TableRef {
            parts: vec![],
            full_name: "ВТ_Данные".to_string(),
            alias: Some(SmolStr::from("ВТ")),
            metadata: Some(ResolvedTable::TempTable {
                name: "ВТ_Данные".to_string(),
                fields: temp_fields,
                field_model_complete: false,
            }),
            is_virtual_table: false,
            virtual_table_params: vec![],
            range: TextRange::default(),
            subquery: Vec::new(),
        };
        scope.add_table(temp_table);

        let items = CompleteFieldsUseCase::execute(&scope, "ВТ", "");

        assert_eq!(items.len(), 2, "Should have 2 fields from temp table");

        assert!(
            items.iter().any(|i| i.label == "Наименование"),
            "Should have field 'Наименование'"
        );
        assert!(items.iter().any(|i| i.label == "Контрагент"), "Should have field 'Контрагент'");

        let name_field = items.iter().find(|i| i.label == "Наименование").unwrap();
        assert!(name_field.detail.is_some(), "Наименование should have detail");
        assert!(
            name_field.detail.as_ref().unwrap().contains("Строка"),
            "Наименование should show String type, got: {:?}",
            name_field.detail
        );

        let contractor_field = items.iter().find(|i| i.label == "Контрагент").unwrap();
        assert!(contractor_field.detail.is_some(), "Контрагент should have detail");

        let detail = contractor_field.detail.as_ref().unwrap();
        assert!(
            detail.contains("Справочник") || detail.contains("Catalog") || detail.contains("Ref"),
            "Контрагент should show Reference type, got: {}",
            detail
        );
        assert!(
            detail.contains("Контрагенты"),
            "Контрагент detail should contain 'Контрагенты', got: {}",
            detail
        );

        assert!(contractor_field.documentation.is_some(), "Контрагент should have documentation");

        let doc = contractor_field.documentation.as_ref().unwrap();
        assert!(
            doc.contains("Таблица: ВТ_Данные"),
            "Documentation should show temp table name, got: {}",
            doc
        );
    }

    #[test]
    fn test_complete_fields_for_temp_table_with_prefix_filter() {
        use bsl_metadata::MdoType;
        use sdbl_hir::{FieldDef, ResolvedTable, Scope, SdblType, TableRef};
        use smol_str::SmolStr;
        use syntax::TextRange;

        let mut scope = Scope::new();

        let temp_fields = vec![
            FieldDef::new("Наименование", SdblType::string()),
            FieldDef::new("НаименованиеПолное", SdblType::string()),
            FieldDef::new("Контрагент", SdblType::reference(MdoType::Catalog, "Контрагенты")),
            FieldDef::new("Код", SdblType::string()),
        ];

        scope.add_temp_table("ВТ_Данные".to_string(), temp_fields.clone());

        let temp_table = TableRef {
            parts: vec![],
            full_name: "ВТ_Данные".to_string(),
            alias: Some(SmolStr::from("ВТ")),
            metadata: Some(ResolvedTable::TempTable {
                name: "ВТ_Данные".to_string(),
                fields: temp_fields,
                field_model_complete: false,
            }),
            is_virtual_table: false,
            virtual_table_params: vec![],
            range: TextRange::default(),
            subquery: Vec::new(),
        };
        scope.add_table(temp_table);

        let items = CompleteFieldsUseCase::execute(&scope, "ВТ", "Наим");

        assert_eq!(items.len(), 2, "Should find 2 fields starting with 'Наим'");
        assert!(items.iter().any(|i| i.label == "Наименование"));
        assert!(items.iter().any(|i| i.label == "НаименованиеПолное"));
        assert!(!items.iter().any(|i| i.label == "Контрагент"));
        assert!(!items.iter().any(|i| i.label == "Код"));

        let items_ko = CompleteFieldsUseCase::execute(&scope, "ВТ", "Ко");

        assert_eq!(items_ko.len(), 2, "Should find 2 fields starting with 'Ко'");
        assert!(items_ko.iter().any(|i| i.label == "Контрагент"));
        assert!(items_ko.iter().any(|i| i.label == "Код"));

        let items_upper = CompleteFieldsUseCase::execute(&scope, "ВТ", "КОД");
        let items_lower = CompleteFieldsUseCase::execute(&scope, "ВТ", "код");

        assert_eq!(items_upper.len(), 1, "Should find 'Код' with uppercase prefix");
        assert_eq!(items_lower.len(), 1, "Should find 'Код' with lowercase prefix");
        assert_eq!(
            items_upper[0].label, items_lower[0].label,
            "Should return same field regardless of case"
        );
    }
}
