//! HIR-structural method metrics.
//!
//! Track 2 Phase B §6.1 — single-pass HIR walk that produces every
//! HIR-derived metric the §6.4 migrated handlers consume. The walker
//! consolidates the logic that lived in three separate places before:
//!
//! - `cognitive_complexity::calculate_complexity` (kept verbatim
//!   while the migration is in flight; deletion follows the §6.4
//!   handler cut-over).
//! - `nested_statements`-handler ad-hoc max-depth visitor.
//! - `if_condition_complexity` per-condition logical-operator count,
//!   reduced to "max over all conditions in the method".
//!
//! Cyclomatic complexity is **not** here — graph-shape metrics live in
//! `cfg::cyclomatic_complexity` per the ROADMAP §Track 2 contract.
//!
//! The visitor walks the HIR statement arena exactly once and emits a
//! [`HirMethodMetrics`] record. Computation is `O(stmts + exprs)` and
//! requires no Salsa access; the cached query lives in
//! `ide-db::queries::method_hir_metrics_query` (§6.3).

use crate::hir::{BinaryOp, Expr, ExprIdx, IfStmt, Stmt, StmtIdx};
use crate::Body;

/// Per-method HIR-structural metrics, produced by
/// [`compute_hir_metrics`].
///
/// Every field is independent — adding a new metric does not invalidate
/// the existing ones, so future slices can extend the struct without
/// re-running the walks the migrated handlers depend on. `Default::default`
/// is the empty-method state (`cognitive = 0`, all counters at 0).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HirMethodMetrics {
    /// SonarSource Cognitive Complexity v1.4 score. Same algorithm as
    /// the legacy `cognitive_complexity::calculate_complexity`, just
    /// in-line with the other metrics.
    pub cognitive: u32,
    /// Deepest statement nesting reached anywhere in the body (0 for
    /// straight-line code; +1 for a body inside `If`, +2 for `If
    /// inside If`, etc.). Compound statements (`Try`, `While`, `For`,
    /// `ForEach`, `PreprocIf`) all increase the depth of their bodies
    /// by one.
    pub max_nesting: u32,
    /// Maximum number of logical `И`/`AND`/`Или`/`OR` operators in any
    /// **conditional** expression in the body — the value the
    /// `IfConditionComplexity` diagnostic compares against its
    /// threshold. The walker reports `max` across every `If`/`Elsif`
    /// condition as well as the implicit conditions of `While` /
    /// `Ternary`.
    pub if_condition_max: u32,
}

/// Single-pass HIR walk producing every metric in [`HirMethodMetrics`].
///
/// Pure — no Salsa, no I/O, no allocation beyond the stack frames the
/// recursion needs. The §6.3 Salsa wrapper passes the cached `Body`
/// directly; in tests the helper takes whatever `Body` the lowering
/// produced.
pub fn compute_hir_metrics(body: &Body) -> HirMethodMetrics {
    let mut visitor = MetricsVisitor::default();
    for &stmt_id in body.body_stmts.iter() {
        visitor.visit_stmt(body, stmt_id, 0);
    }
    visitor.finish()
}

#[derive(Debug, Default)]
struct MetricsVisitor {
    cognitive: u32,
    max_nesting: u32,
    if_condition_max: u32,
}

impl MetricsVisitor {
    fn finish(self) -> HirMethodMetrics {
        HirMethodMetrics {
            cognitive: self.cognitive,
            max_nesting: self.max_nesting,
            if_condition_max: self.if_condition_max,
        }
    }

    /// Track the deepest nesting reached by any statement in the body.
    fn note_depth(&mut self, depth: u32) {
        if depth > self.max_nesting {
            self.max_nesting = depth;
        }
    }

    fn visit_stmt(&mut self, body: &Body, stmt_id: StmtIdx, nesting: u32) {
        self.note_depth(nesting);
        match body.stmt_idx(stmt_id) {
            Stmt::If(if_stmt) => self.visit_if(body, if_stmt, nesting),

            Stmt::While { condition, body: loop_body } => {
                self.cognitive += 1 + nesting;
                self.note_logical_ops(body, *condition);
                self.visit_expr(body, *condition);
                for &child in loop_body.iter() {
                    self.visit_stmt(body, child, nesting + 1);
                }
            }

            Stmt::For { body: loop_body, .. } => {
                self.cognitive += 1 + nesting;
                for &child in loop_body.iter() {
                    self.visit_stmt(body, child, nesting + 1);
                }
            }

            Stmt::ForEach { body: loop_body, .. } => {
                self.cognitive += 1 + nesting;
                for &child in loop_body.iter() {
                    self.visit_stmt(body, child, nesting + 1);
                }
            }

            Stmt::Try { body: try_body, except } => {
                for &child in try_body.iter() {
                    self.visit_stmt(body, child, nesting);
                }
                self.cognitive += 1 + nesting;
                for &child in except.iter() {
                    self.visit_stmt(body, child, nesting + 1);
                }
            }

            Stmt::Goto(_) => {
                self.cognitive += 1;
            }

            Stmt::Expr(expr) => self.visit_expr(body, *expr),
            Stmt::Assign { value, .. } => self.visit_expr(body, *value),
            Stmt::Return { value: Some(v) } | Stmt::Raise { value: Some(v) } => {
                self.visit_expr(body, *v);
            }
            Stmt::Execute { expr } => self.visit_expr(body, *expr),
            Stmt::AddHandler { event, handler } | Stmt::RemoveHandler { event, handler } => {
                self.visit_expr(body, *event);
                self.visit_expr(body, *handler);
            }

            Stmt::PreprocIf(preproc) => {
                self.cognitive += 1 + nesting;
                for &child in preproc.then_branch.iter() {
                    self.visit_stmt(body, child, nesting + 1);
                }
                for (_cond_range, _directive_range, elsif_body) in preproc.elsif_branches.iter() {
                    self.cognitive += 1;
                    for &child in elsif_body.iter() {
                        self.visit_stmt(body, child, nesting + 2);
                    }
                }
                if let Some(ref else_body) = preproc.else_branch {
                    self.cognitive += 1;
                    for &child in else_body.iter() {
                        self.visit_stmt(body, child, nesting + 2);
                    }
                }
            }

            Stmt::Return { value: None }
            | Stmt::Raise { value: None }
            | Stmt::VarDecl { .. }
            | Stmt::Break
            | Stmt::Continue
            | Stmt::Label(_) => {}
        }
    }

    fn visit_if(&mut self, body: &Body, if_stmt: &IfStmt, nesting: u32) {
        self.cognitive += 1 + nesting;
        self.note_logical_ops(body, if_stmt.condition);
        self.visit_expr(body, if_stmt.condition);

        for &child in if_stmt.then_branch.iter() {
            self.visit_stmt(body, child, nesting + 1);
        }

        for (elsif_cond, elsif_body) in if_stmt.elsif_branches.iter() {
            self.cognitive += 1;
            self.note_logical_ops(body, *elsif_cond);
            self.visit_expr(body, *elsif_cond);
            for &child in elsif_body.iter() {
                self.visit_stmt(body, child, nesting + 2);
            }
        }

        if let Some(ref else_body) = if_stmt.else_branch {
            self.cognitive += 1;
            for &child in else_body.iter() {
                self.visit_stmt(body, child, nesting + 2);
            }
        }
    }

    /// Mirror of `cognitive_complexity::count_expr_complexity` — the
    /// only expression-level cognitive contributors are ternary (+1)
    /// and logical AND/OR (+1 each).
    fn visit_expr(&mut self, body: &Body, expr_id: ExprIdx) {
        match body.expr_idx(expr_id) {
            Expr::Ternary { condition, then_expr, else_expr } => {
                self.cognitive += 1;
                // The plan classifies ternary as a *condition* too — count
                // its logical-operator burden against the same per-method
                // ceiling as if/while.
                self.note_logical_ops(body, *condition);
                self.visit_expr(body, *condition);
                self.visit_expr(body, *then_expr);
                self.visit_expr(body, *else_expr);
            }
            Expr::BinaryOp { lhs, rhs, op } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    self.cognitive += 1;
                }
                self.visit_expr(body, *lhs);
                self.visit_expr(body, *rhs);
            }
            Expr::UnaryOp { expr, .. } => self.visit_expr(body, *expr),
            Expr::Call { callee, args } => {
                self.visit_expr(body, *callee);
                for &arg in args.iter() {
                    self.visit_expr(body, arg);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.visit_expr(body, *receiver);
                for &arg in args.iter() {
                    self.visit_expr(body, arg);
                }
            }
            Expr::Index { base, index } => {
                self.visit_expr(body, *base);
                self.visit_expr(body, *index);
            }
            Expr::Field { base, .. } => self.visit_expr(body, *base),
            Expr::New { args, .. } => {
                for &arg in args.iter() {
                    self.visit_expr(body, arg);
                }
            }
            Expr::Array(items) => {
                for &item in items.iter() {
                    self.visit_expr(body, item);
                }
            }
            Expr::Await { expr } => self.visit_expr(body, *expr),
            Expr::Missing | Expr::Literal(_) | Expr::Path(_) | Expr::QualifiedPath(_) => {}
        }
    }

    /// Count `И`/`AND`/`Или`/`OR` operators in `expr` and update the
    /// per-method maximum. Used for the `IfConditionComplexity`
    /// diagnostic threshold.
    fn note_logical_ops(&mut self, body: &Body, expr: ExprIdx) {
        let count = count_logical_ops(body, expr);
        if count > self.if_condition_max {
            self.if_condition_max = count;
        }
    }
}

fn count_logical_ops(body: &Body, expr: ExprIdx) -> u32 {
    let mut total = 0;
    fn walk(body: &Body, expr: ExprIdx, total: &mut u32) {
        match body.expr_idx(expr) {
            Expr::BinaryOp { lhs, rhs, op } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    *total += 1;
                }
                walk(body, *lhs, total);
                walk(body, *rhs, total);
            }
            Expr::UnaryOp { expr, .. } => walk(body, *expr, total),
            Expr::Ternary { condition, then_expr, else_expr } => {
                walk(body, *condition, total);
                walk(body, *then_expr, total);
                walk(body, *else_expr, total);
            }
            Expr::Call { callee, args } => {
                walk(body, *callee, total);
                for &arg in args.iter() {
                    walk(body, arg, total);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                walk(body, *receiver, total);
                for &arg in args.iter() {
                    walk(body, arg, total);
                }
            }
            Expr::Index { base, index } => {
                walk(body, *base, total);
                walk(body, *index, total);
            }
            Expr::Field { base, .. } => walk(body, *base, total),
            Expr::New { args, .. } => {
                for &arg in args.iter() {
                    walk(body, arg, total);
                }
            }
            Expr::Array(items) => {
                for &item in items.iter() {
                    walk(body, item, total);
                }
            }
            Expr::Await { expr } => walk(body, *expr, total),
            Expr::Missing | Expr::Literal(_) | Expr::Path(_) | Expr::QualifiedPath(_) => {}
        }
    }
    walk(body, expr, &mut total);
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::lower_method;
    use syntax::SyntaxKind;

    fn parse_and_lower(code: &str) -> Body {
        let parse = parser::parse(code);
        let root = parse.syntax_node();
        let method_node = root
            .descendants()
            .find(|n| matches!(n.kind(), SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF))
            .expect("Should have a method");
        let is_function = method_node.kind() == SyntaxKind::FUNCTION_DEF;
        let result = lower_method(&method_node, is_function);
        result.body
    }

    /// Empty body: every metric is at its zero state.
    #[test]
    fn empty_method_returns_zero_metrics() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
КонецПроцедуры
"#,
        );
        assert_eq!(compute_hir_metrics(&body), HirMethodMetrics::default());
    }

    /// Single straight-line statement: no decisions, no nesting.
    #[test]
    fn straight_line_keeps_zero_metrics() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Сообщить("Привет");
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.cognitive, 0);
        assert_eq!(m.max_nesting, 0);
        assert_eq!(m.if_condition_max, 0);
    }

    /// Single `If`: cognitive +1 (structural increment with nesting=0),
    /// max_nesting=1 (then-branch body), if_condition_max=0 (no AND/OR).
    #[test]
    fn single_if_metrics() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Если Истина Тогда
        Сообщить("");
    КонецЕсли;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.cognitive, 1);
        assert_eq!(m.max_nesting, 1);
        assert_eq!(m.if_condition_max, 0);
    }

    /// Nested if-while: outer If +1, While inside +1+1 nesting; max
    /// statement depth 2; one logical AND in the inner condition.
    #[test]
    fn nested_if_while_with_and_op() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Если А Тогда
        Пока Б И В Цикл
            Сообщить("");
        КонецЦикла;
    КонецЕсли;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        // If: 1 + 0; While: 1 + 1 + AND(+1) = 3 → total 4.
        assert_eq!(m.cognitive, 4);
        // Body inside While inside If is at depth 2.
        assert_eq!(m.max_nesting, 2);
        // The `Б И В` condition has one AND.
        assert_eq!(m.if_condition_max, 1);
    }

    /// `Если А И Б ИЛИ В Тогда`: condition has 2 logical ops; cognitive
    /// gets +1 (If) +2 (AND/OR) = 3.
    #[test]
    fn if_condition_max_picks_widest_condition() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Если А И Б ИЛИ В Тогда
        Сообщить("");
    КонецЕсли;
    Если Г Тогда
        Сообщить("");
    КонецЕсли;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.cognitive, 1 + 2 + 1);
        assert_eq!(m.if_condition_max, 2);
    }

    /// `Default::default()` matches an empty-method walk.
    #[test]
    fn default_matches_empty_metrics() {
        let body = parse_and_lower(
            r#"
Процедура Пусто()
КонецПроцедуры
"#,
        );
        assert_eq!(compute_hir_metrics(&body), HirMethodMetrics::default());
    }
}
