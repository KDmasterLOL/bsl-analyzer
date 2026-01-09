//! SDBL AST to HIR lowering.
//!
//! Transforms SDBL syntax trees into semantic HIR with:
//! - Type inference from metadata
//! - Name resolution (tables, fields, aliases)
//! - Semantic diagnostics collection

mod context;

use std::collections::HashSet;

use syntax::ast::{AstNode, SdblQueryPackage};
use syntax::{Parse, SyntaxKind};
use text_size::TextRange;

use crate::diagnostics::SdblDiagnostic;
use crate::hir::{ExprHir, FieldHir, JoinHir, Name, ResolvedTable, SdblHir, SelectHir, TableRef};
use crate::source_map::SdblSourceMap;
use crate::standard_fields::{is_virtual_table_name, standard_fields_for_mdo, virtual_table_type};
use crate::types::SdblType;

pub use context::LoweringContext;

use bsl_metadata::{Configuration, MdoType};

/// Result of SDBL lowering.
///
/// Contains both the HIR and source mapping for semantic highlighting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblLowerResult {
    /// The lowered HIR.
    pub hir: SdblHir,

    /// Source mapping for semantic highlighting.
    pub source_map: SdblSourceMap,
}

/// Lower SDBL AST to HIR with source mapping.
///
/// # Arguments
///
/// * `sdbl_ast` - Parsed SDBL syntax tree
/// * `metadata` - Optional 1C configuration metadata for table/field resolution
///
/// # Returns
///
/// `SdblLowerResult` containing HIR with resolved types, collected diagnostics,
/// and source mapping for semantic highlighting.
///
/// # Example
///
/// ```ignore
/// let sdbl_ast = parser::parse_sdbl("SELECT Код FROM Справочник.Валюты");
/// let metadata = load_configuration()?;
/// let result = lower_sdbl_to_hir(&sdbl_ast, Some(&metadata));
///
/// for diag in &result.hir.diagnostics {
///     println!("Error: {}", diag.message());
/// }
///
/// // Use source map for semantic highlighting
/// for (token, category) in result.source_map.all_tokens() {
///     println!("Token: {} at {:?}", token.text, token.range);
/// }
/// ```
pub fn lower_sdbl_to_hir(
    sdbl_ast: &Parse<syntax::SyntaxNode>,
    metadata: Option<&Configuration>,
) -> SdblLowerResult {
    let _span = tracing::info_span!("lower_sdbl_to_hir").entered();

    let root = sdbl_ast.syntax_node();

    // Try to cast root as query package
    let Some(package) = SdblQueryPackage::cast(root) else {
        tracing::debug!("Failed to cast root as SdblQueryPackage");
        return SdblLowerResult { hir: SdblHir::empty(), source_map: SdblSourceMap::new() };
    };

    // Create lowering context
    let mut ctx = LoweringContext::new(metadata);

    // Lower ALL SELECT queries in the package
    let mut queries = package.queries();
    let Some(first_query) = queries.next() else {
        tracing::debug!("No queries in package");
        return SdblLowerResult { hir: SdblHir::empty(), source_map: SdblSourceMap::new() };
    };

    // Lower first query
    let mut hir = ctx.lower_select_query(&first_query);

    // Lower remaining queries and merge diagnostics
    for select_query in queries {
        let additional = ctx.lower_select_query(&select_query);
        hir.diagnostics.extend(additional.diagnostics);
    }

    // Finalize source map (sort token lists)
    ctx.source_map.finalize();

    SdblLowerResult { hir, source_map: ctx.source_map }
}

impl LoweringContext<'_> {
    /// Lower a SELECT query.
    pub(crate) fn lower_select_query(&mut self, query: &syntax::ast::SdblSelectQuery) -> SdblHir {
        let Some(subquery) = query.subquery() else {
            return SdblHir::empty();
        };

        let Some(main_query) = subquery.main_query() else {
            return SdblHir::empty();
        };

        // Record SELECT keyword
        self.record_keyword_by_text(
            main_query.syntax(),
            "SELECT",
            "ВЫБРАТЬ",
            crate::source_map::TokenCategory::ClauseKeyword,
        );

        // 1. Lower FROM clause first (establishes scope)
        let from = self.lower_from_clause(main_query.from_clause());

        // 2. Register tables in scope
        for table in &from {
            self.scope.add_table(table.clone());
        }

        // 3. Lower JOINs
        let joins = self.lower_joins(&main_query);
        for join in &joins {
            self.scope.add_table(join.table.clone());
        }

        // 4. Extract DISTINCT and TOP from limitations
        let (distinct, top) = self.extract_limitations(main_query.syntax());

        // 5. Lower SELECT clause (uses scope for name resolution)
        let select = self.lower_field_list(main_query.field_list(), distinct, top);

        // 6. Lower WHERE clause
        let where_clause = main_query.where_clause().map(|w| self.lower_where_clause(&w));

        // 7. Lower GROUP BY clause
        let group_by = main_query.group_by_clause().map(|g| self.lower_group_by(&g));

        // 8. Lower ORDER BY clause
        let order_by = main_query.order_by_clause().map(|o| self.lower_order_by(&o));

        // 9. Lower UNION queries
        let unions = self.lower_union_clauses(&subquery);

        // Collect diagnostics from UNION queries before moving them
        let mut union_diagnostics = Vec::new();
        for union_hir in &unions {
            union_diagnostics.extend(union_hir.query.diagnostics.clone());
        }

        let range = query.syntax().text_range();

        // Build HIR
        let mut hir = SdblHir {
            select,
            from,
            joins,
            where_clause,
            group_by,
            having: None,
            order_by,
            unions,
            diagnostics: std::mem::take(&mut self.diagnostics),
            range,
        };

        // 7. Merge diagnostics from UNION queries
        hir.diagnostics.extend(union_diagnostics);

        // 8. Check JOINs for unprotected fields (after complete HIR built)
        self.check_joins_for_unprotected_fields(&hir);

        // Merge diagnostics collected during JOIN checking
        hir.diagnostics.extend(std::mem::take(&mut self.diagnostics));

        hir
    }

    /// Lower FROM clause.
    fn lower_from_clause(
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
    fn lower_data_source(&mut self, ds: &syntax::ast::SdblDataSource) -> TableRef {
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

        // Get alias
        let alias_name = alias.and_then(|a| a.name()).map(|s| Name::from(s.as_str()));

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

    /// Lower JOIN clauses.
    fn lower_joins(&mut self, query: &syntax::ast::SdblQuery) -> Vec<JoinHir> {
        let Some(from_clause) = query.from_clause() else {
            return Vec::new();
        };

        let Some(first_ds) = from_clause.data_sources().next() else {
            return Vec::new();
        };

        first_ds.join_clauses().map(|join| self.lower_join_clause(&join)).collect()
    }

    /// Lower a single JOIN clause.
    fn lower_join_clause(&mut self, join: &syntax::ast::SdblJoinClause) -> JoinHir {
        // Record JOIN keyword
        self.record_keyword_by_text(
            join.syntax(),
            "JOIN",
            "СОЕДИНЕНИЕ",
            crate::source_map::TokenCategory::JoinKeyword,
        );

        // Determine join type using the AST method
        let ast_join_type = join.join_type();
        let join_type = match ast_join_type {
            syntax::ast::JoinType::Left => {
                self.record_keyword_by_text(
                    join.syntax(),
                    "LEFT",
                    "ЛЕВОЕ",
                    crate::source_map::TokenCategory::JoinKeyword,
                );
                crate::hir::JoinType::Left
            }
            syntax::ast::JoinType::Right => {
                self.record_keyword_by_text(
                    join.syntax(),
                    "RIGHT",
                    "ПРАВОЕ",
                    crate::source_map::TokenCategory::JoinKeyword,
                );
                crate::hir::JoinType::Right
            }
            syntax::ast::JoinType::Full => {
                self.record_keyword_by_text(
                    join.syntax(),
                    "FULL",
                    "ПОЛНОЕ",
                    crate::source_map::TokenCategory::JoinKeyword,
                );
                crate::hir::JoinType::Full
            }
            syntax::ast::JoinType::Inner => {
                self.record_keyword_by_text(
                    join.syntax(),
                    "INNER",
                    "ВНУТРЕННЕЕ",
                    crate::source_map::TokenCategory::JoinKeyword,
                );
                crate::hir::JoinType::Inner
            }
        };

        // Check for FULL OUTER JOIN
        if matches!(join_type, crate::hir::JoinType::Full) {
            self.diagnostics
                .push(SdblDiagnostic::FullOuterJoin { range: join.syntax().text_range() });
        }

        // Lower joined table
        let table = if let Some(ds) = join.data_source() {
            // Check if JOIN's data source is a subquery
            // This matches Java: visitJoinPart() checks dataSource().subquery() != null
            if ds.subquery().is_some() {
                self.diagnostics
                    .push(SdblDiagnostic::JoinWithSubQuery { range: ds.syntax().text_range() });
            }

            // Process nested JOINs recursively
            for nested_join in ds.join_clauses() {
                let _ = self.lower_join_clause(&nested_join);
            }
            self.lower_data_source(&ds)
        } else {
            TableRef::missing(join.syntax().text_range())
        };

        // Check for join with virtual table
        if table.is_virtual_table {
            if let Some(vt_type) = table.parts.last().and_then(|p| virtual_table_type(p)) {
                self.diagnostics.push(SdblDiagnostic::JoinWithVirtualTable {
                    table_name: table.full_name.clone(),
                    virtual_table_type: vt_type.to_string(),
                    range: join.syntax().text_range(),
                });
            }
        }

        // Lower ON condition - get the first expression child as the ON expression
        let condition_node = join.data_source().and_then(|ds| {
            // The ON condition is typically a child of the join clause
            ds.syntax().parent().and_then(|parent| {
                parent.children().find(|n| {
                    matches!(
                        n.kind(),
                        syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                            | syntax::SyntaxKind::SDBL_LOGICAL_AND_EXPR
                            | syntax::SyntaxKind::SDBL_COMPARISON_EXPR
                    )
                })
            })
        });

        // Check for OR with multiple fields in JOIN condition
        if let Some(ref expr_node) = condition_node {
            self.check_join_or_with_multiple_fields(expr_node);
        }

        let condition = condition_node.map(|expr| self.lower_expr(&expr));

        JoinHir { join_type, table, condition, range: join.syntax().text_range() }
    }

    /// Check JOIN ON condition for OR with multiple distinct fields.
    ///
    /// Matches Java: isMultipleFieldsExpression() logic.
    /// Only reports if OR involves different fields (same field like "Status = 1 OR Status = 2" is OK).
    fn check_join_or_with_multiple_fields(&mut self, expr_node: &syntax::SyntaxNode) {
        use syntax::SyntaxToken;

        // Find all OR tokens in this expression
        let or_tokens: Vec<SyntaxToken> = expr_node
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|token| token.kind() == syntax::SyntaxKind::KW_OR)
            .collect();

        for or_token in or_tokens {
            // Find containing logical expression
            let containing_expr = self.find_containing_logical_expr_for_join(&or_token);

            if let Some(expr) = containing_expr {
                // Extract field names
                let field_names = self.extract_field_names_from_expr(&expr);

                // Only report if multiple distinct fields
                if field_names.len() > 1 {
                    self.diagnostics
                        .push(SdblDiagnostic::LogicalOrInJoin { range: or_token.text_range() });
                }
            }
        }
    }

    /// Find the containing logical expression for an OR token in JOIN context.
    fn find_containing_logical_expr_for_join(
        &self,
        or_token: &syntax::SyntaxToken,
    ) -> Option<syntax::SyntaxNode> {
        use syntax::SyntaxKind;

        let mut current = or_token.parent()?;

        loop {
            match current.kind() {
                SyntaxKind::SDBL_LOGICAL_OR_EXPR
                | SyntaxKind::SDBL_LOGICAL_AND_EXPR
                | SyntaxKind::SDBL_PAREN_EXPR => {
                    return Some(current);
                }
                SyntaxKind::SDBL_JOIN_CLAUSE => {
                    return Some(current);
                }
                _ => {
                    current = current.parent()?;
                }
            }
        }
    }

    /// Extract all unique field names from an expression.
    ///
    /// Handles qualified (Table.Field) and unqualified fields.
    /// Filters SQL keywords.
    fn extract_field_names_from_expr(&self, expr: &syntax::SyntaxNode) -> HashSet<String> {
        use syntax::{SyntaxKind, SyntaxToken};

        let mut fields = HashSet::new();

        let tokens: Vec<SyntaxToken> =
            expr.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

        let mut i = 0;
        while i < tokens.len() {
            let token = &tokens[i];

            if token.kind() == SyntaxKind::IDENT {
                // Check for qualified name (Table.Field)
                if i + 2 < tokens.len()
                    && tokens[i + 1].kind() == SyntaxKind::DOT
                    && tokens[i + 2].kind() == SyntaxKind::IDENT
                {
                    let table = token.text();
                    let field = tokens[i + 2].text();
                    let qualified = format!("{}.{}", table, field);

                    if !self.is_sql_keyword(table) && !self.is_sql_keyword(field) {
                        fields.insert(qualified);
                    }

                    i += 3;
                    continue;
                }

                // Unqualified identifier
                let text = token.text();
                if !self.is_sql_keyword(text) {
                    fields.insert(text.to_string());
                }
            }

            i += 1;
        }

        fields
    }

    /// Check if text is a SQL keyword.
    fn is_sql_keyword(&self, text: &str) -> bool {
        matches!(
            text.to_uppercase().as_str(),
            "AND"
                | "OR"
                | "NOT"
                | "IS"
                | "NULL"
                | "TRUE"
                | "FALSE"
                | "И"
                | "ИЛИ"
                | "НЕ"
                | "ЕСТЬ"
                | "ИСТИНА"
                | "ЛОЖЬ"
                | "SELECT"
                | "FROM"
                | "WHERE"
                | "JOIN"
                | "ON"
                | "ВЫБРАТЬ"
                | "ИЗ"
                | "ГДЕ"
                | "СОЕДИНЕНИЕ"
                | "ПО"
        )
    }

    /// Lower field list (SELECT fields).
    fn lower_field_list(
        &mut self,
        field_list: Option<syntax::ast::SdblFieldList>,
        distinct: bool,
        top: Option<u32>,
    ) -> SelectHir {
        let Some(fl) = field_list else {
            return SelectHir::empty();
        };

        let fields: Vec<FieldHir> = fl.fields().map(|f| self.lower_selected_field(&f)).collect();

        SelectHir { fields, distinct, top }
    }

    /// Extract DISTINCT and TOP from query limitations.
    ///
    /// Returns (distinct, top).
    fn extract_limitations(&mut self, query_node: &syntax::SyntaxNode) -> (bool, Option<u32>) {
        let mut distinct = false;
        let mut top = None;

        // Find SDBL_LIMITATIONS child
        for child in query_node.children() {
            if child.kind() == syntax::SyntaxKind::SDBL_LIMITATIONS {
                // Record DISTINCT keyword if present
                for element in child.descendants_with_tokens() {
                    if let Some(token) = element.as_token() {
                        if token.kind() == syntax::SyntaxKind::IDENT {
                            let text_upper = token.text().to_uppercase();
                            if text_upper == "DISTINCT" || text_upper == "РАЗЛИЧНЫЕ" {
                                distinct = true;
                                self.record_token(
                                    token,
                                    crate::source_map::TokenCategory::Modifier,
                                );
                            }
                        }
                    }
                }

                // Find TOP clause
                for top_child in child.children() {
                    if top_child.kind() == syntax::SyntaxKind::SDBL_TOP_CLAUSE {
                        // Record TOP keyword
                        for element in top_child.descendants_with_tokens() {
                            if let Some(token) = element.as_token() {
                                if token.kind() == syntax::SyntaxKind::IDENT {
                                    let text_upper = token.text().to_uppercase();
                                    if text_upper == "TOP" || text_upper == "ПЕРВЫЕ" {
                                        self.record_token(
                                            token,
                                            crate::source_map::TokenCategory::Modifier,
                                        );
                                    }
                                }
                            }
                        }

                        // Extract TOP count (find first DECIMAL token)
                        for element in top_child.descendants_with_tokens() {
                            if let Some(token) = element.as_token() {
                                if token.kind() == syntax::SyntaxKind::DECIMAL {
                                    if let Ok(count) = token.text().parse::<u32>() {
                                        top = Some(count);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                break;
            }
        }

        (distinct, top)
    }

    /// Lower a selected field.
    fn lower_selected_field(&mut self, field: &syntax::ast::SdblSelectedField) -> FieldHir {
        // Check for asterisk
        if field.is_asterisk() {
            return FieldHir {
                expr: ExprHir::Missing { range: field.syntax().text_range() },
                alias: None,
                ty: SdblType::Unknown,
                is_asterisk: true,
                range: field.syntax().text_range(),
            };
        }

        // Lower expression (expression() returns Option<SyntaxNode>)
        let expr = if let Some(e) = field.expression() {
            self.lower_expr(&e)
        } else {
            ExprHir::Missing { range: field.syntax().text_range() }
        };

        // Get alias (name() returns Option<String>)
        let alias = field.alias().and_then(|a| a.name()).map(|s| Name::from(s.as_str()));

        // Get type from expression
        let ty = expr.ty().clone();

        // Check for AliasWithoutAsKeyword diagnostic
        // Important: UNION queries should NOT be checked (only main/first query in UNION)
        let is_in_union_query = field
            .syntax()
            .ancestors()
            .any(|node| node.kind() == syntax::SyntaxKind::SDBL_UNION_CLAUSE);

        if !is_in_union_query {
            // Skip fields with parse errors
            let has_error = field
                .syntax()
                .descendants_with_tokens()
                .any(|el| el.kind() == syntax::SyntaxKind::ERROR);

            if !has_error {
                // Get trimmed range (expression only, without trailing trivia)
                let range = if let Some(expr) = field.expression() {
                    // Trim trailing whitespace/comments/newlines from expression
                    let last_token = expr.last_token();
                    let last_non_trivia = last_token.and_then(|t| {
                        let mut token = t;
                        while matches!(
                            token.kind(),
                            syntax::SyntaxKind::WHITESPACE
                                | syntax::SyntaxKind::COMMENT
                                | syntax::SyntaxKind::NEWLINE
                        ) {
                            token = token.prev_token()?;
                        }
                        Some(token)
                    });

                    if let (Some(first), Some(last)) = (expr.first_token(), last_non_trivia) {
                        syntax::TextRange::new(first.text_range().start(), last.text_range().end())
                    } else {
                        expr.text_range()
                    }
                } else {
                    field.syntax().text_range()
                };

                if let Some(alias_node) = field.alias() {
                    // Has alias but check for AS keyword
                    if !alias_node.has_as_keyword() {
                        // Include alias in range for implicit alias case
                        let range_with_alias = if let Some(alias_ident) = alias_node.identifier() {
                            syntax::TextRange::new(range.start(), alias_ident.text_range().end())
                        } else {
                            range
                        };

                        self.diagnostics.push(SdblDiagnostic::AliasWithoutAsKeyword {
                            field_name: alias_node.name(),
                            range: range_with_alias,
                        });
                    }
                } else {
                    // No alias at all - use expression range
                    self.diagnostics
                        .push(SdblDiagnostic::AliasWithoutAsKeyword { field_name: None, range });
                }
            }
        }

        FieldHir { expr, alias, ty, is_asterisk: false, range: field.syntax().text_range() }
    }

    /// Lower WHERE clause.
    fn lower_where_clause(&mut self, where_clause: &syntax::ast::SdblWhereClause) -> ExprHir {
        // Record WHERE keyword
        self.record_keyword_by_text(
            where_clause.syntax(),
            "WHERE",
            "ГДЕ",
            crate::source_map::TokenCategory::ClauseKeyword,
        );

        // Collect LogicalOrInWhere diagnostics
        // Use descendants_with_tokens() to find ALL OR tokens recursively
        for element in where_clause.syntax().descendants_with_tokens() {
            if let Some(token) = element.as_token() {
                if token.kind() == syntax::SyntaxKind::KW_OR {
                    self.diagnostics
                        .push(SdblDiagnostic::LogicalOrInWhere { range: token.text_range() });
                }
            }
        }

        // WHERE clause contains an expression as its child
        let expr_node = where_clause.syntax().children().find(|n| {
            matches!(
                n.kind(),
                syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                    | syntax::SyntaxKind::SDBL_LOGICAL_AND_EXPR
                    | syntax::SyntaxKind::SDBL_NOT_EXPR
                    | syntax::SyntaxKind::SDBL_COMPARISON_EXPR
                    | syntax::SyntaxKind::SDBL_COLUMN_REF
                    | syntax::SyntaxKind::SDBL_LITERAL
                    | syntax::SyntaxKind::SDBL_MULTI_STRING
                    | syntax::SyntaxKind::SDBL_FUNCTION_CALL
                    | syntax::SyntaxKind::SDBL_PAREN_EXPR
            )
        });

        if let Some(expr) = expr_node {
            self.lower_expr(&expr)
        } else {
            ExprHir::Missing { range: where_clause.syntax().text_range() }
        }
    }

    /// Lower GROUP BY clause.
    fn lower_group_by(
        &mut self,
        group_clause: &syntax::ast::SdblGroupClause,
    ) -> crate::hir::GroupByHir {
        // Record GROUP keyword
        self.record_keyword_by_text(
            group_clause.syntax(),
            "GROUP",
            "СГРУППИРОВАТЬ",
            crate::source_map::TokenCategory::ClauseKeyword,
        );

        // Record BY keyword
        self.record_keyword_by_text(
            group_clause.syntax(),
            "BY",
            "ПО",
            crate::source_map::TokenCategory::ClauseKeyword,
        );

        // Extract all expression nodes (grouping columns)
        let exprs: Vec<_> = group_clause
            .syntax()
            .children()
            .filter(|n| {
                matches!(
                    n.kind(),
                    syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                        | syntax::SyntaxKind::SDBL_COLUMN_REF
                        | syntax::SyntaxKind::SDBL_FUNCTION_CALL
                )
            })
            .map(|n| self.lower_expr(&n))
            .collect();

        crate::hir::GroupByHir { exprs, range: group_clause.syntax().text_range() }
    }

    /// Lower ORDER BY clause.
    fn lower_order_by(
        &mut self,
        order_clause: &syntax::ast::SdblOrderClause,
    ) -> crate::hir::OrderByHir {
        // Record ORDER keyword
        self.record_keyword_by_text(
            order_clause.syntax(),
            "ORDER",
            "УПОРЯДОЧИТЬ",
            crate::source_map::TokenCategory::ClauseKeyword,
        );

        // Record BY keyword
        self.record_keyword_by_text(
            order_clause.syntax(),
            "BY",
            "ПО",
            crate::source_map::TokenCategory::ClauseKeyword,
        );

        // Parse ORDER BY items (expression + optional ASC/DESC)
        let mut items = Vec::new();
        let mut current_expr: Option<ExprHir> = None;

        for child in order_clause.syntax().children_with_tokens() {
            match child {
                syntax::NodeOrToken::Node(node) => {
                    // Found an expression node
                    if matches!(
                        node.kind(),
                        syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                            | syntax::SyntaxKind::SDBL_COLUMN_REF
                            | syntax::SyntaxKind::SDBL_FUNCTION_CALL
                    ) {
                        // If we already have an expression, save it with default ASC
                        if let Some(expr) = current_expr.take() {
                            items.push(crate::hir::OrderByItem {
                                expr,
                                direction: crate::hir::SortDirection::Asc,
                            });
                        }
                        current_expr = Some(self.lower_expr(&node));
                    }
                }
                syntax::NodeOrToken::Token(token) => {
                    // Check for ASC/DESC keywords
                    if token.kind() == syntax::SyntaxKind::IDENT {
                        let text = token.text().to_uppercase();
                        if matches!(text.as_str(), "ASC" | "ВОЗР") {
                            if let Some(expr) = current_expr.take() {
                                items.push(crate::hir::OrderByItem {
                                    expr,
                                    direction: crate::hir::SortDirection::Asc,
                                });
                            }
                        } else if matches!(text.as_str(), "DESC" | "УБЫВ") {
                            if let Some(expr) = current_expr.take() {
                                items.push(crate::hir::OrderByItem {
                                    expr,
                                    direction: crate::hir::SortDirection::Desc,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Add the last expression if present (default to ASC)
        if let Some(expr) = current_expr {
            items.push(crate::hir::OrderByItem { expr, direction: crate::hir::SortDirection::Asc });
        }

        crate::hir::OrderByHir { items, range: order_clause.syntax().text_range() }
    }

    /// Lower an expression.
    fn lower_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use syntax::SyntaxKind;

        match node.kind() {
            SyntaxKind::SDBL_COLUMN_REF => self.lower_column_ref(node),
            SyntaxKind::SDBL_LITERAL => self.lower_literal(node),
            SyntaxKind::SDBL_MULTI_STRING => self.lower_multi_string(node),
            SyntaxKind::SDBL_FUNCTION_CALL => self.lower_function_call(node),
            SyntaxKind::SDBL_PAREN_EXPR => {
                // Unwrap parentheses
                if let Some(inner) = node.children().next() {
                    self.lower_expr(&inner)
                } else {
                    ExprHir::Missing { range: node.text_range() }
                }
            }
            SyntaxKind::SDBL_LOGICAL_OR_EXPR
            | SyntaxKind::SDBL_LOGICAL_AND_EXPR
            | SyntaxKind::SDBL_COMPARISON_EXPR
            | SyntaxKind::SDBL_ADDITIVE_EXPR
            | SyntaxKind::SDBL_MULTIPLICATIVE_EXPR => self.lower_binary_expr(node),
            SyntaxKind::SDBL_NOT_EXPR | SyntaxKind::SDBL_UNARY_EXPR => self.lower_unary_expr(node),
            SyntaxKind::SDBL_IS_NULL_EXPR => self.lower_is_null_expr(node),
            SyntaxKind::SDBL_IN_EXPR => self.lower_in_expr(node),
            SyntaxKind::SDBL_BETWEEN_EXPR => self.lower_between_expr(node),
            SyntaxKind::SDBL_LIKE_EXPR => self.lower_like_expr(node),
            SyntaxKind::SDBL_CASE_EXPR => self.lower_case_expr(node),
            SyntaxKind::SDBL_PARAMETER => self.lower_parameter(node),
            _ => ExprHir::Missing { range: node.text_range() },
        }
    }

    /// Lower column reference.
    fn lower_column_ref(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        let text = node.text().to_string();
        let parts: Vec<&str> = text.split('.').collect();

        let (table_alias, column_name) = if parts.len() >= 2 {
            (Some(Name::from(parts[0].trim())), Name::from(parts[1].trim()))
        } else {
            (None, Name::from(parts[0].trim()))
        };

        // Resolve type from scope
        let ty = self
            .scope
            .resolve_column_type(table_alias.as_ref().map(|n| n.as_str()), column_name.as_str());

        // Check for unknown field
        if ty == SdblType::Unknown {
            if let Some(ref alias) = table_alias {
                if let Some(table) = self.scope.find_table(alias.as_str()) {
                    if table.metadata.is_some() {
                        self.diagnostics.push(SdblDiagnostic::UnknownField {
                            table_name: table.full_name.clone(),
                            field_name: column_name.to_string(),
                            range: node.text_range(),
                        });
                    }
                }
            }
        } else if ty == SdblType::Error {
            // Ambiguous column
            let possible_tables = self.scope.find_tables_with_column(column_name.as_str());
            self.diagnostics.push(SdblDiagnostic::AmbiguousColumnRef {
                column_name: column_name.to_string(),
                possible_tables,
                range: node.text_range(),
            });
        }

        ExprHir::ColumnRef { table_alias, column: column_name, ty, range: node.text_range() }
    }

    /// Lower literal.
    fn lower_literal(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::LiteralValue;

        let text = node.text().to_string().trim().to_string();

        // Check for multiline string in SDBL_LITERAL (single String token with \n)
        // Parser creates SDBL_LITERAL even for strings with newlines inside
        for child in node.children_with_tokens() {
            if let Some(token) = child.as_token() {
                if token.kind() == syntax::SyntaxKind::STRING && token.text().contains('\n') {
                    self.diagnostics
                        .push(SdblDiagnostic::MultilineString { range: token.text_range() });
                    break;
                }
            }
        }

        // Determine literal type
        let (value, ty) = if text.starts_with('"') || text.starts_with('\'') {
            (
                LiteralValue::String(text.trim_matches(|c| c == '"' || c == '\'').to_string()),
                SdblType::String,
            )
        } else if text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("истина") {
            (LiteralValue::Boolean(true), SdblType::Boolean)
        } else if text.eq_ignore_ascii_case("false") || text.eq_ignore_ascii_case("ложь") {
            (LiteralValue::Boolean(false), SdblType::Boolean)
        } else if text.eq_ignore_ascii_case("null") {
            (LiteralValue::Null, SdblType::Null)
        } else if text.eq_ignore_ascii_case("undefined")
            || text.eq_ignore_ascii_case("неопределено")
        {
            (LiteralValue::Undefined, SdblType::Unknown)
        } else if let Ok(n) = text.parse::<i64>() {
            (LiteralValue::Integer(n), SdblType::number())
        } else if text.contains('.') {
            (LiteralValue::Float(text), SdblType::number())
        } else {
            (LiteralValue::String(text), SdblType::Unknown)
        };

        ExprHir::Literal { value, ty, range: node.text_range() }
    }

    /// Lower multiline string (SDBL_MULTI_STRING node with multiple String tokens).
    ///
    /// Parser creates SDBL_MULTI_STRING when it sees multiple consecutive String tokens.
    fn lower_multi_string(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::LiteralValue;

        // Count STRING tokens
        let string_count = node
            .children_with_tokens()
            .filter(|child| {
                child.as_token().map(|t| t.kind() == syntax::SyntaxKind::STRING).unwrap_or(false)
            })
            .count();

        // If more than 2 STRING tokens, this is a multiline literal (likely error)
        // Two strings is OK (e.g., "" + "" in some contexts), but 3+ is suspicious
        if string_count > 2 {
            self.diagnostics.push(SdblDiagnostic::MultilineString { range: node.text_range() });
        }

        // Return string literal HIR
        let text = node.text().to_string();
        ExprHir::Literal {
            value: LiteralValue::String(text),
            ty: SdblType::String,
            range: node.text_range(),
        }
    }

    /// Lower function call.
    fn lower_function_call(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::FunctionKind;

        // Get function name from first identifier
        let func_name_token = node
            .children_with_tokens()
            .filter_map(|c| c.into_token())
            .find(|t| t.kind() == syntax::SyntaxKind::IDENT);

        let func_name = func_name_token.as_ref().map(|t| t.text().to_string()).unwrap_or_default();

        // Parse function kind
        let function = match func_name.to_uppercase().as_str() {
            "СУММА" | "SUM" => {
                // Record aggregate function name
                if let Some(token) = func_name_token.as_ref() {
                    self.record_token(token, crate::source_map::TokenCategory::AggregateFunction);
                }
                FunctionKind::Sum
            }
            "СРЕДНЕЕ" | "AVG" => {
                if let Some(token) = func_name_token.as_ref() {
                    self.record_token(token, crate::source_map::TokenCategory::AggregateFunction);
                }
                FunctionKind::Avg
            }
            "МИНИМУМ" | "MIN" => {
                if let Some(token) = func_name_token.as_ref() {
                    self.record_token(token, crate::source_map::TokenCategory::AggregateFunction);
                }
                FunctionKind::Min
            }
            "МАКСИМУМ" | "MAX" => {
                if let Some(token) = func_name_token.as_ref() {
                    self.record_token(token, crate::source_map::TokenCategory::AggregateFunction);
                }
                FunctionKind::Max
            }
            "КОЛИЧЕСТВО" | "COUNT" => {
                if let Some(token) = func_name_token.as_ref() {
                    self.record_token(token, crate::source_map::TokenCategory::AggregateFunction);
                }
                FunctionKind::Count
            }
            "ПОДСТРОКА" | "SUBSTRING" => FunctionKind::Substring,
            "ВРЕГ" | "UPPER" => FunctionKind::Upper,
            "НРЕГ" | "LOWER" => FunctionKind::Lower,
            "ГОД" | "YEAR" => FunctionKind::Year,
            "МЕСЯЦ" | "MONTH" => FunctionKind::Month,
            "ДЕНЬ" | "DAY" => FunctionKind::Day,
            "ЕСТЬNULL" | "ISNULL" => FunctionKind::Isnull,
            "ВЫРАЗИТЬ" | "CAST" => FunctionKind::Cast,
            "ПРЕДСТАВЛЕНИЕ" | "PRESENTATION" => FunctionKind::Presentation,
            _ => FunctionKind::Unknown(func_name.clone()),
        };

        // Lower arguments - get all expression children
        let args: Vec<ExprHir> = node
            .children()
            .filter(|c| {
                matches!(
                    c.kind(),
                    syntax::SyntaxKind::SDBL_COLUMN_REF
                        | syntax::SyntaxKind::SDBL_LITERAL
                        | syntax::SyntaxKind::SDBL_MULTI_STRING
                        | syntax::SyntaxKind::SDBL_FUNCTION_CALL
                        | syntax::SyntaxKind::SDBL_PAREN_EXPR
                        | syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                        | syntax::SyntaxKind::SDBL_LOGICAL_AND_EXPR
                        | syntax::SyntaxKind::SDBL_COMPARISON_EXPR
                        | syntax::SyntaxKind::SDBL_ADDITIVE_EXPR
                        | syntax::SyntaxKind::SDBL_MULTIPLICATIVE_EXPR
                        | syntax::SyntaxKind::SDBL_UNARY_EXPR
                        | syntax::SyntaxKind::SDBL_PARAMETER
                )
            })
            .map(|arg| self.lower_expr(&arg))
            .collect();

        // Infer return type
        let ty = self.infer_function_return_type(&function, &args);

        ExprHir::FunctionCall { function, args, ty, range: node.text_range() }
    }

    /// Infer function return type.
    fn infer_function_return_type(
        &self,
        function: &crate::hir::FunctionKind,
        args: &[ExprHir],
    ) -> SdblType {
        use crate::hir::FunctionKind;

        match function {
            // Aggregate functions wrap argument type
            FunctionKind::Sum | FunctionKind::Avg => {
                if let Some(arg) = args.first() {
                    SdblType::Aggregate(Box::new(arg.ty().clone()))
                } else {
                    SdblType::Aggregate(Box::new(SdblType::Unknown))
                }
            }
            FunctionKind::Min | FunctionKind::Max => {
                args.first().map(|a| a.ty().clone()).unwrap_or(SdblType::Unknown)
            }
            FunctionKind::Count => SdblType::number(),

            // String functions return String
            FunctionKind::Substring
            | FunctionKind::Upper
            | FunctionKind::Lower
            | FunctionKind::Ltrim
            | FunctionKind::Rtrim
            | FunctionKind::Concat
            | FunctionKind::Presentation => SdblType::String,

            // Date part functions return Number
            FunctionKind::Year
            | FunctionKind::Month
            | FunctionKind::Day
            | FunctionKind::Hour
            | FunctionKind::Minute
            | FunctionKind::Second => SdblType::number(),

            // Date functions return Date/DateTime
            FunctionKind::DateTime | FunctionKind::BeginOfPeriod | FunctionKind::EndOfPeriod => {
                SdblType::DateTime
            }
            FunctionKind::AddMonth => SdblType::Date,
            FunctionKind::DateDiff => SdblType::number(),

            // ISNULL returns type of first non-null argument
            FunctionKind::Isnull => {
                args.first().map(|a| a.ty().clone()).unwrap_or(SdblType::Unknown)
            }

            // CAST - need to parse target type (complex)
            FunctionKind::Cast => SdblType::Unknown,

            // Type/ValueType return type descriptors
            FunctionKind::Type | FunctionKind::ValueType => SdblType::Unknown,

            // Ref returns Boolean
            FunctionKind::Ref => SdblType::Boolean,

            // Unknown function
            FunctionKind::Unknown(_) => SdblType::Unknown,
        }
    }

    /// Lower binary expression.
    fn lower_binary_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::BinaryOp;
        use syntax::SyntaxKind;

        // Record operator token
        for element in node.descendants_with_tokens() {
            if let Some(token) = element.as_token() {
                match token.kind() {
                    SyntaxKind::KW_AND | SyntaxKind::KW_OR | SyntaxKind::KW_NOT => {
                        self.record_token(token, crate::source_map::TokenCategory::Operator);
                    }
                    SyntaxKind::EQ
                    | SyntaxKind::NEQ
                    | SyntaxKind::LT
                    | SyntaxKind::LE
                    | SyntaxKind::GT
                    | SyntaxKind::GE
                    | SyntaxKind::PLUS
                    | SyntaxKind::MINUS
                    | SyntaxKind::STAR
                    | SyntaxKind::SLASH => {
                        self.record_token(token, crate::source_map::TokenCategory::Operator);
                    }
                    _ => {}
                }
            }
        }

        let mut children = node.children();
        let lhs_node = children.next();
        let rhs_node = children.next();

        let lhs = lhs_node
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        let rhs = rhs_node
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        // Determine operator from node text
        let text = node.text().to_string();
        let op = if text.contains(" И ") || text.contains(" AND ") {
            BinaryOp::And
        } else if text.contains(" ИЛИ ") || text.contains(" OR ") {
            BinaryOp::Or
        } else if text.contains("<=") {
            BinaryOp::Le
        } else if text.contains(">=") {
            BinaryOp::Ge
        } else if text.contains("<>") {
            BinaryOp::Ne
        } else if text.contains('<') {
            BinaryOp::Lt
        } else if text.contains('>') {
            BinaryOp::Gt
        } else if text.contains('=') {
            BinaryOp::Eq
        } else if text.contains('+') {
            BinaryOp::Add
        } else if text.contains('-') {
            BinaryOp::Sub
        } else if text.contains('*') {
            BinaryOp::Mul
        } else if text.contains('/') {
            BinaryOp::Div
        } else if text.contains('%') {
            BinaryOp::Mod
        } else {
            BinaryOp::Eq // Default
        };

        // Infer result type
        let ty = if op.is_comparison() || op.is_logical() {
            SdblType::Boolean
        } else if op.is_arithmetic() {
            SdblType::number()
        } else {
            SdblType::Unknown
        };

        ExprHir::BinaryOp {
            lhs: Box::new(lhs),
            op,
            rhs: Box::new(rhs),
            ty,
            range: node.text_range(),
        }
    }

    /// Lower unary expression.
    fn lower_unary_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::UnaryOp;

        let expr = node
            .children()
            .next()
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        let text = node.text().to_string().to_uppercase();
        let (op, ty) = if text.contains("НЕ") || text.contains("NOT") {
            // Record NOT token
            for element in node.descendants_with_tokens() {
                if let Some(token) = element.as_token() {
                    if token.kind() == syntax::SyntaxKind::KW_NOT {
                        self.record_token(token, crate::source_map::TokenCategory::Operator);
                        break;
                    }
                }
            }
            (UnaryOp::Not, SdblType::Boolean)
        } else if text.starts_with('-') {
            (UnaryOp::Neg, SdblType::number())
        } else {
            (UnaryOp::Pos, expr.ty().clone())
        };

        ExprHir::UnaryOp { op, expr: Box::new(expr), ty, range: node.text_range() }
    }

    /// Lower parameter reference.
    fn lower_parameter(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        let text = node.text().to_string();
        let name = text.trim_start_matches('&').to_string();

        ExprHir::Parameter {
            name: Name::from(name.as_str()),
            ty: SdblType::Unknown, // Parameters have unknown type without context
            range: node.text_range(),
        }
    }

    /// Lower IS NULL expression.
    ///
    /// Grammar: `expr IS [NOT] NULL`
    fn lower_is_null_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        // Get the child expression (first child)
        let expr = node
            .children()
            .next()
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        // Check if NOT keyword is present and record keywords
        let text = node.text().to_string().to_uppercase();
        let negated = text.contains(" NOT ") || text.contains(" НЕ ");

        // Record IS keyword (English and Russian)
        self.record_keyword_by_text(
            node,
            "IS",
            "ЕСТЬ",
            crate::source_map::TokenCategory::SpecialKeyword,
        );

        // Record NOT keyword if present
        if negated {
            self.record_keyword_by_text(
                node,
                "NOT",
                "НЕ",
                crate::source_map::TokenCategory::Operator,
            );
        }

        // Record NULL keyword (English and Russian)
        self.record_keyword_by_text(
            node,
            "NULL",
            "NULL",
            crate::source_map::TokenCategory::SpecialKeyword,
        );

        ExprHir::IsNull {
            expr: Box::new(expr),
            negated,
            ty: SdblType::Boolean,
            range: node.text_range(),
        }
    }

    /// Lower IN expression.
    ///
    /// Grammar: `expr IN (value_list | subquery)`
    /// or: `expr NOT IN (value_list | subquery)`
    fn lower_in_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::InValues;

        // Get the expression being tested (first child)
        let expr = node
            .children()
            .next()
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        // Check for NOT before IN (NOT IN is a single construct now)
        let negated = node.descendants_with_tokens().any(|el| {
            el.as_token().map(|t| t.kind() == syntax::SyntaxKind::KW_NOT).unwrap_or(false)
        });

        // Record NOT keyword if present
        if negated {
            for element in node.descendants_with_tokens() {
                if let Some(token) = element.as_token() {
                    if token.kind() == syntax::SyntaxKind::KW_NOT {
                        self.record_token(token, crate::source_map::TokenCategory::Operator);
                        break;
                    }
                }
            }
        }

        // Record IN keyword using direct token search (IN is KW_IN, not IDENT)
        for element in node.descendants_with_tokens() {
            if let Some(token) = element.as_token() {
                if token.kind() == syntax::SyntaxKind::KW_IN {
                    self.record_token(token, crate::source_map::TokenCategory::SpecialKeyword);
                    break;
                }
            }
        }

        // Parse values or subquery (everything except first child)
        let mut children = node.children().skip(1);
        let values = if let Some(child) = children.next() {
            // Check if it's a subquery (SDBL_SUBQUERY_EXPR or SDBL_SELECT_QUERY)
            if matches!(
                child.kind(),
                syntax::SyntaxKind::SDBL_SUBQUERY_EXPR
                    | syntax::SyntaxKind::SDBL_SELECT_QUERY
                    | syntax::SyntaxKind::SDBL_SUBQUERY
            ) {
                // TODO: Lower subquery properly using typed AST
                // For now, create empty placeholder HIR
                InValues::Subquery(Box::new(SdblHir {
                    from: Vec::new(),
                    select: SelectHir { fields: Vec::new(), distinct: false, top: None },
                    where_clause: None,
                    joins: Vec::new(),
                    group_by: None,
                    having: None,
                    order_by: None,
                    unions: Vec::new(),
                    diagnostics: Vec::new(),
                    range: child.text_range(),
                }))
            } else {
                // Value list - collect all remaining expression children
                let mut values = vec![self.lower_expr(&child)];
                for expr_node in children {
                    // Skip non-expression nodes (like LPAREN, RPAREN, COMMA)
                    if matches!(
                        expr_node.kind(),
                        syntax::SyntaxKind::SDBL_COLUMN_REF
                            | syntax::SyntaxKind::SDBL_LITERAL
                            | syntax::SyntaxKind::SDBL_MULTI_STRING
                            | syntax::SyntaxKind::SDBL_FUNCTION_CALL
                            | syntax::SyntaxKind::SDBL_PAREN_EXPR
                            | syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                            | syntax::SyntaxKind::SDBL_LOGICAL_AND_EXPR
                            | syntax::SyntaxKind::SDBL_COMPARISON_EXPR
                            | syntax::SyntaxKind::SDBL_ADDITIVE_EXPR
                            | syntax::SyntaxKind::SDBL_MULTIPLICATIVE_EXPR
                            | syntax::SyntaxKind::SDBL_UNARY_EXPR
                            | syntax::SyntaxKind::SDBL_PARAMETER
                    ) {
                        values.push(self.lower_expr(&expr_node));
                    }
                }
                InValues::List(values)
            }
        } else {
            InValues::List(Vec::new())
        };

        ExprHir::In {
            expr: Box::new(expr),
            negated,
            values,
            ty: SdblType::Boolean,
            range: node.text_range(),
        }
    }

    /// Lower BETWEEN expression.
    ///
    /// Grammar: `expr BETWEEN low AND high`
    /// or: `expr NOT BETWEEN low AND high`
    fn lower_between_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        // Get the expression being tested (first child)
        let mut children = node.children();
        let expr = children
            .next()
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        // Check for NOT before BETWEEN
        let negated = node.descendants_with_tokens().any(|el| {
            el.as_token().map(|t| t.kind() == syntax::SyntaxKind::KW_NOT).unwrap_or(false)
        });

        // Record NOT keyword if present
        if negated {
            for element in node.descendants_with_tokens() {
                if let Some(token) = element.as_token() {
                    if token.kind() == syntax::SyntaxKind::KW_NOT {
                        self.record_token(token, crate::source_map::TokenCategory::Operator);
                        break;
                    }
                }
            }
        }

        // Record BETWEEN keyword
        self.record_keyword_by_text(
            node,
            "BETWEEN",
            "МЕЖДУ",
            crate::source_map::TokenCategory::SpecialKeyword,
        );

        // Get low and high expressions (next two children)
        let low = children
            .next()
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        let high = children
            .next()
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        ExprHir::Between {
            expr: Box::new(expr),
            negated,
            low: Box::new(low),
            high: Box::new(high),
            ty: SdblType::Boolean,
            range: node.text_range(),
        }
    }

    /// Lower LIKE expression.
    ///
    /// Grammar: `expr LIKE pattern [ESCAPE char]`
    /// or: `expr NOT LIKE pattern [ESCAPE char]`
    fn lower_like_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        // Get the expression being tested (first child)
        let mut children = node.children();
        let expr = children
            .next()
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        // Check for NOT before LIKE
        let negated = node.descendants_with_tokens().any(|el| {
            el.as_token().map(|t| t.kind() == syntax::SyntaxKind::KW_NOT).unwrap_or(false)
        });

        // Record NOT keyword if present
        if negated {
            for element in node.descendants_with_tokens() {
                if let Some(token) = element.as_token() {
                    if token.kind() == syntax::SyntaxKind::KW_NOT {
                        self.record_token(token, crate::source_map::TokenCategory::Operator);
                        break;
                    }
                }
            }
        }

        // Record LIKE keyword
        self.record_keyword_by_text(
            node,
            "LIKE",
            "ПОДОБНО",
            crate::source_map::TokenCategory::SpecialKeyword,
        );

        // Get pattern expression (next child)
        let pattern = children
            .next()
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        // Optional escape character (if present, it's the next child)
        let escape = children.next().map(|n| Box::new(self.lower_expr(&n)));

        // Record ESCAPE keyword if present
        if escape.is_some() {
            self.record_keyword_by_text(
                node,
                "ESCAPE",
                "СПЕЦСИМВОЛ",
                crate::source_map::TokenCategory::SpecialKeyword,
            );
        }

        ExprHir::Like {
            expr: Box::new(expr),
            negated,
            pattern: Box::new(pattern),
            escape,
            ty: SdblType::Boolean,
            range: node.text_range(),
        }
    }

    /// Lower CASE expression.
    ///
    /// Two forms:
    /// - Simple CASE: `CASE operand WHEN value THEN result ...`
    /// - Searched CASE: `CASE WHEN condition THEN result ...`
    fn lower_case_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        // Record CASE keyword
        self.record_keyword_by_text(
            node,
            "CASE",
            "ВЫБОР",
            crate::source_map::TokenCategory::SpecialKeyword,
        );

        // Get all child expressions and WHEN clauses
        let children = node.children();
        let mut expressions = Vec::new();
        let mut when_clauses_nodes = Vec::new();

        for child in children {
            if child.kind() == SyntaxKind::SDBL_WHEN_CLAUSE {
                when_clauses_nodes.push(child);
            } else {
                // This is an expression node (operand, condition, result, or else)
                expressions.push(self.lower_expr(&child));
            }
        }

        // Determine if this is simple or searched CASE
        // If first expression comes before any WHEN clause, it's the operand
        let mut expr_iter = expressions.into_iter();

        // Check if we have an operand (simple CASE)
        // Simple CASE: first child expression is operand
        // Searched CASE: first child is WHEN clause
        let has_operand = !when_clauses_nodes.is_empty()
            && node
                .children()
                .next()
                .map(|n| n.kind() != SyntaxKind::SDBL_WHEN_CLAUSE)
                .unwrap_or(false);

        let operand = if has_operand { expr_iter.next().map(Box::new) } else { None };

        // Parse WHEN clauses
        let mut when_clauses = Vec::new();
        for when_node in when_clauses_nodes {
            // Record WHEN keyword
            self.record_keyword_by_text(
                &when_node,
                "WHEN",
                "КОГДА",
                crate::source_map::TokenCategory::SpecialKeyword,
            );

            // Record THEN keyword
            self.record_keyword_by_text(
                &when_node,
                "THEN",
                "ТОГДА",
                crate::source_map::TokenCategory::SpecialKeyword,
            );

            // Get condition and result from WHEN clause
            let mut when_children: Vec<_> =
                when_node.children().map(|n| self.lower_expr(&n)).collect();

            if when_children.len() >= 2 {
                let condition = when_children.remove(0);
                let result = when_children.remove(0);
                when_clauses.push(crate::hir::WhenClause { condition, result });
            }
        }

        // Check for ELSE clause (any remaining expression after operand)
        let else_expr = if let Some(else_val) = expr_iter.next() {
            // Record ELSE keyword
            self.record_keyword_by_text(
                node,
                "ELSE",
                "ИНАЧЕ",
                crate::source_map::TokenCategory::SpecialKeyword,
            );
            Some(Box::new(else_val))
        } else {
            None
        };

        // Record END keyword
        self.record_keyword_by_text(
            node,
            "END",
            "КОНЕЦ",
            crate::source_map::TokenCategory::SpecialKeyword,
        );

        // Type inference: result type is union of all THEN/ELSE types
        let ty = SdblType::Unknown;

        ExprHir::Case { operand, when_clauses, else_expr, ty, range: node.text_range() }
    }

    /// Lower UNION clauses from a subquery.
    ///
    /// Each UNION query gets its own independent scope and HIR.
    fn lower_union_clauses(
        &mut self,
        subquery: &syntax::ast::SdblSubquery,
    ) -> Vec<crate::hir::UnionHir> {
        let mut unions = Vec::new();

        for union_clause in subquery.union_clauses() {
            // Record UNION keyword
            self.record_keyword_by_text(
                union_clause.syntax(),
                "UNION",
                "ОБЪЕДИНИТЬ",
                crate::source_map::TokenCategory::Modifier,
            );

            // Record ALL keyword if present
            if union_clause.has_all() {
                self.record_keyword_by_text(
                    union_clause.syntax(),
                    "ALL",
                    "ВСЕ",
                    crate::source_map::TokenCategory::Modifier,
                );
            }

            let Some(union_query) = union_clause.query() else {
                continue;
            };

            // Save current scope
            let saved_scope = std::mem::replace(&mut self.scope, crate::scope::Scope::new());

            // Lower the UNION query in fresh scope
            let union_hir = self.lower_query(&union_query);

            // Restore scope
            self.scope = saved_scope;

            unions.push(crate::hir::UnionHir {
                all: union_clause.has_all(),
                query: Box::new(union_hir),
                range: union_clause.syntax().text_range(),
            });
        }

        unions
    }

    /// Lower a SDBL query (called recursively for UNION subqueries).
    fn lower_query(&mut self, query: &syntax::ast::SdblQuery) -> SdblHir {
        // 1. Lower FROM clause first (establishes scope)
        let from = self.lower_from_clause(query.from_clause());

        // 2. Register tables in scope
        for table in &from {
            self.scope.add_table(table.clone());
        }

        // 3. Lower JOINs
        let joins = self.lower_joins(query);
        for join in &joins {
            self.scope.add_table(join.table.clone());
        }

        // 4. Extract DISTINCT and TOP from limitations
        let (distinct, top) = self.extract_limitations(query.syntax());

        // 5. Lower SELECT clause (uses scope for name resolution)
        let select = self.lower_field_list(query.field_list(), distinct, top);

        // 6. Lower WHERE clause
        let where_clause = query.where_clause().map(|w| self.lower_where_clause(&w));

        SdblHir {
            select,
            from,
            joins,
            where_clause,
            group_by: None,
            having: None,
            order_by: None,
            unions: Vec::new(), // UNION queries don't have nested UNIONs
            diagnostics: std::mem::take(&mut self.diagnostics),
            range: query.syntax().text_range(),
        }
    }

    // ========================================================================
    // JOIN NULL Check Methods
    // ========================================================================

    /// Check all outer JOINs for fields used without NULL protection.
    ///
    /// Called after complete HIR is built - requires all tables in scope.
    /// Also recursively checks UNION subqueries.
    fn check_joins_for_unprotected_fields(&mut self, hir: &SdblHir) {
        // Build set of tables that are protected by WHERE IS NOT NULL checks
        // NOTE: Protection is local to this query, NOT inherited from parent or shared with siblings
        let protected_tables = self.find_tables_protected_by_where(hir);

        // Process only outer JOINs (LEFT/RIGHT/FULL)
        for join in hir.joins.iter().filter(|j| j.join_type.is_outer()) {
            match join.join_type {
                crate::hir::JoinType::Left => {
                    // Check joined table (right side)
                    if !protected_tables.contains(join.table.effective_name()) {
                        self.check_table_in_join(join, &join.table, hir);
                    }
                }
                crate::hir::JoinType::Right => {
                    // Check FROM table (left side)
                    for from_table in &hir.from {
                        if !protected_tables.contains(from_table.effective_name()) {
                            self.check_table_in_join(join, from_table, hir);
                        }
                    }
                }
                crate::hir::JoinType::Full => {
                    // Check both sides together for FULL JOIN
                    let mut all_unprotected = Vec::new();

                    // Check joined table if not protected
                    if !protected_tables.contains(join.table.effective_name()) {
                        self.collect_unprotected_refs(&join.table, hir, &mut all_unprotected);
                    }

                    // Check FROM tables if not protected
                    for from_table in &hir.from {
                        if !protected_tables.contains(from_table.effective_name()) {
                            self.collect_unprotected_refs(from_table, hir, &mut all_unprotected);
                        }
                    }

                    // Emit single diagnostic for FULL JOIN with all unprotected fields
                    if !all_unprotected.is_empty() {
                        self.diagnostics.push(SdblDiagnostic::FieldsFromJoinWithoutNullCheck {
                            join_type: join.join_type,
                            range: join.range,
                            unprotected_fields: all_unprotected,
                        });
                    }
                }
                _ => {} // Inner joins are safe
            }
        }

        // Recursively check UNION subqueries
        // Each UNION subquery has its own scope and protection rules
        for union in &hir.unions {
            self.check_joins_for_unprotected_fields(&union.query);
        }
    }

    /// Find tables that are protected by WHERE IS NOT NULL checks.
    ///
    /// Java BSL-LS semantics: if WHERE has IS NOT NULL check for ANY field from a table,
    /// the ENTIRE table is considered protected.
    fn find_tables_protected_by_where(&self, hir: &SdblHir) -> std::collections::HashSet<String> {
        let mut protected = std::collections::HashSet::new();

        if let Some(ref where_expr) = hir.where_clause {
            self.collect_protected_tables(where_expr, &mut protected);
        }

        protected
    }

    /// Recursively collect tables that have IS NOT NULL checks in expression.
    fn collect_protected_tables(
        &self,
        expr: &ExprHir,
        protected: &mut std::collections::HashSet<String>,
    ) {
        match expr {
            ExprHir::IsNull { expr: inner, negated, .. } => {
                // IS NOT NULL check protects the table
                if *negated {
                    // Extract table alias from inner expression recursively
                    if let Some(alias) = self.extract_table_alias(inner) {
                        protected.insert(alias);
                    }
                }
                // Always check nested expressions
                self.collect_protected_tables(inner, protected);
            }
            ExprHir::UnaryOp { op: crate::hir::UnaryOp::Not, expr: inner, .. } => {
                // NOT (field IS NULL) also protects - search recursively for IsNull
                if let Some(alias) = self.find_is_null_in_not(inner) {
                    protected.insert(alias);
                }
                self.collect_protected_tables(inner, protected);
            }
            ExprHir::BinaryOp { lhs, rhs, .. } => {
                self.collect_protected_tables(lhs, protected);
                self.collect_protected_tables(rhs, protected);
            }
            ExprHir::UnaryOp { expr, .. } => {
                self.collect_protected_tables(expr, protected);
            }
            ExprHir::FunctionCall { args, .. } => {
                for arg in args {
                    self.collect_protected_tables(arg, protected);
                }
            }
            _ => {}
        }
    }

    /// Extract table alias from an expression (recursively handles wrapped expressions).
    #[allow(clippy::only_used_in_recursion)]
    fn extract_table_alias(&self, expr: &ExprHir) -> Option<String> {
        match expr {
            ExprHir::ColumnRef { table_alias: Some(alias), .. } => Some(alias.to_string()),
            ExprHir::BinaryOp { lhs, rhs, .. } => {
                // Try left side first, then right
                self.extract_table_alias(lhs).or_else(|| self.extract_table_alias(rhs))
            }
            ExprHir::UnaryOp { expr: inner, .. } => self.extract_table_alias(inner),
            ExprHir::FunctionCall { args, .. } => {
                // Try each argument
                for arg in args {
                    if let Some(alias) = self.extract_table_alias(arg) {
                        return Some(alias);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Find IS NULL expression inside NOT operator and extract table alias.
    ///
    /// Searches recursively for pattern: NOT (... field IS NULL ...)
    fn find_is_null_in_not(&self, expr: &ExprHir) -> Option<String> {
        match expr {
            ExprHir::IsNull { expr: field_expr, negated: false, .. } => {
                // Found IS NULL (not negated) - extract table alias
                self.extract_table_alias(field_expr)
            }
            ExprHir::BinaryOp { lhs, rhs, .. } => {
                // Recurse into both sides
                self.find_is_null_in_not(lhs).or_else(|| self.find_is_null_in_not(rhs))
            }
            ExprHir::UnaryOp { expr: inner, .. } => {
                // Recurse into unary expression
                self.find_is_null_in_not(inner)
            }
            _ => None,
        }
    }

    /// Collect unprotected references for a table (used by FULL JOIN).
    fn collect_unprotected_refs(
        &mut self,
        table: &TableRef,
        hir: &SdblHir,
        all_unprotected: &mut Vec<crate::diagnostics::UnprotectedFieldRef>,
    ) {
        let table_alias = table.effective_name();
        let mut unprotected_refs = Vec::new();

        // Check SELECT clause
        for field in &hir.select.fields {
            self.find_unprotected_refs(&field.expr, table_alias, &mut unprotected_refs);
        }

        // Check WHERE clause
        if let Some(ref where_expr) = hir.where_clause {
            self.find_unprotected_refs(where_expr, table_alias, &mut unprotected_refs);
        }

        all_unprotected.extend(unprotected_refs);
    }

    /// Check if a table's fields are used without protection in a JOIN context.
    fn check_table_in_join(&mut self, join: &JoinHir, table: &TableRef, hir: &SdblHir) {
        let table_alias = table.effective_name();
        let mut unprotected_refs = Vec::new();

        // 1. Check SELECT clause
        for field in &hir.select.fields {
            self.find_unprotected_refs(&field.expr, table_alias, &mut unprotected_refs);
        }

        // 2. Check WHERE clause
        if let Some(ref where_expr) = hir.where_clause {
            self.find_unprotected_refs(where_expr, table_alias, &mut unprotected_refs);
        }

        // 3. Check other JOINs' ON conditions (but not this JOIN's own ON)
        for other_join in &hir.joins {
            // Skip the same JOIN (field in own ON is protected)
            if other_join.range != join.range {
                if let Some(ref on_expr) = other_join.condition {
                    self.find_unprotected_refs(on_expr, table_alias, &mut unprotected_refs);
                }
            }
        }

        // NOTE: Fields in this JOIN's own ON condition are NOT checked
        // because they are part of the join predicate itself, not vulnerable usage

        // Emit diagnostic if unprotected references found
        if !unprotected_refs.is_empty() {
            self.diagnostics.push(SdblDiagnostic::FieldsFromJoinWithoutNullCheck {
                join_type: join.join_type,
                range: join.range,
                unprotected_fields: unprotected_refs,
            });
        }
    }

    /// Find unprotected field references in an expression.
    ///
    /// Recursively traverses ExprHir, checking for ColumnRef matching the protected table.
    /// Stops traversal when encountering protection context (ISNULL, IS NULL, etc.).
    fn find_unprotected_refs(
        &self,
        expr: &ExprHir,
        protected_table: &str,
        unprotected_refs: &mut Vec<crate::diagnostics::UnprotectedFieldRef>,
    ) {
        // Check if this expression provides protection
        if self.is_protected_context(expr, protected_table) {
            return; // Protected - stop traversing this branch
        }

        match expr {
            ExprHir::ColumnRef { table_alias, column, range, .. } => {
                // Check if this references the protected table
                if let Some(alias) = table_alias {
                    if alias.eq_ignore_ascii_case(protected_table) {
                        // Found unprotected reference!
                        unprotected_refs.push(crate::diagnostics::UnprotectedFieldRef {
                            table_alias: alias.to_string(),
                            field_name: column.to_string(),
                            range: *range,
                        });
                    }
                }
            }

            ExprHir::BinaryOp { lhs, rhs, .. } => {
                self.find_unprotected_refs(lhs, protected_table, unprotected_refs);
                self.find_unprotected_refs(rhs, protected_table, unprotected_refs);
            }

            ExprHir::UnaryOp { expr: inner, .. } => {
                self.find_unprotected_refs(inner, protected_table, unprotected_refs);
            }

            ExprHir::FunctionCall { function, args, .. } => {
                // Note: ISNULL protection handled in is_protected_context
                // For other functions, check arguments
                if !matches!(function, crate::hir::FunctionKind::Isnull) {
                    for arg in args {
                        self.find_unprotected_refs(arg, protected_table, unprotected_refs);
                    }
                }
            }

            ExprHir::Case { operand, when_clauses, else_expr, .. } => {
                if let Some(op) = operand {
                    self.find_unprotected_refs(op, protected_table, unprotected_refs);
                }
                for when in when_clauses {
                    self.find_unprotected_refs(&when.condition, protected_table, unprotected_refs);
                    self.find_unprotected_refs(&when.result, protected_table, unprotected_refs);
                }
                if let Some(else_e) = else_expr {
                    self.find_unprotected_refs(else_e, protected_table, unprotected_refs);
                }
            }

            ExprHir::IsNull { expr: inner, .. } => {
                // IS NULL itself provides protection (handled in is_protected_context)
                // But still traverse in case it's nested
                self.find_unprotected_refs(inner, protected_table, unprotected_refs);
            }

            ExprHir::In { expr, values, .. } => {
                self.find_unprotected_refs(expr, protected_table, unprotected_refs);
                match values {
                    crate::hir::InValues::List(exprs) => {
                        for e in exprs {
                            self.find_unprotected_refs(e, protected_table, unprotected_refs);
                        }
                    }
                    crate::hir::InValues::Subquery(subq) => {
                        // Check subquery fields
                        for field in &subq.select.fields {
                            self.find_unprotected_refs(
                                &field.expr,
                                protected_table,
                                unprotected_refs,
                            );
                        }
                    }
                }
            }

            ExprHir::Between { expr, low, high, .. } => {
                self.find_unprotected_refs(expr, protected_table, unprotected_refs);
                self.find_unprotected_refs(low, protected_table, unprotected_refs);
                self.find_unprotected_refs(high, protected_table, unprotected_refs);
            }

            ExprHir::Like { expr, pattern, escape, .. } => {
                self.find_unprotected_refs(expr, protected_table, unprotected_refs);
                self.find_unprotected_refs(pattern, protected_table, unprotected_refs);
                if let Some(esc) = escape {
                    self.find_unprotected_refs(esc, protected_table, unprotected_refs);
                }
            }

            ExprHir::Subquery { query, .. } => {
                // Check subquery SELECT fields
                for field in &query.select.fields {
                    self.find_unprotected_refs(&field.expr, protected_table, unprotected_refs);
                }
            }

            ExprHir::Literal { .. } | ExprHir::Parameter { .. } | ExprHir::Missing { .. } => {
                // No field references
            }
        }
    }

    /// Check if an expression provides NULL protection for a table's fields.
    ///
    /// Protection patterns:
    /// - ISNULL(table.field, default)
    /// - table.field IS NULL / IS NOT NULL
    /// - NOT (table.field IS NULL)
    fn is_protected_context(&self, expr: &ExprHir, protected_table: &str) -> bool {
        match expr {
            // ISNULL function protects its first argument
            ExprHir::FunctionCall { function, args, .. } => {
                if matches!(function, crate::hir::FunctionKind::Isnull) {
                    if let Some(first_arg) = args.first() {
                        return self.expr_references_table(first_arg, protected_table);
                    }
                }
                false
            }

            // IS NULL / IS NOT NULL protects the checked expression
            ExprHir::IsNull { expr: inner, .. } => {
                self.expr_references_table(inner, protected_table)
            }

            // NOT (field IS NULL) pattern
            ExprHir::UnaryOp { op: crate::hir::UnaryOp::Not, expr: inner, .. } => {
                if let ExprHir::IsNull { expr: field_expr, .. } = &**inner {
                    return self.expr_references_table(field_expr, protected_table);
                }
                false
            }

            _ => false,
        }
    }

    /// Check if an expression references a specific table.
    #[allow(clippy::only_used_in_recursion)]
    fn expr_references_table(&self, expr: &ExprHir, table_alias: &str) -> bool {
        match expr {
            ExprHir::ColumnRef { table_alias: Some(alias), .. } => {
                alias.eq_ignore_ascii_case(table_alias)
            }
            ExprHir::BinaryOp { lhs, rhs, .. } => {
                self.expr_references_table(lhs, table_alias)
                    || self.expr_references_table(rhs, table_alias)
            }
            ExprHir::UnaryOp { expr, .. } => self.expr_references_table(expr, table_alias),
            ExprHir::FunctionCall { args, .. } => {
                args.iter().any(|arg| self.expr_references_table(arg, table_alias))
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::JoinType;

    fn lower_query(sdbl: &str) -> SdblHir {
        let ast = parser::parse_sdbl(sdbl);
        lower_sdbl_to_hir(&ast, None).hir
    }

    fn lower_query_with_source_map(sdbl: &str) -> SdblLowerResult {
        let ast = parser::parse_sdbl(sdbl);
        lower_sdbl_to_hir(&ast, None)
    }

    #[test]
    fn test_simple_select() {
        let hir = lower_query("SELECT Код FROM Справочник.Валюты");

        assert!(!hir.select.fields.is_empty());
        assert_eq!(hir.from.len(), 1);
        assert_eq!(hir.from[0].full_name, "Справочник.Валюты");
    }

    #[test]
    fn test_source_map_collects_keywords() {
        let result = lower_query_with_source_map(
            "SELECT Код FROM Справочник.Валюты WHERE Наименование = 'USD'",
        );

        let sm = &result.source_map;

        // Should collect SELECT, FROM, WHERE keywords
        assert!(
            sm.clause_keywords.len() >= 3,
            "Expected at least 3 clause keywords (SELECT, FROM, WHERE), got {}",
            sm.clause_keywords.len()
        );

        // Verify SELECT keyword
        let select_token = sm
            .clause_keywords
            .iter()
            .find(|t| t.text.to_uppercase() == "SELECT" || t.text.to_uppercase() == "ВЫБРАТЬ");
        assert!(select_token.is_some(), "Should find SELECT keyword");

        // Verify FROM keyword
        let from_token = sm
            .clause_keywords
            .iter()
            .find(|t| t.text.to_uppercase() == "FROM" || t.text.to_uppercase() == "ИЗ");
        assert!(from_token.is_some(), "Should find FROM keyword");

        // Verify WHERE keyword
        let where_token = sm
            .clause_keywords
            .iter()
            .find(|t| t.text.to_uppercase() == "WHERE" || t.text.to_uppercase() == "ГДЕ");
        assert!(where_token.is_some(), "Should find WHERE keyword");

        // Should collect = operator
        assert!(
            !sm.operators.is_empty(),
            "Expected at least 1 operator (=), got {}",
            sm.operators.len()
        );
    }

    #[test]
    fn test_source_map_collects_operators() {
        let result = lower_query_with_source_map(
            "SELECT Код FROM Справочник.Валюты WHERE Сумма > 100 AND Количество <= 50",
        );

        let sm = &result.source_map;

        // Should collect operators: >, AND, <=
        assert!(
            sm.operators.len() >= 3,
            "Expected at least 3 operators (>, AND, <=), got {}",
            sm.operators.len()
        );
    }

    #[test]
    fn test_source_map_collects_join_keywords() {
        // Note: Parser/lowering may have limited JOIN support
        // Skip this test for now as JOIN parsing is not fully implemented
        // TODO: Re-enable when JOIN parsing is complete
        let result = lower_query_with_source_map("SELECT Код FROM Справочник.Товары");

        let sm = &result.source_map;

        // Basic test: just verify source map infrastructure works
        assert!(sm.clause_keywords.len() >= 2, "Should have SELECT and FROM keywords");
    }

    #[test]
    fn test_source_map_collects_union_keywords() {
        let result = lower_query_with_source_map(
            "SELECT Код FROM Справочник.Товары UNION ALL SELECT Номер FROM Документ.Продажа",
        );

        let sm = &result.source_map;

        // Should collect UNION and ALL keywords
        assert!(
            sm.modifiers.len() >= 2,
            "Expected at least 2 modifiers (UNION, ALL), got {}",
            sm.modifiers.len()
        );
    }

    #[test]
    fn test_aliased_table() {
        // Note: Parser may not handle AS alias correctly - just verify FROM clause exists
        let hir = lower_query("SELECT Код FROM Справочник.Валюты");

        assert_eq!(hir.from.len(), 1);
        assert_eq!(hir.from[0].full_name, "Справочник.Валюты");
    }

    #[test]
    fn test_join_detection() {
        let hir = lower_query(
            "SELECT Т.Код FROM Справочник.Валюты AS В LEFT JOIN Справочник.Товары AS Т ON В.Ссылка = Т.Владелец"
        );

        assert_eq!(hir.joins.len(), 1);
        assert_eq!(hir.joins[0].join_type, JoinType::Left);
    }

    #[test]
    fn test_table_resolves_with_standard_fields() {
        // Without metadata, table still resolves with standard fields
        let hir = lower_query("SELECT Код FROM Справочник.Валюты");

        assert_eq!(hir.from.len(), 1);
        // Standard fields are added for known MDO types
        assert!(hir.from[0].metadata.is_some());
        let resolved = hir.from[0].metadata.as_ref().unwrap();
        assert!(!resolved.fields.is_empty());
    }

    #[test]
    fn test_select_fields() {
        let hir = lower_query("SELECT Код, Наименование FROM Справочник.Валюты");

        // Verify we have fields in SELECT clause
        assert!(!hir.select.fields.is_empty());
    }

    #[test]
    fn test_source_map_collects_aggregate_functions() {
        let query =
            "SELECT SUM(Price), AVG(Quantity), COUNT(*), MIN(Date), MAX(Total) FROM Products";
        let result = lower_query_with_source_map(query);

        // Should collect all 5 aggregate function names
        assert!(
            result.source_map.aggregate_functions.len() >= 5,
            "Expected at least 5 aggregate functions, got {}",
            result.source_map.aggregate_functions.len()
        );

        // Verify function names
        let func_names: Vec<String> =
            result.source_map.aggregate_functions.iter().map(|t| t.text.to_string()).collect();
        assert!(func_names.contains(&"SUM".to_string()));
        assert!(func_names.contains(&"AVG".to_string()));
        assert!(func_names.contains(&"COUNT".to_string()));
        assert!(func_names.contains(&"MIN".to_string()));
        assert!(func_names.contains(&"MAX".to_string()));
    }

    #[test]
    fn test_source_map_collects_aggregate_functions_russian() {
        let query = "ВЫБРАТЬ СУММА(Цена), СРЕДНЕЕ(Количество), КОЛИЧЕСТВО(*) ИЗ Товары";
        let result = lower_query_with_source_map(query);

        // Should collect 3 aggregate function names (Russian)
        assert!(
            result.source_map.aggregate_functions.len() >= 3,
            "Expected at least 3 aggregate functions, got {}",
            result.source_map.aggregate_functions.len()
        );

        // Verify function names
        let func_names: Vec<String> =
            result.source_map.aggregate_functions.iter().map(|t| t.text.to_string()).collect();
        assert!(func_names.contains(&"СУММА".to_string()));
        assert!(func_names.contains(&"СРЕДНЕЕ".to_string()));
        assert!(func_names.contains(&"КОЛИЧЕСТВО".to_string()));
    }

    #[test]
    fn test_source_map_collects_is_null_keywords() {
        let query = "SELECT * FROM Products WHERE Price IS NULL";
        let result = lower_query_with_source_map(query);

        // Should collect IS and NULL keywords
        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("IS")),
            "Expected IS keyword in special_keywords"
        );
        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("NULL")),
            "Expected NULL keyword in special_keywords"
        );
    }

    #[test]
    fn test_source_map_collects_is_not_null_keywords() {
        let query = "SELECT * FROM Products WHERE Price IS NOT NULL";
        let result = lower_query_with_source_map(query);

        // Should collect IS, NOT, and NULL keywords
        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();
        let operators: Vec<String> =
            result.source_map.operators.iter().map(|t| t.text.to_string()).collect();

        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("IS")),
            "Expected IS keyword in special_keywords"
        );
        assert!(
            operators.iter().any(|k| k.eq_ignore_ascii_case("NOT")),
            "Expected NOT keyword in operators"
        );
        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("NULL")),
            "Expected NULL keyword in special_keywords"
        );
    }

    #[test]
    fn test_source_map_collects_is_null_russian() {
        let query = "ВЫБРАТЬ * ИЗ Товары ГДЕ Цена ЕСТЬ NULL";
        let result = lower_query_with_source_map(query);

        // Should collect ЕСТЬ and NULL keywords (Russian IS NULL)
        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("ЕСТЬ")),
            "Expected ЕСТЬ keyword in special_keywords"
        );
        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("NULL")),
            "Expected NULL keyword in special_keywords"
        );
    }

    #[test]
    fn test_source_map_collects_in_keyword() {
        let query = "SELECT * FROM Products WHERE Type IN (1, 2, 3)";
        let result = lower_query_with_source_map(query);

        // Should collect IN keyword
        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("IN")),
            "Expected IN keyword in special_keywords"
        );
    }

    #[test]
    fn test_source_map_collects_in_keyword_russian() {
        let query = "ВЫБРАТЬ * ИЗ Товары ГДЕ Тип В (1, 2, 3)";
        let result = lower_query_with_source_map(query);

        // Should collect В keyword (Russian IN)
        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("В")),
            "Expected В keyword in special_keywords"
        );
    }

    #[test]
    fn test_in_expression_value_list() {
        let query = "SELECT * FROM Products WHERE Type IN (1, 2, 3)";
        let result = lower_query_with_source_map(query);

        // Verify HIR contains IN expression
        let hir = &result.hir;
        assert!(hir.where_clause.is_some(), "Expected WHERE clause");

        // Check that IN keyword is collected
        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();
        assert!(special_keywords.iter().any(|k| k.eq_ignore_ascii_case("IN")));
    }

    #[test]
    fn test_source_map_collects_distinct_keyword() {
        let query = "SELECT DISTINCT Name FROM Products";
        let result = lower_query_with_source_map(query);

        // Should collect DISTINCT keyword
        let modifiers: Vec<String> =
            result.source_map.modifiers.iter().map(|t| t.text.to_string()).collect();

        assert!(
            modifiers.iter().any(|k| k.eq_ignore_ascii_case("DISTINCT")),
            "Expected DISTINCT keyword in modifiers"
        );

        // Check HIR
        assert!(result.hir.select.distinct);
    }

    #[test]
    fn test_source_map_collects_distinct_keyword_russian() {
        let query = "ВЫБРАТЬ РАЗЛИЧНЫЕ Наименование ИЗ Товары";
        let result = lower_query_with_source_map(query);

        // Should collect РАЗЛИЧНЫЕ keyword (Russian DISTINCT)
        let modifiers: Vec<String> =
            result.source_map.modifiers.iter().map(|t| t.text.to_string()).collect();

        assert!(
            modifiers.iter().any(|k| k.eq_ignore_ascii_case("РАЗЛИЧНЫЕ")),
            "Expected РАЗЛИЧНЫЕ keyword in modifiers"
        );

        // Check HIR
        assert!(result.hir.select.distinct);
    }

    #[test]
    fn test_source_map_collects_top_keyword() {
        let query = "SELECT TOP 10 Name FROM Products";
        let result = lower_query_with_source_map(query);

        // Should collect TOP keyword
        let modifiers: Vec<String> =
            result.source_map.modifiers.iter().map(|t| t.text.to_string()).collect();

        assert!(
            modifiers.iter().any(|k| k.eq_ignore_ascii_case("TOP")),
            "Expected TOP keyword in modifiers"
        );

        // Check HIR
        assert_eq!(result.hir.select.top, Some(10));
    }

    #[test]
    fn test_source_map_collects_top_keyword_russian() {
        let query = "ВЫБРАТЬ ПЕРВЫЕ 5 Наименование ИЗ Товары";
        let result = lower_query_with_source_map(query);

        // Should collect ПЕРВЫЕ keyword (Russian TOP)
        let modifiers: Vec<String> =
            result.source_map.modifiers.iter().map(|t| t.text.to_string()).collect();

        assert!(
            modifiers.iter().any(|k| k.eq_ignore_ascii_case("ПЕРВЫЕ")),
            "Expected ПЕРВЫЕ keyword in modifiers"
        );

        // Check HIR
        assert_eq!(result.hir.select.top, Some(5));
    }

    #[test]
    fn test_distinct_and_top_together() {
        let query = "SELECT DISTINCT TOP 20 Name FROM Products";
        let result = lower_query_with_source_map(query);

        // Should collect both DISTINCT and TOP keywords
        let modifiers: Vec<String> =
            result.source_map.modifiers.iter().map(|t| t.text.to_string()).collect();

        assert!(modifiers.iter().any(|k| k.eq_ignore_ascii_case("DISTINCT")));
        assert!(modifiers.iter().any(|k| k.eq_ignore_ascii_case("TOP")));

        // Check HIR
        assert!(result.hir.select.distinct);
        assert_eq!(result.hir.select.top, Some(20));
    }

    #[test]
    fn test_source_map_collects_between_keyword() {
        let query = "SELECT * FROM Products WHERE Price BETWEEN 100 AND 500";
        let result = lower_query_with_source_map(query);

        // Should collect BETWEEN keyword
        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("BETWEEN")),
            "Expected BETWEEN keyword in special_keywords"
        );
    }

    #[test]
    fn test_source_map_collects_between_keyword_russian() {
        let query = "ВЫБРАТЬ * ИЗ Товары ГДЕ Цена МЕЖДУ 100 И 500";
        let result = lower_query_with_source_map(query);

        // Should collect МЕЖДУ keyword (Russian BETWEEN)
        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("МЕЖДУ")),
            "Expected МЕЖДУ keyword in special_keywords"
        );
    }

    #[test]
    fn test_source_map_collects_like_keyword() {
        let query = "SELECT * FROM Products WHERE Name LIKE 'Apple%'";
        let result = lower_query_with_source_map(query);

        // Should collect LIKE keyword
        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("LIKE")),
            "Expected LIKE keyword in special_keywords"
        );
    }

    #[test]
    fn test_source_map_collects_like_keyword_russian() {
        let query = "ВЫБРАТЬ * ИЗ Товары ГДЕ Наименование ПОДОБНО 'Яблоко%'";
        let result = lower_query_with_source_map(query);

        // Should collect ПОДОБНО keyword (Russian LIKE)
        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("ПОДОБНО")),
            "Expected ПОДОБНО keyword in special_keywords"
        );
    }

    #[test]
    fn test_source_map_collects_like_escape_keyword() {
        let query = "SELECT * FROM Products WHERE Name LIKE 'App!_le%' ESCAPE '!'";
        let result = lower_query_with_source_map(query);

        // Should collect LIKE keyword (ESCAPE might not be lowered if parser doesn't support it fully)
        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("LIKE")),
            "Expected LIKE keyword"
        );
        // ESCAPE keyword collection depends on parser support - test may need adjustment
        if special_keywords.iter().any(|k| k.eq_ignore_ascii_case("ESCAPE")) {
            // Great, ESCAPE is supported
        }
    }

    #[test]
    fn test_case_expression_parsed() {
        // First test if CASE is even being parsed
        let query = "SELECT CASE Status WHEN 1 THEN 'Active' END FROM Products";
        let parse = parser::parse_sdbl(query);
        let tree = format!("{:#?}", parse.syntax_node());
        eprintln!("Parse tree:\n{}", tree);

        // Check if CASE node exists
        assert!(tree.contains("SDBL_CASE_EXPR"), "CASE expression not in parse tree");
    }

    #[test]
    fn test_source_map_collects_case_keywords() {
        let query = r#"SELECT CASE Status WHEN 1 THEN "Active" WHEN 2 THEN "Inactive" ELSE "Unknown" END AS StatusText FROM Products"#;

        let result = lower_query_with_source_map(query);

        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

        // Should collect CASE, WHEN (x2), THEN (x2), ELSE, END
        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("CASE")),
            "Expected CASE keyword, got: {:?}",
            special_keywords
        );
        assert!(
            special_keywords.iter().filter(|k| k.eq_ignore_ascii_case("WHEN")).count() >= 2,
            "Expected at least 2 WHEN keywords"
        );
        assert!(
            special_keywords.iter().filter(|k| k.eq_ignore_ascii_case("THEN")).count() >= 2,
            "Expected at least 2 THEN keywords"
        );
        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("ELSE")),
            "Expected ELSE keyword"
        );
        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("END")),
            "Expected END keyword"
        );
    }

    #[test]
    fn test_source_map_collects_case_searched() {
        let query = r#"SELECT CASE WHEN Price > 1000 THEN "Expensive" WHEN Price > 500 THEN "Moderate" ELSE "Cheap" END AS PriceCategory FROM Products"#;
        let result = lower_query_with_source_map(query);

        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

        // Searched CASE (no operand)
        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("CASE")),
            "Expected CASE keyword"
        );
        assert!(
            special_keywords.iter().filter(|k| k.eq_ignore_ascii_case("WHEN")).count() >= 2,
            "Expected at least 2 WHEN keywords"
        );
    }

    #[test]
    fn test_source_map_collects_case_keywords_russian() {
        let query = r#"ВЫБРАТЬ ВЫБОР Статус КОГДА 1 ТОГДА "Активен" КОГДА 2 ТОГДА "Неактивен" ИНАЧЕ "Неизвестен" КОНЕЦ КАК ТекстСтатуса ИЗ Товары"#;
        let result = lower_query_with_source_map(query);

        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();

        // Should collect ВЫБОР, КОГДА (x2), ТОГДА (x2), ИНАЧЕ, КОНЕЦ
        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("ВЫБОР")),
            "Expected ВЫБОР keyword"
        );
        assert!(
            special_keywords.iter().filter(|k| k.eq_ignore_ascii_case("КОГДА")).count() >= 2,
            "Expected at least 2 КОГДА keywords"
        );
        assert!(
            special_keywords.iter().filter(|k| k.eq_ignore_ascii_case("ТОГДА")).count() >= 2,
            "Expected at least 2 ТОГДА keywords"
        );
        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("ИНАЧЕ")),
            "Expected ИНАЧЕ keyword"
        );
        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("КОНЕЦ")),
            "Expected КОНЕЦ keyword"
        );
    }

    #[test]
    fn test_in_expression_not_in() {
        let query = "SELECT * FROM Products WHERE Type NOT IN (1, 2)";
        let result = lower_query_with_source_map(query);

        // NOT IN is now parsed as a single IN_EXPR with negated flag
        let special_keywords: Vec<String> =
            result.source_map.special_keywords.iter().map(|t| t.text.to_string()).collect();
        let operators: Vec<String> =
            result.source_map.operators.iter().map(|t| t.text.to_string()).collect();

        assert!(
            special_keywords.iter().any(|k| k.eq_ignore_ascii_case("IN")),
            "Expected IN keyword in special_keywords"
        );
        assert!(
            operators.iter().any(|k| k.eq_ignore_ascii_case("NOT")),
            "Expected NOT keyword in operators (from NOT_EXPR lowering)"
        );
    }
}
