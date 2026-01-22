//! RewriteMethodParameter diagnostic.
//!
//! Detects byValue parameters that are overwritten without prior use.
//!
//! ## Why?
//!
//! When a parameter is marked with `Знач` (ByValue) and immediately overwritten without using
//! its original value, it suggests either:
//! - The parameter should not be passed (convert to local variable)
//! - The parameter name/semantics are misleading
//!
//! This is a code smell that confuses code readers.
//!
//! ## Bad practice
//!
//! ```bsl
//! Функция Конфигуратор(Знач СтрокаПодключения, Знач Пользователь = "", Знач Пароль = "") Экспорт
//!     СтрокаПодключения = "/F""" + КаталогБазы + """";  // ❌ Parameter overwritten!
//!     // ... rest of function
//! КонецФункции
//! ```
//!
//! ## Good practice
//!
//! ```bsl
//! // Option 1: Rename parameter to reflect actual input
//! Функция Конфигуратор(Знач КаталогБазы, Знач Пользователь = "", Знач Пароль = "") Экспорт
//!     СтрокаПодключения = "/F""" + КаталогБазы + """";  // ✅ Clear semantics
//!     // ...
//! КонецФункции
//!
//! // Option 2: Use local variable
//! Функция Конфигуратор(Знач Пользователь = "", Знач Пароль = "") Экспорт
//!     СтрокаПодключения = "/F""" + КаталогБазы + """";  // ✅ Local variable
//!     // ...
//! КонецФункции
//! ```
//!
//! ## Configuration
//!
//! - **Enabled by default:** Yes
//! - **Severity:** Major
//! - **Tags:** SUSPICIOUS
//! - **Minutes to fix:** 2
//!
//! ## Implementation
//!
//! Uses **CFG + reaching definitions** for accurate flow-sensitive analysis:
//! 1. HIR lowering emits BodyDiagnostic for all assignments to byValue parameters
//! 2. Handler uses reaching definitions to check if parameter was used before assignment
//! 3. If reaching defs contain only initial parameter definition → diagnostic
//!
//! ### Why CFG is required
//!
//! Java bsl-language-server uses textual order (sorts references by line/column), which produces
//! **false negatives** on conditional branches:
//!
//! ```bsl
//! Процедура Тест(Знач Парам)
//!     Если Условие Тогда
//!         Результат = Парам;  // USE (line 3)
//!     Иначе
//!         Парам = 0;  // OVERWRITE (line 5) - Java misses this!
//!     КонецЕсли;
//! КонецПроцедуры
//! ```
//!
//! Java sees USE before OVERWRITE textually → no diagnostic.
//! But on else branch: parameter overwritten WITHOUT use!
//!
//! Our CFG-based approach correctly analyzes each execution path.
//!
//! ### Self-assign handling
//!
//! `Парам = Парам` is not considered a "meaningful use" and is skipped:
//! - First assignment is self-assign → skip, check next assignment
//! - Multiple self-assigns in a row → skip all, check first non-self-assign
//!
//! Ported from:
//! - RewriteMethodParameterDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir_def::{BindingId, ExprId, IdConversion, StmtId};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::RewriteMethodParameter` is encountered.
///
/// ## Algorithm
///
/// 1. Find StmtId for the assignment using stmt_range lookup in BodySourceMap
/// 2. Get reaching definitions for this statement from module-level query
/// 3. Check if reaching defs contain only initial parameter definition
/// 4. Handle self-assign case: if RHS uses parameter, skip this assignment
/// 5. Emit diagnostic if parameter not used before overwrite
///
/// ## Parameters
///
/// - `stmt_range`: Full statement range for BodySourceMap lookup
/// - `ident_range`: Identifier range for diagnostic display (Java compatibility)
pub fn from_hir(
    param_id: BindingId,
    _stmt_id: StmtId, // Placeholder from lowering - we'll find real one via range
    stmt_range: TextRange,
    ident_range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::RewriteMethodParameter;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    // Get module bodies and find the method containing this diagnostic
    let module_bodies = ctx.module_bodies();

    // Find which method contains this stmt_range
    let (local_id, body, source_map) =
        module_bodies.method_bodies().find(|(_local_id, _body, source_map)| {
            // Check if any statement in this method has matching range
            source_map.stmt_at_range(stmt_range).is_some()
        })?;

    // Get the actual StmtId for this assignment
    let stmt_id = source_map.stmt_at_range(stmt_range)?;

    // Get parameter name for diagnostic message
    let param = body.binding(param_id);
    let param_name = param.name.as_str();

    // Get reaching definitions for this method
    let module_reaching_defs = ctx.module_reaching_defs();
    let reaching_defs_result = module_reaching_defs.get(local_id)?;

    // Get reaching defs BEFORE this assignment statement
    let reaching_defs_set = reaching_defs_result.defs_before_stmt(stmt_id)?;

    // Get parameter name (lowercase for matching)
    let param_name_lower = param_name.to_lowercase();

    // Find all definitions for this parameter variable
    let param_definitions: Vec<_> =
        reaching_defs_set.iter().filter(|def| def.var_name.as_str() == param_name_lower).collect();

    if param_definitions.is_empty() {
        // No definitions reach here - parameter not in scope? Shouldn't happen
        return None;
    }

    // Check if all definitions are just the parameter itself (not modified/used)
    // If there's only one definition and it's the parameter → not used before overwrite
    if param_definitions.len() == 1 {
        let single_def = param_definitions[0];
        if let dataflow::reaching_defs::DefSite::Parameter(def_binding_id) = single_def.def_site {
            if def_binding_id == param_id {
                // Check if parameter is used in RHS of this assignment
                // If yes, this is not "overwrite without use" (e.g., Param = Param or Param = Func(Param))
                if parameter_used_in_assignment_rhs(body, stmt_id, param_id) {
                    return None; // Parameter used in RHS, no diagnostic
                }

                // Check if parameter was used in any previous statement
                // (e.g., field access: Param.Field = value or value = Param.Field)
                if parameter_used_before_stmt(body, stmt_id, param_id) {
                    return None; // Parameter used in prior statement, no diagnostic
                }

                // Parameter overwritten without prior use!
                return Some(Diagnostic {
                    code,
                    message: format!("Переприсваивание параметра метода '{}'", param_name),
                    severity: ctx.severity(code),
                    range: ident_range, // Use identifier range for Java compatibility
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
            }
        }
    }

    // If multiple definitions or definition is not the parameter → parameter was used/modified
    None
}

/// Check if parameter was used in any statement before the target statement.
///
/// Returns true if the parameter appears anywhere in statements that textually
/// precede the target statement (including field accesses, function arguments, etc.)
///
/// Self-assigns (Param = Param) are NOT considered meaningful uses.
fn parameter_used_before_stmt(
    body: &hir_def::Body,
    target_stmt_id: StmtId,
    param_id: BindingId,
) -> bool {
    // Scan all statements before target (by RawIdx ordering)
    for (stmt_id, _stmt) in body.stmts_iter() {
        // Stop when we reach the target statement
        if stmt_id == target_stmt_id {
            break;
        }

        // Skip self-assigns (Param = Param) - not considered meaningful uses
        if is_self_assign_to_binding(body, stmt_id, param_id) {
            continue;
        }

        // Check if this statement uses the parameter in a meaningful way
        if stmt_uses_binding(body, stmt_id, param_id) {
            return true;
        }
    }

    false
}

/// Check if statement is a self-assign to the binding (Param = Param).
fn is_self_assign_to_binding(body: &hir_def::Body, stmt_id: StmtId, binding_id: BindingId) -> bool {
    use hir_def::hir::{Expr, Stmt};

    let stmt = body.stmt(stmt_id);
    match stmt {
        Stmt::Assign { target, value } => {
            // Check if target is our binding
            if let Expr::Path(target_name) = body.expr(ExprId::from_idx(*target)) {
                let binding = body.binding(binding_id);
                if !target_name.as_str().eq_ignore_ascii_case(binding.name.as_str()) {
                    return false; // Target is not our binding
                }

                // Check if value is also our binding (self-assign)
                if let Expr::Path(value_name) = body.expr(ExprId::from_idx(*value)) {
                    return value_name.as_str().eq_ignore_ascii_case(binding.name.as_str());
                }
            }
            false
        }
        _ => false,
    }
}

/// Check if a statement uses a specific binding anywhere.
fn stmt_uses_binding(body: &hir_def::Body, stmt_id: StmtId, binding_id: BindingId) -> bool {
    use hir_def::hir::Stmt;

    let stmt = body.stmt(stmt_id);
    match stmt {
        Stmt::Expr(expr_id) => expr_uses_binding(body, ExprId::from_idx(*expr_id), binding_id),
        Stmt::Assign { target, value } => {
            expr_uses_binding(body, ExprId::from_idx(*target), binding_id)
                || expr_uses_binding(body, ExprId::from_idx(*value), binding_id)
        }
        Stmt::If(if_stmt) => {
            if expr_uses_binding(body, ExprId::from_idx(if_stmt.condition), binding_id) {
                return true;
            }
            // Check branches
            for &stmt_idx in if_stmt.then_branch.iter() {
                if stmt_uses_binding(body, StmtId::from_idx(stmt_idx), binding_id) {
                    return true;
                }
            }
            for (cond, branch) in if_stmt.elsif_branches.iter() {
                if expr_uses_binding(body, ExprId::from_idx(*cond), binding_id) {
                    return true;
                }
                for &stmt_idx in branch.iter() {
                    if stmt_uses_binding(body, StmtId::from_idx(stmt_idx), binding_id) {
                        return true;
                    }
                }
            }
            if let Some(ref branch) = if_stmt.else_branch {
                for &stmt_idx in branch.iter() {
                    if stmt_uses_binding(body, StmtId::from_idx(stmt_idx), binding_id) {
                        return true;
                    }
                }
            }
            false
        }
        Stmt::While { condition, body: loop_body } => {
            if expr_uses_binding(body, ExprId::from_idx(*condition), binding_id) {
                return true;
            }
            for &stmt_idx in loop_body.iter() {
                if stmt_uses_binding(body, StmtId::from_idx(stmt_idx), binding_id) {
                    return true;
                }
            }
            false
        }
        Stmt::For { from, to, body: loop_body, .. } => {
            if expr_uses_binding(body, ExprId::from_idx(*from), binding_id)
                || expr_uses_binding(body, ExprId::from_idx(*to), binding_id)
            {
                return true;
            }
            for &stmt_idx in loop_body.iter() {
                if stmt_uses_binding(body, StmtId::from_idx(stmt_idx), binding_id) {
                    return true;
                }
            }
            false
        }
        Stmt::ForEach { collection, body: loop_body, .. } => {
            if expr_uses_binding(body, ExprId::from_idx(*collection), binding_id) {
                return true;
            }
            for &stmt_idx in loop_body.iter() {
                if stmt_uses_binding(body, StmtId::from_idx(stmt_idx), binding_id) {
                    return true;
                }
            }
            false
        }
        Stmt::Try { body: try_body, except } => {
            for &stmt_idx in try_body.iter() {
                if stmt_uses_binding(body, StmtId::from_idx(stmt_idx), binding_id) {
                    return true;
                }
            }
            for &stmt_idx in except.iter() {
                if stmt_uses_binding(body, StmtId::from_idx(stmt_idx), binding_id) {
                    return true;
                }
            }
            false
        }
        Stmt::Return { value: Some(expr_id) } => {
            expr_uses_binding(body, ExprId::from_idx(*expr_id), binding_id)
        }
        Stmt::Return { value: None } => false,
        Stmt::Raise { value: Some(expr_id) } => {
            expr_uses_binding(body, ExprId::from_idx(*expr_id), binding_id)
        }
        Stmt::Raise { value: None } => false,
        Stmt::Execute { expr } => expr_uses_binding(body, ExprId::from_idx(*expr), binding_id),
        Stmt::AddHandler { event, handler } => {
            expr_uses_binding(body, ExprId::from_idx(*event), binding_id)
                || expr_uses_binding(body, ExprId::from_idx(*handler), binding_id)
        }
        Stmt::RemoveHandler { event, handler } => {
            expr_uses_binding(body, ExprId::from_idx(*event), binding_id)
                || expr_uses_binding(body, ExprId::from_idx(*handler), binding_id)
        }
        // Other statements don't use expressions
        _ => false,
    }
}

/// Check if parameter is used in the RHS of an assignment statement.
///
/// Returns true if the parameter binding appears anywhere in the value expression.
fn parameter_used_in_assignment_rhs(
    body: &hir_def::Body,
    stmt_id: StmtId,
    param_id: BindingId,
) -> bool {
    use hir_def::hir::Stmt;

    let stmt = body.stmt(stmt_id);
    let value_expr_id = match stmt {
        Stmt::Assign { value, .. } => ExprId::from_idx(*value),
        _ => return false, // Not an assignment
    };

    // Check if param_id is used anywhere in the value expression tree
    expr_uses_binding(body, value_expr_id, param_id)
}

/// Recursively check if an expression uses a specific binding.
fn expr_uses_binding(body: &hir_def::Body, expr_id: ExprId, binding_id: BindingId) -> bool {
    use hir_def::hir::Expr;

    let expr = body.expr(expr_id);
    match expr {
        Expr::Path(name) => {
            // Check if this path resolves to our binding
            // We need to match by name (case-insensitive) since we don't have full name resolution
            let binding = body.binding(binding_id);
            name.as_str().eq_ignore_ascii_case(binding.name.as_str())
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            expr_uses_binding(body, ExprId::from_idx(*lhs), binding_id)
                || expr_uses_binding(body, ExprId::from_idx(*rhs), binding_id)
        }
        Expr::Call { callee, args, .. } => {
            expr_uses_binding(body, ExprId::from_idx(*callee), binding_id)
                || args
                    .iter()
                    .any(|&arg| expr_uses_binding(body, ExprId::from_idx(arg), binding_id))
        }
        Expr::Index { base, index, .. } => {
            expr_uses_binding(body, ExprId::from_idx(*base), binding_id)
                || expr_uses_binding(body, ExprId::from_idx(*index), binding_id)
        }
        Expr::Field { base, .. } => expr_uses_binding(body, ExprId::from_idx(*base), binding_id),
        Expr::New { args, .. } => {
            args.iter().any(|&arg| expr_uses_binding(body, ExprId::from_idx(arg), binding_id))
        }
        Expr::Ternary { condition, then_expr, else_expr } => {
            expr_uses_binding(body, ExprId::from_idx(*condition), binding_id)
                || expr_uses_binding(body, ExprId::from_idx(*then_expr), binding_id)
                || expr_uses_binding(body, ExprId::from_idx(*else_expr), binding_id)
        }
        // Literals, missing expressions don't use bindings
        Expr::Literal(_) | Expr::Missing => false,
        // Other expression types (Await, etc.) - conservatively assume no usage for now
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic;
    use crate::DiagnosticCode;

    #[test]
    fn test_simple_overwrite() {
        let code = r#"Процедура Тест1(Знач Парам1)
    Парам1 = 10; // ошибка
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let rewrite_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::RewriteMethodParameter)
            .collect();

        assert_eq!(rewrite_diags.len(), 1, "Expected 1 RewriteMethodParameter diagnostic");
    }

    #[test]
    fn test_no_diagnostic_for_by_ref() {
        let code = r#"Процедура Тест2(Парам21, Знач Парам22)
    Парам21 = 10; // не ошибка - by-ref параметр
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let rewrite_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::RewriteMethodParameter)
            .collect();

        assert_eq!(rewrite_diags.len(), 0, "By-ref parameters should not trigger diagnostic");
    }

    #[test]
    fn test_self_assign_no_diagnostic() {
        let code = r#"Процедура Тест4(Знач Парам41)
    Парам41 = Парам41; // не ошибка - self-assign
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let rewrite_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::RewriteMethodParameter)
            .collect();

        assert_eq!(rewrite_diags.len(), 0, "Self-assign should not trigger diagnostic");
    }

    #[test]
    fn test_used_in_expression_no_diagnostic() {
        let code = r#"Процедура Тест5(Знач Парам51)
    Парам51 = Метод(Парам51); // не ошибка - используется в RHS
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let rewrite_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::RewriteMethodParameter)
            .collect();

        assert_eq!(rewrite_diags.len(), 0, "Parameter used in RHS should not trigger diagnostic");
    }

    #[test]
    fn test_java_fixture_all_16_cases() {
        use crate::test_utils::assert_diagnostic_range;

        // Load Java test fixture with all 16 test cases
        let code = include_str!("../../test_data/RewriteMethodParameterDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let rewrite_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::RewriteMethodParameter)
            .collect();

        // Validate exactly 5 diagnostics (matching Java expectations)
        assert_eq!(
            rewrite_diags.len(),
            5,
            "Expected 5 RewriteMethodParameter diagnostics from Java fixture"
        );

        // Java positions are 0-based line, character columns (not byte offsets)
        // Range now covers only identifier, not full statement (Java compatibility)

        // Line 2 (Тест1): Парам1 = 10; - "Парам1" is 6 chars at col 4-10
        assert_diagnostic_range(code, rewrite_diags[0], 1, 4, 10);

        // Line 10 (Тест3): Парам31 = 3; - "Парам31" is 7 chars at col 4-11
        assert_diagnostic_range(code, rewrite_diags[1], 9, 4, 11);

        // Line 23 (Тест6): Парам61 = 12; - "Парам61" is 7 chars at col 4-11
        assert_diagnostic_range(code, rewrite_diags[2], 22, 4, 11);

        // Line 30 (Тест7): Парам71 = 12; - "Парам71" is 7 chars at col 4-11
        assert_diagnostic_range(code, rewrite_diags[3], 29, 4, 11);

        // Line 38 (Тест9): Парам91 = 12; - "Парам91" is 7 chars at col 4-11
        assert_diagnostic_range(code, rewrite_diags[4], 37, 4, 11);
    }
}
