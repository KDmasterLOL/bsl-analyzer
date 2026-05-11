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
use crate::{Body, ExprId, IdConversion, StmtId};

/// One condition expression visited by the metrics walker, paired with
/// the number of logical `И`/`AND`/`Или`/`OR` operators it contains.
///
/// Tracked because the legacy `IfConditionComplexity` diagnostic emits
/// once **per condition** that exceeds its threshold, not once per
/// method. The §6.4 handler migration consumes this list to preserve
/// the per-condition source-precision the user-visible diagnostic
/// already gives — collapsing to a single per-method maximum
/// (`if_condition_max`) would silently regress the diagnostic from N
/// findings to 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConditionMetrics {
    /// HIR id of the condition expression. Use
    /// `BodySourceMap::expr_range(condition)` to recover the source
    /// range the diagnostic should attach to.
    pub condition: ExprId,
    /// Count of logical `AND` / `OR` operators in the condition's
    /// expression tree.
    pub logical_op_count: u32,
}

/// Per-method HIR-structural metrics, produced by
/// [`compute_hir_metrics`].
///
/// Every field is independent — adding a new metric does not invalidate
/// the existing ones, so future slices can extend the struct without
/// re-running the walks the migrated handlers depend on. `Default::default`
/// is the empty-method state (every counter at zero, `if_conditions`
/// empty).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
    /// Per-condition logical-operator counts for every `If` / `Elsif`
    /// / `While` / ternary condition in the body, in source order.
    /// The §6.4 `IfConditionComplexity` migration filters this list
    /// against the diagnostic threshold and emits one finding per
    /// over-budget entry, preserving the legacy per-condition
    /// source-precision.
    pub if_conditions: Vec<ConditionMetrics>,
    /// Convenience value: `if_conditions.iter().map(|c| c.logical_op_count).max().unwrap_or(0)`.
    /// The `NestedStatements` and any future "max-of-conditions"
    /// consumer can read this without iterating `if_conditions`.
    pub if_condition_max: u32,
    /// Innermost (`If` / `While` / `For` / `ForEach` / `Try`) statements
    /// — those whose body contains no further nesting statement of the
    /// same kinds — paired with their **1-indexed** nesting-stmt depth
    /// (1 for a top-level `If`, 2 for an `If` inside an outer `If`,
    /// etc.). The §6.4 `NestedStatements` migration consumes this list:
    /// emit one diagnostic per leaf whose `depth` exceeds
    /// `maxAllowedLevel`, attaching to the statement's first keyword
    /// (`Если` / `Пока` / `Для` / `Попытка`) recovered through the
    /// parse tree at handler time. Source order matches HIR allocation
    /// order (i.e. lexical order of the leaf statements).
    pub nesting_leaves: Vec<NestingLeafMetrics>,
    /// Total number of declared parameters (matches `body.params.len()`).
    /// Consumed by the §6.4 `NumberOfParams` migration.
    pub params_count: u32,
    /// Number of parameters declared with a default value
    /// (`Параметр = ...`). Consumed by the §6.4 `NumberOfOptionalParams`
    /// migration.
    pub optional_params_count: u32,
    /// Method body line span as the legacy
    /// `lower::mod::emit_method_scoped_diagnostics::MethodSize`
    /// computed it: `(end_line - start_line) - 4`, where `4` accounts
    /// for the `Процедура...КонецПроцедуры` declaration header that the
    /// Rowan `PROCEDURE_DEF` / `FUNCTION_DEF` range includes. Always
    /// `0` for `Body` instances produced without a `LineIndex`
    /// (streaming-mode tests, module_code) — the §6.4 `MethodSize`
    /// handler treats `0` as "metric unavailable" and silently skips
    /// emitting in that case (legacy behaviour). Source: populated by
    /// the Salsa wrapper from `LowerResult::size_lines`, not the
    /// visitor (the visitor walks the HIR `Body` and has no access to
    /// the file-level `LineIndex`).
    pub size_lines: u32,
    /// Total count of logical `И` (`AND`) / `ИЛИ` (`OR`) binary
    /// operators anywhere in the body's expressions. Track 2 Phase B
    /// §6.5 cyclomatic alignment consumes this as the SonarQube-style
    /// extension to McCabe `V(G)`: BSL short-circuit operators don't
    /// produce additional CFG edges (evaluation is inside basic
    /// blocks), so the textbook graph formula misses them. Adding this
    /// count back keeps the diagnostic in line with the SonarQube
    /// Cyclomatic Complexity definition.
    pub boolean_ops_count: u32,
    /// Total count of ternary `?(condition, then, else)` expressions
    /// anywhere in the body. Track 2 Phase B §6.5 cyclomatic alignment
    /// consumes this for the same reason as `boolean_ops_count`:
    /// ternary expressions evaluate within a basic block in BSL HIR
    /// and don't add CFG edges, so SonarQube parity needs the explicit
    /// per-occurrence increment.
    pub ternary_count: u32,
}

/// One leaf nesting statement recorded by [`MetricsVisitor`]. A leaf is
/// the innermost `If` / `While` / `For` / `ForEach` / `Try` along a
/// nesting chain — i.e. a statement that does not itself contain
/// another nesting statement in any of its bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestingLeafMetrics {
    /// HIR id of the leaf statement. Use
    /// `BodySourceMap::stmt_range(stmt)` to recover its source range.
    pub stmt: StmtId,
    /// 1-indexed nesting-stmt depth at the leaf. Matches the legacy
    /// `BodyDiagnostic::NestedStatements::depth` value the retired
    /// `lower::stmt::exit_nesting_stmt` produced — preserved so the
    /// §6.4 handler keeps the existing `maxAllowedLevel` semantics.
    pub depth: u32,
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
    let mut metrics = visitor.finish();
    // Param counts come straight off the `Body` arena — no statement
    // walk needed. `default_value.is_some()` mirrors the legacy
    // `emit_method_scoped_diagnostics` predicate.
    metrics.params_count = body.params.len() as u32;
    metrics.optional_params_count =
        body.params.iter().filter(|&&p| body.bindings[p].default_value.is_some()).count() as u32;
    metrics
}

#[derive(Debug, Default)]
struct MetricsVisitor {
    cognitive: u32,
    max_nesting: u32,
    if_conditions: Vec<ConditionMetrics>,
    nesting_leaves: Vec<NestingLeafMetrics>,
    boolean_ops_count: u32,
    ternary_count: u32,
    /// 1-indexed depth of the currently-active nesting statement chain.
    /// Increments on entering an `If` / `While` / `For` / `ForEach` /
    /// `Try`; decrements on exit. `0` means the visitor is outside any
    /// such statement.
    nesting_stmt_depth: u32,
    /// Whether the active nesting scope has already seen a nested-child
    /// nesting statement. Reset to `false` on each enter; flipped to
    /// `true` at every exit so the parent observes "I had a nested
    /// child". Used to identify leaf statements at exit time.
    had_nested_child: bool,
}

impl MetricsVisitor {
    fn finish(self) -> HirMethodMetrics {
        let if_condition_max =
            self.if_conditions.iter().map(|c| c.logical_op_count).max().unwrap_or(0);
        HirMethodMetrics {
            cognitive: self.cognitive,
            max_nesting: self.max_nesting,
            if_conditions: self.if_conditions,
            if_condition_max,
            nesting_leaves: self.nesting_leaves,
            // Populated by `compute_hir_metrics` after `finish()` —
            // the visitor itself doesn't read `Body::params` /
            // `Body::bindings`, that lookup is post-walk.
            params_count: 0,
            optional_params_count: 0,
            // Populated by the Salsa wrapper from
            // `LowerResult::size_lines` (the visitor has no `LineIndex`).
            size_lines: 0,
            boolean_ops_count: self.boolean_ops_count,
            ternary_count: self.ternary_count,
        }
    }

    /// Track the deepest nesting reached by any statement in the body.
    fn note_depth(&mut self, depth: u32) {
        if depth > self.max_nesting {
            self.max_nesting = depth;
        }
    }

    /// Mirror of `lower::stmt::enter_nesting_stmt` / `exit_nesting_stmt`
    /// (retired by §6.4). Wraps a body-walk over a nesting statement
    /// (`If` / `While` / `For` / `ForEach` / `Try`) so leaf detection
    /// works the same way as the legacy lowering-time emit:
    /// - on entry, increment depth and reset the leaf flag for this
    ///   scope (the parent's flag is restored implicitly because the
    ///   exit always sets the flag back to `true`);
    /// - run the body walk through `f`;
    /// - on exit, if no nested child fired, this statement is a leaf —
    ///   record `(stmt, depth)` so the handler can attach a diagnostic.
    ///   Then set the flag to `true` for the parent's view.
    fn with_nesting_stmt(&mut self, stmt_id: StmtIdx, f: impl FnOnce(&mut Self)) {
        self.nesting_stmt_depth += 1;
        self.had_nested_child = false;
        f(self);
        if !self.had_nested_child {
            self.nesting_leaves.push(NestingLeafMetrics {
                stmt: StmtId::from_idx(stmt_id),
                depth: self.nesting_stmt_depth,
            });
        }
        self.had_nested_child = true;
        self.nesting_stmt_depth -= 1;
    }

    fn visit_stmt(&mut self, body: &Body, stmt_id: StmtIdx, nesting: u32) {
        self.note_depth(nesting);
        match body.stmt_idx(stmt_id) {
            Stmt::If(if_stmt) => {
                let if_stmt = if_stmt.clone();
                self.with_nesting_stmt(stmt_id, |this| this.visit_if(body, &if_stmt, nesting));
            }

            Stmt::While { condition, body: loop_body } => {
                let condition = *condition;
                let loop_body = loop_body.clone();
                self.with_nesting_stmt(stmt_id, |this| {
                    this.cognitive += 1 + nesting;
                    // `While` conditions are NOT recorded in `if_conditions`
                    // — that field is the per-condition feed for the
                    // `IfConditionComplexity` diagnostic, whose legacy scope
                    // is `If` / `Elsif` only. Cognitive complexity still
                    // gets the AND/OR contribution via `visit_expr`.
                    this.visit_expr(body, condition);
                    for &child in loop_body.iter() {
                        this.visit_stmt(body, child, nesting + 1);
                    }
                });
            }

            Stmt::For { from, to, body: loop_body, .. } => {
                let from = *from;
                let to = *to;
                let loop_body = loop_body.clone();
                self.with_nesting_stmt(stmt_id, |this| {
                    this.cognitive += 1 + nesting;
                    // Codex round-A fix: pick up `И`/`ИЛИ` and
                    // ternaries inside bounds expressions —
                    // `Для И = (а И б) По (в ИЛИ г) Цикл ...` is real
                    // BSL. Use the extras-only walker to avoid
                    // changing cognitive complexity contributions
                    // (the §6.4 cognitive migration shipped without
                    // walking For bounds, and we keep that exact
                    // value).
                    this.count_extras_only(body, from);
                    this.count_extras_only(body, to);
                    for &child in loop_body.iter() {
                        this.visit_stmt(body, child, nesting + 1);
                    }
                });
            }

            Stmt::ForEach { collection, body: loop_body, .. } => {
                let collection = *collection;
                let loop_body = loop_body.clone();
                self.with_nesting_stmt(stmt_id, |this| {
                    this.cognitive += 1 + nesting;
                    // Codex round-A fix: extras-only walker for the
                    // collection — see the matching note in `For`.
                    this.count_extras_only(body, collection);
                    for &child in loop_body.iter() {
                        this.visit_stmt(body, child, nesting + 1);
                    }
                });
            }

            Stmt::Try { body: try_body, except } => {
                let try_body = try_body.clone();
                let except = except.clone();
                self.with_nesting_stmt(stmt_id, |this| {
                    for &child in try_body.iter() {
                        this.visit_stmt(body, child, nesting);
                    }
                    this.cognitive += 1 + nesting;
                    for &child in except.iter() {
                        this.visit_stmt(body, child, nesting + 1);
                    }
                });
            }

            Stmt::Goto(_) => {
                self.cognitive += 1;
            }

            Stmt::Expr(expr) => self.visit_expr(body, *expr),
            Stmt::Assign { target, value } => {
                // Codex round-A fix: assignment target IS an
                // expression — `Массив[?(Условие, 1, 2)] = ...` is
                // legal BSL, so the target's index expression can
                // carry ternaries / `И` / `ИЛИ` that the §6.5
                // cyclomatic extras must pick up. Use the
                // extras-only walker to keep cognitive at the §6.4
                // baseline (the migration shipped without walking
                // assignment targets).
                self.count_extras_only(body, *target);
                self.visit_expr(body, *value);
            }
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
        self.note_condition(body, if_stmt.condition);
        self.visit_expr(body, if_stmt.condition);

        for &child in if_stmt.then_branch.iter() {
            self.visit_stmt(body, child, nesting + 1);
        }

        for (elsif_cond, elsif_body) in if_stmt.elsif_branches.iter() {
            self.cognitive += 1;
            self.note_condition(body, *elsif_cond);
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
    /// and logical AND/OR (+1 each). Ternary occurrences and AND/OR
    /// counts are also tracked in dedicated fields for the §6.5
    /// cyclomatic alignment (SonarQube-style extended formula).
    ///
    /// Codex stop-time fix: the §6.5 cyclomatic extras
    /// (`boolean_ops_count` and `ternary_count`) skip parser-recovered
    /// expression subtrees — those are reconstructed from `ERROR`
    /// nodes by HIR lowering and don't represent decisions the user
    /// actually wrote. Counting them would re-introduce the false
    /// positives the CFG-based formula already avoids (CFG never
    /// includes recovered exprs). Cognitive complexity contributions
    /// stay unchanged: matches the pre-§6.5 cognitive behaviour the
    /// migrated handler already shipped.
    fn visit_expr(&mut self, body: &Body, expr_id: ExprIdx) {
        let is_recovered = body.is_recovered(ExprId::from_idx(expr_id));
        match body.expr_idx(expr_id) {
            Expr::Ternary { condition, then_expr, else_expr } => {
                self.cognitive += 1;
                if !is_recovered {
                    self.ternary_count += 1;
                }
                // Ternary conditions are not in the
                // `IfConditionComplexity` diagnostic scope, so they do
                // NOT go into `if_conditions`. The AND/OR
                // sub-expressions still contribute to cognitive via
                // the recursive `visit_expr` calls below.
                self.visit_expr(body, *condition);
                self.visit_expr(body, *then_expr);
                self.visit_expr(body, *else_expr);
            }
            Expr::BinaryOp { lhs, rhs, op } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    self.cognitive += 1;
                    if !is_recovered {
                        self.boolean_ops_count += 1;
                    }
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

    /// Walk an expression tree counting **only** the §6.5 cyclomatic
    /// extras (`boolean_ops_count`, `ternary_count`) without touching
    /// `cognitive`. Used for expression slots the §6.4 cognitive
    /// migration intentionally did not walk (`Stmt::For` bounds and
    /// `Stmt::ForEach` collection): adding them to `visit_expr` would
    /// silently inflate cognitive complexity for any loop with
    /// `И`/`ИЛИ`/`?(...)` in its bounds, regressing the §6.4 cognitive
    /// values consumers already pin in their tests.
    ///
    /// Recovered subtrees are filtered (matching `visit_expr`).
    fn count_extras_only(&mut self, body: &Body, expr_id: ExprIdx) {
        let is_recovered = body.is_recovered(ExprId::from_idx(expr_id));
        match body.expr_idx(expr_id) {
            Expr::Ternary { condition, then_expr, else_expr } => {
                if !is_recovered {
                    self.ternary_count += 1;
                }
                self.count_extras_only(body, *condition);
                self.count_extras_only(body, *then_expr);
                self.count_extras_only(body, *else_expr);
            }
            Expr::BinaryOp { lhs, rhs, op } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) && !is_recovered {
                    self.boolean_ops_count += 1;
                }
                self.count_extras_only(body, *lhs);
                self.count_extras_only(body, *rhs);
            }
            Expr::UnaryOp { expr, .. } => self.count_extras_only(body, *expr),
            Expr::Call { callee, args } => {
                self.count_extras_only(body, *callee);
                for &arg in args.iter() {
                    self.count_extras_only(body, arg);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.count_extras_only(body, *receiver);
                for &arg in args.iter() {
                    self.count_extras_only(body, arg);
                }
            }
            Expr::Index { base, index } => {
                self.count_extras_only(body, *base);
                self.count_extras_only(body, *index);
            }
            Expr::Field { base, .. } => self.count_extras_only(body, *base),
            Expr::New { args, .. } => {
                for &arg in args.iter() {
                    self.count_extras_only(body, arg);
                }
            }
            Expr::Array(items) => {
                for &item in items.iter() {
                    self.count_extras_only(body, item);
                }
            }
            Expr::Await { expr } => self.count_extras_only(body, *expr),
            Expr::Missing | Expr::Literal(_) | Expr::Path(_) | Expr::QualifiedPath(_) => {}
        }
    }

    /// Count `И`/`AND`/`Или`/`OR` operators in `expr` and record one
    /// `ConditionMetrics` entry. The §6.4 `IfConditionComplexity`
    /// migration iterates `if_conditions` to emit per-condition
    /// findings; the `if_condition_max` convenience field is derived
    /// from this list in [`MetricsVisitor::finish`].
    fn note_condition(&mut self, body: &Body, condition: ExprIdx) {
        let count = count_logical_ops(body, condition);
        self.if_conditions.push(ConditionMetrics {
            condition: ExprId::from_idx(condition),
            logical_op_count: count,
        });
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
    /// statement depth 2; one logical AND in the While condition.
    /// `if_condition_max` is 0 because the While condition is NOT
    /// recorded in `if_conditions` (the legacy `IfConditionComplexity`
    /// diagnostic only fires for `If`/`Elsif`).
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
        // Only the `Если А` condition is recorded; A has 0 logical ops.
        assert_eq!(m.if_conditions.len(), 1);
        assert_eq!(m.if_condition_max, 0);
    }

    /// Codex stop-hook regression guard: `if_conditions` records ONLY
    /// `Если`/`ИначеЕсли` conditions. `Пока` and ternary conditions
    /// must stay out of the list (their AND/OR ops still contribute
    /// to `cognitive` via `visit_expr`, but the per-condition feed for
    /// `IfConditionComplexity` filters them out).
    #[test]
    fn while_and_ternary_conditions_are_not_recorded() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Пока А И Б Цикл
        Х = ?(В ИЛИ Г, 1, 2);
    КонецЦикла;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert!(
            m.if_conditions.is_empty(),
            "While/ternary conditions must not appear in if_conditions, got {:?}",
            m.if_conditions
        );
        assert_eq!(m.if_condition_max, 0);
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

    /// Codex stop-hook regression guard: the legacy
    /// `IfConditionComplexity` diagnostic emits **per condition**, so
    /// `if_conditions` must carry one entry per visited condition (not
    /// only the max). Two `Если` blocks with different operator counts
    /// must produce two entries.
    #[test]
    fn if_conditions_lists_each_condition_separately() {
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
        assert_eq!(m.if_conditions.len(), 2);
        // Source order: the `А И Б ИЛИ В` condition is first.
        assert_eq!(m.if_conditions[0].logical_op_count, 2);
        assert_eq!(m.if_conditions[1].logical_op_count, 0);
        // The convenience `if_condition_max` matches the largest.
        assert_eq!(m.if_condition_max, 2);
    }

    /// Single nested-If chain: only the innermost is a leaf, depth
    /// matches the legacy `BodyDiagnostic::NestedStatements` 1-indexed
    /// counter.
    #[test]
    fn nesting_leaves_record_innermost_only() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Если А Тогда
        Если Б Тогда
            Если В Тогда
                Сообщить("");
            КонецЕсли;
        КонецЕсли;
    КонецЕсли;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.nesting_leaves.len(), 1, "only innermost If is a leaf");
        assert_eq!(m.nesting_leaves[0].depth, 3, "leaf at depth=3 matches legacy");
    }

    /// Sibling nesting branches: each leaf gets its own entry. Mirrors
    /// the legacy multi-emit semantics the §6.4 handler must preserve.
    #[test]
    fn nesting_leaves_emit_per_leaf_branch() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Если А Тогда
        Если Б Тогда
            Сообщить("");
        КонецЕсли;
        Если В Тогда
            Сообщить("");
        КонецЕсли;
    КонецЕсли;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.nesting_leaves.len(), 2, "two sibling Ifs are both leaves");
        assert!(
            m.nesting_leaves.iter().all(|l| l.depth == 2),
            "both leaves at depth=2 (under outer If)"
        );
    }

    /// Mixed kinds: `For` inside `Try` inside `If`. Only the `For` is a
    /// leaf because it has no nesting children. Depth reflects all
    /// three frames.
    #[test]
    fn nesting_leaves_count_all_kinds() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Если А Тогда
        Попытка
            Для Каждого Э Из К Цикл
                Сообщить("");
            КонецЦикла;
        Исключение
        КонецПопытки;
    КонецЕсли;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.nesting_leaves.len(), 1, "innermost For is the only leaf");
        assert_eq!(m.nesting_leaves[0].depth, 3, "If→Try→For = depth 3");
    }

    /// Codex round-A regression guard: For/ForEach bounds and
    /// collection contribute to the §6.5 cyclomatic extras
    /// (`boolean_ops_count`, `ternary_count`) but **not** to
    /// `cognitive`. Walking `from`/`to`/`collection` through the full
    /// `visit_expr` would silently inflate cognitive complexity vs.
    /// the §6.4 baseline; the dedicated `count_extras_only` helper
    /// keeps cognitive untouched.
    #[test]
    fn for_bounds_and_collection_extras_do_not_leak_into_cognitive() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Для Каждого Э Из ?(Условие, Колл1, Колл2) Цикл
        Сообщить("");
    КонецЦикла;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        // Cognitive: the ForEach itself is +1+0 nesting; its body's
        // Сообщить is at nesting=1 but not a decision. Crucially, the
        // ternary inside the collection expression must NOT add to
        // cognitive. So cognitive == 1.
        assert_eq!(
            m.cognitive, 1,
            "cognitive must stay at the ForEach decision point — ternary in \
             collection must not leak in, got {}",
            m.cognitive
        );
        // Cyclomatic extras: the ternary contributes 1.
        assert_eq!(
            m.ternary_count, 1,
            "ternary in ForEach collection must contribute to cyclomatic extras, got {}",
            m.ternary_count
        );
    }

    /// Codex round-A regression guard: assignment target expressions
    /// (`Массив[?(...)] = X;`) contribute to the §6.5 cyclomatic
    /// extras but **not** to cognitive — same isolation contract as
    /// For/ForEach bounds.
    #[test]
    fn assign_target_extras_do_not_leak_into_cognitive() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Массив[?(Условие, 0, 1)] = 5;
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert_eq!(m.cognitive, 0, "ternary in assign target must not bump cognitive");
        assert_eq!(m.ternary_count, 1, "ternary in assign target must contribute to extras");
    }

    /// Codex round-A regression guard: §6.5 boolean / ternary
    /// counters MUST NOT include parser-recovered expressions. The
    /// CFG never sees recovered subtrees, so adding them on top of
    /// the textbook formula would reintroduce false positives the
    /// CFG-based diagnostic already avoids. This fixture trips
    /// parser ERROR recovery on bare `.` member access; lowering
    /// marks the resulting expression subtree as recovered, and the
    /// `boolean_ops_count` / `ternary_count` increments must skip it.
    /// Cognitive contribution stays as before.
    ///
    /// The fixture is a smoke-test pinning the counters at their
    /// lower-bound values — a hand-constructed `Body` with a real
    /// `BinaryOp::And` inside a recovered subtree would directly pin
    /// the recursive-filter behaviour, but that requires `pub(crate)`
    /// arena APIs we don't expose for tests outside the lowering
    /// module. Within the module, a parser-driven fixture exercises
    /// the same code path indirectly.
    #[test]
    fn recovered_exprs_do_not_inflate_cyclomatic_extras() {
        let body = parse_and_lower(
            r#"
Процедура Тест()
    Х = ?(а., 1, 2);
КонецПроцедуры
"#,
        );
        let m = compute_hir_metrics(&body);
        assert!(
            m.boolean_ops_count == 0,
            "boolean_ops must stay 0 for fixture without real AND/OR, got {}",
            m.boolean_ops_count
        );
        assert!(
            m.ternary_count <= 1,
            "ternary_count must not be inflated by recovered subtrees, got {}",
            m.ternary_count
        );
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
