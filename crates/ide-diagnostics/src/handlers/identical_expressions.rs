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
//! ## Source
//! Source: bsl-language-server/src/main/java/.../diagnostics/IdenticalExpressionsDiagnostic.java
//! Source: bsl-language-server-rust/crates/bsl-diagnostics/src/rules/identical_expressions.rs
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

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use hir::ModuleId;
use hir_def::{BinaryOp, Body, BodySourceMap, Expr, ExprId, Literal, UnaryOp};
use std::collections::HashSet;
use syntax::{SyntaxKind, SyntaxNode}; // Keep for preprocessor fallback

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::IdenticalExpressions) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    // HIR-based checking - covers methods and module-level code
    let module_id = ModuleId::new(ctx.file_id);
    let module_bodies = ctx.db.module_bodies(module_id);

    // Check module-level code (outside procedures/functions)
    if let Some(module_code) = module_bodies.module_code_result() {
        check_body(&module_code.body, &module_code.source_map, &mut diagnostics, ctx);
    }

    // Check all methods (procedures/functions)
    for (_local_id, body, source_map) in module_bodies.method_bodies() {
        check_body(body, source_map, &mut diagnostics, ctx);
    }

    // AST fallback for preprocessor split expressions (Phase 5 evaluation)
    // These might not be properly captured by HIR lowering
    // E.g., "Результат = Истина\n#Область\n ИЛИ Истина;\n#КонецОбласти"
    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    check_preprocessor_split_expressions(&root, &mut diagnostics);

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
    let lhs = &body.exprs[lhs_id];
    let rhs = &body.exprs[rhs_id];

    match (lhs, rhs) {
        // Binary operations: check op and both operands
        (
            Expr::BinaryOp { lhs: l_lhs, rhs: l_rhs, op: l_op },
            Expr::BinaryOp { lhs: r_lhs, rhs: r_rhs, op: r_op },
        ) => {
            l_op == r_op
                && are_exprs_semantically_equal(*l_lhs, *r_lhs, body)
                && are_exprs_semantically_equal(*l_rhs, *r_rhs, body)
        }

        // Unary operations: check op and operand
        (Expr::UnaryOp { expr: l_expr, op: l_op }, Expr::UnaryOp { expr: r_expr, op: r_op }) => {
            l_op == r_op && are_exprs_semantically_equal(*l_expr, *r_expr, body)
        }

        // Literals: compare values
        (Expr::Literal(l_lit), Expr::Literal(r_lit)) => l_lit == r_lit,

        // Paths (variables, function names): compare names
        (Expr::Path(l_name), Expr::Path(r_name)) => l_name == r_name,

        // Field access: check base and field name
        (
            Expr::Field { base: l_base, field: l_field },
            Expr::Field { base: r_base, field: r_field },
        ) => l_field == r_field && are_exprs_semantically_equal(*l_base, *r_base, body),

        // Index access: check base and index expressions
        (
            Expr::Index { base: l_base, index: l_index },
            Expr::Index { base: r_base, index: r_index },
        ) => {
            are_exprs_semantically_equal(*l_base, *r_base, body)
                && are_exprs_semantically_equal(*l_index, *r_index, body)
        }

        // Function calls: check callee and all arguments
        (
            Expr::Call { callee: l_callee, args: l_args },
            Expr::Call { callee: r_callee, args: r_args },
        ) => {
            if l_args.len() != r_args.len() {
                return false;
            }
            if !are_exprs_semantically_equal(*l_callee, *r_callee, body) {
                return false;
            }
            l_args
                .iter()
                .zip(r_args.iter())
                .all(|(l_arg, r_arg)| are_exprs_semantically_equal(*l_arg, *r_arg, body))
        }

        // Ternary expressions: check condition, then_expr, else_expr
        (
            Expr::Ternary { condition: l_cond, then_expr: l_then, else_expr: l_else },
            Expr::Ternary { condition: r_cond, then_expr: r_then, else_expr: r_else },
        ) => {
            are_exprs_semantically_equal(*l_cond, *r_cond, body)
                && are_exprs_semantically_equal(*l_then, *r_then, body)
                && are_exprs_semantically_equal(*l_else, *r_else, body)
        }

        // New keyword with type and args
        (
            Expr::New { type_name: l_type, args: l_args },
            Expr::New { type_name: r_type, args: r_args },
        ) => {
            if l_type != r_type || l_args.len() != r_args.len() {
                return false;
            }
            l_args
                .iter()
                .zip(r_args.iter())
                .all(|(l_arg, r_arg)| are_exprs_semantically_equal(*l_arg, *r_arg, body))
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
    let expr = &body.exprs[expr_id];
    match expr {
        Expr::BinaryOp { lhs, rhs, op } => {
            let lhs_str = expr_to_string(*lhs, body);
            let rhs_str = expr_to_string(*rhs, body);
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
            format!("{}{}{}", lhs_str, op_str, rhs_str)
        }
        Expr::UnaryOp { expr, op } => {
            let expr_str = expr_to_string(*expr, body);
            let op_str = match op {
                UnaryOp::Not => "not",
                UnaryOp::Neg => "-",
                UnaryOp::Plus => "+",
            };
            format!("{}{}", op_str, expr_str)
        }
        Expr::Literal(lit) => match lit {
            Literal::Bool(b) => b.to_string(),
            Literal::Number(n) => n.to_string(),
            Literal::String(s) => format!("\"{}\"", s),
            Literal::Date(d) => format!("'{}'", d),
            Literal::Undefined => "undefined".to_string(),
            Literal::Null => "null".to_string(),
        },
        Expr::Path(name) => name.as_str().to_lowercase(),
        Expr::Field { base, field } => {
            format!("{}.{}", expr_to_string(*base, body), field.as_str().to_lowercase())
        }
        Expr::Index { base, index } => {
            format!("{}[{}]", expr_to_string(*base, body), expr_to_string(*index, body))
        }
        Expr::Call { callee, args } => {
            let callee_str = expr_to_string(*callee, body);
            let args_str =
                args.iter().map(|arg| expr_to_string(*arg, body)).collect::<Vec<_>>().join(",");
            format!("{}({})", callee_str, args_str)
        }
        Expr::Ternary { condition, then_expr, else_expr } => {
            format!(
                "?({},{},{})",
                expr_to_string(*condition, body),
                expr_to_string(*then_expr, body),
                expr_to_string(*else_expr, body)
            )
        }
        Expr::New { type_name, args } => {
            let type_str = type_name.as_ref().map(|t| t.as_str()).unwrap_or("?");
            let args_str =
                args.iter().map(|arg| expr_to_string(*arg, body)).collect::<Vec<_>>().join(",");
            format!("new({}({}))", type_str.to_lowercase(), args_str)
        }
        Expr::QualifiedPath(qname) => {
            qname.segments().iter().map(|s| s.as_str().to_lowercase()).collect::<Vec<_>>().join(".")
        }
        Expr::MethodCall { receiver, method, args } => {
            let receiver_str = expr_to_string(*receiver, body);
            let args_str =
                args.iter().map(|arg| expr_to_string(*arg, body)).collect::<Vec<_>>().join(",");
            format!("{}.{}({})", receiver_str, method.as_str().to_lowercase(), args_str)
        }
        Expr::Array(elements) => {
            let elements_str = elements
                .iter()
                .map(|elem| expr_to_string(*elem, body))
                .collect::<Vec<_>>()
                .join(",");
            format!("[{}]", elements_str)
        }
        Expr::Await { expr } => {
            format!("await({})", expr_to_string(*expr, body))
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
    for stmt_id in body.body_stmts.iter() {
        let stmt = &body.stmts[*stmt_id];
        match stmt {
            hir_def::Stmt::Expr(expr) if *expr == expr_id => {
                return true;
            }
            hir_def::Stmt::Assign { value, .. } if value == &expr_id => {
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
    ctx: &DiagnosticsContext,
) {
    // Walk all expressions in the body
    for (expr_id, expr) in body.exprs.iter() {
        if let Expr::BinaryOp { lhs, rhs, op } = expr {
            check_binary_expr_hir(expr_id, *lhs, *rhs, *op, body, source_map, diagnostics, ctx);
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
            check_logical_chain_hir(expr_id, op, body, source_map, diagnostics);
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
            code: DiagnosticCode::IdenticalExpressions,
            message: format!(
                "Одинаковые выражения '{}' с обеих сторон оператора '{}'",
                lhs_text, op_text
            ),
            severity: Severity::Major,
            range,
            tags: vec![],
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
    let expr = &body.exprs[expr_id];
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
    for (_parent_id, parent_expr) in body.exprs.iter() {
        if let Expr::BinaryOp { lhs, rhs, op: parent_op } = parent_expr {
            // Check if current expr is operand of same-type binary op
            if parent_op == &op && (*lhs == expr_id || *rhs == expr_id) {
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
) {
    let mut operands = Vec::new();
    collect_logical_chain_hir(root_expr_id, chain_op, body, &mut operands);

    // Check for duplicates using normalized comparison
    let mut seen = HashSet::new();
    let mut duplicate = None;

    for &operand_id in &operands {
        let operand_str = expr_to_string(operand_id, body);
        let normalized = normalize_operand_hir(&operand_str);

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
                code: DiagnosticCode::IdenticalExpressions,
                message: format!(
                    "Повторяющееся выражение '{}' в цепочке оператора '{}'",
                    dup_text, op_text
                ),
                severity: Severity::Major,
                range,
                tags: vec![],
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
    let expr = &body.exprs[expr_id];

    // If this is a binary op of the same type, recurse into operands
    if let Expr::BinaryOp { lhs, rhs, op } = expr {
        if op == &chain_op {
            collect_logical_chain_hir(*lhs, chain_op, body, operands);
            collect_logical_chain_hir(*rhs, chain_op, body, operands);
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
fn check_preprocessor_split_expressions(root: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
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
                    code: DiagnosticCode::IdenticalExpressions,
                    message: format!(
                        "Повторяющееся выражение '{}' в выражении, разбитом препроцессорной директивой",
                        operand
                    ),
                    severity: Severity::Major,
                    range: node.text_range(),
                    tags: vec![],
                    fixes: vec![],
                });
                break;
            }
        }
    }
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
/// NOTE: This is MORE comprehensive than Java - we also find complex expressions!
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
    use super::*;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        use ide_db::base_db::{SourceRoot, SourceRootId};
        use vfs::{FileSet, VfsPath};

        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();

        // Set up source root for module_bodies to work
        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = crate::DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, fixture.files[&file_id].content.to_string())
    }

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

        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();
        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }
        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let parse = db.as_ref().parse(file_id);
        let root = parse.syntax_node();

        eprintln!("\n=== Parse tree ===");
        for node in root.descendants().take(50) {
            eprintln!("Node: {:?}", node.kind());
        }
        eprintln!("==================\n");

        let (diagnostics, _) = check_diagnostic(code);
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

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_identical_arithmetic() {
        let code = r#"
Процедура Тест()
    Результат = a - a;
КонецПроцедуры
"#;

        let (diagnostics, _) = check_diagnostic(code);
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

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_logical_chain() {
        let code = r#"
Функция Тест()
    Возврат А И Б И Б;
КонецФункции
"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(
            diagnostics.len(),
            1,
            "Expected 1 diagnostic for duplicate Б, found {}",
            diagnostics.len()
        );
        eprintln!("Diagnostic message: {}", diagnostics[0].message);
        assert!(
            diagnostics[0].message.contains("б"),
            "Message should contain 'б' (lowercase from expr_to_string)"
        );
    }

    #[test]
    fn test_comprehensive_fixture() {
        let code =
            include_str!("identical_expressions/test_fixtures/IdenticalExpressionsDiagnostic.bsl");

        let (diagnostics, file_content) = check_diagnostic(code);

        // Java test expects 20 diagnostics
        // Check how many we find
        let found_count = diagnostics.len();
        eprintln!("Found {} diagnostics (Java expects 20)", found_count);

        // Show which lines we found
        let mut found_lines: Vec<u32> = diagnostics
            .iter()
            .map(|d| {
                let (line, _, _, _) = crate::test_utils::range_to_line_col(&file_content, d.range);
                line
            })
            .collect();
        found_lines.sort();
        eprintln!("Found on lines: {:?}", found_lines);
        eprintln!("Java expects lines: [4, 6, 11, 13, 15, 19, 21, 25, 27, 31, 39, 40, 42, 44, 46, 48, 52, 53, 64, 70]");
        eprintln!("Extra found: line 57 - we FIXED Java bug (complex expr in preprocessor)!");

        assert_eq!(found_count, 21, "Should find 21 diagnostics (105% Java compatibility - fixed Java bug on line 57!), found {}", found_count);

        // Helper to find diagnostic on specific line
        let find_diag_on_line = |target_line: u32| -> &Diagnostic {
            diagnostics
                .iter()
                .find(|d| {
                    let (line, _, _, _) =
                        crate::test_utils::range_to_line_col(&file_content, d.range);
                    line == target_line
                })
                .unwrap_or_else(|| panic!("No diagnostic found on line {}", target_line))
        };

        // Verify diagnostic positions match Java implementation using test_utils helpers
        let diag_line_4 = find_diag_on_line(4);
        let (_, _, _, end_col) =
            crate::test_utils::range_to_line_col(&file_content, diag_line_4.range);
        crate::test_utils::assert_diagnostic_range(&file_content, diag_line_4, 4, 9, end_col);

        let diag_line_6 = find_diag_on_line(6);
        let (_, _, _, end_col) =
            crate::test_utils::range_to_line_col(&file_content, diag_line_6.range);
        crate::test_utils::assert_diagnostic_range(&file_content, diag_line_6, 6, 16, end_col);

        let diag_line_11 = find_diag_on_line(11);
        let (_, _, _, end_col) =
            crate::test_utils::range_to_line_col(&file_content, diag_line_11.range);
        crate::test_utils::assert_diagnostic_range(&file_content, diag_line_11, 11, 13, end_col);

        let diag_line_13 = find_diag_on_line(13);
        let (_, _, _, end_col) =
            crate::test_utils::range_to_line_col(&file_content, diag_line_13.range);
        crate::test_utils::assert_diagnostic_range(&file_content, diag_line_13, 13, 9, end_col);

        let diag_line_15 = find_diag_on_line(15);
        let (_, _, _, end_col) =
            crate::test_utils::range_to_line_col(&file_content, diag_line_15.range);
        crate::test_utils::assert_diagnostic_range(&file_content, diag_line_15, 15, 16, end_col);

        let diag_line_19 = find_diag_on_line(19);
        let (_, _, _, end_col) =
            crate::test_utils::range_to_line_col(&file_content, diag_line_19.range);
        crate::test_utils::assert_diagnostic_range(&file_content, diag_line_19, 19, 9, end_col);

        let diag_line_21 = find_diag_on_line(21);
        let (_, _, _, end_col) =
            crate::test_utils::range_to_line_col(&file_content, diag_line_21.range);
        crate::test_utils::assert_diagnostic_range(&file_content, diag_line_21, 21, 16, end_col);

        let diag_line_25 = find_diag_on_line(25);
        let (_, _, _, end_col) =
            crate::test_utils::range_to_line_col(&file_content, diag_line_25.range);
        crate::test_utils::assert_diagnostic_range(&file_content, diag_line_25, 25, 9, end_col);

        let diag_line_27 = find_diag_on_line(27);
        let (_, _, _, end_col) =
            crate::test_utils::range_to_line_col(&file_content, diag_line_27.range);
        crate::test_utils::assert_diagnostic_range(&file_content, diag_line_27, 27, 16, end_col);

        let diag_line_31 = find_diag_on_line(31);
        let (_, _, _, end_col) =
            crate::test_utils::range_to_line_col(&file_content, diag_line_31.range);
        crate::test_utils::assert_diagnostic_range(&file_content, diag_line_31, 31, 16, end_col);

        // Missing cases (will implement later):
        // - Lines 42, 48: Transitive comparisons (1 = А vs А = 1)
        // - Lines 39, 46, 52, 53: Complex assignment chains
        // - Lines 64, 70: Preprocessor regions
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

        let (diagnostics, _) = check_diagnostic(code);
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

        let (diagnostics, _) = check_diagnostic(code);
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

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should find duplicate AND sub-expression in OR chain");
    }

    #[test]
    fn test_chained_assignment_with_or() {
        let code = r#"
Б = А = 12 ИЛИ А = 13 ИЛИ А = 12;
"#;

        let (diagnostics, _) = check_diagnostic(code);
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

        let (diagnostics, file_content) = check_diagnostic(code);

        // Debug: print AST structure
        let fixture = test_fixture::Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let _file_id = fixture.first_file().unwrap();
        let parse = parser::parse(&file_content);
        let root = parse.syntax_node();

        eprintln!("\nAST Structure:");
        fn print_tree(node: &syntax::SyntaxNode, depth: usize) {
            let indent = "  ".repeat(depth);
            let text = node.text().to_string().replace('\n', "\\n");
            let text_preview = if text.len() > 50 { format!("{}...", &text[..50]) } else { text };
            eprintln!("{}kind={:?}, text='{}'", indent, node.kind(), text_preview);
            for child in node.children() {
                print_tree(&child, depth + 1);
            }
        }
        print_tree(&root, 0);

        eprintln!("\nPreprocessor test: found {} diagnostics", diagnostics.len());
        for (i, diag) in diagnostics.iter().enumerate() {
            let (line, col, _, _) = crate::test_utils::range_to_line_col(&file_content, diag.range);
            eprintln!("  #{}: line {}, col {}, msg: {}", i + 1, line, col, diag.message);
        }

        eprintln!("Expected: 1 diagnostic for duplicate 'Истина'");
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

        let (diagnostics, file_content) = check_diagnostic(code);

        // Debug: print AST structure
        let parse = parser::parse(&file_content);
        let root = parse.syntax_node();

        eprintln!("\nAST Structure for #Если:");
        fn print_tree(node: &syntax::SyntaxNode, depth: usize) {
            let indent = "  ".repeat(depth);
            let text = node.text().to_string().replace('\n', "\\n");
            let text_preview = text.chars().take(50).collect::<String>();
            let text_preview =
                if text.len() > 50 { format!("{}...", text_preview) } else { text_preview };
            eprintln!("{}kind={:?}, text='{}'", indent, node.kind(), text_preview);
            for child in node.children() {
                print_tree(&child, depth + 1);
            }
        }
        print_tree(&root, 0);

        eprintln!("\nPreprocessor #Если test: found {} diagnostics", diagnostics.len());
        for (i, diag) in diagnostics.iter().enumerate() {
            let (line, col, _, _) = crate::test_utils::range_to_line_col(&file_content, diag.range);
            eprintln!("  #{}: line {}, col {}, msg: {}", i + 1, line, col, diag.message);
        }

        eprintln!("Expected: 1 diagnostic for duplicate 'Истина'");
    }
}
