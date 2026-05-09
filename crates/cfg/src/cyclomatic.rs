//! McCabe's cyclomatic complexity, computed directly from the CFG.
//!
//! Track 2 §6.2 (Phase B). The legacy implementation in
//! `hir-def/src/cyclomatic_complexity.rs` walked the HIR statement
//! arena and incremented per syntactic decision point — a structural
//! count that approximates `V(G)` but does not benefit from the CFG
//! the rest of the dataflow stack already builds. This module computes
//! the formula directly:
//!
//! ```text
//!     V(G) = E - N + 2 * P
//! ```
//!
//! where `N` is the number of nodes reachable from the CFG entry,
//! `E` is the number of live outgoing edges from those nodes, and
//! `P` is the number of connected components (always 1 for a single
//! method body — the CFG always has one entry and one virtual exit).
//!
//! # What "live edges" means
//!
//! [`CfgEdgeType::AdjacentCode`] is the synthetic placeholder the
//! builder emits when an unconditional control flow (`Прервать` /
//! `Продолжить` / `Возврат`) leaves a basic block but the lowering
//! still needs an edge to the next statement so the AST → CFG
//! mapping stays total. Counting that edge inflates `V(G)` and
//! disagrees with every textbook example. The formula here filters
//! it out via [`CfgEdgeType::is_dead_code_edge`].
//!
//! Loop back-edges (`LoopIteration` / `LoopContinue`) and explicit
//! loop exits (`LoopBreak`) are real control flow and are counted.
//!
//! # Why this lives in `cfg`
//!
//! Cyclomatic complexity is a graph-theoretic property of the CFG.
//! Putting it next to the graph it operates on (rather than in
//! `hir-def`, which lowered the syntax) closes the ROADMAP §Track 2
//! contract that "graph-based metrics live in `cfg`".

use crate::graph::ControlFlowGraph;
use petgraph::visit::EdgeRef;

/// Compute McCabe's cyclomatic complexity over `cfg`.
///
/// Returns the linearly-independent path count for the method whose
/// CFG was passed in. The minimum value for any method is `1`
/// (straight-line code with no decisions).
///
/// `V(G) = E - N + 2 * P` with `P = 1` (every method's CFG is one
/// connected component rooted at the entry). Nodes unreachable from
/// the entry are excluded — they are dead code that contributes no
/// runtime decision points.
///
/// Edge filter: [`CfgEdgeType::is_dead_code_edge`] removes
/// `AdjacentCode` placeholder edges. Every other edge kind
/// (`Direct`, `TrueBranch`, `FalseBranch`, `LoopIteration`,
/// `LoopContinue`, `LoopBreak`) is a real control transfer and counts.
///
/// Empty CFGs (no entry point) return `1` as the conventional base.
pub fn cyclomatic_complexity(cfg: &ControlFlowGraph) -> u32 {
    let Some(entry) = cfg.entry_point() else {
        return 1;
    };

    // Live-edges-only BFS from the entry. The cfg's own
    // `reverse_postorder` follows every edge including
    // `AdjacentCode` placeholders, so a block reachable only via
    // dead-code would land in `N` while its live outgoing edges
    // would also land in `E` — desynchronising the formula. Walking
    // through `is_dead_code_edge`-filtered successors keeps `N` and
    // `E` coupled.
    let graph = cfg.graph();
    let mut reachable: rustc_hash::FxHashSet<_> = rustc_hash::FxHashSet::default();
    reachable.insert(entry);
    let mut stack = vec![entry];
    while let Some(node) = stack.pop() {
        for edge in graph.edges(node) {
            if edge.weight().is_dead_code_edge() {
                continue;
            }
            let target = edge.target();
            if reachable.insert(target) {
                stack.push(target);
            }
        }
    }
    let node_count = reachable.len() as i64;

    // Count live edges among reachable nodes (the same filter the
    // BFS used; reapplying it here is the cheapest way to keep the
    // two counts in lockstep without threading mutable state through
    // the walk above).
    let mut edge_count: i64 = 0;
    for &node in &reachable {
        for edge in graph.edges(node) {
            if edge.weight().is_dead_code_edge() {
                continue;
            }
            if !reachable.contains(&edge.target()) {
                continue;
            }
            edge_count += 1;
        }
    }

    // V(G) = E - N + 2 * P, with P = 1. Signed math is mandatory:
    // for a tree-shaped CFG `E = N - 1`, so the subtraction goes
    // negative before `+ 2` brings it back to 1. The minimum
    // cyclomatic complexity for any non-empty method body is 1
    // (straight-line code with no decisions); clamp accordingly to
    // guard against a builder regression that produces `E + 2 < N`.
    let v = edge_count - node_count + 2;
    v.max(1) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::CfgEdgeType;
    use crate::vertex::{BasicBlockVertex, CfgVertex, ConditionalVertex};
    use cfg_types::ExprId;
    use la_arena::RawIdx;

    fn fresh_block() -> CfgVertex {
        CfgVertex::BasicBlock(BasicBlockVertex::new())
    }

    fn fresh_conditional() -> CfgVertex {
        CfgVertex::Conditional(ConditionalVertex::new(ExprId::from_raw(RawIdx::from(0u32))))
    }

    /// `ControlFlowGraph::new()` materialises a virtual `Exit` node at
    /// construction time; tests use `cfg.exit_point()` to wire up
    /// terminal edges instead of inserting a second exit node.
    fn build_cfg() -> ControlFlowGraph {
        ControlFlowGraph::new()
    }

    /// Empty CFG (no entry) — conventional base value of 1.
    #[test]
    fn empty_cfg_returns_one() {
        let cfg = ControlFlowGraph::new();
        assert_eq!(cyclomatic_complexity(&cfg), 1);
    }

    /// Linear method: entry → block1 → exit.
    /// N=3, E=2, V(G) = 2 - 3 + 2 = 1.
    #[test]
    fn straight_line_returns_one() {
        let mut cfg = build_cfg();
        let entry = cfg.add_vertex(fresh_block());
        let mid = cfg.add_vertex(fresh_block());
        let exit = cfg.exit_point();
        cfg.add_edge(entry, mid, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(mid, exit, CfgEdgeType::Direct).unwrap();
        cfg.set_entry_point(entry);
        assert_eq!(cyclomatic_complexity(&cfg), 1);
    }

    /// `Если cond Тогда A Иначе B КонецЕсли;`
    /// N=5 (entry, cond, A, B, exit), E=5. V(G) = 5 - 5 + 2 = 2.
    #[test]
    fn single_if_else_returns_two() {
        let mut cfg = build_cfg();
        let entry = cfg.add_vertex(fresh_block());
        let cond = cfg.add_vertex(fresh_conditional());
        let then_block = cfg.add_vertex(fresh_block());
        let else_block = cfg.add_vertex(fresh_block());
        let exit = cfg.exit_point();
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, then_block, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, else_block, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(then_block, exit, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(else_block, exit, CfgEdgeType::Direct).unwrap();
        cfg.set_entry_point(entry);
        assert_eq!(cyclomatic_complexity(&cfg), 2);
    }

    /// `AdjacentCode` placeholder edges are excluded. The unreachable
    /// node they target is also unreachable from the entry, so
    /// `reverse_postorder` drops it — N=3, E=2, V(G) = 1.
    #[test]
    fn adjacent_code_edge_is_excluded() {
        let mut cfg = build_cfg();
        let entry = cfg.add_vertex(fresh_block());
        let mid = cfg.add_vertex(fresh_block());
        let exit = cfg.exit_point();
        let unreachable = cfg.add_vertex(fresh_block());
        cfg.add_edge(entry, mid, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(mid, exit, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(mid, unreachable, CfgEdgeType::AdjacentCode).unwrap();
        cfg.set_entry_point(entry);
        assert_eq!(cyclomatic_complexity(&cfg), 1);
    }

    /// `Пока cond Цикл body КонецЦикла;`
    /// N=4, live E=4 (entry→cond, cond⇒body, body→cond back-edge,
    /// cond⇒exit). V(G) = 4 - 4 + 2 = 2.
    #[test]
    fn while_loop_returns_two() {
        let mut cfg = build_cfg();
        let entry = cfg.add_vertex(fresh_block());
        let cond = cfg.add_vertex(fresh_conditional());
        let body = cfg.add_vertex(fresh_block());
        let exit = cfg.exit_point();
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, body, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(body, cond, CfgEdgeType::LoopIteration).unwrap();
        cfg.add_edge(cond, exit, CfgEdgeType::FalseBranch).unwrap();
        cfg.set_entry_point(entry);
        assert_eq!(cyclomatic_complexity(&cfg), 2);
    }

    /// Codex stop-hook regression guard: a block reachable ONLY via a
    /// dead-code (`AdjacentCode`) edge must NOT count toward `N`. The
    /// previous implementation used `reverse_postorder`, which walks
    /// every edge regardless of kind, so the orphan block + its live
    /// outgoing edge both leaked into the formula. The live-edges-only
    /// BFS implemented here filters them out:
    ///
    /// entry → live → exit (live `Direct` edges)
    /// live → orphan (dead `AdjacentCode`)
    /// orphan → exit (live `Direct`)
    ///
    /// `orphan` is unreachable along live edges, so `N=3` (entry,
    /// live, exit) and `E=2` (entry→live, live→exit). `orphan→exit`
    /// is dropped because `orphan` itself is excluded from the
    /// reachable set. V(G) = 2 - 3 + 2 = 1.
    #[test]
    fn dead_edge_reachability_excludes_orphan() {
        let mut cfg = build_cfg();
        let entry = cfg.add_vertex(fresh_block());
        let live = cfg.add_vertex(fresh_block());
        let orphan = cfg.add_vertex(fresh_block());
        let exit = cfg.exit_point();
        cfg.add_edge(entry, live, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(live, exit, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(live, orphan, CfgEdgeType::AdjacentCode).unwrap();
        cfg.add_edge(orphan, exit, CfgEdgeType::Direct).unwrap();
        cfg.set_entry_point(entry);
        assert_eq!(cyclomatic_complexity(&cfg), 1);
    }

    /// Nested if inside while: N=6, live E=7, V(G) = 7 - 6 + 2 = 3.
    #[test]
    fn nested_if_in_while_returns_three() {
        let mut cfg = build_cfg();
        let entry = cfg.add_vertex(fresh_block());
        let cond_w = cfg.add_vertex(fresh_conditional());
        let cond_i = cfg.add_vertex(fresh_conditional());
        let body = cfg.add_vertex(fresh_block());
        let skip = cfg.add_vertex(fresh_block());
        let exit = cfg.exit_point();
        cfg.add_edge(entry, cond_w, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond_w, cond_i, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond_i, body, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond_i, skip, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(body, cond_w, CfgEdgeType::LoopIteration).unwrap();
        cfg.add_edge(skip, cond_w, CfgEdgeType::LoopIteration).unwrap();
        cfg.add_edge(cond_w, exit, CfgEdgeType::FalseBranch).unwrap();
        cfg.set_entry_point(entry);
        assert_eq!(cyclomatic_complexity(&cfg), 3);
    }
}
