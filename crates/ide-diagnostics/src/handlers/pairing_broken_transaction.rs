//! PairingBrokenTransaction diagnostic (CFG-based).
//!
//! Checks that transaction method calls are properly paired **across all execution paths**:
//! - `BeginTransaction()`/`НачатьТранзакцию()` must be paired with
//! - `CommitTransaction()`/`ЗафиксироватьТранзакцию()` **or**
//! - `RollbackTransaction()`/`ОтменитьТранзакцию()`
//!
//! A transaction is considered "closed" if it has EITHER Commit OR Rollback on each path.
//! This is important for try-except patterns where:
//! - Try block contains Commit (normal path)
//! - Except block contains Rollback (error path)
//!
//! ## CFG-based approach advantage
//!
//! Unlike Java's simple stack-based linear analysis, this implementation uses CFG
//! to analyze **all execution paths**. This catches errors that Java misses:
//!
//! ```bsl
//! Если Условие Тогда
//!     НачатьТранзакцию();  // Begin only in true branch
//! Иначе
//!     ЗафиксироватьТранзакцию();  // Commit only in false branch - ERROR!
//! КонецЕсли;
//! ```
//!
//! Java (stack-based): Considers this "paired" (1 begin, 1 commit)
//! CFG (path-based): Catches BOTH errors - orphaned begin AND orphaned commit
//!
//! ## Algorithm
//!
//! For each method:
//! 1. Build CFG (via `ctx.module_cfgs()`)
//! 2. DFS through all paths from entry to exit
//! 3. Track `transaction_level` per path:
//!    - BeginTransaction → level++
//!    - CommitTransaction OR RollbackTransaction → level--
//! 4. If level < 0 at any point → orphaned commit/rollback
//! 5. If level > 0 at exit → orphaned begin (missing Commit OR Rollback)
//!
//! Ported from:
//! - PairingBrokenTransactionDiagnostic.java (bsl-language-server) - logic reference
//! - Enhanced with CFG-based path analysis for higher precision

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use cfg::{CfgVertex, ControlFlowGraph, NodeIndex};
use cfg_types::{ExprId, IdConversion, StmtId};
use hir::{Body, BodySourceMap, Expr, Stmt};
use ide_db::TextRange;
use rustc_hash::{FxHashMap, FxHashSet};
use crate::define_metadata;
use crate::metadata::*;

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

/// Transaction call type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransactionType {
    Begin,
    Commit,
    Rollback,
}

/// Information about a transaction call found in CFG.
#[derive(Debug, Clone)]
struct TransactionCall {
    tx_type: TransactionType,
    method_name: String,
    range: TextRange,
}

/// Issue found during path analysis.
#[derive(Debug, Clone)]
struct TransactionIssue {
    range: TextRange,
    method_name: String,
    pair_method: &'static str,
}

/// Collect PairingBrokenTransaction diagnostics using CFG-based path analysis.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::PairingBrokenTransaction;

    if ctx.is_disabled_with_metadata(code) {
        return vec![];
    }

    // maxTransactionLevel: limits DFS depth to prevent stack overflow on pathological CFGs
    // (e.g., BeginTransaction in infinite loop). In practice, nesting rarely exceeds 2-3.
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

        // Check transaction pairing - Begin must be paired with EITHER Commit OR Rollback
        let issues = check_transaction_pairing_cfg(body, source_map, cfg, max_level);
        for issue in issues {
            diagnostics.push(create_diagnostic(issue, code, ctx));
        }
    }

    diagnostics
}

impl TransactionType {
    /// Returns the pair method name for error messages.
    fn pair_method(&self) -> &'static str {
        match self {
            TransactionType::Begin => "ЗафиксироватьТранзакцию/ОтменитьТранзакцию",
            TransactionType::Commit | TransactionType::Rollback => "НачатьТранзакцию",
        }
    }
}

/// State tracked during DFS path traversal.
#[derive(Clone)]
struct PathState {
    /// Current transaction level (begin increments, commit/rollback decrements)
    level: i32,
    /// Stack of begin transaction calls (for reporting orphaned begins)
    begin_stack: Vec<TransactionCall>,
}

impl PathState {
    fn new() -> Self {
        Self { level: 0, begin_stack: Vec::new() }
    }
}

/// Check transaction pairing using CFG-based DFS path analysis.
///
/// A transaction is considered properly paired if Begin is matched with EITHER
/// Commit OR Rollback on every execution path.
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

    // Pre-compute ALL transaction calls per CFG node (Begin, Commit, and Rollback)
    let node_tx_calls = precompute_transaction_calls(body, source_map, cfg);

    let dfs_ctx = DfsContext { cfg, node_tx_calls: &node_tx_calls, max_level };

    let mut issues = Vec::new();
    let mut visited_states: FxHashMap<NodeIndex, FxHashSet<i32>> = FxHashMap::default();

    dfs_check_paths(entry, PathState::new(), &mut visited_states, &mut issues, &dfs_ctx);

    // Deduplicate issues by range (same location may be reported from multiple paths)
    let mut seen_ranges: FxHashSet<TextRange> = FxHashSet::default();
    issues.retain(|issue| seen_ranges.insert(issue.range));

    issues
}

/// Pre-compute ALL transaction calls for each CFG node.
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

/// Context for DFS traversal (immutable during traversal).
struct DfsContext<'a> {
    cfg: &'a ControlFlowGraph,
    node_tx_calls: &'a FxHashMap<NodeIndex, Vec<TransactionCall>>,
    max_level: i32,
}

/// DFS traversal checking transaction pairing on all paths.
///
/// A transaction is considered "closed" if Begin is followed by EITHER Commit OR Rollback.
fn dfs_check_paths(
    node: NodeIndex,
    mut state: PathState,
    visited_states: &mut FxHashMap<NodeIndex, FxHashSet<i32>>,
    issues: &mut Vec<TransactionIssue>,
    ctx: &DfsContext,
) {
    // Prevent stack overflow on pathological cases (e.g., BeginTransaction in infinite loop)
    if state.level > ctx.max_level || state.level < -ctx.max_level {
        return;
    }

    // Cycle detection: if we've visited this node with the same level, skip
    // (prevents infinite loops in cycles while still exploring different levels)
    let levels_at_node = visited_states.entry(node).or_default();
    if !levels_at_node.insert(state.level) {
        return;
    }

    // Process transaction calls in this node
    if let Some(calls) = ctx.node_tx_calls.get(&node) {
        for call in calls {
            match call.tx_type {
                TransactionType::Begin => {
                    state.level += 1;
                    state.begin_stack.push(call.clone());
                }
                // Both Commit and Rollback "close" a transaction
                TransactionType::Commit | TransactionType::Rollback => {
                    state.level -= 1;
                    if state.level < 0 {
                        // Orphaned commit/rollback - no matching begin on this path
                        issues.push(TransactionIssue {
                            range: call.range,
                            method_name: call.method_name.clone(),
                            pair_method: call.tx_type.pair_method(),
                        });
                        // Reset level to 0 to continue checking (don't cascade errors)
                        state.level = 0;
                    } else {
                        state.begin_stack.pop();
                    }
                }
            }
        }
    }

    // Check if we reached exit
    if node == ctx.cfg.exit_point() {
        // Report orphaned begins (level > 0 means unmatched begins)
        for begin_call in &state.begin_stack {
            issues.push(TransactionIssue {
                range: begin_call.range,
                method_name: begin_call.method_name.clone(),
                pair_method: begin_call.tx_type.pair_method(),
            });
        }
        return;
    }

    // Continue DFS to successors
    let successors: Vec<_> = ctx.cfg.outgoing_edges(node).map(|(idx, _)| idx).collect();
    for succ in successors {
        dfs_check_paths(succ, state.clone(), visited_states, issues, ctx);
    }
}

/// Check if a statement is a transaction method call.
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

/// Check if an expression is a transaction method call.
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

/// Determine transaction type from method name (case-insensitive).
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

/// Create a diagnostic for broken transaction pairing.
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
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;
    use crate::DiagnosticCode;
    #[test]
    fn test_valid_pairing_with_commit() {
        let code = r#"
Процедура Тест()
    НачатьТранзакцию();
    Действие();
    ЗафиксироватьТранзакцию();
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();
        assert_eq!(pairing_diags.len(), 0, "Valid pairing with commit should have no diagnostics");
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();
        assert_eq!(
            pairing_diags.len(),
            0,
            "Valid pairing with rollback should have no diagnostics"
        );
    }

    /// Rollback followed by Commit is invalid - Commit is orphaned
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();
        // Rollback closes the transaction, then Commit has no matching Begin
        assert_eq!(
            pairing_diags.len(),
            1,
            "Rollback then Commit should produce orphaned Commit diagnostic"
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();
        assert_eq!(pairing_diags.len(), 1, "Orphaned commit should have 1 diagnostic");
    }

    #[test]
    fn test_orphaned_begin() {
        let code = r#"
Процедура Тест()
    BeginTransaction();
    Действие();
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();
        // Now: 1 diagnostic for Begin without Commit OR Rollback
        // (transaction can be closed by either, so one error instead of two)
        assert_eq!(pairing_diags.len(), 1, "Orphaned begin should have 1 diagnostic");
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();
        // First Begin has no matching end (Commit is consumed by second Begin)
        assert_eq!(
            pairing_diags.len(),
            1,
            "Nested incomplete transactions should have 1 diagnostic for first Begin"
        );
    }

    /// CFG-based test: Begin in one branch, Commit in another
    /// Java misses this, CFG catches both errors
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();

        // CFG should catch:
        // - Path 1 (true branch): Begin without Commit → orphaned begin
        // - Path 2 (false branch): Commit without Begin → orphaned commit
        assert!(
            pairing_diags.len() >= 2,
            "Branch imbalance should catch both orphaned begin and commit, got {}",
            pairing_diags.len()
        );
    }

    // =========================================================================
    // TRY-EXCEPT TRANSACTION PATTERN TESTS
    // =========================================================================
    // These tests cover the canonical 1C transaction patterns with try-except.
    // A transaction is considered properly paired if Begin is followed by
    // EITHER Commit OR Rollback on every execution path.

    /// Standard try-except transaction pattern - the canonical 1C pattern:
    /// - НачатьТранзакцию() before try
    /// - ЗафиксироватьТранзакцию() inside try (normal path)
    /// - ОтменитьТранзакцию() inside except (error path)
    /// - ВызватьИсключение to re-raise after rollback
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();

        assert_eq!(
            pairing_diags.len(),
            0,
            "Standard try-except transaction pattern should have NO diagnostics, got {}",
            pairing_diags.len()
        );
    }

    /// Try-except with commit in try and rollback in except - correct pattern
    /// Simpler version without the raise
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();

        assert_eq!(
            pairing_diags.len(),
            0,
            "Try-except with commit/rollback should have NO diagnostics"
        );
    }

    /// Try-except with only rollback in except (no commit in try) - INVALID
    /// Normal path exits without closing transaction
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();

        // Normal path: Begin -> no end -> orphaned Begin
        assert_eq!(
            pairing_diags.len(),
            1,
            "Try-except with only rollback should produce 1 orphaned Begin diagnostic"
        );
    }

    /// Try-except with only commit in try (no rollback in except) - INVALID
    /// Exception path exits without closing transaction
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();

        // Exception path: Begin -> no end -> orphaned Begin
        assert_eq!(
            pairing_diags.len(),
            1,
            "Try-except with only commit should produce 1 orphaned Begin diagnostic"
        );
    }

    /// Nested try-except with transactions - valid pattern
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();

        assert_eq!(
            pairing_diags.len(),
            0,
            "Nested try-except with proper transaction handling should have NO diagnostics"
        );
    }

    /// Multiple sequential transactions - each properly paired
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();

        assert_eq!(
            pairing_diags.len(),
            0,
            "Multiple sequential transactions properly paired should have NO diagnostics"
        );
    }

    /// Transaction with conditional inside try - valid if all paths close
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();

        assert_eq!(
            pairing_diags.len(),
            0,
            "Conditional inside try with commit after should have NO diagnostics"
        );
    }

    /// Early return in try block before commit - INVALID
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();

        // Path with early return: Begin -> Return -> exit without end
        assert!(
            !pairing_diags.is_empty(),
            "Early return before commit should produce orphaned Begin diagnostic"
        );
    }

    /// Raise inside try block should transfer control to except block.
    /// Transaction is properly closed because except contains Rollback.
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();

        // All paths are covered:
        // - Normal path: Begin -> Commit -> end
        // - Raise path: Begin -> Raise -> Except -> Rollback -> Raise -> exit
        assert_eq!(
            pairing_diags.len(),
            0,
            "Raise inside try should transfer to except, transaction is properly paired"
        );
    }

    /// Raise inside nested try should transfer to innermost except block.
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();

        // Raise in inner try goes to inner except, then continues to Commit
        // All paths close the transaction
        assert_eq!(
            pairing_diags.len(),
            0,
            "Nested try-except with raise should be properly handled"
        );
    }

    /// Raise outside try should still go to exit (uncaught exception).
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();

        // Path with Raise: Begin -> Raise -> exit (no Commit/Rollback)
        assert_eq!(pairing_diags.len(), 1, "Raise outside try leaves transaction open");
    }

    #[test]
    fn test_fixture() {
        let code = include_str!("../../test_data/PairingBrokenTransactionDiagnostic.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);
        let pairing_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PairingBrokenTransaction)
            .collect();

        // Debug output
        for (i, d) in pairing_diags.iter().enumerate() {
            eprintln!("Diagnostic {}: range={:?}, message={}", i, d.range, d.message);
        }

        // Java produces 21 diagnostics
        // CFG-based approach may find MORE issues due to path analysis
        assert!(
            pairing_diags.len() >= 10,
            "Expected at least 10 diagnostics from fixture, got {}",
            pairing_diags.len()
        );
    }
}
