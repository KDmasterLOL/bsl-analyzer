//! # BSL Control Flow Graph (CFG)
//!
//! Control Flow Graph construction and analysis for BSL Language Server.
//!
//! Ported from BSL Language Server (Java) CFG package via bsl-language-server-rust:
//! - `com.github._1c_syntax.bsl.languageserver.cfg`
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
    TryExceptVertex, WhileLoopVertex,
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
