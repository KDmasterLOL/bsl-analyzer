use crate::hir::ExprHir;
use crate::types::SdblType;
use syntax::SyntaxKind;

use crate::lower::context::LoweringContext;

impl LoweringContext<'_> {
    pub(super) fn lower_case_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        self.record_keyword_by_text(
            node,
            "CASE",
            "ВЫБОР",
            crate::source_map::TokenCategory::SpecialKeyword,
        );

        let children = node.children();
        let mut expressions = Vec::new();
        let mut when_clauses_nodes = Vec::new();

        for child in children {
            if child.kind() == SyntaxKind::SDBL_WHEN_CLAUSE {
                when_clauses_nodes.push(child);
            } else {
                expressions.push(self.lower_expr(&child));
            }
        }

        let mut expr_iter = expressions.into_iter();

        let has_operand = !when_clauses_nodes.is_empty()
            && node
                .children()
                .next()
                .map(|n| n.kind() != SyntaxKind::SDBL_WHEN_CLAUSE)
                .unwrap_or(false);

        let operand = if has_operand { expr_iter.next().map(Box::new) } else { None };

        let mut when_clauses = Vec::new();
        for when_node in when_clauses_nodes {
            self.record_keyword_by_text(
                &when_node,
                "WHEN",
                "КОГДА",
                crate::source_map::TokenCategory::SpecialKeyword,
            );

            self.record_keyword_by_text(
                &when_node,
                "THEN",
                "ТОГДА",
                crate::source_map::TokenCategory::SpecialKeyword,
            );

            let mut when_children: Vec<_> =
                when_node.children().map(|n| self.lower_expr(&n)).collect();

            if when_children.len() >= 2 {
                let condition = when_children.remove(0);
                let result = when_children.remove(0);
                when_clauses.push(crate::hir::WhenClause { condition, result });
            }
        }

        let else_expr = if let Some(else_val) = expr_iter.next() {
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

        self.record_keyword_by_text(
            node,
            "END",
            "КОНЕЦ",
            crate::source_map::TokenCategory::SpecialKeyword,
        );

        let ty = SdblType::Unknown;

        ExprHir::Case { operand, when_clauses, else_expr, ty, range: node.text_range() }
    }
}
