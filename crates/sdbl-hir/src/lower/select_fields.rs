//! SELECT field list lowering and limitations (DISTINCT, TOP).

use crate::diagnostics::SdblDiagnostic;
use crate::hir::{ExprHir, FieldHir, Name, SelectHir};
use crate::types::SdblType;
use syntax::ast::AstNode;

use super::context::LoweringContext;

impl<'a> LoweringContext<'a> {
    pub(super) fn lower_field_list(
        &mut self,
        field_list: Option<syntax::ast::SdblFieldList>,
        distinct: bool,
        top: Option<u32>,
    ) -> SelectHir {
        let Some(fl) = field_list else {
            return SelectHir::empty();
        };

        let fields: Vec<FieldHir> = fl.fields().map(|f| self.lower_selected_field(&f)).collect();

        SelectHir { fields, distinct, top }
    }

    /// Extract DISTINCT and TOP from query limitations.
    ///
    /// Returns (distinct, top).
    pub(super) fn extract_limitations(
        &mut self,
        query_node: &syntax::SyntaxNode,
    ) -> (bool, Option<u32>) {
        let mut distinct = false;
        let mut top = None;

        // Find SDBL_LIMITATIONS child
        for child in query_node.children() {
            if child.kind() == syntax::SyntaxKind::SDBL_LIMITATIONS {
                // Record DISTINCT keyword if present
                for element in child.descendants_with_tokens() {
                    if let Some(token) = element.as_token() {
                        if token.kind() == syntax::SyntaxKind::IDENT {
                            let text_upper = token.text().to_uppercase();
                            if text_upper == "DISTINCT" || text_upper == "РАЗЛИЧНЫЕ" {
                                distinct = true;
                                self.record_token(
                                    token,
                                    crate::source_map::TokenCategory::Modifier,
                                );
                            }
                        }
                    }
                }

                // Find TOP clause
                for top_child in child.children() {
                    if top_child.kind() == syntax::SyntaxKind::SDBL_TOP_CLAUSE {
                        // Record TOP keyword
                        for element in top_child.descendants_with_tokens() {
                            if let Some(token) = element.as_token() {
                                if token.kind() == syntax::SyntaxKind::IDENT {
                                    let text_upper = token.text().to_uppercase();
                                    if text_upper == "TOP" || text_upper == "ПЕРВЫЕ" {
                                        self.record_token(
                                            token,
                                            crate::source_map::TokenCategory::Modifier,
                                        );
                                    }
                                }
                            }
                        }

                        // Extract TOP count (find first DECIMAL token)
                        for element in top_child.descendants_with_tokens() {
                            if let Some(token) = element.as_token() {
                                if token.kind() == syntax::SyntaxKind::DECIMAL {
                                    if let Ok(count) = token.text().parse::<u32>() {
                                        top = Some(count);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                break;
            }
        }

        (distinct, top)
    }

    /// Lower a selected field.
    pub(super) fn lower_selected_field(
        &mut self,
        field: &syntax::ast::SdblSelectedField,
    ) -> FieldHir {
        // Check for asterisk
        if field.is_asterisk() {
            return FieldHir {
                expr: ExprHir::Missing { range: field.syntax().text_range() },
                alias: None,
                ty: SdblType::Unknown,
                is_asterisk: true,
                range: field.syntax().text_range(),
            };
        }

        // Lower expression (expression() returns Option<SyntaxNode>)
        let expr = if let Some(e) = field.expression() {
            self.lower_expr(&e)
        } else {
            ExprHir::Missing { range: field.syntax().text_range() }
        };

        // Get alias (name() returns Option<String>)
        let alias = field.alias().and_then(|a| a.name()).map(|s| Name::from(s.as_str()));

        // Get type from expression
        let ty = expr.ty().clone();

        // Check for AliasWithoutAsKeyword diagnostic
        // Important: UNION queries should NOT be checked (only main/first query in UNION)
        let is_in_union_query = field
            .syntax()
            .ancestors()
            .any(|node| node.kind() == syntax::SyntaxKind::SDBL_UNION_CLAUSE);

        if !is_in_union_query {
            // Skip fields with parse errors
            let has_error = field
                .syntax()
                .descendants_with_tokens()
                .any(|el| el.kind() == syntax::SyntaxKind::ERROR);

            if !has_error {
                // Get trimmed range (expression only, without trailing trivia)
                let range = if let Some(expr) = field.expression() {
                    // Trim trailing whitespace/comments/newlines from expression
                    let last_token = expr.last_token();
                    let last_non_trivia = last_token.and_then(|t| {
                        let mut token = t;
                        while matches!(
                            token.kind(),
                            syntax::SyntaxKind::WHITESPACE
                                | syntax::SyntaxKind::COMMENT
                                | syntax::SyntaxKind::NEWLINE
                        ) {
                            token = token.prev_token()?;
                        }
                        Some(token)
                    });

                    if let (Some(first), Some(last)) = (expr.first_token(), last_non_trivia) {
                        syntax::TextRange::new(first.text_range().start(), last.text_range().end())
                    } else {
                        expr.text_range()
                    }
                } else {
                    field.syntax().text_range()
                };

                if let Some(alias_node) = field.alias() {
                    // Has alias but check for AS keyword
                    if !alias_node.has_as_keyword() {
                        // Include alias in range for implicit alias case
                        let range_with_alias = if let Some(alias_ident) = alias_node.identifier() {
                            syntax::TextRange::new(range.start(), alias_ident.text_range().end())
                        } else {
                            range
                        };

                        self.diagnostics.push(SdblDiagnostic::AliasWithoutAsKeyword {
                            field_name: alias_node.name(),
                            range: range_with_alias,
                        });
                    }
                } else {
                    // No alias at all - use expression range
                    self.diagnostics
                        .push(SdblDiagnostic::AliasWithoutAsKeyword { field_name: None, range });
                }
            }
        }

        FieldHir { expr, alias, ty, is_asterisk: false, range: field.syntax().text_range() }
    }
}
