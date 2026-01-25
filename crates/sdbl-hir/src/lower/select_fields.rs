//! SELECT field list lowering and limitations (DISTINCT, TOP).

use crate::hir::{ExprHir, FieldHir, Name, SelectHir};
use crate::types::SdblType;
use syntax::ast::AstNode;

use super::context::LoweringContext;

impl<'a> LoweringContext<'a> {
    /// Lower SELECT field list.
    ///
    /// # Arguments
    /// * `field_list` - The field list AST node
    /// * `distinct` - Whether DISTINCT is specified
    /// * `top` - TOP N limit if specified
    /// * `is_union` - Whether this is a UNION query (skips alias diagnostics)
    pub(super) fn lower_field_list(
        &mut self,
        field_list: Option<syntax::ast::SdblFieldList>,
        distinct: bool,
        top: Option<u32>,
        is_union: bool,
    ) -> SelectHir {
        let Some(fl) = field_list else {
            return SelectHir::empty();
        };

        let fields: Vec<FieldHir> =
            fl.fields().map(|f| self.lower_selected_field(&f, is_union)).collect();

        SelectHir { fields, distinct, top }
    }

    /// Extract DISTINCT and TOP from query limitations.
    ///
    /// Returns (distinct, top_value, top_range).
    pub(super) fn extract_limitations(
        &mut self,
        query_node: &syntax::SyntaxNode,
    ) -> (bool, Option<u32>, Option<syntax::TextRange>) {
        let mut distinct = false;
        let mut top = None;
        let mut top_range = None;

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
                        // Record TOP keyword and capture its range
                        for element in top_child.descendants_with_tokens() {
                            if let Some(token) = element.as_token() {
                                if token.kind() == syntax::SyntaxKind::IDENT {
                                    let text_upper = token.text().to_uppercase();
                                    if text_upper == "TOP" || text_upper == "ПЕРВЫЕ" {
                                        self.record_token(
                                            token,
                                            crate::source_map::TokenCategory::Modifier,
                                        );
                                        top_range = Some(top_child.text_range());
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

        (distinct, top, top_range)
    }

    /// Lower a selected field.
    ///
    /// Collects all information needed for diagnostics into HIR fields.
    /// Diagnostics are emitted in a separate post-lowering phase.
    ///
    /// # Arguments
    /// * `field` - The field AST node
    /// * `_is_union` - Whether this is a UNION query (unused, kept for API stability)
    pub(super) fn lower_selected_field(
        &mut self,
        field: &syntax::ast::SdblSelectedField,
        _is_union: bool,
    ) -> FieldHir {
        let field_range = field.syntax().text_range();

        // Check for asterisk
        if field.is_asterisk() {
            return FieldHir {
                expr: ExprHir::Missing { range: field_range },
                alias: None,
                has_as_keyword: false,
                has_parse_error: false,
                raw_name: None,
                ty: SdblType::Unknown,
                is_asterisk: true,
                diagnostic_range: field_range,
                range: field_range,
            };
        }

        // Lower expression (expression() returns Option<SyntaxNode>)
        let expr = if let Some(e) = field.expression() {
            tracing::trace!(
                expr_text = %e.text(),
                expr_kind = ?e.kind(),
                "DIAGNOSTIC LOWERING: lowering SELECT field expression"
            );
            let lowered = self.lower_expr(&e);
            tracing::trace!(
                expr_text = %e.text(),
                lowered_variant = ?std::mem::discriminant(&lowered),
                lowered_type = ?lowered.ty(),
                "DIAGNOSTIC LOWERING: lowered SELECT field expression"
            );
            lowered
        } else {
            ExprHir::Missing { range: field_range }
        };

        // Get alias node for processing
        let alias_node = field.alias();

        // Compute has_as_keyword
        let has_as_keyword = alias_node.as_ref().map(|a| a.has_as_keyword()).unwrap_or(false);

        // Get alias name and record tokens
        let alias = alias_node
            .as_ref()
            .and_then(|a| {
                // Record AS/КАК keyword in source map for semantic highlighting
                if a.has_as_keyword() {
                    self.record_keyword_by_text(
                        a.syntax(),
                        "AS",
                        "КАК",
                        crate::source_map::TokenCategory::SpecialKeyword,
                    );
                }

                // Record field alias identifier
                if let Some(ident_token) = a.identifier() {
                    self.source_map.add_token(
                        crate::source_map::TokenInfo::new(
                            ident_token.text_range(),
                            ident_token.kind(),
                            ident_token.text(),
                        ),
                        crate::source_map::TokenCategory::FieldAlias,
                    );
                }

                a.name()
            })
            .map(|s| Name::from(s.as_str()));

        // Get type from expression
        let ty = expr.ty().clone();

        // Extract raw field name from AST (last identifier in expression)
        // For "Т.ИмяПоля" -> Some("ИмяПоля")
        // For "COUNT(*)" -> None (not a simple field)
        let raw_name = field.expression().and_then(|expr_node| {
            // Find last IDENT token in expression (skip DOT)
            let mut last_ident = None;
            for element in expr_node.descendants_with_tokens() {
                if let Some(token) = element.as_token() {
                    if token.kind() == syntax::SyntaxKind::IDENT {
                        last_ident = Some(token.text().to_string());
                    }
                }
            }
            last_ident.map(|s| Name::from(s.as_str()))
        });

        // Compute has_parse_error (once, for later diagnostic check)
        let has_parse_error = field
            .syntax()
            .descendants_with_tokens()
            .any(|el| el.kind() == syntax::SyntaxKind::ERROR);

        // Compute diagnostic_range (trimmed expression + optional alias)
        let diagnostic_range = self.compute_diagnostic_range(field, alias_node.as_ref());

        FieldHir {
            expr,
            alias,
            has_as_keyword,
            has_parse_error,
            raw_name,
            ty,
            is_asterisk: false,
            diagnostic_range,
            range: field_range,
        }
    }

    /// Compute trimmed range for diagnostic highlighting.
    ///
    /// Returns:
    /// - For alias without AS: expression start to alias identifier end
    /// - For no alias: expression range trimmed of trailing trivia
    fn compute_diagnostic_range(
        &self,
        field: &syntax::ast::SdblSelectedField,
        alias_node: Option<&syntax::ast::SdblAlias>,
    ) -> syntax::TextRange {
        // Get expression range without trailing trivia
        let expr_range = if let Some(expr) = field.expression() {
            let last_non_trivia = expr.last_token().and_then(|t| {
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

        // Extend to alias identifier if present (for "Field Alias" without AS)
        if let Some(alias) = alias_node {
            if let Some(alias_ident) = alias.identifier() {
                return syntax::TextRange::new(expr_range.start(), alias_ident.text_range().end());
            }
        }

        expr_range
    }
}
