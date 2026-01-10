//! FROM clause lowering and table resolution.

use crate::diagnostics::SdblDiagnostic;
use crate::hir::{FieldDef, Name, ResolvedTable, TableRef};
use crate::standard_fields::{is_virtual_table_name, standard_fields_for_mdo};
use crate::SdblType;
use bsl_metadata::MdoType;
use syntax::ast::AstNode;
use text_size::TextRange;

use super::context::LoweringContext;

impl<'a> LoweringContext<'a> {
    /// Lower FROM clause.
    pub(super) fn lower_from_clause(
        &mut self,
        from_clause: Option<syntax::ast::SdblFromClause>,
    ) -> Vec<TableRef> {
        let Some(from) = from_clause else {
            return Vec::new();
        };

        // Record FROM keyword
        self.record_keyword_by_text(
            from.syntax(),
            "FROM",
            "ИЗ",
            crate::source_map::TokenCategory::ClauseKeyword,
        );

        from.data_sources().map(|ds| self.lower_data_source(&ds)).collect()
    }

    /// Lower a data source (table or subquery).
    pub(super) fn lower_data_source(&mut self, ds: &syntax::ast::SdblDataSource) -> TableRef {
        // Check for subquery
        if let Some(subquery) = ds.subquery() {
            // Check if this data source has JOINs (context: subquery with JOINs)
            // This matches Java: visitDataSources() checks !joinPart().isEmpty() && subquery() != null
            if ds.join_clauses().next().is_some() {
                self.diagnostics.push(SdblDiagnostic::JoinWithSubQuery {
                    range: subquery.syntax().text_range(),
                });
            }

            // Recursively process nested queries in the subquery
            for inner_query in subquery.queries() {
                // Process SELECT fields in nested subquery (for diagnostics)
                if let Some(field_list) = inner_query.field_list() {
                    for field in field_list.fields() {
                        let _ = self.lower_selected_field(&field);
                    }
                }

                // Process FROM clause data sources
                if let Some(from_clause) = inner_query.from_clause() {
                    for inner_ds in from_clause.data_sources() {
                        let _ = self.lower_data_source(&inner_ds);
                    }
                }
            }

            // TODO: Handle subqueries properly
            return TableRef::missing(ds.syntax().text_range());
        }

        let Some(table_ref) = ds.table_ref() else {
            return TableRef::missing(ds.syntax().text_range());
        };

        self.lower_table_ref(&table_ref, ds.alias())
    }

    /// Lower table reference.
    fn lower_table_ref(
        &mut self,
        table_ref: &syntax::ast::SdblTableRef,
        alias: Option<syntax::ast::SdblAlias>,
    ) -> TableRef {
        // Parse table name parts
        let parts = self.parse_table_name(table_ref);
        let full_name = parts.join(".");

        // Check for virtual table
        let is_virtual = parts.last().map(|p| is_virtual_table_name(p)).unwrap_or(false);

        // Resolve in metadata
        let (_metadata, resolved) = self.resolve_table(&parts, table_ref.syntax().text_range());

        // NEW: Extract IDENT token ranges for semantic highlighting
        let ident_ranges: Vec<TextRange> = table_ref
            .syntax()
            .children_with_tokens()
            .filter_map(|child| match child {
                syntax::NodeOrToken::Token(token) if token.kind() == syntax::SyntaxKind::IDENT => {
                    Some(token.text_range())
                }
                _ => None,
            })
            .collect();

        // NEW: Record each table name part in source_map
        for (part, range) in parts.iter().zip(ident_ranges.iter()) {
            let category = if resolved.is_some() {
                crate::source_map::TokenCategory::TableName
            } else {
                crate::source_map::TokenCategory::UnresolvedTableName
            };
            self.source_map.add_token(
                crate::source_map::TokenInfo::new(*range, syntax::SyntaxKind::IDENT, part.as_str()),
                category,
            );
        }

        // Get alias
        let alias_name = alias
            .and_then(|a| {
                // Record AS/КАК keyword in source map for semantic highlighting
                if a.has_as_keyword() {
                    self.record_keyword_by_text(
                        a.syntax(),
                        "AS",
                        "КАК",
                        crate::source_map::TokenCategory::SpecialKeyword,
                    );
                }

                // NEW: Record table alias identifier
                if let Some(ident_token) = a.identifier() {
                    self.source_map.add_token(
                        crate::source_map::TokenInfo::new(
                            ident_token.text_range(),
                            ident_token.kind(),
                            ident_token.text(),
                        ),
                        crate::source_map::TokenCategory::TableAlias,
                    );
                }

                a.name()
            })
            .map(|s| Name::from(s.as_str()));

        TableRef {
            parts: parts.iter().map(|s| Name::from(s.as_str())).collect(),
            full_name,
            alias: alias_name,
            metadata: resolved,
            is_virtual_table: is_virtual,
            virtual_table_params: Vec::new(),
            range: table_ref.syntax().text_range(),
        }
    }

    /// Parse table name into parts.
    fn parse_table_name(&self, table_ref: &syntax::ast::SdblTableRef) -> Vec<String> {
        let text = table_ref.syntax().text().to_string();
        text.split('.').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    }

    /// Resolve table in metadata.
    fn resolve_table(
        &mut self,
        parts: &[String],
        range: TextRange,
    ) -> (Option<MdoType>, Option<ResolvedTable>) {
        tracing::debug!(parts = ?parts, "Resolving table");

        // Check for temporary tables first (single-part names)
        if parts.len() == 1 {
            let table_name = &parts[0];
            if let Some(temp_table) = self.scope.find_temp_table(table_name) {
                tracing::debug!(name = %table_name, fields = temp_table.fields.len(), "Resolved as temporary table");
                return (
                    None,
                    Some(ResolvedTable::TempTable {
                        name: temp_table.name.clone(),
                        fields: temp_table.fields.clone(),
                    }),
                );
            }
        }

        if parts.len() < 2 {
            tracing::debug!("Table parts < 2, skipping resolution");
            return (None, None);
        }

        // Parse MDO type (first part)
        let mdo_type_str = &parts[0];
        let Ok(mdo_type) = mdo_type_str.parse::<MdoType>() else {
            // Not a standard MDO type - could be alias or virtual table
            tracing::debug!(mdo_type_str = mdo_type_str, "Failed to parse MDO type");
            return (None, None);
        };

        let object_name = &parts[1];

        // Check metadata if available
        if let Some(metadata) = self.metadata {
            // Check if object exists in metadata
            let exists = match mdo_type {
                // For registers, check in registers collection
                MdoType::InformationRegister
                | MdoType::AccumulationRegister
                | MdoType::AccountingRegister
                | MdoType::CalculationRegister => {
                    let found = metadata.find_register_by_type_and_name(mdo_type, object_name);
                    tracing::info!(
                        mdo_type = ?mdo_type,
                        object_name = %object_name,
                        found = found.is_some(),
                        total_registers = metadata.registers().len(),
                        "Checking register in metadata"
                    );
                    found.is_some()
                }
                // For other types (Catalog, Document, etc.), check in metadata_objects
                _ => {
                    let found = metadata.has_metadata_object(mdo_type, object_name);
                    tracing::info!(
                        mdo_type = ?mdo_type,
                        object_name = %object_name,
                        found = found,
                        "Checking metadata object"
                    );
                    found
                }
            };

            if !exists {
                tracing::warn!(
                    mdo_type = ?mdo_type,
                    object_name = object_name,
                    "Table not found in metadata"
                );
                // Emit diagnostic: QueryToMissingMetadata
                self.diagnostics.push(SdblDiagnostic::QueryToMissingMetadata {
                    table_name: parts.join("."),
                    range,
                });
                return (Some(mdo_type), None);
            }
        } else {
            tracing::debug!("No metadata available for validation");
        }

        // Build resolved table with standard fields
        let mut fields = standard_fields_for_mdo(mdo_type);
        let full_name_for_logging = parts.join(".");
        tracing::info!(
            full_name = %full_name_for_logging,
            mdo_type = ?mdo_type,
            object_name = %object_name,
            standard_fields = fields.len(),
            has_metadata = self.metadata.is_some(),
            "resolve_table: Built standard fields, checking metadata"
        );

        // Add fields from metadata if available
        if let Some(_metadata) = self.metadata {
            self.add_metadata_fields(mdo_type, object_name, &full_name_for_logging, &mut fields);
        } else {
            tracing::warn!(
                full_name = %full_name_for_logging,
                "resolve_table: No metadata available, cannot add custom fields"
            );
        }

        tracing::info!(
            mdo_type = ?mdo_type,
            object_name = object_name,
            total_fields = fields.len(),
            "Resolved table with fields"
        );

        let resolved = ResolvedTable::Metadata { mdo_type, name: object_name.clone(), fields };

        (Some(mdo_type), Some(resolved))
    }

    /// Add fields from metadata to the fields list.
    fn add_metadata_fields(
        &self,
        mdo_type: MdoType,
        object_name: &str,
        full_name: &str,
        fields: &mut Vec<FieldDef>,
    ) {
        let Some(metadata) = self.metadata else {
            tracing::debug!("No metadata available for field resolution");
            return;
        };

        match mdo_type {
            // For registers, add dimensions, resources, and attributes
            MdoType::InformationRegister
            | MdoType::AccumulationRegister
            | MdoType::AccountingRegister
            | MdoType::CalculationRegister => {
                tracing::info!(
                    full_name = %full_name,
                    mdo_type = ?mdo_type,
                    object_name = %object_name,
                    "add_metadata_fields: Looking up register in metadata"
                );

                // Use find_register_by_type_and_name to ensure we get the correct register type
                if let Some(register) =
                    metadata.find_register_by_type_and_name(mdo_type, object_name)
                {
                    let initial_count = fields.len();

                    // Add dimensions
                    for dimension in register.dimensions() {
                        let ty = dimension
                            .attr_type()
                            .map(|attr_type| self.resolve_attribute_type(attr_type))
                            .unwrap_or(SdblType::Unknown);
                        fields.push(FieldDef::new(dimension.name(), ty));
                    }

                    // Add resources
                    for resource in register.resources() {
                        let ty = resource
                            .attr_type()
                            .map(|attr_type| self.resolve_attribute_type(attr_type))
                            .unwrap_or(SdblType::Unknown);
                        fields.push(FieldDef::new(resource.name(), ty));
                    }

                    // Add attributes
                    for attribute in register.attributes() {
                        let ty = attribute
                            .attr_type()
                            .map(|attr_type| self.resolve_attribute_type(attr_type))
                            .unwrap_or(SdblType::Unknown);
                        fields.push(FieldDef::new(attribute.name(), ty));
                    }

                    tracing::info!(
                        mdo_type = ?mdo_type,
                        object_name = object_name,
                        dimensions = register.dimensions().len(),
                        resources = register.resources().len(),
                        attributes = register.attributes().len(),
                        fields_added = fields.len() - initial_count,
                        total_fields = fields.len(),
                        "Added metadata fields to register"
                    );
                } else {
                    tracing::warn!(
                        full_name = %full_name,
                        mdo_type = ?mdo_type,
                        object_name = %object_name,
                        "Register not found in metadata (type mismatch or missing)"
                    );
                }
            }

            // For catalogs, documents, business processes, and tasks - add attributes
            MdoType::Catalog | MdoType::Document | MdoType::BusinessProcess | MdoType::Task => {
                tracing::info!(
                    full_name = %full_name,
                    mdo_type = ?mdo_type,
                    object_name = %object_name,
                    "add_metadata_fields: Looking up catalog/document/business process/task in metadata"
                );

                if let Some(obj) = metadata.find_metadata_object(mdo_type, object_name) {
                    let initial_count = fields.len();

                    for attribute in &obj.attributes {
                        let ty = self.resolve_attribute_type(&attribute.attr_type);
                        fields.push(FieldDef::new(attribute.name.clone(), ty));
                    }

                    tracing::info!(
                        mdo_type = ?mdo_type,
                        object_name = object_name,
                        attributes = obj.attributes.len(),
                        fields_added = fields.len() - initial_count,
                        total_fields = fields.len(),
                        "Added metadata fields to object"
                    );
                } else {
                    tracing::warn!(
                        full_name = %full_name,
                        mdo_type = ?mdo_type,
                        object_name = %object_name,
                        "Metadata object not found"
                    );
                }
            }

            _ => {}
        }
    }

    /// Resolve AttributeType to SdblType, resolving DefinedType through metadata if needed.
    fn resolve_attribute_type(&self, attr_type: &bsl_metadata::AttributeType) -> SdblType {
        use bsl_metadata::AttributeType;

        match attr_type {
            AttributeType::DefinedType { name } => {
                // Try to resolve underlying type through metadata
                let underlying_type = if let Some(metadata) = &self.metadata {
                    metadata.find_defined_type(name).map(|defined_type| {
                        // Recursively resolve the underlying type
                        Box::new(self.resolve_attribute_type(defined_type.underlying_type()))
                    })
                } else {
                    None
                };

                // Return DefinedType with optional underlying type
                SdblType::DefinedType { name: name.clone(), underlying_type }
            }
            // For all other types, use standard conversion
            _ => SdblType::from_attribute_type(attr_type),
        }
    }
}
