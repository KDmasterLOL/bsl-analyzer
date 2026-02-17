//! TransferringParametersBetweenClientAndServer diagnostic.
//!
//! Detects by-reference parameters in server methods that are not assigned
//! and are called from client methods.
//!
//! ## Why?
//!
//! When parameters are passed between client and server boundaries, they are
//! transmitted as copies. If a parameter is declared without "Знач" (ByValue),
//! changes made on the server are transmitted back to the client. If the
//! parameter is NOT modified inside the server method, there's no reason to
//! transmit it back—this wastes bandwidth and degrades performance.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use cfg_types::IdConversion;
use hir::{AnnotationKind, Expr, Name, Stmt};
use rustc_hash::FxHashSet;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Performance, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Adaptable,
};

const SERVER_ANNOTATIONS: &[AnnotationKind] =
    &[AnnotationKind::AtServer, AnnotationKind::AtServerNoContext];

const CLIENT_ANNOTATION: AnnotationKind = AnnotationKind::AtClient;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::TransferringParametersBetweenClientAndServer;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let symbol_tree = ctx.symbol_tree();
    let module_bodies = ctx.module_bodies();
    let mut diagnostics = Vec::new();

    for method in symbol_tree.methods() {
        let has_server_annotation =
            method.annotations.iter().any(|ann| SERVER_ANNOTATIONS.contains(&ann.kind));

        if !has_server_annotation {
            continue;
        }

        let by_ref_params: Vec<_> =
            method.params.iter().enumerate().filter(|(_, param)| !param.is_val).collect();

        if by_ref_params.is_empty() {
            continue;
        }

        let local_id = method.id.local_id;
        let Some(lower_result) = module_bodies.lower_result(local_id) else {
            continue;
        };

        let body = &lower_result.body;

        let assigned_params = collect_assigned_params(body);

        for (param_idx, param) in by_ref_params {
            let param_name_lower = param.name.as_str().to_lowercase();

            if assigned_params.contains(&param_name_lower) {
                continue;
            }

            if has_client_calls(ctx, &method.name) {
                let item_tree = ctx.item_tree();
                let mut param_range = None;

                for item in item_tree.top_level_items() {
                    match item {
                        hir_def::item_tree::ModItem::Procedure(proc_idx) => {
                            let proc = item_tree.procedure(*proc_idx);
                            if proc.name == method.name {
                                if let Some(param_info) = proc.params.get(param_idx) {
                                    param_range = Some(param_info.name_range);
                                }
                                break;
                            }
                        }
                        hir_def::item_tree::ModItem::Function(func_idx) => {
                            let func = item_tree.function(*func_idx);
                            if func.name == method.name {
                                if let Some(param_info) = func.params.get(param_idx) {
                                    param_range = Some(param_info.name_range);
                                }
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                if let Some(range) = param_range {
                    diagnostics.push(Diagnostic {
                        code,
                        message: format!(
                            "Установите модификатор \"Знач\" для параметра {} метода {}",
                            param.name.as_str(),
                            method.name.as_str()
                        ),
                        severity: ctx.severity(code),
                        range,
                        tags: ctx.tags(code),
                        fixes: vec![],
                    });
                }
            }
        }
    }

    diagnostics
}

fn collect_assigned_params(body: &hir_def::Body) -> FxHashSet<String> {
    let mut assigned = FxHashSet::default();

    for (_stmt_id, stmt) in body.stmts_iter() {
        collect_assigned_from_stmt(stmt, body, &mut assigned);
    }

    assigned
}

fn collect_assigned_from_stmt(stmt: &Stmt, body: &hir_def::Body, assigned: &mut FxHashSet<String>) {
    match stmt {
        Stmt::Assign { target, .. } => {
            let target_id = hir_def::ExprId::from_idx(*target);
            if let Expr::Path(name) = body.expr(target_id) {
                assigned.insert(name.as_str().to_lowercase());
            }
        }
        Stmt::If(if_stmt) => {
            for &stmt_idx in if_stmt.then_branch.iter() {
                let stmt_id = hir_def::StmtId::from_idx(stmt_idx);
                collect_assigned_from_stmt(body.stmt(stmt_id), body, assigned);
            }
            for (_, elsif_stmts) in if_stmt.elsif_branches.iter() {
                for &stmt_idx in elsif_stmts.iter() {
                    let stmt_id = hir_def::StmtId::from_idx(stmt_idx);
                    collect_assigned_from_stmt(body.stmt(stmt_id), body, assigned);
                }
            }
            if let Some(ref else_branch) = if_stmt.else_branch {
                for &stmt_idx in else_branch.iter() {
                    let stmt_id = hir_def::StmtId::from_idx(stmt_idx);
                    collect_assigned_from_stmt(body.stmt(stmt_id), body, assigned);
                }
            }
        }
        Stmt::While { body: stmts, .. }
        | Stmt::For { body: stmts, .. }
        | Stmt::ForEach { body: stmts, .. } => {
            for &stmt_idx in stmts.iter() {
                let stmt_id = hir_def::StmtId::from_idx(stmt_idx);
                collect_assigned_from_stmt(body.stmt(stmt_id), body, assigned);
            }
        }
        Stmt::Try { body: try_block, except, .. } => {
            for &stmt_idx in try_block.iter() {
                let stmt_id = hir_def::StmtId::from_idx(stmt_idx);
                collect_assigned_from_stmt(body.stmt(stmt_id), body, assigned);
            }
            for &stmt_idx in except.iter() {
                let stmt_id = hir_def::StmtId::from_idx(stmt_idx);
                collect_assigned_from_stmt(body.stmt(stmt_id), body, assigned);
            }
        }
        _ => {}
    }
}

fn has_client_calls(ctx: &DiagnosticsContext, server_method_name: &Name) -> bool {
    let symbol_tree = ctx.symbol_tree();
    let module_bodies = ctx.module_bodies();
    let target_lower = server_method_name.as_str().to_lowercase();

    for method in symbol_tree.methods() {
        let is_client = method.annotations.iter().any(|ann| ann.kind == CLIENT_ANNOTATION);

        if !is_client {
            continue;
        }

        let local_id = method.id.local_id;
        let Some(lower_result) = module_bodies.lower_result(local_id) else {
            continue;
        };

        let body = &lower_result.body;

        if has_call_to(body, &target_lower) {
            return true;
        }
    }

    false
}

fn has_call_to(body: &hir_def::Body, target_method_lower: &str) -> bool {
    for (_expr_id, expr) in body.exprs_iter() {
        if check_expr_for_call(expr, target_method_lower, body) {
            return true;
        }
    }

    false
}

fn check_expr_for_call(expr: &Expr, target_method_lower: &str, body: &hir_def::Body) -> bool {
    match expr {
        Expr::Call { callee, args } => {
            let callee_id = hir_def::ExprId::from_idx(*callee);
            if let Expr::Path(name) = body.expr(callee_id) {
                if name.as_str().to_lowercase() == target_method_lower {
                    return true;
                }
            }
            for &arg_idx in args.iter() {
                let arg_id = hir_def::ExprId::from_idx(arg_idx);
                if check_expr_for_call(body.expr(arg_id), target_method_lower, body) {
                    return true;
                }
            }
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            let lhs_id = hir_def::ExprId::from_idx(*lhs);
            let rhs_id = hir_def::ExprId::from_idx(*rhs);
            return check_expr_for_call(body.expr(lhs_id), target_method_lower, body)
                || check_expr_for_call(body.expr(rhs_id), target_method_lower, body);
        }
        Expr::UnaryOp { expr: inner, .. } | Expr::Await { expr: inner } => {
            let inner_id = hir_def::ExprId::from_idx(*inner);
            return check_expr_for_call(body.expr(inner_id), target_method_lower, body);
        }
        Expr::Ternary { condition, then_expr, else_expr } => {
            let condition_id = hir_def::ExprId::from_idx(*condition);
            let then_id = hir_def::ExprId::from_idx(*then_expr);
            let else_id = hir_def::ExprId::from_idx(*else_expr);
            return check_expr_for_call(body.expr(condition_id), target_method_lower, body)
                || check_expr_for_call(body.expr(then_id), target_method_lower, body)
                || check_expr_for_call(body.expr(else_id), target_method_lower, body);
        }
        Expr::MethodCall { receiver, args, .. } => {
            let receiver_id = hir_def::ExprId::from_idx(*receiver);
            if check_expr_for_call(body.expr(receiver_id), target_method_lower, body) {
                return true;
            }
            for &arg_idx in args.iter() {
                let arg_id = hir_def::ExprId::from_idx(arg_idx);
                if check_expr_for_call(body.expr(arg_id), target_method_lower, body) {
                    return true;
                }
            }
        }
        Expr::Index { base, index } => {
            let base_id = hir_def::ExprId::from_idx(*base);
            let index_id = hir_def::ExprId::from_idx(*index);
            return check_expr_for_call(body.expr(base_id), target_method_lower, body)
                || check_expr_for_call(body.expr(index_id), target_method_lower, body);
        }
        Expr::Field { base, .. } => {
            let base_id = hir_def::ExprId::from_idx(*base);
            return check_expr_for_call(body.expr(base_id), target_method_lower, body);
        }
        Expr::New { args, .. } => {
            for &arg_idx in args.iter() {
                let arg_id = hir_def::ExprId::from_idx(arg_idx);
                if check_expr_for_call(body.expr(arg_id), target_method_lower, body) {
                    return true;
                }
            }
        }
        Expr::Array(exprs) => {
            for &expr_idx in exprs.iter() {
                let opaque_id = hir_def::ExprId::from_idx(expr_idx);
                if check_expr_for_call(body.expr(opaque_id), target_method_lower, body) {
                    return true;
                }
            }
        }
        _ => {}
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    #[test]
    fn test_by_ref_param_in_server_method_called_from_client() {
        let code = include_str!(
            "../../test_data/TransferringParametersBetweenClientAndServerDiagnostic.bsl"
        );

        let diagnostics = check_ast_diagnostic(code, check);
        let target_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::TransferringParametersBetweenClientAndServer)
            .collect();

        assert_eq!(target_diags.len(), 1, "Expected 1 diagnostic, got {}", target_diags.len());

        assert_diagnostic_range(code, target_diags[0], 6, 18, 24);
    }

    #[test]
    fn test_no_diagnostic_for_by_value_param() {
        let code = r#"
&НаКлиенте
Процедура Клиент()
    Сервер(2);
КонецПроцедуры

&НаСервере
Процедура Сервер(Знач Парам)
    Результат = Парам + 1;
КонецПроцедуры
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        let target_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::TransferringParametersBetweenClientAndServer)
            .collect();

        assert_eq!(target_diags.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_when_param_assigned() {
        let code = r#"
&НаКлиенте
Процедура Клиент()
    Сервер(2);
КонецПроцедуры

&НаСервере
Процедура Сервер(Парам)
    Парам = 10;
КонецПроцедуры
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        let target_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::TransferringParametersBetweenClientAndServer)
            .collect();

        assert_eq!(target_diags.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_when_not_called_from_client() {
        let code = r#"
&НаКлиентеНаСервереБезКонтекста
Процедура КлиентСервер()
    Сервер(2);
КонецПроцедуры

&НаСервере
Процедура Сервер(Парам)
    Результат = Парам + 1;
КонецПроцедуры
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        let target_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::TransferringParametersBetweenClientAndServer)
            .collect();

        assert_eq!(target_diags.len(), 0);
    }
}
