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
//! let mut solver = DataflowSolver::new(cfg, body, MyTransfer);
//! solver.set_bottom_factory(|| MyLattice { /* ... */ });
//! let result = solver.solve();
//! ```

pub mod liveness;
pub mod path_terminates;
pub mod reaching_defs;

use cfg::{CfgEdgeType, ControlFlowGraph};
use hir_def::body::Body;
use la_arena::RawIdx;
use petgraph::graph::NodeIndex;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

/// Default maximum iterations for dataflow analysis.
///
/// This is the single source of truth for the default value.
/// Can be overridden via `DiagnosticsConfig.dataflow_max_iterations`.
///
/// Lower values = faster but may skip complex methods.
/// Higher values = more accurate but slower on pathological cases.
pub const DEFAULT_MAX_ITERATIONS: usize = 10000;

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
/// The bottom element (⊥) is provided via `DataflowSolver::set_bottom_factory()`,
/// which must be called before `solve()`. This allows lattices that require
/// runtime context (e.g., `Arc<DefinitionIndex>`) to construct their bottom.
///
/// ## Laws
///
/// 1. **Idempotence**: `a.join(a) == a`
/// 2. **Commutativity**: `a.join(b) == b.join(a)`
/// 3. **Associativity**: `a.join(b.join(c)) == (a.join(b)).join(c)`
/// 4. **Bottom**: `bottom.join(a) == a` (where bottom is from factory)
pub trait Lattice: Clone + PartialEq + Eq {
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

    /// Apply an edge-sensitive refinement to a lattice value crossing an edge.
    ///
    /// Called by the solver at every join point — once per predecessor edge in
    /// forward analyses and once per successor edge in backward analyses —
    /// before the value is joined into the accumulating IN/OUT state. The
    /// returned lattice value represents the information that flows *through*
    /// the given edge; e.g. narrowing analyses use this hook to restrict the
    /// state on `TrueBranch` / `FalseBranch` successors of a guard.
    ///
    /// ## Arguments
    ///
    /// - `edge_kind`: the kind of CFG edge being traversed
    ///   ([`CfgEdgeType::TrueBranch`] / [`CfgEdgeType::FalseBranch`] for
    ///   conditional successors, [`CfgEdgeType::Direct`] for sequential
    ///   fall-through, etc.)
    /// - `state`: the upstream lattice value (forward: `OUT[pred]`, backward:
    ///   `IN[succ]`)
    ///
    /// ## Returns
    ///
    /// The refined state after traversing the edge. Default implementation
    /// returns a clone of `state` — edge-blind analyses such as reaching
    /// definitions and liveness do not need to override this.
    ///
    /// ## Contract for edge-sensitive analyses
    ///
    /// `transfer_edge` receives the lattice value but **not** a direct
    /// handle to the source block or the guard expression that produced
    /// the branch. That is by design: the solver already runs
    /// [`Transfer::transfer_expr_in_place`] on the conditional vertex's
    /// condition *before* visiting outgoing edges (see
    /// `DataflowSolver::transfer_block` for `CfgVertex::Conditional` /
    /// `WhileLoop`). An edge-sensitive analysis must therefore encode
    /// whatever guard facts it needs into `L` during `transfer_expr`, so
    /// that `transfer_edge` can consume them here. For Task 6's narrowing
    /// analysis this means the `Name → Ty` lattice also carries a
    /// "pending guard" slot that `transfer_expr` fills when it sees
    /// `ТипЗнч(x) = Тип("…")` and `transfer_edge` consumes (and clears)
    /// on the `TrueBranch` / `FalseBranch` edge.
    ///
    /// Keeping the API narrow (just `edge_kind` + `&L`) keeps the
    /// solver's hot path free of per-edge `body`/`cfg` lookups and
    /// matches the shape `transfer_stmt` / `transfer_expr` already use.
    fn transfer_edge(&self, _edge_kind: CfgEdgeType, state: &L) -> L {
        state.clone()
    }
}

/// Dataflow solver using worklist algorithm.
///
/// Implements Kildall's algorithm for computing fixed-point solution.
///
/// ## Algorithm
///
/// 1. Initialize all blocks via `set_bottom_factory()` (must be called before `solve()`)
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
    /// - Max iterations: DEFAULT_MAX_ITERATIONS (configurable via DiagnosticsConfig.dataflow_max_iterations)
    pub fn new(cfg: Arc<ControlFlowGraph>, body: Body, transfer: T) -> Self {
        Self {
            cfg,
            body,
            transfer,
            block_in: FxHashMap::default(),
            block_out: FxHashMap::default(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
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

    /// Seed the exit block's OUT state.
    ///
    /// Symmetric counterpart of [`set_initial_state`] for backward analyses.
    /// The backward solver iterates from the exit point: when the exit has
    /// no successors it preserves whatever lives in `block_out[exit]`
    /// (see `solve_backward`'s `is_exit && !has_successors` branch). The
    /// default initialisation from [`set_bottom_factory`] makes this `⊥`,
    /// which is correct for analyses whose boundary fact is bottom
    /// (liveness — nothing is live after a function returns) but wrong for
    /// analyses whose boundary fact is non-bottom.
    ///
    /// Example: PathTerminates seeds `OUT[exit] = MayFallthrough(true)` so
    /// "execution can reach the end without `Return`" propagates backwards
    /// through every block until a `Return` / `Raise` transfer kills it.
    /// Without this seed every block sees `false`, the join is a fixed
    /// point, and the analysis returns the trivial answer.
    ///
    /// Call **after** [`set_bottom_factory`] (the factory wipes
    /// `block_out` for every vertex including the exit).
    ///
    /// For forward analyses this is effectively a no-op on the result —
    /// the forward solver consults `block_out[exit]` only as initial state
    /// when computing OUT for the exit, which has no out-edges, so seeding
    /// it does not influence any other block.
    ///
    /// Panics in debug builds if called before
    /// [`set_bottom_factory`] (or any other initialiser that populates
    /// `block_out` for every vertex). Without that prior call the
    /// solver's pre-loop assertion would fire later anyway, but the
    /// failure mode there is opaque ("All blocks must be initialized");
    /// the assert here makes the ordering bug immediately legible.
    pub fn set_initial_state_at_exit(&mut self, initial: L) {
        let exit = self.cfg.exit_point();
        debug_assert!(
            self.cfg
                .vertices()
                .all(|(idx, _)| self.block_in.contains_key(&idx)
                    && self.block_out.contains_key(&idx)),
            "set_initial_state_at_exit must be called *after* set_bottom_factory \
             (or another initialiser that seeds block_in/block_out for every vertex) — \
             otherwise the factory called later will overwrite the seeded exit OUT state",
        );
        self.block_out.insert(exit, initial);
    }

    /// Initialize all blocks with a custom bottom element factory.
    ///
    /// **Must be called before `solve()`**. The factory provides the bottom element (⊥)
    /// for the lattice, which may require runtime context (e.g., `Arc<DefinitionIndex>`
    /// for `ReachingDefs`, `Arc<VariableIndex>` for `Liveness`).
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

        // All blocks must be initialized via set_bottom_factory() before solve()
        let entry = self.cfg.entry_point();
        assert!(
            self.cfg
                .vertices()
                .all(|(idx, _)| self.block_in.contains_key(&idx)
                    && self.block_out.contains_key(&idx)),
            "All blocks must be initialized before solve(). Call set_bottom_factory() first."
        );

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
                // Normal case: join from predecessors.
                //
                // Each predecessor's OUT state is passed through the
                // transfer's edge-sensitive hook before joining so branch-aware
                // analyses (narrowing) can refine state on TrueBranch /
                // FalseBranch edges. For edge-blind analyses the default
                // `transfer_edge` impl is identity (state.clone()).
                let mut state: Option<L> = None;
                for (pred_idx, edge_kind) in self.cfg.incoming_edges(block_idx) {
                    if let Some(pred_out) = self.block_out.get(&pred_idx) {
                        let edge_state = self.transfer.transfer_edge(*edge_kind, pred_out);
                        match &mut state {
                            None => {
                                // First predecessor: move the edge-refined
                                // state in as the starting point.
                                state = Some(edge_state);
                            }
                            Some(s) => {
                                // Subsequent predecessors: join in-place.
                                s.join_in_place(&edge_state);
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

        // All blocks must be initialized via set_bottom_factory() before solve()
        let exit = self.cfg.exit_point();
        let num_blocks = self.cfg.vertices().count();

        assert!(
            self.cfg
                .vertices()
                .all(|(idx, _)| self.block_in.contains_key(&idx)
                    && self.block_out.contains_key(&idx)),
            "All blocks must be initialized before solve(). Call set_bottom_factory() first."
        );

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

                tracing::debug!(
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
                // Normal case: join from successors.
                //
                // Mirror of the forward solver: each successor's IN state is
                // passed through `transfer_edge` before joining so a backward
                // analysis that cares about branch provenance (e.g. taint on
                // the taken branch) can observe the edge kind. Default
                // `transfer_edge` is identity — liveness remains zero-cost.
                let mut state: Option<L> = None;
                for (succ_idx, edge_kind) in self.cfg.outgoing_edges(block_idx) {
                    if let Some(succ_in) = self.block_in.get(&succ_idx) {
                        let edge_state = self.transfer.transfer_edge(*edge_kind, succ_in);
                        match &mut state {
                            None => {
                                state = Some(edge_state);
                            }
                            Some(s) => {
                                s.join_in_place(&edge_state);
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

            CfgVertex::ForLoop(for_vertex) => {
                // For loop: process from/to bound expressions
                let mut state = in_state.clone();
                self.transfer.transfer_expr_in_place(for_vertex.from, &mut state, &self.body);
                self.transfer.transfer_expr_in_place(for_vertex.to, &mut state, &self.body);
                state
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
        let bottom = IntSetLattice { values: vec![] };
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
        let bottom = IntSetLattice { values: vec![] };
        assert_eq!(bottom.join(&a), a);
        assert_eq!(a.join(&bottom), a);
    }

    /// Edge-blind `Transfer` impl — inherits the default `transfer_edge`.
    /// Exists purely to exercise the trait's default implementation so the
    /// zero-regression guarantee for reaching-defs / liveness is pinned.
    struct NoopTransfer;

    impl Transfer<IntSetLattice> for NoopTransfer {
        fn transfer_stmt(&self, _: RawIdx, state: &IntSetLattice, _: &Body) -> IntSetLattice {
            state.clone()
        }
    }

    #[test]
    fn transfer_edge_default_is_identity_across_all_edge_kinds() {
        // Regression guard for Task 6.0: every existing analysis relies on
        // the default `transfer_edge` being a clean identity so migrating to
        // the branch-aware API costs zero.
        let s = IntSetLattice { values: vec![1, 2, 3] };
        let t = NoopTransfer;
        for edge in [
            CfgEdgeType::Direct,
            CfgEdgeType::TrueBranch,
            CfgEdgeType::FalseBranch,
            CfgEdgeType::LoopIteration,
            CfgEdgeType::AdjacentCode,
        ] {
            assert_eq!(t.transfer_edge(edge, &s), s, "edge {edge:?}");
        }
    }

    /// Branch-aware `Transfer` that filters the lattice by sign on
    /// `TrueBranch` / `FalseBranch`. Stand-in for a narrowing analysis's
    /// edge hook: future `NarrowingAnalysis` will return the narrowed
    /// `Name → Ty` map instead of a bit-filtered set, but the override
    /// machinery is the same.
    struct SignSplitTransfer;

    impl Transfer<IntSetLattice> for SignSplitTransfer {
        fn transfer_stmt(&self, _: RawIdx, state: &IntSetLattice, _: &Body) -> IntSetLattice {
            state.clone()
        }
        fn transfer_edge(&self, edge_kind: CfgEdgeType, state: &IntSetLattice) -> IntSetLattice {
            match edge_kind {
                CfgEdgeType::TrueBranch => IntSetLattice {
                    values: state.values.iter().copied().filter(|v| *v > 0).collect(),
                },
                CfgEdgeType::FalseBranch => IntSetLattice {
                    values: state.values.iter().copied().filter(|v| *v <= 0).collect(),
                },
                _ => state.clone(),
            }
        }
    }

    #[test]
    fn transfer_edge_override_refines_per_branch() {
        // Pins that overriding `transfer_edge` is sufficient to split state
        // along `TrueBranch` vs `FalseBranch` — the exact shape Task 6's
        // narrowing analysis will use when reading a boolean guard.
        let s = IntSetLattice { values: vec![-2, -1, 0, 1, 2] };
        let t = SignSplitTransfer;
        assert_eq!(t.transfer_edge(CfgEdgeType::TrueBranch, &s).values, vec![1, 2]);
        assert_eq!(t.transfer_edge(CfgEdgeType::FalseBranch, &s).values, vec![-2, -1, 0]);
        // Non-conditional edges stay identity — LoopIteration / AdjacentCode
        // are guard-free and must not be refined.
        assert_eq!(t.transfer_edge(CfgEdgeType::Direct, &s), s);
        assert_eq!(t.transfer_edge(CfgEdgeType::LoopIteration, &s), s);
        assert_eq!(t.transfer_edge(CfgEdgeType::AdjacentCode, &s), s);
    }

    /// End-to-end: build a hand-rolled diamond CFG, feed an entry state,
    /// run `DataflowSolver::solve()` with a branch-aware `SignSplitTransfer`,
    /// and assert the `TrueBranch` / `FalseBranch` successors observe
    /// refined IN states. Without the solver wiring from this task the
    /// merge block would see the unrefined state on both sides and this
    /// test would fail on the `tside` / `fside` assertions — so it
    /// directly pins the `solve_forward` edge-hook plumbing, not just the
    /// trait method in isolation.
    #[test]
    fn solve_forward_applies_transfer_edge_at_branch_successors() {
        use cfg::{BasicBlockVertex, CfgVertex, ControlFlowGraph};

        // Diamond:
        //
        //   entry ──direct──▶ cond ──true──▶ tside ─┐
        //                         └──false──▶ fside ┤
        //                                           ▼
        //                                         merge ──direct──▶ exit
        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        let cond = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        let tside = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        let fside = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        let merge = cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()));
        let exit = cfg.exit_point();
        cfg.set_entry_point(entry);
        cfg.add_edge(entry, cond, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(cond, tside, CfgEdgeType::TrueBranch).unwrap();
        cfg.add_edge(cond, fside, CfgEdgeType::FalseBranch).unwrap();
        cfg.add_edge(tside, merge, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(fside, merge, CfgEdgeType::Direct).unwrap();
        cfg.add_edge(merge, exit, CfgEdgeType::Direct).unwrap();

        let body = Body::default();
        let initial = IntSetLattice { values: vec![-2, -1, 0, 1, 2] };

        let mut solver = DataflowSolver::new(Arc::new(cfg), body, SignSplitTransfer);
        let bottom = IntSetLattice { values: vec![] };
        solver.set_bottom_factory(move || bottom.clone());
        solver.set_initial_state(initial.clone());
        let result = solver.solve().expect("forward solve converges");

        // TrueBranch edge must have refined the state: only positives reach tside.
        let tside_in = result.block_in(tside).expect("tside IN exists");
        assert_eq!(tside_in.values, vec![1, 2], "TrueBranch edge did not refine state");

        // FalseBranch edge must have refined symmetrically: only ≤0 reach fside.
        let fside_in = result.block_in(fside).expect("fside IN exists");
        assert_eq!(fside_in.values, vec![-2, -1, 0], "FalseBranch edge did not refine state");

        // After the merge, join of both refined sides must recover the full
        // original set — proves we are not over-narrowing on the merge edges
        // and that join_in_place composes correctly with the new hook.
        let merge_in = result.block_in(merge).expect("merge IN exists");
        let mut merged: Vec<i32> = merge_in.values.clone();
        merged.sort_unstable();
        assert_eq!(merged, vec![-2, -1, 0, 1, 2], "diamond merge must recover full set");
    }
}
