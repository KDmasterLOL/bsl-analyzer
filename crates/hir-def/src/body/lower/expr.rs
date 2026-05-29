use parser_error::{ParseError, RecoveryKind};
use syntax::ast_utils::field_tail_name_token;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::body::{
    Body, BodyDiagnostic, ExternalRef, MagicNumberContext, ManagerType, RedundantAccessKind,
};
use crate::hir::{BinaryOp, Expr, ExprIdx, Literal, UnaryOp};
use crate::{Name, QualifiedName};

use super::diagnostics::{is_deprecated_method, is_followed_by_loop_exit};
use super::utils::{extract_string_content, looks_like_sdbl};
use super::LoweringCtx;

fn field_name_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    field_tail_name_token(node)
}

fn trailing_dot_range(refs: &SyntaxNode) -> Option<syntax::TextRange> {
    let children: Vec<_> = refs.children_with_tokens().collect();
    for child in children.into_iter().rev() {
        let kind = child.kind();
        if kind.is_trivia() {
            continue;
        }

        return if kind == SyntaxKind::DOT {
            child.as_token().map(|token| token.text_range())
        } else {
            None
        };
    }

    None
}

pub(crate) fn lower_expr_node(ctx: &mut LoweringCtx, node: &SyntaxNode) -> ExprIdx {
    let actual_node = if node.kind() == SyntaxKind::EXPR {
        node.children().next().unwrap_or_else(|| node.clone())
    } else {
        node.clone()
    };

    lower_expr(ctx, &actual_node)
}

fn lower_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> ExprIdx {
    let range = node.text_range();

    let expr = match node.kind() {
        SyntaxKind::LITERAL => lower_literal(ctx, node),
        SyntaxKind::BINARY_EXPR => lower_binary_expr(ctx, node),
        SyntaxKind::UNARY_EXPR => lower_unary_expr(ctx, node),
        SyntaxKind::TERNARY_EXPR => lower_ternary_expr(ctx, node),
        SyntaxKind::CALL_EXPR => lower_call_expr(ctx, node),
        SyntaxKind::INDEX_EXPR => lower_index_expr(ctx, node),
        SyntaxKind::FIELD_EXPR => lower_field_expr(ctx, node),
        SyntaxKind::NEW_EXPR => lower_new_expr(ctx, node),
        SyntaxKind::PAREN_EXPR => {
            return node
                .children()
                .next()
                .map(|n| lower_expr_node(ctx, &n))
                .unwrap_or_else(|| ctx.missing_expr());
        }
        SyntaxKind::AWAIT_EXPR => {
            return node
                .children()
                .next()
                .map(|n| lower_expr_node(ctx, &n))
                .unwrap_or_else(|| ctx.missing_expr());
        }
        SyntaxKind::IDENT => {
            let text = node.text().to_string();

            use super::diagnostics::{
                is_call_expr_callee, is_field_access_field, is_this_form_identifier,
            };
            if is_this_form_identifier(&text)
                && !ctx.param_names.contains(&text.to_lowercase())
                && !is_call_expr_callee(node)
                && !is_field_access_field(node)
            {
                ctx.diagnostics.push(BodyDiagnostic::UsingThisForm { range });
            }

            Expr::Path(Name::new(&text))
        }
        SyntaxKind::EXPR => {
            return node
                .children()
                .next()
                .map(|n| lower_expr(ctx, &n))
                .unwrap_or_else(|| ctx.missing_expr());
        }
        _ => {
            if let Some(ident) = node
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| tok.kind() == SyntaxKind::IDENT)
            {
                Expr::Path(Name::new(ident.text()))
            } else {
                Expr::Missing
            }
        }
    };

    let expr_id = ctx.alloc_expr(expr, range);

    if let Some(idx) =
        ctx.pending_sdbl.iter().position(|(literal_range, _)| *literal_range == range)
    {
        let (_literal_range, query_info) = ctx.pending_sdbl.remove(idx);
        ctx.body.sdbl_exprs.push((expr_id, query_info));
    }

    expr_id
}

fn lower_literal(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let token = node.children_with_tokens().filter_map(|el| el.into_token()).find(|tok| {
        matches!(
            tok.kind(),
            SyntaxKind::DECIMAL
                | SyntaxKind::FLOAT
                | SyntaxKind::STRING
                | SyntaxKind::STRING_START
                | SyntaxKind::DATE
                | SyntaxKind::KW_TRUE
                | SyntaxKind::KW_FALSE
                | SyntaxKind::KW_UNDEFINED
                | SyntaxKind::KW_NULL
        )
    });

    let Some(token) = token else {
        return Expr::Missing;
    };

    let literal = match token.kind() {
        SyntaxKind::DECIMAL | SyntaxKind::FLOAT => {
            let text = token.text().replace(' ', "");
            let value = text.parse::<f64>().unwrap_or(0.0);

            let value = ordered_float::NotNan::new(value)
                .unwrap_or_else(|_| ordered_float::NotNan::new(0.0).unwrap());

            let context = determine_magic_number_context(&token);
            ctx.emit(BodyDiagnostic::MagicNumber {
                value: text.clone(),
                range: token.text_range(),
                context,
            });

            Literal::Number(value)
        }
        SyntaxKind::STRING | SyntaxKind::STRING_START => {
            let value = extract_string_content(node).unwrap_or_default();

            if looks_like_sdbl(&value) {
                let (sdbl_text, quote_corrections) = syntax::extract_sdbl_with_corrections(node)
                    .unwrap_or_else(|| (value.clone(), vec![]));

                let sdbl_ast = parser::parse_sdbl_with_shared_cache(&sdbl_text);
                let literal_text = node.text().to_string();
                let mut error_ranges_in_bsl = Vec::new();

                for syntax_err in sdbl_ast.errors() {
                    let bsl_range = syntax::sdbl_query::map_range_query_to_literal(
                        &literal_text,
                        syntax_err.range(),
                    );
                    error_ranges_in_bsl.push((bsl_range, syntax_err.structured().clone()));
                }

                let sdbl_root = sdbl_ast.syntax_node();
                for refs in
                    sdbl_root.descendants().filter(|node| node.kind() == SyntaxKind::SDBL_REFS_EXPR)
                {
                    if let Some(dot_range) = trailing_dot_range(&refs) {
                        let bsl_range = syntax::sdbl_query::map_range_query_to_literal(
                            &literal_text,
                            dot_range,
                        );
                        error_ranges_in_bsl.push((
                            bsl_range,
                            ParseError::Custom {
                                message: "незавершённый путь в ссылке",
                                recovery: RecoveryKind::Custom,
                            },
                        ));
                    }
                }

                let query_info = syntax::SdblQueryInfo::new(
                    node.text_range(),
                    sdbl_text,
                    Some(sdbl_ast),
                    quote_corrections,
                    error_ranges_in_bsl,
                );

                ctx.pending_sdbl.push((node.text_range(), query_info));
            }

            Literal::String(value)
        }
        SyntaxKind::DATE => {
            let text = token.text();
            let value = text.trim_start_matches('\'').trim_end_matches('\'').to_string();
            Literal::Date(value)
        }
        SyntaxKind::KW_TRUE => Literal::Bool(true),
        SyntaxKind::KW_FALSE => Literal::Bool(false),
        SyntaxKind::KW_UNDEFINED => Literal::Undefined,
        SyntaxKind::KW_NULL => Literal::Null,
        _ => return Expr::Missing,
    };

    Expr::Literal(literal)
}

fn lower_binary_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    let lhs_node = match children.next() {
        Some(n) => n,
        None => return Expr::Missing,
    };
    let lhs = lower_expr_node(ctx, &lhs_node);

    let op_token = node.children_with_tokens().filter_map(|el| el.into_token()).find(|tok| {
        matches!(
            tok.kind(),
            SyntaxKind::PLUS
                | SyntaxKind::MINUS
                | SyntaxKind::STAR
                | SyntaxKind::SLASH
                | SyntaxKind::PERCENT
                | SyntaxKind::EQ
                | SyntaxKind::NEQ
                | SyntaxKind::LT
                | SyntaxKind::LE
                | SyntaxKind::GT
                | SyntaxKind::GE
                | SyntaxKind::KW_AND
                | SyntaxKind::KW_OR
        )
    });

    let op = op_token
        .map(|tok| match tok.kind() {
            SyntaxKind::PLUS => BinaryOp::Add,
            SyntaxKind::MINUS => BinaryOp::Sub,
            SyntaxKind::STAR => BinaryOp::Mul,
            SyntaxKind::SLASH => BinaryOp::Div,
            SyntaxKind::PERCENT => BinaryOp::Mod,
            SyntaxKind::EQ => BinaryOp::Eq,
            SyntaxKind::NEQ => BinaryOp::Neq,
            SyntaxKind::LT => BinaryOp::Lt,
            SyntaxKind::LE => BinaryOp::Le,
            SyntaxKind::GT => BinaryOp::Gt,
            SyntaxKind::GE => BinaryOp::Ge,
            SyntaxKind::KW_AND => BinaryOp::And,
            SyntaxKind::KW_OR => BinaryOp::Or,
            _ => BinaryOp::Add,
        })
        .unwrap_or(BinaryOp::Add);

    let rhs_node = match children.next() {
        Some(n) => n,
        None => return Expr::Missing,
    };
    let rhs = lower_expr_node(ctx, &rhs_node);

    if matches!(op, BinaryOp::Add) {
        let mut current = rhs_node.clone();
        let mut unary_plus_token = None;

        while current.kind() == SyntaxKind::EXPR {
            if let Some(plus_tok) = current
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| matches!(tok.kind(), SyntaxKind::PLUS))
            {
                unary_plus_token = Some(plus_tok);
                break;
            }

            if let Some(child) = current.children().next() {
                current = child;
            } else {
                break;
            }
        }

        if let Some(unary_plus_tok) = unary_plus_token {
            let has_numeric_literal = current
                .descendants()
                .find(|n| n.kind() == SyntaxKind::LITERAL)
                .map(|lit| {
                    lit.children_with_tokens()
                        .filter_map(|el| el.into_token())
                        .any(|tok| matches!(tok.kind(), SyntaxKind::DECIMAL | SyntaxKind::FLOAT))
                })
                .unwrap_or(false);

            if !has_numeric_literal {
                ctx.emit(BodyDiagnostic::UnaryPlusInConcatenation {
                    range: unary_plus_tok.text_range(),
                });
            }
        }
    }

    Expr::BinaryOp { lhs, rhs, op }
}

fn lower_unary_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let op_token = node.children_with_tokens().filter_map(|el| el.into_token()).find(|tok| {
        matches!(tok.kind(), SyntaxKind::MINUS | SyntaxKind::PLUS | SyntaxKind::KW_NOT)
    });

    let op = op_token
        .map(|tok| match tok.kind() {
            SyntaxKind::MINUS => UnaryOp::Neg,
            SyntaxKind::PLUS => UnaryOp::Plus,
            SyntaxKind::KW_NOT => UnaryOp::Not,
            _ => UnaryOp::Neg,
        })
        .unwrap_or(UnaryOp::Neg);

    let expr_node = match node.children().next() {
        Some(n) => n,
        None => {
            let missing = ctx.missing_expr();
            return Expr::UnaryOp { expr: missing, op };
        }
    };
    let expr = lower_expr_node(ctx, &expr_node);

    Expr::UnaryOp { expr, op }
}

fn lower_ternary_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    ctx.emit(BodyDiagnostic::TernaryOperatorUsage { range: node.text_range() });

    let mut children = node.children();

    let condition =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let then_expr =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let else_expr =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    Expr::Ternary { condition, then_expr, else_expr }
}

fn lower_call_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    let callee_node = match children.next() {
        Some(n) => n,
        None => return Expr::Missing,
    };

    let actual_callee = if callee_node.kind() == SyntaxKind::EXPR {
        callee_node.children().next().unwrap_or_else(|| callee_node.clone())
    } else {
        callee_node.clone()
    };

    let mut is_safe_mode_query_call = false;
    let mut is_str_template_call = false;

    if actual_callee.kind() == SyntaxKind::IDENT {
        let name = actual_callee.text().to_string();

        use super::diagnostics::is_safe_mode_query;
        if is_safe_mode_query(&name) {
            is_safe_mode_query_call = true;
        }

        use super::diagnostics::is_deprecated_global_method_8312;

        if is_deprecated_global_method_8312(&name) {
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedAttribute8312 {
                name: name.clone(),
                kind: crate::body::DeprecatedKind8312::GlobalMethod,
                range: actual_callee.text_range(),
            });
        }

        use super::diagnostics::is_deprecated_current_date;

        if is_deprecated_current_date(&name) {
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedCurrentDate {
                name: name.clone(),
                range: actual_callee.text_range(),
            });
        }

        use super::diagnostics::is_deprecated_find;

        if is_deprecated_find(&name) {
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedFind {
                name: name.clone(),
                range: actual_callee.text_range(),
            });
        }

        use super::diagnostics::is_deprecated_message;

        if is_deprecated_message(&name) {
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedMessage {
                name: name.clone(),
                range: actual_callee.text_range(),
            });
        }

        use super::diagnostics::is_temp_files_dir;

        if is_temp_files_dir(&name) {
            ctx.diagnostics.push(BodyDiagnostic::TempFilesDir {
                name: name.clone(),
                range: actual_callee.text_range(),
            });
        }

        use super::diagnostics::{is_deprecated_managed_form, is_type_method};

        if is_type_method(&name) {
            if let Some(arg_list) = node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST) {
                if let Some(first_arg) = arg_list.children().next() {
                    if let Some(string_token) = first_arg
                        .descendants_with_tokens()
                        .filter_map(|el| el.into_token())
                        .find(|tok| tok.kind() == SyntaxKind::STRING)
                    {
                        let text = string_token.text();
                        if text.len() >= 2 {
                            let inner = &text[1..text.len() - 1];
                            let content = inner.replace("\"\"", "\"");

                            if is_deprecated_managed_form(&content) {
                                ctx.diagnostics.push(BodyDiagnostic::DeprecatedTypeManagedForm {
                                    type_name: content,
                                    range: string_token.text_range(),
                                });
                            }
                        }
                    }
                }
            }
        }

        let name_lower = name.to_lowercase();
        if (name_lower == "eval" || name_lower == "вычислить") && !ctx.is_client_only {
            ctx.diagnostics.push(BodyDiagnostic::ExecuteExternalCode { range: node.text_range() });
        }

        if is_external_app_method(&name) {
            ctx.diagnostics
                .push(BodyDiagnostic::ExternalAppStarting { range: actual_callee.text_range() });
        }

        if is_os_users_method(&name) {
            ctx.diagnostics
                .push(BodyDiagnostic::OSUsersMethod { range: actual_callee.text_range() });
        }

        use super::diagnostics::check_try_number_call;
        if let Some(range) = check_try_number_call(node) {
            ctx.diagnostics.push(BodyDiagnostic::TryNumber { range });
        }

        if is_write_log_event_method(&name) {
            check_write_log_event_call(ctx, node);
        }

        if is_file_system_method(&name) {
            ctx.diagnostics
                .push(BodyDiagnostic::FileSystemAccess { range: actual_callee.text_range() });
        }

        if is_form_data_to_value_method(&name) && !ctx.has_no_context_annotation {
            ctx.diagnostics
                .push(BodyDiagnostic::FormDataToValue { range: actual_callee.text_range() });
        }

        if is_get_form_method(&name) {
            ctx.diagnostics.push(BodyDiagnostic::GetFormMethod {
                method_name: name.clone(),
                range: actual_callee.text_range(),
            });
        }

        if is_proceed_with_call_method(&name_lower) && !ctx.is_instead_method {
            ctx.diagnostics.push(BodyDiagnostic::WrongUseFunctionProceedWithCall {
                range: actual_callee.text_range(),
            });
        }

        if is_str_template_method(&name) {
            is_str_template_call = true;
        }

        if is_deprecated_method(&name) {
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedMethod {
                name: name.clone(),
                range: node.text_range(),
            });
        }

        let name_lower = name.to_lowercase();
        if !ctx.local_vars.contains_key(&name_lower) && !ctx.param_names.contains(&name_lower) {
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedMethodCall {
                callee: name,
                module: None,
                range: actual_callee.text_range(),
            });
        }

        use super::diagnostics::get_modal_method_replacement;
        if let Some(replacement) = get_modal_method_replacement(&name_lower) {
            ctx.diagnostics.push(BodyDiagnostic::UsingModalWindows {
                method_name: actual_callee.text().to_string(),
                replacement: replacement.to_string(),
                range: node.text_range(),
            });
        }

        use super::diagnostics::get_synchronous_call_replacement;
        if !ctx.is_server_method {
            if let Some(replacement) = get_synchronous_call_replacement(&name_lower) {
                ctx.diagnostics.push(BodyDiagnostic::UsingSynchronousCalls {
                    method_name: actual_callee.text().to_string(),
                    replacement: replacement.to_string(),
                    range: node.text_range(),
                });
            }
        }
    } else if actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        let idents: Vec<_> = actual_callee
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .collect();

        tracing::debug!(
            idents_count = idents.len(),
            callee_text = %actual_callee.text(),
            "lower_call_expr: FIELD_EXPR found"
        );

        if idents.len() == 2 {
            let module_name = idents[0].text();
            let method_name_str = idents[1].text();
            let key = module_name.to_lowercase();

            if !ctx.local_vars.contains_key(&key) && !ctx.param_names.contains(&key) {
                ctx.external_refs.push(crate::body::ExternalRef::QualifiedCall {
                    receiver: crate::Name::new(module_name),
                    method: crate::Name::new(method_name_str),
                    range: actual_callee.text_range(),
                });

                ctx.diagnostics.push(BodyDiagnostic::DeprecatedMethodCall {
                    callee: method_name_str.to_string(),
                    module: Some(module_name.to_string()),
                    range: idents[1].text_range(),
                });
            }
        }

        if let Some(method_token) = idents.last() {
            let method_name = method_token.text();
            if is_external_app_method(method_name) {
                ctx.diagnostics
                    .push(BodyDiagnostic::ExternalAppStarting { range: method_token.text_range() });
            }

            if is_file_system_method(method_name) {
                ctx.diagnostics
                    .push(BodyDiagnostic::FileSystemAccess { range: method_token.text_range() });
            }

            if is_form_data_to_value_method(method_name) && !ctx.has_no_context_annotation {
                ctx.diagnostics
                    .push(BodyDiagnostic::FormDataToValue { range: method_token.text_range() });
            }

            if is_get_form_method(method_name) {
                ctx.diagnostics.push(BodyDiagnostic::GetFormMethod {
                    method_name: method_name.to_string(),
                    range: method_token.text_range(),
                });
            }
        }

        check_using_external_code_tools(ctx, &actual_callee, &idents, node);
    }

    let callee = lower_expr_node(ctx, &callee_node);

    let arg_list_node = node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST);

    let args =
        arg_list_node.as_ref().map(|arg_list| lower_arg_list(ctx, arg_list)).unwrap_or_default();

    if is_safe_mode_query_call && is_unsafe_safe_mode_context(node) {
        ctx.emit(BodyDiagnostic::UnsafeSafeModeMethodCall { range: actual_callee.text_range() });
    }

    if is_str_template_call {
        if let Some(ref arg_list) = arg_list_node {
            if let Some(first_arg) = arg_list.children().next() {
                if let Some(template_string) = find_string_in_node(&first_arg) {
                    let param_count = arg_list
                        .children_with_tokens()
                        .filter(|el| el.as_token().is_some_and(|t| t.kind() == SyntaxKind::COMMA))
                        .count();

                    if is_wrong_str_template(&template_string, param_count) {
                        ctx.diagnostics.push(BodyDiagnostic::IncorrectUseOfStrTemplate {
                            range: node.text_range(),
                        });
                    }
                }
            }
        }
    }

    if ctx.in_loop() && actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        if let Some(method_token) = actual_callee
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| matches!(tok.kind(), SyntaxKind::IDENT | SyntaxKind::KW_EXECUTE))
            .last()
        {
            let method_name = method_token.text().to_lowercase();
            if matches!(method_name.as_str(), "execute" | "выполнить") {
                let receiver = match ctx.body.expr_idx(callee) {
                    Expr::Field { base, .. } => Some(*base),
                    Expr::MethodCall { receiver, .. } => Some(*receiver),
                    _ => None,
                };

                if let Some(receiver_id) = receiver {
                    if let Some(var_name) = extract_receiver_name(ctx, receiver_id) {
                        if ctx.is_query_var(&var_name) {
                            ctx.emit(BodyDiagnostic::CreateQueryInCycle {
                                range: node.text_range(),
                            });
                        }
                    }
                }
            }
        }
    }

    if actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        let object_name_opt = actual_callee.children().next().and_then(|base_node| {
            if base_node.kind() == SyntaxKind::IDENT {
                return Some(base_node.text().to_string());
            }

            if base_node.kind() == SyntaxKind::EXPR {
                if let Some(ident_child) =
                    base_node.children().find(|n| n.kind() == SyntaxKind::IDENT)
                {
                    return Some(ident_child.text().to_string());
                }
            }

            base_node
                .descendants_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| tok.kind() == SyntaxKind::IDENT)
                .map(|tok| tok.text().to_string())
        });

        let method_token_opt = actual_callee
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .last();

        if let (Some(object_name), Some(method_token)) = (object_name_opt, method_token_opt) {
            use super::diagnostics::is_deprecated_attribute_8312;

            if let Some(kind) =
                is_deprecated_attribute_8312(&object_name, method_token.text(), true)
            {
                ctx.diagnostics.push(BodyDiagnostic::DeprecatedAttribute8312 {
                    name: method_token.text().to_string(),
                    kind,
                    range: method_token.text_range(),
                });
            }
        }
    }

    if actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        if let Some(method_token) = actual_callee
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .last()
        {
            let method_name = method_token.text().to_lowercase();
            if matches!(method_name.as_str(), "delete" | "удалить") {
                let receiver = match ctx.body.expr_idx(callee) {
                    Expr::Field { base, .. } => Some(*base),
                    Expr::MethodCall { receiver, .. } => Some(*receiver),
                    _ => None,
                };

                if let Some(receiver_id) = receiver {
                    if let Some(collection_text) = ctx.matches_foreach_collection(receiver_id) {
                        if !is_followed_by_loop_exit(node) {
                            ctx.emit(BodyDiagnostic::DeletingCollectionItem {
                                collection_text: collection_text.to_string(),
                                range: node.text_range(),
                            });
                        }
                    }
                }
            }
        }
    }

    if actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        if let Some(method_token) = actual_callee
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .last()
        {
            let method_name = method_token.text().to_lowercase();
            if matches!(method_name.as_str(), "insert" | "вставить" | "add" | "добавить")
            {
                let receiver = match ctx.body.expr_idx(callee) {
                    Expr::Field { base, .. } => Some(*base),
                    Expr::MethodCall { receiver, .. } => Some(*receiver),
                    _ => None,
                };

                if let Some(receiver_id) = receiver {
                    for &arg_id in args.iter() {
                        if exprs_are_equal(&ctx.body, receiver_id, arg_id) {
                            ctx.emit(BodyDiagnostic::SelfInsertion { range: node.text_range() });
                            break;
                        }
                    }
                }
            }
        }
    }

    if actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        if let Some(method_token) = actual_callee
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .last()
        {
            use super::diagnostics::is_find_element_method;
            if is_find_element_method(method_token.text()) {
                let has_literal_first_arg = check_find_element_first_arg(&args, ctx);
                if has_literal_first_arg {
                    let range = if let Some(ref arg_list) = arg_list_node {
                        method_token.text_range().cover(arg_list.text_range())
                    } else {
                        method_token.text_range()
                    };
                    ctx.emit(BodyDiagnostic::UsingFindElementByString { range });
                }
            }
        }
    }

    if actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        if let Some(method_token) = actual_callee
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .last()
        {
            use super::diagnostics::is_find_by_code_method;
            if is_find_by_code_method(method_token.text()) {
                if let Some(receiver) = actual_callee.first_child() {
                    if receiver.kind() == SyntaxKind::FIELD_EXPR {
                        let object_name = receiver
                            .children_with_tokens()
                            .filter_map(|e| e.into_token())
                            .filter(|t| t.kind() == SyntaxKind::IDENT)
                            .last()
                            .map(|t| t.text().to_string());

                        let manager_name = receiver.first_child().and_then(|base| {
                            if let Some(token) = base.first_token() {
                                if token.kind() == SyntaxKind::IDENT {
                                    return Some(token.text().to_string());
                                }
                            }
                            None
                        });

                        if let (Some(manager), Some(object)) = (manager_name, object_name) {
                            ctx.emit(BodyDiagnostic::UnsafeFindByCode {
                                manager_name: manager,
                                object_name: object,
                                range: method_token.text_range(),
                            });
                        }
                    }
                }
            }
        }
    }

    if actual_callee.kind() == SyntaxKind::IDENT {
        let callee_name = actual_callee.text().to_string();

        let is_local = {
            let key = callee_name.to_lowercase();
            ctx.local_vars.contains_key(&key) || ctx.param_names.contains(&key)
        };

        if !is_local {
            let arg_presence = arg_list_node.as_ref().map(extract_arg_presence).unwrap_or_default();

            ctx.diagnostics.push(BodyDiagnostic::MissedRequiredParameter {
                callee: callee_name,
                module: None,
                mdo_type: None,
                mdo_name: None,
                args: arg_presence,
                range: node.text_range(),
            });
        }
    }

    if actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        if let Some(replacement) =
            maybe_lower_as_qualified_call(ctx, node, &actual_callee, arg_list_node.as_ref(), &args)
        {
            return replacement;
        }
    }

    Expr::Call { callee, args: args.into_boxed_slice() }
}

fn maybe_lower_as_qualified_call(
    ctx: &mut LoweringCtx,
    call_node: &SyntaxNode,
    field_expr_node: &SyntaxNode,
    arg_list_node: Option<&SyntaxNode>,
    args: &[ExprIdx],
) -> Option<Expr> {
    let call_info = analyze_qualified_call(field_expr_node, ctx)?;

    let field_token = field_name_token(field_expr_node)?;
    let field_name = Name::new(field_token.text());

    let arg_presence = arg_list_node.map(extract_arg_presence).unwrap_or_default();

    match call_info {
        QualifiedCallInfo::TwoLevel { module } => {
            let is_this_object = {
                let lower = module.to_lowercase();
                lower == "этотобъект" || lower == "thisobject"
            };

            if is_this_object {
                ctx.diagnostics.push(BodyDiagnostic::MissedRequiredParameter {
                    callee: field_name.as_str().to_string(),
                    module: None,
                    mdo_type: None,
                    mdo_name: None,
                    args: arg_presence,
                    range: call_node.text_range(),
                });
                return None;
            }

            None
        }
        QualifiedCallInfo::ThreeLevel { mdo_type, mdo_name } => {
            ctx.diagnostics.push(BodyDiagnostic::RedundantAccessToObject {
                kind: RedundantAccessKind::ThreeLevel {
                    mdo_type: mdo_type.clone(),
                    mdo_name: mdo_name.clone(),
                },
                range: call_node.text_range(),
            });

            ctx.diagnostics.push(BodyDiagnostic::MissedRequiredParameter {
                callee: field_name.as_str().to_string(),
                module: None,
                mdo_type: Some(mdo_type.clone()),
                mdo_name: Some(mdo_name.clone()),
                args: arg_presence,
                range: call_node.text_range(),
            });

            if let Some(manager_type) = parse_manager_type(&mdo_type) {
                ctx.external_refs.push(ExternalRef::ManagerAccess {
                    manager_type,
                    object_name: Name::new(&mdo_name),
                    method: Some(field_name.clone()),
                    range: call_node.text_range(),
                });
            }

            let qualified_path = QualifiedName::from_segments([
                Name::new(&mdo_type),
                Name::new(&mdo_name),
                field_name.clone(),
            ]);
            let new_callee = ctx
                .alloc_expr(Expr::QualifiedPath(Box::new(qualified_path)), call_node.text_range());

            Some(Expr::Call { callee: new_callee, args: args.to_vec().into_boxed_slice() })
        }
    }
}

fn lower_arg_list(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Vec<ExprIdx> {
    if let Some(comma_range) = find_trailing_comma(node) {
        ctx.diagnostics.push(BodyDiagnostic::ExtraCommas { range: comma_range });
    }

    let mut args = Vec::new();
    let mut current_expr: Option<ExprIdx> = None;
    let mut has_any_content = false;

    for child in node.children_with_tokens() {
        match child.kind() {
            SyntaxKind::COMMA => {
                args.push(current_expr.unwrap_or_else(|| ctx.missing_expr()));
                current_expr = None;
            }
            SyntaxKind::L_PAREN | SyntaxKind::R_PAREN => {}
            kind if kind.is_trivia() => {}
            _ => {
                if let Some(expr_node) = child.as_node() {
                    current_expr = Some(lower_expr_node(ctx, expr_node));
                    has_any_content = true;
                }
            }
        }
    }

    if has_any_content || !args.is_empty() {
        args.push(current_expr.unwrap_or_else(|| ctx.missing_expr()));
    }

    args
}

fn find_trailing_comma(arg_list: &SyntaxNode) -> Option<syntax::TextRange> {
    use syntax::NodeOrToken;

    let tokens: Vec<_> = arg_list.children_with_tokens().collect();
    let mut iter = tokens.iter().rev().filter(|element| !is_trivia_element(element));

    let r_paren = iter.next()?;
    if !matches!(r_paren, NodeOrToken::Token(t) if t.kind() == SyntaxKind::R_PAREN) {
        return None;
    }

    let prev = iter.next()?;
    match prev {
        NodeOrToken::Token(token) if token.kind() == SyntaxKind::COMMA => Some(token.text_range()),
        _ => None,
    }
}

fn is_trivia_element(element: &syntax::NodeOrToken<SyntaxNode, syntax::SyntaxToken>) -> bool {
    matches!(
        element,
        syntax::NodeOrToken::Token(t) if matches!(
            t.kind(),
            SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT
        )
    )
}

fn extract_arg_presence(arg_list: &SyntaxNode) -> Vec<bool> {
    let mut args = Vec::new();
    let mut has_expr = false;

    for child in arg_list.children_with_tokens() {
        match child.kind() {
            SyntaxKind::COMMA => {
                args.push(has_expr);
                has_expr = false;
            }
            SyntaxKind::L_PAREN | SyntaxKind::R_PAREN => {}
            kind if kind.is_trivia() => {}
            _ => {
                has_expr = true;
            }
        }
    }

    if arg_list.children().count() > 0 || has_expr {
        args.push(has_expr);
    }

    args
}

fn lower_index_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    let base =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let index =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    Expr::Index { base, index }
}

fn lower_field_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    let base =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let field_token = field_name_token(node);

    let field_name =
        field_token.as_ref().map(|tok| Name::new(tok.text())).unwrap_or_else(Name::missing);

    let object_name_opt = node.children().next().and_then(|base_node| {
        if base_node.kind() == SyntaxKind::IDENT {
            return Some(base_node.text().to_string());
        }

        if base_node.kind() == SyntaxKind::EXPR {
            if let Some(ident_child) = base_node.children().find(|n| n.kind() == SyntaxKind::IDENT)
            {
                return Some(ident_child.text().to_string());
            }
        }

        base_node
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::IDENT)
            .map(|tok| tok.text().to_string())
    });

    if let (Some(field_tok), Some(object_name)) = (&field_token, &object_name_opt) {
        use super::diagnostics::is_deprecated_attribute_8312;

        if let Some(kind) = is_deprecated_attribute_8312(object_name, field_tok.text(), false) {
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedAttribute8312 {
                name: field_tok.text().to_string(),
                kind,
                range: field_tok.text_range(),
            });
        }
    }

    let direct_base_name = node.children().next().and_then(|base_node| {
        if base_node.kind() == SyntaxKind::IDENT {
            return Some(base_node.text().to_string());
        }
        if base_node.kind() == SyntaxKind::EXPR {
            if let Some(ident_child) = base_node.children().find(|n| n.kind() == SyntaxKind::IDENT)
            {
                return Some(ident_child.text().to_string());
            }
        }
        None
    });
    if let Some(ref base_name) = direct_base_name {
        let lower = base_name.to_lowercase();
        if lower == "этотобъект" || lower == "thisobject" {
            ctx.diagnostics.push(BodyDiagnostic::RedundantAccessToObject {
                kind: RedundantAccessKind::ThisObject { prefix: base_name.clone() },
                range: node.text_range(),
            });
        }
    }

    Expr::Field { base, field: field_name }
}

fn extract_receiver_name(ctx: &LoweringCtx, expr_id: ExprIdx) -> Option<String> {
    let expr = ctx.body.expr_idx(expr_id);
    match expr {
        Expr::Path(name) => Some(name.as_str().to_string()),
        Expr::Field { base, field } => {
            let base_name = extract_receiver_name(ctx, *base)?;
            Some(format!("{}.{}", base_name, field.as_str()))
        }
        _ => None,
    }
}

enum QualifiedCallInfo {
    TwoLevel { module: String },
    ThreeLevel { mdo_type: String, mdo_name: String },
}

fn analyze_qualified_call(node: &SyntaxNode, ctx: &LoweringCtx) -> Option<QualifiedCallInfo> {
    let first_child = node.children().next()?;

    if first_child.kind() == SyntaxKind::FIELD_EXPR {
        let inner_base = first_child.children().next()?;
        let mdo_type = match inner_base.kind() {
            SyntaxKind::IDENT => inner_base.text().to_string(),
            SyntaxKind::EXPR => {
                let ident_nodes: Vec<_> =
                    inner_base.children().filter(|n| n.kind() == SyntaxKind::IDENT).collect();
                if ident_nodes.len() == 1 {
                    ident_nodes[0].text().to_string()
                } else {
                    return None;
                }
            }
            _ => return None,
        };

        let mdo_name = first_child
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .last()
            .map(|tok| tok.text().to_string())?;

        let key = mdo_type.to_lowercase();
        if ctx.local_vars.contains_key(&key) || ctx.param_names.contains(&key) {
            return None;
        }

        bsl_metadata::MdoType::from_plural(&mdo_type)?;

        tracing::debug!(
            mdo_type = %mdo_type,
            mdo_name = %mdo_name,
            "Detected three-level call"
        );
        return Some(QualifiedCallInfo::ThreeLevel { mdo_type, mdo_name });
    }

    let module_name = if first_child.kind() == SyntaxKind::IDENT {
        Some(first_child.text().to_string())
    } else if first_child.kind() == SyntaxKind::EXPR {
        let idents: Vec<_> =
            first_child.children().filter(|n| n.kind() == SyntaxKind::IDENT).collect();
        if idents.len() == 1 {
            Some(idents[0].text().to_string())
        } else {
            None
        }
    } else {
        None
    };

    let module = module_name?;

    let key = module.to_lowercase();
    if ctx.local_vars.contains_key(&key) || ctx.param_names.contains(&key) {
        return None;
    }

    Some(QualifiedCallInfo::TwoLevel { module })
}

fn lower_new_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let type_name = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)
        .map(|tok| Name::new(tok.text()));

    if let Some(ref name) = type_name {
        if is_file_system_type(name.as_str()) {
            ctx.diagnostics.push(BodyDiagnostic::FileSystemAccess { range: node.text_range() });
        }
    }

    let style_type_name = if let Some(ref name) = type_name {
        if is_style_element_type(name.as_str()) {
            Some(name.as_str().to_string())
        } else {
            None
        }
    } else {
        extract_type_name_from_first_arg(node).filter(|name| is_style_element_type(name))
    };

    if let Some(style_name) = style_type_name {
        ctx.diagnostics.push(BodyDiagnostic::StyleElementConstructors {
            type_name: style_name,
            range: node.text_range(),
        });
    }

    let is_system_info = if let Some(ref name) = type_name {
        is_system_information_type(name.as_str())
    } else {
        extract_type_name_from_first_arg(node)
            .map(|name| is_system_information_type(&name))
            .unwrap_or(false)
    };

    if is_system_info {
        ctx.diagnostics.push(BodyDiagnostic::UseSystemInformation { range: node.text_range() });
    }

    if !ctx.in_platform_guard {
        let unix_unavailable_type = if let Some(ref name) = type_name {
            is_unix_unavailable_type(name.as_str()).then(|| name.as_str().to_string())
        } else {
            extract_type_name_from_first_arg(node).filter(|name| is_unix_unavailable_type(name))
        };

        if let Some(type_name_str) = unix_unavailable_type {
            ctx.diagnostics.push(BodyDiagnostic::UsingObjectNotAvailableUnix {
                type_name: type_name_str,
                range: node.text_range(),
            });
        }
    }

    let args = node
        .children()
        .find(|n| n.kind() == SyntaxKind::ARG_LIST)
        .map(|arg_list| lower_arg_list(ctx, &arg_list))
        .unwrap_or_default();

    Expr::New { type_name, args: args.into_boxed_slice() }
}

pub(crate) fn exprs_are_equal(body: &Body, lhs: ExprIdx, rhs: ExprIdx) -> bool {
    match (body.expr_idx(lhs), body.expr_idx(rhs)) {
        (Expr::Missing, Expr::Missing) => true,
        (Expr::Path(name1), Expr::Path(name2)) => name1.eq_ignore_case(name2),
        (Expr::QualifiedPath(p1), Expr::QualifiedPath(p2)) => {
            let s1 = p1.segments();
            let s2 = p2.segments();
            s1.len() == s2.len() && s1.iter().zip(s2.iter()).all(|(a, b)| a.eq_ignore_case(b))
        }
        (Expr::Field { base: b1, field: f1 }, Expr::Field { base: b2, field: f2 }) => {
            f1.eq_ignore_case(f2) && exprs_are_equal(body, *b1, *b2)
        }
        (Expr::Index { base: b1, index: i1 }, Expr::Index { base: b2, index: i2 }) => {
            exprs_are_equal(body, *b1, *b2) && exprs_are_equal(body, *i1, *i2)
        }
        (
            Expr::MethodCall { receiver: r1, method: m1, .. },
            Expr::MethodCall { receiver: r2, method: m2, .. },
        ) => m1.eq_ignore_case(m2) && exprs_are_equal(body, *r1, *r2),
        (Expr::Call { callee: c1, .. }, Expr::Call { callee: c2, .. }) => {
            exprs_are_equal(body, *c1, *c2)
        }
        _ => false,
    }
}

fn is_os_users_method(name: &str) -> bool {
    bsl_platform::security::registry()
        .lookup_global(name)
        .is_some_and(|e| matches!(e.category, bsl_platform::security::Category::OsUsers))
}

fn is_external_app_method(name: &str) -> bool {
    bsl_platform::security::registry()
        .lookup_global(name)
        .is_some_and(|e| matches!(e.category, bsl_platform::security::Category::ExternalApp))
}

fn is_file_system_type(name: &str) -> bool {
    bsl_platform::security::registry()
        .lookup_constructor(name)
        .is_some_and(|e| matches!(e.category, bsl_platform::security::Category::FileSystem))
}

fn is_style_element_type(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "цвет" | "color" | "шрифт" | "font" | "рамка" | "border")
}

fn is_system_information_type(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "системнаяинформация" | "systeminfo")
}

fn is_unix_unavailable_type(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "comобъект" | "comobject" | "почта" | "mail")
}

fn is_write_log_event_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "записьжурналарегистрации" || lower == "writelogevent"
}

fn check_write_log_event_call(ctx: &mut LoweringCtx, node: &SyntaxNode) {
    let arg_list = match node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST) {
        Some(al) => al,
        None => return,
    };

    let args = collect_arguments(&arg_list);
    let arg_count = args.len();

    let log_level_empty = args.get(1).map(|a| a.is_none()).unwrap_or(true);
    let comment_empty = args.get(4).map(|a| a.is_none()).unwrap_or(true);

    let has_error_log_level =
        args.get(1).and_then(|a| a.as_ref()).map(has_error_log_level_value).unwrap_or(false);

    let has_detail_error_description =
        args.get(4).and_then(|a| a.as_ref()).map(has_detail_error_description).unwrap_or(false);

    let has_detail_error_description = if !has_detail_error_description && ctx.in_except_block {
        if let Some(comment_arg) = args.get(4).and_then(|a| a.as_ref()) {
            resolve_comment_in_except_block(comment_arg, node).unwrap_or_default()
        } else {
            false
        }
    } else {
        has_detail_error_description
    };

    ctx.diagnostics.push(BodyDiagnostic::UsageWriteLogEvent {
        in_except_block: ctx.in_except_block,
        arg_count,
        log_level_empty,
        comment_empty,
        has_error_log_level,
        has_detail_error_description,
        except_has_raise: ctx.except_has_raise,
        range: node.text_range(),
    });
}

fn collect_arguments(arg_list: &SyntaxNode) -> Vec<Option<SyntaxNode>> {
    let mut args: Vec<Option<SyntaxNode>> = Vec::new();
    let mut current_arg: Option<SyntaxNode> = None;
    let mut has_content = false;

    for child in arg_list.children_with_tokens() {
        match child.kind() {
            SyntaxKind::COMMA => {
                args.push(current_arg.take());
                has_content = true;
            }
            SyntaxKind::L_PAREN | SyntaxKind::R_PAREN => {}
            kind if kind.is_trivia() => {}
            _ => {
                if current_arg.is_none() {
                    if let Some(node) = child.as_node() {
                        current_arg = Some(node.clone());
                        has_content = true;
                    }
                }
            }
        }
    }

    if current_arg.is_some() || has_content {
        args.push(current_arg);
    }

    args
}

fn has_error_log_level_value(arg: &SyntaxNode) -> bool {
    let text = arg.text().to_string().to_lowercase();
    if text.contains("уровеньжурналарегистрации") || text.contains("eventloglevel")
    {
        return text.contains("ошибка") || text.contains("error");
    }
    true
}

fn has_detail_error_description(arg: &SyntaxNode) -> bool {
    let text = arg.text().to_string().to_lowercase();
    (text.contains("подробноепредставлениеошибки") || text.contains("detailerrordescription"))
        && (text.contains("информацияобошибке") || text.contains("errorinfo"))
}

fn resolve_comment_in_except_block(arg: &SyntaxNode, call_node: &SyntaxNode) -> Option<bool> {
    let arg_text = arg.text().to_string();
    let var_name = arg_text.trim();
    if var_name.is_empty() || !var_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let var_name = var_name.to_lowercase();

    let except_clause = call_node.ancestors().find(|n| n.kind() == SyntaxKind::EXCEPT_CLAUSE)?;

    let stmt_list = except_clause.children().find(|n| n.kind() == SyntaxKind::STMT_LIST)?;

    for child in stmt_list.children() {
        if child.kind() == SyntaxKind::ASSIGN_STMT {
            let lhs_node = match child.children().next() {
                Some(n) => n,
                None => continue,
            };
            let lhs_name = lhs_node.text().to_string().trim().to_lowercase();
            if lhs_name == var_name {
                let rhs_text = child.text().to_string().to_lowercase();
                let has_detail = (rhs_text.contains("подробноепредставлениеошибки")
                    || rhs_text.contains("detailerrordescription"))
                    && (rhs_text.contains("информацияобошибке") || rhs_text.contains("errorinfo"));
                return Some(has_detail);
            }
        }
    }

    Some(true)
}

fn extract_type_name_from_first_arg(node: &SyntaxNode) -> Option<String> {
    let arg_list = node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST)?;
    let first_arg = arg_list.children().next()?;

    let string_token = first_arg
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::STRING)?;

    let text = string_token.text();
    if text.len() >= 2 && (text.starts_with('"') || text.starts_with('\'')) {
        Some(text[1..text.len() - 1].to_string())
    } else {
        None
    }
}

fn is_file_system_method(name: &str) -> bool {
    bsl_platform::security::registry()
        .lookup_global(name)
        .is_some_and(|e| matches!(e.category, bsl_platform::security::Category::FileSystem))
}

fn is_form_data_to_value_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "данныеформывзначение" | "formdatatovalue")
}

fn is_get_form_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "получитьформу" | "getform")
}

fn is_proceed_with_call_method(name_lower: &str) -> bool {
    matches!(name_lower, "продолжитьвызов" | "proceedwithcall")
}

fn check_using_external_code_tools(
    ctx: &mut LoweringCtx,
    _actual_callee: &SyntaxNode,
    idents: &[syntax::SyntaxToken],
    call_node: &SyntaxNode,
) {
    use super::diagnostics::{is_external_code_tools_method, is_external_code_tools_name};

    if idents.len() != 2 {
        return;
    }

    let receiver_name = idents[0].text();
    let method_name = idents[1].text();

    let receiver_key = receiver_name.to_lowercase();
    let is_local =
        ctx.local_vars.contains_key(&receiver_key) || ctx.param_names.contains(&receiver_key);

    if !is_local
        && is_external_code_tools_name(receiver_name)
        && is_external_code_tools_method(method_name)
    {
        ctx.diagnostics
            .push(BodyDiagnostic::UsingExternalCodeTools { range: call_node.text_range() });
    }
}

fn is_str_template_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "стршаблон" | "strtemplate")
}

fn is_wrong_str_template(template_string: &str, used_params_count: usize) -> bool {
    let is_wrong_call = compare_template_and_params(template_string, used_params_count);
    if !is_wrong_call {
        return false;
    }

    let cleaned = remove_double_percent(template_string);
    compare_template_and_params(&cleaned, used_params_count)
}

fn remove_double_percent(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'%' && bytes[i + 1] == b'%' {
            i += 2;
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

fn parse_placeholder(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    if pos >= bytes.len() || bytes[pos] != b'%' {
        return None;
    }

    let start = pos + 1;
    if start >= bytes.len() {
        return None;
    }

    if bytes[start] == b'(' {
        let num_start = start + 1;
        let mut num_end = num_start;
        while num_end < bytes.len() && bytes[num_end].is_ascii_digit() {
            num_end += 1;
        }
        if num_end > num_start && num_end < bytes.len() && bytes[num_end] == b')' {
            let num_str = std::str::from_utf8(&bytes[num_start..num_end]).ok()?;
            let num: usize = num_str.parse().ok()?;
            return Some((num, num_end - pos + 1));
        }
        return None;
    }

    let mut num_end = start;
    while num_end < bytes.len() && bytes[num_end].is_ascii_digit() {
        num_end += 1;
    }
    if num_end > start {
        let num_str = std::str::from_utf8(&bytes[start..num_end]).ok()?;
        let num: usize = num_str.parse().ok()?;
        return Some((num, num_end - pos));
    }

    None
}

#[allow(clippy::nonminimal_bool)]
fn compare_template_and_params(template_string: &str, used_params_count: usize) -> bool {
    let bytes = template_string.as_bytes();
    let have_params = used_params_count > 0;

    let mut has_valid_placeholder = false;
    let mut has_wrong_number = false;
    let mut used_placeholders = [false; 11];

    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'%' {
                i += 2;
                continue;
            }

            if let Some((num, len)) = parse_placeholder(bytes, i) {
                if (1..=10).contains(&num) {
                    has_valid_placeholder = true;
                    used_placeholders[num] = true;
                    if num > used_params_count {
                        return true;
                    }
                } else {
                    has_wrong_number = true;
                }
                i += len;
                continue;
            }
        }
        i += 1;
    }

    if has_wrong_number {
        return true;
    }
    if has_valid_placeholder && !have_params {
        return true;
    }
    if !has_valid_placeholder && have_params {
        return true;
    }

    if has_valid_placeholder {
        for &used in used_placeholders.iter().take(used_params_count + 1).skip(1) {
            if !used {
                return true;
            }
        }
    }

    false
}

fn find_string_in_node(node: &SyntaxNode) -> Option<String> {
    for token in node.descendants_with_tokens() {
        if let syntax::NodeOrToken::Token(t) = token {
            if t.kind() == SyntaxKind::STRING {
                let text = t.text().to_string();
                if text.len() > 2 {
                    return Some(text[1..text.len() - 1].to_string());
                }
            }
        }
    }
    None
}

fn parse_manager_type(mdo_type: &str) -> Option<ManagerType> {
    let lower = mdo_type.to_lowercase();
    match lower.as_str() {
        "документы" | "documents" => Some(ManagerType::Documents),
        "справочники" | "catalogs" => Some(ManagerType::Catalogs),
        "обработки" | "dataprocessors" => Some(ManagerType::DataProcessors),
        "отчёты" | "отчеты" | "reports" => Some(ManagerType::Reports),
        "регистрысведений" | "informationregisters" => {
            Some(ManagerType::InformationRegisters)
        }
        "регистрынакопления" | "accumulationregisters" => {
            Some(ManagerType::AccumulationRegisters)
        }
        _ => None,
    }
}

fn check_find_element_first_arg(args: &[ExprIdx], ctx: &LoweringCtx) -> bool {
    if args.is_empty() {
        return true;
    }

    let first_arg = args[0];
    let expr = ctx.body.expr_idx(first_arg);
    matches!(expr, Expr::Literal(Literal::String(_)) | Expr::Literal(Literal::Number(_)))
}

fn determine_magic_number_context(token: &syntax::SyntaxToken) -> MagicNumberContext {
    let mut node = token.parent();

    let mut in_binary_expr = false;
    let mut in_arg_list = false;
    let mut arg_index: usize = 0;
    let mut in_ternary = false;
    let mut in_call = false;
    let mut in_assign = false;
    let mut has_dot_in_assign = false;

    while let Some(current) = node {
        match current.kind() {
            SyntaxKind::PARAM => {
                return MagicNumberContext::InDefaultParam;
            }
            SyntaxKind::INDEX_EXPR => {
                return MagicNumberContext::InArrayIndex;
            }
            SyntaxKind::NEW_EXPR => {
                if let Some(type_name) = current
                    .children_with_tokens()
                    .filter_map(|el| el.into_token())
                    .find(|tok| tok.kind() == SyntaxKind::IDENT)
                {
                    let name = type_name.text().to_lowercase();
                    if name.contains("структура")
                        || name.contains("structure")
                        || name.contains("соответствие")
                        || name.contains("map")
                    {
                        return MagicNumberContext::InStructureConstructor;
                    }
                    return MagicNumberContext::InConstructor { type_name: name };
                }
            }
            SyntaxKind::BINARY_EXPR => {
                in_binary_expr = true;
            }
            SyntaxKind::ARG_LIST => {
                in_arg_list = true;
                arg_index = current
                    .children_with_tokens()
                    .take_while(|child| !child.text_range().contains_range(token.text_range()))
                    .filter(|child| child.as_token().is_some_and(|t| t.kind() == SyntaxKind::COMMA))
                    .count();
            }
            SyntaxKind::TERNARY_EXPR => {
                in_ternary = true;
            }
            SyntaxKind::CALL_STMT | SyntaxKind::CALL_EXPR => {
                in_call = true;
                if let Some(method_name) = find_method_name_for_magic_number(&current) {
                    let name = method_name.to_lowercase();
                    if name == "вставить" || name == "insert" {
                        return MagicNumberContext::InStructureInsert;
                    }
                    if (name == "окр" || name == "round") && arg_index == 1 {
                        return MagicNumberContext::InRoundPrecision;
                    }
                }
            }
            SyntaxKind::ASSIGN_STMT => {
                in_assign = true;
                has_dot_in_assign = current
                    .children_with_tokens()
                    .any(|el| el.as_token().is_some_and(|t| t.kind() == SyntaxKind::DOT));
            }
            SyntaxKind::RETURN_STMT => {
                return MagicNumberContext::InReturn;
            }
            SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF => {
                break;
            }
            _ => {}
        }
        node = current.parent();
    }

    if in_assign {
        if has_dot_in_assign && !in_arg_list {
            return MagicNumberContext::InPropertyAssignment;
        }
        if !in_binary_expr && !in_arg_list {
            return MagicNumberContext::InSimpleAssignment;
        }
        if in_ternary && !in_binary_expr {
            return MagicNumberContext::InTernaryBranch;
        }
    }

    if in_call && in_arg_list && !in_binary_expr {
        return MagicNumberContext::InMethodCall;
    }

    if in_binary_expr {
        return MagicNumberContext::InExpression;
    }

    MagicNumberContext::Other
}

fn find_method_name_for_magic_number(node: &SyntaxNode) -> Option<String> {
    for child in node.descendants() {
        if child.kind() == SyntaxKind::FIELD_EXPR {
            return child
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .filter(|t| t.kind() == SyntaxKind::IDENT)
                .last()
                .map(|t| t.text().to_string());
        }
        if child.kind() == SyntaxKind::ARG_LIST {
            break;
        }
    }

    for child in node.children_with_tokens() {
        match child {
            syntax::NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT => {
                return Some(t.text().to_string());
            }
            syntax::NodeOrToken::Node(n) if n.kind() == SyntaxKind::IDENT => {
                if let Some(t) = n.first_token() {
                    return Some(t.text().to_string());
                }
            }
            syntax::NodeOrToken::Node(n) if n.kind() == SyntaxKind::ARG_LIST => break,
            _ => {}
        }
    }

    None
}

fn is_unsafe_safe_mode_context(call_node: &SyntaxNode) -> bool {
    let mut current = call_node.parent();

    while let Some(node) = current {
        match node.kind() {
            SyntaxKind::UNARY_EXPR
                if node
                    .children_with_tokens()
                    .filter_map(|el| el.into_token())
                    .any(|tok| tok.kind() == SyntaxKind::KW_NOT) =>
            {
                return true;
            }
            SyntaxKind::BINARY_EXPR => {
                let has_comparison =
                    node.children_with_tokens().filter_map(|el| el.into_token()).any(|tok| {
                        matches!(
                            tok.kind(),
                            SyntaxKind::EQ
                                | SyntaxKind::NEQ
                                | SyntaxKind::LT
                                | SyntaxKind::LE
                                | SyntaxKind::GT
                                | SyntaxKind::GE
                        )
                    });
                if has_comparison {
                    return false;
                }
                let has_boolean = node
                    .children_with_tokens()
                    .filter_map(|el| el.into_token())
                    .any(|tok| matches!(tok.kind(), SyntaxKind::KW_AND | SyntaxKind::KW_OR));
                if has_boolean {
                    return true;
                }
            }
            SyntaxKind::PAREN_EXPR | SyntaxKind::EXPR => {}
            SyntaxKind::IF_STMT | SyntaxKind::ELSIF_CLAUSE => {
                if let Some(cond) = node.children().find(|n| n.kind() == SyntaxKind::EXPR) {
                    let call_range = call_node.text_range();
                    let contains_call = cond
                        .descendants()
                        .any(|n| n.kind() == SyntaxKind::CALL_EXPR && n.text_range() == call_range);
                    if contains_call {
                        let has_comparison = cond.descendants().any(|n| {
                            n.kind() == SyntaxKind::BINARY_EXPR
                                && n.children_with_tokens().filter_map(|el| el.into_token()).any(
                                    |tok| {
                                        matches!(
                                            tok.kind(),
                                            SyntaxKind::EQ
                                                | SyntaxKind::NEQ
                                                | SyntaxKind::LT
                                                | SyntaxKind::LE
                                                | SyntaxKind::GT
                                                | SyntaxKind::GE
                                        )
                                    },
                                )
                        });
                        return !has_comparison;
                    }
                }
                return false;
            }
            SyntaxKind::ASSIGN_STMT => {
                if let Some(rhs_node) = node.children().nth(1) {
                    let call_range = call_node.text_range();
                    if rhs_node.text_range() == call_range {
                        return false;
                    }
                    if rhs_node.kind() == SyntaxKind::EXPR {
                        if let Some(inner) = rhs_node.children().next() {
                            if inner.text_range() == call_range {
                                return false;
                            }
                        }
                    }
                }
                return false;
            }
            SyntaxKind::ARG_LIST | SyntaxKind::CALL_EXPR | SyntaxKind::CALL_STMT => {
                return false;
            }
            SyntaxKind::STMT_LIST
            | SyntaxKind::PROCEDURE_DEF
            | SyntaxKind::FUNCTION_DEF
            | SyntaxKind::SOURCE_FILE => {
                break;
            }
            _ => {}
        }
        current = node.parent();
    }

    false
}
