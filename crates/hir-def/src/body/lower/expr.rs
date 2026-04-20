//! Expression lowering.
//!
//! This module handles lowering of BSL expressions from AST to HIR.

use syntax::{SyntaxKind, SyntaxNode};

use crate::body::{
    Body, BodyDiagnostic, ExternalRef, MagicNumberContext, ManagerType, RedundantAccessKind,
};
use crate::hir::{BinaryOp, Expr, ExprIdx, Literal, UnaryOp};
use crate::{Name, QualifiedName};

use super::diagnostics::{is_deprecated_method, is_followed_by_loop_exit};
use super::utils::{extract_string_content, looks_like_sdbl};
use super::LoweringCtx;

/// Lower an expression node (handles EXPR wrapper).
pub(crate) fn lower_expr_node(ctx: &mut LoweringCtx, node: &SyntaxNode) -> ExprIdx {
    // Handle EXPR wrapper - unwrap to get actual expression
    let actual_node = if node.kind() == SyntaxKind::EXPR {
        node.children().next().unwrap_or_else(|| node.clone())
    } else {
        node.clone()
    };

    lower_expr(ctx, &actual_node)
}

/// Lower an expression.
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

            // Check for deprecated ЭтаФорма/ThisForm usage
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

    // Associate SDBL with ExprId by matching TextRange
    if let Some(idx) =
        ctx.pending_sdbl.iter().position(|(literal_range, _)| *literal_range == range)
    {
        let (_literal_range, query_info) = ctx.pending_sdbl.remove(idx);
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

            // Emit MagicNumber diagnostic candidate
            // Actual filtering by authorizedNumbers and context happens in from_hir()
            let context = determine_magic_number_context(&token);
            ctx.emit(BodyDiagnostic::MagicNumber {
                value: text.clone(),
                range: token.text_range(),
                context,
            });

            Literal::Number(value)
        }
        SyntaxKind::STRING | SyntaxKind::STRING_START => {
            // Extract full string content (handles multiline with |)
            let value = extract_string_content(node).unwrap_or_default();

            // Check if this is SDBL query
            if looks_like_sdbl(&value) {
                // Re-extract with quote corrections for accurate position mapping
                let (sdbl_text, quote_corrections) = syntax::extract_sdbl_with_corrections(node)
                    .unwrap_or_else(|| (value.clone(), vec![]));

                let sdbl_ast = parser::parse_sdbl_with_shared_cache(&sdbl_text);

                // Store query info regardless of parse errors
                // - Valid queries: query_ast = Some(ast) with no errors
                // - Invalid queries: query_ast = Some(ast) with errors
                // QueryParseError diagnostic uses is_valid() to detect parse errors
                let query_info = syntax::SdblQueryInfo::new(
                    node.text_range(),
                    sdbl_text,
                    Some(sdbl_ast),
                    quote_corrections,
                );

                ctx.pending_sdbl.push((node.text_range(), query_info));
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

    // Track privileged mode method name for later diagnostic check
    let mut privileged_mode_name: Option<String> = None;

    // Track if this is a SafeMode() query call for UnsafeSafeModeMethodCall
    let mut is_safe_mode_query_call = false;

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

        // Check for SafeMode() query (the getter)
        use super::diagnostics::is_safe_mode_query;
        if is_safe_mode_query(&name) {
            is_safe_mode_query_call = true;
        }

        // Check for privileged mode methods
        use super::diagnostics::is_set_privileged_mode;
        if is_set_privileged_mode(&name) {
            privileged_mode_name = Some(name.clone());
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

        use super::diagnostics::is_temp_files_dir;

        if is_temp_files_dir(&name) {
            ctx.diagnostics.push(BodyDiagnostic::TempFilesDir {
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

        // Check for OSUsers method (security risk)
        if is_os_users_method(&name) {
            // Emit OSUsersMethod diagnostic
            // Range is just the method name (IDENT token), not the whole call
            ctx.diagnostics
                .push(BodyDiagnostic::OSUsersMethod { range: actual_callee.text_range() });
        }

        // Check for Число()/Number() inside try block (TryNumber diagnostic)
        use super::diagnostics::check_try_number_call;
        if let Some(range) = check_try_number_call(node) {
            ctx.diagnostics.push(BodyDiagnostic::TryNumber { range });
        }

        // Check for WriteLogEvent / ЗаписьЖурналаРегистрации
        if is_write_log_event_method(&name) {
            check_write_log_event_call(ctx, node);
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

        // Check for ProceedWithCall/ПродолжитьВызов outside &Вместо method
        if is_proceed_with_call_method(&name_lower) && !ctx.is_instead_method {
            ctx.diagnostics.push(BodyDiagnostic::WrongUseFunctionProceedWithCall {
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
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedMethod {
                name: name.clone(),
                range: node.text_range(),
            });
        }

        // Emit DeprecatedMethodCall candidate for local calls
        // Skip if the name is a local variable or parameter (not a method call)
        let name_lower = name.to_lowercase();
        if !ctx.local_vars.contains_key(&name_lower) && !ctx.param_names.contains(&name_lower) {
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedMethodCall {
                callee: name,
                module: None,
                range: actual_callee.text_range(),
            });
        }

        // Check for modal window methods (UsingModalWindows diagnostic)
        use super::diagnostics::get_modal_method_replacement;
        if let Some(replacement) = get_modal_method_replacement(&name_lower) {
            ctx.diagnostics.push(BodyDiagnostic::UsingModalWindows {
                method_name: actual_callee.text().to_string(),
                replacement: replacement.to_string(),
                range: node.text_range(),
            });
        }

        // Check for synchronous call methods (UsingSynchronousCalls diagnostic)
        // Skip if method has server annotation (&НаСервере or &НаСервереБезКонтекста)
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

                // Emit DeprecatedMethodCall candidate for qualified calls
                ctx.diagnostics.push(BodyDiagnostic::DeprecatedMethodCall {
                    callee: method_name_str.to_string(),
                    module: Some(module_name.to_string()),
                    range: idents[1].text_range(),
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

        // Check for UsingExternalCodeTools diagnostic
        // Pattern: ExternalCodeTools.Create() or ExternalCodeTools.Connect()
        // where ExternalCodeTools is: ВнешниеОбработки, ExternalDataProcessors,
        // ВнешниеОтчеты, ExternalReports, РасширенияКонфигурации, ConfigurationExtensions
        check_using_external_code_tools(ctx, &actual_callee, &idents, node);
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

    // Check for SetPrivilegedMode diagnostic after args are lowered
    if privileged_mode_name.is_some() {
        // Safe only if argument is literal False (disabling privileged mode)
        let is_safe = if !args.is_empty() {
            matches!(&ctx.body.exprs[args[0]], Expr::Literal(Literal::Bool(false)))
        } else {
            false // No argument - not safe
        };

        if !is_safe {
            // Find the method name token for the range
            let method_token = callee_node
                .descendants_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| tok.kind() == SyntaxKind::IDENT);

            if let Some(token) = method_token {
                ctx.diagnostics
                    .push(BodyDiagnostic::SetPrivilegedModeCall { range: token.text_range() });
            }
        }
    }

    // Check for UnsafeSafeModeMethodCall: SafeMode() used without explicit comparison
    if is_safe_mode_query_call && is_unsafe_safe_mode_context(node) {
        ctx.emit(BodyDiagnostic::UnsafeSafeModeMethodCall { range: actual_callee.text_range() });
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
                let receiver = match ctx.body.expr_idx(callee) {
                    Expr::Field { base, .. } => Some(*base),
                    Expr::MethodCall { receiver, .. } => Some(*receiver),
                    _ => None,
                };

                if let Some(receiver_id) = receiver {
                    if let Some(collection_text) = ctx.matches_foreach_collection(receiver_id) {
                        // Skip if Delete is followed by Break or Return - this is a safe pattern
                        // Example: Delete(item); Break; - iteration stops immediately
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

    // Check for SelfInsertion: Collection.Insert/Add(Collection)
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

    // Check for UsingFindElementByString: FindByDescription/FindByCode/FindByNumber with literal argument
    if actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        if let Some(method_token) = actual_callee
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .last()
        {
            use super::diagnostics::is_find_element_method;
            if is_find_element_method(method_token.text()) {
                // Check if first argument is literal (string or number) or no arguments
                let has_literal_first_arg = check_find_element_first_arg(&args, ctx);
                if has_literal_first_arg {
                    // Range covers method name token and argument list
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

    // Check for UnsafeFindByCode: Manager.Object.FindByCode()
    if actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        if let Some(method_token) = actual_callee
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .last()
        {
            use super::diagnostics::is_find_by_code_method;
            if is_find_by_code_method(method_token.text()) {
                // Receiver should be FIELD_EXPR: Manager.Object
                if let Some(receiver) = actual_callee.first_child() {
                    if receiver.kind() == SyntaxKind::FIELD_EXPR {
                        // Extract object name (last IDENT in receiver)
                        let object_name = receiver
                            .children_with_tokens()
                            .filter_map(|e| e.into_token())
                            .filter(|t| t.kind() == SyntaxKind::IDENT)
                            .last()
                            .map(|t| t.text().to_string());

                        // Extract manager name (first child IDENT)
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

    // Emit MissedRequiredParameter diagnostic for local calls (simple IDENT).
    // Qualified calls are handled by `maybe_lower_as_qualified_call` below.
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

    // For qualified calls (Module.Method, MdoType.MdoName.Method) promote HIR
    // to `Expr::Call { callee: QualifiedPath }` and emit qualified-call diagnostics.
    if actual_callee.kind() == SyntaxKind::FIELD_EXPR {
        if let Some(replacement) =
            maybe_lower_as_qualified_call(ctx, node, &actual_callee, arg_list_node.as_ref(), &args)
        {
            return replacement;
        }
    }

    Expr::Call { callee, args: args.into_boxed_slice() }
}

/// Rewrite `a.b()` / `a.b.c()` into `Expr::Call { callee: QualifiedPath, args }`
/// and emit the diagnostics that depend on the call shape.
///
/// - Returns `Some(Expr::Call{QualifiedPath,args})` for CommonModule calls and
///   manager-object calls where `analyze_qualified_call` recognises the pattern.
/// - For `ЭтотОбъект.Method()` the diagnostic is emitted as a *local* call
///   (`module = None`) and `None` is returned so the caller keeps the original
///   `Expr::Call { callee: Expr::Field, args }` shape.
/// - Returns `None` for anything else (e.g. `obj.Method()` on a local variable),
///   leaving HIR shape untouched.
fn maybe_lower_as_qualified_call(
    ctx: &mut LoweringCtx,
    call_node: &SyntaxNode,
    field_expr_node: &SyntaxNode,
    arg_list_node: Option<&SyntaxNode>,
    args: &[ExprIdx],
) -> Option<Expr> {
    let call_info = analyze_qualified_call(field_expr_node, ctx)?;

    // Last IDENT inside the FIELD_EXPR is the method name.
    let field_token = field_expr_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| tok.kind() == SyntaxKind::IDENT)
        .last()?;
    let field_name = Name::new(field_token.text());

    let arg_presence = arg_list_node.map(extract_arg_presence).unwrap_or_default();

    match call_info {
        QualifiedCallInfo::TwoLevel { module } => {
            let is_this_object = {
                let lower = module.to_lowercase();
                lower == "этотобъект" || lower == "thisobject"
            };

            if is_this_object {
                // ThisObject.Method() is semantically a local call:
                // resolve the method against the current module, not a CommonModule.
                // RedundantAccessToObject::ThisObject is already emitted in lower_field_expr.
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

            ctx.diagnostics.push(BodyDiagnostic::RedundantAccessToObject {
                kind: RedundantAccessKind::TwoLevel { module: module.clone() },
                range: call_node.text_range(),
            });

            ctx.diagnostics.push(BodyDiagnostic::MissedRequiredParameter {
                callee: field_name.as_str().to_string(),
                module: Some(module.clone()),
                mdo_type: None,
                mdo_name: None,
                args: arg_presence,
                range: call_node.text_range(),
            });

            ctx.diagnostics.push(BodyDiagnostic::MissingCommonModuleMethod {
                module: module.clone(),
                method: field_name.as_str().to_string(),
                range: call_node.text_range(),
            });

            let qualified_path =
                QualifiedName::from_segments([Name::new(&module), field_name.clone()]);
            let new_callee = ctx
                .alloc_expr(Expr::QualifiedPath(Box::new(qualified_path)), call_node.text_range());

            Some(Expr::Call { callee: new_callee, args: args.to_vec().into_boxed_slice() })
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

            // Module dependency graph: record Manager.Object.Method access.
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

/// Lower argument list.
///
/// Handles empty arguments (e.g., `Method(a,,b)`) by creating `Expr::Missing` for empty positions.
/// This preserves the argument count for diagnostics like NumberOfValuesInStructureConstructor.
fn lower_arg_list(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Vec<ExprIdx> {
    // Check for trailing comma before lowering
    if let Some(comma_range) = find_trailing_comma(node) {
        ctx.diagnostics.push(BodyDiagnostic::ExtraCommas { range: comma_range });
    }

    let mut args = Vec::new();
    let mut current_expr: Option<ExprIdx> = None;
    let mut has_any_content = false;

    for child in node.children_with_tokens() {
        match child.kind() {
            SyntaxKind::COMMA => {
                // Push current argument (or Missing if empty)
                args.push(current_expr.unwrap_or_else(|| ctx.missing_expr()));
                current_expr = None;
            }
            SyntaxKind::L_PAREN | SyntaxKind::R_PAREN => {
                // Skip parentheses
            }
            kind if kind.is_trivia() => {
                // Skip whitespace and comments
            }
            _ => {
                // Expression node - lower it
                if let Some(expr_node) = child.as_node() {
                    current_expr = Some(lower_expr_node(ctx, expr_node));
                    has_any_content = true;
                }
            }
        }
    }

    // Handle last argument (after last comma or only argument)
    if has_any_content || !args.is_empty() {
        args.push(current_expr.unwrap_or_else(|| ctx.missing_expr()));
    }

    args
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
/// FIELD_EXPR represents plain property access here (no ARG_LIST — that's a
/// sibling under CALL_EXPR and is handled by `lower_call_expr` together with
/// `maybe_lower_as_qualified_call`). This function is therefore responsible
/// for field-access concerns only:
/// - `DeprecatedAttribute8312` on field access (`is_method_call = false`)
/// - `RedundantAccessToObject::ThisObject` for `ЭтотОбъект.Field`
/// - returning `Expr::Field { base, field }`
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

    // FIELD_EXPR as a call callee is handled in `lower_call_expr`, which emits the
    // method-call variants. Here we only see plain field access, so the
    // `is_method_call` flag passed to `is_deprecated_attribute_8312` is always `false`.
    if let (Some(field_tok), Some(object_name)) = (&field_token, &object_name_opt) {
        use super::diagnostics::is_deprecated_attribute_8312;

        if let Some(kind) = is_deprecated_attribute_8312(object_name, field_tok.text(), false) {
            ctx.diagnostics.push(BodyDiagnostic::DeprecatedAttribute8312 {
                name: field_tok.text().to_string(), // Preserve original case
                kind,
                range: field_tok.text_range(),
            });
        }
    }

    // NOTE: External app starting methods are now detected in lower_call_expr()
    // for both global calls (IDENT) and method calls (FIELD_EXPR)

    // === Emit candidates for RedundantAccessToObject ===
    // ThisObject pattern: ЭтотОбъект.Field or ThisObject.Field
    // This check applies to both field access and method calls
    // Only check when the DIRECT base is an identifier (not a chained FIELD_EXPR),
    // to avoid emitting duplicate diagnostics for chains like ЭтотОбъект.A.B.C
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

/// Extract receiver variable name from an expression for CreateQueryInCycle diagnostic.
///
/// Extracts variable name from expressions like:
/// - Запрос -> "Запрос"
/// - Запрос2.info -> "Запрос2.info"
fn extract_receiver_name(ctx: &LoweringCtx, expr_id: ExprIdx) -> Option<String> {
    let expr = ctx.body.expr_idx(expr_id);
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

    // Check for three-level call: first child is FIELD_EXPR.
    // The inner FIELD_EXPR must be exactly `IDENT.IDENT`, otherwise this is a
    // chained call such as `func().field.field2` — NOT a manager access, so we
    // must not classify it as ThreeLevel.
    if first_child.kind() == SyntaxKind::FIELD_EXPR {
        // Direct base of the inner FIELD_EXPR must be a plain identifier.
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
            // CALL_EXPR / nested FIELD_EXPR / anything else — not a manager access.
            _ => return None,
        };

        // Field name is the last IDENT token at this level.
        let mdo_name = first_child
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .last()
            .map(|tok| tok.text().to_string())?;

        // Check if mdo_type is shadowed by a local variable.
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
            ctx.diagnostics.push(BodyDiagnostic::FileSystemAccess { range: node.text_range() });
        }
    }

    // Check for style element constructors (Цвет/Color, Шрифт/Font, Рамка/Border)
    // Two syntax forms: Новый Цвет(...) and Новый("Цвет", ...)
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

    // Check for SystemInformation constructor (СистемнаяИнформация/SystemInfo)
    // Two syntax forms: Новый СистемнаяИнформация and Новый("СистемнаяИнформация")
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

    // Check for Unix-unavailable objects (COMObject, Mail) without platform guard
    // Two syntax forms: Новый COMОбъект(...) and Новый("COMОбъект", ...)
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
pub(crate) fn exprs_are_equal(body: &Body, lhs: ExprIdx, rhs: ExprIdx) -> bool {
    match (body.expr_idx(lhs), body.expr_idx(rhs)) {
        // Missing expressions are equal (used for global function calls like Mass())
        (Expr::Missing, Expr::Missing) => true,

        // Simple variable: A = a (case-insensitive)
        (Expr::Path(name1), Expr::Path(name2)) => name1.eq_ignore_case(name2),

        // Qualified path: Module.Method = module.method (case-insensitive, segment by segment)
        (Expr::QualifiedPath(p1), Expr::QualifiedPath(p2)) => {
            let s1 = p1.segments();
            let s2 = p2.segments();
            s1.len() == s2.len() && s1.iter().zip(s2.iter()).all(|(a, b)| a.eq_ignore_case(b))
        }

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

/// Check if method name is OSUsers (security risk).
///
/// ПользователиОС() / OSUsers() method returns information about operating system users.
/// This creates security vulnerabilities:
/// - Pass-the-hash attack vectors
/// - Information disclosure
/// - May violate security policies
fn is_os_users_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "пользователиос" | "osusers")
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

/// Check if type name is a style element (Цвет/Color, Шрифт/Font, Рамка/Border).
fn is_style_element_type(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "цвет" | "color" | "шрифт" | "font" | "рамка" | "border")
}

/// Check if type name is SystemInformation (СистемнаяИнформация/SystemInfo).
fn is_system_information_type(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "системнаяинформация" | "systeminfo")
}

/// Check if type name is an object not available on Unix (COMObject, Mail).
/// These are Windows-only objects and should be guarded by platform checks.
fn is_unix_unavailable_type(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "comобъект" | "comobject" | "почта" | "mail")
}

/// Check if method name is WriteLogEvent / ЗаписьЖурналаРегистрации.
fn is_write_log_event_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "записьжурналарегистрации" || lower == "writelogevent"
}

/// Check WriteLogEvent call and emit diagnostic with validation info.
fn check_write_log_event_call(ctx: &mut LoweringCtx, node: &SyntaxNode) {
    let arg_list = match node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST) {
        Some(al) => al,
        None => return,
    };

    // Parse arguments properly, handling empty positions (consecutive commas)
    let args = collect_arguments(&arg_list);
    let arg_count = args.len();

    // Check if 2nd param (log level, index 1) is empty
    let log_level_empty = args.get(1).map(|a| a.is_none()).unwrap_or(true);

    // Check if 5th param (comment, index 4) is empty
    let comment_empty = args.get(4).map(|a| a.is_none()).unwrap_or(true);

    // Check if log level contains Error value (УровеньЖурналаРегистрации.Ошибка / EventLogLevel.Error)
    let has_error_log_level =
        args.get(1).and_then(|a| a.as_ref()).map(has_error_log_level_value).unwrap_or(false);

    // Check if comment contains DetailErrorDescription(ErrorInfo())
    let has_detail_error_description =
        args.get(4).and_then(|a| a.as_ref()).map(has_detail_error_description).unwrap_or(false);

    // If direct check failed and we're in except, try resolving variable assignment
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

/// Collect arguments from ARG_LIST, handling empty positions (consecutive commas).
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

    // Don't forget the last argument after the final comma (or only argument without commas)
    if current_arg.is_some() || has_content {
        args.push(current_arg);
    }

    args
}

/// Check if argument contains Error log level value.
///
/// Two-phase heuristic:
/// 1. If it's an EventLogLevel enum reference, check for Error variant
/// 2. For non-literal expressions (variables, function calls) → assume OK (return true)
fn has_error_log_level_value(arg: &SyntaxNode) -> bool {
    let text = arg.text().to_string().to_lowercase();
    if text.contains("уровеньжурналарегистрации") || text.contains("eventloglevel")
    {
        return text.contains("ошибка") || text.contains("error");
    }
    true
}

/// Check if argument contains DetailErrorDescription(ErrorInfo()).
fn has_detail_error_description(arg: &SyntaxNode) -> bool {
    let text = arg.text().to_string().to_lowercase();
    (text.contains("подробноепредставлениеошибки") || text.contains("detailerrordescription"))
        && (text.contains("информацияобошибке") || text.contains("errorinfo"))
}

/// When the 5th arg is a variable, search the enclosing EXCEPT_CLAUSE for
/// assignments to that variable. Check if the assignment RHS contains
/// ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()).
///
/// Returns:
/// - `Some(true)` — found assignment with DetailErrorDescription, or no assignment in except
/// - `Some(false)` — found assignment WITHOUT DetailErrorDescription
/// - `None` — not a simple variable or no except context
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

    // No assignment in except block → assume OK
    Some(true)
}

/// Extract type name from first string argument of Новый("ТипОбъекта", ...).
fn extract_type_name_from_first_arg(node: &SyntaxNode) -> Option<String> {
    let arg_list = node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST)?;
    let first_arg = arg_list.children().next()?;

    // Look for STRING token in the first argument
    let string_token = first_arg
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::STRING)?;

    let text = string_token.text();
    // Remove quotes from string literal
    if text.len() >= 2 && (text.starts_with('"') || text.starts_with('\'')) {
        Some(text[1..text.len() - 1].to_string())
    } else {
        None
    }
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

/// Check if method name is ProceedWithCall / ПродолжитьВызов.
///
/// This function can only be called inside extension methods with &Вместо annotation.
/// Calling it from methods with &До, &После or without extension annotation causes runtime error.
fn is_proceed_with_call_method(name_lower: &str) -> bool {
    matches!(name_lower, "продолжитьвызов" | "proceedwithcall")
}

/// Check for UsingExternalCodeTools diagnostic.
///
/// Detects calls to external code execution mechanisms:
/// - ВнешниеОбработки / ExternalDataProcessors
/// - ВнешниеОтчеты / ExternalReports
/// - РасширенияКонфигурации / ConfigurationExtensions
///
/// When combined with dangerous methods:
/// - Создать / Create
/// - Подключить / Connect
///
/// Detection logic:
/// 1. For simple two-level calls (idents.len() == 2):
///    First ident must be external code tools, second must be dangerous method
/// 2. For chained calls (e.g., ExternalReports.Connect().Create()):
///    We check if any methodCall within the context calls a dangerous method
///    on an external code tools object
///
/// Exclusions:
/// - Qualified access like `Справочники.ВнешниеОбработки` (external code tools not at root)
/// - Variable access like `Обработка.ExternalReports` (not direct global access)
fn check_using_external_code_tools(
    ctx: &mut LoweringCtx,
    _actual_callee: &SyntaxNode,
    idents: &[syntax::SyntaxToken],
    call_node: &SyntaxNode,
) {
    use super::diagnostics::{is_external_code_tools_method, is_external_code_tools_name};

    // Only check two-level calls: ExternalCodeTools.Method()
    // For chained calls like ExternalReports.Connect().Create(), the inner call
    // will be processed recursively during lowering, so we don't need special handling.
    if idents.len() != 2 {
        return;
    }

    let receiver_name = idents[0].text();
    let method_name = idents[1].text();

    // Check if receiver is an external code tools class AND not a local variable
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
    let is_wrong_call = compare_template_and_params(template_string, used_params_count);
    if !is_wrong_call {
        return false;
    }

    // Remove %% escapes and check again
    let cleaned = remove_double_percent(template_string);
    compare_template_and_params(&cleaned, used_params_count)
}

/// Remove %% escape sequences from template string.
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

/// Parse a placeholder at position, returns (number, length) or None.
/// Handles both %N and %(N) formats where N is a number.
fn parse_placeholder(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    if pos >= bytes.len() || bytes[pos] != b'%' {
        return None;
    }

    let start = pos + 1;
    if start >= bytes.len() {
        return None;
    }

    // Check for %(N) format
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

    // Check for %N format (one or more digits)
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

/// Compare template string and parameter count.
#[allow(clippy::nonminimal_bool)]
fn compare_template_and_params(template_string: &str, used_params_count: usize) -> bool {
    let bytes = template_string.as_bytes();
    let have_params = used_params_count > 0;

    let mut has_valid_placeholder = false;
    let mut has_wrong_number = false;
    let mut used_placeholders = [false; 11]; // Index 1-10

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

/// Check if first argument of a FindElement method triggers UsingFindElementByString.
///
/// Returns true if:
/// - No arguments provided (empty call like `НайтиПоНаименованию()`)
/// - First argument is a string literal
/// - First argument is a number literal
fn check_find_element_first_arg(args: &[ExprIdx], ctx: &LoweringCtx) -> bool {
    if args.is_empty() {
        return true;
    }

    let first_arg = args[0];
    let expr = ctx.body.expr_idx(first_arg);
    matches!(expr, Expr::Literal(Literal::String(_)) | Expr::Literal(Literal::Number(_)))
}

/// Determine MagicNumber context by walking AST parents.
///
/// Context determines if the magic number should be excluded based on:
/// - Constructor type (excluded if in excludedConstructors list)
/// - Structure/Map Insert() call
/// - Array index access (excluded if allowMagicIndexes = true)
/// - Default parameter value
/// - Property assignment
/// - Simple assignment
fn determine_magic_number_context(token: &syntax::SyntaxToken) -> MagicNumberContext {
    let mut node = token.parent();

    // Track what contexts we've seen while walking up
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
                // Default parameter value: Функция Метод(Значение = 566)
                return MagicNumberContext::InDefaultParam;
            }
            SyntaxKind::INDEX_EXPR => {
                // Array index access: Массив[20]
                return MagicNumberContext::InArrayIndex;
            }
            SyntaxKind::NEW_EXPR => {
                // Constructor: Новый ТипОбъекта(10, 2)
                // Extract type name
                if let Some(type_name) = current
                    .children_with_tokens()
                    .filter_map(|el| el.into_token())
                    .find(|tok| tok.kind() == SyntaxKind::IDENT)
                {
                    let name = type_name.text().to_lowercase();
                    // Check if it's a structure/map constructor
                    if name.contains("структура")
                        || name.contains("structure")
                        || name.contains("соответствие")
                        || name.contains("map")
                    {
                        return MagicNumberContext::InStructureConstructor;
                    }
                    // Return constructor context with type name for excludedConstructors check
                    return MagicNumberContext::InConstructor { type_name: name };
                }
            }
            SyntaxKind::BINARY_EXPR => {
                in_binary_expr = true;
            }
            SyntaxKind::ARG_LIST => {
                in_arg_list = true;
                // Determine argument index by counting commas before our token
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
                // Check if this is Structure.Insert() or Map.Insert()
                if let Some(method_name) = find_method_name_for_magic_number(&current) {
                    let name = method_name.to_lowercase();
                    if name == "вставить" || name == "insert" {
                        return MagicNumberContext::InStructureInsert;
                    }
                    // Round/Окр: second argument is precision, self-documenting
                    if (name == "окр" || name == "round") && arg_index == 1 {
                        return MagicNumberContext::InRoundPrecision;
                    }
                }
            }
            SyntaxKind::ASSIGN_STMT => {
                in_assign = true;
                // Check if this is property assignment (has DOT)
                has_dot_in_assign = current
                    .children_with_tokens()
                    .any(|el| el.as_token().is_some_and(|t| t.kind() == SyntaxKind::DOT));
            }
            SyntaxKind::RETURN_STMT => {
                return MagicNumberContext::InReturn;
            }
            SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF => {
                // Reached method boundary - stop walking
                break;
            }
            _ => {}
        }
        node = current.parent();
    }

    // Determine context from accumulated flags
    if in_assign {
        if has_dot_in_assign && !in_arg_list {
            // Property assignment: Структура.Поле = 20
            return MagicNumberContext::InPropertyAssignment;
        }
        if !in_binary_expr && !in_arg_list {
            // Simple assignment: День = 6
            return MagicNumberContext::InSimpleAssignment;
        }
        if in_ternary && !in_binary_expr {
            // Ternary branch in assignment: Result = ?(cond, 1, 2)
            return MagicNumberContext::InTernaryBranch;
        }
    }

    if in_call && in_arg_list && !in_binary_expr {
        // Method call argument: .Добавить(2)
        return MagicNumberContext::InMethodCall;
    }

    if in_binary_expr {
        // Expression with operators: СекундВЧасе = 60 * 60
        return MagicNumberContext::InExpression;
    }

    MagicNumberContext::Other
}

/// Find method name in a CALL_STMT or CALL_EXPR node for MagicNumber context.
fn find_method_name_for_magic_number(node: &SyntaxNode) -> Option<String> {
    // Look for FIELD_EXPR which contains the method call structure
    for child in node.descendants() {
        if child.kind() == SyntaxKind::FIELD_EXPR {
            // In FIELD_EXPR, method name is the last IDENT token
            return child
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .filter(|t| t.kind() == SyntaxKind::IDENT)
                .last()
                .map(|t| t.text().to_string());
        }
        // Don't descend into ARG_LIST
        if child.kind() == SyntaxKind::ARG_LIST {
            break;
        }
    }

    // For simple function calls without dot, find the first IDENT node or token before ARG_LIST
    for child in node.children_with_tokens() {
        match child {
            syntax::NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT => {
                return Some(t.text().to_string());
            }
            syntax::NodeOrToken::Node(n) if n.kind() == SyntaxKind::IDENT => {
                // IDENT node wrapping an IDENT token
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

/// Check if a SafeMode() call is in an unsafe context.
///
/// Unsafe contexts:
/// - `Не БезопасныйРежим()` (NOT operator)
/// - `БезопасныйРежим() ИЛИ ...` (boolean AND/OR)
/// - `Если БезопасныйРежим() Тогда` (sole condition without comparison)
///
/// Safe contexts:
/// - `БезопасныйРежим() = Истина` (explicit comparison)
/// - `Перем = БезопасныйРежим()` (assignment)
/// - `Метод(БезопасныйРежим())` (argument)
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
