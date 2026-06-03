use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::cfg::{CfgEdgeType, CfgVertex, ControlFlowGraph, NodeIndex};
use hir::{Body, BodySourceMap, Expr, ExprId, IdConversion, Stmt, StmtId};
use ide_db::TextRange;
use rustc_hash::{FxHashMap, FxHashSet};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransactionType {
    Begin,
    Commit,
    Rollback,
}

#[derive(Debug, Clone)]
struct TransactionCall {
    tx_type: TransactionType,
    method_name: String,
    range: TextRange,
}

#[derive(Debug, Clone)]
struct TransactionIssue {
    range: TextRange,
    method_name: String,
    pair_method: &'static str,
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::PairingBrokenTransaction;

    if ctx.is_disabled_with_metadata(code) {
        return vec![];
    }

    let max_level = ctx
        .config
        .get_int(DiagnosticCode::PairingBrokenTransaction, "maxTransactionLevel")
        .unwrap_or(32) as i32;

    let module_bodies = ctx.module_bodies();
    let module_cfgs = ctx.module_cfgs();
    let mut diagnostics = Vec::new();

    for (local_id, body) in module_bodies.iter_bodies() {
        let source_map = match module_bodies.source_map(local_id) {
            Some(sm) => sm,
            None => continue,
        };

        let cfg = match module_cfgs.get(local_id) {
            Some(cfg) => cfg,
            None => continue,
        };

        let issues = check_transaction_pairing_cfg(body, source_map, cfg, max_level);
        for issue in issues {
            diagnostics.push(create_diagnostic(issue, code, ctx));
        }
    }

    diagnostics
}

impl TransactionType {
    fn pair_method(&self) -> &'static str {
        match self {
            TransactionType::Begin => "ЗафиксироватьТранзакцию/ОтменитьТранзакцию",
            TransactionType::Commit | TransactionType::Rollback => "НачатьТранзакцию",
        }
    }
}

#[derive(Clone)]
struct PathState {
    level: i32,
    begin_stack: Vec<TransactionCall>,
}

impl PathState {
    fn new() -> Self {
        Self { level: 0, begin_stack: Vec::new() }
    }
}

fn check_transaction_pairing_cfg(
    body: &Body,
    source_map: &BodySourceMap,
    cfg: &ControlFlowGraph,
    max_level: i32,
) -> Vec<TransactionIssue> {
    let entry = match cfg.entry_point() {
        Some(e) => e,
        None => return vec![],
    };

    let node_tx_calls = precompute_transaction_calls(body, source_map, cfg);

    let dfs_ctx = DfsContext { cfg, node_tx_calls: &node_tx_calls, max_level };

    let mut issues = Vec::new();
    let mut visited_states: FxHashMap<NodeIndex, FxHashSet<i32>> = FxHashMap::default();

    dfs_check_paths(entry, PathState::new(), &mut visited_states, &mut issues, &dfs_ctx);

    let mut seen_ranges: FxHashSet<TextRange> = FxHashSet::default();
    issues.retain(|issue| seen_ranges.insert(issue.range));

    issues
}

fn precompute_transaction_calls(
    body: &Body,
    source_map: &BodySourceMap,
    cfg: &ControlFlowGraph,
) -> FxHashMap<NodeIndex, Vec<TransactionCall>> {
    let mut result: FxHashMap<NodeIndex, Vec<TransactionCall>> = FxHashMap::default();

    for (node_idx, vertex) in cfg.vertices() {
        let mut calls = Vec::new();

        if let CfgVertex::BasicBlock(block) = vertex {
            for &stmt_id in block.statements() {
                if let Some(call) = check_transaction_call(body, stmt_id, source_map) {
                    calls.push(call);
                }
            }
        }

        if !calls.is_empty() {
            result.insert(node_idx, calls);
        }
    }

    result
}

struct DfsContext<'a> {
    cfg: &'a ControlFlowGraph,
    node_tx_calls: &'a FxHashMap<NodeIndex, Vec<TransactionCall>>,
    max_level: i32,
}

fn dfs_check_paths(
    node: NodeIndex,
    mut state: PathState,
    visited_states: &mut FxHashMap<NodeIndex, FxHashSet<i32>>,
    issues: &mut Vec<TransactionIssue>,
    ctx: &DfsContext,
) {
    if state.level > ctx.max_level || state.level < -ctx.max_level {
        return;
    }

    let levels_at_node = visited_states.entry(node).or_default();
    if !levels_at_node.insert(state.level) {
        return;
    }

    if let Some(calls) = ctx.node_tx_calls.get(&node) {
        for call in calls {
            match call.tx_type {
                TransactionType::Begin => {
                    state.level += 1;
                    state.begin_stack.push(call.clone());
                }
                TransactionType::Commit | TransactionType::Rollback => {
                    state.level -= 1;
                    if state.level < 0 {
                        issues.push(TransactionIssue {
                            range: call.range,
                            method_name: call.method_name.clone(),
                            pair_method: call.tx_type.pair_method(),
                        });
                        state.level = 0;
                    } else {
                        state.begin_stack.pop();
                    }
                }
            }
        }
    }

    if node == ctx.cfg.exit_point() {
        for begin_call in &state.begin_stack {
            issues.push(TransactionIssue {
                range: begin_call.range,
                method_name: begin_call.method_name.clone(),
                pair_method: begin_call.tx_type.pair_method(),
            });
        }
        return;
    }

    if matches!(ctx.cfg.vertex(node), Some(CfgVertex::TryExcept(_))) {
        let mut try_node = None;
        let mut except_node = None;

        for (idx, edge_type) in ctx.cfg.outgoing_edges(node) {
            match edge_type {
                CfgEdgeType::TrueBranch => try_node = Some(idx),
                CfgEdgeType::FalseBranch => except_node = Some(idx),
                _ => {}
            }
        }

        if let Some(try_n) = try_node {
            dfs_check_paths(try_n, state.clone(), visited_states, issues, ctx);
        }

        if let Some(except_n) = except_node {
            let has_raise_edges = ctx
                .cfg
                .incoming_edges(except_n)
                .any(|(_, edge_type)| !matches!(edge_type, CfgEdgeType::FalseBranch));

            if !has_raise_edges {
                dfs_check_paths(except_n, state.clone(), visited_states, issues, ctx);
            }
        }
    } else {
        let successors: Vec<_> = ctx
            .cfg
            .outgoing_edges(node)
            .filter(|(_, edge_type)| !matches!(edge_type, CfgEdgeType::AdjacentCode))
            .map(|(idx, _)| idx)
            .collect();
        for succ in successors {
            dfs_check_paths(succ, state.clone(), visited_states, issues, ctx);
        }
    }
}

fn check_transaction_call(
    body: &Body,
    stmt_id: StmtId,
    source_map: &BodySourceMap,
) -> Option<TransactionCall> {
    let stmt = body.stmt(stmt_id);

    let expr_id = match stmt {
        Stmt::Expr(expr_idx) => ExprId::from_idx(*expr_idx),
        Stmt::Assign { value, .. } => ExprId::from_idx(*value),
        _ => return None,
    };

    check_expr_transaction_call(body, expr_id, source_map)
}

fn check_expr_transaction_call(
    body: &Body,
    expr_id: ExprId,
    source_map: &BodySourceMap,
) -> Option<TransactionCall> {
    let expr = body.expr(expr_id);

    if let Expr::Call { callee, .. } = expr {
        let callee_id = ExprId::from_idx(*callee);
        let callee_expr = body.expr(callee_id);

        if let Expr::Path(name) = callee_expr {
            let method_name = name.as_str();
            if let Some(tx_type) = get_transaction_type(method_name) {
                let range = source_map.expr_range(expr_id)?;
                return Some(TransactionCall {
                    tx_type,
                    method_name: method_name.to_string(),
                    range,
                });
            }
        }
    }

    None
}

fn get_transaction_type(name: &str) -> Option<TransactionType> {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "начатьтранзакцию" | "begintransaction" => Some(TransactionType::Begin),
        "зафиксироватьтранзакцию" | "committransaction" => {
            Some(TransactionType::Commit)
        }
        "отменитьтранзакцию" | "rollbacktransaction" => {
            Some(TransactionType::Rollback)
        }
        _ => None,
    }
}

fn create_diagnostic(
    issue: TransactionIssue,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: format!(
            "Нарушена парность использования метода '{}' и '{}'",
            issue.pair_method, issue.method_name
        ),
        severity: ctx.severity(code),
        range: issue.range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_valid_pairing_with_commit() {
        let code = r#"
Процедура Тест()
    НачатьТранзакцию();
    Действие();
    ЗафиксироватьТранзакцию();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_valid_pairing_with_rollback() {
        let code = r#"
Процедура Тест()
    НачатьТранзакцию();
    Действие();
    ОтменитьТранзакцию();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_rollback_then_commit_is_invalid() {
        let code = r#"
Процедура Тест()
    НачатьТранзакцию();
    Действие();
    ОтменитьТранзакцию();
    ЗафиксироватьТранзакцию();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#"
                PairingBrokenTransaction @ 6:5..6:30
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ЗафиксироватьТранзакцию'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_orphaned_commit() {
        let code = r#"
Процедура Тест()
    Действие();
    ЗафиксироватьТранзакцию();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#"
                PairingBrokenTransaction @ 4:5..4:30
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ЗафиксироватьТранзакцию'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_orphaned_begin() {
        let code = r#"
Процедура Тест()
    BeginTransaction();
    Действие();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#"
                PairingBrokenTransaction @ 3:5..3:23
                  message: Нарушена парность использования метода 'ЗафиксироватьТранзакцию/ОтменитьТранзакцию' и 'BeginTransaction'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_nested_transactions() {
        let code = r#"
Процедура Тест()
    НачатьТранзакцию();
    НачатьТранзакцию();
    Действие();
    ЗафиксироватьТранзакцию();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#"
                PairingBrokenTransaction @ 3:5..3:23
                  message: Нарушена парность использования метода 'ЗафиксироватьТранзакцию/ОтменитьТранзакцию' и 'НачатьТранзакцию'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_branch_imbalance() {
        let code = r#"
Процедура Тест(Условие)
    Если Условие Тогда
        НачатьТранзакцию();
    Иначе
        ЗафиксироватьТранзакцию();
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#"
                PairingBrokenTransaction @ 4:9..4:27
                  message: Нарушена парность использования метода 'ЗафиксироватьТранзакцию/ОтменитьТранзакцию' и 'НачатьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 6:9..6:34
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ЗафиксироватьТранзакцию'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_standard_try_except_transaction_pattern() {
        let code = r#"
Процедура ОбновитьПоЗадаче(Задача)

    НачатьТранзакцию();
    Попытка

        Блокировка = Новый БлокировкаДанных;
        Блокировка.Заблокировать();

        НаборЗаписей = СоздатьНаборЗаписей();
        НаборЗаписей.Записать();

        ЗафиксироватьТранзакцию();

    Исключение
        ОтменитьТранзакцию();
        ВызватьИсключение;
    КонецПопытки;

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_try_except_commit_rollback_no_raise() {
        let code = r#"
Процедура Тест()
    НачатьТранзакцию();
    Попытка
        Действие();
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_try_except_only_rollback_invalid() {
        let code = r#"
Процедура Тест()
    НачатьТранзакцию();
    Попытка
        Действие();
        // Missing ЗафиксироватьТранзакцию here!
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#"
                PairingBrokenTransaction @ 3:5..3:23
                  message: Нарушена парность использования метода 'ЗафиксироватьТранзакцию/ОтменитьТранзакцию' и 'НачатьТранзакцию'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_try_except_only_commit_invalid() {
        let code = r#"
Процедура Тест()
    НачатьТранзакцию();
    Попытка
        Действие();
        ЗафиксироватьТранзакцию();
    Исключение
        // Missing ОтменитьТранзакцию here!
        ЗаписатьОшибку();
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#"
                PairingBrokenTransaction @ 3:5..3:23
                  message: Нарушена парность использования метода 'ЗафиксироватьТранзакцию/ОтменитьТранзакцию' и 'НачатьТранзакцию'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_nested_try_except_valid() {
        let code = r#"
Процедура Тест()
    НачатьТранзакцию();
    Попытка
        Попытка
            Действие();
        Исключение
            ОбработатьОшибку();
        КонецПопытки;
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_multiple_sequential_transactions_valid() {
        let code = r#"
Процедура Тест()
    // First transaction
    НачатьТранзакцию();
    Попытка
        Действие1();
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;

    // Second transaction
    НачатьТранзакцию();
    Попытка
        Действие2();
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_conditional_inside_try_valid() {
        let code = r#"
Процедура Тест(Условие)
    НачатьТранзакцию();
    Попытка
        Если Условие Тогда
            Действие1();
        Иначе
            Действие2();
        КонецЕсли;
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_early_return_before_commit_invalid() {
        let code = r#"
Процедура Тест(Условие)
    НачатьТранзакцию();
    Попытка
        Если Условие Тогда
            Возврат;  // Early return without commit!
        КонецЕсли;
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#"
                PairingBrokenTransaction @ 3:5..3:23
                  message: Нарушена парность использования метода 'ЗафиксироватьТранзакцию/ОтменитьТранзакцию' и 'НачатьТранзакцию'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_raise_inside_try_transfers_to_except() {
        let code = r#"
Процедура Тест(Условие)
    НачатьТранзакцию();
    Попытка
        Если Условие Тогда
            ВызватьИсключение "Ошибка";
        КонецЕсли;
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_raise_inside_nested_try() {
        let code = r#"
Процедура Тест(Условие)
    НачатьТранзакцию();
    Попытка
        Попытка
            Если Условие Тогда
                ВызватьИсключение "Внутренняя ошибка";
            КонецЕсли;
        Исключение
            // Inner except - does NOT rollback, just logs
            ЗаписатьОшибку();
        КонецПопытки;
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_raise_outside_try_goes_to_exit() {
        let code = r#"
Процедура Тест(Условие)
    НачатьТранзакцию();
    Если Условие Тогда
        ВызватьИсключение "Ошибка без try";
    КонецЕсли;
    ЗафиксироватьТранзакцию();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#"
                PairingBrokenTransaction @ 3:5..3:23
                  message: Нарушена парность использования метода 'ЗафиксироватьТранзакцию/ОтменитьТранзакцию' и 'НачатьТранзакцию'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_begin_inside_try_with_nested_raise() {
        let code = r#"
Процедура Тест()
    Попытка
        НачатьТранзакцию();
        Попытка
            Действие();
        Исключение
            ВызватьИсключение;
        КонецПопытки;
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_break_after_commit_in_loop_valid() {
        let code = r#"
Процедура Тест()
    Пока Истина Цикл
        НачатьТранзакцию();
        Действие();
        ЗафиксироватьТранзакцию();
        Прервать;
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_break_with_open_transaction_invalid() {
        let code = r#"
Процедура Тест()
    Пока Истина Цикл
        НачатьТранзакцию();
        Действие();
        Прервать;
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#"
                PairingBrokenTransaction @ 4:9..4:27
                  message: Нарушена парность использования метода 'ЗафиксироватьТранзакцию/ОтменитьТранзакцию' и 'НачатьТранзакцию'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_multiple_commit_continue_in_loop() {
        let code = r#"
Процедура Тест()
    Пока Истина Цикл
        НачатьТранзакцию();
        Попытка
            Если Условие1 Тогда
                ЗафиксироватьТранзакцию();
                Продолжить;
            КонецЕсли;
            Если Условие2 Тогда
                ЗафиксироватьТранзакцию();
                Продолжить;
            КонецЕсли;
            ЗафиксироватьТранзакцию();
        Исключение
            ОтменитьТранзакцию();
        КонецПопытки;
    КонецЦикла;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_fixture() {
        let code = r#"Процедура Проц1()
    // Парность соблюдается
    НачатьТранзакцию();
    Действие();
    ОтменитьТранзакцию();
    ЗафиксироватьТранзакцию();
КонецПроцедуры

Процедура Проц2()
    // Парность соблюдается
    BeginTransaction();
    Действие();
    RollbackTransaction();
    CommitTransaction();
КонецПроцедуры

Функция Функ1()
    // Парность соблюдается
    BeginTransaction();
    Действие();
    RollbackTransaction();
    CommitTransaction();
    Возврат Истина;
КонецФункции

Процедура Проц3()
    Действие();
    ЗафиксироватьТранзакцию(); // Парность не соблюдается здесь
КонецПроцедуры

Процедура Проц4()
    BeginTransaction(); // Парность не соблюдается здесь
    Действие();
КонецПроцедуры

Процедура Проц5()
    НачатьТранзакцию();
    Действие();
    ЗафиксироватьТранзакцию();
    ОтменитьТранзакцию();
    ЗафиксироватьТранзакцию(); // Парность не соблюдается здесь
КонецПроцедуры

Процедура Проц6()
    НачатьТранзакцию(); // Парность не соблюдается здесь для Зафаксировать И Отменить
    НачатьТранзакцию(); // Парность не соблюдается здесь для Отменить
    Действие();
    ЗафиксироватьТранзакцию();
КонецПроцедуры

Процедура Проц7()
    // Парность соблюдается
    НачатьТранзакцию(); // Парность не соблюдается здесь для Отменить
    НачатьТранзакцию(); // Парность не соблюдается здесь для Отменить
    Действие();
    ЗафиксироватьТранзакцию();
    НачатьТранзакцию(); // Парность не соблюдается здесь для Отменить
    ЗафиксироватьТранзакцию();
    ЗафиксироватьТранзакцию();
КонецПроцедуры

Процедура Проц8()
    // Парность соблюдается
    НачатьТранзакцию();
    Если Истина Тогда
        НачатьТранзакцию();
    КонецЕсли;
    Действие();
    Если Условие1() Тогда
        ЗафиксироватьТранзакцию();
    КонецЕсли;
    ОтменитьТранзакцию();
    НачатьТранзакцию();
    ЗафиксироватьТранзакцию();
    ОтменитьТранзакцию();
    НачатьТранзакцию();
    ЗафиксироватьТранзакцию();
    ОтменитьТранзакцию();
КонецПроцедуры

Процедура Проц8()
    НачатьТранзакцию();
    ЗафиксироватьТранзакцию();
    ОтменитьТранзакцию();
    ЗафиксироватьТранзакцию(); // Парность не соблюдается здесь
    НачатьТранзакцию();
    ЗафиксироватьТранзакцию();
    ОтменитьТранзакцию();
    ЗафиксироватьТранзакцию(); // Парность не соблюдается здесь
    ОтменитьТранзакцию(); // Парность не соблюдается здесь
КонецПроцедуры

Процедура Проц9()
    НачатьТранзакцию(); // Парность не соблюдается здесь
        НачатьТранзакцию(); // Парность не соблюдается здесь для отменить
        ЗафиксироватьТранзакцию();
        НачатьТранзакцию(); // Парность не соблюдается здесь для отменить
        ЗафиксироватьТранзакцию();
        НачатьТранзакцию();
        ЗафиксироватьТранзакцию();
    ЗафиксироватьТранзакцию();
    ОтменитьТранзакцию();
    ЗафиксироватьТранзакцию(); // Парность не соблюдается здесь
КонецПроцедуры

Процедура Проц9()
    НачатьТранзакцию(); // Парность не соблюдается здесь для отменить
        НачатьТранзакцию(); // Парность не соблюдается здесь для отменить
        ЗафиксироватьТранзакцию();
        НачатьТранзакцию(); // Парность не соблюдается здесь для отменить
        ЗафиксироватьТранзакцию();
        НачатьТранзакцию();
        ЗафиксироватьТранзакцию();
    ЗафиксироватьТранзакцию();
    зафиксироватьТРАНЗакциЮ(); // Парность не соблюдается здесь
    ОтменитьТранзакцию();
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#"
                PairingBrokenTransaction @ 6:5..6:30
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ЗафиксироватьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 14:5..14:24
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'CommitTransaction'
                  severity: Major
                PairingBrokenTransaction @ 22:5..22:24
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'CommitTransaction'
                  severity: Major
                PairingBrokenTransaction @ 28:5..28:30
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ЗафиксироватьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 32:5..32:23
                  message: Нарушена парность использования метода 'ЗафиксироватьТранзакцию/ОтменитьТранзакцию' и 'BeginTransaction'
                  severity: Major
                PairingBrokenTransaction @ 40:5..40:25
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ОтменитьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 41:5..41:30
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ЗафиксироватьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 45:5..45:23
                  message: Нарушена парность использования метода 'ЗафиксироватьТранзакцию/ОтменитьТранзакцию' и 'НачатьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 72:5..72:25
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ОтменитьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 75:5..75:25
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ОтменитьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 78:5..78:25
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ОтменитьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 84:5..84:25
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ОтменитьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 85:5..85:30
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ЗафиксироватьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 88:5..88:25
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ОтменитьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 89:5..89:30
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ЗафиксироватьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 90:5..90:25
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ОтменитьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 102:5..102:25
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ОтменитьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 103:5..103:30
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ЗафиксироватьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 115:5..115:30
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'зафиксироватьТРАНЗакциЮ'
                  severity: Major
                PairingBrokenTransaction @ 116:5..116:25
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ОтменитьТранзакцию'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_interprocedural_begin_commit_are_not_paired_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Процедура Открыть()
    НачатьТранзакцию();
КонецПроцедуры

Процедура Закрыть()
    ЗафиксироватьТранзакцию();
КонецПроцедуры"#,
            DiagnosticCode::PairingBrokenTransaction,
            expect![[r#"
                PairingBrokenTransaction @ 2:5..2:23
                  message: Нарушена парность использования метода 'ЗафиксироватьТранзакцию/ОтменитьТранзакцию' и 'НачатьТранзакцию'
                  severity: Major
                PairingBrokenTransaction @ 6:5..6:30
                  message: Нарушена парность использования метода 'НачатьТранзакцию' и 'ЗафиксироватьТранзакцию'
                  severity: Major"#]],
        );
    }
}
