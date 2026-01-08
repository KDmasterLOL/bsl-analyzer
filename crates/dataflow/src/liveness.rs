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
use hir_def::body::Body;
use hir_def::hir::{Expr, Stmt};
use hir_def::ExprId;
use la_arena::RawIdx;
use rustc_hash::FxHashSet;
use smol_str::SmolStr;

/// Liveness lattice: set of live variables at a program point.
///
/// Variables are stored in lowercase for case-insensitive matching (BSL is case-insensitive).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Liveness {
    /// Set of variables that are live (may be read in the future).
    live_vars: FxHashSet<SmolStr>,
}

impl Liveness {
    /// Create a new empty liveness set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a variable is live.
    pub fn is_live(&self, var_name: &str) -> bool {
        let lowercase: SmolStr = var_name.to_lowercase().into();
        self.live_vars.contains(&lowercase)
    }

    /// Add a variable to the live set.
    pub fn insert(&mut self, var_name: &str) {
        self.live_vars.insert(var_name.to_lowercase().into());
    }

    /// Remove a variable from the live set (variable is killed).
    pub fn remove(&mut self, var_name: &str) {
        let lowercase: SmolStr = var_name.to_lowercase().into();
        self.live_vars.remove(&lowercase);
    }

    /// Get the number of live variables.
    pub fn len(&self) -> usize {
        self.live_vars.len()
    }

    /// Check if there are no live variables.
    pub fn is_empty(&self) -> bool {
        self.live_vars.is_empty()
    }
}

impl Lattice for Liveness {
    /// Bottom element: no variables are live.
    ///
    /// This represents the state at program exit where nothing will be read anymore.
    fn bottom() -> Self {
        Self::new()
    }

    /// Join operation: union of live variables.
    ///
    /// A variable is live if it's live in ANY successor.
    /// This is a **may-analysis**: conservative, assumes all paths are possible.
    fn join(&self, other: &Self) -> Self {
        Self { live_vars: self.live_vars.union(&other.live_vars).cloned().collect() }
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
                    let binding = body.binding(binding_id);
                    in_state.remove(binding.name.as_str());
                }

                // Note: VarDecl in HIR doesn't have initializers
                // They're represented as separate Assign statements
            }

            Stmt::Assign { target, value } => {
                // Assignment: DEF(target), USE(value)

                // Kill: target variable (if simple path)
                if let Expr::Path(name) = body.expr(*target) {
                    // Simple assignment: X = ...
                    in_state.remove(name.as_str());
                } else {
                    // Complex assignment: Obj.Field = ... or Arr[i] = ...
                    // Base object is used (read), not killed
                    collect_expr_vars(*target, body, &mut in_state.live_vars);
                }

                // Gen: value expression variables are used
                collect_expr_vars(*value, body, &mut in_state.live_vars);
            }

            Stmt::Expr(expr_id) => {
                // Expression statement: USE(expr)
                // Gen: all variables in expression are used
                collect_expr_vars(*expr_id, body, &mut in_state.live_vars);
            }

            Stmt::Return { value } => {
                // Return statement: USE(return_value)
                if let Some(expr_id) = value {
                    collect_expr_vars(*expr_id, body, &mut in_state.live_vars);
                }
            }

            Stmt::Raise { value } => {
                // Raise statement: USE(value)
                if let Some(expr_id) = value {
                    collect_expr_vars(*expr_id, body, &mut in_state.live_vars);
                }
            }

            Stmt::If { condition, .. } => {
                // If statement: USE(condition)
                collect_expr_vars(*condition, body, &mut in_state.live_vars);

                // Branches are handled by CFG - each branch is a separate basic block
                // We just need to handle condition here
                // The transfer function is called separately for each basic block in branches
            }

            Stmt::While { condition, .. } => {
                // While loop: USE(condition)
                collect_expr_vars(*condition, body, &mut in_state.live_vars);

                // Loop body is in separate basic blocks (handled by CFG)
                // Transfer function will be called for each basic block in the loop
            }

            Stmt::For { var, from, to, .. } => {
                // For loop: DEF(var), USE(from), USE(to)

                // Gen: from and to expressions
                collect_expr_vars(*from, body, &mut in_state.live_vars);
                collect_expr_vars(*to, body, &mut in_state.live_vars);

                // Kill: loop variable (it's defined by for loop)
                let binding = body.binding(*var);
                in_state.remove(binding.name.as_str());

                // Loop body is in separate basic blocks (handled by CFG)
            }

            Stmt::ForEach { var, collection, .. } => {
                // ForEach loop: DEF(var), USE(collection)

                // Gen: collection expression
                collect_expr_vars(*collection, body, &mut in_state.live_vars);

                // Kill: loop variable
                let binding = body.binding(*var);
                in_state.remove(binding.name.as_str());

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
                collect_expr_vars(*expr, body, &mut in_state.live_vars);
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
        collect_expr_vars(expr_id, body, &mut in_state.live_vars);
        in_state
    }
}

/// Recursively collect all variables used in an expression.
///
/// Adds variable names (lowercase) to the `vars` set.
fn collect_expr_vars(expr_id: ExprId, body: &Body, vars: &mut FxHashSet<SmolStr>) {
    let expr = body.expr(expr_id);

    match expr {
        Expr::Missing => {}

        Expr::Path(name) => {
            // Variable reference: add to live set
            vars.insert(name.as_str().to_lowercase().into());
        }

        Expr::Literal(_) => {
            // Literals don't use variables
        }

        Expr::BinaryOp { lhs, rhs, .. } => {
            collect_expr_vars(*lhs, body, vars);
            collect_expr_vars(*rhs, body, vars);
        }

        Expr::UnaryOp { expr, .. } => {
            collect_expr_vars(*expr, body, vars);
        }

        Expr::Call { callee, args } => {
            collect_expr_vars(*callee, body, vars);
            for &arg_expr in args.iter() {
                collect_expr_vars(arg_expr, body, vars);
            }
        }

        Expr::MethodCall { receiver, args, .. } => {
            collect_expr_vars(*receiver, body, vars);
            for &arg_expr in args.iter() {
                collect_expr_vars(arg_expr, body, vars);
            }
        }

        Expr::Field { base, .. } => {
            collect_expr_vars(*base, body, vars);
        }

        Expr::Index { base, index } => {
            collect_expr_vars(*base, body, vars);
            collect_expr_vars(*index, body, vars);
        }

        Expr::Ternary { condition, then_expr, else_expr } => {
            collect_expr_vars(*condition, body, vars);
            collect_expr_vars(*then_expr, body, vars);
            collect_expr_vars(*else_expr, body, vars);
        }

        Expr::New { args, .. } => {
            for &arg_expr in args.iter() {
                collect_expr_vars(arg_expr, body, vars);
            }
        }

        Expr::Await { expr } => {
            collect_expr_vars(*expr, body, vars);
        }

        Expr::QualifiedPath(_) => {
            // Qualified path (e.g., ModuleName.FunctionName) - no local variables
        }

        Expr::Array(elements) => {
            // Array literal: [expr1, expr2, ...]
            for &elem_expr in elements.iter() {
                collect_expr_vars(elem_expr, body, vars);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_liveness_bottom() {
        let bottom = Liveness::bottom();
        assert!(bottom.is_empty());
    }

    #[test]
    fn test_liveness_insert() {
        let mut liveness = Liveness::new();
        liveness.insert("Переменная");
        assert!(liveness.is_live("Переменная"));
        assert!(liveness.is_live("переменная")); // Case-insensitive
    }

    #[test]
    fn test_liveness_join() {
        let mut a = Liveness::new();
        a.insert("X");
        a.insert("Y");

        let mut b = Liveness::new();
        b.insert("Y");
        b.insert("Z");

        let joined = a.join(&b);
        assert!(joined.is_live("X"));
        assert!(joined.is_live("Y"));
        assert!(joined.is_live("Z"));
    }

    #[test]
    fn test_liveness_join_idempotent() {
        let mut a = Liveness::new();
        a.insert("X");
        let joined = a.join(&a);
        assert_eq!(joined, a);
    }

    #[test]
    fn test_liveness_join_commutative() {
        let mut a = Liveness::new();
        a.insert("X");
        let mut b = Liveness::new();
        b.insert("Y");
        assert_eq!(a.join(&b), b.join(&a));
    }
}
