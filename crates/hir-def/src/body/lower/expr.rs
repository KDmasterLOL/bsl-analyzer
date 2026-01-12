//! Expression lowering.
//!
//! This module handles lowering of BSL expressions from AST to HIR.

use syntax::{SyntaxKind, SyntaxNode};

use crate::body::{Body, BodyDiagnostic, ExternalRef, ManagerType};
use crate::hir::{BinaryOp, Expr, ExprId, Literal, UnaryOp};
use crate::{Name, QualifiedName};

use super::diagnostics::is_deprecated_method;
use super::utils::{extract_string_content, looks_like_sdbl};
use super::LoweringCtx;

/// Lower an expression node (handles EXPR wrapper).
pub(crate) fn lower_expr_node(ctx: &mut LoweringCtx, node: &SyntaxNode) -> ExprId {
    // Handle EXPR wrapper - unwrap to get actual expression
    let actual_node = if node.kind() == SyntaxKind::EXPR {
        node.children().next().unwrap_or_else(|| node.clone())
    } else {
        node.clone()
    };

    lower_expr(ctx, &actual_node)
}

/// Lower an expression.
fn lower_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> ExprId {
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
            // Unwrap parenthesized expression
            return node
                .children()
                .next()
                .map(|n| lower_expr_node(ctx, &n))
                .unwrap_or_else(|| ctx.missing_expr());
        }
        SyntaxKind::AWAIT_EXPR => {
            // Unwrap await expression (Ждать <expr>)
            // Just lower the inner expression - await semantic is not modeled in HIR yet
            return node
                .children()
                .next()
                .map(|n| lower_expr_node(ctx, &n))
                .unwrap_or_else(|| ctx.missing_expr());
        }
        SyntaxKind::IDENT => {
            // Identifier - variable reference
            let text = node.text().to_string();
            Expr::Path(Name::new(&text))
        }
        SyntaxKind::EXPR => {
            // Wrapped expression
            return node
                .children()
                .next()
                .map(|n| lower_expr(ctx, &n))
                .unwrap_or_else(|| ctx.missing_expr());
        }
        _ => {
            // Try to find IDENT token for simple identifier expressions
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

    // Associate SDBL with ExprId
    if let Some(idx) = ctx.pending_sdbl.iter().position(|(query_text, _)| {
        if let Expr::Literal(Literal::String(ref expr_string)) = ctx.body.exprs[expr_id] {
            query_text == expr_string
        } else {
            false
        }
    }) {
        let (_query_text, query_info) = ctx.pending_sdbl.remove(idx);
        ctx.body.sdbl_exprs.push((expr_id, query_info));
    }

    expr_id
}

/// Lower a literal expression.
fn lower_literal(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    // Find the actual literal token
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

            // Wrap in NotNan, fallback to 0.0 if somehow NaN (should never happen with parsed literals)
            let value = ordered_float::NotNan::new(value)
                .unwrap_or_else(|_| ordered_float::NotNan::new(0.0).unwrap());
            Literal::Number(value)
        }
        SyntaxKind::STRING | SyntaxKind::STRING_START => {
            // Extract full string content (handles multiline with |)
            let value = extract_string_content(node).unwrap_or_default();

            // Check if this is SDBL query
            if looks_like_sdbl(&value) {
                let sdbl_ast = parser::parse_sdbl(&value);

                if !sdbl_ast.has_errors() {
                    let query_info = syntax::SdblQueryInfo::new(
                        node.text_range(),
                        value.clone(),
                        Some(sdbl_ast),
                    );

                    ctx.pending_sdbl.push((value.clone(), query_info));
                }
            }

            Literal::String(value)
        }
        SyntaxKind::DATE => {
            let text = token.text();
            // Remove quotes
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

/// Lower binary expression.
fn lower_binary_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    let lhs_node = match children.next() {
        Some(n) => n,
        None => return Expr::Missing,
    };
    let lhs = lower_expr_node(ctx, &lhs_node);

    // Find operator token
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

    Expr::BinaryOp { lhs, rhs, op }
}

/// Lower unary expression.
fn lower_unary_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    // Find operator token
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

/// Lower ternary expression.
fn lower_ternary_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    let condition =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let then_expr =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let else_expr =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    Expr::Ternary { condition, then_expr, else_expr }
}

/// Lower call expression.
fn lower_call_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    // Callee can be identifier, field expression, etc.
    let callee_node = match children.next() {
        Some(n) => n,
        None => return Expr::Missing,
    };

    // Check if this is a global call to a deprecated method
    // Unwrap EXPR wrapper if present
    let actual_callee = if callee_node.kind() == SyntaxKind::EXPR {
        callee_node.children().next().unwrap_or_else(|| callee_node.clone())
    } else {
        callee_node.clone()
    };

    // Track safe mode method name for later diagnostic check
    let mut safe_mode_name: Option<String> = None;

    // Track if this is a StrTemplate call for later validation
    let mut is_str_template_call = false;

    // Only check for IDENT (global function call), not FIELD_EXPR (method call)
    if actual_callee.kind() == SyntaxKind::IDENT {
        let name = actual_callee.text().to_string();

        // Check for safe mode methods FIRST (before name is moved)
        use super::diagnostics::is_safe_mode_method;
        if is_safe_mode_method(&name) {
            safe_mode_name = Some(name.clone());
        }

        // Check for deprecated global methods (8.3.12)
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
            // Emit DeprecatedCurrentDate diagnostic
            // Range covers just the method name (IDENT token)
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedCurrentDate {
                name: name.clone(),
                range: actual_callee.text_range(),
            });
        }

        use super::diagnostics::is_deprecated_find;

        if is_deprecated_find(&name) {
            // Emit DeprecatedFind diagnostic
            // Range covers just the method name (IDENT token)
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedFind {
                name: name.clone(),
                range: actual_callee.text_range(),
            });
        }

        use super::diagnostics::is_deprecated_message;

        if is_deprecated_message(&name) {
            // Emit DeprecatedMessage diagnostic
            // Range covers just the method name (IDENT token)
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedMessage {
                name: name.clone(),
                range: actual_callee.text_range(),
            });
        }

        use super::diagnostics::{is_deprecated_managed_form, is_type_method};

        if is_type_method(&name) {
            // Check for Type("УправляемаяФорма") / Type("ManagedForm")
            // Find ARG_LIST and check first argument
            if let Some(arg_list) = node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST) {
                // Get first argument (first child of ARG_LIST)
                if let Some(first_arg) = arg_list.children().next() {
                    // Check if it's a STRING literal
                    if let Some(string_token) = first_arg
                        .descendants_with_tokens()
                        .filter_map(|el| el.into_token())
                        .find(|tok| tok.kind() == SyntaxKind::STRING)
                    {
                        let text = string_token.text();
                        if text.len() >= 2 {
                            // Remove quotes and unescape
                            let inner = &text[1..text.len() - 1];
                            let content = inner.replace("\"\"", "\"");

                            if is_deprecated_managed_form(&content) {
                                // Emit DeprecatedTypeManagedForm diagnostic
                                // Range covers the string literal token
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

        // Check for Eval/Вычислить calls (forbidden on server) BEFORE name is moved
        let name_lower = name.to_lowercase();
        if (name_lower == "eval" || name_lower == "вычислить") && !ctx.is_client_only {
            // Emit ExecuteExternalCode diagnostic
            // Range covers the entire call expression including arguments
            ctx.diagnostics.push(BodyDiagnostic::ExecuteExternalCode { range: node.text_range() });
        }

        // Check for external app starting methods
        if is_external_app_method(&name) {
            // Emit ExternalAppStarting diagnostic
            // Range is just the method name (IDENT token), not the whole call
            ctx.diagnostics
                .push(BodyDiagnostic::ExternalAppStarting { range: actual_callee.text_range() });
        }

        // Check for file system access methods
        if is_file_system_method(&name) {
            // Emit FileSystemAccess diagnostic
            // Range is just the method name (IDENT token), not the whole call
            ctx.diagnostics
                .push(BodyDiagnostic::FileSystemAccess { range: actual_callee.text_range() });
        }

        // Check for FormDataToValue method in context methods
        if is_form_data_to_value_method(&name) && !ctx.has_no_context_annotation {
            // Emit FormDataToValue diagnostic
            // Range is just the method name (IDENT token), not the whole call
            ctx.diagnostics
                .push(BodyDiagnostic::FormDataToValue { range: actual_callee.text_range() });
        }

        // Check for deprecated GetForm/ПолучитьФорму method
        if is_get_form_method(&name) {
            // Emit GetFormMethod diagnostic
            // Range is just the method name (IDENT token), not the whole call
            ctx.diagnostics.push(BodyDiagnostic::GetFormMethod {
                method_name: name.clone(),
                range: actual_callee.text_range(),
            });
        }

        // Track StrTemplate call for later validation (after arg_list_node is available)
        if is_str_template_method(&name) {
            is_str_template_call = true;
        }

        if is_deprecated_method(&name) {
            // Emit DeprecatedMethod diagnostic
            // Range covers the entire call expression including arguments
            ctx.diagnostics
                .push(BodyDiagnostic::DeprecatedMethod { name, range: node.text_range() });
        }
    } else if actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        // Check for external app methods in qualified calls (obj.method())
        // Extract all IDENT tokens from FIELD_EXPR (use descendants to unwrap EXPR wrappers)
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

        // Collect ExternalRef for module dependency graph (two-level calls: Module.Method())
        if idents.len() == 2 {
            let module_name = idents[0].text();
            let method_name_str = idents[1].text();
            let key = module_name.to_lowercase();

            // Only collect if module_name is NOT a local variable or parameter
            if !ctx.local_vars.contains_key(&key) && !ctx.param_names.contains(&key) {
                ctx.external_refs.push(crate::body::ExternalRef::QualifiedCall {
                    receiver: crate::Name::new(module_name),
                    method: crate::Name::new(method_name_str),
                    range: actual_callee.text_range(),
                });
            }
        }

        // Extract method name from FIELD_EXPR (last IDENT token after DOT)
        if let Some(method_token) = idents.last() {
            let method_name = method_token.text();
            if is_external_app_method(method_name) {
                // Range is just the method name token, not the whole call
                ctx.diagnostics
                    .push(BodyDiagnostic::ExternalAppStarting { range: method_token.text_range() });
            }

            // Check for file system access methods in qualified calls
            if is_file_system_method(method_name) {
                // Range is just the method name token, not the whole call
                ctx.diagnostics
                    .push(BodyDiagnostic::FileSystemAccess { range: method_token.text_range() });
            }

            // Check for FormDataToValue method in context methods (qualified calls)
            if is_form_data_to_value_method(method_name) && !ctx.has_no_context_annotation {
                // Range is just the method name token, not the whole call
                ctx.diagnostics
                    .push(BodyDiagnostic::FormDataToValue { range: method_token.text_range() });
            }

            // Check for deprecated GetForm/ПолучитьФорму method (qualified calls)
            if is_get_form_method(method_name) {
                // Range is just the method name token, not the whole call
                ctx.diagnostics.push(BodyDiagnostic::GetFormMethod {
                    method_name: method_name.to_string(),
                    range: method_token.text_range(),
                });
            }
        }
    }

    let callee = lower_expr_node(ctx, &callee_node);

    // Find ARG_LIST for both lowering and diagnostics
    let arg_list_node = node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST);

    // Arguments
    let args =
        arg_list_node.as_ref().map(|arg_list| lower_arg_list(ctx, arg_list)).unwrap_or_default();

    // Check for DisableSafeMode diagnostic after args are lowered
    if let Some(method_name) = safe_mode_name {
        // Check if this is a safe call by examining first argument
        let is_safe = if !args.is_empty() {
            match &ctx.body.exprs[args[0]] {
                Expr::Literal(Literal::Bool(true)) => {
                    // SetSafeMode(True) is safe
                    method_name.to_lowercase() == "установитьбезопасныйрежим"
                        || method_name.to_lowercase() == "setsafemode"
                }
                Expr::Literal(Literal::Bool(false)) => {
                    // SetSafeModeDisabled(False) is safe
                    method_name.to_lowercase() == "установитьотключениебезопасногорежима"
                        || method_name.to_lowercase() == "setsafemodedisabled"
                }
                _ => false, // Variable or other expression - unsafe
            }
        } else {
            false // No argument - unsafe
        };

        if !is_safe {
            // Find the method name token for the range
            let method_token = callee_node
                .descendants_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| tok.kind() == SyntaxKind::IDENT);

            if let Some(token) = method_token {
                ctx.diagnostics.push(BodyDiagnostic::DisableSafeMode {
                    method_name,
                    range: token.text_range(),
                });
            }
        }
    }

    // Check for StrTemplate/СтрШаблон incorrect usage
    if is_str_template_call {
        if let Some(ref arg_list) = arg_list_node {
            // Extract template string (first argument)
            if let Some(first_arg) = arg_list.children().next() {
                if let Some(template_string) = find_string_in_node(&first_arg) {
                    // Count parameters (number of commas = number of arguments - 1)
                    let param_count = arg_list
                        .children_with_tokens()
                        .filter(|el| el.as_token().is_some_and(|t| t.kind() == SyntaxKind::COMMA))
                        .count();

                    // Validate template
                    if is_wrong_str_template(&template_string, param_count) {
                        ctx.diagnostics.push(BodyDiagnostic::IncorrectUseOfStrTemplate {
                            range: node.text_range(),
                        });
                    }
                }
            }
        }
    }

    // Check for Query.Execute() call inside a loop for CreateQueryInCycle diagnostic
    if ctx.in_loop() && actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        // Extract method name from FIELD_EXPR (last IDENT or KW_EXECUTE token)
        if let Some(method_token) = actual_callee
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| matches!(tok.kind(), SyntaxKind::IDENT | SyntaxKind::KW_EXECUTE))
            .last()
        {
            let method_name = method_token.text().to_lowercase();
            if matches!(method_name.as_str(), "execute" | "выполнить") {
                // Extract receiver from HIR (callee can be Field or MethodCall)
                let receiver = match ctx.body.expr(callee) {
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

    // Check for deprecated Chart methods (8.3.12) - вызовы через field expression
    if actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        // Extract object name (first child of FIELD_EXPR)
        let object_name_opt = actual_callee.children().next().and_then(|base_node| {
            // Try different approaches to extract IDENT text
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

        // Extract method name (last IDENT token)
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
                    name: method_token.text().to_string(), // Preserve original case
                    kind,
                    range: method_token.text_range(),
                });
            }
        }
    }

    // Check for Collection.Delete() call inside ForEach for DeletingCollectionItem diagnostic
    if actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        // Extract method name from FIELD_EXPR (last IDENT token)
        if let Some(method_token) = actual_callee
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .last()
        {
            let method_name = method_token.text().to_lowercase();
            if matches!(method_name.as_str(), "delete" | "удалить") {
                // Extract receiver from HIR (callee can be Field or MethodCall)
                let receiver = match ctx.body.expr(callee) {
                    Expr::Field { base, .. } => Some(*base),
                    Expr::MethodCall { receiver, .. } => Some(*receiver),
                    _ => None,
                };

                if let Some(receiver_id) = receiver {
                    if let Some(collection_text) = ctx.matches_foreach_collection(receiver_id) {
                        ctx.emit(BodyDiagnostic::DeletingCollectionItem {
                            collection_text: collection_text.to_string(),
                            range: node.text_range(),
                        });
                    }
                }
            }
        }
    }

    // Emit MissedRequiredParameter diagnostic for local calls (simple IDENT)
    // Qualified calls (FIELD_EXPR) are handled in lower_field_expr
    if actual_callee.kind() == SyntaxKind::IDENT {
        let callee_name = actual_callee.text().to_string();

        // Skip if callee is a local variable (object with call operator)
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

    Expr::Call { callee, args: args.into_boxed_slice() }
}

/// Lower argument list.
fn lower_arg_list(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Vec<ExprId> {
    // Check for trailing comma before lowering
    if let Some(comma_range) = find_trailing_comma(node) {
        ctx.diagnostics.push(BodyDiagnostic::ExtraCommas { range: comma_range });
    }

    node.children().map(|n| lower_expr_node(ctx, &n)).collect()
}

/// Find the first trailing comma in an ARG_LIST node.
/// Returns the TextRange of the first trailing comma, or None.
fn find_trailing_comma(arg_list: &SyntaxNode) -> Option<syntax::TextRange> {
    use syntax::NodeOrToken;

    // Collect all children_with_tokens and iterate backwards
    let tokens: Vec<_> = arg_list.children_with_tokens().collect();
    let mut iter = tokens.iter().rev().filter(|element| !is_trivia_element(element));

    // First should be R_PAREN
    let r_paren = iter.next()?;
    if !matches!(r_paren, NodeOrToken::Token(t) if t.kind() == SyntaxKind::R_PAREN) {
        return None;
    }

    // Next should be either COMMA (bad) or expression/L_PAREN (good)
    let prev = iter.next()?;
    match prev {
        NodeOrToken::Token(token) if token.kind() == SyntaxKind::COMMA => Some(token.text_range()),
        _ => None,
    }
}

/// Check if an element is trivia (whitespace, newline, comment)
fn is_trivia_element(element: &syntax::NodeOrToken<SyntaxNode, syntax::SyntaxToken>) -> bool {
    matches!(
        element,
        syntax::NodeOrToken::Token(t) if matches!(
            t.kind(),
            SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT
        )
    )
}

/// Extract which arguments have values from an ARG_LIST node.
///
/// Returns a Boolean vector where:
/// - `true` = argument has an expression
/// - `false` = argument is empty (between commas with no value)
///
/// ## Examples
/// - `Method()` → `[]`
/// - `Method(5)` → `[true]`
/// - `Method(, 2)` → `[false, true]`
/// - `Method(5, 2)` → `[true, true]`
/// - `Method(5,)` → `[true, false]`
/// - `Method(,)` → `[false, false]`
fn extract_arg_presence(arg_list: &SyntaxNode) -> Vec<bool> {
    let mut args = Vec::new();
    let mut has_expr = false;

    for child in arg_list.children_with_tokens() {
        match child.kind() {
            SyntaxKind::COMMA => {
                args.push(has_expr);
                has_expr = false;
            }
            SyntaxKind::L_PAREN | SyntaxKind::R_PAREN => {
                // Skip parentheses
            }
            kind if kind.is_trivia() => {
                // Skip whitespace and comments
            }
            _ => {
                // Any other node indicates an expression is present
                has_expr = true;
            }
        }
    }

    // Handle last argument (after last comma or only argument)
    // Only push if we're inside the argument list (has children)
    if arg_list.children().count() > 0 || has_expr {
        args.push(has_expr);
    }

    args
}

/// Lower index expression.
fn lower_index_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    let base =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let index =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    Expr::Index { base, index }
}

/// Lower field expression.
///
/// Handles:
/// - Two-level calls: `Module.Method()` - emits MissedRequiredParameter with module
/// - Three-level calls: `Документы.ПКО.Method()` - emits MissedRequiredParameter with mdo_type/mdo_name
/// - Field access: `obj.field` - no diagnostics
fn lower_field_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    let base =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    // Find field name (IDENT token after DOT)
    let field_token = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| tok.kind() == SyntaxKind::IDENT)
        .last();

    let field_name =
        field_token.as_ref().map(|tok| Name::new(tok.text())).unwrap_or_else(Name::missing);

    // === Detect deprecated attributes/methods (8.3.12) ===
    // Extract object name from base expression (first child)
    // For FIELD_EXPR like "Диаграмма.ПолучитьПалитру()", first child is the base (Диаграмма)
    let object_name_opt = node.children().next().and_then(|base_node| {
        // Try different approaches to extract IDENT text
        // 1. Direct IDENT node
        if base_node.kind() == SyntaxKind::IDENT {
            return Some(base_node.text().to_string());
        }

        // 2. EXPR wrapper with IDENT child
        if base_node.kind() == SyntaxKind::EXPR {
            if let Some(ident_child) = base_node.children().find(|n| n.kind() == SyntaxKind::IDENT)
            {
                return Some(ident_child.text().to_string());
            }
        }

        // 3. Find first IDENT token in descendants
        base_node
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::IDENT)
            .map(|tok| tok.text().to_string())
    });

    // Check if this is actually a method call (has ARG_LIST)
    let arg_list_node = node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST);
    let is_method_call = arg_list_node.is_some();

    // Check for deprecated attributes/methods (8.3.12)
    if let (Some(field_tok), Some(object_name)) = (&field_token, &object_name_opt) {
        use super::diagnostics::is_deprecated_attribute_8312;

        if let Some(kind) =
            is_deprecated_attribute_8312(object_name, field_tok.text(), is_method_call)
        {
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedAttribute8312 {
                name: field_tok.text().to_string(), // Preserve original case
                kind,
                range: field_tok.text_range(),
            });
        }
    }

    // NOTE: External app starting methods are now detected in lower_call_expr()
    // for both global calls (IDENT) and method calls (FIELD_EXPR)

    if arg_list_node.is_some() {
        let method = field_name.to_string();
        tracing::warn!(method = %method, "lower_field_expr: has arg_list, analyzing call");

        // Analyze call structure to determine call type (for diagnostics)
        let call_info = analyze_qualified_call(node, ctx);
        tracing::warn!(has_call_info = call_info.is_some(), "lower_field_expr: call_info result");

        // Collect ExternalRef for module dependency graph
        if let Some(ref info) = &call_info {
            match info {
                QualifiedCallInfo::TwoLevel { module } => {
                    ctx.external_refs.push(ExternalRef::QualifiedCall {
                        receiver: Name::new(module),
                        method: field_name.clone(),
                        range: node.text_range(),
                    });
                }
                QualifiedCallInfo::ThreeLevel { mdo_type, mdo_name } => {
                    if let Some(manager_type) = parse_manager_type(mdo_type) {
                        ctx.external_refs.push(ExternalRef::ManagerAccess {
                            manager_type,
                            object_name: Name::new(mdo_name),
                            method: Some(field_name.clone()),
                            range: node.text_range(),
                        });
                    }
                }
            }
        }

        if let Some(ref info) = call_info {
            match info {
                QualifiedCallInfo::TwoLevel { module } => {
                    // NOTE: MissingCommonModuleMethod diagnostic is now generated via path resolution
                    // in ide-diagnostics instead of during lowering. This provides more accurate
                    // diagnostics using the workspace symbol index from Phase 2.

                    // Emit MissedRequiredParameter diagnostic for qualified calls.
                    let arg_presence =
                        arg_list_node.as_ref().map(extract_arg_presence).unwrap_or_default();

                    ctx.diagnostics.push(BodyDiagnostic::MissedRequiredParameter {
                        callee: method,
                        module: Some(module.clone()),
                        mdo_type: None,
                        mdo_name: None,
                        args: arg_presence,
                        range: node.text_range(),
                    });
                }
                QualifiedCallInfo::ThreeLevel { mdo_type, mdo_name } => {
                    // Three-level call: Документы.ПКО.Method()
                    let arg_presence =
                        arg_list_node.as_ref().map(extract_arg_presence).unwrap_or_default();

                    ctx.diagnostics.push(BodyDiagnostic::MissedRequiredParameter {
                        callee: method,
                        module: None,
                        mdo_type: Some(mdo_type.clone()),
                        mdo_name: Some(mdo_name.clone()),
                        args: arg_presence,
                        range: node.text_range(),
                    });
                }
            }
        }

        // === NEW: Build QualifiedPath for qualified method calls ===
        // Check if this is a qualified call (Module.Method or A.B.Method)
        // by examining the base expression type
        let base_expr = ctx.body.expr(base);
        let qualified_path_opt = match base_expr {
            Expr::Path(module_name) => {
                // Two-level qualified call: Module.Method()
                // Only create QualifiedPath if analyze_qualified_call detected it
                // (to avoid treating local variables as modules)
                if call_info.is_some() {
                    Some(QualifiedName::from_segments([module_name.clone(), field_name.clone()]))
                } else {
                    None
                }
            }
            Expr::QualifiedPath(path) => {
                // Multi-level qualified call: add another segment
                // Example: Documents.PKO.Method() where path = [Documents, PKO]
                let mut segments = path.segments().to_vec();
                segments.push(field_name.clone());
                Some(QualifiedName::from_segments(segments))
            }
            _ => {
                // Not a qualified call - regular method call on an expression
                // Example: GetObject().Method() or variable.Method()
                None
            }
        };

        let args = arg_list_node
            .as_ref()
            .map(|arg_list| lower_arg_list(ctx, arg_list))
            .unwrap_or_default();

        if let Some(qualified_path) = qualified_path_opt {
            // This is a qualified call - create Call with QualifiedPath as callee
            // Example: Module.Method(args) → Call { callee: QualifiedPath([Module, Method]), args }

            // Emit MissingCommonModuleMethod diagnostic for two-level calls (Module.Method)
            // Resolution and export validation will happen in from_hir() handler with ctx.db
            if qualified_path.len() == 2 {
                let module = qualified_path.first().as_str().to_string();
                let method = qualified_path.last().as_str().to_string();

                ctx.diagnostics.push(BodyDiagnostic::MissingCommonModuleMethod {
                    module,
                    method,
                    range: node.text_range(),
                });
            }

            let callee = ctx.alloc_expr(Expr::QualifiedPath(qualified_path), node.text_range());
            Expr::Call { callee, args: args.into_boxed_slice() }
        } else {
            // Regular method call on an expression
            Expr::MethodCall { receiver: base, method: field_name, args: args.into_boxed_slice() }
        }
    } else {
        Expr::Field { base, field: field_name }
    }
}

/// Extract receiver variable name from an expression for CreateQueryInCycle diagnostic.
///
/// Extracts variable name from expressions like:
/// - Запрос -> "Запрос"
/// - Запрос2.info -> "Запрос2.info"
fn extract_receiver_name(ctx: &LoweringCtx, expr_id: ExprId) -> Option<String> {
    let expr = ctx.body.expr(expr_id);
    match expr {
        Expr::Path(name) => Some(name.as_str().to_string()),
        Expr::Field { base, field } => {
            // Build field path: base.field
            let base_name = extract_receiver_name(ctx, *base)?;
            Some(format!("{}.{}", base_name, field.as_str()))
        }
        _ => None,
    }
}

/// Information about a qualified call structure.
enum QualifiedCallInfo {
    /// Two-level call: `Module.Method()`
    TwoLevel { module: String },
    /// Three-level call: `Документы.ПКО.Method()`
    ThreeLevel { mdo_type: String, mdo_name: String },
}

/// Analyze a FIELD_EXPR node to determine the qualified call type.
///
/// Returns:
/// - `Some(TwoLevel)` for `Module.Method()` where Module is not a local variable
/// - `Some(ThreeLevel)` for `MdoType.MdoName.Method()` (e.g., Документы.ПКО.Method)
/// - `None` for local variable calls or field access
fn analyze_qualified_call(node: &SyntaxNode, ctx: &LoweringCtx) -> Option<QualifiedCallInfo> {
    let first_child = node.children().next()?;
    tracing::warn!(
        node_kind = ?node.kind(),
        first_child_kind = ?first_child.kind(),
        first_child_text = %first_child.text(),
        "analyze_qualified_call: entry"
    );

    // Check for three-level call: first child is FIELD_EXPR
    // Structure: FIELD_EXPR > FIELD_EXPR > [IDENT, DOT, IDENT]
    if first_child.kind() == SyntaxKind::FIELD_EXPR {
        // Extract mdo_type and mdo_name from nested FIELD_EXPR
        let idents: Vec<String> = first_child
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .map(|tok| tok.text().to_string())
            .collect();

        tracing::trace!(
            idents = ?idents,
            first_child_kind = ?first_child.kind(),
            "Analyzing potential three-level call"
        );

        if idents.len() == 2 {
            let mdo_type = idents[0].clone();
            let mdo_name = idents[1].clone();

            // Check if mdo_type is a local variable
            let key = mdo_type.to_lowercase();
            if ctx.local_vars.contains_key(&key) || ctx.param_names.contains(&key) {
                return None;
            }

            tracing::debug!(
                mdo_type = %mdo_type,
                mdo_name = %mdo_name,
                "Detected three-level call"
            );
            return Some(QualifiedCallInfo::ThreeLevel { mdo_type, mdo_name });
        }
        return None;
    }

    // Check for two-level call: first child is IDENT or EXPR containing IDENT
    let module_name = if first_child.kind() == SyntaxKind::IDENT {
        Some(first_child.text().to_string())
    } else if first_child.kind() == SyntaxKind::EXPR {
        // Unwrap EXPR if it contains a single IDENT
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

    // Check if module name is a local variable
    let key = module.to_lowercase();
    if ctx.local_vars.contains_key(&key) || ctx.param_names.contains(&key) {
        tracing::warn!(module = %module, "analyze_qualified_call: is local variable, returning None");
        return None;
    }

    tracing::warn!(module = %module, "analyze_qualified_call: returning TwoLevel");
    Some(QualifiedCallInfo::TwoLevel { module })
}

/// Lower new expression.
fn lower_new_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    // Type name (IDENT after NEW keyword)
    let type_name = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)
        .map(|tok| Name::new(tok.text()));

    // Check for file system access (security diagnostic)
    if let Some(ref name) = type_name {
        if is_file_system_type(name.as_str()) {
            // Emit FileSystemAccess diagnostic
            // Range is the entire NEW_EXPR node (matches Java behavior)
            ctx.diagnostics.push(BodyDiagnostic::FileSystemAccess { range: node.text_range() });
        }
    }

    // Arguments
    let args = node
        .children()
        .find(|n| n.kind() == SyntaxKind::ARG_LIST)
        .map(|arg_list| lower_arg_list(ctx, &arg_list))
        .unwrap_or_default();

    Expr::New { type_name, args: args.into_boxed_slice() }
}

/// Check if two expressions are semantically equal (case-insensitive for names).
/// Used for detecting self-assignment patterns like `a = a` or `obj.field = obj.field`.
pub(crate) fn exprs_are_equal(body: &Body, lhs: ExprId, rhs: ExprId) -> bool {
    match (body.expr(lhs), body.expr(rhs)) {
        // Missing expressions are equal (used for global function calls like Mass())
        (Expr::Missing, Expr::Missing) => true,

        // Simple variable: A = a (case-insensitive)
        (Expr::Path(name1), Expr::Path(name2)) => name1.eq_ignore_case(name2),

        // Field access: obj.field = obj.field
        (Expr::Field { base: b1, field: f1 }, Expr::Field { base: b2, field: f2 }) => {
            f1.eq_ignore_case(f2) && exprs_are_equal(body, *b1, *b2)
        }

        // Index access: arr[i] = arr[i]
        (Expr::Index { base: b1, index: i1 }, Expr::Index { base: b2, index: i2 }) => {
            exprs_are_equal(body, *b1, *b2) && exprs_are_equal(body, *i1, *i2)
        }

        // Method call: obj.method() = obj.method()
        // Arguments are ignored - obj.method(1) = obj.method(2) for our purposes
        (
            Expr::MethodCall { receiver: r1, method: m1, .. },
            Expr::MethodCall { receiver: r2, method: m2, .. },
        ) => m1.eq_ignore_case(m2) && exprs_are_equal(body, *r1, *r2),

        // Function call: func() = func()
        // Arguments are ignored - func(1) = func(2) for our purposes
        (Expr::Call { callee: c1, .. }, Expr::Call { callee: c2, .. }) => {
            exprs_are_equal(body, *c1, *c2)
        }

        // Different expression types or complex expressions - not equal
        _ => false,
    }
}

/// Check if method name is an external application starting method.
///
/// These methods allow starting external applications/executing system commands:
/// - КомандаСистемы / System
/// - ЗапуститьСистему / RunSystem
/// - ЗапуститьПриложение / RunApp
/// - НачатьЗапускПриложения / BeginRunningApplication
/// - ЗапуститьПриложениеАсинх / RunAppAsync
/// - ЗапуститьПрограмму
/// - ОткрытьПроводник
/// - ОткрытьФайл
fn is_external_app_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "командасистемы"
            | "system"
            | "запуститьсистему"
            | "runsystem"
            | "запуститьприложение"
            | "runapp"
            | "начатьзапускприложения"
            | "beginrunningapplication"
            | "запуститьприложениеасинх"
            | "runappasync"
            | "запуститьпрограмму"
            | "открытьпроводник"
            | "открытьфайл"
    )
}

/// Check if type name indicates file system access (NEW expression).
///
/// Constructor types that indicate file system access:
/// - File/Файл - file operations
/// - xBase - database file access
/// - HTMLWriter/ЗаписьHTML, HTMLReader/ЧтениеHTML - HTML file operations
/// - FastInfosetWriter/Reader - Fast Infoset file operations
/// - XSLTransform - XSLT file processing
/// - ZipFileWriter/Reader - archive operations
/// - TextWriter/Reader - text file operations
/// - TextExtraction - text extraction from files
/// - BinaryData - binary file operations
/// - FileStream - file stream operations
/// - FileStreamsManager - file stream management
/// - DataWriter/Reader - data file operations
fn is_file_system_type(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "file"
            | "файл"
            | "xbase"
            | "htmlwriter"
            | "записьhtml"
            | "htmlreader"
            | "чтениеhtml"
            | "fastinfosetreader"
            | "чтениеfastinfoset"
            | "fastinfosetwriter"
            | "записьfastinfoset"
            | "xsltransform"
            | "преобразованиеxsl"
            | "zipfilewriter"
            | "записьzipфайла"
            | "zipfilereader"
            | "чтениеzipфайла"
            | "textreader"
            | "чтениетекста"
            | "textwriter"
            | "записьтекста"
            | "textextraction"
            | "извлечениетекста"
            | "binarydata"
            | "двоичныеданные"
            | "filestream"
            | "файловыйпоток"
            | "filestreamsmanager"
            | "менеджерфайловыхпотоков"
            | "datawriter"
            | "записьданных"
            | "datareader"
            | "чтениеданных"
    )
}

/// Check if method name indicates file system access (global method).
///
/// Global methods that indicate file system access:
/// - File operations: ЗначениеВФайл, КопироватьФайл, ПереместитьФайл, etc.
/// - Directory operations: СоздатьКаталог, КаталогВременныхФайлов, etc.
/// - Extension operations: УстановитьРасширениеРаботыСФайлами, etc.
/// - Async operations: КопироватьФайлАсинх, СоздатьКаталогАсинх, etc.
fn is_file_system_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        // File operations
        "значениевфайл"
            | "valuetofile"
            | "копироватьфайл"
            | "filecopy"
            | "объединитьфайлы"
            | "mergefiles"
            | "переместитьфайл"
            | "movefile"
            | "разделитьфайл"
            | "splitfile"
            | "создатькаталог"
            | "createdirectory"
            | "удалитьфайлы"
            | "deletefiles"
            // Directory operations
            | "каталогпрограммы"
            | "bindir"
            | "каталогвременныхфайлов"
            | "tempfilesdir"
            | "каталогдокументов"
            | "documentsdir"
            | "рабочийкаталогданныхпользователя"
            | "userdataworkdir"
            // Extension operations
            | "начатьподключениерасширенияработысфайлами"
            | "beginattachingfilesystemextension"
            | "начатьустановкурасширенияработысфайлами"
            | "begininstallfilesystemextension"
            | "установитьрасширениеработысфайлами"
            | "installfilesystemextension"
            | "установитьрасширениеработысфайламиасинх"
            | "installfilesystemextensionasync"
            | "подключитьрасширениеработысфайламиасинх"
            | "attachfilesystemextensionasync"
            // Async directory operations
            | "каталогвременныхфайловасинх"
            | "tempfilesdirasync"
            | "каталогдокументовасинх"
            | "documentsdirasync"
            | "рабочийкаталогданныхпользователяасинч"
            | "userdataworkdirasync"
            | "начатьполучениякаталогавременныхфайлов"
            | "begingettingtempfilesdir"
            | "начатьполучениякаталогадокументов"
            | "begingettingdocumentsdir"
            | "начатьполучениярабочегокаталогаданныхпользователя"
            | "begingettinguserdataworkdir"
            // Async file operations
            | "копироватьфайласинх"
            | "copyfileasync"
            | "найтифайлыасинч"
            | "findfilesasync"
            | "начатькопированияфайла"
            | "begincopyingfile"
            | "начатьперемещенияфайла"
            | "beginmovingfile"
            | "начатьпоискфайлов"
            | "beginfindingfiles"
            | "начатьсозданиядвоичныхданныхизфайла"
            | "begincreatebinarydatafromfile"
            | "начатьсозданиякаталога"
            | "begincreatingdirectory"
            | "начатьудаленияфайлов"
            | "begindeletingfiles"
            | "переместитьфайласинч"
            | "movefileasync"
            | "создатьдвоичныеданныеизфайласинч"
            | "createbinarydatafromfileasync"
            | "создатькаталогасинч"
            | "createdirectoryasync"
            | "удалитьфайлыасинч"
            | "deletefilesasync"
    )
}

/// Check if method name is FormDataToValue.
///
/// FormDataToValue / ДанныеФормыВЗначение method converts form data to value.
/// Using it in context methods is bad practice - creates unnecessary form dependency.
/// Allowed in БезКонтекста methods (@НаСервереБезКонтекста, @НаКлиентеНаСервереБезКонтекста).
fn is_form_data_to_value_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "данныеформывзначение" | "formdatatovalue")
}

/// Check if method name is GetForm.
///
/// GetForm / ПолучитьФорму is a deprecated method that returns managed form objects.
/// Should be replaced with OpenForm / ОткрытьФорму.
fn is_get_form_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "получитьформу" | "getform")
}

/// Check if method name is StrTemplate.
///
/// StrTemplate / СтрШаблон is a string formatting method that requires validation.
fn is_str_template_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "стршаблон" | "strtemplate")
}

/// Check if StrTemplate usage is incorrect.
///
/// Validates:
/// - Parameter count matches template placeholders (%1-%10)
/// - No invalid placeholders (%0, %11+)
/// - All required parameters present
fn is_wrong_str_template(template_string: &str, used_params_count: usize) -> bool {
    use once_cell::sync::Lazy;
    use regex::Regex;

    static TWO_PERCENT_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new("%%").unwrap());

    let is_wrong_call = compare_template_and_params(template_string, used_params_count);
    if !is_wrong_call {
        return false;
    }

    // Remove %% escapes and check again
    let str = TWO_PERCENT_PATTERN.replace_all(template_string, "");
    compare_template_and_params(&str, used_params_count)
}

/// Compare template string and parameter count.
#[allow(clippy::nonminimal_bool)]
fn compare_template_and_params(template_string: &str, used_params_count: usize) -> bool {
    use once_cell::sync::Lazy;
    use regex::Regex;

    // These patterns are used across multiple functions, so we define them at the module level
    // to avoid recompilation on every call.
    static PARAMS_PATTERN_INNER: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"%(?:(10|[1-9])|\((10|[1-9])\))").unwrap());

    static WRONG_NUMBERS_PATTERN_INNER: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"%(?:(1[1-9]\d*|[2-9]\d+|0|10\d+)|\((1[1-9]\d*|[2-9]\d+|0|10\d+)\))").unwrap()
    });

    let have_params = used_params_count > 0;
    let matches = PARAMS_PATTERN_INNER.is_match(template_string);

    // Check conditions (keep logic as-is for clarity, matches Java implementation):
    // 1. Template has parameters but no arguments provided
    // 2. Template has no parameters but arguments provided
    // 3. Template has parameters and various/mismatched params
    // 4. Wrong parameter numbers (0, 11+)
    (matches && !have_params)
        || (!matches && have_params)
        || (matches && various_params(used_params_count, template_string))
        || WRONG_NUMBERS_PATTERN_INNER.is_match(template_string)
}

/// Check if template has mismatched parameter indices.
fn various_params(used_params_count: usize, template_string: &str) -> bool {
    use once_cell::sync::Lazy;
    use regex::Regex;
    use std::collections::HashSet;

    static PARAMS_PATTERN: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"%(?:(10|[1-9])|\((10|[1-9])\))").unwrap());

    let mut template_params = HashSet::new();
    let bytes = template_string.as_bytes();

    for cap in PARAMS_PATTERN.captures_iter(template_string) {
        let match_obj = cap.get(0).unwrap();
        let pos = match_obj.start();

        // Skip if this is part of %% escape sequence
        if pos > 0 && bytes.get(pos - 1) == Some(&b'%') {
            continue;
        }

        // Group 1: %N format, Group 2: %(N) format
        let group = cap.get(1).or_else(|| cap.get(2));
        if let Some(g) = group {
            if let Ok(index) = g.as_str().parse::<usize>() {
                if index > used_params_count {
                    return true;
                }
                template_params.insert(index);
            }
        }
    }

    // Check if all parameters from 1..used_params_count are present
    for i in 1..=used_params_count {
        if !template_params.contains(&i) {
            return true;
        }
    }

    false
}

/// Extract string content from AST node.
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

/// Parse manager type from MDO type string.
///
/// Converts Russian/English MDO type names to ManagerType enum:
/// - Документы / Documents -> Documents
/// - Справочники / Catalogs -> Catalogs
/// - Обработки / DataProcessors -> DataProcessors
/// - Отчёты / Reports -> Reports
/// - РегистрыСведений / InformationRegisters -> InformationRegisters
/// - РегистрыНакопления / AccumulationRegisters -> AccumulationRegisters
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
