//! Reaching Definitions Analysis.
//!
//! Tracks which variable assignments (definitions) reach each program point.
//! This is a classic forward dataflow analysis using a may-analysis (union lattice).
//!
//! ## Algorithm
//!
//! For each basic block:
//! - **Gen**: Definitions created in this block
//! - **Kill**: Definitions overwritten in this block (same variable name)
//! - **IN[B]** = ∪ OUT[P] for all predecessors P
//! - **OUT[B]** = Gen[B] ∪ (IN[B] - Kill[B])
//!
//! ## Use Cases
//!
//! - Uninitialized variable detection
//! - Constant propagation
//! - Dead code elimination
//! - Variable usage tracking

use hir_def::{
    body::Body,
    hir::{BindingId, Expr, Stmt},
    Name,
};
use la_arena::RawIdx;
use rustc_hash::FxHashSet;
use smol_str::SmolStr;

use crate::{Lattice, Transfer};

/// Where a variable was defined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DefSite {
    /// Function/procedure parameter.
    Parameter(BindingId),

    /// Variable declaration (Перем x).
    VarDecl(BindingId),

    /// Assignment statement (x = value).
    Assignment(RawIdx), // StmtId as RawIdx to avoid circular dependency

    /// For loop variable (Для x = 1 По 10).
    ForLoop(BindingId),

    /// ForEach loop variable (Для Каждого x Из collection).
    ForEachLoop(BindingId),

    /// Unknown/external definition (imported, global, etc.).
    Unknown,
}

/// A definition: variable name + where it was defined.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Definition {
    /// Variable name (case-insensitive in BSL).
    pub var_name: SmolStr,

    /// Where this definition occurred.
    pub def_site: DefSite,
}

impl Definition {
    /// Create a new definition.
    pub fn new(var_name: SmolStr, def_site: DefSite) -> Self {
        // Normalize to lowercase for case-insensitive matching (BSL is case-insensitive)
        let var_name = SmolStr::new(var_name.to_lowercase());
        Self { var_name, def_site }
    }

    /// Create a parameter definition.
    pub fn parameter(name: &Name, binding_id: BindingId) -> Self {
        Self::new(SmolStr::new(name.as_str()), DefSite::Parameter(binding_id))
    }

    /// Create a variable declaration definition.
    pub fn var_decl(name: &Name, binding_id: BindingId) -> Self {
        Self::new(SmolStr::new(name.as_str()), DefSite::VarDecl(binding_id))
    }

    /// Create an assignment definition.
    pub fn assignment(var_name: SmolStr, stmt_id: RawIdx) -> Self {
        Self::new(var_name, DefSite::Assignment(stmt_id))
    }

    /// Create a for-loop variable definition.
    pub fn for_loop(name: &Name, binding_id: BindingId) -> Self {
        Self::new(SmolStr::new(name.as_str()), DefSite::ForLoop(binding_id))
    }

    /// Create a for-each loop variable definition.
    pub fn for_each_loop(name: &Name, binding_id: BindingId) -> Self {
        Self::new(SmolStr::new(name.as_str()), DefSite::ForEachLoop(binding_id))
    }

    /// Create an unknown definition.
    pub fn unknown(var_name: SmolStr) -> Self {
        Self::new(var_name, DefSite::Unknown)
    }
}

/// Reaching definitions lattice: set of definitions.
///
/// A definition reaches a program point if there is a path from the definition
/// to that point without an intervening kill (redefinition of the same variable).
///
/// ## Lattice Properties
///
/// - **Bottom (⊥)**: Empty set (no definitions)
/// - **Top (⊤)**: All possible definitions (not represented explicitly)
/// - **Order**: Set inclusion (A ⊑ B iff A ⊆ B)
/// - **Join (⊔)**: Set union (definitions reach if they reach from ANY path)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachingDefs {
    /// Set of definitions that reach this program point.
    defs: FxHashSet<Definition>,
}

impl ReachingDefs {
    /// Create an empty set of definitions.
    pub fn new() -> Self {
        Self { defs: FxHashSet::default() }
    }

    /// Create a set with a single definition.
    pub fn singleton(def: Definition) -> Self {
        let mut defs = FxHashSet::default();
        defs.insert(def);
        Self { defs }
    }

    /// Create a set from multiple definitions.
    pub fn from_definitions(defs: impl IntoIterator<Item = Definition>) -> Self {
        Self { defs: defs.into_iter().collect() }
    }

    /// Get all definitions.
    pub fn defs(&self) -> &FxHashSet<Definition> {
        &self.defs
    }

    /// Get all definitions for a specific variable (case-insensitive).
    pub fn defs_for_var(&self, var_name: &str) -> impl Iterator<Item = &Definition> {
        let normalized = var_name.to_lowercase();
        self.defs.iter().filter(move |def| def.var_name == normalized.as_str())
    }

    /// Check if any definition exists for a variable.
    pub fn has_def_for_var(&self, var_name: &str) -> bool {
        self.defs_for_var(var_name).next().is_some()
    }

    /// Add a definition to the set.
    pub fn insert(&mut self, def: Definition) {
        self.defs.insert(def);
    }

    /// Remove all definitions for a variable (kill operation).
    pub fn kill(&mut self, var_name: &str) {
        let normalized = var_name.to_lowercase();
        self.defs.retain(|def| def.var_name != normalized.as_str());
    }

    /// Gen-kill: kill old definitions, then add new definition.
    pub fn gen_kill(&mut self, var_name: &str, new_def: Definition) {
        self.kill(var_name);
        self.insert(new_def);
    }

    /// Number of definitions in the set.
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    /// Check if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

impl Default for ReachingDefs {
    fn default() -> Self {
        Self::new()
    }
}

impl Lattice for ReachingDefs {
    /// Bottom element: empty set (no definitions).
    fn bottom() -> Self {
        Self::new()
    }

    /// Join: set union (definition reaches if it reaches from ANY predecessor).
    fn join(&self, other: &Self) -> Self {
        let mut defs = self.defs.clone();
        defs.extend(other.defs.iter().cloned());
        Self { defs }
    }

    /// Check if self is more informative than other (self ⊆ other).
    fn is_more_informative_than(&self, other: &Self) -> bool {
        self.defs.is_subset(&other.defs)
    }
}

/// Transfer function for reaching definitions.
///
/// Applies gen-kill logic for each statement:
/// - **Gen**: Create new definitions for assignments, var decls, loop variables
/// - **Kill**: Remove old definitions for the same variable
pub struct ReachingDefsTransfer;

impl ReachingDefsTransfer {
    /// Extract variable name from an expression (for assignment targets).
    ///
    /// Handles:
    /// - Simple variables: `x = 5` → "x"
    /// - Field access: `obj.field = 5` → "obj.field"
    /// - Index access: `arr[i] = 5` → "arr"
    ///
    /// Returns None for complex expressions.
    fn extract_var_name(expr_id: hir_def::hir::ExprId, body: &Body) -> Option<SmolStr> {
        match body.expr(expr_id) {
            Expr::Path(name) => Some(SmolStr::new(name.as_str())),

            Expr::Field { base, field } => {
                // For field access, track the full path (obj.field)
                let base_name = Self::extract_var_name(*base, body)?;
                Some(SmolStr::new(format!("{}.{}", base_name, field.as_str())))
            }

            Expr::Index { base, .. } => {
                // For index access, track the base variable (arr[i] → arr)
                Self::extract_var_name(*base, body)
            }

            _ => None,
        }
    }
}

/// Result of reaching definitions analysis for a single method.
///
/// Provides high-level API for querying definitions that reach program points.
/// Does not store CFG to maintain Send+Sync bounds.
///
/// Phase 6.5: Added PartialEq, Eq for Salsa query compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachingDefsResult {
    /// IN sets for each block (definitions reaching block entry).
    block_in: rustc_hash::FxHashMap<petgraph::graph::NodeIndex, ReachingDefs>,

    /// OUT sets for each block (definitions reaching block exit).
    block_out: rustc_hash::FxHashMap<petgraph::graph::NodeIndex, ReachingDefs>,

    /// Reverse mapping: StmtId → BasicBlock that contains it.
    /// Allows fast lookup of which block a statement belongs to.
    stmt_to_block: rustc_hash::FxHashMap<hir_def::hir::StmtId, petgraph::graph::NodeIndex>,

    /// Statements in each block (for intra-block analysis).
    /// Stored separately to avoid holding non-Send CFG reference.
    block_stmts: rustc_hash::FxHashMap<petgraph::graph::NodeIndex, Vec<la_arena::RawIdx>>,

    /// HIR body (cloned for convenience).
    body: Body,
}

impl ReachingDefsResult {
    /// Create a new result from dataflow analysis.
    pub fn new(dataflow: crate::DataflowResult<ReachingDefs>) -> Self {
        use cfg::CfgVertex;

        // Build reverse mapping: stmt_id → block and extract statement lists
        let mut stmt_to_block = rustc_hash::FxHashMap::default();
        let mut block_stmts = rustc_hash::FxHashMap::default();

        for (block_idx, vertex) in dataflow.cfg().vertices() {
            if let CfgVertex::BasicBlock(basic_block) = vertex {
                // Store the statement list for this block
                let stmts: Vec<la_arena::RawIdx> =
                    basic_block.statements().iter().map(|stmt_id| stmt_id.into_raw()).collect();

                block_stmts.insert(block_idx, stmts);

                // Build reverse mapping
                for &stmt_id in basic_block.statements() {
                    stmt_to_block.insert(stmt_id, block_idx);
                }
            }
        }

        // Extract IN/OUT sets (copying to avoid holding CFG reference)
        let mut block_in = rustc_hash::FxHashMap::default();
        let mut block_out = rustc_hash::FxHashMap::default();

        for (block_idx, in_state, out_state) in dataflow.blocks() {
            block_in.insert(block_idx, in_state.clone());
            block_out.insert(block_idx, out_state.clone());
        }

        Self { block_in, block_out, stmt_to_block, block_stmts, body: dataflow.body().clone() }
    }

    /// Get all definitions that reach the beginning of a statement.
    ///
    /// Returns None if the statement is not found in the CFG.
    pub fn defs_before_stmt(&self, stmt_id: hir_def::hir::StmtId) -> Option<&ReachingDefs> {
        let block_idx = self.stmt_to_block.get(&stmt_id)?;
        self.block_in.get(block_idx)
    }

    /// Get all definitions that reach the end of a statement.
    ///
    /// Returns None if the statement is not found in the CFG.
    pub fn defs_after_stmt(&self, stmt_id: hir_def::hir::StmtId) -> Option<&ReachingDefs> {
        let block_idx = self.stmt_to_block.get(&stmt_id)?;
        self.block_out.get(block_idx)
    }

    /// Get all definitions that reach a specific point within a statement's block.
    ///
    /// This performs intra-block analysis by applying transfer functions sequentially
    /// to all statements in the block before the target statement.
    ///
    /// This is more precise than `defs_before_stmt()` which only returns block_in.
    pub fn defs_up_to_stmt(&self, stmt_id: hir_def::hir::StmtId) -> Option<ReachingDefs> {
        let block_idx = self.stmt_to_block.get(&stmt_id)?;
        let stmt_list = self.block_stmts.get(block_idx)?;

        // Start with IN set for this block
        let mut state = self.block_in.get(block_idx)?.clone();

        // Apply transfer function to all statements before target in this block
        for &hir_stmt_raw in stmt_list {
            let hir_stmt_id = hir_def::hir::StmtId::from_raw(hir_stmt_raw);

            // Stop before the target statement
            if hir_stmt_id == stmt_id {
                break;
            }

            // Apply transfer function
            let transfer = ReachingDefsTransfer;
            state = transfer.transfer_stmt(hir_stmt_raw, &state, &self.body);
        }

        Some(state)
    }

    /// Get all definitions for a variable that reach a statement.
    ///
    /// This is the main API for diagnostics: given a variable usage at stmt_id,
    /// find all places where that variable was defined.
    ///
    /// Uses intra-block analysis to account for definitions in the same block.
    pub fn defs_for_var_at_stmt(
        &self,
        var_name: &str,
        stmt_id: hir_def::hir::StmtId,
    ) -> Option<Vec<Definition>> {
        let reaching = self.defs_up_to_stmt(stmt_id)?;
        Some(reaching.defs_for_var(var_name).cloned().collect())
    }

    /// Check if a variable has any definition reaching a statement.
    ///
    /// Useful for uninitialized variable detection.
    pub fn var_is_defined_at_stmt(&self, var_name: &str, stmt_id: hir_def::hir::StmtId) -> bool {
        self.defs_before_stmt(stmt_id)
            .map(|reaching| reaching.has_def_for_var(var_name))
            .unwrap_or(false)
    }

    /// Get the HIR body.
    pub fn body(&self) -> &Body {
        &self.body
    }

    /// Get all definitions reaching a specific basic block.
    pub fn defs_at_block(&self, block_idx: petgraph::graph::NodeIndex) -> Option<&ReachingDefs> {
        self.block_in.get(&block_idx)
    }
}

impl Transfer<ReachingDefs> for ReachingDefsTransfer {
    fn transfer_stmt(&self, stmt_id: RawIdx, state: &ReachingDefs, body: &Body) -> ReachingDefs {
        use hir_def::hir::StmtId;

        // Convert RawIdx back to StmtId
        let stmt_id = StmtId::from_raw(stmt_id);

        let mut new_state = state.clone();

        match body.stmt(stmt_id) {
            // Assignment: kill old definitions for target, gen new definition
            Stmt::Assign { target, .. } => {
                if let Some(var_name) = Self::extract_var_name(*target, body) {
                    let def = Definition::assignment(var_name.clone(), stmt_id.into_raw());
                    new_state.gen_kill(&var_name, def);
                }
            }

            // Variable declaration: gen definition for each declared variable
            Stmt::VarDecl { bindings } => {
                for &binding_id in bindings.iter() {
                    let binding = body.binding(binding_id);
                    let def = Definition::var_decl(&binding.name, binding_id);
                    new_state.insert(def);
                }
            }

            // For loop: gen definition for loop variable
            Stmt::For { var, .. } => {
                let binding = body.binding(*var);
                let def = Definition::for_loop(&binding.name, *var);
                new_state.gen_kill(binding.name.as_str(), def);
            }

            // ForEach loop: gen definition for loop variable
            Stmt::ForEach { var, .. } => {
                let binding = body.binding(*var);
                let def = Definition::for_each_loop(&binding.name, *var);
                new_state.gen_kill(binding.name.as_str(), def);
            }

            // Other statements don't create definitions
            _ => {}
        }

        new_state
    }
}

// ============================================================================
// Module-level collection for batch processing
// ============================================================================

/// Collection of reaching definitions results for all methods in a module.
///
/// Built once per module and cached by Salsa. This enables batch processing
/// where all reaching definitions analyses are performed in one pass with
/// shared CFG construction.
///
/// # Usage
///
/// ```ignore
/// // In Salsa query:
/// let module_reaching_defs = db.module_reaching_definitions(module_id);
/// let result = module_reaching_defs.get(local_method_id)?;
/// ```
///
/// # Performance
///
/// On doc3 project (96,317 methods):
/// - Per-method: ~100+ seconds (Salsa overhead + duplicate CFG construction)
/// - Module-level: ~5-20 seconds (shared CFG + batch processing)
/// - Expected speedup: 3-5x
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleReachingDefs {
    results: rustc_hash::FxHashMap<u32, std::sync::Arc<ReachingDefsResult>>,
}

impl ModuleReachingDefs {
    /// Create a new collection of reaching definitions results.
    pub fn new(results: rustc_hash::FxHashMap<u32, std::sync::Arc<ReachingDefsResult>>) -> Self {
        Self { results }
    }

    /// Get reaching definitions result for a specific method.
    ///
    /// Returns `None` if analysis failed for this method (e.g., didn't converge).
    pub fn get(&self, local_id: u32) -> Option<&std::sync::Arc<ReachingDefsResult>> {
        self.results.get(&local_id)
    }

    /// Get the number of methods analyzed.
    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// Check if this collection is empty.
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definition_creation() {
        let name = Name::new("Переменная");
        let binding_id = BindingId::from_raw(la_arena::RawIdx::from_u32(0));

        let param_def = Definition::parameter(&name, binding_id);
        assert_eq!(param_def.var_name, "переменная"); // lowercase
        assert!(matches!(param_def.def_site, DefSite::Parameter(_)));

        let var_def = Definition::var_decl(&name, binding_id);
        assert!(matches!(var_def.def_site, DefSite::VarDecl(_)));
    }

    #[test]
    fn test_reaching_defs_lattice_bottom() {
        let bottom = ReachingDefs::bottom();
        assert!(bottom.is_empty());
    }

    #[test]
    fn test_reaching_defs_lattice_join() {
        let name = Name::new("x");
        let binding_id = BindingId::from_raw(la_arena::RawIdx::from_u32(0));

        let def1 = Definition::parameter(&name, binding_id);
        let def2 = Definition::var_decl(&name, binding_id);

        let set1 = ReachingDefs::singleton(def1.clone());
        let set2 = ReachingDefs::singleton(def2.clone());

        let joined = set1.join(&set2);
        assert_eq!(joined.len(), 2);
        assert!(joined.defs().contains(&def1));
        assert!(joined.defs().contains(&def2));
    }

    #[test]
    fn test_reaching_defs_lattice_join_idempotent() {
        let name = Name::new("x");
        let binding_id = BindingId::from_raw(la_arena::RawIdx::from_u32(0));
        let def = Definition::parameter(&name, binding_id);

        let set = ReachingDefs::singleton(def);
        let joined = set.join(&set);
        assert_eq!(joined, set);
    }

    #[test]
    fn test_reaching_defs_lattice_join_commutative() {
        let name = Name::new("x");
        let binding_id = BindingId::from_raw(la_arena::RawIdx::from_u32(0));

        let def1 = Definition::parameter(&name, binding_id);
        let def2 = Definition::var_decl(&name, binding_id);

        let set1 = ReachingDefs::singleton(def1);
        let set2 = ReachingDefs::singleton(def2);

        assert_eq!(set1.join(&set2), set2.join(&set1));
    }

    #[test]
    fn test_reaching_defs_lattice_bottom_identity() {
        let name = Name::new("x");
        let binding_id = BindingId::from_raw(la_arena::RawIdx::from_u32(0));
        let def = Definition::parameter(&name, binding_id);

        let set = ReachingDefs::singleton(def);
        let bottom = ReachingDefs::bottom();

        assert_eq!(set.join(&bottom), set);
        assert_eq!(bottom.join(&set), set);
    }

    #[test]
    fn test_gen_kill() {
        let name = Name::new("x");
        let binding_id1 = BindingId::from_raw(la_arena::RawIdx::from_u32(0));
        let binding_id2 = BindingId::from_raw(la_arena::RawIdx::from_u32(1));

        let def1 = Definition::parameter(&name, binding_id1);
        let def2 = Definition::var_decl(&name, binding_id2);

        let mut state = ReachingDefs::singleton(def1.clone());
        assert_eq!(state.len(), 1);

        // Gen-kill: replace def1 with def2
        state.gen_kill("x", def2.clone());
        assert_eq!(state.len(), 1);
        assert!(!state.defs().contains(&def1));
        assert!(state.defs().contains(&def2));
    }

    #[test]
    fn test_case_insensitive() {
        let name_upper = Name::new("ПЕРЕМЕННАЯ");
        let name_lower = Name::new("переменная");
        let binding_id = BindingId::from_raw(la_arena::RawIdx::from_u32(0));

        let def1 = Definition::parameter(&name_upper, binding_id);
        let def2 = Definition::var_decl(&name_lower, binding_id);

        // Both should normalize to same name
        assert_eq!(def1.var_name, def2.var_name);

        let mut state = ReachingDefs::singleton(def1);
        state.gen_kill("ПЕРЕМЕННАЯ", def2.clone());

        // Should have killed def1 and added def2
        assert_eq!(state.len(), 1);
        assert!(state.defs().contains(&def2));
    }

    #[test]
    fn test_defs_for_var() {
        let name_x = Name::new("x");
        let name_y = Name::new("y");
        let binding_id = BindingId::from_raw(la_arena::RawIdx::from_u32(0));

        let def_x = Definition::parameter(&name_x, binding_id);
        let def_y = Definition::parameter(&name_y, binding_id);

        let state = ReachingDefs::from_definitions([def_x.clone(), def_y.clone()]);

        let x_defs: Vec<_> = state.defs_for_var("x").collect();
        assert_eq!(x_defs.len(), 1);
        assert_eq!(x_defs[0], &def_x);

        assert!(state.has_def_for_var("x"));
        assert!(state.has_def_for_var("y"));
        assert!(!state.has_def_for_var("z"));
    }
}
