//! Expression lowering.
//!
//! This module handles lowering of BSL expressions from AST to HIR.

use syntax::{SyntaxKind, SyntaxNode};

use crate::body::{Body, BodyDiagnostic};
use crate::hir::{BinaryOp, Expr, ExprId, Literal, UnaryOp};
use crate::Name;

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
        SyntaxKind::IDENT => {
            // Identifier - variable reference
            let text = node.text().to_string();
            ctx.mark_var_used(&text);
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
                ctx.mark_var_used(ident.text());
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

            // Check for magic number
            if is_magic_number(value) {
                ctx.emit(BodyDiagnostic::MagicNumber {
                    value: text.clone(),
                    range: token.text_range(),
                });
            }

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

/// Check if a number is a "magic number" (should be a named constant).
fn is_magic_number(value: f64) -> bool {
    // Common non-magic numbers
    const ALLOWED: &[f64] = &[-1.0, 0.0, 1.0, 2.0, 10.0, 100.0];

    if ALLOWED.contains(&value) {
        return false;
    }

    // Numbers with many digits are likely magic
    value.abs() > 2.0
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

    // Only check for IDENT (global function call), not FIELD_EXPR (method call)
    if actual_callee.kind() == SyntaxKind::IDENT {
        let name = actual_callee.text().to_string();
        if is_deprecated_method(&name) {
            // Emit DeprecatedMethod diagnostic
            // Range covers the entire call expression including arguments
            ctx.diagnostics
                .push(BodyDiagnostic::DeprecatedMethod { name, range: node.text_range() });
        }
    }

    let callee = lower_expr_node(ctx, &callee_node);

    // Find ARG_LIST for both lowering and diagnostics
    let arg_list_node = node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST);

    // Arguments
    let args =
        arg_list_node.as_ref().map(|arg_list| lower_arg_list(ctx, arg_list)).unwrap_or_default();

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
    node.children().map(|n| lower_expr_node(ctx, &n)).collect()
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
    let field_name = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| tok.kind() == SyntaxKind::IDENT)
        .last()
        .map(|tok| Name::new(tok.text()))
        .unwrap_or_else(Name::missing);

    // Check if this is actually a method call (has ARG_LIST)
    let arg_list_node = node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST);

    if arg_list_node.is_some() {
        let method = field_name.to_string();

        // Analyze call structure to determine call type
        let call_info = analyze_qualified_call(node, ctx);

        if let Some(info) = call_info {
            match info {
                QualifiedCallInfo::TwoLevel { module } => {
                    // Emit MissingCommonModuleMethod diagnostic for potential CommonModule calls.
                    ctx.diagnostics.push(BodyDiagnostic::MissingCommonModuleMethod {
                        module: module.clone(),
                        method: method.clone(),
                        range: node.text_range(),
                    });

                    // Emit MissedRequiredParameter diagnostic for qualified calls.
                    let arg_presence =
                        arg_list_node.as_ref().map(extract_arg_presence).unwrap_or_default();

                    ctx.diagnostics.push(BodyDiagnostic::MissedRequiredParameter {
                        callee: method,
                        module: Some(module),
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
                        mdo_type: Some(mdo_type),
                        mdo_name: Some(mdo_name),
                        args: arg_presence,
                        range: node.text_range(),
                    });
                }
            }
        }

        let args = arg_list_node
            .as_ref()
            .map(|arg_list| lower_arg_list(ctx, arg_list))
            .unwrap_or_default();

        Expr::MethodCall { receiver: base, method: field_name, args: args.into_boxed_slice() }
    } else {
        Expr::Field { base, field: field_name }
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
        return None;
    }

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

        // Different expression types or complex expressions - not equal
        _ => false,
    }
}
