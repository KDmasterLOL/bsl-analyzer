//! Binary and unary operator lowering.

use crate::hir::ExprHir;
use crate::lower::context::LoweringContext;
use crate::types::SdblType;

impl<'a> LoweringContext<'a> {
    pub(super) fn lower_binary_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
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
