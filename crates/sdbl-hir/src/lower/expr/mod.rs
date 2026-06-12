mod case_expr;
mod ops;
mod predicates;

use crate::diagnostics::SdblDiagnostic;
use crate::hir::ExprHir;
use crate::hir::Name;
use crate::types::SdblType;
use stdx::case::CaseExt;
use text_size::TextRange;

use super::context::LoweringContext;

impl LoweringContext<'_> {
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

    fn lower_column_ref(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        let text = node.text().to_string();
        let str_parts: Vec<&str> = text.split('.').collect();

        let parts: Vec<Name> = str_parts.iter().map(|s| Name::from(s.trim())).collect();

        tracing::debug!(
            text = %text,
            parts_count = parts.len(),
            "lower_column_ref called"
        );

        let ident_ranges: Vec<TextRange> = node
            .children_with_tokens()
            .filter_map(|child| match child {
                syntax::NodeOrToken::Token(token) if token.kind().is_name_token() => {
                    Some(token.text_range())
                }
                _ => None,
            })
            .collect();

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

        if ty == SdblType::Unknown {
            if let Some(alias) = table_alias_str {
                if self.scope.find_field_def(Some(alias), column_name_str).is_none() {
                    if let Some(table) = self.scope.find_table(alias) {
                        // First-hop only: a nested `Т.Поле.ПодПоле` validates just
                        // `Поле`. Emission is gated on `field_model_complete` so
                        // we never flag a missing field on a table whose field
                        // set may be incomplete (extensions, virtual tables, …).
                        if table.metadata.as_ref().is_some_and(|m| m.field_model_complete()) {
                            self.diagnostics.push(SdblDiagnostic::UnknownField {
                                table_name: table.full_name.clone(),
                                field_name: column_name_str.to_string(),
                                range: ident_ranges
                                    .get(1)
                                    .copied()
                                    .unwrap_or_else(|| node.text_range()),
                            });
                        }
                    }
                }
            }
        } else if ty == SdblType::Error {
            let possible_tables = self.scope.find_tables_with_column(column_name_str);
            self.diagnostics.push(SdblDiagnostic::AmbiguousColumnRef {
                column_name: column_name_str.to_string(),
                possible_tables,
                range: node.text_range(),
            });
        }

        for (idx, range) in ident_ranges.iter().enumerate() {
            if idx == 0 && parts.len() >= 2 {
                self.source_map.add_token(
                    crate::source_map::TokenInfo::new(
                        *range,
                        syntax::SyntaxKind::IDENT,
                        str_parts[idx].trim(),
                    ),
                    crate::source_map::TokenCategory::TableAlias,
                );
            } else {
                let field_exists =
                    self.scope.find_field_def(table_alias_str, column_name_str).is_some();
                let can_validate_field = self.resolver.is_some()
                    && match table_alias_str {
                        Some(alias) => self
                            .scope
                            .find_table(alias)
                            .map(|table| table.metadata.is_some())
                            .unwrap_or(false),
                        None => self.scope.all_tables().any(|table| table.metadata.is_some()),
                    };
                let category = if (ty != SdblType::Unknown && ty != SdblType::Error)
                    || field_exists
                    || !can_validate_field
                {
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

    fn lower_literal(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::LiteralValue;

        let text = node.text().to_string().trim().to_string();

        for child in node.children_with_tokens() {
            if let Some(token) = child.as_token() {
                if token.kind() == syntax::SyntaxKind::STRING && token.text().contains('\n') {
                    self.diagnostics
                        .push(SdblDiagnostic::MultilineString { range: token.text_range() });
                    break;
                }
            }
        }

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

    fn lower_multi_string(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::LiteralValue;

        let string_count = node
            .children_with_tokens()
            .filter(|child| {
                child.as_token().map(|t| t.kind() == syntax::SyntaxKind::STRING).unwrap_or(false)
            })
            .count();

        if string_count > 2 {
            self.diagnostics.push(SdblDiagnostic::MultilineString { range: node.text_range() });
        }

        let text = node.text().to_string();
        ExprHir::Literal {
            value: LiteralValue::String(text),
            ty: SdblType::string(),
            range: node.text_range(),
        }
    }

    fn lower_function_call(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::FunctionKind;

        let func_name_token = node
            .children_with_tokens()
            .filter_map(|c| c.into_token())
            .find(|t| t.kind() == syntax::SyntaxKind::IDENT);

        let func_name = func_name_token.as_ref().map(|t| t.text().to_string()).unwrap_or_default();

        let function = match func_name.to_uppercase().as_str() {
            "СУММА" | "SUM" => {
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

        let args: Vec<ExprHir> = if matches!(function, FunctionKind::Value) {
            self.lower_value_function_args(node)
        } else {
            node.children()
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
                .collect()
        };

        let member_access: Vec<Name> = {
            let tokens: Vec<_> =
                node.children_with_tokens().filter_map(|c| c.into_token()).collect();

            let rparen_pos = tokens.iter().position(|t| t.kind() == syntax::SyntaxKind::R_PAREN);

            if let Some(pos) = rparen_pos {
                tokens[pos + 1..]
                    .iter()
                    .filter(|t| t.kind().is_name_token())
                    .map(|t| Name::from(t.text()))
                    .collect()
            } else {
                Vec::new()
            }
        };

        let ty = if matches!(function, FunctionKind::Cast) {
            self.resolve_cast_target(node)
        } else {
            self.infer_function_return_type(&function, &args)
        };

        ExprHir::FunctionCall { function, args, member_access, ty, range: node.text_range() }
    }

    fn infer_function_return_type(
        &self,
        function: &crate::hir::FunctionKind,
        args: &[ExprHir],
    ) -> SdblType {
        use crate::hir::FunctionKind;

        match function {
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

            FunctionKind::Substring
            | FunctionKind::Upper
            | FunctionKind::Lower
            | FunctionKind::Ltrim
            | FunctionKind::Rtrim
            | FunctionKind::Concat
            | FunctionKind::Presentation => SdblType::string(),

            FunctionKind::Year
            | FunctionKind::Month
            | FunctionKind::Day
            | FunctionKind::Hour
            | FunctionKind::Minute
            | FunctionKind::Second => SdblType::number(),

            FunctionKind::DateTime | FunctionKind::BeginOfPeriod | FunctionKind::EndOfPeriod => {
                SdblType::DateTime
            }
            FunctionKind::AddMonth => SdblType::Date,
            FunctionKind::DateDiff => SdblType::number(),

            FunctionKind::Isnull => {
                args.first().map(|a| a.ty().clone()).unwrap_or(SdblType::Unknown)
            }

            FunctionKind::Cast => SdblType::Unknown,

            FunctionKind::Type | FunctionKind::ValueType => SdblType::Unknown,

            FunctionKind::Value => SdblType::AnyRef,

            FunctionKind::Ref => SdblType::Boolean,

            FunctionKind::Unknown(_) => SdblType::Unknown,
        }
    }

    fn resolve_cast_target(&self, node: &syntax::SyntaxNode) -> SdblType {
        let Some(type_node) = node.children().find(|c| c.kind() == syntax::SyntaxKind::SDBL_TYPE)
        else {
            return SdblType::Unknown;
        };

        let mut name_parts: Vec<String> = Vec::new();
        let mut decimals: Vec<u32> = Vec::new();
        let mut in_parens = false;

        for child in type_node.children_with_tokens() {
            let Some(token) = child.into_token() else { continue };
            match token.kind() {
                syntax::SyntaxKind::IDENT if !in_parens => {
                    name_parts.push(token.text().to_string());
                }
                syntax::SyntaxKind::L_PAREN => in_parens = true,
                syntax::SyntaxKind::R_PAREN => in_parens = false,
                syntax::SyntaxKind::DECIMAL if in_parens => {
                    if let Ok(n) = token.text().parse::<u32>() {
                        decimals.push(n);
                    }
                }
                _ => {}
            }
        }

        if name_parts.is_empty() {
            return SdblType::Unknown;
        }

        if name_parts.len() == 1 {
            return classify_primitive_cast_target(&name_parts[0], &decimals);
        }

        if name_parts.len() == 2 {
            if let Ok(mdo_type) = name_parts[0].parse::<bsl_metadata::MdoType>() {
                return SdblType::reference(mdo_type, &name_parts[1]);
            }
        }

        SdblType::Unknown
    }

    fn lower_parameter(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        let text = node.text().to_string();
        let name = text.trim_start_matches('&').to_string();

        ExprHir::Parameter {
            name: Name::from(name.as_str()),
            ty: SdblType::Unknown,
            range: node.text_range(),
        }
    }

    fn lower_value_function_args(&mut self, node: &syntax::SyntaxNode) -> Vec<ExprHir> {
        let Some(col_ref) =
            node.descendants().find(|n| n.kind() == syntax::SyntaxKind::SDBL_COLUMN_REF)
        else {
            return node
                .children()
                .filter(|c| {
                    matches!(
                        c.kind(),
                        syntax::SyntaxKind::SDBL_COLUMN_REF
                            | syntax::SyntaxKind::SDBL_LITERAL
                            | syntax::SyntaxKind::SDBL_MISSING_ARG
                    )
                })
                .map(|arg| self.lower_expr(&arg))
                .collect();
        };

        let text = col_ref.text().to_string();
        let str_parts: Vec<&str> = text.split('.').collect();
        let parts: Vec<Name> = str_parts.iter().map(|s| Name::from(s.trim())).collect();

        let ident_ranges: Vec<(TextRange, String)> = col_ref
            .children_with_tokens()
            .filter_map(|child| match child {
                syntax::NodeOrToken::Token(token) if token.kind().is_name_token() => {
                    Some((token.text_range(), token.text().to_string()))
                }
                _ => None,
            })
            .collect();

        let mdo_type_parsed =
            str_parts.first().and_then(|s| s.trim().parse::<bsl_metadata::MdoType>().ok());

        if let Some(mdo_type) = mdo_type_parsed {
            if let Some((range, text)) = ident_ranges.first() {
                self.source_map.add_token(
                    crate::source_map::TokenInfo::new(*range, syntax::SyntaxKind::IDENT, text),
                    crate::source_map::TokenCategory::MdoType,
                );
            }

            if let Some((range, text)) = ident_ranges.get(1) {
                self.source_map.add_token(
                    crate::source_map::TokenInfo::new(*range, syntax::SyntaxKind::IDENT, text),
                    crate::source_map::TokenCategory::TableName,
                );
            }

            if let Some((range, text)) = ident_ranges.get(2) {
                let object_name = str_parts.get(1).map(|s| s.trim()).unwrap_or("");
                let value_name = text.trim();

                let is_empty_ref = {
                    let lower = value_name.fold_lower();
                    lower == "пустаяссылка" || lower == "emptyref"
                };

                let is_valid = if is_empty_ref {
                    true
                } else if mdo_type == bsl_metadata::MdoType::Enum {
                    self.resolver
                        .and_then(|r| r.resolve_metadata_object(mdo_type, object_name))
                        .map(|obj| obj.find_enum_value(value_name).is_some())
                        .unwrap_or(true)
                } else if matches!(
                    mdo_type,
                    bsl_metadata::MdoType::Catalog
                        | bsl_metadata::MdoType::Document
                        | bsl_metadata::MdoType::ChartOfCharacteristicTypes
                        | bsl_metadata::MdoType::ChartOfCalculationTypes
                        | bsl_metadata::MdoType::ChartOfAccounts
                ) {
                    self.resolver
                        .and_then(|r| r.resolve_metadata_object(mdo_type, object_name))
                        .map(|obj| {
                            if obj.predefined_items.is_empty() {
                                true
                            } else {
                                obj.find_predefined_item(value_name).is_some()
                            }
                        })
                        .unwrap_or(true)
                } else {
                    true
                };

                let category = if is_valid {
                    crate::source_map::TokenCategory::FieldName
                } else {
                    crate::source_map::TokenCategory::UnresolvedFieldName
                };

                self.source_map.add_token(
                    crate::source_map::TokenInfo::new(*range, syntax::SyntaxKind::IDENT, text),
                    category,
                );
            }
        } else {
            for (range, text) in &ident_ranges {
                self.source_map.add_token(
                    crate::source_map::TokenInfo::new(*range, syntax::SyntaxKind::IDENT, text),
                    crate::source_map::TokenCategory::UnresolvedFieldName,
                );
            }
        }

        vec![ExprHir::ColumnRef { parts, ty: SdblType::AnyRef, range: col_ref.text_range() }]
    }

    fn lower_tuple_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
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

fn classify_primitive_cast_target(name: &str, decimals: &[u32]) -> SdblType {
    match name.to_uppercase().as_str() {
        "ЧИСЛО" | "NUMBER" => SdblType::Number {
            precision: decimals.first().and_then(|n| u8::try_from(*n).ok()),
            scale: decimals.get(1).and_then(|n| u8::try_from(*n).ok()),
        },
        "СТРОКА" | "STRING" => SdblType::String { length: decimals.first().copied() },
        "ДАТА" | "DATE" => SdblType::Date,
        "БУЛЕВО" | "BOOLEAN" => SdblType::Boolean,
        _ => SdblType::Unknown,
    }
}
