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

        // Build resolved table
        let full_name_for_logging = parts.join(".");

        // For tabular sections, don't add standard fields for the main object
        // (they will be added inside add_metadata_fields)
        let mut fields = if tabular_section_name.is_none() {
            standard_fields_for_mdo(mdo_type)
        } else {
            Vec::new()
        };

        tracing::info!(
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

        tracing::info!(
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
            | MdoType::ChartOfCharacteristicTypes
            | MdoType::ChartOfAccounts => {
                // Valid - continue
            }
            _ => {
                tracing::warn!(
                    mdo_type = ?mdo_type,
                    object_name = %object_name,
                    tabular_section_name = %tabular_section_name,
                    "MDO type does not support tabular sections"
                );
                return;
            }
        }

        // 2. Find parent object in metadata
        let Some(parent_obj) = metadata.find_metadata_object(mdo_type, object_name) else {
            tracing::warn!(
                mdo_type = ?mdo_type,
                object_name = %object_name,
                "Parent object not found in metadata"
            );
            return;
        };

        // 3. Find tabular section by name
        let Some(tabular_section) = parent_obj.find_tabular_section(tabular_section_name) else {
            tracing::warn!(
                mdo_type = ?mdo_type,
                object_name = %object_name,
                tabular_section_name = %tabular_section_name,
                available_sections = ?parent_obj.tabular_sections.iter()
                    .map(|ts| ts.name())
                    .collect::<Vec<_>>(),
                "Tabular section not found in parent object"
            );
            return;
        };

        tracing::info!(
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
            // Parse type from type_str
            let ty = self.parse_tabular_section_attribute_type(attribute.type_str());

            fields.push(FieldDef::new_with_names(
                attribute.name().to_string(),
                attribute.name_en().map(|s| s.to_string()),
                ty,
                false, // Not a standard attribute
            ));
        }

        tracing::info!(
            mdo_type = ?mdo_type,
            object_name = %object_name,
            tabular_section_name = %tabular_section_name,
            total_fields = fields.len(),
            "Added tabular section fields"
        );
    }

    /// Parse attribute type from type_str (simplified for MVP).
    fn parse_tabular_section_attribute_type(&self, type_str: &str) -> SdblType {
        // Simplified type parsing for common cases
        // TODO: Enhance with full type parser later

        let type_str = type_str.trim();

        tracing::debug!(type_str = %type_str, "Parsing tabular section attribute type");

        // Check for reference types in format "МдоТип.ИмяОбъекта"
        // The type_str comes from Display format of AttributeType::Ref
        if let Some(dot_pos) = type_str.find('.') {
            let type_part = &type_str[..dot_pos];
            let name_part = &type_str[dot_pos + 1..];

            // Try to parse MDO type (expects singular form like "Задача", "Справочник")
            if let Ok(mdo_type) = type_part.parse::<MdoType>() {
                return SdblType::reference(mdo_type, name_part);
            }
        }

        // Check for primitive and special types
        match type_str.to_lowercase().as_str() {
            s if s.starts_with("string") || s.starts_with("строка") => {
                // Extract length if present: "String(100)" or "Строка(100)"
                if let Some(start) = s.find('(') {
                    if let Some(end) = s.find(')') {
                        if let Ok(len) = s[start + 1..end].trim().parse::<u32>() {
                            return SdblType::string_with_length(len);
                        }
                    }
                }
                SdblType::string()
            }
            s if s.starts_with("number") || s.starts_with("число") => {
                // Extract precision/scale if present: "Number(10, 2)"
                SdblType::number()
            }
            "boolean" | "булево" => SdblType::Boolean,
            "date" | "дата" => SdblType::Date,
            "datetime" | "датавремя" => SdblType::DateTime,
            "уникальныйидентификатор" => SdblType::Uuid,
            "хранилищезначения" => SdblType::ValueStorage,
            "любаяссылка" => SdblType::AnyRef,
            s if s.starts_with("определяемыйтип.") => {
                // Extract defined type name after "ОпределяемыйТип."
                let prefix_len = "ОпределяемыйТип.".len();
                let name = type_str[prefix_len..].to_string();
                SdblType::DefinedType { name, underlying_type: None }
            }
            _ => {
                tracing::debug!(type_str = %type_str, "Unknown type, using SdblType::Unknown");
                SdblType::Unknown
            }
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
