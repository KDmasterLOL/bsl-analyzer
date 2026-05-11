//! WHERE, GROUP BY, ORDER BY clause lowering.

use crate::diagnostics::SdblDiagnostic;
use crate::hir::{ExprHir, SelectHir};
use syntax::ast::AstNode;

use super::context::LoweringContext;

impl LoweringContext {
    /// Lower WHERE clause.
    pub(super) fn lower_where_clause(
        &mut self,
        where_clause: &syntax::ast::SdblWhereClause,
    ) -> ExprHir {
        // Record WHERE keyword
        self.record_keyword_by_text(
            where_clause.syntax(),
            "WHERE",
            "ГДЕ",
            crate::source_map::TokenCategory::ClauseKeyword,
        );

        // Collect LogicalOrInWhere diagnostics
        // Skip nested subqueries - they will collect their own OR tokens
        self.collect_or_tokens_excluding_subqueries(where_clause.syntax());

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
    pub(super) fn lower_group_by(
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
    pub(super) fn lower_order_by(
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

    /// Record INDEX BY clause semantic tokens.
    ///
    /// INDEX BY references selected output columns. A simple one-part reference
    /// that matches a SELECT alias/name is recorded as a field alias instead of
    /// being resolved against FROM tables.
    pub(super) fn lower_index_by_clause(
        &mut self,
        query_node: &syntax::SyntaxNode,
        select: &SelectHir,
    ) {
        let Some(index_clause) =
            query_node.children().find(|n| n.kind() == syntax::SyntaxKind::SDBL_INDEX_BY)
        else {
            return;
        };

        self.record_keyword_by_text(
            &index_clause,
            "INDEX",
            "ИНДЕКСИРОВАТЬ",
            crate::source_map::TokenCategory::ClauseKeyword,
        );
        self.record_keyword_by_text(
            &index_clause,
            "BY",
            "ПО",
            crate::source_map::TokenCategory::ClauseKeyword,
        );

        for child in index_clause.children() {
            if let Some(column_ref) = simple_selected_output_ref(&child, select) {
                if let Some(token) =
                    column_ref.children_with_tokens().find_map(|it| it.into_token())
                {
                    self.record_token(&token, crate::source_map::TokenCategory::FieldAlias);
                }
            } else if matches!(
                child.kind(),
                syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                    | syntax::SyntaxKind::SDBL_COLUMN_REF
                    | syntax::SyntaxKind::SDBL_FUNCTION_CALL
            ) {
                self.lower_expr(&child);
            }
        }
    }

    /// Record TOTALS BY clause semantic tokens.
    ///
    /// TOTALS BY group items can reference selected output columns and can carry
    /// flat `ONLY`/`HIERARCHY` modifiers. The HIR does not model totals yet, so
    /// this pass keeps semantic highlighting attached to the parser contract.
    pub(super) fn lower_totals_by_clause(
        &mut self,
        query_node: &syntax::SyntaxNode,
        select: &SelectHir,
    ) {
        let Some(totals_clause) =
            query_node.children().find(|n| n.kind() == syntax::SyntaxKind::SDBL_TOTALS_BY)
        else {
            return;
        };

        let mut after_by = false;

        for child in totals_clause.children_with_tokens() {
            match child {
                syntax::NodeOrToken::Token(token) if token.kind() == syntax::SyntaxKind::IDENT => {
                    let text = token.text();
                    if text.eq_ignore_ascii_case("TOTALS") || text.eq_ignore_ascii_case("ИТОГИ")
                    {
                        self.record_token(&token, crate::source_map::TokenCategory::ClauseKeyword);
                    } else if text.eq_ignore_ascii_case("BY") || text.eq_ignore_ascii_case("ПО") {
                        self.record_token(&token, crate::source_map::TokenCategory::ClauseKeyword);
                        after_by = true;
                    } else if matches!(
                        text.to_uppercase().as_str(),
                        "ONLY" | "ТОЛЬКО" | "HIERARCHY" | "ИЕРАРХИЯ"
                    ) {
                        self.record_token(&token, crate::source_map::TokenCategory::Modifier);
                    }
                }
                syntax::NodeOrToken::Node(node) if is_sdbl_clause_expr(&node) => {
                    if after_by {
                        if let Some(column_ref) = simple_selected_output_ref(&node, select) {
                            if let Some(token) =
                                column_ref.children_with_tokens().find_map(|it| it.into_token())
                            {
                                self.record_token(
                                    &token,
                                    crate::source_map::TokenCategory::FieldAlias,
                                );
                            }
                            continue;
                        }
                    }

                    self.lower_expr(&node);
                }
                _ => {}
            }
        }
    }

    pub(super) fn lower_drop_query(&mut self, drop_query: &syntax::SyntaxNode) {
        self.record_keyword_by_text(
            drop_query,
            "DROP",
            "УНИЧТОЖИТЬ",
            crate::source_map::TokenCategory::ClauseKeyword,
        );

        let table_token = drop_query
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == syntax::SyntaxKind::IDENT)
            .find(|token| {
                let text = token.text();
                !text.eq_ignore_ascii_case("DROP") && !text.eq_ignore_ascii_case("УНИЧТОЖИТЬ")
            });

        if let Some(table_token) = table_token {
            self.record_token(&table_token, crate::source_map::TokenCategory::TableName);
            self.scope.remove_temp_table(table_token.text());
        }
    }

    /// Collect OR tokens from a node, excluding nested subqueries.
    ///
    /// Nested subqueries have their own WHERE clauses which will collect their OR tokens
    /// when the subquery is lowered separately.
    fn collect_or_tokens_excluding_subqueries(&mut self, node: &syntax::SyntaxNode) {
        for child in node.children_with_tokens() {
            match child {
                syntax::NodeOrToken::Token(token) => {
                    if token.kind() == syntax::SyntaxKind::KW_OR {
                        self.diagnostics
                            .push(SdblDiagnostic::LogicalOrInWhere { range: token.text_range() });
                    }
                }
                syntax::NodeOrToken::Node(child_node) => {
                    // Skip nested subqueries - they will collect their own OR tokens
                    if !matches!(
                        child_node.kind(),
                        syntax::SyntaxKind::SDBL_SUBQUERY
                            | syntax::SyntaxKind::SDBL_SUBQUERY_EXPR
                            | syntax::SyntaxKind::SDBL_SELECT_QUERY
                    ) {
                        self.collect_or_tokens_excluding_subqueries(&child_node);
                    }
                }
            }
        }
    }
}

fn is_sdbl_clause_expr(node: &syntax::SyntaxNode) -> bool {
    matches!(
        node.kind(),
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
}

fn simple_selected_output_ref(
    node: &syntax::SyntaxNode,
    select: &SelectHir,
) -> Option<syntax::SyntaxNode> {
    let mut column_refs =
        node.descendants().filter(|child| child.kind() == syntax::SyntaxKind::SDBL_COLUMN_REF);
    let column_ref = column_refs.next()?;
    if column_refs.next().is_some() {
        return None;
    }
    if node.text().to_string().trim() != column_ref.text().to_string().trim() {
        return None;
    }

    let mut idents = column_ref.children_with_tokens().filter_map(|it| match it {
        syntax::NodeOrToken::Token(token) if token.kind() == syntax::SyntaxKind::IDENT => {
            Some(token.text().to_string())
        }
        _ => None,
    });

    let name = idents.next()?;
    if idents.next().is_some() {
        return None;
    }

    let matches_output = select
        .fields
        .iter()
        .filter_map(|field| field.alias_or_name())
        .any(|output_name| output_name.to_lowercase() == name.to_lowercase());

    matches_output.then_some(column_ref)
}
