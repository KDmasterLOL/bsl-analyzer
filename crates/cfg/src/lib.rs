//! # BSL Control Flow Graph (CFG)
//!
//! Control Flow Graph construction and analysis for BSL Language Server.
//!
//! ## Overview
//!
//! The Control Flow Graph represents the flow of execution through a BSL method/function.
//! Each node (vertex) in the graph represents a basic block or control structure, and edges
//! represent possible execution paths.
//!
//! ## Architecture
//!
//! - **CfgVertex**: Nodes in the graph (basic blocks, conditionals, loops, etc.)
//! - **CfgEdgeType**: Types of edges (direct, true/false branches, loop iterations)
//! - **ControlFlowGraph**: The graph structure itself
//! - **CfgBuilder**: Constructs CFG from Rowan AST
//!
//! ## Loop-context semantics (Track 1 §1.3)
//!
//! `Прервать` / `Продолжить` / `Перейти` are modelled as **live**
//! [`CfgEdgeType::LoopBreak`] / [`CfgEdgeType::LoopContinue`] /
//! direct edges — never as dead-fallthrough
//! [`CfgEdgeType::AdjacentCode`]. Each `Прервать` from within a
//! loop emits a single live edge to the after-loop merge block;
//! `Продолжить` emits a live edge to the loop header. The
//! after-loop block is the *same* `BasicBlock` the loop's
//! false-branch targets — `CfgBuilder` reuses it as a merge
//! point rather than synthesising a separate `LoopExit` vertex
//! (one merge per loop is sufficient; avoiding the extra vertex
//! kind keeps the dataflow framework simpler).
//!
//! [`CfgEdgeType::AdjacentCode`] marks the **dead** fallthrough
//! successor of an unconditional jump (`Возврат`,
//! `ВызватьИсключение`, `Прервать`, `Продолжить`, `Перейти`).
//! It's the edge-type convention used by the builder to encode
//! "the textually next statement that no execution path can
//! actually reach"; analyses pick it out via
//! [`CfgEdgeType::is_dead_code_edge`]. Track 1's dataflow
//! framework — both `path_terminates` (backward) and
//! `temp_resource` (forward) — drops this edge to bottom in
//! `transfer_edge`, so paths that never actually reach the next
//! block do not seed exit state with phantom values. The
//! `LoopBreak` (live) vs. `AdjacentCode` (dead) distinction is
//! load-bearing: `UnreachableCode` and the open-resource lattice
//! both reason correctly off the same edge model without a
//! custom DFS.
//!
//! `Перейти <Label>` resolves to a live `Direct` edge from the
//! source block to the label vertex. Forward references park
//! until the label is visited, then get patched into the same
//! `Direct` edge. Either way, the source block also receives
//! the unconditional-jump's `AdjacentCode` edge to a dead
//! fallthrough block — that's the regular shape for *any*
//! unconditional jump (`Возврат`, `ВызватьИсключение`, `Прервать`,
//! `Продолжить`, `Перейти`). If the label is never visited,
//! the source block carries *only* the `AdjacentCode` edge (no
//! live outgoing) — diagnosing the unresolved label is
//! parser/lowering's responsibility, not CFG's.
//!
//! ## Example
//!
//! ```rust,ignore
//! use cfg::{ControlFlowGraph, CfgVertex, CfgEdgeType, CfgBuilder};
//!
//! // Build CFG from function body
//! let mut builder = CfgBuilder::new();
//! let cfg = builder.build_graph(function_body_node);
//!
//! // Analyze paths
//! let exit_point = cfg.exit_point();
//! for (source_idx, edge_type) in cfg.incoming_edges(exit_point) {
//!     // Check if path has missing return
//! }
//! ```

pub mod builder;
pub mod collection;
pub mod edge;
pub mod graph;
pub mod vertex;

// Re-export main types for convenience
pub use builder::CfgBuilder;
pub use collection::ModuleCfgs;
pub use edge::CfgEdgeType;
pub use graph::ControlFlowGraph;
pub use vertex::{
    BasicBlockVertex, CfgVertex, ConditionalVertex, ForEachLoopVertex, ForLoopVertex, LabelVertex,
    PreprocConditionVertex, TryExceptVertex, WhileLoopVertex,
};

// Re-export petgraph types used in public API
pub use petgraph::graph::NodeIndex;

// Version info
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        // Check version is valid semver format (contains at least one dot)
        assert!(VERSION.contains('.'));
    }
}
