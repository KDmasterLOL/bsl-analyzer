//! CASE expression lowering.

use crate::hir::ExprHir;
use crate::types::SdblType;
use syntax::SyntaxKind;

use crate::lower::context::LoweringContext;

impl LoweringContext {
    pub(super) fn lower_case_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
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
}
