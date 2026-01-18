//! FROM clause lowering and table resolution.

use crate::diagnostics::SdblDiagnostic;
use crate::hir::{FieldDef, Name, ResolvedTable, TableRef};
use crate::standard_fields::is_virtual_table_name;
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

        from.data_sources().map(|ds| self.lower_data_source_in_from(&ds)).collect()
    }

    /// Lower a data source in FROM clause (checks for subquery with JOINs).
    fn lower_data_source_in_from(&mut self, ds: &syntax::ast::SdblDataSource) -> TableRef {
        // Check if this FROM data source has JOINs after subquery
        // Example: FROM (SELECT ...) AS Sub LEFT JOIN T2 ...
        if let Some(subquery) = ds.subquery() {
            if ds.join_clauses().next().is_some() {
                self.diagnostics.push(SdblDiagnostic::JoinWithSubQuery {
                    range: subquery.syntax().text_range(),
                });
            }
        }

        self.lower_data_source(ds)
    }

    /// Lower a data source (table or subquery).
    pub(super) fn lower_data_source(&mut self, ds: &syntax::ast::SdblDataSource) -> TableRef {
        // Check for subquery
        if let Some(subquery) = ds.subquery() {
            // NOTE: Diagnostic for "JOIN with subquery" is handled in join_clause.rs:114-117
            // to avoid duplication. We only check for JOINs INSIDE the subquery here.

            // Process ALL queries in subquery (main query + UNION queries)
            let mut all_hirs = Vec::new();
            let mut all_fields = Vec::new();

            for query in subquery.queries() {
                // NOTE: Diagnostic for JOINs inside subquery is handled by lower_query()
                // which calls lower_from_clause() -> lower_data_source_in_from()
                // No need to check here to avoid duplication

                // Push scope frame for nested query (isolate FROM/JOIN tables)
                self.scope.push_frame();

                // Lower the nested query to HIR (nested queries are not UNION queries)
                let nested_hir = self.lower_query(&query, false);

                // Pop scope frame
                self.scope.pop_frame();

                // Collect diagnostics from nested query
                self.diagnostics.extend(nested_hir.diagnostics.clone());

                // Extract fields from SELECT for metadata (only from first query)
                if all_fields.is_empty() {
                    all_fields = nested_hir
                        .select
                        .fields
                        .iter()
                        .filter_map(|f| {
                            f.alias_or_name()
                                .map(|name| crate::hir::FieldDef::new(name.as_str(), f.ty.clone()))
                        })
                        .collect();
                }

                all_hirs.push(Box::new(nested_hir));
            }

            if all_hirs.is_empty() {
                // Fallback if no queries in subquery (should not happen in valid SDBL)
                return TableRef::missing(ds.syntax().text_range());
            }

            // Get alias
            let alias_name = ds.alias().and_then(|a| a.name().map(|n| Name::from(n.as_str())));

            // Create TableRef with all subquery HIRs
            return TableRef {
                parts: Vec::new(),
                full_name: alias_name.as_ref().map(|a| a.to_string()).unwrap_or_default(),
                alias: alias_name.clone(),
                metadata: Some(crate::hir::ResolvedTable::TempTable {
                    name: alias_name.map(|a| a.to_string()).unwrap_or_default(),
                    fields: all_fields,
                }),
                is_virtual_table: false,
                virtual_table_params: Vec::new(),
                subquery: all_hirs,
                range: ds.syntax().text_range(),
            };
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
        for (idx, (part, range)) in parts.iter().zip(ident_ranges.iter()).enumerate() {
            let category = if resolved.is_some() {
                // First part is MDO type (Справочник, Документ), rest are object names
                if idx == 0 && parts.len() > 1 {
                    crate::source_map::TokenCategory::MdoType
                } else {
                    crate::source_map::TokenCategory::TableName
                }
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
            subquery: Vec::new(),
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

        // Check if this is a 3-part name (tabular section reference)
        // But NOT if the third part is a virtual table name (СрезПоследних, Остатки, etc.)
        let tabular_section_name = if parts.len() == 3 && !is_virtual_table_name(&parts[2]) {
            Some(parts[2].as_str())
        } else {
            None
        };

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
                    tracing::debug!(
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
                    tracing::debug!(
                        mdo_type = ?mdo_type,
                        object_name = %object_name,
                        found = found,
                        "Checking metadata object"
                    );
                    found
                }
            };

            if !exists {
                tracing::debug!(
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

        // Build resolved table
        let full_name_for_logging = parts.join(".");

        // Start with empty fields - metadata will provide all fields including standard ones
        let mut fields = Vec::new();

        tracing::debug!(
            full_name = %full_name_for_logging,
            mdo_type = ?mdo_type,
            object_name = %object_name,
            tabular_section = ?tabular_section_name,
            initial_fields = fields.len(),
            has_metadata = self.metadata.is_some(),
            "resolve_table: Starting field resolution"
        );

        // Add fields from metadata if available
        if let Some(_metadata) = self.metadata {
            self.add_metadata_fields(
                mdo_type,
                object_name,
                tabular_section_name,
                &full_name_for_logging,
                &mut fields,
            );
        } else {
            tracing::debug!(
                full_name = %full_name_for_logging,
                "resolve_table: No metadata available, cannot add custom fields"
            );
        }

        tracing::debug!(
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
        tabular_section_name: Option<&str>,
        full_name: &str,
        fields: &mut Vec<FieldDef>,
    ) {
        let Some(metadata) = self.metadata else {
            tracing::debug!("No metadata available for field resolution");
            return;
        };

        // Handle tabular section if 3-part name detected
        if let Some(ts_name) = tabular_section_name {
            self.add_tabular_section_fields(mdo_type, object_name, ts_name, full_name, fields);
            return; // Early return - don't process as main object
        }

        // Continue with existing logic for main objects
        match mdo_type {
            // For registers, add dimensions, resources, and attributes
            MdoType::InformationRegister
            | MdoType::AccumulationRegister
            | MdoType::AccountingRegister
            | MdoType::CalculationRegister => {
                tracing::debug!(
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

                    tracing::debug!(
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
                    tracing::debug!(
                        full_name = %full_name,
                        mdo_type = ?mdo_type,
                        object_name = %object_name,
                        "Register not found in metadata (may be from extension)"
                    );
                }
            }

            // For catalogs, documents, business processes, tasks, exchange plans - add attributes
            MdoType::Catalog
            | MdoType::Document
            | MdoType::BusinessProcess
            | MdoType::Task
            | MdoType::ExchangePlan => {
                tracing::debug!(
                    full_name = %full_name,
                    mdo_type = ?mdo_type,
                    object_name = %object_name,
                    "add_metadata_fields: Looking up metadata object"
                );

                if let Some(obj) = metadata.find_metadata_object(mdo_type, object_name) {
                    let initial_count = fields.len();

                    for attribute in &obj.attributes {
                        let ty = self.resolve_attribute_type(&attribute.attr_type);
                        fields.push(FieldDef::new(attribute.name.clone(), ty));
                    }

                    tracing::debug!(
                        mdo_type = ?mdo_type,
                        object_name = object_name,
                        attributes = obj.attributes.len(),
                        fields_added = fields.len() - initial_count,
                        total_fields = fields.len(),
                        "Added metadata fields to object"
                    );
                } else {
                    tracing::debug!(
                        full_name = %full_name,
                        mdo_type = ?mdo_type,
                        object_name = %object_name,
                        "Metadata object not found (may be from extension)"
                    );
                }
            }

            _ => {}
        }
    }

    /// Add fields from tabular section to the fields list.
    fn add_tabular_section_fields(
        &self,
        mdo_type: MdoType,
        object_name: &str,
        tabular_section_name: &str,
        full_name: &str,
        fields: &mut Vec<FieldDef>,
    ) {
        let Some(metadata) = self.metadata else {
            tracing::debug!("No metadata available for tabular section resolution");
            return;
        };

        tracing::debug!(
            full_name = %full_name,
            mdo_type = ?mdo_type,
            object_name = %object_name,
            tabular_section_name = %tabular_section_name,
            "add_tabular_section_fields: Looking up tabular section in metadata"
        );

        // 1. Validate MDO type supports tabular sections
        match mdo_type {
            MdoType::Catalog
            | MdoType::Document
            | MdoType::BusinessProcess
            | MdoType::Task
            | MdoType::ExchangePlan
            | MdoType::ChartOfCharacteristicTypes
            | MdoType::ChartOfAccounts => {
                // Valid - continue
            }
            _ => {
                tracing::debug!(
                    mdo_type = ?mdo_type,
                    object_name = %object_name,
                    tabular_section_name = %tabular_section_name,
                    "MDO type does not support tabular sections"
                );
                return;
            }
        }

        // 2. Find parent object in metadata
        // Note: parent object may come from extension (not loaded)
        let Some(parent_obj) = metadata.find_metadata_object(mdo_type, object_name) else {
            tracing::debug!(
                mdo_type = ?mdo_type,
                object_name = %object_name,
                "Parent object not found in metadata (may be from extension)"
            );
            return;
        };

        // 3. Find tabular section by name
        // Note: tabular section may come from extension (not loaded)
        let Some(tabular_section) = parent_obj.find_tabular_section(tabular_section_name) else {
            tracing::debug!(
                mdo_type = ?mdo_type,
                object_name = %object_name,
                tabular_section_name = %tabular_section_name,
                available_sections = ?parent_obj.tabular_sections.iter()
                    .map(|ts| ts.name())
                    .collect::<Vec<_>>(),
                "Tabular section not found in parent object (may be from extension)"
            );
            return;
        };

        tracing::debug!(
            tabular_section_name = %tabular_section_name,
            attributes_count = tabular_section.attributes().len(),
            "Found tabular section in metadata"
        );

        // 4. Add standard Ссылка field (reference to parent object)
        let ref_type = SdblType::reference(mdo_type, object_name);
        fields.push(FieldDef::new_with_names(
            "Ссылка".to_string(),
            Some("Ref".to_string()),
            ref_type,
            true, // is_standard
        ));

        // 5. Add all tabular section attributes
        for attribute in tabular_section.attributes() {
            // Use structured AttributeType directly (single source of truth)
            let ty = SdblType::from_attribute_type(attribute.attr_type());

            fields.push(FieldDef::new_with_names(
                attribute.name().to_string(),
                attribute.name_en().map(|s| s.to_string()),
                ty,
                false, // Not a standard attribute
            ));
        }

        tracing::debug!(
            mdo_type = ?mdo_type,
            object_name = %object_name,
            tabular_section_name = %tabular_section_name,
            total_fields = fields.len(),
            "Added tabular section fields"
        );
    }

    /// Parse attribute type from type_str (simplified for MVP).
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
