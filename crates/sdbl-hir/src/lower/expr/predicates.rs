//! Predicate expression lowering (IN, BETWEEN, LIKE, IS NULL).

use crate::hir::ExprHir;
use crate::hir::{SdblHir, SelectHir};
use crate::types::SdblType;

use crate::lower::context::LoweringContext;

/// Check if expression is a column reference (not allowed as LIKE pattern).
///
/// Returns true if the expression is:
/// - Direct column reference (ColumnRef)
/// - Parenthesized column reference
fn is_column_ref_pattern(expr: &ExprHir) -> bool {
    match expr {
        ExprHir::ColumnRef { .. } => true,
        ExprHir::UnaryOp { expr: inner, .. } => is_column_ref_pattern(inner),
        _ => false,
    }
}

impl LoweringContext {
    /// Lower a subquery in IN expression context.
    ///
    /// This properly lowers subqueries in WHERE ... IN (SELECT ...) to collect
    /// diagnostics like SelectTopWithoutOrderBy.
    fn lower_in_subquery(&mut self, node: &syntax::SyntaxNode) -> SdblHir {
        use syntax::ast::AstNode;

        // Try to find SDBL_SUBQUERY inside the expression
        let subquery_node = if node.kind() == syntax::SyntaxKind::SDBL_SUBQUERY {
            Some(node.clone())
        } else {
            node.descendants().find(|n| n.kind() == syntax::SyntaxKind::SDBL_SUBQUERY)
        };

        if let Some(sq_node) = subquery_node {
            if let Some(subquery) = syntax::ast::SdblSubquery::cast(sq_node) {
                let queries: Vec<_> = subquery.queries().collect();
                let has_union_siblings = queries.len() > 1;

                // Lower first query as main, collect diagnostics
                if let Some(main_query) = queries.first() {
                    self.scope.push_frame();
                    let mut main_hir = self.lower_query(main_query, false, has_union_siblings);
                    self.scope.pop_frame();

                    // Lower remaining queries as UNION queries
                    for union_query in queries.iter().skip(1) {
                        self.scope.push_frame();
                        let union_hir = self.lower_query(union_query, true, true);
                        self.scope.pop_frame();
                        main_hir.diagnostics.extend(union_hir.diagnostics);
                    }

                    return main_hir;
                }
            }
        }

        // Fallback: return empty HIR if we can't parse the subquery
        SdblHir {
            select: SelectHir { fields: Vec::new(), distinct: false, top: None },
            into_table: None,
            from: Vec::new(),
            joins: Vec::new(),
            where_clause: None,
            group_by: None,
            having: None,
            order_by: None,
            unions: Vec::new(),
            diagnostics: Vec::new(),
            range: node.text_range(),
        }
    }

    pub(super) fn lower_is_null_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
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
    pub(super) fn lower_in_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
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
                // Lower subquery properly to collect diagnostics
                let subquery_hir = self.lower_in_subquery(&child);
                InValues::Subquery(Box::new(subquery_hir))
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

    /// Lower IN HIERARCHY expression.
    ///
    /// Grammar: `expr IN HIERARCHY(root_expr)`
    /// Example: `ГруппыКонтактовПользователей.Родитель В ИЕРАРХИИ(&Папка)`
    ///
    /// Returns Boolean indicating if the expression is in the hierarchy of the root.
    pub(super) fn lower_in_hierarchy_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        // Get the expression being tested (first child)
        let mut children = node.children();
        let expr = children
            .next()
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        // Record IN keyword
        for element in node.descendants_with_tokens() {
            if let Some(token) = element.as_token() {
                if token.kind() == syntax::SyntaxKind::KW_IN {
                    self.record_token(token, crate::source_map::TokenCategory::SpecialKeyword);
                    break;
                }
            }
        }

        // Record HIERARCHY keyword (it's mapped to IDENT token)
        self.record_keyword_by_text(
            node,
            "HIERARCHY",
            "ИЕРАРХИИ",
            crate::source_map::TokenCategory::SpecialKeyword,
        );

        // Get root expression (second child)
        let root_expr = children
            .next()
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        // For now, treat IN HIERARCHY as a binary comparison (placeholder)
        // In the future, this should properly handle hierarchy checks
        ExprHir::BinaryOp {
            lhs: Box::new(expr),
            op: crate::hir::BinaryOp::Eq, // Placeholder
            rhs: Box::new(root_expr),
            ty: SdblType::Boolean,
            range: node.text_range(),
        }
    }

    /// Lower BETWEEN expression.
    ///
    /// Grammar: `expr BETWEEN low AND high`
    /// or: `expr NOT BETWEEN low AND high`
    pub(super) fn lower_between_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
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
    pub(super) fn lower_like_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        // Get the expression being tested (first child)
        let mut children_iter = node.children();
        let first_child = children_iter.next();
        let expr = first_child
            .as_ref()
            .map(|n| self.lower_expr(n))
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
        let pattern_child = children_iter.next();
        let pattern = pattern_child
            .as_ref()
            .map(|n| self.lower_expr(n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        // Optional escape character (if present, it's the next child)
        let escape_child = children_iter.next();
        let escape = escape_child.as_ref().map(|n| Box::new(self.lower_expr(n)));

        // Record ESCAPE keyword if present
        if escape.is_some() {
            self.record_keyword_by_text(
                node,
                "ESCAPE",
                "СПЕЦСИМВОЛ",
                crate::source_map::TokenCategory::SpecialKeyword,
            );
        }

        // Compute tight range by finding the last non-whitespace token
        // This excludes trailing spaces, comments, and newlines
        let tight_range = {
            let start = node.text_range().start();
            // Find the last non-trivia token in the node
            let end = node
                .descendants_with_tokens()
                .filter_map(|el| el.into_token())
                .filter(|t| {
                    !matches!(
                        t.kind(),
                        syntax::SyntaxKind::WHITESPACE
                            | syntax::SyntaxKind::NEWLINE
                            | syntax::SyntaxKind::COMMENT
                    )
                })
                .last()
                .map(|t| t.text_range().end())
                .unwrap_or_else(|| node.text_range().end());
            syntax::TextRange::new(start, end)
        };

        self.diagnostics
            .push(crate::diagnostics::SdblDiagnostic::UsingLikeInQuery { range: tight_range });

        // Check if pattern is a column reference - this is incorrect usage
        // Pattern must be: string literal, parameter, or function call
        if is_column_ref_pattern(&pattern) {
            self.diagnostics.push(crate::diagnostics::SdblDiagnostic::IncorrectUseLikeInQuery {
                range: tight_range,
            });
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

    /// Lower REFS expression (type check for reference).
    ///
    /// Grammar: `expr REFS mdo`
    /// Example: `Исполнители.Исполнитель ССЫЛКА Справочник.ПолныеРоли`
    ///
    /// Returns Boolean indicating if the expression is a reference of the specified type.
    pub(super) fn lower_refs_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        // Get the expression being tested (first child)
        let mut children = node.children();
        let expr = children
            .next()
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        // Record REFS keyword (it's mapped to IDENT token)
        self.record_keyword_by_text(
            node,
            "REFS",
            "ССЫЛКА",
            crate::source_map::TokenCategory::SpecialKeyword,
        );

        // For now, treat REFS as a simple boolean comparison
        // In the future, this should validate against metadata types
        // Result type is always Boolean
        ExprHir::BinaryOp {
            lhs: Box::new(expr),
            op: crate::hir::BinaryOp::Eq, // Placeholder - should be a specialized REFS op
            rhs: Box::new(ExprHir::Missing { range: node.text_range() }), // MDO not lowered yet
            ty: SdblType::Boolean,
            range: node.text_range(),
        }
    }
}
