//! Stable CFG snapshot helpers for topology-focused tests.

use crate::{CfgEdgeType, CfgVertex, ControlFlowGraph};
use petgraph::algo::dominators::simple_fast;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use rustc_hash::FxHashMap;

/// Format a CFG using structural fingerprints instead of node indices.
///
/// Fingerprints intentionally exclude `NodeIndex` allocation order. Basic
/// blocks in the current CFG only store opaque HIR statement IDs, not a `Body`,
/// so ordinary non-empty basic blocks fall back to `CALL_STMT`; terminating
/// blocks use CFG-local edge topology to recover a more specific statement
/// kind where possible.
pub fn format_cfg(cfg: &ControlFlowGraph) -> String {
    let depths = dominator_depths(cfg);
    let mut base_fingerprints = FxHashMap::default();
    let mut base_counts = FxHashMap::default();

    for (idx, vertex) in cfg.vertices() {
        let base = base_fingerprint(cfg, idx, vertex, *depths.get(&idx).unwrap_or(&0));
        *base_counts.entry(base.clone()).or_insert(0usize) += 1;
        base_fingerprints.insert(idx, base);
    }

    let mut fingerprints = FxHashMap::default();
    for (idx, _) in cfg.vertices() {
        let base =
            base_fingerprints.get(&idx).expect("every CFG vertex must have a base fingerprint");
        if base_counts.get(base).copied().unwrap_or(0) > 1 {
            fingerprints
                .insert(idx, format!("{base}:#{}", predecessor_hash(cfg, idx, &base_fingerprints)));
        } else {
            fingerprints.insert(idx, base.clone());
        }
    }

    let mut blocks: Vec<_> = fingerprints.values().cloned().collect();
    blocks.sort();

    let mut edges: Vec<_> = cfg
        .graph()
        .edge_references()
        .map(|edge| {
            let from = fingerprints
                .get(&edge.source())
                .expect("edge source must have a fingerprint")
                .clone();
            let to = fingerprints
                .get(&edge.target())
                .expect("edge target must have a fingerprint")
                .clone();
            (from, to, edge_kind_name(*edge.weight()).to_owned())
        })
        .collect();
    edges.sort();

    let mut out = String::new();
    out.push_str("blocks:\n");
    for block in blocks {
        out.push_str("  ");
        out.push_str(&block);
        out.push('\n');
    }
    out.push_str("edges:\n");
    for (from, to, kind) in edges {
        out.push_str("  ");
        out.push_str(&from);
        out.push_str(" -> ");
        out.push_str(&to);
        out.push_str(" [");
        out.push_str(&kind);
        out.push_str("]\n");
    }
    out
}

fn dominator_depths(cfg: &ControlFlowGraph) -> FxHashMap<NodeIndex, usize> {
    let mut depths = FxHashMap::default();
    let Some(entry) = cfg.entry_point() else {
        return depths;
    };
    if !cfg.contains_vertex(entry) {
        return depths;
    }

    let dominators = simple_fast(cfg.graph(), entry);
    for (idx, _) in cfg.vertices() {
        let depth = dominators.strict_dominators(idx).map_or(0, Iterator::count);
        depths.insert(idx, depth);
    }
    depths
}

fn base_fingerprint(
    cfg: &ControlFlowGraph,
    idx: NodeIndex,
    vertex: &CfgVertex,
    dom_depth: usize,
) -> String {
    format!("{}:{}:{dom_depth}", role(cfg, idx), first_stmt_kind(cfg, idx, vertex))
}

fn role(cfg: &ControlFlowGraph, idx: NodeIndex) -> &'static str {
    if cfg.entry_point() == Some(idx) {
        "ENTRY"
    } else if cfg.exit_point() == idx {
        "EXIT"
    } else {
        "NORMAL"
    }
}

fn first_stmt_kind(cfg: &ControlFlowGraph, idx: NodeIndex, vertex: &CfgVertex) -> &'static str {
    match vertex {
        CfgVertex::BasicBlock(block) => {
            if block.is_empty() {
                "EMPTY"
            } else if has_outgoing_edge(cfg, idx, CfgEdgeType::LoopBreak) {
                "BREAK_STMT"
            } else if has_outgoing_edge(cfg, idx, CfgEdgeType::LoopContinue) {
                "CONTINUE_STMT"
            } else if has_adjacent_fallthrough(cfg, idx) && has_direct_label_target(cfg, idx) {
                "GOTO_STMT"
            } else if has_adjacent_fallthrough(cfg, idx) && has_direct_exit_target(cfg, idx) {
                "RETURN_STMT"
            } else if has_adjacent_fallthrough(cfg, idx) {
                "RAISE_STMT"
            } else {
                "CALL_STMT"
            }
        }
        CfgVertex::Conditional(_) => "IF_STMT",
        CfgVertex::WhileLoop(_) => "WHILE_STMT",
        CfgVertex::ForLoop(_) => "FOR_STMT",
        CfgVertex::ForEachLoop(_) => "FOR_EACH_STMT",
        CfgVertex::TryExcept(_) => "TRY_STMT",
        CfgVertex::Label(_) => "LABEL_STMT",
        CfgVertex::PreprocCondition(_) => "PRE_IF_DIR",
        CfgVertex::Exit => "EMPTY",
    }
}

fn has_outgoing_edge(cfg: &ControlFlowGraph, idx: NodeIndex, kind: CfgEdgeType) -> bool {
    cfg.outgoing_edges(idx).any(|(_, edge)| *edge == kind)
}

fn has_adjacent_fallthrough(cfg: &ControlFlowGraph, idx: NodeIndex) -> bool {
    has_outgoing_edge(cfg, idx, CfgEdgeType::AdjacentCode)
}

fn has_direct_label_target(cfg: &ControlFlowGraph, idx: NodeIndex) -> bool {
    cfg.outgoing_edges(idx).any(|(target, edge)| {
        *edge == CfgEdgeType::Direct && matches!(cfg.vertex(target), Some(CfgVertex::Label(_)))
    })
}

fn has_direct_exit_target(cfg: &ControlFlowGraph, idx: NodeIndex) -> bool {
    cfg.outgoing_edges(idx)
        .any(|(target, edge)| *edge == CfgEdgeType::Direct && target == cfg.exit_point())
}

fn predecessor_hash(
    cfg: &ControlFlowGraph,
    idx: NodeIndex,
    base_fingerprints: &FxHashMap<NodeIndex, String>,
) -> String {
    let mut parts: Vec<_> = cfg
        .incoming_edges(idx)
        .map(|(pred, edge)| {
            let pred_fingerprint =
                base_fingerprints.get(&pred).map(String::as_str).unwrap_or("UNKNOWN");
            format!("{pred_fingerprint}:{}", edge_kind_name(*edge))
        })
        .collect();
    parts.sort();

    let mut hash = 0xcbf29ce484222325u64;
    for part in parts {
        for byte in part.bytes().chain([0]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("{:08x}", hash as u32)
}

fn edge_kind_name(kind: CfgEdgeType) -> &'static str {
    match kind {
        CfgEdgeType::Direct => "Direct",
        CfgEdgeType::TrueBranch => "TrueBranch",
        CfgEdgeType::FalseBranch => "FalseBranch",
        CfgEdgeType::LoopIteration => "LoopIteration",
        CfgEdgeType::LoopBreak => "LoopBreak",
        CfgEdgeType::LoopContinue => "LoopContinue",
        CfgEdgeType::AdjacentCode => "AdjacentCode",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BasicBlockVertex;

    #[test]
    fn format_cfg_stable_across_block_renumber() {
        fn graph_a() -> ControlFlowGraph {
            let mut cfg = ControlFlowGraph::new();
            let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let left = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let right = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let merge = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            cfg.set_entry_point(entry);
            cfg.add_edge(entry, left, CfgEdgeType::TrueBranch).unwrap();
            cfg.add_edge(entry, right, CfgEdgeType::FalseBranch).unwrap();
            cfg.add_edge(left, merge, CfgEdgeType::Direct).unwrap();
            cfg.add_edge(right, merge, CfgEdgeType::Direct).unwrap();
            cfg.add_edge(merge, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
            cfg
        }

        fn graph_b() -> ControlFlowGraph {
            let mut cfg = ControlFlowGraph::new();
            let merge = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let right = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let left = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
            cfg.set_entry_point(entry);
            cfg.add_edge(entry, right, CfgEdgeType::FalseBranch).unwrap();
            cfg.add_edge(entry, left, CfgEdgeType::TrueBranch).unwrap();
            cfg.add_edge(right, merge, CfgEdgeType::Direct).unwrap();
            cfg.add_edge(left, merge, CfgEdgeType::Direct).unwrap();
            cfg.add_edge(merge, cfg.exit_point(), CfgEdgeType::Direct).unwrap();
            cfg
        }

        assert_eq!(format_cfg(&graph_a()), format_cfg(&graph_b()));
    }
}
