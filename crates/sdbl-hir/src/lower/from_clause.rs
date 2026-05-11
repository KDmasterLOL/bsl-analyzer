//! FROM clause lowering and table resolution.

use crate::diagnostics::SdblDiagnostic;
use crate::hir::{FieldDef, Name, ResolvedTable, TableRef};
use crate::scope::is_standard_attribute_name;
use crate::standard_fields::{is_virtual_table_name, virtual_table_type};
use crate::SdblType;
use bsl_metadata::MdoType;
use syntax::ast::AstNode;
use text_size::TextRange;

use super::context::LoweringContext;

impl LoweringContext {
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

    /// Lower a data source in FROM clause (checks for subquery/virtual table with JOINs).
    fn lower_data_source_in_from(&mut self, ds: &syntax::ast::SdblDataSource) -> TableRef {
        // Check if this FROM data source has JOINs after subquery
        // Example: FROM (SELECT ...) AS Sub LEFT JOIN T2 ...
        // Track 2 §4 Slice 3: subqueries that aggregate (GROUP BY or aggregate
        // function in SELECT list) are exempted — they are doing something the
        // underlying table cannot, so the rule does not apply.
        if let Some(subquery) = ds.subquery() {
            if ds.join_clauses().next().is_some() && !subquery_has_aggregation(&subquery) {
                self.diagnostics.push(SdblDiagnostic::JoinWithSubQuery {
                    range: subquery.syntax().text_range(),
                });
            }
        }

        // Check if this FROM data source is a virtual table with JOINs
        if let Some(table_ref) = ds.table_ref() {
            if ds.join_clauses().next().is_some() {
                let parts: Vec<String> = table_ref
                    .syntax()
                    .children_with_tokens()
                    .filter_map(|child| match child {
                        syntax::NodeOrToken::Token(token)
                            if token.kind() == syntax::SyntaxKind::IDENT =>
                        {
                            Some(token.text().to_string())
                        }
                        _ => None,
                    })
                    .collect();

                if let Some(last_part) = parts.last() {
                    if let Some(vt_type) = virtual_table_type(last_part) {
                        let full_name = parts.join(".");
                        self.diagnostics.push(SdblDiagnostic::JoinWithVirtualTable {
                            table_name: full_name,
                            virtual_table_type: vt_type.to_string(),
                            range: table_ref.syntax().text_range(),
                        });
                    }
                }
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

            // Determine if there are UNION siblings in this subquery
            let queries: Vec<_> = subquery.queries().collect();
            let has_union_siblings = queries.len() > 1;

            for (query_index, query) in queries.into_iter().enumerate() {
                // NOTE: Diagnostic for JOINs inside subquery is handled by lower_query()
                // which calls lower_from_clause() -> lower_data_source_in_from()
                // No need to check here to avoid duplication

                // Push scope frame for nested query (isolate FROM/JOIN tables)
                self.scope.push_frame();

                // Lower the nested query to HIR
                let nested_hir = self.lower_query(&query, has_union_siblings, query_index == 0);

                // Pop scope frame
                self.scope.pop_frame();

                // NOTE: Don't copy nested query diagnostics to parent.
                // all_diagnostics() recursively collects them from subquery HIR.

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
        let (_metadata, mut resolved) = self.resolve_table(&parts, table_ref.syntax().text_range());

        // Transform fields for virtual tables (e.g., Обороты → resources get Оборот suffix)
        if is_virtual {
            if let Some(vt_type) = parts.last().and_then(|p| virtual_table_type(p)) {
                if let Some(r) = resolved.take() {
                    resolved = Some(Self::transform_for_virtual_table(r, vt_type));
                }
            }
        }

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

        // Lower virtual table parameters if this is a virtual table
        // Virtual table params are expression children inside SDBL_TABLE_REF after L_PAREN
        // Example: РегистрНакопления.Расчеты.Обороты(&Начало, &Конец, , (A.B, C.D) В ...)
        // Include ERROR nodes for empty parameter slots (e.g., "Остатки(, )")
        let virtual_table_params = if is_virtual {
            tracing::debug!(table_name = %full_name, "Lowering virtual table parameters");

            // Push temporary scope with register dimensions for VT condition resolution.
            // This allows unqualified references like `Партнер В (...)` in VT params
            // to resolve against the register's dimensions.
            let has_vt_scope = if let Some(ref r) = resolved {
                let dims = r.dimensions();
                if !dims.is_empty() {
                    self.scope.push_frame();
                    let dim_table = TableRef {
                        parts: Vec::new(),
                        full_name: String::new(),
                        alias: None,
                        metadata: Some(ResolvedTable::Metadata {
                            mdo_type: MdoType::AccumulationRegister,
                            name: String::new(),
                            fields: dims.to_vec(),
                        }),
                        is_virtual_table: false,
                        virtual_table_params: Vec::new(),
                        subquery: Vec::new(),
                        range: table_ref.syntax().text_range(),
                    };
                    self.scope.add_table(dim_table);
                    true
                } else {
                    false
                }
            } else {
                false
            };

            // Determine VT type for positional parameter handling
            let vt_type = parts.last().and_then(|p| virtual_table_type(p));
            let has_periodicity = vt_type.map(|vt| vt.has_periodicity()).unwrap_or(false);

            let param_nodes: Vec<_> = table_ref
                .syntax()
                .children()
                .filter(|n| {
                    matches!(
                        n.kind(),
                        syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                            | syntax::SyntaxKind::SDBL_LOGICAL_AND_EXPR
                            | syntax::SyntaxKind::SDBL_COMPARISON_EXPR
                            | syntax::SyntaxKind::SDBL_ADDITIVE_EXPR
                            | syntax::SyntaxKind::SDBL_MULTIPLICATIVE_EXPR
                            | syntax::SyntaxKind::SDBL_UNARY_EXPR
                            | syntax::SyntaxKind::SDBL_COLUMN_REF
                            | syntax::SyntaxKind::SDBL_LITERAL
                            | syntax::SyntaxKind::SDBL_FUNCTION_CALL
                            | syntax::SyntaxKind::SDBL_PARAMETER
                            | syntax::SyntaxKind::SDBL_PAREN_EXPR
                            | syntax::SyntaxKind::SDBL_TUPLE_EXPR
                            | syntax::SyntaxKind::SDBL_IN_EXPR
                            | syntax::SyntaxKind::SDBL_MISSING_ARG
                            | syntax::SyntaxKind::ERROR
                    )
                })
                .collect();

            let params: Vec<_> = param_nodes
                .into_iter()
                .enumerate()
                .map(|(idx, expr)| {
                    // 3rd parameter (index 2) for Turnovers/BalanceAndTurnovers is periodicity.
                    // The parser wraps single identifiers in precedence nodes
                    // (LOGICAL_OR → ... → COLUMN_REF), so find the inner COLUMN_REF.
                    if idx == 2 && has_periodicity {
                        let col_ref = if expr.kind() == syntax::SyntaxKind::SDBL_COLUMN_REF {
                            Some(expr.clone())
                        } else {
                            expr.descendants()
                                .find(|n| n.kind() == syntax::SyntaxKind::SDBL_COLUMN_REF)
                        };
                        if let Some(ref col) = col_ref {
                            if let Some(token) = col.first_token() {
                                if token.kind() == syntax::SyntaxKind::IDENT
                                    && crate::standard_fields::is_periodicity_value(token.text())
                                {
                                    self.source_map.add_token(
                                        crate::source_map::TokenInfo::new(
                                            token.text_range(),
                                            syntax::SyntaxKind::IDENT,
                                            token.text(),
                                        ),
                                        crate::source_map::TokenCategory::SpecialKeyword,
                                    );
                                    return crate::hir::ExprHir::Literal {
                                        value: crate::hir::LiteralValue::String(
                                            token.text().to_string(),
                                        ),
                                        ty: SdblType::string(),
                                        range: expr.text_range(),
                                    };
                                }
                            }
                        }
                    }
                    self.lower_expr(&expr)
                })
                .collect();

            // Pop temporary VT scope
            if has_vt_scope {
                self.scope.pop_frame();
            }

            params
        } else {
            Vec::new()
        };

        // Check for VirtualTableCallWithoutParameters diagnostic
        if is_virtual {
            self.check_virtual_table_params(&full_name, &virtual_table_params, table_ref.syntax());
        }

        TableRef {
            parts: parts.iter().map(|s| Name::from(s.as_str())).collect(),
            full_name,
            alias: alias_name,
            metadata: resolved,
            is_virtual_table: is_virtual,
            virtual_table_params,
            subquery: Vec::new(),
            range: table_ref.syntax().text_range(),
        }
    }

    /// Parse table name into parts.
    ///
    /// Uses IDENT tokens instead of text split to correctly handle virtual tables
    /// with parameters like `Регистр.Расчеты.Обороты(&Начало, ..., (A.B) В ...)`.
    fn parse_table_name(&self, table_ref: &syntax::ast::SdblTableRef) -> Vec<String> {
        // Extract only IDENT tokens as table name parts
        // This correctly handles virtual tables - parameters are child nodes, not IDENT tokens
        table_ref
            .syntax()
            .children_with_tokens()
            .filter_map(|child| match child {
                syntax::NodeOrToken::Token(token) if token.kind() == syntax::SyntaxKind::IDENT => {
                    Some(token.text().to_string())
                }
                _ => None,
            })
            .collect()
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

        // Handle ExternalDataSource specially (4-part or 6-part paths)
        // 4-part: ВнешнийИсточникДанных.EDSName.Таблица.TableName
        // 6-part: ВнешнийИсточникДанных.EDSName.Куб.CubeName.ТаблицаИзмерения.DimTableName
        if mdo_type == MdoType::ExternalDataSource && parts.len() >= 4 {
            return self.resolve_external_data_source(parts, range);
        }

        // Check if this is a 3-part name (tabular section reference)
        // But NOT if the third part is a virtual table name (СрезПоследних, Остатки, etc.)
        let tabular_section_name = if parts.len() == 3 && !is_virtual_table_name(&parts[2]) {
            Some(parts[2].as_str())
        } else {
            None
        };

        // Check metadata if available
        if let Some(ref metadata) = self.metadata {
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
        let is_register = matches!(
            mdo_type,
            MdoType::InformationRegister
                | MdoType::AccumulationRegister
                | MdoType::AccountingRegister
                | MdoType::CalculationRegister
        );

        tracing::debug!(
            full_name = %full_name_for_logging,
            mdo_type = ?mdo_type,
            object_name = %object_name,
            tabular_section = ?tabular_section_name,
            is_register = is_register,
            has_metadata = self.metadata.is_some(),
            "resolve_table: Starting field resolution"
        );

        // Registers (without tabular section) → Register variant with structured fields
        if is_register && tabular_section_name.is_none() {
            let resolved =
                self.build_register_resolved(mdo_type, object_name, &full_name_for_logging);
            return (Some(mdo_type), resolved);
        }

        // Non-register types and tabular sections → Metadata variant
        let mut fields = Vec::new();
        if self.metadata.is_some() {
            self.add_metadata_fields(
                mdo_type,
                object_name,
                tabular_section_name,
                &full_name_for_logging,
                &mut fields,
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

    /// Build `ResolvedTable::Register` with structured dimensions/resources/attributes.
    fn build_register_resolved(
        &self,
        mdo_type: MdoType,
        object_name: &str,
        full_name: &str,
    ) -> Option<ResolvedTable> {
        let metadata = self.metadata.as_ref()?;
        let register = metadata.find_register_by_type_and_name(mdo_type, object_name)?;

        let mut dimensions = Vec::new();
        for dim in register.dimensions() {
            let ty = dim
                .attr_type()
                .map(|at| self.resolve_attribute_type(at))
                .unwrap_or(SdblType::Unknown);
            dimensions.push(FieldDef::new(dim.name(), ty));
        }

        let mut resources = Vec::new();
        for res in register.resources() {
            let ty = res
                .attr_type()
                .map(|at| self.resolve_attribute_type(at))
                .unwrap_or(SdblType::Unknown);
            resources.push(FieldDef::new_with_names(
                res.name().to_string(),
                res.name_en().map(|s| s.to_string()),
                ty,
                false,
            ));
        }

        let mut attributes = Vec::new();
        for attr in register.attributes() {
            let ty = attr
                .attr_type()
                .map(|at| self.resolve_attribute_type(at))
                .unwrap_or(SdblType::Unknown);
            attributes.push(FieldDef::new_with_names(
                attr.name().to_string(),
                attr.name_en().map(|s| s.to_string()),
                ty,
                false,
            ));
        }

        // Combined fields for API compatibility
        let mut fields = Vec::new();
        fields.extend(dimensions.iter().cloned());
        fields.extend(resources.iter().cloned());
        fields.extend(attributes.iter().cloned());

        tracing::debug!(
            mdo_type = ?mdo_type,
            object_name = object_name,
            full_name = full_name,
            dimensions = dimensions.len(),
            resources = resources.len(),
            attributes = attributes.len(),
            total_fields = fields.len(),
            "Built Register resolved table"
        );

        Some(ResolvedTable::Register {
            mdo_type,
            name: object_name.to_string(),
            fields,
            dimensions,
            resources,
            attributes,
        })
    }

    /// Transform register fields for a virtual table type.
    ///
    /// Generates synthetic fields based on virtual table semantics:
    /// - Обороты: resources get `Оборот` suffix
    /// - Остатки: resources get `Остаток` suffix
    /// - ОстаткиИОбороты: resources get three suffix variants
    /// - СрезПоследних/СрезПервых: fields as-is + Период
    fn transform_for_virtual_table(
        resolved: ResolvedTable,
        vt_type: crate::standard_fields::VirtualTableType,
    ) -> ResolvedTable {
        use crate::standard_fields::VirtualTableType;

        let ResolvedTable::Register { mdo_type, name, dimensions, resources, attributes, .. } =
            resolved
        else {
            return resolved;
        };

        match vt_type {
            VirtualTableType::Turnovers => {
                let new_resources: Vec<FieldDef> = resources
                    .iter()
                    .map(|r| {
                        FieldDef::new_with_names(
                            format!("{}Оборот", r.name),
                            r.name_en.as_ref().map(|en| format!("{}Turnover", en)),
                            r.ty.clone(),
                            false,
                        )
                    })
                    .collect();

                let mut fields = vec![
                    FieldDef::standard("Период", "Period", SdblType::Date),
                    FieldDef::standard("Регистратор", "Recorder", SdblType::AnyRef),
                    FieldDef::standard("НомерСтроки", "LineNumber", SdblType::number()),
                ];
                fields.extend(dimensions.iter().cloned());
                fields.extend(new_resources.iter().cloned());

                ResolvedTable::Register {
                    mdo_type,
                    name,
                    fields,
                    dimensions,
                    resources: new_resources,
                    attributes: Vec::new(),
                }
            }
            VirtualTableType::Balance => {
                let new_resources: Vec<FieldDef> = resources
                    .iter()
                    .map(|r| {
                        FieldDef::new_with_names(
                            format!("{}Остаток", r.name),
                            r.name_en.as_ref().map(|en| format!("{}Balance", en)),
                            r.ty.clone(),
                            false,
                        )
                    })
                    .collect();

                let mut fields = Vec::new();
                fields.extend(dimensions.iter().cloned());
                fields.extend(new_resources.iter().cloned());

                ResolvedTable::Register {
                    mdo_type,
                    name,
                    fields,
                    dimensions,
                    resources: new_resources,
                    attributes: Vec::new(),
                }
            }
            VirtualTableType::BalanceAndTurnovers => {
                let mut new_resources = Vec::new();
                for r in &resources {
                    new_resources.push(FieldDef::new_with_names(
                        format!("{}НачальныйОстаток", r.name),
                        r.name_en.as_ref().map(|en| format!("{}OpeningBalance", en)),
                        r.ty.clone(),
                        false,
                    ));
                    new_resources.push(FieldDef::new_with_names(
                        format!("{}Оборот", r.name),
                        r.name_en.as_ref().map(|en| format!("{}Turnover", en)),
                        r.ty.clone(),
                        false,
                    ));
                    new_resources.push(FieldDef::new_with_names(
                        format!("{}КонечныйОстаток", r.name),
                        r.name_en.as_ref().map(|en| format!("{}ClosingBalance", en)),
                        r.ty.clone(),
                        false,
                    ));
                }

                let mut fields = vec![
                    FieldDef::standard("Период", "Period", SdblType::Date),
                    FieldDef::standard("Регистратор", "Recorder", SdblType::AnyRef),
                ];
                fields.extend(dimensions.iter().cloned());
                fields.extend(new_resources.iter().cloned());

                ResolvedTable::Register {
                    mdo_type,
                    name,
                    fields,
                    dimensions,
                    resources: new_resources,
                    attributes: Vec::new(),
                }
            }
            VirtualTableType::SliceLast | VirtualTableType::SliceFirst => {
                let mut fields = vec![FieldDef::standard("Период", "Period", SdblType::Date)];
                fields.extend(dimensions.iter().cloned());
                fields.extend(resources.iter().cloned());
                fields.extend(attributes.iter().cloned());

                ResolvedTable::Register {
                    mdo_type,
                    name,
                    fields,
                    dimensions,
                    resources,
                    attributes,
                }
            }
            // Changes, RecordsWithExtDimensions, etc. — fields as-is
            _ => ResolvedTable::Register {
                mdo_type,
                name: name.clone(),
                fields: {
                    let mut f = Vec::new();
                    f.extend(dimensions.iter().cloned());
                    f.extend(resources.iter().cloned());
                    f.extend(attributes.iter().cloned());
                    f
                },
                dimensions,
                resources,
                attributes,
            },
        }
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
        let Some(ref metadata) = self.metadata else {
            tracing::debug!("No metadata available for field resolution");
            return;
        };

        // Handle tabular section if 3-part name detected
        if let Some(ts_name) = tabular_section_name {
            self.add_tabular_section_fields(mdo_type, object_name, ts_name, full_name, fields);
            return; // Early return - don't process as main object
        }

        // Continue with existing logic for non-register main objects
        // (Registers are handled by build_register_resolved() in resolve_table())
        match mdo_type {
            // For object-like metadata tables, add attributes collected by metadata loading,
            // including platform standard attributes such as Ссылка/Ref.
            MdoType::Catalog
            | MdoType::Document
            | MdoType::BusinessProcess
            | MdoType::Task
            | MdoType::ExchangePlan
            | MdoType::ChartOfCharacteristicTypes
            | MdoType::ChartOfAccounts
            | MdoType::ChartOfCalculationTypes => {
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
                        let is_standard = is_standard_attribute_name(&attribute.name)
                            || attribute.name_en.as_deref().is_some_and(is_standard_attribute_name);
                        fields.push(FieldDef::new_with_names(
                            attribute.name.clone(),
                            attribute.name_en.clone(),
                            ty,
                            is_standard,
                        ));
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
        let Some(ref metadata) = self.metadata else {
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

        // 4b. Add standard НомерСтроки field
        fields.push(FieldDef::new_with_names(
            "НомерСтроки".to_string(),
            Some("LineNumber".to_string()),
            SdblType::number(),
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

    /// Resolve ExternalDataSource table path.
    ///
    /// Handles 4-part and 6-part paths:
    /// - 4-part: ВнешнийИсточникДанных.EDSName.Таблица.TableName
    /// - 6-part: ВнешнийИсточникДанных.EDSName.Куб.CubeName.ТаблицаИзмерения.DimTableName
    fn resolve_external_data_source(
        &mut self,
        parts: &[String],
        range: TextRange,
    ) -> (Option<MdoType>, Option<ResolvedTable>) {
        let eds_name = &parts[1];

        tracing::debug!(
            eds_name = %eds_name,
            parts_len = parts.len(),
            "Resolving ExternalDataSource path"
        );

        let Some(ref metadata) = self.metadata else {
            tracing::debug!("No metadata available for EDS validation");
            return (Some(MdoType::ExternalDataSource), None);
        };

        // Check if EDS exists
        let eds_obj = metadata.find_metadata_object(MdoType::ExternalDataSource, eds_name);
        if eds_obj.is_none() {
            tracing::debug!(eds_name = %eds_name, "ExternalDataSource not found in metadata");
            self.diagnostics.push(SdblDiagnostic::QueryToMissingMetadata {
                table_name: format!("{}.{}", parts[0], eds_name),
                range,
            });
            return (Some(MdoType::ExternalDataSource), None);
        }

        let eds_obj = eds_obj.unwrap();

        // Check 3rd part: "Таблица"/"Table" or "Куб"/"Cube"
        let container_type = parts[2].to_lowercase();

        // 4-part path: ВнешнийИсточникДанных.EDSName.Таблица.TableName
        if parts.len() == 4 && (container_type == "таблица" || container_type == "table") {
            // For now, we don't validate individual tables inside EDS
            // This would require loading Table children which is not yet implemented
            tracing::debug!(
                eds_name = %eds_name,
                table_name = %parts[3],
                "EDS table path (table validation not implemented)"
            );
            return (Some(MdoType::ExternalDataSource), None);
        }

        // 6-part path: ВнешнийИсточникДанных.EDSName.Куб.CubeName.ТаблицаИзмерения.DimTableName
        if parts.len() == 6 && (container_type == "куб" || container_type == "cube") {
            let cube_name = &parts[3];
            let dim_table_type = parts[4].to_lowercase();
            let dim_table_name = &parts[5];

            // Validate Cube exists in EDS children
            let cube_obj = eds_obj.find_child(cube_name);
            if cube_obj.is_none() {
                tracing::debug!(
                    eds_name = %eds_name,
                    cube_name = %cube_name,
                    "Cube not found in ExternalDataSource"
                );
                self.diagnostics.push(SdblDiagnostic::QueryToMissingMetadata {
                    table_name: format!("{}.{}.{}.{}", parts[0], eds_name, parts[2], cube_name),
                    range,
                });
                return (Some(MdoType::ExternalDataSource), None);
            }

            let cube_obj = cube_obj.unwrap();

            // Validate DimensionTable if path specifies it
            if dim_table_type == "таблицаизмерения" || dim_table_type == "dimensiontable"
            {
                let dim_table_obj = cube_obj.find_child(dim_table_name);
                if dim_table_obj.is_none() {
                    tracing::debug!(
                        cube_name = %cube_name,
                        dim_table_name = %dim_table_name,
                        "DimensionTable not found in Cube"
                    );
                    self.diagnostics.push(SdblDiagnostic::QueryToMissingMetadata {
                        table_name: format!(
                            "{}.{}.{}.{}.{}.{}",
                            parts[0], eds_name, parts[2], cube_name, parts[4], dim_table_name
                        ),
                        range,
                    });
                    return (Some(MdoType::ExternalDataSource), None);
                }
            }

            tracing::debug!(
                eds_name = %eds_name,
                cube_name = %cube_name,
                dim_table_name = %dim_table_name,
                "EDS Cube DimensionTable resolved"
            );
            return (Some(MdoType::ExternalDataSource), None);
        }

        // Fallback for other patterns
        tracing::debug!(
            parts = ?parts,
            "Unhandled EDS path pattern"
        );
        (Some(MdoType::ExternalDataSource), None)
    }

    /// Resolve [`bsl_metadata::AttributeType`] to [`SdblType`], expanding a
    /// `DefinedType` reference through the attached metadata.
    ///
    /// Recursion is cycle-guarded — `A → B → A` shaped chains in the
    /// configuration return a `DefinedType` node with `underlying_type = None`
    /// instead of overflowing the stack.
    pub(crate) fn resolve_attribute_type(
        &self,
        attr_type: &bsl_metadata::AttributeType,
    ) -> SdblType {
        let mut visited = std::collections::HashSet::new();
        self.resolve_attribute_type_inner(attr_type, &mut visited)
    }

    fn resolve_attribute_type_inner(
        &self,
        attr_type: &bsl_metadata::AttributeType,
        visited: &mut std::collections::HashSet<String>,
    ) -> SdblType {
        use bsl_metadata::{AttributeType, MetadataResolver};

        match attr_type {
            AttributeType::DefinedType { name } => {
                let key = name.to_lowercase();
                if !visited.insert(key.clone()) {
                    // Cycle detected — return the surface name without an
                    // underlying type so downstream consumers still see "this
                    // is some DefinedType" but cannot recurse further.
                    return SdblType::DefinedType { name: name.clone(), underlying_type: None };
                }
                let underlying_type =
                    self.metadata.as_ref().and_then(|m| m.resolve_defined_type(name)).map(
                        |underlying| {
                            Box::new(self.resolve_attribute_type_inner(underlying, visited))
                        },
                    );
                // Snapshot/restore: pop the name once we leave its lowering
                // so sibling `Composite` arms that share an intermediate
                // DefinedType (`{DefT.A, DefT.B}` both chaining through
                // `X → terminal`) can each walk through it without
                // observing a false cycle. Mirrors the BSL HIR fix in
                // `hir-ty/src/lower/mod.rs::lower_qualified_inner`.
                visited.remove(&key);
                SdblType::DefinedType { name: name.clone(), underlying_type }
            }
            // Recurse into composite arms so nested `DefinedType` references
            // are resolved through metadata. The metadata-blind
            // `SdblType::from_attribute_type` would silently drop
            // `underlying_type`, breaking downstream field resolution
            // (`Scope::resolve_defined_type_fields`).
            AttributeType::Composite { types } => {
                let arms: Vec<SdblType> =
                    types.iter().map(|t| self.resolve_attribute_type_inner(t, visited)).collect();
                if arms.is_empty() {
                    SdblType::Unknown
                } else if arms.len() == 1 {
                    arms.into_iter().next().unwrap()
                } else {
                    SdblType::Composite { types: arms }
                }
            }
            // For all other types, use standard conversion
            _ => SdblType::from_attribute_type(attr_type),
        }
    }

    /// Check virtual table parameters and emit diagnostic if missing.
    ///
    /// Errors on:
    /// - Virtual table without parentheses: `СрезПоследних`
    /// - Virtual table with empty parentheses: `Остатки()`
    /// - Virtual table where all params after first (period) are empty: `Остатки(&Период, )`
    ///
    /// OK:
    /// - `Остатки(Склад = &Параметр)` - has condition
    /// - `Остатки(, Склад = &Параметр)` - empty period, but has condition
    /// - `СрезПоследних(&Период)` - period param provided
    fn check_virtual_table_params(
        &mut self,
        table_name: &str,
        params: &[crate::hir::ExprHir],
        table_ref_node: &syntax::SyntaxNode,
    ) {
        use crate::hir::ExprHir;

        // Check if parentheses are present by looking for L_PAREN token
        let has_parens = table_ref_node
            .children_with_tokens()
            .any(|child| matches!(child, syntax::NodeOrToken::Token(t) if t.kind() == syntax::SyntaxKind::L_PAREN));

        let range = table_ref_node.text_range();

        // No parentheses at all - error
        if !has_parens {
            self.diagnostics.push(SdblDiagnostic::VirtualTableCallWithoutParameters {
                table_name: table_name.to_string(),
                expected_params: vec!["Период".to_string(), "Условие".to_string()],
                range,
            });
            return;
        }

        // Empty parentheses or all params empty - error
        if params.is_empty() {
            self.diagnostics.push(SdblDiagnostic::VirtualTableCallWithoutParameters {
                table_name: table_name.to_string(),
                expected_params: vec!["Период".to_string(), "Условие".to_string()],
                range,
            });
            return;
        }

        // Skip first param (period), check remaining
        // If there's more than one slot, at least one after first must be non-empty
        if params.len() > 1 {
            let has_non_empty_after_first =
                params[1..].iter().any(|p| !matches!(p, ExprHir::Missing { .. }));

            if !has_non_empty_after_first {
                self.diagnostics.push(SdblDiagnostic::VirtualTableCallWithoutParameters {
                    table_name: table_name.to_string(),
                    expected_params: vec!["Условие".to_string()],
                    range,
                });
            }
        }
        // Single param is OK for СрезПоследних(&Период)
    }
}

/// Returns true if any query (main or UNION part) inside the subquery contains
/// aggregation — a `GROUP BY` (`СГРУППИРОВАТЬ ПО`) clause or a call to an
/// aggregate function (`СУММА`/`SUM`, `СРЕДНЕЕ`/`AVG`, `МИНИМУМ`/`MIN`,
/// `МАКСИМУМ`/`MAX`, `КОЛИЧЕСТВО`/`COUNT`) in its direct SELECT list.
///
/// Used to exempt aggregating subqueries from `JoinWithSubQuery` — a subquery
/// that aggregates is doing something the underlying table cannot, so the
/// «no joins on subqueries» rule does not apply.
///
/// The aggregate-call scan inspects only `SDBL_FUNCTION_CALL` nodes and
/// matches the first direct `IDENT` child token (the function-name position).
/// Column references and aliases that happen to be named `Сумма`/`Sum` are
/// not in function-call position and therefore do not suppress the
/// diagnostic.
pub(super) fn subquery_has_aggregation(subquery: &syntax::ast::SdblSubquery) -> bool {
    subquery.queries().any(|q| {
        if q.group_by_clause().is_some() {
            return true;
        }
        let Some(field_list) = q.field_list() else {
            return false;
        };
        let has_aggregate = field_list.fields().any(|field| field_contains_aggregate_call(&field));
        has_aggregate
    })
}

fn field_contains_aggregate_call(field: &syntax::ast::SdblSelectedField) -> bool {
    let Some(expr) = field.expression() else {
        return false;
    };
    expr.descendants()
        .filter(|n| n.kind() == syntax::SyntaxKind::SDBL_FUNCTION_CALL)
        .any(|call| function_call_is_aggregate(&call))
}

fn function_call_is_aggregate(call: &syntax::SyntaxNode) -> bool {
    // First direct IDENT child of an `SDBL_FUNCTION_CALL` is the function
    // name (matches the dispatch in `lower_function_call`).
    call.children_with_tokens()
        .filter_map(|nt| nt.into_token())
        .find(|t| t.kind() == syntax::SyntaxKind::IDENT)
        .is_some_and(|t| is_aggregate_name(t.text()))
}

fn is_aggregate_name(name: &str) -> bool {
    matches!(
        name.to_uppercase().as_str(),
        "СУММА"
            | "SUM"
            | "СРЕДНЕЕ"
            | "AVG"
            | "МИНИМУМ"
            | "MIN"
            | "МАКСИМУМ"
            | "MAX"
            | "КОЛИЧЕСТВО"
            | "COUNT"
    )
}
