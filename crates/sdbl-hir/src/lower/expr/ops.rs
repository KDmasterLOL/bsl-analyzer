//! Binary and unary operator lowering.

use crate::hir::ExprHir;
use crate::lower::context::LoweringContext;
use crate::types::SdblType;

impl<'a> LoweringContext<'a> {
    pub(super) fn lower_binary_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::BinaryOp;
        use syntax::SyntaxKind;

        tracing::debug!(
            node_text = %node.text(),
            node_kind = ?node.kind(),
            "DIAGNOSTIC LOWERING: lower_binary_expr called"
        );

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

        // Collect ALL children (for chained operators like A И B И C)
        let children: Vec<_> = node.children().collect();

        // IMPORTANT: If there's only one child (no operator), just return it unwrapped.
        // This happens when parser creates operator precedence nodes without actual operators.
        // For example: "Таблица.Поле" may be wrapped in LOGICAL_OR_EXPR → LOGICAL_AND_EXPR →
        // ADDITIVE_EXPR → MULTIPLICATIVE_EXPR → COLUMN_REF, even though there are no operators.
        if children.len() == 1 {
            tracing::debug!(
                node_text = %node.text(),
                "DIAGNOSTIC LOWERING: Binary expr with no operator - unwrapping child"
            );
            return self.lower_expr(&children[0]);
        }

        // Handle missing children (error case)
        if children.is_empty() {
            return ExprHir::Missing { range: node.text_range() };
        }

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

        // Build left-associative binary tree for chained operators
        // Example: A И B И C → BinaryOp { lhs: BinaryOp { lhs: A, op: And, rhs: B }, op: And, rhs: C }
        let mut result = self.lower_expr(&children[0]);

        for child in &children[1..] {
            let rhs = self.lower_expr(child);
            result = ExprHir::BinaryOp {
                lhs: Box::new(result),
                op,
                rhs: Box::new(rhs),
                ty: ty.clone(),
                range: node.text_range(),
            };
        }

        result
    }

    /// Lower unary expression.
    pub(super) fn lower_unary_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
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
}
