//! Detects server parameters that are copied back to the client without need.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::call_graph::{CallTarget, CallerId, EdgeKind};
use hir::{AnnotationKind, Expr, IdConversion, Stmt};
use rustc_hash::FxHashSet;

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
    let summary = ctx.call_summary(hir::ModuleId::new(ctx.file_id));
    let mut diagnostics = Vec::new();

    // Precompute client-only method local_ids (AtClient annotation)
    let client_method_ids: FxHashSet<u32> = symbol_tree
        .methods()
        .filter(|m| m.annotations.iter().any(|ann| ann.kind == CLIENT_ANNOTATION))
        .map(|m| m.id.local_id)
        .collect();

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

        // Check once per method: is this server method called from any client method?
        let server_local_id = method.id.local_id;
        let has_client_call = summary.call_edges.iter().any(|edge| {
            edge.kind == EdgeKind::DirectLocal
                && matches!(&edge.target, CallTarget::Local { callee_local_id } if *callee_local_id == server_local_id)
                && matches!(edge.caller, CallerId::Method(caller_id) if client_method_ids.contains(&caller_id))
        });

        if !has_client_call {
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

            let item_tree = ctx.item_tree();
            let mut param_range = None;

            for item in item_tree.top_level_items() {
                match item {
                    hir::ModItem::Procedure(proc_idx) => {
                        let proc = item_tree.procedure(*proc_idx);
                        if proc.name == method.name {
                            if let Some(param_info) = proc.params.get(param_idx) {
                                param_range = Some(param_info.name_range);
                            }
                            break;
                        }
                    }
                    hir::ModItem::Function(func_idx) => {
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

    diagnostics
}

fn collect_assigned_params(body: &hir::Body) -> FxHashSet<String> {
    let mut assigned = FxHashSet::default();

    for (_stmt_id, stmt) in body.stmts_iter() {
        collect_assigned_from_stmt(stmt, body, &mut assigned);
    }

    assigned
}

fn collect_assigned_from_stmt(stmt: &Stmt, body: &hir::Body, assigned: &mut FxHashSet<String>) {
    match stmt {
        Stmt::Assign { target, .. } => {
            let target_id = hir::ExprId::from_idx(*target);
            if let Expr::Path(name) = body.expr(target_id) {
                assigned.insert(name.as_str().to_lowercase());
            }
        }
        Stmt::If(if_stmt) => {
            for &stmt_idx in if_stmt.then_branch.iter() {
                let stmt_id = hir::StmtId::from_idx(stmt_idx);
                collect_assigned_from_stmt(body.stmt(stmt_id), body, assigned);
            }
            for (_, elsif_stmts) in if_stmt.elsif_branches.iter() {
                for &stmt_idx in elsif_stmts.iter() {
                    let stmt_id = hir::StmtId::from_idx(stmt_idx);
                    collect_assigned_from_stmt(body.stmt(stmt_id), body, assigned);
                }
            }
            if let Some(ref else_branch) = if_stmt.else_branch {
                for &stmt_idx in else_branch.iter() {
                    let stmt_id = hir::StmtId::from_idx(stmt_idx);
                    collect_assigned_from_stmt(body.stmt(stmt_id), body, assigned);
                }
            }
        }
        Stmt::While { body: stmts, .. }
        | Stmt::For { body: stmts, .. }
        | Stmt::ForEach { body: stmts, .. } => {
            for &stmt_idx in stmts.iter() {
                let stmt_id = hir::StmtId::from_idx(stmt_idx);
                collect_assigned_from_stmt(body.stmt(stmt_id), body, assigned);
            }
        }
        Stmt::Try { body: try_block, except, .. } => {
            for &stmt_idx in try_block.iter() {
                let stmt_id = hir::StmtId::from_idx(stmt_idx);
                collect_assigned_from_stmt(body.stmt(stmt_id), body, assigned);
            }
            for &stmt_idx in except.iter() {
                let stmt_id = hir::StmtId::from_idx(stmt_idx);
                collect_assigned_from_stmt(body.stmt(stmt_id), body, assigned);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    #[test]
    fn test_by_ref_param_in_server_method_called_from_client() {
        let code = r#"&НаКлиенте
Процедура Клиент1()
    Сервер1(2);
КонецПроцедуры

&НаСервере
Процедура Сервер1(Парам1) // ошибка
    Метод(Парам1);
КонецПроцедуры

&НаКлиенте
Процедура Клиент2()
    Сервер2(2);
КонецПроцедуры

&НаСервере
Процедура Сервер2(Знач Парам1) // не ошибка
КонецПроцедуры

&НаКлиенте
Процедура Клиент3()
    Сервер3(2);
КонецПроцедуры

&НаСервере
Процедура Сервер3(Парам1) // не ошибка
    Парам1 = 10;
КонецПроцедуры

&НаКлиентеНаСервереБезКонтекста
Процедура КлиентСервер4(Парам1) // не ошибка
    Сервер4(2);
КонецПроцедуры

&НаСервере
Процедура Сервер4(Парам1) // не ошибка
    ЕщеМетод(Парам1);
КонецПроцедуры

&НаСервере
Процедура Метод(Парам1)
КонецПроцедуры

&НаСервере
Процедура ЕщеМетод(Парам1)
КонецПроцедуры
"#;

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
