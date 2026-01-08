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

use std::sync::atomic::{AtomicU64, Ordering};

pub mod builder;
pub mod collection;
pub mod edge;
pub mod graph;
pub mod vertex;

// ============================================================================
// CFG Builder Profiling Counters
// ============================================================================

/// Number of times CFG was built
pub static CFG_BUILD_CALLS: AtomicU64 = AtomicU64::new(0);

/// Total time spent building CFGs (nanoseconds)
pub static CFG_BUILD_TIME_NS: AtomicU64 = AtomicU64::new(0);

/// Number of walk_statement_hir calls
pub static CFG_WALK_STMT_CALLS: AtomicU64 = AtomicU64::new(0);

/// Number of add_vertex calls
pub static CFG_ADD_VERTEX_CALLS: AtomicU64 = AtomicU64::new(0);

/// Number of add_edge calls
pub static CFG_ADD_EDGE_CALLS: AtomicU64 = AtomicU64::new(0);

/// Statement type counters
pub static CFG_WALK_IF_CALLS: AtomicU64 = AtomicU64::new(0);
pub static CFG_WALK_WHILE_CALLS: AtomicU64 = AtomicU64::new(0);
pub static CFG_WALK_FOR_CALLS: AtomicU64 = AtomicU64::new(0);
pub static CFG_WALK_FOREACH_CALLS: AtomicU64 = AtomicU64::new(0);
pub static CFG_WALK_TRY_CALLS: AtomicU64 = AtomicU64::new(0);
pub static CFG_WALK_RETURN_CALLS: AtomicU64 = AtomicU64::new(0);
pub static CFG_WALK_RAISE_CALLS: AtomicU64 = AtomicU64::new(0);
pub static CFG_WALK_BREAK_CALLS: AtomicU64 = AtomicU64::new(0);
pub static CFG_WALK_CONTINUE_CALLS: AtomicU64 = AtomicU64::new(0);
pub static CFG_WALK_GOTO_CALLS: AtomicU64 = AtomicU64::new(0);
pub static CFG_WALK_LABEL_CALLS: AtomicU64 = AtomicU64::new(0);
pub static CFG_WALK_OTHER_CALLS: AtomicU64 = AtomicU64::new(0);

/// Reset all CFG profiling counters
pub fn reset_cfg_counters() {
    CFG_BUILD_CALLS.store(0, Ordering::Relaxed);
    CFG_BUILD_TIME_NS.store(0, Ordering::Relaxed);
    CFG_WALK_STMT_CALLS.store(0, Ordering::Relaxed);
    CFG_ADD_VERTEX_CALLS.store(0, Ordering::Relaxed);
    CFG_ADD_EDGE_CALLS.store(0, Ordering::Relaxed);
    CFG_WALK_IF_CALLS.store(0, Ordering::Relaxed);
    CFG_WALK_WHILE_CALLS.store(0, Ordering::Relaxed);
    CFG_WALK_FOR_CALLS.store(0, Ordering::Relaxed);
    CFG_WALK_FOREACH_CALLS.store(0, Ordering::Relaxed);
    CFG_WALK_TRY_CALLS.store(0, Ordering::Relaxed);
    CFG_WALK_RETURN_CALLS.store(0, Ordering::Relaxed);
    CFG_WALK_RAISE_CALLS.store(0, Ordering::Relaxed);
    CFG_WALK_BREAK_CALLS.store(0, Ordering::Relaxed);
    CFG_WALK_CONTINUE_CALLS.store(0, Ordering::Relaxed);
    CFG_WALK_GOTO_CALLS.store(0, Ordering::Relaxed);
    CFG_WALK_LABEL_CALLS.store(0, Ordering::Relaxed);
    CFG_WALK_OTHER_CALLS.store(0, Ordering::Relaxed);
}

/// Print CFG profiling counters
pub fn print_cfg_counters() {
    let build_calls = CFG_BUILD_CALLS.load(Ordering::Relaxed);
    let build_time_ms = CFG_BUILD_TIME_NS.load(Ordering::Relaxed) / 1_000_000;
    let walk_stmt_calls = CFG_WALK_STMT_CALLS.load(Ordering::Relaxed);
    let add_vertex_calls = CFG_ADD_VERTEX_CALLS.load(Ordering::Relaxed);
    let add_edge_calls = CFG_ADD_EDGE_CALLS.load(Ordering::Relaxed);

    eprintln!("\n=== CFG Builder Profiling ===");
    eprintln!("build_calls:          {:>12}", build_calls);
    eprintln!("build_time_ms:        {:>12}", build_time_ms);
    if build_calls > 0 {
        eprintln!("avg_time_per_method:  {:>12.2} ms", build_time_ms as f64 / build_calls as f64);
    }
    eprintln!("walk_stmt_calls:      {:>12}", walk_stmt_calls);
    eprintln!("add_vertex_calls:     {:>12}", add_vertex_calls);
    eprintln!("add_edge_calls:       {:>12}", add_edge_calls);

    eprintln!("\n--- Statement Type Breakdown ---");
    let if_calls = CFG_WALK_IF_CALLS.load(Ordering::Relaxed);
    let while_calls = CFG_WALK_WHILE_CALLS.load(Ordering::Relaxed);
    let for_calls = CFG_WALK_FOR_CALLS.load(Ordering::Relaxed);
    let foreach_calls = CFG_WALK_FOREACH_CALLS.load(Ordering::Relaxed);
    let try_calls = CFG_WALK_TRY_CALLS.load(Ordering::Relaxed);
    let return_calls = CFG_WALK_RETURN_CALLS.load(Ordering::Relaxed);
    let raise_calls = CFG_WALK_RAISE_CALLS.load(Ordering::Relaxed);
    let break_calls = CFG_WALK_BREAK_CALLS.load(Ordering::Relaxed);
    let continue_calls = CFG_WALK_CONTINUE_CALLS.load(Ordering::Relaxed);
    let goto_calls = CFG_WALK_GOTO_CALLS.load(Ordering::Relaxed);
    let label_calls = CFG_WALK_LABEL_CALLS.load(Ordering::Relaxed);
    let other_calls = CFG_WALK_OTHER_CALLS.load(Ordering::Relaxed);

    eprintln!("if_statements:        {:>12}", if_calls);
    eprintln!("while_loops:          {:>12}", while_calls);
    eprintln!("for_loops:            {:>12}", for_calls);
    eprintln!("foreach_loops:        {:>12}", foreach_calls);
    eprintln!("try_statements:       {:>12}", try_calls);
    eprintln!("return_statements:    {:>12}", return_calls);
    eprintln!("raise_statements:     {:>12}", raise_calls);
    eprintln!("break_statements:     {:>12}", break_calls);
    eprintln!("continue_statements:  {:>12}", continue_calls);
    eprintln!("goto_statements:      {:>12}", goto_calls);
    eprintln!("label_statements:     {:>12}", label_calls);
    eprintln!("other_statements:     {:>12}", other_calls);
}

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
