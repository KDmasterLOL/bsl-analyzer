//! Path-terminates analysis: backward dataflow that determines whether
//! execution can fall through from a given program point to the function's
//! exit without going through a `Return` or `Raise` statement.
//!
//! ## What is "may fallthrough"?
//!
//! A program point `p` may fallthrough if there exists at least one
//! execution path from `p` to the function's exit point that does NOT
//! cross a `Return` / `Raise` statement and does NOT use the dead-code
//! [`CfgEdgeType::AdjacentCode`] edge.
//!
//! For an entry block this is equivalent to the question
//! "does this function have at least one path that reaches its
//! `КонецФункции` / `КонецПроцедуры` without an explicit return?" — exactly
//! the predicate the `AllFunctionPathMustHaveReturn` diagnostic
//! (Track 1 §1.6) needs.
//!
//! ## Algorithm (backward)
//!
//! Lattice: [`MayFallthrough`] — a single boolean with `join = OR` (any
//! path → may fallthrough).
//! - `bottom = false` (no path reaches the exit without `Return`/`Raise`)
//! - `top = true`    (at least one such path exists)
//!
//! Boundary fact: `OUT[exit] = MayFallthrough(true)` — the exit point is
//! vacuously fallthrough-reachable from itself. Seeded via
//! [`crate::DataflowSolver::set_initial_state_at_exit`]; without this seed
//! the solver collapses to a trivial `false`-everywhere fixpoint.
//!
//! Transfer:
//! - per-statement: `Return` / `Raise` → `false`; everything else → identity.
//! - per-edge: [`CfgEdgeType::AdjacentCode`] (the dead-fallthrough successor
//!   of an unconditional jump) → `false`; everything else → identity. The
//!   live `LoopBreak` / `LoopContinue` and the synthetic `LoopIteration`
//!   pass through unchanged.
//!
//! ### Order invariance within a basic block
//!
//! The framework's [`crate::DataflowSolver::solve_backward`] applies
//! `transfer_stmt` to a basic block's statements in source order even
//! though the analysis is backward. For every other dataflow analysis in
//! this crate the per-stmt transfer is order-sensitive, so this would be
//! wrong — but for `MayFallthrough` it is provably correct: `Return` /
//! `Raise` is an *absorbing* terminator (its result is `false` regardless
//! of input), and identity preserves whatever boundary value arrived from
//! the exit side. So whether we sweep `[s1, s2, Return, s4]` forward or
//! backward, the fixpoint is the same `false`.

use cfg::{CfgEdgeType, ControlFlowGraph};
use hir_def::body::Body;
use hir_def::hir::Stmt;
use hir_def::StmtId;
use la_arena::RawIdx;
use petgraph::graph::NodeIndex;
use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::{DataflowResult, DataflowSolver, Direction, Lattice, Transfer, DEFAULT_MAX_ITERATIONS};

/// Configuration for the path-terminates analyser.
///
/// Mirrors [`cfg::CfgBuilder`]'s `produce_loop_iterations` knob on the
/// dataflow side: whether a loop is assumed to execute its body at least
/// once when computing fallthrough.
///
/// Default: `loops_executed_at_least_once = false` — a `Пока ... Цикл`
/// whose condition is statically true is still treated as
/// potentially-skippable for path-termination purposes. This matches the
/// BSL convention where the empty-iteration path through any loop counts
/// as "fallthrough" for the missing-return diagnostic (Track 1 §1.6
/// "loops_executed_at_least_once = false (default)" rationale).
///
/// **Setting `true` is not yet implemented and panics on use**
/// (see [`analyze_path_terminates`]). Reason: the framework's
/// [`Transfer::transfer_edge`] does not see the source vertex, so the
/// analyser cannot distinguish "FalseBranch from a loop header" from
/// "FalseBranch from a regular conditional" without an API change or a
/// pre-computed `is_loop_false_branch` lookup. Silently falling back to
/// `false` would return a knowingly-wrong result with no caller signal,
/// so the analyser fails fast instead. The field is kept on the struct
/// so call-sites can express intent and the implementation can fill in
/// later without churning the public signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PathTerminatesConfig {
    pub loops_executed_at_least_once: bool,
}

/// Lattice element for the path-terminates analysis.
///
/// Wraps a single boolean: "at this program point, may execution reach
/// the function's exit without an intervening `Return` / `Raise`?".
/// `true` = may fallthrough, `false` = no such path exists.
///
/// Lattice order `false ⊑ true`, `join = OR`. Bottom is `false` (the
/// join identity, used by [`crate::DataflowSolver::set_bottom_factory`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MayFallthrough(pub bool);

impl MayFallthrough {
    pub const BOTTOM: Self = Self(false);
    pub const TOP: Self = Self(true);

    #[inline]
    pub fn is_fallthrough(self) -> bool {
        self.0
    }
}

impl Lattice for MayFallthrough {
    /// `OR` — fallthrough at a join point if *any* successor may fallthrough.
    fn join(&self, other: &Self) -> Self {
        Self(self.0 || other.0)
    }

    fn join_in_place(&mut self, other: &Self) {
        self.0 |= other.0;
    }
}

/// Backward transfer for [`MayFallthrough`].
pub struct PathTerminatesTransfer {
    // Held for forward-compat with `loops_executed_at_least_once = true`;
    // see [`PathTerminatesConfig`].
    #[allow(dead_code)]
    config: PathTerminatesConfig,
}

impl PathTerminatesTransfer {
    pub fn new(config: PathTerminatesConfig) -> Self {
        Self { config }
    }
}

impl Transfer<MayFallthrough> for PathTerminatesTransfer {
    fn transfer_stmt(
        &self,
        stmt_id: RawIdx,
        state: &MayFallthrough,
        body: &Body,
    ) -> MayFallthrough {
        match body.stmt(StmtId::from_raw(stmt_id)) {
            Stmt::Return { .. } | Stmt::Raise { .. } => MayFallthrough::BOTTOM,
            _ => *state,
        }
    }

    fn transfer_stmt_in_place(&self, stmt_id: RawIdx, state: &mut MayFallthrough, body: &Body) {
        if matches!(body.stmt(StmtId::from_raw(stmt_id)), Stmt::Return { .. } | Stmt::Raise { .. })
        {
            *state = MayFallthrough::BOTTOM;
        }
    }

    fn transfer_edge(&self, edge_kind: CfgEdgeType, state: &MayFallthrough) -> MayFallthrough {
        match edge_kind {
            // Dead-fallthrough successor of `Прервать` / `Продолжить` /
            // `Возврат` / `goto` / `Raise` — execution cannot actually
            // reach the textually-next statement, so this edge contributes
            // no fallthrough information.
            CfgEdgeType::AdjacentCode => MayFallthrough::BOTTOM,
            _ => *state,
        }
    }
}

/// Result of the path-terminates analysis for a single method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathTerminatesResult {
    inner: DataflowResult<MayFallthrough>,
}

impl PathTerminatesResult {
    /// "May execution starting at the entry of `block` reach the exit
    /// without crossing a `Return` / `Raise`?"
    ///
    /// This is `IN[block]` of the backward analysis. Querying the CFG's
    /// entry vertex yields the predicate `AllFunctionPathMustHaveReturn`
    /// (Track 1 §1.6) actually wants.
    ///
    /// Returns `false` for vertices not in the CFG (defensive — should
    /// not happen for a well-formed input).
    pub fn may_fallthrough_at_block(&self, block: NodeIndex) -> bool {
        self.inner.block_in(block).copied().unwrap_or(MayFallthrough::BOTTOM).0
    }

    /// CFG inspected by this analysis. Useful for the consumer that
    /// wants to look up the entry point on the same graph instance the
    /// analyser saw.
    pub fn cfg(&self) -> &ControlFlowGraph {
        self.inner.cfg()
    }
}

/// Run path-terminates analysis on a single method's CFG.
///
/// Returns `None` only if the underlying [`DataflowSolver`] reports
/// non-convergence — for the 2-element `MayFallthrough` lattice this is
/// effectively unreachable (the worklist converges in one full RPO pass
/// for any DAG and in O(loop-nest depth) for graphs with cycles), but
/// the option type is kept to match the framework's contract.
///
/// Panics if `config.loops_executed_at_least_once` is `true` —
/// the `true` branch is not yet implemented (see
/// [`PathTerminatesConfig`] for the rationale). Silently falling back
/// to `false` would mask a config-level intent mismatch.
pub fn analyze_path_terminates(
    body: &Body,
    cfg: &ControlFlowGraph,
    config: PathTerminatesConfig,
    max_iterations: usize,
) -> Option<PathTerminatesResult> {
    assert!(
        !config.loops_executed_at_least_once,
        "PathTerminatesConfig::loops_executed_at_least_once = true is not yet implemented; \
         see crates/dataflow/src/path_terminates.rs PathTerminatesConfig docs for status"
    );
    let transfer = PathTerminatesTransfer::new(config);
    let mut solver = DataflowSolver::new(Arc::new(cfg.clone()), body.clone(), transfer);
    solver.set_direction(Direction::Backward);
    solver.set_max_iterations(max_iterations);
    solver.set_bottom_factory(|| MayFallthrough::BOTTOM);
    // Boundary fact: the exit is fallthrough-reachable from itself.
    // Without this seed `set_bottom_factory` leaves `OUT[exit] = false`
    // and the analysis collapses to a trivial false-everywhere fixpoint.
    solver.set_initial_state_at_exit(MayFallthrough::TOP);
    Some(PathTerminatesResult { inner: solver.solve()? })
}

/// Convenience wrapper around [`analyze_path_terminates`] using the
/// default [`DEFAULT_MAX_ITERATIONS`] cap.
pub fn analyze_path_terminates_default(
    body: &Body,
    cfg: &ControlFlowGraph,
) -> Option<PathTerminatesResult> {
    analyze_path_terminates(body, cfg, PathTerminatesConfig::default(), DEFAULT_MAX_ITERATIONS)
}

// ============================================================================
// Module-level batch result (for Salsa caching, mirrors `ModuleCfgs`)
// ============================================================================

/// Collection of [`PathTerminatesResult`] for every method in a module.
///
/// Built once per module and cached by Salsa via `module_path_terminates_query`.
/// Mirrors [`crate::liveness::ModuleLiveness`] / [`cfg::ModuleCfgs`] in shape
/// so callers can use the same `.get(local_id)` access pattern.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModulePathTerminates {
    results: FxHashMap<u32, Arc<PathTerminatesResult>>,
}

impl ModulePathTerminates {
    pub fn new(results: FxHashMap<u32, Arc<PathTerminatesResult>>) -> Self {
        Self { results }
    }

    pub fn get(&self, local_id: u32) -> Option<&Arc<PathTerminatesResult>> {
        self.results.get(&local_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, &Arc<PathTerminatesResult>)> + '_ {
        self.results.iter().map(|(&id, r)| (id, r))
    }

    pub fn len(&self) -> usize {
        self.results.len()
    }

    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }
}

// ============================================================================
// Tests — hand-rolled `Body` + `ControlFlowGraph`
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use cfg::{BasicBlockVertex, CfgEdgeType, CfgVertex};
    use cfg_types::IdConversion as _;
    use hir_def::hir::{Expr, Literal, Stmt};

    /// Allocate a no-op `Stmt::Expr(literal)` and return its `StmtId`.
    fn alloc_noop(body: &mut Body) -> cfg_types::StmtId {
        let lit = body.alloc_expr(Expr::Literal(Literal::Bool(true)));
        body.alloc_stmt(Stmt::Expr(lit.to_idx()))
    }

    /// Allocate a `Bool(true)` literal and return its opaque `ExprId` —
    /// the form needed by `cfg::ConditionalVertex.condition`.
    fn alloc_bool_true(body: &mut Body) -> cfg_types::ExprId {
        body.alloc_expr(Expr::Literal(Literal::Bool(true)))
    }

    /// Allocate a `Stmt::Return { value: None }` and return its `StmtId`.
    fn alloc_return(body: &mut Body) -> cfg_types::StmtId {
        body.alloc_stmt(Stmt::Return { value: None })
    }

    /// Allocate a `Stmt::Raise { value: None }` and return its `StmtId`.
    fn alloc_raise(body: &mut Body) -> cfg_types::StmtId {
        body.alloc_stmt(Stmt::Raise { value: None })
    }

    fn make_block(stmts: &[cfg_types::StmtId]) -> CfgVertex {
        let mut block = BasicBlockVertex::new();
        for &s in stmts {
            block.add_statement(s);
        }
        CfgVertex::BasicBlock(block)
    }

    /// `Функция Тест() X = 1; КонецФункции` — function falls through to
    /// the end without a `Return`. `may_fallthrough_at_block(entry)` must
    /// be `true`, which is what `AllFunctionPathMustHaveReturn` will
    /// later turn into a diagnostic.
    #[test]
    fn linear_no_return_fallthroughs() {
        let mut body = Body::new();
        let s1 = alloc_noop(&mut body);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(make_block(&[s1]));
        cfg.set_entry_point(entry);
        cfg.add_edge(entry, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let r = analyze_path_terminates_default(&body, &cfg).expect("converges");
        assert!(r.may_fallthrough_at_block(entry), "linear body without Return must fallthrough");
    }

    /// `Функция Тест() Возврат; КонецФункции` — every path returns. The
    /// transfer kills fallthrough at the `Return` statement, so
    /// `IN[entry] = false`.
    #[test]
    fn linear_with_return_blocks_fallthrough() {
        let mut body = Body::new();
        let ret = alloc_return(&mut body);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(make_block(&[ret]));
        cfg.set_entry_point(entry);
        // The CFG builder for a real `Возврат;` would emit an
        // `AdjacentCode` edge to the textually-next statement; here the
        // `Return` is last so we just point at the exit. Both routings
        // exercise the same transfer (Return → false at the stmt level).
        cfg.add_edge(entry, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let r = analyze_path_terminates_default(&body, &cfg).expect("converges");
        assert!(!r.may_fallthrough_at_block(entry), "Return must kill fallthrough");
    }

    /// `Функция Тест() ВызватьИсключение; КонецФункции` — `Raise`
    /// terminates the path identically to `Return`.
    #[test]
    fn raise_blocks_fallthrough() {
        let mut body = Body::new();
        let raise = alloc_raise(&mut body);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(make_block(&[raise]));
        cfg.set_entry_point(entry);
        cfg.add_edge(entry, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let r = analyze_path_terminates_default(&body, &cfg).expect("converges");
        assert!(!r.may_fallthrough_at_block(entry), "Raise must kill fallthrough");
    }

    /// Diamond: `Если cond Тогда Возврат; Иначе ничего; КонецЕсли` — the
    /// then-branch returns, the else-branch falls through. Join at the
    /// merge point is `OR` so the function itself may fallthrough — and
    /// `entry` should reflect that.
    #[test]
    fn if_then_return_else_open_fallthroughs() {
        let mut body = Body::new();
        let then_ret = alloc_return(&mut body);
        let else_noop = alloc_noop(&mut body);
        let cond_expr = alloc_bool_true(&mut body);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(make_block(&[]));
        let cond =
            cfg.add_vertex(CfgVertex::Conditional(cfg::ConditionalVertex { condition: cond_expr }));
        let then_block = cfg.add_vertex(make_block(&[then_ret]));
        let else_block = cfg.add_vertex(make_block(&[else_noop]));
        let merge = cfg.add_vertex(make_block(&[]));
        cfg.set_entry_point(entry);
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, then_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, else_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(then_block, merge, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(else_block, merge, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(merge, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let r = analyze_path_terminates_default(&body, &cfg).expect("converges");
        assert!(
            r.may_fallthrough_at_block(entry),
            "open else-branch keeps fallthrough alive at entry"
        );
        assert!(
            !r.may_fallthrough_at_block(then_block),
            "then-branch ends in Return; its IN should be false"
        );
        assert!(r.may_fallthrough_at_block(else_block), "else-branch is open");
    }

    /// Both branches return → no fallthrough at entry. Mirror of the
    /// existing `test_no_diagnostic_simple_if_else_both_return` fixture
    /// in `all_function_path_must_have_return`.
    #[test]
    fn if_both_return_blocks_fallthrough() {
        let mut body = Body::new();
        let then_ret = alloc_return(&mut body);
        let else_ret = alloc_return(&mut body);
        let cond_expr = alloc_bool_true(&mut body);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(make_block(&[]));
        let cond =
            cfg.add_vertex(CfgVertex::Conditional(cfg::ConditionalVertex { condition: cond_expr }));
        let then_block = cfg.add_vertex(make_block(&[then_ret]));
        let else_block = cfg.add_vertex(make_block(&[else_ret]));
        let merge = cfg.add_vertex(make_block(&[]));
        cfg.set_entry_point(entry);
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, then_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, else_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(then_block, merge, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(else_block, merge, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(merge, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let r = analyze_path_terminates_default(&body, &cfg).expect("converges");
        assert!(
            !r.may_fallthrough_at_block(entry),
            "both branches Return → entry has no fallthrough path"
        );
    }

    /// `AdjacentCode` is dead — execution cannot really cross it, so it
    /// must contribute nothing to fallthrough. Build a tiny graph where
    /// `entry → noop_block` is `AdjacentCode`, and assert IN[entry] is
    /// `false` (no live path to exit). This pins the `transfer_edge`
    /// behaviour for the dead-fallthrough successor of an unconditional
    /// jump.
    #[test]
    fn adjacent_code_edge_kills_fallthrough() {
        let mut body = Body::new();
        let s1 = alloc_noop(&mut body);
        let s2 = alloc_noop(&mut body);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(make_block(&[s1]));
        let dead = cfg.add_vertex(make_block(&[s2]));
        cfg.set_entry_point(entry);
        cfg.add_edge(entry, dead, CfgEdgeType::AdjacentCode).unwrap();
        cfg.add_edge(dead, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let r = analyze_path_terminates_default(&body, &cfg).expect("converges");
        assert!(!r.may_fallthrough_at_block(entry), "AdjacentCode must not propagate fallthrough");
    }

    /// Dual edges: a `Прервать` block has TWO outgoing edges per
    /// builder.rs Step C — `LoopBreak` (live) to the after-loop merge
    /// and `AdjacentCode` (dead) to the textually-next statement. Verify
    /// the live `LoopBreak` does carry fallthrough through to the
    /// predecessor, while the parallel `AdjacentCode` carries none.
    #[test]
    fn loop_break_carries_fallthrough_adjacent_does_not() {
        let mut body = Body::new();
        let break_block_stmts = alloc_noop(&mut body); // stand-in for `Прервать` placeholder
        let after_loop_stmt = alloc_noop(&mut body);
        let dead_after_break = alloc_noop(&mut body);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(make_block(&[break_block_stmts]));
        let after_loop = cfg.add_vertex(make_block(&[after_loop_stmt]));
        let dead = cfg.add_vertex(make_block(&[dead_after_break]));
        cfg.set_entry_point(entry);
        // Live break edge to the loop-exit merge.
        cfg.add_edge(entry, after_loop, CfgEdgeType::LoopBreak).unwrap();
        // Parallel dead-fallthrough edge (the textually-next stmt that
        // can't actually run because the block ended in `Прервать`).
        cfg.add_edge(entry, dead, CfgEdgeType::AdjacentCode).unwrap();
        cfg.add_edge(after_loop, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        cfg.add_edge(dead, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let r = analyze_path_terminates_default(&body, &cfg).expect("converges");
        assert!(
            r.may_fallthrough_at_block(entry),
            "LoopBreak is a live edge → fallthrough must reach entry through after_loop"
        );
        assert!(r.may_fallthrough_at_block(after_loop), "after_loop has a direct path to exit");
    }

    /// `Попытка Возврат; Исключение Возврат; КонецПопытки;` — both the
    /// try-body and the except-body return. The CFG models this with a
    /// `TryExceptVertex` whose two outgoing live edges (one for the
    /// success path, one for the exception path) merge into a join
    /// block. Even though `TryExceptVertex` is structurally distinct
    /// from `Conditional`, the path-terminates lattice does not
    /// special-case vertex kinds — what matters is that both successor
    /// paths kill fallthrough at their respective `Return` statements
    /// and the join is `OR`-of-`false`-and-`false` = `false`. Mirrors
    /// the existing `test_no_diagnostic_try_except_both_return` fixture
    /// in `all_function_path_must_have_return`.
    #[test]
    fn try_both_return_blocks_fallthrough() {
        let mut body = Body::new();
        let try_ret = alloc_return(&mut body);
        let except_ret = alloc_return(&mut body);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(make_block(&[]));
        let try_vertex = cfg.add_vertex(CfgVertex::TryExcept(cfg::TryExceptVertex::new()));
        let try_block = cfg.add_vertex(make_block(&[try_ret]));
        let except_block = cfg.add_vertex(make_block(&[except_ret]));
        let merge = cfg.add_vertex(make_block(&[]));
        cfg.set_entry_point(entry);
        cfg.add_edge(entry, try_vertex, CfgEdgeType::Direct).unwrap();
        // Both edges are `Direct` (live) — TryExceptVertex does not
        // gate edges on TrueBranch/FalseBranch the way Conditional
        // does; both arms are reachable.
        cfg.add_edge(try_vertex, try_block, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(try_vertex, except_block, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(try_block, merge, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(except_block, merge, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(merge, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let r = analyze_path_terminates_default(&body, &cfg).expect("converges");
        assert!(
            !r.may_fallthrough_at_block(entry),
            "Try+Except both Return → entry has no fallthrough path"
        );
        assert!(!r.may_fallthrough_at_block(try_block), "try-body ends in Return");
        assert!(!r.may_fallthrough_at_block(except_block), "except-body ends in Return");
    }

    /// Asymmetric try/except: try-body falls through, except-body
    /// returns. Entry must report fallthrough (the success path keeps
    /// fallthrough alive even though the exception path terminates).
    #[test]
    fn try_open_except_return_fallthroughs() {
        let mut body = Body::new();
        let try_noop = alloc_noop(&mut body);
        let except_ret = alloc_return(&mut body);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(make_block(&[]));
        let try_vertex = cfg.add_vertex(CfgVertex::TryExcept(cfg::TryExceptVertex::new()));
        let try_block = cfg.add_vertex(make_block(&[try_noop]));
        let except_block = cfg.add_vertex(make_block(&[except_ret]));
        let merge = cfg.add_vertex(make_block(&[]));
        cfg.set_entry_point(entry);
        cfg.add_edge(entry, try_vertex, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(try_vertex, try_block, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(try_vertex, except_block, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(try_block, merge, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(except_block, merge, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(merge, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let r = analyze_path_terminates_default(&body, &cfg).expect("converges");
        assert!(
            r.may_fallthrough_at_block(entry),
            "open try-body keeps fallthrough alive at entry"
        );
        assert!(r.may_fallthrough_at_block(try_block), "try-body has no Return");
        assert!(!r.may_fallthrough_at_block(except_block), "except-body returns");
    }

    /// Stub config flag must fail-fast, not silently fall back to the
    /// `false` behaviour. See [`PathTerminatesConfig`] for rationale.
    #[test]
    #[should_panic(expected = "loops_executed_at_least_once = true is not yet implemented")]
    fn loops_executed_at_least_once_true_panics() {
        let body = Body::new();
        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(make_block(&[]));
        cfg.set_entry_point(entry);
        cfg.add_edge(entry, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        let _ = analyze_path_terminates(
            &body,
            &cfg,
            PathTerminatesConfig { loops_executed_at_least_once: true },
            DEFAULT_MAX_ITERATIONS,
        );
    }

    /// Lattice law spot-checks for `MayFallthrough`.
    #[test]
    fn lattice_join_is_or() {
        assert_eq!(MayFallthrough(true).join(&MayFallthrough(false)), MayFallthrough(true));
        assert_eq!(MayFallthrough(false).join(&MayFallthrough(false)), MayFallthrough(false));
        assert_eq!(MayFallthrough(true).join(&MayFallthrough(true)), MayFallthrough(true));
    }

    #[test]
    fn lattice_bottom_is_join_identity() {
        let bottom = MayFallthrough::BOTTOM;
        for x in [MayFallthrough(false), MayFallthrough(true)] {
            assert_eq!(bottom.join(&x), x, "bottom.join(x) == x");
            assert_eq!(x.join(&bottom), x, "x.join(bottom) == x");
        }
    }

    #[test]
    fn lattice_join_is_commutative_and_idempotent() {
        let t = MayFallthrough(true);
        let f = MayFallthrough(false);
        assert_eq!(t.join(&f), f.join(&t));
        assert_eq!(t.join(&t), t);
        assert_eq!(f.join(&f), f);
    }

    #[test]
    fn module_path_terminates_collection_basic() {
        let mut results: FxHashMap<u32, Arc<PathTerminatesResult>> = FxHashMap::default();
        let body = Body::new();
        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(make_block(&[]));
        cfg.set_entry_point(entry);
        cfg.add_edge(entry, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
        let r = analyze_path_terminates_default(&body, &cfg).expect("converges");
        results.insert(0, Arc::new(r));

        let collection = ModulePathTerminates::new(results);
        assert_eq!(collection.len(), 1);
        assert!(collection.get(0).is_some());
        assert!(collection.get(42).is_none());
    }
}
