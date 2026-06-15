use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{BindingId, ExprId, IdConversion, StmtId};
use ide_db::TextRange;
use stdx::case::{eq_ignore_case, CaseExt};

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

/// Outcome of scanning statements in execution order looking for a read of the
/// parameter that happens before the rewrite.
enum Scan {
    /// No read encountered yet; keep scanning following statements.
    NotFound,
    /// The parameter is read before the rewrite — the rewrite is not blind.
    ReadFound,
    /// Reached the rewrite statement; nothing executes after it counts as "before".
    ReachedTarget,
}

/// A by-value parameter is only suspicious when it is overwritten *before* being
/// read. A flat statement-arena scan misses reads that live in the header of an
/// enclosing block (`Если Параметр = Неопределено Тогда Параметр = …`), which is
/// the idiomatic way to compute a non-literal default. Walk the statement tree in
/// execution order instead and stop at the rewrite itself.
fn parameter_used_before_stmt(
    body: &hir::Body,
    target_stmt_id: StmtId,
    param_id: BindingId,
) -> bool {
    let top_level: Vec<StmtId> = body.body_stmts().collect();
    matches!(scan_stmts_before_target(body, &top_level, target_stmt_id, param_id), Scan::ReadFound)
}

fn scan_stmts_before_target(
    body: &hir::Body,
    stmts: &[StmtId],
    target_stmt_id: StmtId,
    param_id: BindingId,
) -> Scan {
    for &stmt_id in stmts {
        match scan_stmt_before_target(body, stmt_id, target_stmt_id, param_id) {
            Scan::NotFound => {}
            other => return other,
        }
    }
    Scan::NotFound
}

fn scan_idx_slice(
    body: &hir::Body,
    stmts: &[hir::StmtIdx],
    target_stmt_id: StmtId,
    param_id: BindingId,
) -> Scan {
    let ids: Vec<StmtId> = stmts.iter().map(|&idx| StmtId::from_idx(idx)).collect();
    scan_stmts_before_target(body, &ids, target_stmt_id, param_id)
}

fn scan_stmt_before_target(
    body: &hir::Body,
    stmt_id: StmtId,
    target_stmt_id: StmtId,
    param_id: BindingId,
) -> Scan {
    use hir::{Expr, Stmt};

    if stmt_id == target_stmt_id {
        return Scan::ReachedTarget;
    }

    let reads =
        |expr_idx: hir::ExprIdx| expr_uses_binding(body, ExprId::from_idx(expr_idx), param_id);

    match body.stmt(stmt_id) {
        Stmt::If(if_stmt) => scan_if_before_target(body, if_stmt, target_stmt_id, param_id),
        Stmt::PreprocIf(preproc) => {
            for branch in preproc.branches() {
                match scan_idx_slice(body, branch.stmts, target_stmt_id, param_id) {
                    Scan::NotFound => {}
                    other => return other,
                }
            }
            Scan::NotFound
        }
        Stmt::While { condition, body: loop_body } => {
            if reads(*condition) {
                return Scan::ReadFound;
            }
            scan_idx_slice(body, loop_body, target_stmt_id, param_id)
        }
        Stmt::For { from, to, body: loop_body, .. } => {
            if reads(*from) || reads(*to) {
                return Scan::ReadFound;
            }
            scan_idx_slice(body, loop_body, target_stmt_id, param_id)
        }
        Stmt::ForEach { collection, body: loop_body, .. } => {
            if reads(*collection) {
                return Scan::ReadFound;
            }
            scan_idx_slice(body, loop_body, target_stmt_id, param_id)
        }
        Stmt::Try { body: try_body, except } => {
            match scan_idx_slice(body, try_body, target_stmt_id, param_id) {
                Scan::NotFound => {}
                other => return other,
            }
            scan_idx_slice(body, except, target_stmt_id, param_id)
        }
        Stmt::Assign { target, value } => {
            // Self-assignments (`П = П`) are not meaningful use.
            if is_self_assign_to_binding(body, stmt_id, param_id) {
                return Scan::NotFound;
            }
            if reads(*value) {
                return Scan::ReadFound;
            }
            // A bare write target (`П = …`) is not a read, but a member/index target
            // (`П.Поле = …`, `П[i] = …`) reads the parameter.
            let target_expr = body.expr(ExprId::from_idx(*target));
            let target_is_bare_param = matches!(
                target_expr,
                Expr::Path(name)
                    if eq_ignore_case(name.as_str(), body.binding(param_id).name.as_str())
            );
            if !target_is_bare_param && reads(*target) {
                return Scan::ReadFound;
            }
            Scan::NotFound
        }
        _ => {
            if stmt_uses_binding(body, stmt_id, param_id) {
                Scan::ReadFound
            } else {
                Scan::NotFound
            }
        }
    }
}

/// Path-sensitive scan of an `If` for a read that precedes the rewrite.
///
/// Reads in a guarding condition always count: every condition up to the
/// branch that owns the rewrite is evaluated before it runs. Reads in a branch
/// *body* only count when that body either contains the rewrite (the read
/// precedes it on the same path) or the whole `If` precedes the rewrite (the
/// rewrite lives in a following sibling, so the `If` fully executes first).
/// A read in a sibling branch that is mutually exclusive with the rewrite's
/// branch must NOT suppress the diagnostic.
fn scan_if_before_target(
    body: &hir::Body,
    if_stmt: &hir::IfStmt,
    target_stmt_id: StmtId,
    param_id: BindingId,
) -> Scan {
    let reads =
        |expr_idx: hir::ExprIdx| expr_uses_binding(body, ExprId::from_idx(expr_idx), param_id);

    let target_in_if = if_contains_target(body, if_stmt, target_stmt_id);

    if reads(if_stmt.condition) {
        return Scan::ReadFound;
    }
    match scan_if_branch(body, &if_stmt.then_branch, target_stmt_id, param_id, target_in_if) {
        Scan::NotFound => {}
        other => return other,
    }
    for (cond, branch) in if_stmt.elsif_branches.iter() {
        if reads(*cond) {
            return Scan::ReadFound;
        }
        match scan_if_branch(body, branch, target_stmt_id, param_id, target_in_if) {
            Scan::NotFound => {}
            other => return other,
        }
    }
    if let Some(branch) = &if_stmt.else_branch {
        match scan_if_branch(body, branch, target_stmt_id, param_id, target_in_if) {
            Scan::NotFound => {}
            other => return other,
        }
    }
    Scan::NotFound
}

fn scan_if_branch(
    body: &hir::Body,
    branch: &[hir::StmtIdx],
    target_stmt_id: StmtId,
    param_id: BindingId,
    target_in_if: bool,
) -> Scan {
    if stmts_contain_target(body, branch, target_stmt_id) {
        // The rewrite lives in this branch — its result is authoritative.
        return scan_idx_slice(body, branch, target_stmt_id, param_id);
    }
    // The rewrite is not in this branch. A read here only counts when the whole
    // `If` precedes the rewrite; otherwise this branch is a mutually exclusive
    // sibling of the rewrite's branch and must be ignored.
    match scan_idx_slice(body, branch, target_stmt_id, param_id) {
        Scan::ReadFound if !target_in_if => Scan::ReadFound,
        _ => Scan::NotFound,
    }
}

fn if_contains_target(body: &hir::Body, if_stmt: &hir::IfStmt, target_stmt_id: StmtId) -> bool {
    stmts_contain_target(body, &if_stmt.then_branch, target_stmt_id)
        || if_stmt
            .elsif_branches
            .iter()
            .any(|(_, branch)| stmts_contain_target(body, branch, target_stmt_id))
        || if_stmt
            .else_branch
            .as_ref()
            .is_some_and(|branch| stmts_contain_target(body, branch, target_stmt_id))
}

fn stmts_contain_target(body: &hir::Body, stmts: &[hir::StmtIdx], target_stmt_id: StmtId) -> bool {
    stmts.iter().any(|&idx| stmt_contains_target(body, StmtId::from_idx(idx), target_stmt_id))
}

fn stmt_contains_target(body: &hir::Body, stmt_id: StmtId, target_stmt_id: StmtId) -> bool {
    use hir::Stmt;

    if stmt_id == target_stmt_id {
        return true;
    }
    match body.stmt(stmt_id) {
        Stmt::If(if_stmt) => if_contains_target(body, if_stmt, target_stmt_id),
        Stmt::PreprocIf(preproc) => preproc
            .branches()
            .any(|branch| stmts_contain_target(body, branch.stmts, target_stmt_id)),
        Stmt::While { body: loop_body, .. }
        | Stmt::For { body: loop_body, .. }
        | Stmt::ForEach { body: loop_body, .. } => {
            stmts_contain_target(body, loop_body, target_stmt_id)
        }
        Stmt::Try { body: try_body, except } => {
            stmts_contain_target(body, try_body, target_stmt_id)
                || stmts_contain_target(body, except, target_stmt_id)
        }
        _ => false,
    }
}

fn is_self_assign_to_binding(body: &hir::Body, stmt_id: StmtId, binding_id: BindingId) -> bool {
    use hir::{Expr, Stmt};

    let stmt = body.stmt(stmt_id);
    match stmt {
        Stmt::Assign { target, value } => {
            if let Expr::Path(target_name) = body.expr(ExprId::from_idx(*target)) {
                let binding = body.binding(binding_id);
                if !eq_ignore_case(target_name.as_str(), binding.name.as_str()) {
                    return false;
                }

                if let Expr::Path(value_name) = body.expr(ExprId::from_idx(*value)) {
                    return eq_ignore_case(value_name.as_str(), binding.name.as_str());
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

    let uses = |idx: hir::ExprIdx| expr_uses_binding(body, ExprId::from_idx(idx), binding_id);

    match body.expr(expr_id) {
        Expr::Path(name) => {
            let binding = body.binding(binding_id);
            eq_ignore_case(name.as_str(), binding.name.as_str())
        }
        Expr::BinaryOp { lhs, rhs, .. } => uses(*lhs) || uses(*rhs),
        Expr::UnaryOp { expr, .. } => uses(*expr),
        Expr::Call { callee, args } => uses(*callee) || args.iter().any(|&arg| uses(arg)),
        Expr::MethodCall { receiver, args, .. } => {
            uses(*receiver) || args.iter().any(|&arg| uses(arg))
        }
        Expr::Index { base, index } => uses(*base) || uses(*index),
        Expr::Field { base, .. } => uses(*base),
        Expr::New { args, .. } => args.iter().any(|&arg| uses(arg)),
        Expr::Array(elements) => elements.iter().any(|&arg| uses(arg)),
        Expr::Ternary { condition, then_expr, else_expr } => {
            uses(*condition) || uses(*then_expr) || uses(*else_expr)
        }
        Expr::Await { expr } => uses(*expr),
        Expr::QualifiedPath(_) | Expr::Literal(_) | Expr::Missing => false,
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
    fn test_default_guard_idiom_no_diagnostic() {
        let code = r#"Процедура Тест(Знач Вариант = Неопределено)
    Если Вариант = Неопределено Тогда
        Вариант = ТекущаяДатаСеанса(); // не ошибка - вычисление дефолта
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RewriteMethodParameter,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_read_inside_call_in_condition_no_diagnostic() {
        let code = r#"Функция Тест(Знач Значение)
    Если Не ЗначениеЗаполнено(Значение) Тогда
        Значение = ТекущаяДатаСеанса(); // не ошибка - прочитан в ЗначениеЗаполнено
    КонецЕсли;
    Возврат Значение;
КонецФункции"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RewriteMethodParameter,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_read_with_cyrillic_case_mismatch_no_diagnostic() {
        // Объявление и использование различаются регистром кириллицы (н/Н):
        // BSL регистронезависим, чтение в условии должно подавлять диагностику.
        let code = r#"Функция Тест(Знач КодИнсп = Неопределено)
    Если КодИНСП = Неопределено ИЛИ ПустаяСтрока(КодИНСП) Тогда
        КодИНСП = "0000";
    КонецЕсли;
    Возврат КодИнсп;
КонецФункции"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RewriteMethodParameter,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_read_in_elsif_condition_no_diagnostic() {
        let code = r#"Процедура Тест(Знач Режим)
    Если Ложь Тогда
        Возврат;
    ИначеЕсли Режим = "А" Тогда
        Режим = "Б"; // не ошибка - прочитан в условии ИначеЕсли
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RewriteMethodParameter,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_read_in_sibling_branch_does_not_suppress() {
        // Чтение параметра в ветке Тогда взаимоисключающе с перезаписью в
        // ветке ИначеЕсли — оно не должно подавлять диагностику.
        let code = r#"Процедура Тест(Знач Парам)
    Если Условие1 Тогда
        Значение = Парам; // другая ветка исполнения
    ИначеЕсли Условие2 Тогда
        Парам = 10; // ошибка - на этом пути Парам не прочитан
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RewriteMethodParameter,
            expect![[r#"
            RewriteMethodParameter @ 5:9..5:14
              message: Переприсваивание параметра метода 'Парам'
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_read_in_preceding_sibling_if_suppresses() {
        // Если весь оператор Если предшествует перезаписи (перезапись — следующий
        // оператор), чтение в любой его ветке считается использованием до записи.
        let code = r#"Процедура Тест(Знач Парам)
    Если Условие Тогда
        Значение = Парам;
    КонецЕсли;
    Парам = 10; // не ошибка - Парам прочитан в предшествующем Если
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RewriteMethodParameter,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_rewrite_after_unrelated_guard_is_flagged() {
        let code = r#"Процедура Тест(Знач Парам)
    Если Истина Тогда
        Парам = 10; // ошибка - параметр нигде не прочитан до перезаписи
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RewriteMethodParameter,
            expect![[r#"
            RewriteMethodParameter @ 3:9..3:14
              message: Переприсваивание параметра метода 'Парам'
              severity: Warning"#]],
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
