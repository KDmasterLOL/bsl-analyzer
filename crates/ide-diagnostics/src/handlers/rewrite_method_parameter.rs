use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{BindingId, ExprId, IdConversion, StmtId};
use ide_db::TextRange;
use stdx::case::CaseExt;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    param_id: BindingId,
    _stmt_id: StmtId,
    stmt_range: TextRange,
    ident_range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::RewriteMethodParameter;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let module_bodies = ctx.module_bodies();

    let (local_id, body, source_map) = module_bodies
        .method_bodies()
        .find(|(_local_id, _body, source_map)| source_map.stmt_at_range(stmt_range).is_some())?;

    let stmt_id = source_map.stmt_at_range(stmt_range)?;

    let param = body.binding(param_id);
    let param_name = param.name.as_str();

    let module_reaching_defs = ctx.module_reaching_defs();
    let reaching_defs_result = module_reaching_defs.get(local_id)?;

    let reaching_defs_set = reaching_defs_result.defs_before_stmt(stmt_id)?;

    let param_name_lower = param_name.fold_lower();

    let param_definitions: Vec<_> =
        reaching_defs_set.iter().filter(|def| def.var_name.as_str() == param_name_lower).collect();

    if param_definitions.is_empty() {
        return None;
    }

    if param_definitions.len() == 1 {
        let single_def = param_definitions[0];
        if let hir::dataflow::reaching_defs::DefSite::Parameter(def_binding_id) =
            single_def.def_site
        {
            if def_binding_id == param_id {
                if parameter_used_in_assignment_rhs(body, stmt_id, param_id) {
                    return None;
                }

                if parameter_used_before_stmt(body, stmt_id, param_id) {
                    return None;
                }

                return Some(Diagnostic {
                    code,
                    message: format!("Переприсваивание параметра метода '{}'", param_name),
                    severity: ctx.severity(code),
                    range: ident_range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
            }
        }
    }

    None
}

fn parameter_used_before_stmt(
    body: &hir::Body,
    target_stmt_id: StmtId,
    param_id: BindingId,
) -> bool {
    for (stmt_id, _stmt) in body.stmts_iter() {
        if stmt_id == target_stmt_id {
            break;
        }

        if is_self_assign_to_binding(body, stmt_id, param_id) {
            continue;
        }

        if stmt_uses_binding(body, stmt_id, param_id) {
            return true;
        }
    }

    false
}

fn is_self_assign_to_binding(body: &hir::Body, stmt_id: StmtId, binding_id: BindingId) -> bool {
    use hir::{Expr, Stmt};

    let stmt = body.stmt(stmt_id);
    match stmt {
        Stmt::Assign { target, value } => {
            if let Expr::Path(target_name) = body.expr(ExprId::from_idx(*target)) {
                let binding = body.binding(binding_id);
                if !target_name.as_str().eq_ignore_ascii_case(binding.name.as_str()) {
                    return false;
                }

                if let Expr::Path(value_name) = body.expr(ExprId::from_idx(*value)) {
                    return value_name.as_str().eq_ignore_ascii_case(binding.name.as_str());
                }
            }
            false
        }
        _ => false,
    }
}

fn stmt_uses_binding(body: &hir::Body, stmt_id: StmtId, binding_id: BindingId) -> bool {
    use hir::Stmt;

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
        _ => false,
    }
}

fn parameter_used_in_assignment_rhs(
    body: &hir::Body,
    stmt_id: StmtId,
    param_id: BindingId,
) -> bool {
    use hir::Stmt;

    let stmt = body.stmt(stmt_id);
    let value_expr_id = match stmt {
        Stmt::Assign { value, .. } => ExprId::from_idx(*value),
        _ => return false,
    };

    expr_uses_binding(body, value_expr_id, param_id)
}

fn expr_uses_binding(body: &hir::Body, expr_id: ExprId, binding_id: BindingId) -> bool {
    use hir::Expr;

    let expr = body.expr(expr_id);
    match expr {
        Expr::Path(name) => {
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
        Expr::Literal(_) | Expr::Missing => false,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{check_diagnostics_snapshot_for, format_diags};
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_simple_overwrite() {
        let code = r#"Процедура Тест1(Знач Парам1)
    Парам1 = 10; // ошибка
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RewriteMethodParameter,
            expect![[r#"
            RewriteMethodParameter @ 2:5..2:11
              message: Переприсваивание параметра метода 'Парам1'
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_no_diagnostic_for_by_ref() {
        let code = r#"Процедура Тест2(Парам21, Знач Парам22)
    Парам21 = 10; // не ошибка - by-ref параметр
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RewriteMethodParameter,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_self_assign_no_diagnostic() {
        let code = r#"Процедура Тест4(Знач Парам41)
    Парам41 = Парам41; // не ошибка - self-assign
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RewriteMethodParameter,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_used_in_expression_no_diagnostic() {
        let code = r#"Процедура Тест5(Знач Парам51)
    Парам51 = Метод(Парам51); // не ошибка - используется в RHS
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RewriteMethodParameter,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_java_fixture_all_16_cases() {
        let code = r#"Процедура Тест1(Знач Парам1)
    Парам1 = 10; // ошибка
КонецПроцедуры

Процедура Тест2(Парам21, Знач Парам22)
    Парам21 = 10; // не ошибка
КонецПроцедуры

Процедура Тест3(Знач Парам31 = 1, Знач Парам32 = 2 )
    Парам31 = 3; // ошибка
КонецПроцедуры

Процедура Тест4(Знач Парам41)
    Парам41 = Парам41; // не ошибка
КонецПроцедуры

Процедура Тест5(Знач Парам51)
    Парам51 = Метод(Парам51); // не ошибка
КонецПроцедуры

Процедура Тест6(Знач Парам61)
    Парам61 = Парам61;
    Парам61 = 12; // ошибка
КонецПроцедуры

Процедура Тест7(Знач Парам71)
    Парам71 = Парам71;
    Парам71 = Парам71;
    Парам71 = Парам71;
    Парам71 = 12; // ошибка
КонецПроцедуры

Процедура Тест8(Знач Парам81) // для покрытия
    ЛокальнаяПеременная = 10;
КонецПроцедуры

Процедура Тест9(Знач Парам91)
    Парам91 = 12; // ошибка
    Значение = Парам91;
КонецПроцедуры

Процедура Тест10(Знач Парам101)
    Значение = Парам101; // не ошибка
КонецПроцедуры

Процедура Тест11(Знач Парам111)
    Парам111 = Парам111.Реквизит; // не ошибка
КонецПроцедуры

Процедура Тест12(Знач Парам121)
    Парам121 = Парам121 + Выражение; // не ошибка
КонецПроцедуры

Процедура Тест13(Знач Парам131)
    Парам131 = 1 + Парам131; // не ошибка
    Возврат Парам131;
КонецПроцедуры

Процедура Тест14(Знач Парам141)
    Парам141.Значение = 10; // не ошибка
КонецПроцедуры

Процедура Тест15(Знач Парам151)
    Парам151.Значение1 = Парам151.Значение2; // не ошибка
    Парам151 = 10; // не ошибка
КонецПроцедуры

Процедура Тест16(Знач Парам161)
    Парам161.Значение = Парам161; // не ошибка
    Парам161 = 10; // не ошибка
КонецПроцедуры"#;
        let diagnostics = crate::test_utils::check_hir_diagnostic(code);
        let rewrite_diags = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::RewriteMethodParameter)
            .collect::<Vec<_>>();
        expect![[r#"
            RewriteMethodParameter @ 2:5..2:11
              message: Переприсваивание параметра метода 'Парам1'
              severity: Warning
            RewriteMethodParameter @ 10:5..10:12
              message: Переприсваивание параметра метода 'Парам31'
              severity: Warning
            RewriteMethodParameter @ 23:5..23:12
              message: Переприсваивание параметра метода 'Парам61'
              severity: Warning
            RewriteMethodParameter @ 30:5..30:12
              message: Переприсваивание параметра метода 'Парам71'
              severity: Warning
            RewriteMethodParameter @ 38:5..38:12
              message: Переприсваивание параметра метода 'Парам91'
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &rewrite_diags));
    }
}
