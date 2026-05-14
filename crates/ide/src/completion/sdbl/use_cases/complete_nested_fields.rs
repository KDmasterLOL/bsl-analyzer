//! Use case: Complete nested field references.

use crate::completion::sdbl::domain::{FieldFormatter, SdblCompletionItem};
use crate::completion::CompletionItemKind;
use sdbl_hir::{Scope, SdblType};

/// Use case for completing nested field references.
///
/// Handles completion for chains like "Alias.Field1.Field2." where Field1 has reference type.
pub struct CompleteNestedFieldsUseCase;

impl CompleteNestedFieldsUseCase {
    /// Execute the use case: get fields for nested reference chain.
    ///
    /// # Arguments
    /// - `scope`: SDBL scope with table information and metadata
    /// - `alias`: Starting table alias
    /// - `field_chain`: Chain of field names already traversed
    /// - `prefix`: Filter prefix (case-insensitive)
    ///
    /// # Returns
    /// List of matching fields from the resolved reference type
    pub fn execute(
        scope: &Scope,
        alias: &str,
        field_chain: &[String],
        prefix: &str,
    ) -> Vec<SdblCompletionItem> {
        let prefix_lower = prefix.to_lowercase();

        tracing::info!(
            alias = %alias,
            field_chain_len = field_chain.len(),
            prefix = %prefix,
            "CompleteNestedFieldsUseCase: resolving nested field type"
        );

        // Resolve type through the chain
        let final_type = scope.resolve_nested_field_type(alias, field_chain);

        if final_type.is_unknown_or_error() {
            tracing::warn!("failed to resolve nested field chain type");
            return Vec::new();
        }

        tracing::debug!(ty = ?final_type, "resolved nested field type");

        // Get fields for resolved type
        let fields = match final_type {
            SdblType::Ref(ref mdo_ref) => scope.get_fields_for_ref(mdo_ref),
            SdblType::Composite { ref types } => scope.get_fields_for_composite(types),
            SdblType::DefinedType { ref name, ref underlying_type } => {
                scope.get_fields_for_defined_type(name, underlying_type)
            }
            _ => {
                tracing::debug!(ty = ?final_type, "resolved to non-reference type");
                return Vec::new();
            }
        };

        tracing::info!(fields_count = fields.len(), "got fields from resolved type");

        // Extract table name from resolved type for completion detail
        let table_name = Self::extract_table_name(&final_type);

        // Filter by prefix and convert to SdblCompletionItem
        fields
            .into_iter()
            .filter(|field| {
                if prefix.is_empty() {
                    true
                } else {
                    field.name.to_lowercase().starts_with(&prefix_lower)
                }
            })
            .map(|field| {
                // Format field type with FieldFormatter
                let (detail, documentation) =
                    FieldFormatter::format_field_type(&field.ty, &table_name, field.is_standard);

                SdblCompletionItem::new(&field.name, CompletionItemKind::Field)
                    .with_detail(detail)
                    .with_documentation(documentation)
            })
            .collect()
    }

    /// Extract table name from resolved type for display in completion detail.
    fn extract_table_name(ty: &SdblType) -> String {
        match ty {
            SdblType::Ref(mdo_ref) => {
                format!("{}.{}", mdo_ref.mdo_type.russian_name(), mdo_ref.name)
            }
            SdblType::Composite { .. } => {
                // For composite types, always show "Составной тип"
                // Individual types are already listed in documentation
                "Составной тип".to_string()
            }
            SdblType::DefinedType { name, underlying_type } => {
                // Unwrap DefinedType to underlying type
                if let Some(underlying) = underlying_type {
                    Self::extract_table_name(underlying)
                } else {
                    format!("ОпределяемыйТип.{}", name)
                }
            }
            _ => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::MdoType;
    use sdbl_hir::{FieldDef, ResolvedTable, TableRef};
    use smol_str::SmolStr;
    use std::sync::Arc;

    fn make_test_scope_with_metadata() -> Scope {
        // Create mock Configuration with Валюты catalog
        let mut config = bsl_metadata::Configuration::new("TestConfig");
        let catalog = bsl_metadata::MetadataObject {
            mdo_type: bsl_metadata::MdoType::Catalog,
            name: "Валюты".to_string(),
            name_en: Some("Currencies".to_string()),
            attributes: vec![
                // Standard attributes (normally added by XML parser)
                bsl_metadata::Attribute {
                    name: "Ссылка".to_string(),
                    name_en: Some("Ref".to_string()),
                    attr_type: bsl_metadata::AttributeType::Ref {
                        mdo_type: bsl_metadata::MdoType::Catalog,
                        name: "Валюты".to_string(),
                    },
                },
                bsl_metadata::Attribute {
                    name: "ПометкаУдаления".to_string(),
                    name_en: Some("DeletionMark".to_string()),
                    attr_type: bsl_metadata::AttributeType::Boolean,
                },
                // Custom attributes
                bsl_metadata::Attribute {
                    name: "ОсновнаяВалюта".to_string(),
                    name_en: Some("BaseCurrency".to_string()),
                    attr_type: bsl_metadata::AttributeType::Ref {
                        mdo_type: bsl_metadata::MdoType::Catalog,
                        name: "Валюты".to_string(),
                    },
                },
                bsl_metadata::Attribute {
                    name: "Код".to_string(),
                    name_en: Some("Code".to_string()),
                    attr_type: bsl_metadata::AttributeType::String { length: Some(10) },
                },
            ],
            tabular_sections: vec![],
            children: vec![],
            enum_values: vec![],
            predefined_items: vec![],
            check_unique: false,
            code_series: bsl_metadata::CodeSeries::default(),
            constant_type: None,
            register_records: vec![],
        };
        config.add_metadata_object(catalog);

        let metadata_arc = Arc::new(config);
        let mut scope = Scope::new_with_metadata(Some(metadata_arc));

        // Add table Валюты with alias В
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
                    FieldDef::new(
                        "ОсновнаяВалюта",
                        SdblType::reference(MdoType::Catalog, "Валюты"),
                    ),
                    FieldDef::new("Код", SdblType::string()),
                ],
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
    fn test_complete_nested_single_level() {
        let scope = make_test_scope_with_metadata();

        // В.ОсновнаяВалюта. -> should show fields of Валюты
        let items =
            CompleteNestedFieldsUseCase::execute(&scope, "В", &["ОсновнаяВалюта".to_string()], "");

        // Should have standard fields: Ссылка, ПометкаУдаления
        // Plus custom: ОсновнаяВалюта, Код
        assert!(items.len() >= 2, "Expected at least 2 fields, got {}", items.len());

        // Check that we have Ссылка and Код from metadata
        assert!(items.iter().any(|i| i.label == "Ссылка"));
        assert!(items.iter().any(|i| i.label == "Код"));
    }

    #[test]
    fn test_complete_nested_with_prefix() {
        let scope = make_test_scope_with_metadata();

        // В.ОсновнаяВалюта.Ко -> should filter to fields starting with "Ко"
        let items = CompleteNestedFieldsUseCase::execute(
            &scope,
            "В",
            &["ОсновнаяВалюта".to_string()],
            "Ко",
        );

        // Should return only Код
        assert!(items.iter().any(|i| i.label == "Код"));
        assert!(!items.iter().any(|i| i.label == "Ссылка"));
    }

    #[test]
    fn test_complete_nested_case_insensitive() {
        let scope = make_test_scope_with_metadata();

        let items_upper = CompleteNestedFieldsUseCase::execute(
            &scope,
            "В",
            &["ОсновнаяВалюта".to_string()],
            "КОД",
        );
        let items_lower = CompleteNestedFieldsUseCase::execute(
            &scope,
            "В",
            &["ОсновнаяВалюта".to_string()],
            "код",
        );

        // Should return same results
        assert_eq!(items_upper.len(), items_lower.len());
    }

    #[test]
    fn test_complete_nested_unknown_alias() {
        let scope = make_test_scope_with_metadata();

        let items = CompleteNestedFieldsUseCase::execute(
            &scope,
            "НеизвестныйАлиас",
            &["Поле".to_string()],
            "",
        );

        // Should return empty
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_complete_nested_unknown_field() {
        let scope = make_test_scope_with_metadata();

        // В.НесуществующееПоле. -> type resolution should fail
        let items = CompleteNestedFieldsUseCase::execute(
            &scope,
            "В",
            &["НесуществующееПоле".to_string()],
            "",
        );

        // Should return empty
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_complete_nested_primitive_field() {
        let scope = make_test_scope_with_metadata();

        // В.Код. -> Код is String (primitive), cannot traverse further
        let items = CompleteNestedFieldsUseCase::execute(&scope, "В", &["Код".to_string()], "");

        // Should return empty because String is not traversable
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_complete_nested_shows_table_name() {
        let scope = make_test_scope_with_metadata();

        // В.ОсновнаяВалюта. -> should show fields with table name in documentation
        let items =
            CompleteNestedFieldsUseCase::execute(&scope, "В", &["ОсновнаяВалюта".to_string()], "");

        assert!(!items.is_empty(), "Expected at least one item");

        // Check that documentation contains table name
        let kod_item = items.iter().find(|i| i.label == "Код").expect("Expected Код field");

        assert!(kod_item.documentation.is_some(), "Expected documentation to be present");

        let doc = kod_item.documentation.as_ref().unwrap();
        assert!(
            doc.contains("Таблица: Справочник.Валюты"),
            "Expected documentation to contain 'Таблица: Справочник.Валюты', got: {}",
            doc
        );
    }

    #[test]
    fn test_extract_table_name_for_composite() {
        // Test that extract_table_name returns "Составной тип" for composite types
        let composite_type = SdblType::Composite {
            types: vec![
                SdblType::reference(MdoType::Catalog, "Валюты"),
                SdblType::reference(MdoType::Document, "ПриходнаяНакладная"),
            ],
        };

        let table_name = CompleteNestedFieldsUseCase::extract_table_name(&composite_type);

        assert_eq!(
            table_name, "Составной тип",
            "Expected 'Составной тип' for composite type, got: {}",
            table_name
        );
    }

    #[test]
    fn test_extract_table_name_for_ref() {
        // Test that extract_table_name returns full name for reference types
        let ref_type = SdblType::reference(MdoType::Catalog, "Валюты");

        let table_name = CompleteNestedFieldsUseCase::extract_table_name(&ref_type);

        assert_eq!(
            table_name, "Справочник.Валюты",
            "Expected 'Справочник.Валюты' for ref type, got: {}",
            table_name
        );
    }
}
