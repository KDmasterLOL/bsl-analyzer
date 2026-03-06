//! JOIN clause lowering.

use std::collections::HashSet;

use crate::diagnostics::SdblDiagnostic;
use crate::hir::JoinHir;
use crate::hir::TableRef;
use crate::standard_fields::virtual_table_type;
use syntax::ast::AstNode;

use super::context::LoweringContext;

impl LoweringContext {
    /// Lower all JOIN clauses (including nested) from query.
    ///
    /// Returns a flat list of all JOINs, recursively collecting nested JOINs.
    /// This ensures all tables from nested JOINs are available in completion scope.
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

    /// Recursively lower JOIN clause and collect all nested JOINs.
    fn lower_join_clause_recursive(
        &mut self,
        join: &syntax::ast::SdblJoinClause,
        out: &mut Vec<JoinHir>,
    ) {
        let join_hir = self.lower_join_clause(join);

        // First collect nested JOINs (depth-first)
        if let Some(ds) = join.data_source() {
            for nested_join in ds.join_clauses() {
                self.lower_join_clause_recursive(&nested_join, out);
            }
        }

        // Then add this JOIN
        out.push(join_hir);
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
            // This matches bsl-language-server: visitJoinPart() checks dataSource().subquery() != null
            if ds.subquery().is_some() {
                self.diagnostics
                    .push(SdblDiagnostic::JoinWithSubQuery { range: ds.syntax().text_range() });
            }

            // NOTE: Nested JOINs are now handled by lower_join_clause_recursive()
            // which calls this function recursively. No need to process them here.
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
                    range: table.range,
                });
            }
        }

        // Lower ON condition - get the expression child directly from JOIN clause
        // AST structure: SDBL_JOIN_CLAUSE contains SDBL_DATA_SOURCE + expression (ON condition)
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

        // Check for OR with multiple fields in JOIN condition
        if let Some(ref expr_node) = condition_node {
            self.check_join_or_with_multiple_fields(expr_node);
        }

        let condition = condition_node.map(|expr| self.lower_expr(&expr));

        JoinHir { join_type, table, condition, range: join.syntax().text_range() }
    }

    /// Check JOIN ON condition for OR with multiple distinct fields.
    ///
    /// Matches bsl-language-server: isMultipleFieldsExpression() logic.
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
}
