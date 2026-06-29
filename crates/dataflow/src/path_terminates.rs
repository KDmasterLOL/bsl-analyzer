use cfg::{CfgEdgeType, ControlFlowGraph};
use hir_def::body::Body;
use hir_def::hir::Stmt;
use hir_def::StmtId;
use la_arena::RawIdx;
use petgraph::graph::NodeIndex;
use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::{DataflowResult, DataflowSolver, Direction, Lattice, Transfer, DEFAULT_MAX_ITERATIONS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PathTerminatesConfig {
    pub loops_executed_at_least_once: bool,
}

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
    fn join(&self, other: &Self) -> Self {
        Self(self.0 || other.0)
    }

    fn join_in_place(&mut self, other: &Self) {
        self.0 |= other.0;
    }
}

pub struct PathTerminatesTransfer {
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
            CfgEdgeType::AdjacentCode => MayFallthrough::BOTTOM,
            _ => *state,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathTerminatesResult {
    inner: DataflowResult<MayFallthrough>,
}

impl PathTerminatesResult {
    pub fn may_fallthrough_at_block(&self, block: NodeIndex) -> bool {
        self.inner.block_in(block).copied().unwrap_or(MayFallthrough::BOTTOM).0
    }

    pub fn cfg(&self) -> &ControlFlowGraph {
        self.inner.cfg()
    }

    /// Approximate live heap bytes for Salsa's `memory_usage` report. The lattice
    /// value `MayFallthrough` is `Copy` and owns no heap, so only the block-state
    /// maps' backbone and the owned [`Body`] clone contribute.
    pub fn estimated_heap(&self) -> usize {
        self.inner.estimated_heap_with(|_| 0)
    }
}

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
    solver.set_initial_state_at_exit(MayFallthrough::TOP);
    Some(PathTerminatesResult { inner: solver.solve()? })
}

pub fn analyze_path_terminates_default(
    body: &Body,
    cfg: &ControlFlowGraph,
) -> Option<PathTerminatesResult> {
    analyze_path_terminates(body, cfg, PathTerminatesConfig::default(), DEFAULT_MAX_ITERATIONS)
}

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

    /// Approximate live heap bytes for Salsa's `memory_usage` report: the per-method
    /// results table plus each owned [`PathTerminatesResult`].
    pub fn estimated_heap(&self) -> usize {
        let mut bytes =
            crate::map_table_bytes::<u32, Arc<PathTerminatesResult>>(self.results.len());
        for result in self.results.values() {
            bytes += result.estimated_heap();
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfg::{BasicBlockVertex, CfgEdgeType, CfgVertex};
    use cfg_types::IdConversion as _;
    use hir_def::hir::{Expr, Literal, Stmt};

    fn alloc_noop(body: &mut Body) -> cfg_types::StmtId {
        let lit = body.alloc_expr(Expr::Literal(Literal::Bool(true)));
        body.alloc_stmt(Stmt::Expr(lit.to_idx()))
    }

    fn alloc_bool_true(body: &mut Body) -> cfg_types::ExprId {
        body.alloc_expr(Expr::Literal(Literal::Bool(true)))
    }

    fn alloc_return(body: &mut Body) -> cfg_types::StmtId {
        body.alloc_stmt(Stmt::Return { value: None })
    }

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

    #[test]
    fn linear_with_return_blocks_fallthrough() {
        let mut body = Body::new();
        let ret = alloc_return(&mut body);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(make_block(&[ret]));
        cfg.set_entry_point(entry);
        cfg.add_edge(entry, cfg.exit_point(), CfgEdgeType::Direct).unwrap();

        let r = analyze_path_terminates_default(&body, &cfg).expect("converges");
        assert!(!r.may_fallthrough_at_block(entry), "Return must kill fallthrough");
    }

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

    #[test]
    fn loop_break_carries_fallthrough_adjacent_does_not() {
        let mut body = Body::new();
        let break_block_stmts = alloc_noop(&mut body);
        let after_loop_stmt = alloc_noop(&mut body);
        let dead_after_break = alloc_noop(&mut body);

        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(make_block(&[break_block_stmts]));
        let after_loop = cfg.add_vertex(make_block(&[after_loop_stmt]));
        let dead = cfg.add_vertex(make_block(&[dead_after_break]));
        cfg.set_entry_point(entry);
        cfg.add_edge(entry, after_loop, CfgEdgeType::LoopBreak).unwrap();
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
