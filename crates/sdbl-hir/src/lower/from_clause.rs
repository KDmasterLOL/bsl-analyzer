//! FROM clause lowering and table resolution.

use crate::diagnostics::SdblDiagnostic;
use crate::hir::{Name, ResolvedTable, TableRef};
use crate::standard_fields::{is_virtual_table_name, standard_fields_for_mdo};
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
        if parts.len() < 2 {
            return (None, None);
        }

        // Parse MDO type (first part)
        let mdo_type_str = &parts[0];
        let Ok(mdo_type) = mdo_type_str.parse::<MdoType>() else {
            // Not a standard MDO type - could be alias or virtual table
            return (None, None);
        };

        let object_name = &parts[1];

        // Check metadata if available
        if let Some(metadata) = self.metadata {
            if !metadata.has_metadata_object(mdo_type, object_name) {
                // Emit diagnostic: QueryToMissingMetadata
                self.diagnostics.push(SdblDiagnostic::QueryToMissingMetadata {
                    table_name: parts.join("."),
                    range,
                });
                return (Some(mdo_type), None);
            }
        }

        // Build resolved table with standard fields
        let fields = standard_fields_for_mdo(mdo_type);

        let resolved = ResolvedTable { mdo_type, name: object_name.clone(), fields };

        (Some(mdo_type), Some(resolved))
    }
}
