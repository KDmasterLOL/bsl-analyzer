//! Use case: Complete fields for CAST expression types.

use crate::completion::sdbl::domain::{FieldFormatter, MetadataProvider, SdblCompletionItem};
use crate::completion::CompletionItemKind;
use bsl_metadata::MdoType;
use sdbl_hir::{FieldDef, MdoRef, Scope, SdblType};

/// Use case for completing fields after CAST expression.
///
/// Handles completion for patterns like "ВЫРАЗИТЬ(X КАК Документ.Продажа)."
/// where we need to suggest fields of the specified MDO type.
pub struct CompleteCastFieldsUseCase;

impl CompleteCastFieldsUseCase {
    /// Execute the use case: get fields for CAST target type.
    ///
    /// # Arguments
    /// - `scope`: Optional SDBL scope (for metadata and field resolution)
    /// - `metadata_provider`: Fallback metadata provider if scope unavailable
    /// - `mdo_type`: MDO type from CAST expression
    /// - `object_name`: Object name from CAST expression
    /// - `field_chain`: Chain of field names after CAST (may be empty)
    /// - `prefix`: Filter prefix (case-insensitive)
    ///
    /// # Returns
    /// List of matching fields from the CAST target type
    pub fn execute<M: MetadataProvider>(
        scope: Option<&Scope>,
        _metadata_provider: &M,
        mdo_type: MdoType,
        object_name: &str,
        field_chain: &[String],
        prefix: &str,
    ) -> Vec<SdblCompletionItem> {
        let prefix_lower = prefix.to_lowercase();

        tracing::info!(
            ?mdo_type,
            object_name = %object_name,
            field_chain_len = field_chain.len(),
            prefix = %prefix,
            "CompleteCastFieldsUseCase: resolving CAST target type"
        );

        // Create MdoRef for the CAST target type
        let base_mdo_ref = MdoRef { mdo_type, name: object_name.to_string() };

        // If we have a scope, use it for field resolution (handles metadata + standard fields)
        let Some(scope) = scope else {
            tracing::warn!("no scope available for CAST field completion");
            return Vec::new();
        };

        // Get fields for base type first
        let base_fields = scope.get_fields_for_ref(&base_mdo_ref);
        if base_fields.is_empty() {
            tracing::warn!("no fields found for CAST target type");
            return Vec::new();
        }

        // If there's a field chain, resolve through it
        let (final_type, final_fields) = if field_chain.is_empty() {
            (SdblType::Ref(base_mdo_ref), base_fields)
        } else {
            // Walk through the chain to resolve final type
            match Self::resolve_field_chain(scope, base_fields, field_chain) {
                Some((ty, fields)) => (ty, fields),
                None => {
                    tracing::warn!("failed to resolve CAST field chain");
                    return Vec::new();
                }
            }
        };

        tracing::info!(fields_count = final_fields.len(), "got fields from CAST target type");

        // Extract table name for completion detail
        let table_name = Self::extract_table_name(&final_type);

        // Filter by prefix and convert to SdblCompletionItem
        final_fields
            .into_iter()
            .filter(|field| {
                if prefix.is_empty() {
                    true
                } else {
                    field.name.to_lowercase().starts_with(&prefix_lower)
                }
            })
            .map(|field| {
                let (detail, documentation) =
                    FieldFormatter::format_field_type(&field.ty, &table_name, field.is_standard);

                SdblCompletionItem::new(&field.name, CompletionItemKind::Field)
                    .with_detail(detail)
                    .with_documentation(documentation)
            })
            .collect()
    }

    /// Resolve field chain starting from given fields.
    ///
    /// For each field in chain, finds the field type and resolves its fields.
    fn resolve_field_chain(
        scope: &Scope,
        mut current_fields: Vec<FieldDef>,
        field_chain: &[String],
    ) -> Option<(SdblType, Vec<FieldDef>)> {
        let mut current_type = SdblType::Unknown;

        for field_name in field_chain {
            // Find field in current context
            let field = current_fields.iter().find(|f| f.matches_name(field_name))?;

            current_type = field.ty.clone();

            // Resolve next level based on type
            current_fields = match &current_type {
                SdblType::Ref(mdo_ref) => scope.get_fields_for_ref(mdo_ref),
                SdblType::Composite { types } => scope.get_fields_for_composite(types),
                SdblType::DefinedType { name, underlying_type } => {
                    scope.get_fields_for_defined_type(name, underlying_type)
                }
                _ => {
                    // Primitive type - cannot traverse further
                    return None;
                }
            };

            if current_fields.is_empty() {
                return None;
            }
        }

        Some((current_type, current_fields))
    }

    /// Extract table name from resolved type for display.
    fn extract_table_name(ty: &SdblType) -> String {
        match ty {
            SdblType::Ref(mdo_ref) => {
                format!("{}.{}", mdo_ref.mdo_type.russian_name(), mdo_ref.name)
            }
            SdblType::Composite { .. } => "Составной тип".to_string(),
            SdblType::DefinedType { name, underlying_type } => {
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
    use crate::completion::sdbl::domain::MetadataProvider;
    use bsl_metadata::Configuration;
    use sdbl_hir::{FieldDef, ResolvedTable, TableRef};
    use smol_str::SmolStr;
    use std::sync::Arc;
    use syntax::TextRange;

    struct TestMetadataProvider(Option<Arc<Configuration>>);

    impl MetadataProvider for TestMetadataProvider {
        fn get_configuration(&self) -> Option<Arc<Configuration>> {
            self.0.clone()
        }
    }

    fn make_test_scope_with_document() -> Scope {
        // Create mock Configuration with a Document type
        let mut config = Configuration::new("TestConfig");
        let document = bsl_metadata::MetadataObject {
            mdo_type: MdoType::Document,
            name: "НачислениеИСписаниеБонусныхБаллов".to_string(),
            name_en: Some("BonusPointsAccrualAndWriteOff".to_string()),
            attributes: vec![
                bsl_metadata::Attribute {
                    name: "ПричинаНачисленияИСписанияБонусныхБаллов".to_string(),
                    name_en: Some("BonusPointsAccrualAndWriteOffReason".to_string()),
                    attr_type: bsl_metadata::AttributeType::Ref {
                        mdo_type: MdoType::Catalog,
                        name: "ПричиныНачисленияИСписанияБонусныхБаллов".to_string(),
                    },
                },
                bsl_metadata::Attribute {
                    name: "Контрагент".to_string(),
                    name_en: Some("Counterparty".to_string()),
                    attr_type: bsl_metadata::AttributeType::Ref {
                        mdo_type: MdoType::Catalog,
                        name: "Контрагенты".to_string(),
                    },
                },
            ],
            tabular_sections: vec![],
            children: vec![],
            enum_values: vec![],
            predefined_items: vec![],
            check_unique: false,
            code_series: bsl_metadata::CodeSeries::default(),
        };
        config.add_metadata_object(document);

        let metadata_arc = Arc::new(config);
        let mut scope = Scope::new_with_metadata(Some(metadata_arc));

        // Add a dummy table so the scope has some context
        let table = TableRef {
            parts: vec![SmolStr::from("Справочник"), SmolStr::from("Валюты")],
            full_name: "Справочник.Валюты".to_string(),
            alias: Some(SmolStr::from("Т")),
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Catalog,
                name: "Валюты".to_string(),
                fields: vec![
                    FieldDef::standard(
                        "Ссылка",
                        "Ref",
                        SdblType::reference(MdoType::Catalog, "Валюты"),
                    ),
                    FieldDef::new("Код", SdblType::string()),
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
    fn test_complete_cast_direct_fields() {
        let scope = make_test_scope_with_document();
        let provider = TestMetadataProvider(None);

        // ВЫРАЗИТЬ(... КАК Документ.НачислениеИСписаниеБонусныхБаллов).
        let items = CompleteCastFieldsUseCase::execute(
            Some(&scope),
            &provider,
            MdoType::Document,
            "НачислениеИСписаниеБонусныхБаллов",
            &[],
            "",
        );

        // Should have fields from the document
        assert!(!items.is_empty(), "Expected fields from Document type");

        // Check for custom attribute
        assert!(
            items.iter().any(|i| i.label == "ПричинаНачисленияИСписанияБонусныхБаллов"),
            "Expected ПричинаНачисленияИСписанияБонусныхБаллов field"
        );
    }

    #[test]
    fn test_complete_cast_with_prefix() {
        let scope = make_test_scope_with_document();
        let provider = TestMetadataProvider(None);

        // ВЫРАЗИТЬ(... КАК Документ.НачислениеИСписаниеБонусныхБаллов).При
        let items = CompleteCastFieldsUseCase::execute(
            Some(&scope),
            &provider,
            MdoType::Document,
            "НачислениеИСписаниеБонусныхБаллов",
            &[],
            "При",
        );

        // Should filter to fields starting with "При"
        assert!(
            items.iter().all(|i| i.label.to_lowercase().starts_with("при")),
            "Expected only fields starting with 'При'"
        );
    }

    #[test]
    fn test_complete_cast_no_scope() {
        let provider = TestMetadataProvider(None);

        // Without scope, should return empty
        let items = CompleteCastFieldsUseCase::execute(
            None,
            &provider,
            MdoType::Document,
            "НачислениеИСписаниеБонусныхБаллов",
            &[],
            "",
        );

        assert!(items.is_empty(), "Expected empty results without scope");
    }
}
