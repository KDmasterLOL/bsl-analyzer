//! Predicate expression lowering (IN, BETWEEN, LIKE, IS NULL).

use crate::hir::ExprHir;
use crate::hir::{SdblHir, SelectHir};
use crate::types::SdblType;

use crate::lower::context::LoweringContext;

impl<'a> LoweringContext<'a> {
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
}
