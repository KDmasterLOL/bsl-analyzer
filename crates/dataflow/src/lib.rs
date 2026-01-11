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

pub mod liveness;
pub mod reaching_defs;

use cfg::ControlFlowGraph;
use hir_def::body::Body;
use la_arena::RawIdx;
use petgraph::graph::NodeIndex;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

/// Direction of dataflow analysis.
///
/// ## Forward Analysis
///
/// Information flows from entry to exit.
/// - IN\[B\] = join(OUT\[pred\]) for all predecessors
/// - OUT\[B\] = transfer(IN\[B\], B)
/// - Examples: Reaching definitions, constant propagation
///
/// ## Backward Analysis
///
/// Information flows from exit to entry.
/// - OUT\[B\] = join(IN\[succ\]) for all successors
/// - IN\[B\] = transfer(OUT\[B\], B)
/// - Examples: Liveness analysis, available expressions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Forward dataflow (entry → exit)
    Forward,
    /// Backward dataflow (exit → entry)
    Backward,
}

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

    /// In-place join operation - modifies self to be the join of self and other.
    ///
    /// This is an optimization to avoid unnecessary allocations.
    /// Default implementation falls back to `join()`.
    fn join_in_place(&mut self, other: &Self) {
        *self = self.join(other);
    }

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
/// Defines how statements and expressions transform abstract state.
///
/// ## Forward Analysis
///
/// Given input state IN and a statement, computes output state OUT.
/// - OUT\[stmt\] = transfer(IN\[stmt\], stmt)
/// - IN\[stmt\] = join of OUT\[pred\] for all predecessors
///
/// ## Backward Analysis
///
/// Given output state OUT and a statement, computes input state IN.
/// - IN\[stmt\] = transfer(OUT\[stmt\], stmt)
/// - OUT\[stmt\] = join of IN\[succ\] for all successors
///
/// **Note:** The same `transfer_stmt()` method is used for both directions,
/// but the interpretation differs:
/// - Forward: `state` parameter is IN, return value is OUT
/// - Backward: `state` parameter is OUT, return value is IN
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

    /// Apply transfer function in-place for a single statement.
    ///
    /// Optimized version that modifies state directly instead of returning a new one.
    /// Default implementation falls back to `transfer_stmt()`.
    fn transfer_stmt_in_place(&self, stmt_id: RawIdx, state: &mut L, body: &Body) {
        *state = self.transfer_stmt(stmt_id, state, body);
    }

    /// Apply transfer function for an expression (used in control flow vertices).
    ///
    /// This is called for expressions in control flow vertices like While conditions,
    /// For loop bounds, etc.
    ///
    /// ## Arguments
    ///
    /// - `expr_id`: HIR expression ID
    /// - `state`: Input state
    /// - `body`: HIR body for looking up expression details
    ///
    /// ## Returns
    ///
    /// Output state after processing the expression.
    ///
    /// Default implementation returns state unchanged.
    fn transfer_expr(&self, _expr_id: hir_def::ExprId, state: &L, _body: &Body) -> L {
        state.clone()
    }

    /// Apply transfer function in-place for an expression.
    ///
    /// Optimized version that modifies state directly.
    /// Default implementation falls back to `transfer_expr()`.
    fn transfer_expr_in_place(&self, expr_id: hir_def::ExprId, state: &mut L, body: &Body) {
        *state = self.transfer_expr(expr_id, state, body);
    }
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
    /// Control flow graph (shared via Arc for efficient caching)
    cfg: Arc<ControlFlowGraph>,

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

    /// Direction of dataflow analysis (forward or backward)
    direction: Direction,
}

impl<L: Lattice, T: Transfer<L>> DataflowSolver<L, T> {
    /// Create a new dataflow solver (defaults to forward analysis).
    ///
    /// ## Arguments
    ///
    /// - `cfg`: Control flow graph to analyze (shared via Arc)
    /// - `body`: HIR body for statement lookup
    /// - `transfer`: Transfer function implementation
    ///
    /// ## Default Settings
    ///
    /// - Direction: Forward
    /// - Max iterations: 10000 (configurable via DiagnosticsConfig.dataflow_max_iterations)
    pub fn new(cfg: Arc<ControlFlowGraph>, body: Body, transfer: T) -> Self {
        Self {
            cfg,
            body,
            transfer,
            block_in: FxHashMap::default(),
            block_out: FxHashMap::default(),
            max_iterations: 10000, // Default for complex real-world methods (configurable)
            direction: Direction::Forward, // Default to forward analysis
        }
    }

    /// Set the direction of dataflow analysis.
    ///
    /// ## Arguments
    ///
    /// - `direction`: Direction::Forward or Direction::Backward
    ///
    /// ## Example
    ///
    /// ```ignore
    /// let mut solver = DataflowSolver::new(cfg, body, transfer);
    /// solver.set_direction(Direction::Backward);  // For liveness analysis
    /// let result = solver.solve();
    /// ```
    pub fn set_direction(&mut self, direction: Direction) {
        self.direction = direction;
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

    /// Initialize all blocks with a custom bottom element factory.
    ///
    /// This is useful when `L::bottom()` requires additional context that isn't
    /// available at trait level (e.g., VariableIndex for BitSet-based Liveness).
    ///
    /// ## Example
    ///
    /// ```ignore
    /// let var_index = VariableIndex::from_body(&body);
    /// let mut solver = DataflowSolver::new(cfg, body, transfer);
    /// solver.set_bottom_factory(|| Liveness::new(var_index.clone()));
    /// let result = solver.solve();
    /// ```
    pub fn set_bottom_factory<F>(&mut self, factory: F)
    where
        F: Fn() -> L,
    {
        // Initialize all blocks with custom bottom element
        for (block_idx, _vertex) in self.cfg.vertices() {
            self.block_in.insert(block_idx, factory());
            self.block_out.insert(block_idx, factory());
        }
    }

    /// Run dataflow analysis and return fixed-point solution.
    ///
    /// Uses worklist algorithm to compute IN and OUT sets for each block.
    /// Direction (forward/backward) is determined by the `direction` field.
    ///
    /// ## Returns
    ///
    /// `DataflowResult` containing IN/OUT sets for all blocks, or None if analysis didn't converge.
    pub fn solve(self) -> Option<DataflowResult<L>> {
        match self.direction {
            Direction::Forward => self.solve_forward(),
            Direction::Backward => self.solve_backward(),
        }
    }

    /// Run forward dataflow analysis.
    ///
    /// Computes fixed-point solution using worklist algorithm:
    /// - IN\[B\] = join of OUT\[pred\] for all predecessors
    /// - OUT\[B\] = transfer(IN\[B\], B)
    fn solve_forward(mut self) -> Option<DataflowResult<L>> {
        use std::collections::VecDeque;

        // Initialize all blocks to bottom (only if not already initialized by set_bottom_factory or set_initial_state)
        let entry = self.cfg.entry_point();
        for (block_idx, _vertex) in self.cfg.vertices() {
            // Use entry().or_insert_with() to avoid overwriting existing initialization
            self.block_in.entry(block_idx).or_insert_with(L::bottom);
            self.block_out.entry(block_idx).or_insert_with(L::bottom);
        }

        // Ensure entry block has an IN state (default to bottom if not set)
        if let Some(entry_block) = entry {
            self.block_in.entry(entry_block).or_insert_with(L::bottom);
        }

        // Worklist: blocks in reverse postorder (optimal for forward analysis)
        // RPO minimizes iterations by visiting nodes in topological order
        let rpo = self.cfg.reverse_postorder();
        let mut worklist: VecDeque<NodeIndex> = rpo.into_iter().collect();

        // Tracking set for O(1) contains check (instead of O(n) VecDeque::contains)
        let mut worklist_set: FxHashSet<NodeIndex> = worklist.iter().copied().collect();

        let mut iterations = 0;

        while let Some(block_idx) = worklist.pop_front() {
            worklist_set.remove(&block_idx); // O(1)
            iterations += 1;

            // Safety check: prevent infinite loops
            if iterations > self.max_iterations {
                tracing::warn!(
                    "Dataflow analysis exceeded max iterations ({}), returning partial solution",
                    self.max_iterations
                );
                // Return partial solution instead of None - conservative but usable
                break;
            }

            // Compute IN[block] = join of OUT[pred] for all predecessors
            // Special case: if this is entry block with no predecessors, preserve initial state
            let has_predecessors = self.cfg.incoming_edges(block_idx).next().is_some();
            let is_entry = Some(block_idx) == entry;

            let in_state = if is_entry && !has_predecessors {
                // Entry block with no predecessors: preserve initial state
                self.block_in.get(&block_idx).cloned().expect("block_in should be initialized")
            } else {
                // Normal case: join from predecessors
                // Clone first predecessor's OUT state, then join_in_place with the rest
                let mut state: Option<L> = None;
                for (pred_idx, _edge) in self.cfg.incoming_edges(block_idx) {
                    if let Some(pred_out) = self.block_out.get(&pred_idx) {
                        match &mut state {
                            None => {
                                // First predecessor: clone its OUT state as starting point
                                state = Some(pred_out.clone());
                            }
                            Some(s) => {
                                // Subsequent predecessors: join in-place (no clone!)
                                s.join_in_place(pred_out);
                            }
                        }
                    }
                }
                // If no predecessors had OUT state, use current block_in
                state.unwrap_or_else(|| {
                    self.block_in.get(&block_idx).cloned().expect("block_in should be initialized")
                })
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

                // Add successors to worklist (O(1) check via worklist_set)
                for (succ_idx, _edge) in self.cfg.outgoing_edges(block_idx) {
                    if worklist_set.insert(succ_idx) {
                        // Returns true if newly inserted
                        worklist.push_back(succ_idx);
                    }
                }
            }
        }

        tracing::debug!("Forward dataflow analysis converged in {} iterations", iterations);

        Some(DataflowResult {
            block_in: self.block_in,
            block_out: self.block_out,
            cfg: self.cfg,
            body: self.body,
        })
    }

    /// Run backward dataflow analysis.
    ///
    /// Computes fixed-point solution using worklist algorithm (backward):
    /// - OUT\[B\] = join of IN\[succ\] for all successors
    /// - IN\[B\] = transfer(OUT\[B\], B)
    ///
    /// Starts from exit point and propagates backwards to entry.
    fn solve_backward(mut self) -> Option<DataflowResult<L>> {
        use std::collections::VecDeque;

        // PROFILING: track solver calls and timing

        // Initialize all blocks to bottom (only if not already initialized by set_bottom_factory)
        let exit = self.cfg.exit_point();
        let num_blocks = self.cfg.vertices().count();

        for (block_idx, _vertex) in self.cfg.vertices() {
            // Use entry().or_insert_with() to avoid overwriting existing initialization
            self.block_in.entry(block_idx).or_insert_with(L::bottom);
            self.block_out.entry(block_idx).or_insert_with(L::bottom);
        }

        // Ensure exit block has an OUT state (default to bottom if not set)
        self.block_out.entry(exit).or_insert_with(L::bottom);

        // Worklist: blocks in postorder from exit (optimal for backward analysis)
        // Postorder minimizes iterations by visiting nodes in dependency order
        let postorder = self.cfg.postorder_from_exit();
        let mut worklist: VecDeque<NodeIndex> = postorder.into_iter().collect();

        // Tracking set for O(1) contains check (instead of O(n) VecDeque::contains)
        let mut worklist_set: FxHashSet<NodeIndex> = worklist.iter().copied().collect();

        let mut iterations = 0;
        let mut block_visit_count: rustc_hash::FxHashMap<NodeIndex, usize> =
            rustc_hash::FxHashMap::default();

        while let Some(block_idx) = worklist.pop_front() {
            worklist_set.remove(&block_idx); // O(1)
            iterations += 1;
            *block_visit_count.entry(block_idx).or_insert(0) += 1;

            // Safety check: prevent infinite loops
            if iterations > self.max_iterations {
                let max_visits = block_visit_count.values().max().copied().unwrap_or(0);
                let avg_visits = iterations as f64 / num_blocks as f64;

                tracing::warn!(
                    "Backward dataflow analysis exceeded max iterations: {} iterations, {} blocks, max visits per block: {}, avg visits: {:.1}",
                    iterations,
                    num_blocks,
                    max_visits,
                    avg_visits
                );

                // Log most frequently visited blocks
                let mut frequent_blocks: Vec<_> = block_visit_count.iter().collect();
                frequent_blocks.sort_by_key(|(_, &count)| std::cmp::Reverse(count));

                if !frequent_blocks.is_empty() {
                    tracing::debug!("Top 5 most visited blocks:");
                    for (idx, count) in frequent_blocks.iter().take(5) {
                        tracing::debug!("  Block {:?}: {} visits", idx, count);
                    }
                }

                // PROFILING: count exceeded max iterations

                // Return partial solution instead of None - conservative but usable
                break;
            }

            // Compute OUT[block] = join of IN[succ] for all successors
            // Special case: if this is exit block with no successors, preserve initial state
            let has_successors = self.cfg.outgoing_edges(block_idx).next().is_some();
            let is_exit = block_idx == exit;

            let out_state = if is_exit && !has_successors {
                // Exit block with no successors: preserve initial state (usually bottom)
                self.block_out.get(&block_idx).cloned().expect("block_out should be initialized")
            } else {
                // Normal case: join from successors
                // Clone first successor's IN state, then join_in_place with the rest
                let mut state: Option<L> = None;
                for (succ_idx, _edge) in self.cfg.outgoing_edges(block_idx) {
                    if let Some(succ_in) = self.block_in.get(&succ_idx) {
                        match &mut state {
                            None => {
                                // First successor: clone its IN state as starting point
                                state = Some(succ_in.clone());
                            }
                            Some(s) => {
                                // Subsequent successors: join in-place (no clone!)
                                s.join_in_place(succ_in);
                            }
                        }
                    }
                }
                // If no successors had IN state, use current block_out
                state.unwrap_or_else(|| {
                    self.block_out
                        .get(&block_idx)
                        .cloned()
                        .expect("block_out should be initialized")
                })
            };

            // Update OUT[block]
            self.block_out.insert(block_idx, out_state.clone());

            // Compute IN[block] = transfer(OUT[block], block)
            // Note: for backward analysis, transfer expects OUT and returns IN
            let in_state = self.transfer_block(block_idx, &out_state);

            // Check if IN[block] changed
            let changed = if let Some(old_in) = self.block_in.get(&block_idx) {
                &in_state != old_in
            } else {
                true
            };

            if changed {
                // Update IN[block]
                self.block_in.insert(block_idx, in_state);

                // Add predecessors to worklist (backward propagation, O(1) check via worklist_set)
                for (pred_idx, _edge) in self.cfg.incoming_edges(block_idx) {
                    if worklist_set.insert(pred_idx) {
                        // Returns true if newly inserted
                        worklist.push_back(pred_idx);
                    }
                }
            }
        }

        // Log convergence statistics
        let max_visits = block_visit_count.values().max().copied().unwrap_or(0);
        let avg_visits = if num_blocks > 0 { iterations as f64 / num_blocks as f64 } else { 0.0 };

        if iterations > 100 {
            tracing::info!(
                "Backward dataflow analysis converged: {} iterations, {} blocks, max visits: {}, avg visits: {:.1}",
                iterations,
                num_blocks,
                max_visits,
                avg_visits
            );
        } else {
            tracing::debug!(
                "Backward dataflow analysis converged: {} iterations, {} blocks",
                iterations,
                num_blocks
            );
        }

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
    /// For special vertices (While, For, ForEach, If), processes the condition/control expression.
    fn transfer_block(&self, block_idx: NodeIndex, in_state: &L) -> L {
        use cfg::CfgVertex;

        let Some(vertex) = self.cfg.vertex(block_idx) else {
            return in_state.clone();
        };

        match vertex {
            CfgVertex::BasicBlock(block) => {
                // Apply transfer function to each statement in the block
                // Clone once, then modify in-place for all statements
                let mut state = in_state.clone();
                for &stmt_id in block.statements() {
                    // Use in-place transfer to avoid cloning for each statement
                    self.transfer.transfer_stmt_in_place(
                        stmt_id.into_raw(),
                        &mut state,
                        &self.body,
                    );
                }
                state
            }

            CfgVertex::WhileLoop(while_vertex) => {
                // While loop: process the condition expression
                // Clone once, then modify in-place
                let mut state = in_state.clone();
                self.transfer.transfer_expr_in_place(
                    while_vertex.condition,
                    &mut state,
                    &self.body,
                );
                state
            }

            CfgVertex::Conditional(conditional_vertex) => {
                // If statement: process the condition expression
                // Clone once, then modify in-place
                let mut state = in_state.clone();
                self.transfer.transfer_expr_in_place(
                    conditional_vertex.condition,
                    &mut state,
                    &self.body,
                );
                state
            }

            CfgVertex::ForLoop(_) => {
                // ForLoop: from/to expressions are processed via transfer_stmt
                // when the For statement itself is in a BasicBlock
                in_state.clone()
            }

            CfgVertex::ForEachLoop(foreach_vertex) => {
                // ForEach loop: process the collection expression
                // Clone once, then modify in-place
                let mut state = in_state.clone();
                self.transfer.transfer_expr_in_place(
                    foreach_vertex.collection,
                    &mut state,
                    &self.body,
                );
                state
            }

            // Other vertex types (Entry, Exit, etc.) don't have expressions to process
            _ => in_state.clone(),
        }
    }
}

/// Result of dataflow analysis.
///
/// Contains IN and OUT sets for each block, plus references to CFG and Body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataflowResult<L: Lattice> {
    /// IN sets for each block
    block_in: FxHashMap<NodeIndex, L>,

    /// OUT sets for each block
    block_out: FxHashMap<NodeIndex, L>,

    /// Control flow graph (for mapping statements to blocks, shared via Arc)
    cfg: Arc<ControlFlowGraph>,

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
