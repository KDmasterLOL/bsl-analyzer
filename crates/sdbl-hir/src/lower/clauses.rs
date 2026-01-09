//! WHERE, GROUP BY, ORDER BY clause lowering.

use crate::diagnostics::SdblDiagnostic;
use crate::hir::ExprHir;
use syntax::ast::AstNode;

use super::context::LoweringContext;

impl<'a> LoweringContext<'a> {
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
}
