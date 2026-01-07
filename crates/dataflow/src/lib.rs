//! Generic dataflow analysis framework for BSL code.
//!
//! This crate provides a reusable infrastructure for implementing dataflow analyses
//! such as reaching definitions, constant propagation, live variable analysis, etc.
//!
//! ## Architecture
//!
//! - **Lattice**: Abstract domain for dataflow values (e.g., sets, constants, etc.)
//! - **Transfer**: Transfer functions that compute OUT from IN for each statement
//! - **DataflowSolver**: Worklist algorithm that computes fixed-point solution
//!
//! ## Example
//!
//! ```rust,ignore
//! use dataflow::{Lattice, Transfer, DataflowSolver};
//!
//! // 1. Define your lattice
//! #[derive(Clone, PartialEq, Eq)]
//! struct MyLattice { /* ... */ }
//!
//! impl Lattice for MyLattice {
//!     fn bottom() -> Self { /* ... */ }
//!     fn join(&self, other: &Self) -> Self { /* ... */ }
//! }
//!
//! // 2. Define transfer function
//! struct MyTransfer;
//!
//! impl Transfer<MyLattice> for MyTransfer {
//!     fn transfer_stmt(&self, stmt_id: RawIdx, state: &MyLattice, body: &Body) -> MyLattice {
//!         // Compute how stmt transforms state
//!     }
//! }
//!
//! // 3. Run analysis
//! let solver = DataflowSolver::new(cfg, body, MyTransfer);
//! let result = solver.solve();
//! ```

pub mod reaching_defs;

use cfg::ControlFlowGraph;
use hir_def::body::Body;
use la_arena::RawIdx;
use petgraph::graph::NodeIndex;
use rustc_hash::FxHashMap;

/// Lattice trait for abstract interpretation.
///
/// A lattice defines the abstract domain for dataflow analysis.
/// It must form a partial order with join (least upper bound) operation.
///
/// ## Laws
///
/// 1. **Idempotence**: `a.join(a) == a`
/// 2. **Commutativity**: `a.join(b) == b.join(a)`
/// 3. **Associativity**: `a.join(b.join(c)) == (a.join(b)).join(c)`
/// 4. **Bottom**: `bottom().join(a) == a`
pub trait Lattice: Clone + PartialEq + Eq {
    /// The bottom element (⊥) - least informative value.
    ///
    /// Bottom represents "no information" or the start of analysis.
    fn bottom() -> Self;

    /// Join operation (⊔) - computes least upper bound.
    ///
    /// Merges information from two lattice values.
    /// Used at control flow merge points (e.g., after if/else).
    fn join(&self, other: &Self) -> Self;

    /// Check if self is more informative than other.
    ///
    /// Returns true if `self ⊑ other` (self is below or equal to other in lattice order).
    /// Used to detect fixed-point convergence.
    fn is_more_informative_than(&self, other: &Self) -> bool {
        self == other
    }
}

/// Transfer function trait.
///
/// Defines how statements transform abstract state.
/// Given input state IN and a statement, computes output state OUT.
///
/// ## Forward Analysis
///
/// - OUT\[stmt\] = transfer(IN\[stmt\], stmt)
/// - IN\[stmt\] = join of OUT\[pred\] for all predecessors
///
/// ## Backward Analysis
///
/// (Not yet supported, but can be added by reversing CFG traversal)
pub trait Transfer<L: Lattice> {
    /// Apply transfer function for a single statement.
    ///
    /// Given the input state and statement ID, computes the output state.
    ///
    /// ## Arguments
    ///
    /// - `stmt_id`: HIR statement index (RawIdx from la-arena)
    /// - `state`: Input state (IN set)
    /// - `body`: HIR body for looking up statement details
    ///
    /// ## Returns
    ///
    /// Output state (OUT set) after executing the statement.
    fn transfer_stmt(&self, stmt_id: RawIdx, state: &L, body: &Body) -> L;
}

/// Dataflow solver using worklist algorithm.
///
/// Implements Kildall's algorithm for computing fixed-point solution.
///
/// ## Algorithm
///
/// 1. Initialize all blocks to bottom (⊥)
/// 2. Add all blocks to worklist
/// 3. While worklist is not empty:
///    a. Remove a block from worklist
///    b. Compute IN\[block\] = join of OUT\[pred\] for all predecessors
///    c. Compute OUT\[block\] = transfer(IN\[block\], block)
///    d. If OUT\[block\] changed, add successors to worklist
/// 4. Return fixed-point solution (IN and OUT for all blocks)
pub struct DataflowSolver<L: Lattice, T: Transfer<L>> {
    /// Control flow graph
    cfg: ControlFlowGraph,

    /// HIR body (for statement lookup in transfer functions)
    body: Body,

    /// Transfer function
    transfer: T,

    /// IN sets for each block
    block_in: FxHashMap<NodeIndex, L>,

    /// OUT sets for each block
    block_out: FxHashMap<NodeIndex, L>,

    /// Maximum iterations before giving up (prevents infinite loops)
    max_iterations: usize,
}

impl<L: Lattice, T: Transfer<L>> DataflowSolver<L, T> {
    /// Create a new dataflow solver.
    ///
    /// ## Arguments
    ///
    /// - `cfg`: Control flow graph to analyze
    /// - `body`: HIR body for statement lookup
    /// - `transfer`: Transfer function implementation
    pub fn new(cfg: ControlFlowGraph, body: Body, transfer: T) -> Self {
        Self {
            cfg,
            body,
            transfer,
            block_in: FxHashMap::default(),
            block_out: FxHashMap::default(),
            max_iterations: 100, // Reasonable default for BSL methods
        }
    }

    /// Set maximum iterations (default: 100).
    ///
    /// If analysis doesn't converge within this limit, it will stop and return partial results.
    /// This prevents infinite loops for malformed CFGs.
    pub fn set_max_iterations(&mut self, max_iterations: usize) {
        self.max_iterations = max_iterations;
    }

    /// Set initial state for the entry block.
    ///
    /// This is useful for setting up initial facts before analysis,
    /// such as parameter definitions for reaching definitions.
    ///
    /// ## Example
    ///
    /// ```ignore
    /// let mut solver = DataflowSolver::new(cfg, body, transfer);
    /// solver.set_initial_state(initial_defs); // Set parameters
    /// let result = solver.solve();
    /// ```
    pub fn set_initial_state(&mut self, initial: L) {
        if let Some(entry) = self.cfg.entry_point() {
            self.block_in.insert(entry, initial);
        }
    }

    /// Run dataflow analysis and return fixed-point solution.
    ///
    /// Uses worklist algorithm to compute IN and OUT sets for each block.
    ///
    /// ## Returns
    ///
    /// `DataflowResult` containing IN/OUT sets for all blocks, or None if analysis didn't converge.
    pub fn solve(mut self) -> Option<DataflowResult<L>> {
        use std::collections::VecDeque;

        // Initialize all blocks to bottom (skip entry if already set via set_initial_state)
        let entry = self.cfg.entry_point();
        for (block_idx, _vertex) in self.cfg.vertices() {
            // Don't overwrite entry block if it has initial state
            if Some(block_idx) != entry || !self.block_in.contains_key(&block_idx) {
                self.block_in.insert(block_idx, L::bottom());
            }
            self.block_out.insert(block_idx, L::bottom());
        }

        // If entry block wasn't set via set_initial_state, initialize to bottom
        if let Some(entry_block) = entry {
            self.block_in.entry(entry_block).or_insert(L::bottom());
        }

        // Worklist: blocks that need reprocessing
        let mut worklist: VecDeque<NodeIndex> = self.cfg.vertices().map(|(idx, _)| idx).collect();

        let mut iterations = 0;

        while let Some(block_idx) = worklist.pop_front() {
            iterations += 1;

            // Safety check: prevent infinite loops
            if iterations > self.max_iterations {
                tracing::warn!(
                    "Dataflow analysis exceeded max iterations ({}), stopping",
                    self.max_iterations
                );
                return None;
            }

            // Compute IN[block] = join of OUT[pred] for all predecessors
            // Special case: if this is entry block with no predecessors, preserve initial state
            let has_predecessors = self.cfg.incoming_edges(block_idx).next().is_some();
            let is_entry = Some(block_idx) == entry;

            let in_state = if is_entry && !has_predecessors {
                // Entry block with no predecessors: preserve initial state
                self.block_in.get(&block_idx).cloned().unwrap_or_else(L::bottom)
            } else {
                // Normal case: join from predecessors
                let mut state = L::bottom();
                for (pred_idx, _edge) in self.cfg.incoming_edges(block_idx) {
                    if let Some(pred_out) = self.block_out.get(&pred_idx) {
                        state = state.join(pred_out);
                    }
                }
                state
            };

            // Update IN[block]
            self.block_in.insert(block_idx, in_state.clone());

            // Compute OUT[block] = transfer(IN[block], block)
            let out_state = self.transfer_block(block_idx, &in_state);

            // Check if OUT[block] changed
            let changed = if let Some(old_out) = self.block_out.get(&block_idx) {
                &out_state != old_out
            } else {
                true
            };

            if changed {
                // Update OUT[block]
                self.block_out.insert(block_idx, out_state);

                // Add successors to worklist
                for (succ_idx, _edge) in self.cfg.outgoing_edges(block_idx) {
                    if !worklist.contains(&succ_idx) {
                        worklist.push_back(succ_idx);
                    }
                }
            }
        }

        tracing::debug!("Dataflow analysis converged in {} iterations", iterations);

        Some(DataflowResult {
            block_in: self.block_in,
            block_out: self.block_out,
            cfg: self.cfg,
            body: self.body,
        })
    }

    /// Apply transfer function to a basic block.
    ///
    /// Walks through all statements in the block and applies transfer function sequentially.
    fn transfer_block(&self, block_idx: NodeIndex, in_state: &L) -> L {
        use cfg::CfgVertex;

        let Some(vertex) = self.cfg.vertex(block_idx) else {
            return in_state.clone();
        };

        match vertex {
            CfgVertex::BasicBlock(block) => {
                // Apply transfer function to each statement in the block
                // Phase 6.3: Re-enabled with HIR-based CFG (statements() returns &[StmtId])
                let mut state = in_state.clone();
                for &stmt_id in block.statements() {
                    state = self.transfer.transfer_stmt(stmt_id.into_raw(), &state, &self.body);
                }
                state
            }
            // Other vertex types (Conditional, Loop, etc.) don't contain statements
            // They just represent control flow structure
            _ => in_state.clone(),
        }
    }
}

/// Result of dataflow analysis.
///
/// Contains IN and OUT sets for each block, plus references to CFG and Body.
#[derive(Debug, Clone)]
pub struct DataflowResult<L: Lattice> {
    /// IN sets for each block
    block_in: FxHashMap<NodeIndex, L>,

    /// OUT sets for each block
    block_out: FxHashMap<NodeIndex, L>,

    /// Control flow graph (for mapping statements to blocks)
    cfg: ControlFlowGraph,

    /// HIR body (for statement lookup)
    body: Body,
}

impl<L: Lattice> DataflowResult<L> {
    /// Get IN state for a block.
    pub fn block_in(&self, block: NodeIndex) -> Option<&L> {
        self.block_in.get(&block)
    }

    /// Get OUT state for a block.
    pub fn block_out(&self, block: NodeIndex) -> Option<&L> {
        self.block_out.get(&block)
    }

    /// Get the control flow graph.
    pub fn cfg(&self) -> &ControlFlowGraph {
        &self.cfg
    }

    /// Get the HIR body.
    pub fn body(&self) -> &Body {
        &self.body
    }

    /// Iterate over all blocks with their IN and OUT states.
    pub fn blocks(&self) -> impl Iterator<Item = (NodeIndex, &L, &L)> {
        self.block_in.iter().filter_map(move |(idx, in_state)| {
            self.block_out.get(idx).map(|out_state| (*idx, in_state, out_state))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Example lattice: Set of integers (for testing)
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct IntSetLattice {
        values: Vec<i32>,
    }

    impl Lattice for IntSetLattice {
        fn bottom() -> Self {
            Self { values: vec![] }
        }

        fn join(&self, other: &Self) -> Self {
            let mut values = self.values.clone();
            for &v in &other.values {
                if !values.contains(&v) {
                    values.push(v);
                }
            }
            values.sort_unstable();
            Self { values }
        }
    }

    #[test]
    fn test_lattice_bottom() {
        let bottom = IntSetLattice::bottom();
        assert!(bottom.values.is_empty());
    }

    #[test]
    fn test_lattice_join() {
        let a = IntSetLattice { values: vec![1, 2, 3] };
        let b = IntSetLattice { values: vec![2, 3, 4] };
        let joined = a.join(&b);
        assert_eq!(joined.values, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_lattice_join_idempotent() {
        let a = IntSetLattice { values: vec![1, 2, 3] };
        let joined = a.join(&a);
        assert_eq!(joined, a);
    }

    #[test]
    fn test_lattice_join_commutative() {
        let a = IntSetLattice { values: vec![1, 2] };
        let b = IntSetLattice { values: vec![3, 4] };
        assert_eq!(a.join(&b), b.join(&a));
    }

    #[test]
    fn test_lattice_bottom_identity() {
        let a = IntSetLattice { values: vec![1, 2, 3] };
        let bottom = IntSetLattice::bottom();
        assert_eq!(bottom.join(&a), a);
        assert_eq!(a.join(&bottom), a);
    }
}
