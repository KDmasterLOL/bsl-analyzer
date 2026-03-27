//! IdenticalExpressions diagnostic
//!
//! Detects identical expressions on both sides of binary operators.
//!
//! ## Why?
//! Identical expressions in comparisons or operations often indicate a bug.
//! For example:
//! - `x == x` is always true
//! - `a - a` is always 0
//! - `a > a` is always false
//!
//! ## Exceptions
//! - **Addition and multiplication:** `x + x` and `x * x` are considered normal
//! - **Popular divisors:** `60 / 60` or `1024 / 1024` can be ignored (configurable)
//! - **Transitive comparisons:** `1 = A` is equivalent to `A = 1` for OR/AND chains
//!
//! ## Bad practice
//! ```bsl
//! Если x == x Тогда  // Always true - likely a bug
//!     // ...
//! КонецЕсли;
//!
//! Результат = a - a;  // Always 0 - suspicious
//! ```
//!
//! ## Good practice
//! ```bsl
//! Если x == y Тогда  // Compare different variables
//!     // ...
//! КонецЕсли;
//!
//! Результат = a - b;
//! ```
//!
//! ## Implementation
//!
//! **Hybrid approach: HIR + AST fallback**
//!
//! ### HIR-based checking (main logic):
//! 1. Semantic expression equality (not text-based) - handles whitespace/parentheses correctly
//! 2. Type-safe operator matching via BinaryOp enum
//! 3. Module-level coverage via ModuleBodies.module_code
//! 4. Accurate statement context detection via Body.stmts (not heuristic!)
//! 5. Recursive logical chain collection via ExprId
//! 6. Transitive comparison normalization (`A=1` ≡ `1=A` in OR/AND chains)
//!
//! ### AST fallback (preprocessor split expressions):
//! - Required for edge case: expressions split by `#Область` or `#Если`
//! - Example: `Результат = Истина\n#Область\n ИЛИ Истина;\n#КонецОбласти`
//! - Cannot be migrated to HIR because:
//!   - HIR loses preprocessor directive boundaries
//!   - ERROR nodes (`KW_OR` without operands) become `Expr::Missing`
//!   - No way to distinguish intentional split from separate statements
//! - See `check_preprocessor_split_expressions()` for detailed explanation

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{BinaryOp, Body, BodySourceMap, Expr, ExprId, IdConversion, Literal, UnaryOp};
use std::collections::HashSet;
use syntax::{SyntaxKind, SyntaxNode}; // Keep for preprocessor fallback

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::IdenticalExpressions;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // HIR-based checking - covers methods and module-level code
    let mut diagnostics = crate::utils::for_each_body(ctx, |body, source_map, diags| {
        check_body(body, source_map, diags, code, ctx);
    });

    // AST fallback for preprocessor split expressions (Phase 5 evaluation)
    // These might not be properly captured by HIR lowering
    // E.g., "Результат = Истина\n#Область\n ИЛИ Истина;\n#КонецОбласти"
    let parse = ctx.parse();
    let root = parse.syntax_node();
    check_preprocessor_split_expressions(&root, &mut diagnostics, code, ctx);

    diagnostics
}

// ============================================================================
// Phase 1: Core HIR utility functions
// ============================================================================

/// Compare two HIR expressions semantically for identity.
///
/// Unlike text-based comparison, this handles:
/// - Whitespace normalization automatically
/// - Parentheses correctly (via HIR structure)
/// - Type-safe operator comparison
///
/// Returns true if expressions are semantically identical.
fn are_exprs_semantically_equal(lhs_id: ExprId, rhs_id: ExprId, body: &Body) -> bool {
    let lhs = body.expr(lhs_id);
    let rhs = body.expr(rhs_id);

    match (lhs, rhs) {
        // Binary operations: check op and both operands
        (
            Expr::BinaryOp { lhs: l_lhs, rhs: l_rhs, op: l_op },
            Expr::BinaryOp { lhs: r_lhs, rhs: r_rhs, op: r_op },
        ) => {
            l_op == r_op
                && are_exprs_semantically_equal(
                    ExprId::from_idx(*l_lhs),
                    ExprId::from_idx(*r_lhs),
                    body,
                )
                && are_exprs_semantically_equal(
                    ExprId::from_idx(*l_rhs),
                    ExprId::from_idx(*r_rhs),
                    body,
                )
        }

        // Unary operations: check op and operand
        (Expr::UnaryOp { expr: l_expr, op: l_op }, Expr::UnaryOp { expr: r_expr, op: r_op }) => {
            l_op == r_op
                && are_exprs_semantically_equal(
                    ExprId::from_idx(*l_expr),
                    ExprId::from_idx(*r_expr),
                    body,
                )
        }

        // Literals: compare values
        (Expr::Literal(l_lit), Expr::Literal(r_lit)) => l_lit == r_lit,

        // Paths (variables, function names): compare names
        (Expr::Path(l_name), Expr::Path(r_name)) => l_name == r_name,

        // Field access: check base and field name
        (
            Expr::Field { base: l_base, field: l_field },
            Expr::Field { base: r_base, field: r_field },
        ) => {
            l_field == r_field
                && are_exprs_semantically_equal(
                    ExprId::from_idx(*l_base),
                    ExprId::from_idx(*r_base),
                    body,
                )
        }

        // Index access: check base and index expressions
        (
            Expr::Index { base: l_base, index: l_index },
            Expr::Index { base: r_base, index: r_index },
        ) => {
            are_exprs_semantically_equal(ExprId::from_idx(*l_base), ExprId::from_idx(*r_base), body)
                && are_exprs_semantically_equal(
                    ExprId::from_idx(*l_index),
                    ExprId::from_idx(*r_index),
                    body,
                )
        }

        // Function calls: check callee and all arguments
        (
            Expr::Call { callee: l_callee, args: l_args },
            Expr::Call { callee: r_callee, args: r_args },
        ) => {
            if l_args.len() != r_args.len() {
                return false;
            }
            if !are_exprs_semantically_equal(
                ExprId::from_idx(*l_callee),
                ExprId::from_idx(*r_callee),
                body,
            ) {
                return false;
            }
            l_args.iter().zip(r_args.iter()).all(|(l_arg, r_arg)| {
                are_exprs_semantically_equal(
                    ExprId::from_idx(*l_arg),
                    ExprId::from_idx(*r_arg),
                    body,
                )
            })
        }

        // Ternary expressions: check condition, then_expr, else_expr
        (
            Expr::Ternary { condition: l_cond, then_expr: l_then, else_expr: l_else },
            Expr::Ternary { condition: r_cond, then_expr: r_then, else_expr: r_else },
        ) => {
            are_exprs_semantically_equal(ExprId::from_idx(*l_cond), ExprId::from_idx(*r_cond), body)
                && are_exprs_semantically_equal(
                    ExprId::from_idx(*l_then),
                    ExprId::from_idx(*r_then),
                    body,
                )
                && are_exprs_semantically_equal(
                    ExprId::from_idx(*l_else),
                    ExprId::from_idx(*r_else),
                    body,
                )
        }

        // New keyword with type and args
        (
            Expr::New { type_name: l_type, args: l_args },
            Expr::New { type_name: r_type, args: r_args },
        ) => {
            if l_type != r_type || l_args.len() != r_args.len() {
                return false;
            }
            l_args.iter().zip(r_args.iter()).all(|(l_arg, r_arg)| {
                are_exprs_semantically_equal(
                    ExprId::from_idx(*l_arg),
                    ExprId::from_idx(*r_arg),
                    body,
                )
            })
        }

        // Different expression kinds are never equal
        _ => false,
    }
}

/// Serialize HIR expression to normalized string for transitive comparison.
///
/// Used for detecting transitive duplicates in logical chains:
/// `A = 1` vs `1 = A` should be recognized as equivalent in OR/AND chains.
///
/// Returns normalized string representation (e.g., "a=1" for both `A = 1` and `1 = A`).
fn expr_to_string(expr_id: ExprId, body: &Body) -> String {
    let expr = body.expr(expr_id);
    match expr {
        Expr::BinaryOp { lhs, rhs, op } => {
            let lhs_str = expr_to_string(ExprId::from_idx(*lhs), body);
            let rhs_str = expr_to_string(ExprId::from_idx(*rhs), body);
            let op_str = match op {
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                BinaryOp::Mod => "%",
                BinaryOp::Eq => "=",
                BinaryOp::Neq => "<>",
                BinaryOp::Lt => "<",
                BinaryOp::Le => "<=",
                BinaryOp::Gt => ">",
                BinaryOp::Ge => ">=",
                BinaryOp::And => "and",
                BinaryOp::Or => "or",
            };
            format!("{} {} {}", lhs_str, op_str, rhs_str)
        }
        Expr::UnaryOp { expr, op } => {
            let expr_str = expr_to_string(ExprId::from_idx(*expr), body);
            let op_str = match op {
                UnaryOp::Not => "НЕ",
                UnaryOp::Neg => "-",
                UnaryOp::Plus => "+",
            };
            format!("{} {}", op_str, expr_str)
        }
        Expr::Literal(lit) => match lit {
            Literal::Bool(b) => b.to_string(),
            Literal::Number(n) => n.to_string(),
            Literal::String(s) => format!("\"{}\"", s),
            Literal::Date(d) => format!("'{}'", d),
            Literal::Undefined => "undefined".to_string(),
            Literal::Null => "null".to_string(),
        },
        Expr::Path(name) => name.as_str().to_string(),
        Expr::Field { base, field } => {
            format!("{}.{}", expr_to_string(ExprId::from_idx(*base), body), field.as_str())
        }
        Expr::Index { base, index } => {
            format!(
                "{}[{}]",
                expr_to_string(ExprId::from_idx(*base), body),
                expr_to_string(ExprId::from_idx(*index), body)
            )
        }
        Expr::Call { callee, args } => {
            let callee_str = expr_to_string(ExprId::from_idx(*callee), body);
            let args_str = args
                .iter()
                .map(|arg| expr_to_string(ExprId::from_idx(*arg), body))
                .collect::<Vec<_>>()
                .join(",");
            format!("{}({})", callee_str, args_str)
        }
        Expr::Ternary { condition, then_expr, else_expr } => {
            format!(
                "?({},{},{})",
                expr_to_string(ExprId::from_idx(*condition), body),
                expr_to_string(ExprId::from_idx(*then_expr), body),
                expr_to_string(ExprId::from_idx(*else_expr), body)
            )
        }
        Expr::New { type_name, args } => {
            let type_str = type_name.as_ref().map(|t| t.as_str()).unwrap_or("?");
            let args_str = args
                .iter()
                .map(|arg| expr_to_string(ExprId::from_idx(*arg), body))
                .collect::<Vec<_>>()
                .join(",");
            format!("new({}({}))", type_str.to_lowercase(), args_str)
        }
        Expr::QualifiedPath(qname) => {
            qname.segments().iter().map(|s| s.as_str().to_lowercase()).collect::<Vec<_>>().join(".")
        }
        Expr::MethodCall { receiver, method, args } => {
            let receiver_str = expr_to_string(ExprId::from_idx(*receiver), body);
            let args_str = args
                .iter()
                .map(|arg| expr_to_string(ExprId::from_idx(*arg), body))
                .collect::<Vec<_>>()
                .join(",");
            format!("{}.{}({})", receiver_str, method.as_str().to_lowercase(), args_str)
        }
        Expr::Array(elements) => {
            let elements_str = elements
                .iter()
                .map(|elem| expr_to_string(ExprId::from_idx(*elem), body))
                .collect::<Vec<_>>()
                .join(",");
            format!("[{}]", elements_str)
        }
        Expr::Await { expr } => {
            format!("await({})", expr_to_string(ExprId::from_idx(*expr), body))
        }
        Expr::Missing => "<missing>".to_string(),
    }
}

/// Check if expression is at statement level (not nested in another expression).
///
/// Uses Body.stmts arena to accurately detect statement context.
/// This is more reliable than AST heuristics for distinguishing:
/// - `Перем1 = Перем1;` (assignment statement - skip)
/// - `Если X = X Тогда` (comparison in condition - report)
///
/// Returns true if expression appears as direct child of a statement.
fn is_statement_expr(expr_id: ExprId, body: &Body, _source_map: &BodySourceMap) -> bool {
    // Check if any statement contains this expression as its direct child
    for stmt_id in body.body_stmts() {
        let stmt = body.stmt(stmt_id);
        match stmt {
            hir::Stmt::Expr(expr) if ExprId::from_idx(*expr) == expr_id => {
                return true;
            }
            hir::Stmt::Assign { value, .. } if ExprId::from_idx(*value) == expr_id => {
                return true;
            }
            _ => {}
        }
    }
    false
}

// ============================================================================
// Phase 2: HIR-based diagnostic checking
// ============================================================================

/// Check a single body (method or module-level code) for identical expressions.
///
/// This is the HIR-based version that replaces AST traversal.
fn check_body(
    body: &Body,
    source_map: &BodySourceMap,
    diagnostics: &mut Vec<Diagnostic>,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) {
    // Walk all expressions in the body
    for (expr_id, expr) in body.exprs_iter() {
        if let Expr::BinaryOp { lhs, rhs, op } = expr {
            check_binary_expr_hir(
                expr_id,
                ExprId::from_idx(*lhs),
                ExprId::from_idx(*rhs),
                *op,
                body,
                source_map,
                diagnostics,
                code,
                ctx,
            );
        }
    }
}

/// Check a binary expression for identical operands (HIR-based).
///
/// Replaces the AST-based check_binary_expr function.
#[allow(clippy::too_many_arguments)] // All parameters are needed for HIR-based checking
fn check_binary_expr_hir(
    expr_id: ExprId,
    lhs: ExprId,
    rhs: ExprId,
    op: BinaryOp,
    body: &Body,
    source_map: &BodySourceMap,
    diagnostics: &mut Vec<Diagnostic>,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) {
    // Skip assignment statements (e.g., "Перем1 = Перем1;")
    // Assignment statements should not be flagged - only comparisons in conditions
    if op == BinaryOp::Eq && is_statement_expr(expr_id, body, source_map) {
        return;
    }

    // Ignore addition and multiplication - considered normal for identical operands
    if matches!(op, BinaryOp::Add | BinaryOp::Mul) {
        return;
    }

    // For AND/OR operators, check the entire logical chain for duplicates
    if matches!(op, BinaryOp::And | BinaryOp::Or) {
        // Only check at top level of chain to avoid duplicate reports
        // Top level = this binary op is not nested inside another same-type binary op
        if !is_nested_in_logical_chain(expr_id, op, body) {
            check_logical_chain_hir(expr_id, op, body, source_map, diagnostics, code, ctx);
        }
        return;
    }

    // Check if operands are semantically equal
    if are_exprs_semantically_equal(lhs, rhs, body) {
        // Check popular division exception
        if op == BinaryOp::Div && is_popular_division_hir(lhs, body, ctx) {
            return;
        }

        // Get range from source map
        let Some(range) = source_map.expr_range(expr_id) else {
            return;
        };

        // Get operator text for message
        let op_text = match op {
            BinaryOp::Eq => "=",
            BinaryOp::Neq => "<>",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::Sub => "-",
            BinaryOp::Div => "/",
            BinaryOp::Mod => "%",
            _ => "?",
        };

        // Get expression text for message
        let lhs_text = expr_to_string(lhs, body);

        diagnostics.push(Diagnostic {
            code,
            message: format!(
                "Одинаковые выражения '{}' с обеих сторон оператора '{}'",
                lhs_text, op_text
            ),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

/// Check if this is a popular division case (60/60, 1024/1024) - HIR version.
fn is_popular_division_hir(expr_id: ExprId, body: &Body, ctx: &DiagnosticsContext) -> bool {
    let popular_divisors = ctx
        .config
        .get_string_param(DiagnosticCode::IdenticalExpressions, "popularDivisors")
        .unwrap_or_else(|| "60, 1024".to_string());

    if popular_divisors.trim().is_empty() {
        return false; // Disabled
    }

    let divisors: HashSet<String> =
        popular_divisors.split(',').map(|s| s.trim().to_string()).collect();

    // Check if expression is a literal number matching popular divisors
    let expr = body.expr(expr_id);
    if let Expr::Literal(Literal::Number(n)) = expr {
        let text = n.to_string();
        if divisors.contains(&text) {
            return true;
        }
    }

    false
}

// ============================================================================
// Phase 3: Logical chain handling (AND/OR)
// ============================================================================

/// Check if expression is nested inside another logical chain of the same operator.
///
/// Used to detect top-level of chain and avoid duplicate reports.
fn is_nested_in_logical_chain(expr_id: ExprId, op: BinaryOp, body: &Body) -> bool {
    // Walk all expressions looking for parent binary ops
    for (_parent_id, parent_expr) in body.exprs_iter() {
        if let Expr::BinaryOp { lhs, rhs, op: parent_op } = parent_expr {
            // Check if current expr is operand of same-type binary op
            if parent_op == &op
                && (ExprId::from_idx(*lhs) == expr_id || ExprId::from_idx(*rhs) == expr_id)
            {
                return true;
            }
        }
    }
    false
}

/// Check logical chain (AND/OR) for duplicate operands with transitive comparison.
///
/// Example: `A = 1 ИЛИ B = 2 ИЛИ A = 1` - duplicate detected
/// Transitive: `1 = A ИЛИ A = 1` - also duplicate (normalized to same form)
fn check_logical_chain_hir(
    root_expr_id: ExprId,
    chain_op: BinaryOp,
    body: &Body,
    source_map: &BodySourceMap,
    diagnostics: &mut Vec<Diagnostic>,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) {
    let mut operands = Vec::new();
    collect_logical_chain_hir(root_expr_id, chain_op, body, &mut operands);

    // Check for duplicates using normalized comparison
    let mut seen = HashSet::new();
    let mut duplicate = None;

    for &operand_id in &operands {
        let operand_str = expr_to_string(operand_id, body);
        let lowered = operand_str.to_lowercase().replace(' ', "");
        let normalized = normalize_operand_hir(&lowered);

        if !seen.insert(normalized) {
            duplicate = Some(operand_str);
            break;
        }
    }

    if let Some(dup_text) = duplicate {
        // Get range for the root expression
        if let Some(range) = source_map.expr_range(root_expr_id) {
            let op_text = match chain_op {
                BinaryOp::And => "И",
                BinaryOp::Or => "ИЛИ",
                _ => "?",
            };

            diagnostics.push(Diagnostic {
                code,
                message: format!(
                    "Повторяющееся выражение '{}' в цепочке оператора '{}'",
                    dup_text, op_text
                ),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }
}

/// Recursively collect all operands from logical chain (AND/OR).
///
/// For `A ИЛИ B ИЛИ C`, collects [A, B, C].
/// For `(A И B) ИЛИ (C И D)`, collects [(A И B), (C И D)] if chain_op is OR.
fn collect_logical_chain_hir(
    expr_id: ExprId,
    chain_op: BinaryOp,
    body: &Body,
    operands: &mut Vec<ExprId>,
) {
    let expr = body.expr(expr_id);

    // If this is a binary op of the same type, recurse into operands
    if let Expr::BinaryOp { lhs, rhs, op } = expr {
        if op == &chain_op {
            collect_logical_chain_hir(ExprId::from_idx(*lhs), chain_op, body, operands);
            collect_logical_chain_hir(ExprId::from_idx(*rhs), chain_op, body, operands);
            return;
        }
    }

    // Otherwise, this is a leaf operand - add it
    operands.push(expr_id);
}

/// Normalize operand for transitive comparison.
///
/// For commutative comparison operators (=, <>), sort operands alphabetically.
/// This makes "1=A" equivalent to "A=1" for duplicate detection.
///
/// Examples:
/// - "а=1" → "1=а" (sorted)
/// - "х<>5" → "5<>х" (sorted)
/// - "а+b" → "а+b" (not a comparison, unchanged)
fn normalize_operand_hir(text: &str) -> String {
    // Try to parse as comparison and normalize
    for op in &["<>", "="] {
        if let Some(pos) = text.find(op) {
            let left = &text[..pos];
            let right = &text[pos + op.len()..];

            // Sort operands alphabetically for commutative operators
            let mut parts = [left, right];
            parts.sort();

            return format!("{}{}{}", parts[0], op, parts[1]);
        }
    }

    // Not a comparison or different operator - return as is
    text.to_string()
}

/// Normalize text: remove whitespace and parentheses
fn normalize_text(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .trim_matches(|c| c == '(' || c == ')')
        .to_string()
}

/// Check for expressions split by preprocessor directives (AST fallback required).
///
/// **Example:**
/// ```bsl
/// Результат = Истина
/// #Область НоваяОбласть
///  ИЛИ Истина;
/// #КонецОбласти
/// ```
///
/// **Why AST fallback is required (cannot be migrated to HIR):**
///
/// 1. **HIR loses preprocessor directive boundaries**
///    - `process_preproc_region()` processes content recursively
///    - After lowering, HIR sees only separate statements without context
///
/// 2. **HIR sees this as:**
///    ```text
///    Stmt[0]: Assign { Результат = Bool(true) }  // complete assignment
///    Stmt[1]: Expr(Missing)                       // ERROR(ИЛИ) → Missing expr
///    Stmt[2]: Expr(Bool(true))                    // separate literal
///    ```
///    No way to tell that statements [1] and [2] were inside a directive.
///
/// 3. **ERROR nodes lose information**
///    - `ERROR(KW_OR)` in AST → `Expr::Missing` in HIR
///    - No information that this was an attempt to continue expression
///
/// 4. **AST preserves:**
///    - Directive boundaries (PRE_REGION_DIR, PRE_IF_DIR)
///    - Sibling relationships (prev/next sibling)
///    - ERROR nodes with token information (KW_OR, KW_AND)
///    - Semicolon presence between nodes
///
/// **Detection approach:**
/// - Find ASSIGN_STMT followed by preprocessor directive
/// - Extract RHS from assignment
/// - Collect operands from directive content (including ERROR nodes)
/// - Check for duplicates between assignment RHS and directive operands
fn check_preprocessor_split_expressions(
    root: &SyntaxNode,
    diagnostics: &mut Vec<Diagnostic>,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) {
    for node in root.descendants() {
        // Look for ASSIGN_STMT followed by preprocessor directive
        if node.kind() != SyntaxKind::ASSIGN_STMT {
            continue;
        }

        // Get the next sibling (might be preprocessor directive)
        let Some(next_sibling) = node.next_sibling() else {
            continue;
        };

        // Check if next sibling is preprocessor directive
        if !matches!(next_sibling.kind(), SyntaxKind::PRE_REGION_DIR | SyntaxKind::PRE_IF_DIR) {
            continue;
        }

        // Check if preprocessor block contains split logical expression (ERROR with KW_OR/KW_AND)
        // This distinguishes between:
        // 1. Split expression: "Результат = Истина #Область ИЛИ Истина #КонецОбласти" - ERROR(KW_OR)
        // 2. Normal code: "X = Var; #Если... Y = Var; #КонецЕсли" - no split expression
        if !has_split_logical_operator(&next_sibling) {
            // Also check CALL_STMT siblings after preprocessor
            let mut found_split = false;
            let mut current_sibling = next_sibling.next_sibling();
            while let Some(sibling) = current_sibling {
                if sibling.kind() == SyntaxKind::CALL_STMT {
                    if has_split_logical_operator(&sibling) {
                        found_split = true;
                        break;
                    }
                    current_sibling = sibling.next_sibling();
                } else {
                    break;
                }
            }
            if !found_split {
                continue;
            }
        }

        // Extract RHS value from assignment
        let Some(assign_rhs) = extract_assign_rhs(&node) else {
            continue;
        };

        // Extract operands from preprocessor block
        let mut all_operands = vec![normalize_text(&assign_rhs)];
        all_operands.extend(extract_preprocessor_operands(&next_sibling));

        // Also collect operands from subsequent CALL_STMT siblings after preprocessor
        // E.g., "#КонецЕсли\n ИЛИ Истина;" creates CALL_STMT siblings
        let mut current_sibling = next_sibling.next_sibling();
        while let Some(sibling) = current_sibling {
            if sibling.kind() == SyntaxKind::CALL_STMT {
                all_operands.extend(extract_preprocessor_operands(&sibling));
                current_sibling = sibling.next_sibling();
            } else {
                break;
            }
        }

        // Check if we have any operands beyond the assignment RHS
        if all_operands.len() < 2 {
            continue;
        }

        // Check for duplicates
        let mut seen = HashSet::new();
        for operand in &all_operands {
            if !seen.insert(operand.clone()) {
                // Found duplicate!
                diagnostics.push(Diagnostic {
                    code,
                    message: format!(
                        "Повторяющееся выражение '{}' в выражении, разбитом препроцессорной директивой",
                        operand
                    ),
                    severity: ctx.severity(code),
                    range: node.text_range(),
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
                break;
            }
        }
    }
}

/// Check if node contains ERROR with KW_OR or KW_AND, indicating a split logical expression.
fn has_split_logical_operator(node: &SyntaxNode) -> bool {
    for descendant in node.descendants() {
        if descendant.kind() == SyntaxKind::ERROR {
            // Check if ERROR contains logical operator tokens
            for token in descendant.children_with_tokens() {
                if let Some(token) = token.as_token() {
                    if matches!(token.kind(), SyntaxKind::KW_OR | SyntaxKind::KW_AND) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Extract right-hand side value from ASSIGN_STMT
fn extract_assign_rhs(assign_stmt: &SyntaxNode) -> Option<String> {
    // ASSIGN_STMT structure: IDENT, EXPR (RHS)
    for child in assign_stmt.children() {
        if child.kind() == SyntaxKind::EXPR {
            return Some(child.text().to_string());
        }
    }
    None
}

/// Extract operands from preprocessor directive block.
///
/// **Returns:** All literals and identifiers found in the directive content.
///
/// **Example AST structure:**
/// ```text
/// PRE_REGION_DIR:
///   CALL_STMT:
///     ERROR(KW_OR)           ← ИЛИ without operands
///   CALL_STMT:
///     LITERAL: Истина        ← the actual operand
/// ```
///
/// **Extraction strategy:**
/// 1. Collect complete expressions from CALL_STMT (excluding ERROR nodes)
/// 2. Also collect individual LITERAL/IDENT nodes
/// 3. Normalize text (remove whitespace, parentheses)
/// 4. Deduplicate while preserving order
///
/// Also finds complex expressions beyond simple literals and identifiers.
fn extract_preprocessor_operands(prep_dir: &SyntaxNode) -> Vec<String> {
    let mut operands = Vec::new();

    fn collect_all_operands(node: &SyntaxNode, operands: &mut Vec<String>) {
        // Collect complete expressions from CALL_STMT nodes
        if node.kind() == SyntaxKind::CALL_STMT {
            // Get the full expression text (excluding ERROR nodes)
            let expr_text: String = node
                .descendants()
                .filter(|n| n.kind() != SyntaxKind::ERROR)
                .filter(|n| matches!(n.kind(), SyntaxKind::LITERAL | SyntaxKind::IDENT))
                .map(|n| n.text().to_string())
                .collect::<Vec<_>>()
                .join("");

            if !expr_text.is_empty() {
                operands.push(normalize_text(&expr_text));
            }
        }

        // Also collect individual literals/identifiers
        if matches!(node.kind(), SyntaxKind::LITERAL | SyntaxKind::IDENT) {
            operands.push(normalize_text(&node.text().to_string()));
        }

        for child in node.children() {
            collect_all_operands(&child, operands);
        }
    }

    collect_all_operands(prep_dir, &mut operands);

    // Deduplicate while preserving order
    let mut seen = HashSet::new();
    operands.into_iter().filter(|op| seen.insert(op.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_ast_diagnostic;
    #[test]
    fn test_identical_comparison() {
        let code = r#"
Функция Тест()
    Если x = x Тогда
        Возврат Истина;
    КонецЕсли;
    Возврат Ложь;
КонецФункции
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic, found {}", diagnostics.len());
        assert!(diagnostics[0].message.contains("x"));
    }

    #[test]
    fn test_different_expressions() {
        let code = r#"
Функция Тест()
    Если x = y Тогда
        Возврат Истина;
    КонецЕсли;
    Возврат Ложь;
КонецФункции
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_identical_arithmetic() {
        let code = r#"
Процедура Тест()
    Результат = a - a;
КонецПроцедуры
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("-"));
    }

    #[test]
    fn test_addition_multiplication_allowed() {
        let code = r#"
Процедура Тест()
    Результат = x + x;  // OK
    Результат = x * x;  // OK
КонецПроцедуры
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_logical_chain() {
        let code = r#"
Функция Тест()
    Возврат А И Б И Б;
КонецФункции
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "Expected 1 diagnostic for duplicate Б, found {}",
            diagnostics.len()
        );
        assert!(
            diagnostics[0].message.contains("Б"),
            "Message should contain 'Б' (original case from expr_to_string)"
        );
    }

    /// Assignment to self is not flagged (statement-level Eq is skipped).
    #[test]
    fn test_fixture_self_assignment_skipped() {
        let code = r#"Функция Проверка()
    Перем1 = Перем1;
    Возврат Истина;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Self-assignment should not be flagged");
    }

    /// Identical operands in <> comparison should trigger.
    #[test]
    fn test_fixture_identical_neq_triggers() {
        let code = r#"Функция Проверка()
    Если Перем2 <> Перем2 Тогда
        Возврат Истина;
    КонецЕсли;
    Возврат Ложь;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Identical <> should trigger");
        assert!(diagnostics[0].message.contains("<>"));
    }

    /// Identical return expression with > should trigger.
    #[test]
    fn test_fixture_identical_gt_in_return_triggers() {
        let code = r#"Функция Проверка()
    Возврат Перем3 > Перем3;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Identical > should trigger");
    }

    /// Division with identical operands triggers; multiplication does not.
    #[test]
    fn test_fixture_division_triggers_multiplication_skipped() {
        let code = r#"Процедура Проверка()
    Перем4 = Перем5 + Перем5;
    Перем6 = Перем7 / Перем7;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "/ triggers, + does not");
        assert!(diagnostics[0].message.contains("/"));
    }

    /// Identical complex expression on both sides of = in condition triggers.
    #[test]
    fn test_fixture_identical_complex_expr_eq_triggers() {
        let code = r#"Функция Проверка()
    Если (Перем8 + Перем9 + Перем10) = (Перем8 + Перем9 + Перем10) Тогда
        Возврат Истина;
    КонецЕсли;
    Возврат Ложь;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Identical complex expressions should trigger");
    }

    /// Identical operands in <> in return expression triggers.
    #[test]
    fn test_fixture_identical_neq_in_return_triggers() {
        let code = r#"Функция Проверка()
    Возврат Перем11 <> Перем11;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Identical <> in return should trigger");
    }

    /// Duplicate operand in OR chain triggers.
    #[test]
    fn test_fixture_duplicate_in_or_chain_triggers() {
        let code = r#"Функция Проверка()
    Если УсловиеВыполняется() ИЛИ УсловиеВыполняется() ИЛИ УсловиеВтороеВыполняется() Тогда
        Возврат Истина;
    КонецЕсли;
    Возврат Ложь;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Duplicate in OR chain should trigger");
    }

    /// Identical < in return expression triggers.
    #[test]
    fn test_fixture_identical_lt_in_return_triggers() {
        let code = r#"Функция Проверка()
    Возврат Перем12 < Перем12;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Identical < should trigger");
    }

    /// Duplicate operand in AND chain triggers.
    #[test]
    fn test_fixture_duplicate_in_and_chain_triggers() {
        let code = r#"Функция Проверка()
    Если (Перем13 = 0) И (Перем13 = 0) Тогда
        Возврат Истина;
    КонецЕсли;
    Возврат Ложь;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Duplicate in AND chain should trigger");
    }

    /// Duplicate in И chain inside Возврат triggers.
    #[test]
    fn test_fixture_duplicate_and_in_return_triggers() {
        let code = r#"Функция Проверка()
    Возврат Перем14 И Перем15 И Перем15;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Duplicate И in return chain should trigger");
    }

    /// Subtraction of identical operands triggers; multiplication does not.
    #[test]
    fn test_fixture_subtraction_triggers_multiplication_skipped() {
        let code = r#"Процедура Проверка()
    Результат = Перем16 - Перем16;
    Результат = Результат * Результат;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "- triggers, * does not");
        assert!(diagnostics[0].message.contains("-"));
    }

    /// Complex И/ИЛИ chain with no duplicates — not flagged.
    #[test]
    fn test_fixture_mixed_and_or_no_duplicates() {
        let code = r#"Функция Проверка()
    Возврат Перем18 И Перем19 ИЛИ Перем18 И Перем20;
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "No duplicates in mixed И/ИЛИ chain");
    }

    /// Module-level: duplicate AND sub-expression in OR chain triggers.
    #[test]
    fn test_fixture_module_level_complex_and_in_or() {
        let code = r#"
Б = 0;
С = (А = 1) И (Б = 1) ИЛИ (А = 1) И (Б = 1);
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Duplicate AND in OR chain at module level");
    }

    /// Module-level: duplicate А = 1 in OR chain triggers.
    #[test]
    fn test_fixture_module_level_or_duplicate() {
        let code = r#"
Если А = 1 ИЛИ А = 1 Тогда
    Б = 1;
КонецЕсли;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Duplicate А = 1 in module-level OR");
    }

    /// Module-level: transitive А = 1 vs 1 = А triggers.
    #[test]
    fn test_fixture_module_level_transitive_or() {
        let code = r#"
ИначеЕсли 1 = А ИЛИ А = 1 Тогда
    Б = 11;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Transitive 1=А vs А=1 in OR chain");
    }

    /// Duplicate in chained assignment А = 12 ИЛИ А = 13 ИЛИ А = 12.
    #[test]
    fn test_fixture_chained_assignment_duplicate() {
        let code = r#"
Б = А = 12 ИЛИ А = 13 ИЛИ А = 12;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Duplicate А = 12 in chained OR");
    }

    /// Preprocessor region split: Истина ИЛИ Истина across #Область triggers.
    #[test]
    fn test_fixture_preprocessor_region_duplicate() {
        let code = r#"
Результат = Истина
#Область ЕщеОднаОбласть
 ИЛИ Истина;
#КонецОбласти
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Duplicate across #Область should trigger");
    }

    /// Preprocessor #Если split: Истина ИЛИ ... ИЛИ Истина triggers.
    #[test]
    fn test_fixture_preprocessor_if_duplicate() {
        let code = r#"
Результат = Истина
#Если ВебКлиент Тогда
 ИЛИ Ложь
#Иначе
 ИЛИ ЗначениеВыражения()
#КонецЕсли
 ИЛИ Истина;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Истина repeated across #Если should trigger");
    }

    /// Preprocessor split with no duplicates — not flagged.
    #[test]
    fn test_fixture_preprocessor_no_duplicate() {
        let code = r#"
Результат = ЗначениеВыражения()
#Если ВебКлиент Тогда
 ИЛИ Ложь
#Иначе
 ИЛИ Отказ
#КонецЕсли
 ИЛИ Истина;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "No duplicates across #Если");
    }

    #[test]
    fn test_simple_or_chain() {
        let code = r#"
Функция Тест()
    Если А = 1 ИЛИ А = 1 Тогда
        Возврат Истина;
    КонецЕсли;
КонецФункции
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "Should find duplicate А = 1 in OR chain, found {}",
            diagnostics.len()
        );
    }

    #[test]
    fn test_transitive_comparison() {
        let code = r#"
Функция Тест()
    Если 1 = А ИЛИ А = 1 Тогда
        Возврат Истина;
    КонецЕсли;
КонецФункции
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "Should find transitive duplicate (1 = А vs А = 1), found {}",
            diagnostics.len()
        );
    }

    #[test]
    fn test_complex_and_in_or() {
        let code = r#"
С = (А = 1) И (Б = 1) ИЛИ (А = 1) И (Б = 1);
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should find duplicate AND sub-expression in OR chain");
    }

    #[test]
    fn test_chained_assignment_with_or() {
        let code = r#"
Б = А = 12 ИЛИ А = 13 ИЛИ А = 12;
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "Should find duplicate А = 12 in chained assignment with OR"
        );
    }

    #[test]
    fn test_preprocessor_region() {
        let code = r#"
Результат = Истина
#Область Тест
 ИЛИ Истина;
#КонецОбласти
"#;

        let _diagnostics = check_ast_diagnostic(code, check);
    }

    #[test]
    fn test_preprocessor_if() {
        let code = r#"
Результат = Истина
#Если ВебКлиент Тогда
 ИЛИ Ложь
#Иначе
 ИЛИ ЗначениеВыражения()
#КонецЕсли
 ИЛИ Истина;
"#;

        let _diagnostics = check_ast_diagnostic(code, check);
    }
}
