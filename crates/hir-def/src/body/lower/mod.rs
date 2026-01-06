//! AST to HIR lowering.
//!
//! This module converts AST method bodies into HIR representation.
//! Diagnostics are collected as a byproduct of lowering.
//!
//! ## Lowering strategy
//!
//! The lowering process walks the AST once and:
//! 1. Allocates HIR nodes in arenas
//! 2. Records source mappings for diagnostics
//! 3. Emits diagnostics when issues are detected
//!
//! This is more efficient than having separate diagnostic passes
//! because we traverse the AST only once.
//!
//! ## Module structure
//!
//! - `stmt` - Statement lowering
//! - `expr` - Expression lowering
//! - `preproc` - Preprocessor directive handling
//! - `control_flow` - Return path and unreachable code analysis
//! - `diagnostics` - Diagnostic helpers (async calls, transactions, deprecated methods)
//! - `utils` - Utility functions

mod control_flow;
mod diagnostics;
mod expr;
mod preproc;
mod stmt;
mod utils;

#[cfg(test)]
mod tests;

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::{SyntaxKind, SyntaxNode};
use text_size::TextRange;

use crate::body::{Body, BodyDiagnostic, BodySourceMap, LowerResult};
use crate::hir::{Binding, BindingId, Expr, ExprId, Stmt, StmtId};
use crate::Name;

pub use control_flow::check_missing_return_paths;

/// Lowering context.
///
/// Holds state during AST → HIR conversion.
pub(crate) struct LoweringCtx {
    pub(crate) body: Body,
    pub(crate) source_map: BodySourceMap,
    pub(crate) diagnostics: Vec<BodyDiagnostic>,
    /// Whether we're lowering a function (vs procedure).
    /// Used for diagnostics like FunctionShouldHaveReturn.
    #[allow(dead_code)] // Will be used in Phase 2 for return path analysis
    pub(crate) is_function: bool,

    /// Declared local variables: lowercase name -> (original name, declaration range)
    /// Used for UnusedVariable diagnostic.
    pub(crate) local_vars: FxHashMap<String, (Name, TextRange)>,

    /// Used variable names (lowercase).
    /// When a variable is referenced in an expression, its name is added here.
    pub(crate) used_vars: FxHashSet<String>,

    /// Known external variable names (lowercase) - module variables, etc.
    /// These should not be registered as implicit local variables.
    pub(crate) known_externals: FxHashSet<String>,

    /// Parameter names (lowercase).
    /// Parameters should not trigger "unused variable" even if only assigned.
    pub(crate) param_names: FxHashSet<String>,

    /// Pending SDBL queries (before ExprId allocation).
    pub(crate) pending_sdbl: Vec<(String, syntax::SdblQueryInfo)>,

    /// Loop nesting depth (0 = not in loop, 1+ = inside loop).
    /// Used for CreateQueryInCycle diagnostic.
    pub(crate) loop_depth: usize,

    /// Query-like variables: lowercase name -> VarType.
    /// Tracks Query, QueryBuilder, ReportBuilder variables for CreateQueryInCycle diagnostic.
    pub(crate) query_vars: FxHashMap<String, QueryVarType>,

    /// ForEach collection stack: (collection_expr_id, collection_text) tuples.
    /// Tracks the collection being iterated for DeletingCollectionItem diagnostic.
    /// Stack handles nested ForEach loops.
    pub(crate) foreach_collections: Vec<(ExprId, String)>,
}

/// Type of query-like variable for CreateQueryInCycle diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum QueryVarType {
    Query,
    QueryBuilder,
    ReportBuilder,
    Undefined,
}

impl LoweringCtx {
    /// Create a new lowering context.
    pub(crate) fn new(is_function: bool) -> Self {
        Self::new_with_externals(is_function, FxHashSet::default())
    }

    /// Create a new lowering context with known external variable names.
    pub(crate) fn new_with_externals(
        is_function: bool,
        known_externals: FxHashSet<String>,
    ) -> Self {
        Self {
            body: Body::new(),
            source_map: BodySourceMap::new(),
            diagnostics: Vec::new(),
            is_function,
            local_vars: FxHashMap::default(),
            used_vars: FxHashSet::default(),
            known_externals,
            param_names: FxHashSet::default(),
            pending_sdbl: Vec::new(),
            loop_depth: 0,
            query_vars: FxHashMap::default(),
            foreach_collections: Vec::new(),
        }
    }

    /// Register a parameter name.
    pub(crate) fn register_param(&mut self, name: &str) {
        self.param_names.insert(name.to_lowercase());
    }

    /// Register a local variable declaration.
    /// Called when processing VAR statements and loop variables.
    pub(crate) fn register_local_var(&mut self, name: Name, range: TextRange) {
        let key = name.as_str().to_lowercase();
        self.local_vars.insert(key, (name, range));
    }

    /// Mark a variable as used (read).
    /// Called when a variable is referenced in an expression.
    pub(crate) fn mark_var_used(&mut self, name: &str) {
        self.used_vars.insert(name.to_lowercase());
    }

    /// Unmark a variable as used.
    /// Called for assignment targets - they are written, not read.
    pub(crate) fn unmark_var_used(&mut self, name: &str) {
        self.used_vars.remove(&name.to_lowercase());
    }

    /// Emit diagnostics for unused local variables.
    pub(crate) fn check_unused_variables(&mut self) {
        for (key, (name, range)) in &self.local_vars {
            // Skip parameters - they're inputs and may be modified for output
            if self.param_names.contains(key) {
                continue;
            }
            if !self.used_vars.contains(key) {
                self.diagnostics
                    .push(BodyDiagnostic::UnusedVariable { name: name.to_string(), range: *range });
            }
        }
    }

    /// Check if a name is a known external (module variable).
    pub(crate) fn is_known_external(&self, name: &str) -> bool {
        self.known_externals.contains(&name.to_lowercase())
    }

    /// Get variables that were referenced but not locally declared.
    /// These are potential module-level variable references.
    pub(crate) fn referenced_externals(&self) -> FxHashSet<String> {
        self.used_vars.iter().filter(|name| !self.local_vars.contains_key(*name)).cloned().collect()
    }

    /// Allocate an expression and record its source range.
    pub(crate) fn alloc_expr(&mut self, expr: Expr, range: TextRange) -> ExprId {
        let id = self.body.exprs.alloc(expr);
        self.source_map.record_expr(id, range);
        id
    }

    /// Allocate a statement and record its source range.
    pub(crate) fn alloc_stmt(&mut self, stmt: Stmt, range: TextRange) -> StmtId {
        let id = self.body.stmts.alloc(stmt);
        self.source_map.record_stmt(id, range);
        id
    }

    /// Allocate a binding and record its source range.
    pub(crate) fn alloc_binding(&mut self, binding: Binding, range: TextRange) -> BindingId {
        let id = self.body.bindings.alloc(binding);
        self.source_map.record_binding(id, range);
        id
    }

    /// Allocate a missing expression (for error recovery).
    pub(crate) fn missing_expr(&mut self) -> ExprId {
        self.body.exprs.alloc(Expr::Missing)
    }

    /// Enter a loop (increment loop depth).
    pub(crate) fn enter_loop(&mut self) {
        self.loop_depth += 1;
    }

    /// Leave a loop (decrement loop depth).
    pub(crate) fn leave_loop(&mut self) {
        if self.loop_depth > 0 {
            self.loop_depth -= 1;
        }
    }

    /// Check if currently inside a loop.
    pub(crate) fn in_loop(&self) -> bool {
        self.loop_depth > 0
    }

    /// Register a query-like variable (Query, QueryBuilder, ReportBuilder).
    pub(crate) fn register_query_var(&mut self, name: String, var_type: QueryVarType) {
        self.query_vars.insert(name.to_lowercase(), var_type);
    }

    /// Get query variable type by name (case-insensitive).
    pub(crate) fn get_query_var_type(&self, name: &str) -> Option<QueryVarType> {
        self.query_vars.get(&name.to_lowercase()).copied()
    }

    /// Check if a variable is a query-like type.
    pub(crate) fn is_query_var(&self, name: &str) -> bool {
        matches!(
            self.get_query_var_type(name),
            Some(QueryVarType::Query | QueryVarType::QueryBuilder | QueryVarType::ReportBuilder)
        )
    }

    /// Enter a ForEach loop, tracking the collection being iterated.
    pub(crate) fn enter_foreach(&mut self, collection_expr: ExprId, collection_text: String) {
        self.foreach_collections.push((collection_expr, collection_text));
    }

    /// Leave a ForEach loop.
    pub(crate) fn leave_foreach(&mut self) {
        self.foreach_collections.pop();
    }

    /// Check if an expression matches any active ForEach collection (case-insensitive).
    /// Returns collection text for diagnostic message if matched.
    pub(crate) fn matches_foreach_collection(&self, expr: ExprId) -> Option<&str> {
        use crate::body::lower::expr::exprs_are_equal;

        for (collection_expr, collection_text) in self.foreach_collections.iter().rev() {
            if exprs_are_equal(&self.body, *collection_expr, expr) {
                return Some(collection_text.as_str());
            }
        }
        None
    }

    /// Emit a diagnostic.
    pub(crate) fn emit(&mut self, diagnostic: BodyDiagnostic) {
        self.diagnostics.push(diagnostic);
    }
}

/// Lower a method AST node to HIR.
pub fn lower_method(method_node: &SyntaxNode, is_function: bool) -> LowerResult {
    lower_method_with_externals(method_node, is_function, FxHashSet::default())
}

/// Lower a method AST node to HIR with known external variable names.
///
/// External variable names (like module-level variables) are passed to avoid
/// registering them as implicit local variables.
pub fn lower_method_with_externals(
    method_node: &SyntaxNode,
    is_function: bool,
    known_externals: FxHashSet<String>,
) -> LowerResult {
    let mut ctx = LoweringCtx::new_with_externals(is_function, known_externals);

    // Lower parameters
    if let Some(param_list) = method_node.children().find(|n| n.kind() == SyntaxKind::PARAM_LIST) {
        let params = stmt::lower_params(&mut ctx, &param_list);
        ctx.body.params = params.into_boxed_slice();
    }

    // Lower body statements
    if let Some(stmt_list) = method_node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST) {
        let stmts = stmt::lower_stmt_list(&mut ctx, &stmt_list);
        ctx.body.body_stmts = stmts.into_boxed_slice();

        let has_return = control_flow::has_return_statement(&stmt_list);

        // Check for FunctionShouldHaveReturn (no return statement at all)
        if is_function && !has_return {
            // Get function name range for diagnostic
            let name_range = method_node
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| tok.kind() == SyntaxKind::IDENT)
                .map(|tok| tok.text_range())
                .unwrap_or_else(|| method_node.text_range());

            ctx.emit(BodyDiagnostic::FunctionShouldHaveReturn { range: name_range });
        }

        // Check for MissingReturn (some paths don't return)
        // Only check if function has at least one return (otherwise FunctionShouldHaveReturn fires)
        if is_function && has_return && control_flow::check_missing_return_paths(&stmt_list) {
            // Get function name range for diagnostic
            let name_range = method_node
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| tok.kind() == SyntaxKind::IDENT)
                .map(|tok| tok.text_range())
                .unwrap_or_else(|| method_node.text_range());

            ctx.emit(BodyDiagnostic::MissingReturn { range: name_range });
        }

        // Check for empty body
        if stmt_list.children().count() == 0 {
            ctx.emit(BodyDiagnostic::EmptyCodeBlock { range: stmt_list.text_range() });
        }

        // Check for code after async calls
        diagnostics::check_code_after_async_call(&mut ctx, &stmt_list);
    }

    // Check for unused local variables
    ctx.check_unused_variables();

    // Collect referenced externals before consuming ctx
    let referenced_externals = ctx.referenced_externals();

    LowerResult {
        body: ctx.body,
        source_map: ctx.source_map,
        diagnostics: ctx.diagnostics,
        referenced_externals,
    }
}

/// Lower module-level code (statements outside procedures/functions).
///
/// This handles initialization code that runs when the module is loaded.
/// Also detects unreachable code at the module level.
pub fn lower_module_code(root: &SyntaxNode) -> LowerResult {
    let mut ctx = LoweringCtx::new(false);

    let mut stmts = Vec::new();
    let mut unreachable_start: Option<TextRange> = None;
    let mut unreachable_end: Option<TextRange> = None;

    for node in root.children() {
        // Handle preprocessor directives at module level
        if node.kind() == SyntaxKind::PRE_IF_DIR {
            if unreachable_start.is_some() {
                unreachable_end = Some(node.text_range());
            }
            preproc::process_preproc_if(&mut ctx, &node);
            continue;
        }
        if node.kind() == SyntaxKind::PRE_REGION_DIR {
            if unreachable_start.is_some() {
                unreachable_end = Some(node.text_range());
            }
            preproc::process_preproc_region(&mut ctx, &node);
            continue;
        }

        // Skip non-statement nodes (procedures, functions, var declarations, etc.)
        if !control_flow::is_statement_node(&node) {
            continue;
        }

        // Skip VAR_DEF - module-level Перем declarations are tracked separately
        // in lower_module_bodies via module_vars. Processing them here would cause
        // duplicate unused variable diagnostics.
        if node.kind() == SyntaxKind::VAR_DEF {
            continue;
        }

        // If we're in unreachable mode, extend the range
        if unreachable_start.is_some() {
            unreachable_end = Some(node.text_range());
            if let Some(stmt_id) = stmt::lower_stmt(&mut ctx, &node) {
                stmts.push(stmt_id);
            }
            continue;
        }

        // Lower the statement
        if let Some(stmt_id) = stmt::lower_stmt(&mut ctx, &node) {
            stmts.push(stmt_id);

            // Check if this statement is a control flow that makes subsequent code unreachable
            if control_flow::is_control_flow_terminator(&node)
                || (node.kind() == SyntaxKind::IF_STMT
                    && control_flow::if_all_branches_terminate(&node))
            {
                unreachable_start = Some(node.text_range());
            }
        }
    }

    // Emit unreachable code diagnostic for module-level code
    if let (Some(start), Some(end)) = (unreachable_start, unreachable_end) {
        if let Some(first_unreachable) = control_flow::find_first_unreachable_at_root(root, start) {
            let range = TextRange::new(first_unreachable.start(), end.end());
            ctx.emit(BodyDiagnostic::UnreachableCode { range });
        }
    }

    ctx.body.body_stmts = stmts.into_boxed_slice();

    // Check for unused local variables (implicit module-level variables)
    ctx.check_unused_variables();

    let referenced_externals = ctx.referenced_externals();

    LowerResult {
        body: ctx.body,
        source_map: ctx.source_map,
        diagnostics: ctx.diagnostics,
        referenced_externals,
    }
}
