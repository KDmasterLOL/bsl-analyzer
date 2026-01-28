//! Expression lowering.

mod case_expr;
mod ops;
mod predicates;

use crate::diagnostics::SdblDiagnostic;
use crate::hir::ExprHir;
use crate::hir::Name;
use crate::types::SdblType;
use text_size::TextRange;

use super::context::LoweringContext;

impl<'a> LoweringContext<'a> {
    pub(in crate::lower) fn lower_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use syntax::SyntaxKind;

        tracing::debug!(
            node_text = %node.text(),
            node_kind = ?node.kind(),
            "DIAGNOSTIC LOWERING: lower_expr called"
        );

        match node.kind() {
            SyntaxKind::SDBL_COLUMN_REF => self.lower_column_ref(node),
            SyntaxKind::SDBL_LITERAL => self.lower_literal(node),
            SyntaxKind::SDBL_MULTI_STRING => self.lower_multi_string(node),
            SyntaxKind::SDBL_FUNCTION_CALL => self.lower_function_call(node),
            SyntaxKind::SDBL_PAREN_EXPR => {
                // Unwrap parentheses
                if let Some(inner) = node.children().next() {
                    self.lower_expr(&inner)
                } else {
                    ExprHir::Missing { range: node.text_range() }
                }
            }
            SyntaxKind::SDBL_TUPLE_EXPR => self.lower_tuple_expr(node),
            SyntaxKind::SDBL_LOGICAL_OR_EXPR
            | SyntaxKind::SDBL_LOGICAL_AND_EXPR
            | SyntaxKind::SDBL_COMPARISON_EXPR
            | SyntaxKind::SDBL_ADDITIVE_EXPR
            | SyntaxKind::SDBL_MULTIPLICATIVE_EXPR => self.lower_binary_expr(node),
            SyntaxKind::SDBL_NOT_EXPR | SyntaxKind::SDBL_UNARY_EXPR => self.lower_unary_expr(node),
            SyntaxKind::SDBL_IS_NULL_EXPR => self.lower_is_null_expr(node),
            SyntaxKind::SDBL_IN_EXPR => self.lower_in_expr(node),
            SyntaxKind::SDBL_IN_HIERARCHY_EXPR => self.lower_in_hierarchy_expr(node),
            SyntaxKind::SDBL_BETWEEN_EXPR => self.lower_between_expr(node),
            SyntaxKind::SDBL_LIKE_EXPR => self.lower_like_expr(node),
            SyntaxKind::SDBL_REFS_EXPR => self.lower_refs_expr(node),
            SyntaxKind::SDBL_CASE_EXPR => self.lower_case_expr(node),
            SyntaxKind::SDBL_PARAMETER => self.lower_parameter(node),
            SyntaxKind::SDBL_MISSING_ARG => ExprHir::Missing { range: node.text_range() },
            _ => ExprHir::Missing { range: node.text_range() },
        }
    }

    /// Lower column reference.
    ///
    /// Stores all parts of the path in HIR. Diagnostics like QueryNestedFieldsByDot
    /// are checked later by analyzing the HIR structure, not during lowering.
    fn lower_column_ref(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        let text = node.text().to_string();
        let str_parts: Vec<&str> = text.split('.').collect();

        // Convert to Name parts
        let parts: Vec<Name> = str_parts.iter().map(|s| Name::from(s.trim())).collect();

        tracing::debug!(
            text = %text,
            parts_count = parts.len(),
            "lower_column_ref called"
        );

        // Extract IDENT ranges from AST for semantic highlighting
        let ident_ranges: Vec<TextRange> = node
            .children_with_tokens()
            .filter_map(|child| match child {
                syntax::NodeOrToken::Token(token) if token.kind() == syntax::SyntaxKind::IDENT => {
                    Some(token.text_range())
                }
                _ => None,
            })
            .collect();

        // Resolve type from scope (using first part as alias, second as column for 2+ parts)
        let (table_alias_str, column_name_str) = if parts.len() >= 2 {
            (Some(parts[0].as_str()), parts[1].as_str())
        } else {
            (None, parts[0].as_str())
        };

        let ty = self.scope.resolve_column_type(table_alias_str, column_name_str);

        tracing::debug!(
            text = %text,
            resolved_type = ?ty,
            "resolved column type from scope"
        );

        // Check for unknown field
        if ty == SdblType::Unknown {
            if let Some(alias) = table_alias_str {
                if let Some(table) = self.scope.find_table(alias) {
                    if table.metadata.is_some() {
                        self.diagnostics.push(SdblDiagnostic::UnknownField {
                            table_name: table.full_name.clone(),
                            field_name: column_name_str.to_string(),
                            range: node.text_range(),
                        });
                    }
                }
            }
        } else if ty == SdblType::Error {
            // Ambiguous column
            let possible_tables = self.scope.find_tables_with_column(column_name_str);
            self.diagnostics.push(SdblDiagnostic::AmbiguousColumnRef {
                column_name: column_name_str.to_string(),
                possible_tables,
                range: node.text_range(),
            });
        }

        // Record ALL identifiers in source_map for semantic highlighting
        // This handles nested field references like Table.Field1.Field2.Field3
        for (idx, range) in ident_ranges.iter().enumerate() {
            if idx == 0 && parts.len() >= 2 {
                // First identifier = table alias
                self.source_map.add_token(
                    crate::source_map::TokenInfo::new(
                        *range,
                        syntax::SyntaxKind::IDENT,
                        str_parts[idx].trim(),
                    ),
                    crate::source_map::TokenCategory::TableAlias,
                );
            } else {
                // All other identifiers = field names (intermediate or final)
                let category = if ty != SdblType::Unknown && ty != SdblType::Error {
                    crate::source_map::TokenCategory::FieldName
                } else {
                    crate::source_map::TokenCategory::UnresolvedFieldName
                };
                self.source_map.add_token(
                    crate::source_map::TokenInfo::new(
                        *range,
                        syntax::SyntaxKind::IDENT,
                        str_parts[idx].trim(),
                    ),
                    category,
                );
            }
        }

        ExprHir::ColumnRef { parts, ty, range: node.text_range() }
    }

    /// Lower literal.
    fn lower_literal(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::LiteralValue;

        let text = node.text().to_string().trim().to_string();

        // Check for multiline string in SDBL_LITERAL (single String token with \n)
        // Parser creates SDBL_LITERAL even for strings with newlines inside
        for child in node.children_with_tokens() {
            if let Some(token) = child.as_token() {
                if token.kind() == syntax::SyntaxKind::STRING && token.text().contains('\n') {
                    self.diagnostics
                        .push(SdblDiagnostic::MultilineString { range: token.text_range() });
                    break;
                }
            }
        }

        // Determine literal type
        let (value, ty) = if text.starts_with('"') || text.starts_with('\'') {
            (
                LiteralValue::String(text.trim_matches(|c| c == '"' || c == '\'').to_string()),
                SdblType::string(),
            )
        } else if text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("истина") {
            (LiteralValue::Boolean(true), SdblType::Boolean)
        } else if text.eq_ignore_ascii_case("false") || text.eq_ignore_ascii_case("ложь") {
            (LiteralValue::Boolean(false), SdblType::Boolean)
        } else if text.eq_ignore_ascii_case("null") {
            (LiteralValue::Null, SdblType::Null)
        } else if text.eq_ignore_ascii_case("undefined")
            || text.eq_ignore_ascii_case("неопределено")
        {
            (LiteralValue::Undefined, SdblType::Unknown)
        } else if let Ok(n) = text.parse::<i64>() {
            (LiteralValue::Integer(n), SdblType::number())
        } else if text.contains('.') {
            (LiteralValue::Float(text), SdblType::number())
        } else {
            (LiteralValue::String(text), SdblType::Unknown)
        };

        ExprHir::Literal { value, ty, range: node.text_range() }
    }

    /// Lower multiline string (SDBL_MULTI_STRING node with multiple String tokens).
    ///
    /// Parser creates SDBL_MULTI_STRING when it sees multiple consecutive String tokens.
    fn lower_multi_string(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::LiteralValue;

        // Count STRING tokens
        let string_count = node
            .children_with_tokens()
            .filter(|child| {
                child.as_token().map(|t| t.kind() == syntax::SyntaxKind::STRING).unwrap_or(false)
            })
            .count();

        // If more than 2 STRING tokens, this is a multiline literal (likely error)
        // Two strings is OK (e.g., "" + "" in some contexts), but 3+ is suspicious
        if string_count > 2 {
            self.diagnostics.push(SdblDiagnostic::MultilineString { range: node.text_range() });
        }

        // Return string literal HIR
        let text = node.text().to_string();
        ExprHir::Literal {
            value: LiteralValue::String(text),
            ty: SdblType::string(),
            range: node.text_range(),
        }
    }

    /// Lower function call.
    fn lower_function_call(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::FunctionKind;

        // Get function name from first identifier
        let func_name_token = node
            .children_with_tokens()
            .filter_map(|c| c.into_token())
            .find(|t| t.kind() == syntax::SyntaxKind::IDENT);

        let func_name = func_name_token.as_ref().map(|t| t.text().to_string()).unwrap_or_default();

        // Parse function kind
        let function = match func_name.to_uppercase().as_str() {
            "СУММА" | "SUM" => {
                // Record aggregate function name
                if let Some(token) = func_name_token.as_ref() {
                    self.record_token(token, crate::source_map::TokenCategory::AggregateFunction);
                }
                FunctionKind::Sum
            }
            "СРЕДНЕЕ" | "AVG" => {
                if let Some(token) = func_name_token.as_ref() {
                    self.record_token(token, crate::source_map::TokenCategory::AggregateFunction);
                }
                FunctionKind::Avg
            }
            "МИНИМУМ" | "MIN" => {
                if let Some(token) = func_name_token.as_ref() {
                    self.record_token(token, crate::source_map::TokenCategory::AggregateFunction);
                }
                FunctionKind::Min
            }
            "МАКСИМУМ" | "MAX" => {
                if let Some(token) = func_name_token.as_ref() {
                    self.record_token(token, crate::source_map::TokenCategory::AggregateFunction);
                }
                FunctionKind::Max
            }
            "КОЛИЧЕСТВО" | "COUNT" => {
                if let Some(token) = func_name_token.as_ref() {
                    self.record_token(token, crate::source_map::TokenCategory::AggregateFunction);
                }
                FunctionKind::Count
            }
            "ПОДСТРОКА"
            | "SUBSTRING"
            | "ВРЕГ"
            | "UPPER"
            | "НРЕГ"
            | "LOWER"
            | "ГОД"
            | "YEAR"
            | "МЕСЯЦ"
            | "MONTH"
            | "ДЕНЬ"
            | "DAY"
            | "ЕСТЬNULL"
            | "ISNULL"
            | "ВЫРАЗИТЬ"
            | "CAST"
            | "ПРЕДСТАВЛЕНИЕ"
            | "PRESENTATION"
            | "ЗНАЧЕНИЕ"
            | "VALUE" => {
                // Record built-in SDBL function token for semantic highlighting
                if let Some(token) = func_name_token.as_ref() {
                    self.record_token(token, crate::source_map::TokenCategory::BuiltinFunction);
                }
                match func_name.to_uppercase().as_str() {
                    "ПОДСТРОКА" | "SUBSTRING" => FunctionKind::Substring,
                    "ВРЕГ" | "UPPER" => FunctionKind::Upper,
                    "НРЕГ" | "LOWER" => FunctionKind::Lower,
                    "ГОД" | "YEAR" => FunctionKind::Year,
                    "МЕСЯЦ" | "MONTH" => FunctionKind::Month,
                    "ДЕНЬ" | "DAY" => FunctionKind::Day,
                    "ЕСТЬNULL" | "ISNULL" => FunctionKind::Isnull,
                    "ВЫРАЗИТЬ" | "CAST" => FunctionKind::Cast,
                    "ПРЕДСТАВЛЕНИЕ" | "PRESENTATION" => FunctionKind::Presentation,
                    "ЗНАЧЕНИЕ" | "VALUE" => FunctionKind::Value,
                    _ => unreachable!(),
                }
            }
            _ => FunctionKind::Unknown(func_name.clone()),
        };

        // Lower arguments - get all expression children
        let args: Vec<ExprHir> = node
            .children()
            .filter(|c| {
                matches!(
                    c.kind(),
                    syntax::SyntaxKind::SDBL_COLUMN_REF
                        | syntax::SyntaxKind::SDBL_LITERAL
                        | syntax::SyntaxKind::SDBL_MULTI_STRING
                        | syntax::SyntaxKind::SDBL_FUNCTION_CALL
                        | syntax::SyntaxKind::SDBL_PAREN_EXPR
                        | syntax::SyntaxKind::SDBL_TUPLE_EXPR
                        | syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                        | syntax::SyntaxKind::SDBL_LOGICAL_AND_EXPR
                        | syntax::SyntaxKind::SDBL_COMPARISON_EXPR
                        | syntax::SyntaxKind::SDBL_ADDITIVE_EXPR
                        | syntax::SyntaxKind::SDBL_MULTIPLICATIVE_EXPR
                        | syntax::SyntaxKind::SDBL_UNARY_EXPR
                        | syntax::SyntaxKind::SDBL_PARAMETER
                )
            })
            .map(|arg| self.lower_expr(&arg))
            .collect();

        // Extract member access chain after closing paren
        // Token sequence: FUNC ( args ) . Field1 . Field2
        // We need IDENT tokens that come after RPAREN
        let member_access: Vec<Name> = {
            let tokens: Vec<_> =
                node.children_with_tokens().filter_map(|c| c.into_token()).collect();

            // Find position of closing paren
            let rparen_pos = tokens.iter().position(|t| t.kind() == syntax::SyntaxKind::R_PAREN);

            if let Some(pos) = rparen_pos {
                // Collect IDENT tokens after RPAREN (these are member access fields)
                tokens[pos + 1..]
                    .iter()
                    .filter(|t| t.kind() == syntax::SyntaxKind::IDENT)
                    .map(|t| Name::from(t.text()))
                    .collect()
            } else {
                Vec::new()
            }
        };

        // Infer return type
        // Note: QueryNestedFieldsByDot for CAST with 2+ member_access is checked
        // in post-lowering phase (check_nested_fields_by_dot)
        let ty = self.infer_function_return_type(&function, &args);

        ExprHir::FunctionCall { function, args, member_access, ty, range: node.text_range() }
    }

    /// Infer function return type.
    fn infer_function_return_type(
        &self,
        function: &crate::hir::FunctionKind,
        args: &[ExprHir],
    ) -> SdblType {
        use crate::hir::FunctionKind;

        match function {
            // Aggregate functions wrap argument type
            FunctionKind::Sum | FunctionKind::Avg => {
                if let Some(arg) = args.first() {
                    SdblType::Aggregate(Box::new(arg.ty().clone()))
                } else {
                    SdblType::Aggregate(Box::new(SdblType::Unknown))
                }
            }
            FunctionKind::Min | FunctionKind::Max => {
                args.first().map(|a| a.ty().clone()).unwrap_or(SdblType::Unknown)
            }
            FunctionKind::Count => SdblType::number(),

            // String functions return String
            FunctionKind::Substring
            | FunctionKind::Upper
            | FunctionKind::Lower
            | FunctionKind::Ltrim
            | FunctionKind::Rtrim
            | FunctionKind::Concat
            | FunctionKind::Presentation => SdblType::string(),

            // Date part functions return Number
            FunctionKind::Year
            | FunctionKind::Month
            | FunctionKind::Day
            | FunctionKind::Hour
            | FunctionKind::Minute
            | FunctionKind::Second => SdblType::number(),

            // Date functions return Date/DateTime
            FunctionKind::DateTime | FunctionKind::BeginOfPeriod | FunctionKind::EndOfPeriod => {
                SdblType::DateTime
            }
            FunctionKind::AddMonth => SdblType::Date,
            FunctionKind::DateDiff => SdblType::number(),

            // ISNULL returns type of first non-null argument
            FunctionKind::Isnull => {
                args.first().map(|a| a.ty().clone()).unwrap_or(SdblType::Unknown)
            }

            // CAST - need to parse target type (complex)
            FunctionKind::Cast => SdblType::Unknown,

            // Type/ValueType return type descriptors
            FunctionKind::Type | FunctionKind::ValueType => SdblType::Unknown,

            // VALUE returns typed reference (simplified to AnyRef for now)
            FunctionKind::Value => SdblType::AnyRef,

            // Ref returns Boolean
            FunctionKind::Ref => SdblType::Boolean,

            // Unknown function
            FunctionKind::Unknown(_) => SdblType::Unknown,
        }
    }

    /// Lower parameter expression.
    fn lower_parameter(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        let text = node.text().to_string();
        let name = text.trim_start_matches('&').to_string();

        ExprHir::Parameter {
            name: Name::from(name.as_str()),
            ty: SdblType::Unknown, // Parameters have unknown type without context
            range: node.text_range(),
        }
    }

    /// Lower tuple expression (expr1, expr2, ...).
    ///
    /// Used for row-wise comparison in IN predicates:
    /// `(field1, field2) IN (SELECT col1, col2 FROM ...)`
    fn lower_tuple_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        // Get all expression children - filter by expression SyntaxKinds
        let elements: Vec<ExprHir> = node
            .children()
            .filter(|c| {
                matches!(
                    c.kind(),
                    syntax::SyntaxKind::SDBL_COLUMN_REF
                        | syntax::SyntaxKind::SDBL_LITERAL
                        | syntax::SyntaxKind::SDBL_MULTI_STRING
                        | syntax::SyntaxKind::SDBL_FUNCTION_CALL
                        | syntax::SyntaxKind::SDBL_PAREN_EXPR
                        | syntax::SyntaxKind::SDBL_TUPLE_EXPR
                        | syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                        | syntax::SyntaxKind::SDBL_LOGICAL_AND_EXPR
                        | syntax::SyntaxKind::SDBL_COMPARISON_EXPR
                        | syntax::SyntaxKind::SDBL_ADDITIVE_EXPR
                        | syntax::SyntaxKind::SDBL_MULTIPLICATIVE_EXPR
                        | syntax::SyntaxKind::SDBL_UNARY_EXPR
                        | syntax::SyntaxKind::SDBL_PARAMETER
                )
            })
            .map(|child| self.lower_expr(&child))
            .collect();

        ExprHir::Tuple { elements, range: node.text_range() }
    }
}
