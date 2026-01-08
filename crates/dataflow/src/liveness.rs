//! Liveness analysis for detecting unused variables.
//!
//! This module implements backward liveness analysis using the dataflow framework.
//!
//! ## What is Liveness?
//!
//! A variable is "live" at a program point if its value may be read on some path
//! to the program exit. A variable is "dead" if it will never be read again.
//!
//! ## Algorithm (Backward Analysis)
//!
//! Backward dataflow: we start from the exit and work backwards.
//!
//! ```text
//! IN[B] = USE[B] ∪ (OUT[B] - DEF[B])
//! OUT[B] = ∪ IN[S] for all successors S
//! ```
//!
//! - **USE[B]**: Variables read in block B
//! - **DEF[B]**: Variables defined (assigned) in block B
//! - **IN[B]**: Variables live at the start of B
//! - **OUT[B]**: Variables live at the end of B
//!
//! ## Example
//!
//! ```bsl
//! Процедура Тест()
//!     Перем X, Y;
//!     X = 10;           // DEF(X), X becomes live
//!     Y = X + 5;        // USE(X), DEF(Y), Y becomes live
//!     Сообщить(Y);      // USE(Y), Y dies after this
//! КонецПроцедуры
//! ```
//!
//! **Backwards analysis:**
//! - After line 5: Y is live
//! - Before line 5 (OUT of line 4): Y is live
//! - After line 4 (IN of line 4): X, Y are live
//! - After line 3 (OUT of line 3): X is live
//! - After line 2: Nothing is live
//!
//! **Unused variables:** None (both X and Y are used)
//!
//! ## Why Backwards?
//!
//! Liveness naturally flows backwards:
//! - If a variable is used in statement S, it must be live before S
//! - If a variable is defined in statement S, it's dead after S (unless used later)
//!
//! Forward analysis would require computing all future uses, which is less efficient.

use super::{Lattice, Transfer};
use fixedbitset::FixedBitSet;
use hir_def::body::Body;
use hir_def::hir::{Expr, Stmt};
use hir_def::ExprId;
use la_arena::RawIdx;
use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

// ============================================================================
// Profiling counters for performance analysis
// ============================================================================

/// Counter for get_index() calls (each involves to_lowercase + SmolStr allocation)
pub static GET_INDEX_CALLS: AtomicU64 = AtomicU64::new(0);

/// Counter for is_live() calls
pub static IS_LIVE_CALLS: AtomicU64 = AtomicU64::new(0);

/// Counter for insert() calls
pub static INSERT_CALLS: AtomicU64 = AtomicU64::new(0);

/// Counter for remove() calls
pub static REMOVE_CALLS: AtomicU64 = AtomicU64::new(0);

/// Counter for collect_expr_vars() calls
pub static COLLECT_EXPR_CALLS: AtomicU64 = AtomicU64::new(0);

/// Counter for VariableIndex::from_body() calls
pub static FROM_BODY_CALLS: AtomicU64 = AtomicU64::new(0);

/// Total time spent in VariableIndex::from_body() (nanoseconds)
pub static FROM_BODY_TIME_NS: AtomicU64 = AtomicU64::new(0);

/// Reset all profiling counters to zero.
pub fn reset_counters() {
    GET_INDEX_CALLS.store(0, Ordering::Relaxed);
    IS_LIVE_CALLS.store(0, Ordering::Relaxed);
    INSERT_CALLS.store(0, Ordering::Relaxed);
    REMOVE_CALLS.store(0, Ordering::Relaxed);
    COLLECT_EXPR_CALLS.store(0, Ordering::Relaxed);
    FROM_BODY_CALLS.store(0, Ordering::Relaxed);
    FROM_BODY_TIME_NS.store(0, Ordering::Relaxed);
}

/// Print profiling counters.
pub fn print_counters() {
    let get_index = GET_INDEX_CALLS.load(Ordering::Relaxed);
    let is_live = IS_LIVE_CALLS.load(Ordering::Relaxed);
    let insert = INSERT_CALLS.load(Ordering::Relaxed);
    let remove = REMOVE_CALLS.load(Ordering::Relaxed);
    let collect_expr = COLLECT_EXPR_CALLS.load(Ordering::Relaxed);
    let from_body = FROM_BODY_CALLS.load(Ordering::Relaxed);
    let from_body_ms = FROM_BODY_TIME_NS.load(Ordering::Relaxed) / 1_000_000;

    eprintln!("\n=== Liveness Profiling ===");
    eprintln!("  get_index_calls:      {:>12}", get_index);
    eprintln!("  is_live_calls:        {:>12}", is_live);
    eprintln!("  insert_calls:         {:>12}", insert);
    eprintln!("  remove_calls:         {:>12}", remove);
    eprintln!("  collect_expr_calls:   {:>12}", collect_expr);
    eprintln!("  from_body_calls:      {:>12}", from_body);
    eprintln!("  from_body_time_ms:    {:>12}", from_body_ms);
}

/// Maps variable names to compact indices for BitSet representation.
///
/// Shared across all Liveness instances in a single method analysis via Arc.
/// This allows O(1) variable lookup and compact bitset storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableIndex {
    /// Map from lowercase variable name to index
    name_to_idx: FxHashMap<SmolStr, usize>,
    /// Map from index back to name (for debugging)
    idx_to_name: Vec<SmolStr>,
    /// Cache: ExprId -> variable index for all Path expressions
    /// This enables O(1) lookup in collect_expr_vars without string allocation
    expr_idx_cache: FxHashMap<ExprId, usize>,
    /// Cache: BindingId -> variable index for all bindings
    /// This enables O(1) lookup in transfer_stmt without string allocation
    binding_idx_cache: FxHashMap<hir_def::BindingId, usize>,
}

impl VariableIndex {
    /// Create variable index from a method body.
    ///
    /// Extracts all variable names (bindings + implicit variables from assignments)
    /// and assigns sequential indices.
    pub fn from_body(body: &Body) -> Arc<Self> {
        FROM_BODY_CALLS.fetch_add(1, Ordering::Relaxed);
        let start = Instant::now();

        let mut name_to_idx = FxHashMap::default();
        let mut idx_to_name = Vec::new();

        // Collect all variable names from bindings
        for (_local_id, binding) in body.bindings.iter() {
            let lowercase: SmolStr = binding.name.as_str().to_lowercase().into();

            if !name_to_idx.contains_key(&lowercase) {
                let idx = idx_to_name.len();
                name_to_idx.insert(lowercase.clone(), idx);
                idx_to_name.push(lowercase);
            }
        }

        // Also collect implicit variables (from Assign statements without Перем declaration)
        // These are Path expressions on the LHS of assignments
        fn collect_implicit_vars(
            stmts: &[hir_def::StmtId],
            body: &Body,
            name_to_idx: &mut FxHashMap<SmolStr, usize>,
            idx_to_name: &mut Vec<SmolStr>,
        ) {
            for stmt_id in stmts {
                match &body.stmts[*stmt_id] {
                    hir_def::hir::Stmt::Assign { target, .. } => {
                        if let Expr::Path(name) = &body.exprs[*target] {
                            let lowercase: SmolStr = name.as_str().to_lowercase().into();
                            if !name_to_idx.contains_key(&lowercase) {
                                let idx = idx_to_name.len();
                                name_to_idx.insert(lowercase.clone(), idx);
                                idx_to_name.push(lowercase);
                            }
                        }
                    }
                    // Recursively check nested statements in control flow
                    hir_def::hir::Stmt::If { then_branch, elsif_branches, else_branch, .. } => {
                        collect_implicit_vars(then_branch, body, name_to_idx, idx_to_name);
                        for (_cond, stmts) in elsif_branches.iter() {
                            collect_implicit_vars(stmts, body, name_to_idx, idx_to_name);
                        }
                        if let Some(else_stmts) = else_branch {
                            collect_implicit_vars(else_stmts, body, name_to_idx, idx_to_name);
                        }
                    }
                    hir_def::hir::Stmt::While { body: loop_body, .. } => {
                        collect_implicit_vars(loop_body, body, name_to_idx, idx_to_name);
                    }
                    hir_def::hir::Stmt::For { body: loop_body, .. } => {
                        collect_implicit_vars(loop_body, body, name_to_idx, idx_to_name);
                    }
                    hir_def::hir::Stmt::ForEach { body: loop_body, .. } => {
                        collect_implicit_vars(loop_body, body, name_to_idx, idx_to_name);
                    }
                    hir_def::hir::Stmt::Try { body: try_body, except, .. } => {
                        collect_implicit_vars(try_body, body, name_to_idx, idx_to_name);
                        collect_implicit_vars(except, body, name_to_idx, idx_to_name);
                    }
                    _ => {}
                }
            }
        }

        collect_implicit_vars(&body.body_stmts, body, &mut name_to_idx, &mut idx_to_name);

        // Build expr_idx_cache: map all Path expressions to their variable indices
        // This enables O(1) lookup in collect_expr_vars without string allocation
        let mut expr_idx_cache = FxHashMap::default();
        for (expr_id, expr) in body.exprs.iter() {
            if let Expr::Path(name) = expr {
                let lowercase: SmolStr = name.as_str().to_lowercase().into();
                if let Some(&idx) = name_to_idx.get(&lowercase) {
                    expr_idx_cache.insert(expr_id, idx);
                }
            }
        }

        // Build binding_idx_cache: map all bindings to their variable indices
        // This enables O(1) lookup in transfer_stmt for VarDecl, For, ForEach
        let mut binding_idx_cache = FxHashMap::default();
        for (binding_id, binding) in body.bindings.iter() {
            let lowercase: SmolStr = binding.name.as_str().to_lowercase().into();
            if let Some(&idx) = name_to_idx.get(&lowercase) {
                binding_idx_cache.insert(binding_id, idx);
            }
        }

        FROM_BODY_TIME_NS.fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);

        Arc::new(Self { name_to_idx, idx_to_name, expr_idx_cache, binding_idx_cache })
    }

    /// Get index for a variable name (lowercase).
    ///
    /// **Performance:** This method allocates a SmolStr for lowercase conversion.
    /// For hot paths, prefer `get_index_by_smolstr` with pre-computed lowercase.
    pub fn get_index(&self, var_name: &str) -> Option<usize> {
        GET_INDEX_CALLS.fetch_add(1, Ordering::Relaxed);
        let lowercase: SmolStr = var_name.to_lowercase().into();
        self.name_to_idx.get(&lowercase).copied()
    }

    /// Get index for a variable name without allocation.
    ///
    /// **Performance:** O(1) lookup, no allocation. Caller must provide lowercase SmolStr.
    #[inline]
    pub fn get_index_by_smolstr(&self, lowercase_name: &SmolStr) -> Option<usize> {
        self.name_to_idx.get(lowercase_name).copied()
    }

    /// Get the internal name->index map for batch lookups.
    ///
    /// This is useful for building a cache of variable indices.
    #[inline]
    pub fn name_to_idx_map(&self) -> &FxHashMap<SmolStr, usize> {
        &self.name_to_idx
    }

    /// Get variable index by ExprId (for Path expressions).
    ///
    /// **Performance:** O(1) lookup from pre-computed cache, no allocation.
    /// Returns None if expr_id is not a Path or the variable is not known.
    #[inline]
    pub fn get_index_by_expr(&self, expr_id: ExprId) -> Option<usize> {
        self.expr_idx_cache.get(&expr_id).copied()
    }

    /// Get variable index by BindingId.
    ///
    /// **Performance:** O(1) lookup from pre-computed cache, no allocation.
    #[inline]
    pub fn get_index_by_binding(&self, binding_id: hir_def::BindingId) -> Option<usize> {
        self.binding_idx_cache.get(&binding_id).copied()
    }

    /// Get total number of variables.
    pub fn size(&self) -> usize {
        self.idx_to_name.len()
    }

    /// Get variable name by index (for debugging).
    #[allow(dead_code)]
    pub fn get_name(&self, idx: usize) -> Option<&SmolStr> {
        self.idx_to_name.get(idx)
    }
}

/// Liveness lattice: BitSet of live variables at a program point.
///
/// Uses BitSet for compact storage and fast join operations.
/// Variables are case-insensitive (BSL language property).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Liveness {
    /// Bitset where bit i indicates if variable i is live
    live_vars: FixedBitSet,
    /// Shared variable index mapping (Arc for cheap clones)
    var_index: Arc<VariableIndex>,
}

impl Liveness {
    /// Create a new empty liveness set with given variable index.
    pub fn new(var_index: Arc<VariableIndex>) -> Self {
        Self { live_vars: FixedBitSet::with_capacity(var_index.size()), var_index }
    }

    /// Check if a variable is live.
    ///
    /// **Performance:** Allocates for lowercase conversion. Use `is_live_by_idx` in hot paths.
    pub fn is_live(&self, var_name: &str) -> bool {
        IS_LIVE_CALLS.fetch_add(1, Ordering::Relaxed);
        self.var_index.get_index(var_name).map(|idx| self.live_vars.contains(idx)).unwrap_or(false)
    }

    /// Check if a variable is live by index.
    ///
    /// **Performance:** O(1), no allocation.
    #[inline]
    pub fn is_live_by_idx(&self, idx: usize) -> bool {
        self.live_vars.contains(idx)
    }

    /// Add a variable to the live set.
    ///
    /// **Performance:** Allocates for lowercase conversion. Use `insert_by_idx` in hot paths.
    pub fn insert(&mut self, var_name: &str) {
        INSERT_CALLS.fetch_add(1, Ordering::Relaxed);
        if let Some(idx) = self.var_index.get_index(var_name) {
            self.live_vars.insert(idx);
        }
    }

    /// Add a variable to the live set by index.
    ///
    /// **Performance:** O(1), no allocation.
    #[inline]
    pub fn insert_by_idx(&mut self, idx: usize) {
        INSERT_CALLS.fetch_add(1, Ordering::Relaxed);
        self.live_vars.insert(idx);
    }

    /// Remove a variable from the live set (variable is killed).
    ///
    /// **Performance:** Allocates for lowercase conversion. Use `remove_by_idx` in hot paths.
    pub fn remove(&mut self, var_name: &str) {
        REMOVE_CALLS.fetch_add(1, Ordering::Relaxed);
        if let Some(idx) = self.var_index.get_index(var_name) {
            self.live_vars.set(idx, false);
        }
    }

    /// Remove a variable from the live set by index.
    ///
    /// **Performance:** O(1), no allocation.
    #[inline]
    pub fn remove_by_idx(&mut self, idx: usize) {
        REMOVE_CALLS.fetch_add(1, Ordering::Relaxed);
        self.live_vars.set(idx, false);
    }

    /// Get access to the raw BitSet for batch operations.
    #[inline]
    pub fn live_vars(&self) -> &FixedBitSet {
        &self.live_vars
    }

    /// Get mutable access to the raw BitSet for batch operations.
    #[inline]
    pub fn live_vars_mut(&mut self) -> &mut FixedBitSet {
        &mut self.live_vars
    }

    /// Get the number of live variables.
    pub fn len(&self) -> usize {
        self.live_vars.count_ones(..)
    }

    /// Check if there are no live variables.
    pub fn is_empty(&self) -> bool {
        self.live_vars.is_clear()
    }

    /// Get the variable index (for creating new instances).
    pub fn var_index(&self) -> &Arc<VariableIndex> {
        &self.var_index
    }
}

impl Lattice for Liveness {
    /// Bottom element: no variables are live.
    ///
    /// This represents the state at program exit where nothing will be read anymore.
    ///
    /// **Note**: Cannot create bottom without VariableIndex!
    /// The solver must use initialization with a factory function.
    fn bottom() -> Self {
        panic!("Liveness::bottom() requires VariableIndex - use new() with var_index instead")
    }

    /// Join operation: union of live variables (bitwise OR).
    ///
    /// A variable is live if it's live in ANY successor.
    /// This is a **may-analysis**: conservative, assumes all paths are possible.
    ///
    /// Uses fast bitwise OR operation on FixedBitSet (much faster than hash set union).
    fn join(&self, other: &Self) -> Self {
        debug_assert!(
            Arc::ptr_eq(&self.var_index, &other.var_index),
            "Cannot join liveness sets from different methods"
        );

        let mut result = self.live_vars.clone();
        result.union_with(&other.live_vars);

        Self {
            live_vars: result,
            var_index: self.var_index.clone(), // Arc clone is O(1)
        }
    }

    /// In-place join: union of live variables without allocation.
    ///
    /// Optimized version that modifies self directly instead of creating a new Liveness.
    /// This avoids BitSet clone which is O(n/64) for n variables.
    fn join_in_place(&mut self, other: &Self) {
        debug_assert!(
            Arc::ptr_eq(&self.var_index, &other.var_index),
            "Cannot join liveness sets from different methods"
        );

        // Just do in-place union - no clone needed!
        self.live_vars.union_with(&other.live_vars);
    }
}

/// Liveness transfer function (backward).
///
/// For each statement, computes IN from OUT:
/// - **Kill**: Variables defined (assigned) are removed from live set
/// - **Gen**: Variables used (read) are added to live set
///
/// ## Example
///
/// ```bsl
/// X = Y + Z;  // DEF(X), USE(Y), USE(Z)
/// ```
///
/// Backwards:
/// - OUT = {X, Y, Z} (assume X, Y, Z are live after this statement)
/// - Kill X (it's defined here)
/// - Gen Y, Z (they're used here)
/// - IN = {Y, Z}
pub struct LivenessTransfer;

impl Transfer<Liveness> for LivenessTransfer {
    /// Apply transfer function for a single statement (backward).
    ///
    /// Given OUT state (live variables after statement), computes IN state (live variables before statement).
    fn transfer_stmt(&self, stmt_id: RawIdx, state: &Liveness, body: &Body) -> Liveness {
        use hir_def::StmtId;

        // OUT[stmt] = state (from successors)
        let mut in_state = state.clone();

        let stmt = body.stmt(StmtId::from_raw(stmt_id));

        match stmt {
            Stmt::VarDecl { bindings } => {
                // Variable declaration: DEF(var)
                // Kill: remove all declared variables from live set (they're defined here)
                for &binding_id in bindings.iter() {
                    // Fast path: use pre-computed binding index (O(1), no allocation)
                    if let Some(idx) = in_state.var_index().get_index_by_binding(binding_id) {
                        in_state.remove_by_idx(idx);
                    }
                }

                // Note: VarDecl in HIR doesn't have initializers
                // They're represented as separate Assign statements
            }

            Stmt::Assign { target, value } => {
                // Assignment: DEF(target), USE(value)

                // Kill: target variable (if simple path)
                if matches!(body.expr(*target), Expr::Path(_)) {
                    // Fast path: use pre-computed expr index (O(1), no allocation)
                    if let Some(idx) = in_state.var_index().get_index_by_expr(*target) {
                        in_state.remove_by_idx(idx);
                    }
                } else {
                    // Complex assignment: Obj.Field = ... or Arr[i] = ...
                    // Base object is used (read), not killed
                    collect_expr_vars(*target, body, &mut in_state);
                }

                // Gen: value expression variables are used
                collect_expr_vars(*value, body, &mut in_state);
            }

            Stmt::Expr(expr_id) => {
                // Expression statement: USE(expr)
                // Gen: all variables in expression are used
                collect_expr_vars(*expr_id, body, &mut in_state);
            }

            Stmt::Return { value } => {
                // Return statement: USE(return_value)
                if let Some(expr_id) = value {
                    collect_expr_vars(*expr_id, body, &mut in_state);
                }
            }

            Stmt::Raise { value } => {
                // Raise statement: USE(value)
                if let Some(expr_id) = value {
                    collect_expr_vars(*expr_id, body, &mut in_state);
                }
            }

            Stmt::If { condition, .. } => {
                // If statement: USE(condition)
                collect_expr_vars(*condition, body, &mut in_state);

                // Branches are handled by CFG - each branch is a separate basic block
                // We just need to handle condition here
                // The transfer function is called separately for each basic block in branches
            }

            Stmt::While { condition, .. } => {
                // While loop: USE(condition)
                collect_expr_vars(*condition, body, &mut in_state);

                // Loop body is in separate basic blocks (handled by CFG)
                // Transfer function will be called for each basic block in the loop
            }

            Stmt::For { var, from, to, .. } => {
                // For loop: DEF(var), USE(from), USE(to)

                // Gen: from and to expressions
                collect_expr_vars(*from, body, &mut in_state);
                collect_expr_vars(*to, body, &mut in_state);

                // Kill: loop variable (it's defined by for loop)
                // Fast path: use pre-computed binding index (O(1), no allocation)
                if let Some(idx) = in_state.var_index().get_index_by_binding(*var) {
                    in_state.remove_by_idx(idx);
                }

                // Loop body is in separate basic blocks (handled by CFG)
            }

            Stmt::ForEach { var, collection, .. } => {
                // ForEach loop: DEF(var), USE(collection)

                // Gen: collection expression
                collect_expr_vars(*collection, body, &mut in_state);

                // Kill: loop variable
                // Fast path: use pre-computed binding index (O(1), no allocation)
                if let Some(idx) = in_state.var_index().get_index_by_binding(*var) {
                    in_state.remove_by_idx(idx);
                }

                // Loop body is in separate basic blocks (handled by CFG)
            }

            Stmt::Try { .. } => {
                // Try-Except: bodies are in separate basic blocks
                // Transfer function will be called for each block separately
            }

            Stmt::Break | Stmt::Continue | Stmt::Goto(_) | Stmt::Label(_) => {
                // Control flow statements: no variables used/defined
            }

            Stmt::Execute { expr } => {
                // Execute statement: USE(expr)
                collect_expr_vars(*expr, body, &mut in_state);
            }

            Stmt::AddHandler { .. } | Stmt::RemoveHandler { .. } => {
                // Event handler statements - may use variables but ignore for now
                // TODO: handle event handler expressions
            }
        }

        in_state
    }

    /// Apply transfer function for an expression (backward).
    ///
    /// For control flow expressions (While condition, For bounds, etc.),
    /// marks all variables in the expression as live (GEN).
    fn transfer_expr(&self, expr_id: hir_def::ExprId, state: &Liveness, body: &Body) -> Liveness {
        let mut in_state = state.clone();
        collect_expr_vars(expr_id, body, &mut in_state);
        in_state
    }

    /// Optimized in-place transfer for a statement (no clone).
    ///
    /// Modifies state directly instead of returning a new Liveness.
    fn transfer_stmt_in_place(&self, stmt_id: RawIdx, state: &mut Liveness, body: &Body) {
        use hir_def::StmtId;

        let stmt = body.stmt(StmtId::from_raw(stmt_id));

        match stmt {
            Stmt::VarDecl { bindings } => {
                for &binding_id in bindings.iter() {
                    if let Some(idx) = state.var_index().get_index_by_binding(binding_id) {
                        state.remove_by_idx(idx);
                    }
                }
            }

            Stmt::Assign { target, value } => {
                if matches!(body.expr(*target), Expr::Path(_)) {
                    if let Some(idx) = state.var_index().get_index_by_expr(*target) {
                        state.remove_by_idx(idx);
                    }
                } else {
                    collect_expr_vars(*target, body, state);
                }
                collect_expr_vars(*value, body, state);
            }

            Stmt::Expr(expr_id) => {
                collect_expr_vars(*expr_id, body, state);
            }

            Stmt::Return { value } => {
                if let Some(expr_id) = value {
                    collect_expr_vars(*expr_id, body, state);
                }
            }

            Stmt::Raise { value } => {
                if let Some(expr_id) = value {
                    collect_expr_vars(*expr_id, body, state);
                }
            }

            Stmt::If { condition, .. } => {
                collect_expr_vars(*condition, body, state);
            }

            Stmt::While { condition, .. } => {
                collect_expr_vars(*condition, body, state);
            }

            Stmt::For { var, from, to, .. } => {
                collect_expr_vars(*from, body, state);
                collect_expr_vars(*to, body, state);
                if let Some(idx) = state.var_index().get_index_by_binding(*var) {
                    state.remove_by_idx(idx);
                }
            }

            Stmt::ForEach { var, collection, .. } => {
                collect_expr_vars(*collection, body, state);
                if let Some(idx) = state.var_index().get_index_by_binding(*var) {
                    state.remove_by_idx(idx);
                }
            }

            Stmt::Try { .. } => {}

            Stmt::Break | Stmt::Continue | Stmt::Goto(_) | Stmt::Label(_) => {}

            Stmt::Execute { expr } => {
                collect_expr_vars(*expr, body, state);
            }

            Stmt::AddHandler { .. } | Stmt::RemoveHandler { .. } => {}
        }
    }

    /// Optimized in-place transfer for an expression (no clone).
    fn transfer_expr_in_place(&self, expr_id: hir_def::ExprId, state: &mut Liveness, body: &Body) {
        collect_expr_vars(expr_id, body, state);
    }
}

/// Recursively collect all variables used in an expression.
///
/// Uses pre-computed expr_idx_cache for O(1) Path lookups without string allocation.
/// Falls back to string-based lookup for expressions not in cache.
fn collect_expr_vars(expr_id: ExprId, body: &Body, liveness: &mut Liveness) {
    COLLECT_EXPR_CALLS.fetch_add(1, Ordering::Relaxed);

    // Fast path: check expr_idx_cache first (O(1), no allocation)
    if let Some(idx) = liveness.var_index().get_index_by_expr(expr_id) {
        liveness.insert_by_idx(idx);
        return;
    }

    let expr = body.expr(expr_id);

    match expr {
        Expr::Missing => {}

        Expr::Path(_) => {
            // Path not in expr_idx_cache means it's a non-local identifier
            // (global variable, built-in function, method name, etc.)
            // These are not tracked in liveness analysis - skip them
        }

        Expr::Literal(_) => {
            // Literals don't use variables
        }

        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_expr_vars(*lhs, body, liveness);
            collect_expr_vars(*rhs, body, liveness);
        }

        Expr::UnaryOp { expr, .. } => {
            collect_expr_vars(*expr, body, liveness);
        }

        Expr::Call { callee, args } => {
            collect_expr_vars(*callee, body, liveness);
            for &arg_expr in args.iter() {
                collect_expr_vars(arg_expr, body, liveness);
            }
        }

        Expr::MethodCall { receiver, args, .. } => {
            collect_expr_vars(*receiver, body, liveness);
            for &arg_expr in args.iter() {
                collect_expr_vars(arg_expr, body, liveness);
            }
        }

        Expr::Field { base, .. } => {
            collect_expr_vars(*base, body, liveness);
        }

        Expr::Index { base, index } => {
            collect_expr_vars(*base, body, liveness);
            collect_expr_vars(*index, body, liveness);
        }

        Expr::Ternary { condition, then_expr, else_expr } => {
            collect_expr_vars(*condition, body, liveness);
            collect_expr_vars(*then_expr, body, liveness);
            collect_expr_vars(*else_expr, body, liveness);
        }

        Expr::New { args, .. } => {
            for &arg_expr in args.iter() {
                collect_expr_vars(arg_expr, body, liveness);
            }
        }

        Expr::Await { expr } => {
            collect_expr_vars(*expr, body, liveness);
        }

        Expr::QualifiedPath(_) => {
            // Qualified path (e.g., ModuleName.FunctionName) - no local variables
        }

        Expr::Array(elements) => {
            // Array literal: [expr1, expr2, ...]
            for &elem_expr in elements.iter() {
                collect_expr_vars(elem_expr, body, liveness);
            }
        }
    }
}

/// Perform liveness analysis directly without going through Salsa queries.
///
/// This is an optimized path for batch processing multiple methods from the same file,
/// avoiding repeated Salsa lookups for module_bodies and method_cfg.
///
/// # Arguments
/// * `body` - HIR body of the method
/// * `cfg` - Control Flow Graph (already built)
/// * `var_index` - Variable index (already built)
///
/// # Returns
/// DataflowResult with liveness information for each CFG block, or None if analysis fails.
pub fn liveness_analysis_direct(
    body: &hir_def::Body,
    cfg: &cfg::ControlFlowGraph,
    var_index: std::sync::Arc<VariableIndex>,
) -> Option<crate::DataflowResult<Liveness>> {
    let transfer = LivenessTransfer;
    let mut solver =
        crate::DataflowSolver::new(std::sync::Arc::new(cfg.clone()), body.clone(), transfer);
    solver.set_direction(crate::Direction::Backward);
    solver.set_bottom_factory(|| Liveness::new(var_index.clone()));
    solver.solve()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a test VariableIndex with given variable names
    fn create_var_index(var_names: &[&str]) -> Arc<VariableIndex> {
        let mut name_to_idx = FxHashMap::default();
        let mut idx_to_name = Vec::new();

        for &name in var_names {
            let lowercase: SmolStr = name.to_lowercase().into();
            if !name_to_idx.contains_key(&lowercase) {
                let idx = idx_to_name.len();
                name_to_idx.insert(lowercase.clone(), idx);
                idx_to_name.push(lowercase);
            }
        }

        Arc::new(VariableIndex {
            name_to_idx,
            idx_to_name,
            expr_idx_cache: FxHashMap::default(),
            binding_idx_cache: FxHashMap::default(),
        })
    }

    #[test]
    fn test_liveness_insert() {
        let var_index = create_var_index(&["Переменная"]);
        let mut liveness = Liveness::new(var_index);
        liveness.insert("Переменная");
        assert!(liveness.is_live("Переменная"));
        assert!(liveness.is_live("переменная")); // Case-insensitive
    }

    #[test]
    fn test_liveness_join() {
        let var_index = create_var_index(&["X", "Y", "Z"]);

        let mut a = Liveness::new(var_index.clone());
        a.insert("X");
        a.insert("Y");

        let mut b = Liveness::new(var_index);
        b.insert("Y");
        b.insert("Z");

        let joined = a.join(&b);
        assert!(joined.is_live("X"));
        assert!(joined.is_live("Y"));
        assert!(joined.is_live("Z"));
    }

    #[test]
    fn test_liveness_join_idempotent() {
        let var_index = create_var_index(&["X"]);
        let mut a = Liveness::new(var_index);
        a.insert("X");
        let joined = a.join(&a);
        assert_eq!(joined, a);
    }

    #[test]
    fn test_liveness_join_commutative() {
        let var_index = create_var_index(&["X", "Y"]);
        let mut a = Liveness::new(var_index.clone());
        a.insert("X");
        let mut b = Liveness::new(var_index);
        b.insert("Y");
        assert_eq!(a.join(&b), b.join(&a));
    }
}
