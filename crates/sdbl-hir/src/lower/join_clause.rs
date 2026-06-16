use std::collections::HashSet;

use crate::diagnostics::SdblDiagnostic;
use crate::hir::JoinHir;
use crate::hir::TableRef;
use crate::standard_fields::virtual_table_type;
use syntax::ast::AstNode;

use super::context::LoweringContext;

impl LoweringContext<'_> {
    pub(super) fn lower_joins(&mut self, query: &syntax::ast::SdblQuery) -> Vec<JoinHir> {
        let Some(from_clause) = query.from_clause() else {
            return Vec::new();
        };

        let Some(first_ds) = from_clause.data_sources().next() else {
            return Vec::new();
        };

        let mut all_joins = Vec::new();
        for join in first_ds.join_clauses() {
            self.lower_join_clause_recursive(&join, &mut all_joins);
        }
        all_joins
    }

    fn lower_join_clause_recursive(
        &mut self,
        join: &syntax::ast::SdblJoinClause,
        out: &mut Vec<JoinHir>,
    ) {
        let join_hir = self.lower_join_clause(join);

        if let Some(ds) = join.data_source() {
            for nested_join in ds.join_clauses() {
                self.lower_join_clause_recursive(&nested_join, out);
            }
        }

        out.push(join_hir);
    }

    fn lower_join_clause(&mut self, join: &syntax::ast::SdblJoinClause) -> JoinHir {
        self.record_keyword_by_text(
            join.syntax(),
            "JOIN",
            "СОЕДИНЕНИЕ",
            crate::source_map::TokenCategory::JoinKeyword,
        );

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

        if matches!(join_type, crate::hir::JoinType::Full) {
            self.diagnostics
                .push(SdblDiagnostic::FullOuterJoin { range: join.syntax().text_range() });
        }

        let table = if let Some(ds) = join.data_source() {
            if let Some(subquery) = ds.subquery() {
                self.diagnostics.push(SdblDiagnostic::JoinWithSubQuery {
                    range: subquery.syntax().text_range(),
                });
            }

            self.lower_data_source(&ds)
        } else {
            TableRef::missing(join.syntax().text_range())
        };

        if table.is_virtual_table {
            if let Some(vt_type) = table.parts.last().and_then(|p| virtual_table_type(p)) {
                self.diagnostics.push(SdblDiagnostic::JoinWithVirtualTable {
                    table_name: table.full_name.clone(),
                    virtual_table_type: vt_type.to_string(),
                    range: table.range,
                });
            }
        }

        self.scope.add_table(table.clone());

        let condition_node = join.syntax().children().find(|n| {
            matches!(
                n.kind(),
                syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                    | syntax::SyntaxKind::SDBL_LOGICAL_AND_EXPR
                    | syntax::SyntaxKind::SDBL_COMPARISON_EXPR
                    | syntax::SyntaxKind::SDBL_IS_NULL_EXPR
                    | syntax::SyntaxKind::SDBL_IN_EXPR
                    | syntax::SyntaxKind::SDBL_BETWEEN_EXPR
                    | syntax::SyntaxKind::SDBL_LIKE_EXPR
            )
        });

        if let Some(ref expr_node) = condition_node {
            self.check_join_or_with_multiple_fields(expr_node);
        }

        let condition = condition_node.map(|expr| self.lower_expr(&expr));

        JoinHir { join_type, table, condition, range: join.syntax().text_range() }
    }

    fn check_join_or_with_multiple_fields(&mut self, expr_node: &syntax::SyntaxNode) {
        use syntax::SyntaxToken;

        let or_tokens: Vec<SyntaxToken> = expr_node
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|token| token.kind() == syntax::SyntaxKind::KW_OR)
            .collect();

        for or_token in or_tokens {
            let containing_expr = self.find_containing_logical_expr_for_join(&or_token);

            if let Some(expr) = containing_expr {
                let field_names = self.extract_field_names_from_expr(&expr);

                if field_names.len() > 1 {
                    self.diagnostics
                        .push(SdblDiagnostic::LogicalOrInJoin { range: or_token.text_range() });
                }
            }
        }
    }

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

    fn extract_field_names_from_expr(&self, expr: &syntax::SyntaxNode) -> HashSet<String> {
        use syntax::{SyntaxKind, SyntaxToken};

        let mut fields = HashSet::new();

        let tokens: Vec<SyntaxToken> =
            expr.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

        let mut i = 0;
        while i < tokens.len() {
            let token = &tokens[i];

            if token.kind().is_name_token()
                && i + 2 < tokens.len()
                && tokens[i + 1].kind() == SyntaxKind::DOT
                && tokens[i + 2].kind().is_name_token()
            {
                let mut parts: Vec<String> = vec![token.text().to_string()];
                parts.push(tokens[i + 2].text().to_string());
                i += 3;

                while i + 1 < tokens.len()
                    && tokens[i].kind() == SyntaxKind::DOT
                    && tokens[i + 1].kind().is_name_token()
                {
                    parts.push(tokens[i + 1].text().to_string());
                    i += 2;
                }

                if !parts.iter().any(|p| self.is_sql_keyword(p)) {
                    fields.insert(parts.join("."));
                }

                continue;
            }

            if token.kind() == SyntaxKind::IDENT {
                let text = token.text();
                if !self.is_sql_keyword(text) {
                    fields.insert(text.to_string());
                }
            }

            i += 1;
        }

        fields
    }

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
}
