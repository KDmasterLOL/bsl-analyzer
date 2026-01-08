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
mod magic_number;
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
    /// Note: This set is modified by `unmark_var_used()` for assignment targets.
    pub(crate) used_vars: FxHashSet<String>,

    /// Read variable names (lowercase) - never cleared.
    /// Tracks variables that were ever read (not just written).
    /// Used for UnusedVariable diagnostic to handle cases where variable is
    /// read in loop condition but later assigned in loop body.
    pub(crate) read_vars: FxHashSet<String>,

    /// Known external variable names (lowercase) - module variables, etc.
    /// These should not be registered as implicit local variables.
    pub(crate) known_externals: FxHashSet<String>,

    /// Parameter names (lowercase).
    /// Parameters should not trigger "unused variable" even if only assigned.
    pub(crate) param_names: FxHashSet<String>,

    /// By-reference parameter names (lowercase) - parameters without "Знач" keyword.
    /// Used for FunctionOutParameter diagnostic.
    pub(crate) by_ref_param_names: FxHashSet<String>,

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

    /// Whether current method is client-only (&НаКлиенте annotation ONLY).
    /// Used for ExecuteExternalCode diagnostic - Execute/Eval is allowed only in client-only context.
    pub(crate) is_client_only: bool,

    /// Whether current method has "БезКонтекста" (NoContext) annotation.
    /// Used for FormDataToValue diagnostic - call is allowed in БезКонтекста methods.
    /// Checks for @НаСервереБезКонтекста or @НаКлиентеНаСервереБезКонтекста.
    pub(crate) has_no_context_annotation: bool,
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
            read_vars: FxHashSet::default(),
            known_externals,
            param_names: FxHashSet::default(),
            by_ref_param_names: FxHashSet::default(),
            pending_sdbl: Vec::new(),
            loop_depth: 0,
            query_vars: FxHashMap::default(),
            foreach_collections: Vec::new(),
            is_client_only: false, // Will be set in lower_method_with_externals
            has_no_context_annotation: false, // Will be set in lower_method_with_externals
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
        let lowercase = name.to_lowercase();
        self.used_vars.insert(lowercase.clone());
        self.read_vars.insert(lowercase);
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

/// Check if method has ONLY &НаКлиенте annotation.
///
/// For ExecuteExternalCode diagnostic - Execute/Eval is only allowed in client-only context.
fn is_client_only_method(method_node: &SyntaxNode) -> bool {
    let annotations: Vec<_> = method_node
        .children()
        .filter(|child| {
            matches!(child.kind(), SyntaxKind::ANNOTATION | SyntaxKind::COMPILER_DIRECTIVE)
        })
        .collect();

    // Must have exactly ONE annotation
    if annotations.len() != 1 {
        return false;
    }

    // Check if it's &НаКлиенте / &AtClient
    annotations[0]
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|token| token.kind() == SyntaxKind::ANN_AT_CLIENT)
}

/// Check if method name collides with platform 8.3.12 global context methods.
///
/// Platform 8.3.12 added bitwise operation methods to global context.
/// User-defined methods with these names will conflict.
/// List matches GlobalContextMethodCollision8312Diagnostic.java.
fn is_global_context_collision_8312(name: &str) -> bool {
    const COLLISION_METHODS: &[&str] = &[
        // Russian variants
        "проверитьбит",
        "проверитьпобитовоймаске",
        "установитьбит",
        "побитовоеи",
        "побитовоеили",
        "побитовоене",
        "побитовоеине",
        "побитовоеисключительноеили",
        "побитовыйсдвигвлево",
        "побитовыйсдвигвправо",
        // English variants
        "checkbit",
        "checkbybitmask",
        "setbit",
        "bitwiseand",
        "bitwiseor",
        "bitwisenot",
        "bitwiseandnot",
        "bitwisexor",
        "bitwiseshiftleft",
        "bitwiseshiftright",
    ];

    COLLISION_METHODS.contains(&name.to_lowercase().as_str())
}

/// Check if method has "БезКонтекста" (NoContext) annotation.
///
/// For FormDataToValue diagnostic - call is allowed in БезКонтекста methods.
/// Returns true if method has:
/// - @НаСервереБезКонтекста / @AtServerNoContext
/// - @НаКлиентеНаСервереБезКонтекста / @AtClientAtServerNoContext
fn has_no_context_annotation_method(method_node: &SyntaxNode) -> bool {
    let annotations: Vec<_> = method_node
        .children()
        .filter(|child| {
            matches!(child.kind(), SyntaxKind::ANNOTATION | SyntaxKind::COMPILER_DIRECTIVE)
        })
        .collect();

    // Check any annotation for БезКонтекста tokens
    annotations.iter().any(|ann| {
        ann.descendants_with_tokens().filter_map(|el| el.into_token()).any(|token| {
            matches!(
                token.kind(),
                SyntaxKind::ANN_AT_SERVER_NO_CONTEXT
                    | SyntaxKind::ANN_AT_CLIENT_AT_SERVER_NO_CONTEXT
            )
        })
    })
}

/// Check if function always returns the same primitive value.
///
/// For FunctionReturnsSamePrimitive diagnostic.
/// Skips "Attachable" methods (names starting with "Подключаемый_" or "Attachable_").
fn check_function_returns_same_primitive(ctx: &mut LoweringCtx, method_node: &SyntaxNode) {
    use crate::hir::{Expr, Literal, Stmt};

    // Skip attachable methods
    if let Some(name_token) = method_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)
    {
        let name = name_token.text().to_lowercase();
        if name.starts_with("подключаемый_") || name.starts_with("attachable_") {
            return;
        }
    }

    // Collect all return statements with their literal values
    let mut return_literals: Vec<&Literal> = Vec::new();

    for (stmt_id, _) in ctx.body.stmts.iter() {
        if let Stmt::Return { value: Some(expr_id) } = &ctx.body.stmts[stmt_id] {
            // Check if expression is a literal (primitive)
            if let Expr::Literal(lit) = &ctx.body.exprs[*expr_id] {
                return_literals.push(lit);
            } else {
                // Non-primitive return found (variable, function call, etc.)
                return;
            }
        }
    }

    // Need at least 2 return statements
    if return_literals.len() < 2 {
        return;
    }

    // Compare all literals - check if all are the same
    let first = return_literals[0];
    let all_same = return_literals[1..].iter().all(|lit| literals_equal(first, lit));

    if all_same {
        // Emit diagnostic on function name
        if let Some(name_token) = method_node
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::IDENT)
        {
            ctx.emit(BodyDiagnostic::FunctionReturnsSamePrimitive {
                range: name_token.text_range(),
            });
        }
    }
}

/// Compare two literals for equality (case-insensitive for strings by default).
///
/// Matches Java behavior: strings are compared case-insensitively unless configured otherwise.
/// TODO: add caseSensitiveForString parameter when needed.
fn literals_equal(a: &crate::hir::Literal, b: &crate::hir::Literal) -> bool {
    use crate::hir::Literal;

    match (a, b) {
        (Literal::Number(a), Literal::Number(b)) => (a - b).abs() < f64::EPSILON,
        (Literal::String(a), Literal::String(b)) => {
            // Case-insensitive comparison (default behavior)
            a.to_uppercase() == b.to_uppercase()
        }
        (Literal::Date(a), Literal::Date(b)) => a == b,
        (Literal::Bool(a), Literal::Bool(b)) => a == b,
        (Literal::Undefined, Literal::Undefined) => true,
        (Literal::Null, Literal::Null) => true,
        _ => false,
    }
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

    // Check if method is client-only (&НаКлиенте annotation ONLY)
    ctx.is_client_only = is_client_only_method(method_node);

    // Check if method has "БезКонтекста" (NoContext) annotation
    ctx.has_no_context_annotation = has_no_context_annotation_method(method_node);

    // Check for FunctionNameStartsWithGet diagnostic
    if is_function {
        // Get function name
        if let Some(name_token) = method_node
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::IDENT)
        {
            let name_text = name_token.text();
            // Check if name starts with "Получить" (case-insensitive)
            // Only Russian "Получить" is checked, not English "Get"
            if name_text.to_lowercase().starts_with("получить") {
                ctx.emit(BodyDiagnostic::FunctionNameStartsWithGet {
                    name: name_text.to_string(),
                    range: name_token.text_range(),
                });
            }

            // Check for GlobalContextMethodCollision8312 diagnostic
            // Applies to both functions and procedures
            if is_global_context_collision_8312(name_text) {
                ctx.emit(BodyDiagnostic::GlobalContextMethodCollision8312 {
                    method_name: name_text.to_string(),
                    range: name_token.text_range(),
                });
            }
        }
    } else {
        // For procedures, only check GlobalContextMethodCollision8312
        if let Some(name_token) = method_node
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|tok| tok.kind() == SyntaxKind::IDENT)
        {
            let name_text = name_token.text();
            if is_global_context_collision_8312(name_text) {
                ctx.emit(BodyDiagnostic::GlobalContextMethodCollision8312 {
                    method_name: name_text.to_string(),
                    range: name_token.text_range(),
                });
            }
        }
    }

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

        // NOTE: Empty function/procedure bodies are NOT checked by EmptyCodeBlock diagnostic.
        // They are handled by a separate diagnostic (if needed).

        // Check for code after async calls
        diagnostics::check_code_after_async_call(&mut ctx, &stmt_list);
    }

    // Check for FunctionReturnsSamePrimitive
    if is_function {
        check_function_returns_same_primitive(&mut ctx, method_node);
    }

    // Check for unused local variables
    ctx.check_unused_variables();

    // Check for magic numbers using HIR
    magic_number::check_magic_numbers(&ctx.body, &ctx.source_map, &mut ctx.diagnostics);

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
