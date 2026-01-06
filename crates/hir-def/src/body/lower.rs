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

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::{SyntaxKind, SyntaxNode};
use text_size::TextRange;

use crate::body::{Body, BodyDiagnostic, BodySourceMap, LowerResult};
use crate::hir::{BinaryOp, Binding, BindingId, Expr, ExprId, Literal, Stmt, StmtId, UnaryOp};
use crate::Name;

/// Lowering context.
///
/// Holds state during AST → HIR conversion.
pub struct LoweringCtx {
    body: Body,
    source_map: BodySourceMap,
    diagnostics: Vec<BodyDiagnostic>,
    /// Whether we're lowering a function (vs procedure).
    /// Used for diagnostics like FunctionShouldHaveReturn.
    #[allow(dead_code)] // Will be used in Phase 2 for return path analysis
    is_function: bool,

    /// Declared local variables: lowercase name -> (original name, declaration range)
    /// Used for UnusedVariable diagnostic.
    local_vars: FxHashMap<String, (Name, TextRange)>,

    /// Used variable names (lowercase).
    /// When a variable is referenced in an expression, its name is added here.
    used_vars: FxHashSet<String>,

    /// Known external variable names (lowercase) - module variables, etc.
    /// These should not be registered as implicit local variables.
    known_externals: FxHashSet<String>,

    /// Parameter names (lowercase).
    /// Parameters should not trigger "unused variable" even if only assigned.
    param_names: FxHashSet<String>,

    /// Pending SDBL queries (before ExprId allocation).
    pending_sdbl: Vec<(String, syntax::SdblQueryInfo)>,
}

impl LoweringCtx {
    /// Create a new lowering context.
    pub fn new(is_function: bool) -> Self {
        Self::new_with_externals(is_function, FxHashSet::default())
    }

    /// Create a new lowering context with known external variable names.
    pub fn new_with_externals(is_function: bool, known_externals: FxHashSet<String>) -> Self {
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
        }
    }

    /// Register a parameter name.
    fn register_param(&mut self, name: &str) {
        self.param_names.insert(name.to_lowercase());
    }

    /// Register a local variable declaration.
    /// Called when processing VAR statements and loop variables.
    fn register_local_var(&mut self, name: Name, range: TextRange) {
        let key = name.as_str().to_lowercase();
        self.local_vars.insert(key, (name, range));
    }

    /// Mark a variable as used (read).
    /// Called when a variable is referenced in an expression.
    fn mark_var_used(&mut self, name: &str) {
        self.used_vars.insert(name.to_lowercase());
    }

    /// Unmark a variable as used.
    /// Called for assignment targets - they are written, not read.
    fn unmark_var_used(&mut self, name: &str) {
        self.used_vars.remove(&name.to_lowercase());
    }

    /// Emit diagnostics for unused local variables.
    fn check_unused_variables(&mut self) {
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
    fn is_known_external(&self, name: &str) -> bool {
        self.known_externals.contains(&name.to_lowercase())
    }

    /// Get variables that were referenced but not locally declared.
    /// These are potential module-level variable references.
    fn referenced_externals(&self) -> FxHashSet<String> {
        self.used_vars.iter().filter(|name| !self.local_vars.contains_key(*name)).cloned().collect()
    }

    /// Allocate an expression and record its source range.
    fn alloc_expr(&mut self, expr: Expr, range: TextRange) -> ExprId {
        let id = self.body.exprs.alloc(expr);
        self.source_map.record_expr(id, range);
        id
    }

    /// Allocate a statement and record its source range.
    fn alloc_stmt(&mut self, stmt: Stmt, range: TextRange) -> StmtId {
        let id = self.body.stmts.alloc(stmt);
        self.source_map.record_stmt(id, range);
        id
    }

    /// Allocate a binding and record its source range.
    fn alloc_binding(&mut self, binding: Binding, range: TextRange) -> BindingId {
        let id = self.body.bindings.alloc(binding);
        self.source_map.record_binding(id, range);
        id
    }

    /// Allocate a missing expression (for error recovery).
    fn missing_expr(&mut self) -> ExprId {
        self.body.exprs.alloc(Expr::Missing)
    }

    /// Emit a diagnostic.
    fn emit(&mut self, diagnostic: BodyDiagnostic) {
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
        let params = lower_params(&mut ctx, &param_list);
        ctx.body.params = params.into_boxed_slice();
    }

    // Lower body statements
    if let Some(stmt_list) = method_node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST) {
        let stmts = lower_stmt_list(&mut ctx, &stmt_list);
        ctx.body.body_stmts = stmts.into_boxed_slice();

        let has_return = has_return_statement(&stmt_list);

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
        if is_function && has_return && check_missing_return_paths(&stmt_list) {
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
            process_preproc_if(&mut ctx, &node);
            continue;
        }
        if node.kind() == SyntaxKind::PRE_REGION_DIR {
            if unreachable_start.is_some() {
                unreachable_end = Some(node.text_range());
            }
            process_preproc_region(&mut ctx, &node);
            continue;
        }

        // Skip non-statement nodes (procedures, functions, var declarations, etc.)
        if !is_statement_node(&node) {
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
            if let Some(stmt_id) = lower_stmt(&mut ctx, &node) {
                stmts.push(stmt_id);
            }
            continue;
        }

        // Lower the statement
        if let Some(stmt_id) = lower_stmt(&mut ctx, &node) {
            stmts.push(stmt_id);

            // Check if this statement is a control flow that makes subsequent code unreachable
            if is_control_flow_terminator(&node)
                || (node.kind() == SyntaxKind::IF_STMT && if_all_branches_terminate(&node))
            {
                unreachable_start = Some(node.text_range());
            }
        }
    }

    // Emit unreachable code diagnostic for module-level code
    if let (Some(start), Some(end)) = (unreachable_start, unreachable_end) {
        if let Some(first_unreachable) = find_first_unreachable_at_root(root, start) {
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

/// Find the first unreachable node at module root level.
fn find_first_unreachable_at_root(root: &SyntaxNode, after_range: TextRange) -> Option<TextRange> {
    for child in root.children() {
        let child_start = child.text_range().start();
        if child_start > after_range.end()
            && (is_statement_node(&child)
                || matches!(child.kind(), SyntaxKind::PRE_IF_DIR | SyntaxKind::PRE_REGION_DIR))
        {
            return Some(child.text_range());
        }
    }
    None
}

/// Check if a statement list contains at least one return statement.
fn has_return_statement(stmt_list: &SyntaxNode) -> bool {
    stmt_list.descendants().any(|n| n.kind() == SyntaxKind::RETURN_STMT)
}

/// Check if function has missing return paths using CFG analysis.
///
/// Returns true if some execution paths don't have explicit return statements.
/// This uses the same CFG analysis as AllFunctionPathMustHaveReturn diagnostic.
fn check_missing_return_paths(stmt_list: &SyntaxNode) -> bool {
    use cfg::{CfgBuilder, CfgEdgeType, CfgVertex};

    // Build CFG with default configuration (loops executed at least once)
    let mut builder = CfgBuilder::new();
    builder.produce_loop_iterations(true); // Default: assume loops execute at least once
    let cfg = builder.build_graph(stmt_list);

    let exit_point = cfg.exit_point();

    // Check all incoming edges to exit point
    let incoming: Vec<_> = cfg.incoming_edges(exit_point).collect();

    for (source_idx, edge_type) in incoming.iter() {
        if let Some(vertex) = cfg.vertex(*source_idx) {
            // Check if this path has missing return
            let has_missing = match vertex {
                CfgVertex::BasicBlock(block) => {
                    // Check incoming edges
                    let incoming_edges: Vec<_> = cfg.incoming_edges(*source_idx).collect();

                    // Endless loop bypass is unreachable
                    let from_endless_loop = incoming_edges.iter().any(|(src_idx, edge)| {
                        matches!(edge, CfgEdgeType::FalseBranch)
                            && matches!(
                                cfg.vertex(*src_idx),
                                Some(CfgVertex::WhileLoop(loop_v)) if loop_v.is_endless()
                            )
                    });

                    if from_endless_loop {
                        false
                    } else {
                        check_basic_block_missing_return(*source_idx, block, &cfg)
                    }
                }
                CfgVertex::WhileLoop(loop_vertex) => {
                    // Endless loops are assumed to return inside
                    if loop_vertex.is_endless() {
                        false
                    } else {
                        // Loop false branch (didn't execute) is OK if loops_executed_at_least_once
                        **edge_type != CfgEdgeType::FalseBranch
                    }
                }
                CfgVertex::ForLoop(_) | CfgVertex::ForEachLoop(_) => {
                    // Loop false branch (didn't execute) is OK
                    **edge_type != CfgEdgeType::FalseBranch
                }
                CfgVertex::Conditional(_) => {
                    // Missing else clause
                    true
                }
                _ => false,
            };

            if has_missing {
                return true;
            }
        }
    }

    false
}

/// Check if a basic block has missing return.
fn check_basic_block_missing_return(
    vertex_idx: cfg::NodeIndex,
    block: &cfg::BasicBlockVertex,
    cfg: &cfg::ControlFlowGraph,
) -> bool {
    use cfg::CfgEdgeType;
    use cfg::CfgVertex;

    if block.is_empty() {
        // Check incoming edges
        let incoming_edges: Vec<_> = cfg.incoming_edges(vertex_idx).collect();

        // Loop false branch is OK
        let from_loop_false = incoming_edges.iter().any(|(source_idx, edge)| {
            matches!(edge, CfgEdgeType::FalseBranch)
                && matches!(
                    cfg.vertex(*source_idx),
                    Some(
                        CfgVertex::WhileLoop(_) | CfgVertex::ForLoop(_) | CfgVertex::ForEachLoop(_)
                    )
                )
        });

        if from_loop_false {
            return false;
        }

        // Missing else clause
        let from_conditional_false = incoming_edges.iter().any(|(source_idx, edge)| {
            matches!(edge, CfgEdgeType::FalseBranch)
                && matches!(cfg.vertex(*source_idx), Some(CfgVertex::Conditional(_)))
        });

        if from_conditional_false {
            return true;
        }

        // Check if all incoming edges have returns
        let all_have_returns = incoming_edges.iter().all(|(source_idx, _)| {
            if let Some(CfgVertex::BasicBlock(src_block)) = cfg.vertex(*source_idx) {
                src_block.last_statement().is_some_and(|stmt| {
                    matches!(stmt.kind(), SyntaxKind::RETURN_STMT | SyntaxKind::RAISE_STMT)
                })
            } else {
                false
            }
        });

        return !all_have_returns;
    }

    // Non-empty block: check last statement
    let has_explicit_return = block.last_statement().is_some_and(|stmt| {
        matches!(stmt.kind(), SyntaxKind::RETURN_STMT | SyntaxKind::RAISE_STMT)
    });

    !has_explicit_return
}

/// Lower parameter list.
fn lower_params(ctx: &mut LoweringCtx, param_list: &SyntaxNode) -> Vec<BindingId> {
    let mut params = Vec::new();

    for param in param_list.children().filter(|n| n.kind() == SyntaxKind::PARAM) {
        if let Some(binding_id) = lower_param(ctx, &param) {
            params.push(binding_id);
        }
    }

    params
}

/// Lower a single parameter.
fn lower_param(ctx: &mut LoweringCtx, param: &SyntaxNode) -> Option<BindingId> {
    let name_token = param
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)?;

    let is_val = param
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::KW_VAL);

    // Register parameter name so it's not flagged as unused
    ctx.register_param(name_token.text());

    let binding = Binding::new(Name::new(name_token.text()), is_val);
    Some(ctx.alloc_binding(binding, name_token.text_range()))
}

/// Lower a statement list.
///
/// Also detects unreachable code after control flow statements (return, raise, break, continue)
/// and after if-else where all branches terminate.
fn lower_stmt_list(ctx: &mut LoweringCtx, stmt_list: &SyntaxNode) -> Vec<StmtId> {
    lower_stmt_list_with_unreachable(ctx, stmt_list, true)
}

/// Lower a statement list with optional unreachable code detection.
///
/// The `emit_diagnostics` parameter controls whether to emit unreachable code diagnostics.
/// This is useful for recursive processing where we want to collect statements but not
/// emit duplicate diagnostics.
fn lower_stmt_list_with_unreachable(
    ctx: &mut LoweringCtx,
    stmt_list: &SyntaxNode,
    emit_diagnostics: bool,
) -> Vec<StmtId> {
    let mut stmts = Vec::new();
    let mut unreachable_start: Option<TextRange> = None;
    let mut unreachable_end: Option<TextRange> = None;

    // Track pending BeginTransaction node for BeginTransactionBeforeTryCatch diagnostic
    let mut pending_begin_transaction: Option<SyntaxNode> = None;

    for child in stmt_list.children() {
        // Handle preprocessor directives - process content recursively
        if child.kind() == SyntaxKind::PRE_IF_DIR {
            // Check if this directive is unreachable
            if unreachable_start.is_some() {
                unreachable_end = Some(child.text_range());
            } else {
                process_preproc_if(ctx, &child);
                // Check if all branches terminate - subsequent code is unreachable
                if preproc_if_all_branches_terminate(&child) {
                    unreachable_start = Some(child.text_range());
                }
            }
            continue;
        }
        if child.kind() == SyntaxKind::PRE_REGION_DIR {
            // Check if this region is unreachable
            if unreachable_start.is_some() {
                unreachable_end = Some(child.text_range());
            } else {
                process_preproc_region(ctx, &child);
                // Check if region terminates - propagate unreachable state
                if preproc_region_terminates(&child) {
                    unreachable_start = Some(child.text_range());
                }
            }
            continue;
        }

        // Skip non-statement nodes
        if !is_statement_node(&child) {
            continue;
        }

        // BeginTransactionBeforeTryCatch: Check for Try statement (consumes pending BeginTransaction)
        if emit_diagnostics && child.kind() == SyntaxKind::TRY_STMT {
            pending_begin_transaction = None;
        }

        // If we're in unreachable mode, extend the range
        if unreachable_start.is_some() {
            unreachable_end = Some(child.text_range());
            // Still lower the statement (for completeness), but we've marked it unreachable
            if let Some(stmt_id) = lower_stmt(ctx, &child) {
                stmts.push(stmt_id);
            }
            continue;
        }

        // BeginTransactionBeforeTryCatch: Check for BeginTransaction call
        if emit_diagnostics {
            let is_begin_trans = is_global_begin_transaction_call(&child);

            if is_begin_trans {
                // If we have pending BeginTransaction, emit diagnostic for it
                if let Some(pending_node) = pending_begin_transaction.take() {
                    let extended_range =
                        extend_range_with_semicolon(&pending_node, pending_node.text_range());
                    ctx.emit(BodyDiagnostic::BeginTransactionBeforeTryCatch {
                        range: extended_range,
                    });
                }

                // Check if BeginTransaction is inside Try body
                if is_inside_try_body(&child) {
                    let extended_range = extend_range_with_semicolon(&child, child.text_range());
                    ctx.emit(BodyDiagnostic::BeginTransactionBeforeTryCatch {
                        range: extended_range,
                    });
                } else {
                    // Store as pending (will be consumed by Try or reported as error)
                    pending_begin_transaction = Some(child.clone());
                }
            } else if child.kind() != SyntaxKind::TRY_STMT {
                // Any other statement (not Try, not BeginTransaction) while pending → ERROR
                if let Some(pending_node) = pending_begin_transaction.take() {
                    let extended_range =
                        extend_range_with_semicolon(&pending_node, pending_node.text_range());
                    ctx.emit(BodyDiagnostic::BeginTransactionBeforeTryCatch {
                        range: extended_range,
                    });
                }
            }
        }

        // Lower the statement
        if let Some(stmt_id) = lower_stmt(ctx, &child) {
            stmts.push(stmt_id);

            // Check if this statement is a control flow that makes subsequent code unreachable
            if is_control_flow_terminator(&child) {
                // Mark that subsequent statements are unreachable
                // The range will start from the next statement
                unreachable_start = Some(child.text_range());
            }
            // Check if if-statement has all branches terminating
            else if child.kind() == SyntaxKind::IF_STMT && if_all_branches_terminate(&child) {
                unreachable_start = Some(child.text_range());
            }
        }
    }

    // Emit unreachable code diagnostic if we found any
    if emit_diagnostics {
        if let (Some(_start), Some(end)) = (unreachable_start, unreachable_end) {
            // Find the first unreachable statement's range
            // We need to get the range from after the control flow statement to the end
            if let Some(first_unreachable) = find_first_unreachable_stmt(stmt_list, _start) {
                let range = TextRange::new(first_unreachable.start(), end.end());
                ctx.emit(BodyDiagnostic::UnreachableCode { range });
            }
        }

        // BeginTransactionBeforeTryCatch: If there's still pending at end of list → ERROR
        if let Some(pending_node) = pending_begin_transaction {
            let extended_range =
                extend_range_with_semicolon(&pending_node, pending_node.text_range());
            ctx.emit(BodyDiagnostic::BeginTransactionBeforeTryCatch { range: extended_range });
        }
    }

    stmts
}

/// Check if a node is a statement (vs whitespace, comments, etc.)
fn is_statement_node(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::ASSIGN_STMT
            | SyntaxKind::CALL_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::RAISE_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::GOTO_STMT
            | SyntaxKind::LABEL_STMT
            | SyntaxKind::EXECUTE_STMT
            | SyntaxKind::ADD_HANDLER_STMT
            | SyntaxKind::REMOVE_HANDLER_STMT
            | SyntaxKind::VAR_DEF
    )
}

/// Check if a statement terminates control flow (making subsequent code unreachable).
fn is_control_flow_terminator(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::RETURN_STMT
            | SyntaxKind::RAISE_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::GOTO_STMT
    )
}

/// Find the first statement after a control flow terminator.
fn find_first_unreachable_stmt(
    stmt_list: &SyntaxNode,
    after_range: TextRange,
) -> Option<TextRange> {
    for child in stmt_list.children() {
        if is_statement_node(&child) && child.text_range().start() > after_range.end() {
            return Some(child.text_range());
        }
        // Also check for preprocessor directives as unreachable
        if matches!(child.kind(), SyntaxKind::PRE_IF_DIR | SyntaxKind::PRE_REGION_DIR)
            && child.text_range().start() > after_range.end()
        {
            return Some(child.text_range());
        }
    }
    None
}

/// Process preprocessor `#Если` directive, analyzing each branch for unreachable code.
fn process_preproc_if(ctx: &mut LoweringCtx, node: &SyntaxNode) {
    // Process the main branch (content after condition, before elsif/else/endif)
    process_preproc_branch_content(ctx, node);

    // Process ElsIf clauses
    for elsif in node.children().filter(|n| n.kind() == SyntaxKind::PRE_ELSIF_CLAUSE) {
        process_preproc_branch_content(ctx, &elsif);
    }

    // Process Else clause
    for else_clause in node.children().filter(|n| n.kind() == SyntaxKind::PRE_ELSE_CLAUSE) {
        process_preproc_branch_content(ctx, &else_clause);
    }
}

/// Process preprocessor `#Область` directive, analyzing content for unreachable code.
fn process_preproc_region(ctx: &mut LoweringCtx, node: &SyntaxNode) {
    process_preproc_branch_content(ctx, node);
}

/// Process content within a preprocessor branch (or region).
///
/// Looks for statements and nested preprocessor directives, tracking unreachable code.
fn process_preproc_branch_content(ctx: &mut LoweringCtx, node: &SyntaxNode) {
    let mut unreachable_start: Option<TextRange> = None;
    let mut unreachable_end: Option<TextRange> = None;

    for child in node.children() {
        // Handle nested preprocessor directives
        if child.kind() == SyntaxKind::PRE_IF_DIR {
            // Check if this preprocessor directive is unreachable
            if unreachable_start.is_some() {
                unreachable_end = Some(child.text_range());
            } else {
                // Process the preprocessor directive
                process_preproc_if(ctx, &child);
                // Check if all branches of this preprocessor terminate
                if preproc_if_all_branches_terminate(&child) {
                    unreachable_start = Some(child.text_range());
                }
            }
            continue;
        }
        if child.kind() == SyntaxKind::PRE_REGION_DIR {
            if unreachable_start.is_some() {
                unreachable_end = Some(child.text_range());
            }
            process_preproc_region(ctx, &child);
            continue;
        }

        // Handle statement lists within the branch
        if child.kind() == SyntaxKind::STMT_LIST {
            // Process the statement list for unreachable code
            lower_stmt_list_with_unreachable(ctx, &child, true);
            // Check if stmt_list terminates - propagate unreachable state
            if unreachable_start.is_none() && stmt_list_terminates(&child) {
                unreachable_start = Some(child.text_range());
            }
            continue;
        }

        // Handle individual statements (might appear directly in preprocessor content)
        if is_statement_node(&child) {
            if unreachable_start.is_some() {
                unreachable_end = Some(child.text_range());
                lower_stmt(ctx, &child);
                continue;
            }

            lower_stmt(ctx, &child);

            if is_control_flow_terminator(&child)
                || (child.kind() == SyntaxKind::IF_STMT && if_all_branches_terminate(&child))
            {
                unreachable_start = Some(child.text_range());
            }
        }
    }

    // Emit unreachable code diagnostic for this branch
    if let (Some(start), Some(end)) = (unreachable_start, unreachable_end) {
        if let Some(first_unreachable) = find_first_unreachable_in_preproc(node, start) {
            let range = TextRange::new(first_unreachable.start(), end.end());
            ctx.emit(BodyDiagnostic::UnreachableCode { range });
        }
    }
}

/// Find the first unreachable node in preprocessor content.
fn find_first_unreachable_in_preproc(
    node: &SyntaxNode,
    after_range: TextRange,
) -> Option<TextRange> {
    for child in node.children() {
        let child_start = child.text_range().start();
        if child_start > after_range.end()
            && (is_statement_node(&child)
                || matches!(child.kind(), SyntaxKind::PRE_IF_DIR | SyntaxKind::PRE_REGION_DIR))
        {
            return Some(child.text_range());
        }
    }
    None
}

/// Check if an if-statement has all branches terminating (with return/raise).
///
/// This returns true only if:
/// 1. The if-statement has an else branch
/// 2. All branches (then, elsif*, else) end with a terminator or another if-all-branches-terminate
fn if_all_branches_terminate(node: &SyntaxNode) -> bool {
    // Must have an else clause for all branches to be covered
    let has_else = node.children().any(|n| n.kind() == SyntaxKind::ELSE_CLAUSE);
    if !has_else {
        return false;
    }

    // Check then branch (first STMT_LIST)
    let then_stmt_list = node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);
    if !then_stmt_list.is_some_and(|n| stmt_list_terminates(&n)) {
        return false;
    }

    // Check all elsif branches
    for elsif in node.children().filter(|n| n.kind() == SyntaxKind::ELSIF_CLAUSE) {
        let elsif_stmt_list = elsif.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);
        if !elsif_stmt_list.is_some_and(|n| stmt_list_terminates(&n)) {
            return false;
        }
    }

    // Check else branch
    let else_clause = node.children().find(|n| n.kind() == SyntaxKind::ELSE_CLAUSE);
    if let Some(else_node) = else_clause {
        let else_stmt_list = else_node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);
        if !else_stmt_list.is_some_and(|n| stmt_list_terminates(&n)) {
            return false;
        }
    }

    true
}

/// Check if a statement list ends with a terminator.
///
/// A statement list terminates if its last statement is a terminator (return/raise/break/continue)
/// or an if-statement where all branches terminate.
fn stmt_list_terminates(stmt_list: &SyntaxNode) -> bool {
    // Get the last statement (skip preprocessor directives, regions, etc.)
    let last_stmt = stmt_list
        .children()
        .filter(|n| {
            is_statement_node(n)
                || n.kind() == SyntaxKind::PRE_IF_DIR
                || n.kind() == SyntaxKind::PRE_REGION_DIR
        })
        .last();

    match last_stmt {
        Some(node) => {
            if is_control_flow_terminator(&node) {
                true
            } else if node.kind() == SyntaxKind::IF_STMT {
                if_all_branches_terminate(&node)
            } else if node.kind() == SyntaxKind::PRE_IF_DIR {
                // For preprocessor #Если, we can't statically know which branch runs,
                // so conservatively return false
                false
            } else if node.kind() == SyntaxKind::PRE_REGION_DIR {
                // Check if region ends with terminator
                preproc_region_terminates(&node)
            } else {
                false
            }
        }
        None => false,
    }
}

/// Check if a preprocessor region ends with a terminator.
fn preproc_region_terminates(region: &SyntaxNode) -> bool {
    // Get the last statement/directive in the region
    let last = region
        .children()
        .filter(|n| {
            is_statement_node(n)
                || n.kind() == SyntaxKind::PRE_IF_DIR
                || n.kind() == SyntaxKind::PRE_REGION_DIR
                || n.kind() == SyntaxKind::STMT_LIST
        })
        .last();

    match last {
        Some(node) if node.kind() == SyntaxKind::STMT_LIST => stmt_list_terminates(&node),
        Some(node) if is_control_flow_terminator(&node) => true,
        Some(node) if node.kind() == SyntaxKind::IF_STMT => if_all_branches_terminate(&node),
        Some(node) if node.kind() == SyntaxKind::PRE_REGION_DIR => preproc_region_terminates(&node),
        Some(node) if node.kind() == SyntaxKind::PRE_IF_DIR => {
            preproc_if_all_branches_terminate(&node)
        }
        _ => false,
    }
}

/// Check if a preprocessor #Если directive has all branches terminating.
///
/// For code after #КонецЕсли to be unreachable, ALL branches must terminate:
/// - The main branch (after #Если ... Тогда)
/// - All #ИначеЕсли branches
/// - The #Иначе branch (must exist)
fn preproc_if_all_branches_terminate(node: &SyntaxNode) -> bool {
    // Must have an #Иначе clause for all branches to be covered
    let has_else = node.children().any(|n| n.kind() == SyntaxKind::PRE_ELSE_CLAUSE);
    if !has_else {
        return false;
    }

    // Check main branch (content directly in PRE_IF_DIR before any clause)
    if !preproc_branch_terminates(node) {
        return false;
    }

    // Check all #ИначеЕсли branches
    for elsif in node.children().filter(|n| n.kind() == SyntaxKind::PRE_ELSIF_CLAUSE) {
        if !preproc_branch_terminates(&elsif) {
            return false;
        }
    }

    // Check #Иначе branch
    let else_clause = node.children().find(|n| n.kind() == SyntaxKind::PRE_ELSE_CLAUSE);
    if let Some(else_node) = else_clause {
        if !preproc_branch_terminates(&else_node) {
            return false;
        }
    }

    true
}

/// Check if a preprocessor branch (main, elsif, or else) terminates.
fn preproc_branch_terminates(branch: &SyntaxNode) -> bool {
    // Get the last statement/directive/stmt_list in the branch
    let last = branch
        .children()
        .filter(|n| {
            is_statement_node(n)
                || n.kind() == SyntaxKind::PRE_IF_DIR
                || n.kind() == SyntaxKind::PRE_REGION_DIR
                || n.kind() == SyntaxKind::STMT_LIST
        })
        .last();

    match last {
        Some(node) if node.kind() == SyntaxKind::STMT_LIST => stmt_list_terminates(&node),
        Some(node) if is_control_flow_terminator(&node) => true,
        Some(node) if node.kind() == SyntaxKind::IF_STMT => if_all_branches_terminate(&node),
        Some(node) if node.kind() == SyntaxKind::PRE_REGION_DIR => preproc_region_terminates(&node),
        Some(node) if node.kind() == SyntaxKind::PRE_IF_DIR => {
            preproc_if_all_branches_terminate(&node)
        }
        _ => false,
    }
}

/// Lower a single statement.
fn lower_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<StmtId> {
    let range = node.text_range();

    let stmt = match node.kind() {
        SyntaxKind::ASSIGN_STMT => lower_assign_stmt(ctx, node),
        SyntaxKind::CALL_STMT => lower_call_stmt(ctx, node),
        SyntaxKind::RETURN_STMT => lower_return_stmt(ctx, node),
        SyntaxKind::IF_STMT => lower_if_stmt(ctx, node),
        SyntaxKind::WHILE_STMT => lower_while_stmt(ctx, node),
        SyntaxKind::FOR_STMT => lower_for_stmt(ctx, node),
        SyntaxKind::FOR_EACH_STMT => lower_for_each_stmt(ctx, node),
        SyntaxKind::TRY_STMT => lower_try_stmt(ctx, node),
        SyntaxKind::RAISE_STMT => lower_raise_stmt(ctx, node),
        SyntaxKind::BREAK_STMT => Some(Stmt::Break),
        SyntaxKind::CONTINUE_STMT => Some(Stmt::Continue),
        SyntaxKind::GOTO_STMT => lower_goto_stmt(ctx, node),
        SyntaxKind::LABEL_STMT => lower_label_stmt(ctx, node),
        SyntaxKind::EXECUTE_STMT => lower_execute_stmt(ctx, node),
        SyntaxKind::ADD_HANDLER_STMT => lower_add_handler_stmt(ctx, node),
        SyntaxKind::REMOVE_HANDLER_STMT => lower_remove_handler_stmt(ctx, node),
        SyntaxKind::VAR_DEF => lower_var_decl(ctx, node),
        SyntaxKind::EMPTY_STMT => return None,
        _ => return None,
    }?;

    Some(ctx.alloc_stmt(stmt, range))
}

/// Lower assignment statement.
fn lower_assign_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let mut children = node.children().peekable();

    // First child should be target expression (or EXPR wrapper)
    let target_node = children.next()?;
    let target = lower_expr_node(ctx, &target_node);

    // For simple variable assignment (X = value), the target is WRITTEN, not read.
    // We need to unmark it from used_vars since lower_expr incorrectly marked it.
    // For field/index access (Obj.Field = value, Arr[i] = value), the base IS read.
    //
    // Also, if the target is a simple Path and not already in local_vars,
    // this is an implicit variable declaration (BSL allows this).
    let target_name = if let Expr::Path(name) = ctx.body.expr(target) {
        Some((name.clone(), get_target_range(&target_node)))
    } else {
        None
    };
    if let Some((name, range)) = target_name {
        let key = name.as_str().to_lowercase();
        // Register implicit variable if not already declared.
        // But don't register if it's a known external (module variable) or parameter.
        if !ctx.local_vars.contains_key(&key)
            && !ctx.is_known_external(name.as_str())
            && !ctx.param_names.contains(&key)
        {
            ctx.register_local_var(name.clone(), range);
        }
        // Unmark from used - assignment is a write, not a read
        ctx.unmark_var_used(name.as_str());
    }

    // Second child should be value expression (or EXPR wrapper)
    let value_node = children.next()?;
    let value = lower_expr_node(ctx, &value_node);

    // Check for self-assignment (a = a, obj.field = obj.field)
    if exprs_are_equal(&ctx.body, target, value) {
        ctx.emit(BodyDiagnostic::SelfAssign { range: node.text_range() });
    }

    Some(Stmt::Assign { target, value })
}

/// Get the range of the target identifier in an assignment.
/// Looks for the first IDENT token within the node.
fn get_target_range(node: &SyntaxNode) -> TextRange {
    // Find IDENT token in the target expression
    fn find_ident(node: &SyntaxNode) -> Option<TextRange> {
        for token in node.descendants_with_tokens() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text_range());
            }
        }
        None
    }

    find_ident(node).unwrap_or_else(|| node.text_range())
}

/// Check if two expressions are semantically equal (case-insensitive for names).
/// Used for detecting self-assignment patterns like `a = a` or `obj.field = obj.field`.
fn exprs_are_equal(body: &Body, lhs: ExprId, rhs: ExprId) -> bool {
    match (body.expr(lhs), body.expr(rhs)) {
        // Simple variable: A = a (case-insensitive)
        (Expr::Path(name1), Expr::Path(name2)) => name1.eq_ignore_case(name2),

        // Field access: obj.field = obj.field
        (Expr::Field { base: b1, field: f1 }, Expr::Field { base: b2, field: f2 }) => {
            f1.eq_ignore_case(f2) && exprs_are_equal(body, *b1, *b2)
        }

        // Index access: arr[i] = arr[i]
        (Expr::Index { base: b1, index: i1 }, Expr::Index { base: b2, index: i2 }) => {
            exprs_are_equal(body, *b1, *b2) && exprs_are_equal(body, *i1, *i2)
        }

        // Different expression types or complex expressions - not equal
        _ => false,
    }
}

/// Lower call statement.
fn lower_call_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    // CALL_STMT contains an expression (usually CALL_EXPR or FIELD_EXPR)
    let expr_node = node.children().next()?;
    let expr = lower_expr_node(ctx, &expr_node);
    Some(Stmt::Expr(expr))
}

/// Lower return statement.
fn lower_return_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let value = node.children().next().map(|n| lower_expr_node(ctx, &n));
    Some(Stmt::Return { value })
}

/// Lower if statement.
fn lower_if_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let mut children = node.children().peekable();

    // Condition (first EXPR or expression node)
    let condition_node = children.next()?;
    let condition = lower_expr_node(ctx, &condition_node);

    // Collect all branch STMT_LIST nodes for duplicate detection
    let mut branch_nodes: Vec<SyntaxNode> = Vec::new();

    // Then branch (STMT_LIST)
    let then_stmt_list = children.next().filter(|n| n.kind() == SyntaxKind::STMT_LIST);
    let then_branch = then_stmt_list.as_ref().map(|n| lower_stmt_list(ctx, n)).unwrap_or_default();

    // Check for empty then branch
    if then_branch.is_empty() {
        if let Some(ref stmt_list) = then_stmt_list {
            ctx.emit(BodyDiagnostic::EmptyCodeBlock { range: stmt_list.text_range() });
        }
    }

    // Add then branch to branch_nodes for duplicate detection
    if let Some(stmt_list) = then_stmt_list {
        branch_nodes.push(stmt_list);
    }

    // Elsif branches
    let mut elsif_branches = Vec::new();
    for elsif in node.children().filter(|n| n.kind() == SyntaxKind::ELSIF_CLAUSE) {
        let mut elsif_children = elsif.children();
        if let Some(cond_node) = elsif_children.next() {
            let cond = lower_expr_node(ctx, &cond_node);
            let stmt_list_node = elsif_children.find(|n| n.kind() == SyntaxKind::STMT_LIST);
            let body = stmt_list_node.as_ref().map(|n| lower_stmt_list(ctx, n)).unwrap_or_default();

            // Check for empty elsif branch
            if body.is_empty() {
                if let Some(ref stmt_list) = stmt_list_node {
                    ctx.emit(BodyDiagnostic::EmptyCodeBlock { range: stmt_list.text_range() });
                }
            }

            // Add elsif branch to branch_nodes for duplicate detection
            if let Some(stmt_list) = stmt_list_node {
                branch_nodes.push(stmt_list);
            }

            elsif_branches.push((cond, body.into_boxed_slice()));
        }
    }

    // Else branch
    let else_branch =
        node.children().find(|n| n.kind() == SyntaxKind::ELSE_CLAUSE).and_then(|else_clause| {
            else_clause.children().find(|n| n.kind() == SyntaxKind::STMT_LIST).map(|n| {
                let stmts = lower_stmt_list(ctx, &n);

                // Check for empty else branch
                if stmts.is_empty() {
                    ctx.emit(BodyDiagnostic::EmptyCodeBlock { range: n.text_range() });
                }

                // Add else branch to branch_nodes for duplicate detection
                branch_nodes.push(n.clone());

                stmts.into_boxed_slice()
            })
        });

    // Check for duplicated code blocks
    check_duplicated_code_blocks(ctx, &branch_nodes);

    Some(Stmt::If {
        condition,
        then_branch: then_branch.into_boxed_slice(),
        elsif_branches: elsif_branches.into_boxed_slice(),
        else_branch,
    })
}

/// Check for duplicated code blocks in if/elsif/else branches.
///
/// Compares all pairs of branches and emits diagnostics for identical blocks.
fn check_duplicated_code_blocks(ctx: &mut LoweringCtx, branch_nodes: &[SyntaxNode]) {
    use std::collections::HashSet;

    if branch_nodes.len() < 2 {
        return;
    }

    // Track which blocks we've already reported as duplicates
    let mut reported: HashSet<usize> = HashSet::new();

    // Compare all pairs of code blocks
    for i in 0..branch_nodes.len() - 1 {
        if reported.contains(&i) {
            continue;
        }

        let current_block = &branch_nodes[i];

        // Find all identical blocks after current one
        let mut has_duplicate = false;
        for (j, other_block) in branch_nodes.iter().enumerate().skip(i + 1) {
            // Skip empty blocks (both must be non-empty for comparison)
            if is_empty_block(current_block) && is_empty_block(other_block) {
                continue;
            }

            // Compare blocks structurally
            if are_blocks_identical(current_block, other_block) {
                has_duplicate = true;
                reported.insert(j);
            }
        }

        if has_duplicate {
            // Report diagnostic on the first block with duplicates
            ctx.emit(BodyDiagnostic::IfElseDuplicatedCodeBlock {
                range: current_block.text_range(),
            });
        }
    }
}

/// Check if a code block is empty (no children or only whitespace).
fn is_empty_block(block: &SyntaxNode) -> bool {
    block.children().next().is_none()
}

/// Compare two code blocks for structural equality.
///
/// Uses normalized text comparison (case-insensitive, whitespace-normalized).
fn are_blocks_identical(block1: &SyntaxNode, block2: &SyntaxNode) -> bool {
    // Normalize and compare text content
    let text1 = normalize_code_block(block1);
    let text2 = normalize_code_block(block2);

    if text1 != text2 {
        return false;
    }

    // Additional structural check: same number of statements
    let stmt_count1 = count_statements(block1);
    let stmt_count2 = count_statements(block2);

    stmt_count1 == stmt_count2 && stmt_count1 > 0
}

/// Normalize code block for comparison.
///
/// Removes whitespace and converts to lowercase (bilingual support).
fn normalize_code_block(block: &SyntaxNode) -> String {
    block
        .text()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

/// Count the number of statement nodes in a code block.
fn count_statements(block: &SyntaxNode) -> usize {
    block
        .descendants()
        .filter(|node| {
            matches!(
                node.kind(),
                SyntaxKind::CALL_STMT
                    | SyntaxKind::ASSIGN_STMT
                    | SyntaxKind::RETURN_STMT
                    | SyntaxKind::IF_STMT
                    | SyntaxKind::WHILE_STMT
                    | SyntaxKind::FOR_STMT
                    | SyntaxKind::BREAK_STMT
                    | SyntaxKind::CONTINUE_STMT
                    | SyntaxKind::RAISE_STMT
                    | SyntaxKind::TRY_STMT
            )
        })
        .count()
}

/// Lower while statement.
fn lower_while_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let mut children = node.children();

    let condition_node = children.next()?;
    let condition = lower_expr_node(ctx, &condition_node);

    let body = children
        .find(|n| n.kind() == SyntaxKind::STMT_LIST)
        .map(|n| {
            let stmts = lower_stmt_list(ctx, &n);

            // Check for empty while body
            if stmts.is_empty() {
                ctx.emit(BodyDiagnostic::EmptyCodeBlock { range: n.text_range() });
            }

            stmts.into_boxed_slice()
        })
        .unwrap_or_default();

    Some(Stmt::While { condition, body })
}

/// Lower for statement.
fn lower_for_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    // Find loop variable (IDENT token after FOR keyword)
    let var_token = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)?;

    let name = Name::new(var_token.text());
    let range = var_token.text_range();

    // Register loop variable for unused variable tracking
    ctx.register_local_var(name.clone(), range);

    let var = ctx.alloc_binding(Binding::var(name), range);

    let mut expr_iter = node.children().filter(|n| {
        matches!(
            n.kind(),
            SyntaxKind::EXPR
                | SyntaxKind::LITERAL
                | SyntaxKind::BINARY_EXPR
                | SyntaxKind::UNARY_EXPR
                | SyntaxKind::CALL_EXPR
        )
    });

    let from =
        expr_iter.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let to =
        expr_iter.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let body = node
        .children()
        .find(|n| n.kind() == SyntaxKind::STMT_LIST)
        .map(|n| {
            let stmts = lower_stmt_list(ctx, &n);

            // Check for empty for body
            if stmts.is_empty() {
                ctx.emit(BodyDiagnostic::EmptyCodeBlock { range: n.text_range() });
            }

            stmts.into_boxed_slice()
        })
        .unwrap_or_default();

    Some(Stmt::For { var, from, to, body })
}

/// Lower for-each statement.
fn lower_for_each_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    // Find loop variable (first IDENT token)
    let var_token = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)?;

    let name = Name::new(var_token.text());
    let range = var_token.text_range();

    // Register loop variable for unused variable tracking
    ctx.register_local_var(name.clone(), range);

    let var = ctx.alloc_binding(Binding::var(name), range);

    // Collection is the first expression child
    let collection = node
        .children()
        .find(|n| {
            matches!(
                n.kind(),
                SyntaxKind::EXPR
                    | SyntaxKind::CALL_EXPR
                    | SyntaxKind::FIELD_EXPR
                    | SyntaxKind::INDEX_EXPR
            )
        })
        .map(|n| lower_expr_node(ctx, &n))
        .unwrap_or_else(|| ctx.missing_expr());

    let body = node
        .children()
        .find(|n| n.kind() == SyntaxKind::STMT_LIST)
        .map(|n| {
            let stmts = lower_stmt_list(ctx, &n);

            // Check for empty for-each body
            if stmts.is_empty() {
                ctx.emit(BodyDiagnostic::EmptyCodeBlock { range: n.text_range() });
            }

            stmts.into_boxed_slice()
        })
        .unwrap_or_default();

    Some(Stmt::ForEach { var, collection, body })
}

/// Lower try statement.
fn lower_try_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let body = node
        .children()
        .find(|n| n.kind() == SyntaxKind::STMT_LIST)
        .map(|n| {
            let stmts = lower_stmt_list(ctx, &n);

            // Check for empty try body
            if stmts.is_empty() {
                ctx.emit(BodyDiagnostic::EmptyCodeBlock { range: n.text_range() });
            }

            stmts.into_boxed_slice()
        })
        .unwrap_or_default();

    let except = node
        .children()
        .find(|n| n.kind() == SyntaxKind::EXCEPT_CLAUSE)
        .and_then(|except_clause| {
            except_clause.children().find(|n| n.kind() == SyntaxKind::STMT_LIST).map(|n| {
                let stmts = lower_stmt_list(ctx, &n);

                // Check for empty except body
                if stmts.is_empty() {
                    ctx.emit(BodyDiagnostic::EmptyCodeBlock { range: n.text_range() });
                }

                stmts.into_boxed_slice()
            })
        })
        .unwrap_or_default();

    Some(Stmt::Try { body, except })
}

/// Lower raise statement.
fn lower_raise_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let value = node.children().next().map(|n| lower_expr_node(ctx, &n));
    Some(Stmt::Raise { value })
}

/// Lower goto statement.
fn lower_goto_stmt(_ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let label_token = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)?;

    Some(Stmt::Goto(Name::new(label_token.text())))
}

/// Lower label statement.
fn lower_label_stmt(_ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let label_token = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)?;

    Some(Stmt::Label(Name::new(label_token.text())))
}

/// Lower execute statement.
fn lower_execute_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let expr = node
        .children()
        .next()
        .map(|n| lower_expr_node(ctx, &n))
        .unwrap_or_else(|| ctx.missing_expr());

    Some(Stmt::Execute { expr })
}

/// Lower add handler statement.
fn lower_add_handler_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let mut expr_iter = node.children();

    let event =
        expr_iter.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let handler =
        expr_iter.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    Some(Stmt::AddHandler { event, handler })
}

/// Lower remove handler statement.
fn lower_remove_handler_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let mut expr_iter = node.children();

    let event =
        expr_iter.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let handler =
        expr_iter.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    Some(Stmt::RemoveHandler { event, handler })
}

/// Lower variable declaration.
fn lower_var_decl(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let mut bindings = Vec::new();

    for ident in node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| tok.kind() == SyntaxKind::IDENT)
    {
        let name = Name::new(ident.text());
        let range = ident.text_range();

        // Register for unused variable tracking
        ctx.register_local_var(name.clone(), range);

        let binding_id = ctx.alloc_binding(Binding::var(name), range);
        bindings.push(binding_id);
    }

    if bindings.is_empty() {
        return None;
    }

    Some(Stmt::VarDecl { bindings: bindings.into_boxed_slice() })
}

/// Lower an expression node (handles EXPR wrapper).
fn lower_expr_node(ctx: &mut LoweringCtx, node: &SyntaxNode) -> ExprId {
    // Handle EXPR wrapper - unwrap to get actual expression
    let actual_node = if node.kind() == SyntaxKind::EXPR {
        node.children().next().unwrap_or_else(|| node.clone())
    } else {
        node.clone()
    };

    lower_expr(ctx, &actual_node)
}

/// Lower an expression.
fn lower_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> ExprId {
    let range = node.text_range();

    let expr = match node.kind() {
        SyntaxKind::LITERAL => lower_literal(ctx, node),
        SyntaxKind::BINARY_EXPR => lower_binary_expr(ctx, node),
        SyntaxKind::UNARY_EXPR => lower_unary_expr(ctx, node),
        SyntaxKind::TERNARY_EXPR => lower_ternary_expr(ctx, node),
        SyntaxKind::CALL_EXPR => lower_call_expr(ctx, node),
        SyntaxKind::INDEX_EXPR => lower_index_expr(ctx, node),
        SyntaxKind::FIELD_EXPR => lower_field_expr(ctx, node),
        SyntaxKind::NEW_EXPR => lower_new_expr(ctx, node),
        SyntaxKind::PAREN_EXPR => {
            // Unwrap parenthesized expression
            return node
                .children()
                .next()
                .map(|n| lower_expr_node(ctx, &n))
                .unwrap_or_else(|| ctx.missing_expr());
        }
        SyntaxKind::IDENT => {
            // Identifier - variable reference
            let text = node.text().to_string();
            ctx.mark_var_used(&text);
            Expr::Path(Name::new(&text))
        }
        SyntaxKind::EXPR => {
            // Wrapped expression
            return node
                .children()
                .next()
                .map(|n| lower_expr(ctx, &n))
                .unwrap_or_else(|| ctx.missing_expr());
        }
        _ => {
            // Try to find IDENT token for simple identifier expressions
            if let Some(ident) = node
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| tok.kind() == SyntaxKind::IDENT)
            {
                ctx.mark_var_used(ident.text());
                Expr::Path(Name::new(ident.text()))
            } else {
                Expr::Missing
            }
        }
    };

    let expr_id = ctx.alloc_expr(expr, range);

    // Associate SDBL with ExprId
    if let Some(idx) = ctx.pending_sdbl.iter().position(|(query_text, _)| {
        if let Expr::Literal(Literal::String(ref expr_string)) = ctx.body.exprs[expr_id] {
            query_text == expr_string
        } else {
            false
        }
    }) {
        let (_query_text, query_info) = ctx.pending_sdbl.remove(idx);
        ctx.body.sdbl_exprs.push((expr_id, query_info));
    }

    expr_id
}

/// Lower a literal expression.
fn lower_literal(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    // Find the actual literal token
    let token = node.children_with_tokens().filter_map(|el| el.into_token()).find(|tok| {
        matches!(
            tok.kind(),
            SyntaxKind::DECIMAL
                | SyntaxKind::FLOAT
                | SyntaxKind::STRING
                | SyntaxKind::STRING_START
                | SyntaxKind::DATE
                | SyntaxKind::KW_TRUE
                | SyntaxKind::KW_FALSE
                | SyntaxKind::KW_UNDEFINED
                | SyntaxKind::KW_NULL
        )
    });

    let Some(token) = token else {
        return Expr::Missing;
    };

    let literal = match token.kind() {
        SyntaxKind::DECIMAL | SyntaxKind::FLOAT => {
            let text = token.text().replace(' ', "");
            let value = text.parse::<f64>().unwrap_or(0.0);

            // Check for magic number
            if is_magic_number(value) {
                ctx.emit(BodyDiagnostic::MagicNumber {
                    value: text.clone(),
                    range: token.text_range(),
                });
            }

            Literal::Number(value)
        }
        SyntaxKind::STRING | SyntaxKind::STRING_START => {
            // Extract full string content (handles multiline with |)
            let value = extract_string_content(node).unwrap_or_default();

            // Check if this is SDBL query
            if looks_like_sdbl(&value) {
                let sdbl_ast = parser::parse_sdbl(&value);

                if !sdbl_ast.has_errors() {
                    let query_info = syntax::SdblQueryInfo::new(
                        node.text_range(),
                        value.clone(),
                        Some(sdbl_ast),
                    );

                    ctx.pending_sdbl.push((value.clone(), query_info));
                }
            }

            Literal::String(value)
        }
        SyntaxKind::DATE => {
            let text = token.text();
            // Remove quotes
            let value = text.trim_start_matches('\'').trim_end_matches('\'').to_string();
            Literal::Date(value)
        }
        SyntaxKind::KW_TRUE => Literal::Bool(true),
        SyntaxKind::KW_FALSE => Literal::Bool(false),
        SyntaxKind::KW_UNDEFINED => Literal::Undefined,
        SyntaxKind::KW_NULL => Literal::Null,
        _ => return Expr::Missing,
    };

    Expr::Literal(literal)
}

/// Check if a number is a "magic number" (should be a named constant).
fn is_magic_number(value: f64) -> bool {
    // Common non-magic numbers
    const ALLOWED: &[f64] = &[-1.0, 0.0, 1.0, 2.0, 10.0, 100.0];

    if ALLOWED.contains(&value) {
        return false;
    }

    // Numbers with many digits are likely magic
    value.abs() > 2.0
}

/// Lower binary expression.
fn lower_binary_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    let lhs_node = match children.next() {
        Some(n) => n,
        None => return Expr::Missing,
    };
    let lhs = lower_expr_node(ctx, &lhs_node);

    // Find operator token
    let op_token = node.children_with_tokens().filter_map(|el| el.into_token()).find(|tok| {
        matches!(
            tok.kind(),
            SyntaxKind::PLUS
                | SyntaxKind::MINUS
                | SyntaxKind::STAR
                | SyntaxKind::SLASH
                | SyntaxKind::PERCENT
                | SyntaxKind::EQ
                | SyntaxKind::NEQ
                | SyntaxKind::LT
                | SyntaxKind::LE
                | SyntaxKind::GT
                | SyntaxKind::GE
                | SyntaxKind::KW_AND
                | SyntaxKind::KW_OR
        )
    });

    let op = op_token
        .map(|tok| match tok.kind() {
            SyntaxKind::PLUS => BinaryOp::Add,
            SyntaxKind::MINUS => BinaryOp::Sub,
            SyntaxKind::STAR => BinaryOp::Mul,
            SyntaxKind::SLASH => BinaryOp::Div,
            SyntaxKind::PERCENT => BinaryOp::Mod,
            SyntaxKind::EQ => BinaryOp::Eq,
            SyntaxKind::NEQ => BinaryOp::Neq,
            SyntaxKind::LT => BinaryOp::Lt,
            SyntaxKind::LE => BinaryOp::Le,
            SyntaxKind::GT => BinaryOp::Gt,
            SyntaxKind::GE => BinaryOp::Ge,
            SyntaxKind::KW_AND => BinaryOp::And,
            SyntaxKind::KW_OR => BinaryOp::Or,
            _ => BinaryOp::Add,
        })
        .unwrap_or(BinaryOp::Add);

    let rhs_node = match children.next() {
        Some(n) => n,
        None => return Expr::Missing,
    };
    let rhs = lower_expr_node(ctx, &rhs_node);

    Expr::BinaryOp { lhs, rhs, op }
}

/// Lower unary expression.
fn lower_unary_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    // Find operator token
    let op_token = node.children_with_tokens().filter_map(|el| el.into_token()).find(|tok| {
        matches!(tok.kind(), SyntaxKind::MINUS | SyntaxKind::PLUS | SyntaxKind::KW_NOT)
    });

    let op = op_token
        .map(|tok| match tok.kind() {
            SyntaxKind::MINUS => UnaryOp::Neg,
            SyntaxKind::PLUS => UnaryOp::Plus,
            SyntaxKind::KW_NOT => UnaryOp::Not,
            _ => UnaryOp::Neg,
        })
        .unwrap_or(UnaryOp::Neg);

    let expr_node = match node.children().next() {
        Some(n) => n,
        None => {
            let missing = ctx.missing_expr();
            return Expr::UnaryOp { expr: missing, op };
        }
    };
    let expr = lower_expr_node(ctx, &expr_node);

    Expr::UnaryOp { expr, op }
}

/// Lower ternary expression.
fn lower_ternary_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    let condition =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let then_expr =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let else_expr =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    Expr::Ternary { condition, then_expr, else_expr }
}

/// Lower call expression.
fn lower_call_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    // Callee can be identifier, field expression, etc.
    let callee_node = match children.next() {
        Some(n) => n,
        None => return Expr::Missing,
    };

    // Check if this is a global call to a deprecated method
    // Unwrap EXPR wrapper if present
    let actual_callee = if callee_node.kind() == SyntaxKind::EXPR {
        callee_node.children().next().unwrap_or_else(|| callee_node.clone())
    } else {
        callee_node.clone()
    };

    // Only check for IDENT (global function call), not FIELD_EXPR (method call)
    if actual_callee.kind() == SyntaxKind::IDENT {
        let name = actual_callee.text().to_string();
        if is_deprecated_method(&name) {
            // Emit DeprecatedMethod diagnostic
            // Range covers the entire call expression including arguments
            ctx.diagnostics
                .push(BodyDiagnostic::DeprecatedMethod { name, range: node.text_range() });
        }
    }

    let callee = lower_expr_node(ctx, &callee_node);

    // Find ARG_LIST for both lowering and diagnostics
    let arg_list_node = node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST);

    // Arguments
    let args =
        arg_list_node.as_ref().map(|arg_list| lower_arg_list(ctx, arg_list)).unwrap_or_default();

    // Emit MissedRequiredParameter diagnostic for local calls (simple IDENT)
    // Qualified calls (FIELD_EXPR) are handled in lower_field_expr
    if actual_callee.kind() == SyntaxKind::IDENT {
        let callee_name = actual_callee.text().to_string();

        // Skip if callee is a local variable (object with call operator)
        let is_local = {
            let key = callee_name.to_lowercase();
            ctx.local_vars.contains_key(&key) || ctx.param_names.contains(&key)
        };

        if !is_local {
            let arg_presence = arg_list_node.as_ref().map(extract_arg_presence).unwrap_or_default();

            ctx.diagnostics.push(BodyDiagnostic::MissedRequiredParameter {
                callee: callee_name,
                module: None,
                mdo_type: None,
                mdo_name: None,
                args: arg_presence,
                range: node.text_range(),
            });
        }
    }

    Expr::Call { callee, args: args.into_boxed_slice() }
}

/// Lower argument list.
fn lower_arg_list(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Vec<ExprId> {
    node.children().map(|n| lower_expr_node(ctx, &n)).collect()
}

/// Extract which arguments have values from an ARG_LIST node.
///
/// Returns a Boolean vector where:
/// - `true` = argument has an expression
/// - `false` = argument is empty (between commas with no value)
///
/// ## Examples
/// - `Method()` → `[]`
/// - `Method(5)` → `[true]`
/// - `Method(, 2)` → `[false, true]`
/// - `Method(5, 2)` → `[true, true]`
/// - `Method(5,)` → `[true, false]`
/// - `Method(,)` → `[false, false]`
fn extract_arg_presence(arg_list: &SyntaxNode) -> Vec<bool> {
    let mut args = Vec::new();
    let mut has_expr = false;

    for child in arg_list.children_with_tokens() {
        match child.kind() {
            SyntaxKind::COMMA => {
                args.push(has_expr);
                has_expr = false;
            }
            SyntaxKind::L_PAREN | SyntaxKind::R_PAREN => {
                // Skip parentheses
            }
            kind if kind.is_trivia() => {
                // Skip whitespace and comments
            }
            _ => {
                // Any other node indicates an expression is present
                has_expr = true;
            }
        }
    }

    // Handle last argument (after last comma or only argument)
    // Only push if we're inside the argument list (has children)
    if arg_list.children().count() > 0 || has_expr {
        args.push(has_expr);
    }

    args
}

/// Lower index expression.
fn lower_index_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    let base =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    let index =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    Expr::Index { base, index }
}

/// Lower field expression.
///
/// Handles:
/// - Two-level calls: `Module.Method()` - emits MissedRequiredParameter with module
/// - Three-level calls: `Документы.ПКО.Method()` - emits MissedRequiredParameter with mdo_type/mdo_name
/// - Field access: `obj.field` - no diagnostics
fn lower_field_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    let mut children = node.children();

    let base =
        children.next().map(|n| lower_expr_node(ctx, &n)).unwrap_or_else(|| ctx.missing_expr());

    // Find field name (IDENT token after DOT)
    let field_name = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|tok| tok.kind() == SyntaxKind::IDENT)
        .last()
        .map(|tok| Name::new(tok.text()))
        .unwrap_or_else(Name::missing);

    // Check if this is actually a method call (has ARG_LIST)
    let arg_list_node = node.children().find(|n| n.kind() == SyntaxKind::ARG_LIST);

    if arg_list_node.is_some() {
        let method = field_name.to_string();

        // Analyze call structure to determine call type
        let call_info = analyze_qualified_call(node, ctx);

        if let Some(info) = call_info {
            match info {
                QualifiedCallInfo::TwoLevel { module } => {
                    // Emit MissingCommonModuleMethod diagnostic for potential CommonModule calls.
                    ctx.diagnostics.push(BodyDiagnostic::MissingCommonModuleMethod {
                        module: module.clone(),
                        method: method.clone(),
                        range: node.text_range(),
                    });

                    // Emit MissedRequiredParameter diagnostic for qualified calls.
                    let arg_presence =
                        arg_list_node.as_ref().map(extract_arg_presence).unwrap_or_default();

                    ctx.diagnostics.push(BodyDiagnostic::MissedRequiredParameter {
                        callee: method,
                        module: Some(module),
                        mdo_type: None,
                        mdo_name: None,
                        args: arg_presence,
                        range: node.text_range(),
                    });
                }
                QualifiedCallInfo::ThreeLevel { mdo_type, mdo_name } => {
                    // Three-level call: Документы.ПКО.Method()
                    let arg_presence =
                        arg_list_node.as_ref().map(extract_arg_presence).unwrap_or_default();

                    ctx.diagnostics.push(BodyDiagnostic::MissedRequiredParameter {
                        callee: method,
                        module: None,
                        mdo_type: Some(mdo_type),
                        mdo_name: Some(mdo_name),
                        args: arg_presence,
                        range: node.text_range(),
                    });
                }
            }
        }

        let args = arg_list_node
            .as_ref()
            .map(|arg_list| lower_arg_list(ctx, arg_list))
            .unwrap_or_default();

        Expr::MethodCall { receiver: base, method: field_name, args: args.into_boxed_slice() }
    } else {
        Expr::Field { base, field: field_name }
    }
}

/// Information about a qualified call structure.
enum QualifiedCallInfo {
    /// Two-level call: `Module.Method()`
    TwoLevel { module: String },
    /// Three-level call: `Документы.ПКО.Method()`
    ThreeLevel { mdo_type: String, mdo_name: String },
}

/// Analyze a FIELD_EXPR node to determine the qualified call type.
///
/// Returns:
/// - `Some(TwoLevel)` for `Module.Method()` where Module is not a local variable
/// - `Some(ThreeLevel)` for `MdoType.MdoName.Method()` (e.g., Документы.ПКО.Method)
/// - `None` for local variable calls or field access
fn analyze_qualified_call(node: &SyntaxNode, ctx: &LoweringCtx) -> Option<QualifiedCallInfo> {
    let first_child = node.children().next()?;

    // Check for three-level call: first child is FIELD_EXPR
    // Structure: FIELD_EXPR > FIELD_EXPR > [IDENT, DOT, IDENT]
    if first_child.kind() == SyntaxKind::FIELD_EXPR {
        // Extract mdo_type and mdo_name from nested FIELD_EXPR
        let idents: Vec<String> = first_child
            .descendants_with_tokens()
            .filter_map(|el| el.into_token())
            .filter(|tok| tok.kind() == SyntaxKind::IDENT)
            .map(|tok| tok.text().to_string())
            .collect();

        tracing::trace!(
            idents = ?idents,
            first_child_kind = ?first_child.kind(),
            "Analyzing potential three-level call"
        );

        if idents.len() == 2 {
            let mdo_type = idents[0].clone();
            let mdo_name = idents[1].clone();

            // Check if mdo_type is a local variable
            let key = mdo_type.to_lowercase();
            if ctx.local_vars.contains_key(&key) || ctx.param_names.contains(&key) {
                return None;
            }

            tracing::debug!(
                mdo_type = %mdo_type,
                mdo_name = %mdo_name,
                "Detected three-level call"
            );
            return Some(QualifiedCallInfo::ThreeLevel { mdo_type, mdo_name });
        }
        return None;
    }

    // Check for two-level call: first child is IDENT or EXPR containing IDENT
    let module_name = if first_child.kind() == SyntaxKind::IDENT {
        Some(first_child.text().to_string())
    } else if first_child.kind() == SyntaxKind::EXPR {
        // Unwrap EXPR if it contains a single IDENT
        let idents: Vec<_> =
            first_child.children().filter(|n| n.kind() == SyntaxKind::IDENT).collect();
        if idents.len() == 1 {
            Some(idents[0].text().to_string())
        } else {
            None
        }
    } else {
        None
    };

    let module = module_name?;

    // Check if module name is a local variable
    let key = module.to_lowercase();
    if ctx.local_vars.contains_key(&key) || ctx.param_names.contains(&key) {
        return None;
    }

    Some(QualifiedCallInfo::TwoLevel { module })
}

/// Lower new expression.
fn lower_new_expr(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Expr {
    // Type name (IDENT after NEW keyword)
    let type_name = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)
        .map(|tok| Name::new(tok.text()));

    // Arguments
    let args = node
        .children()
        .find(|n| n.kind() == SyntaxKind::ARG_LIST)
        .map(|arg_list| lower_arg_list(ctx, &arg_list))
        .unwrap_or_default();

    Expr::New { type_name, args: args.into_boxed_slice() }
}

/// Check if a method name is deprecated (8.3.10 or 8.3.17).
/// Returns true if the method is deprecated.
fn is_deprecated_method(name: &str) -> bool {
    let lower = name.to_lowercase();

    // Deprecated methods from 8.3.10 and 8.3.17
    matches!(
        lower.as_str(),
        // 8.3.10 - Client application methods
        "установитькраткийзаголовокприложения"
            | "получитькраткийзаголовокприложения"
            | "установитьзаголовокклиентскогоприложения"
            | "получитьзаголовокклиентскогоприложения"
            | "текущийвариантосновногошрифтаклиентскогоприложения"
            | "текущийвариантинтерфейсаклиентскогоприложения"
            | "setshortapplicationcaption"
            | "getshortapplicationcaption"
            | "setclientapplicationcaption"
            | "getclientapplicationcaption"
            | "clientapplicationbasefontcurrentvariant"
            | "clientapplicationinterfacecurrentvariant"
            // 8.3.17 - Error handling methods
            | "краткоепредставлениеошибки"
            | "подробноепредставлениеошибки"
            | "показатьинформациюобошибке"
            | "brieferrorrepresentation"
            | "detailederrorrepresentation"
            | "showerrorinformation"
            // Common
            | "получитьформу"
            | "getform"
    )
}

/// Check if a statement is a global BeginTransaction/НачатьТранзакцию call.
///
/// Returns true if the statement is a non-qualified call to BeginTransaction/НачатьТранзакцию.
/// Filters out:
/// - Non-CALL_STMT nodes
/// - Qualified calls like `Connector.BeginTransaction()`
fn is_global_begin_transaction_call(node: &SyntaxNode) -> bool {
    // Must be CALL_STMT
    if node.kind() != SyntaxKind::CALL_STMT {
        return false;
    }

    // Skip if contains FIELD_EXPR (qualified call like Object.Method())
    if node.descendants().any(|n| n.kind() == SyntaxKind::FIELD_EXPR) {
        return false;
    }

    // Get first identifier token (method name)
    let ident = node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT);

    let Some(ident) = ident else {
        return false;
    };

    let name = ident.text().to_lowercase();
    name == "начатьтранзакцию" || name == "begintransaction"
}

/// Check if a node is inside a Try-Catch block body.
///
/// Walks up the AST tree looking for TRY_STMT ancestors.
fn is_inside_try_body(node: &SyntaxNode) -> bool {
    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        if parent.kind() == SyntaxKind::TRY_STMT {
            return true;
        }
        current = parent;
    }
    false
}

/// Extend a text range to include the following semicolon token if present.
///
/// Java BSLParser.StatementContext includes the SEMICOLON in the statement range.
/// Our CALL_STMT does not include SEMICOLON (it's a separate token).
/// To match Java ranges, we extend the range to include the semicolon.
fn extend_range_with_semicolon(node: &SyntaxNode, original_range: TextRange) -> TextRange {
    use syntax::NodeOrToken;

    if let Some(NodeOrToken::Token(token)) = node.next_sibling_or_token() {
        if token.kind() == SyntaxKind::SEMICOLON {
            return original_range.cover(token.text_range());
        }
    }
    original_range
}

/// Check if string looks like SDBL query.
fn looks_like_sdbl(s: &str) -> bool {
    if s.len() < 15 {
        return false;
    }
    let upper = s.to_uppercase();
    upper.contains("SELECT") || upper.contains("ВЫБРАТЬ")
}

/// Extract string content from LITERAL node.
///
/// Handles both simple strings ("text") and multiline strings with | prefixes.
fn extract_string_content(node: &SyntaxNode) -> Option<String> {
    let mut result = String::new();
    let mut tokens = node.children_with_tokens().filter_map(|it| it.into_token());

    let first_token = tokens.next()?;

    match first_token.kind() {
        SyntaxKind::STRING => {
            let text = first_token.text();
            if text.len() < 2 {
                return None;
            }
            let inner = &text[1..text.len() - 1];
            result = inner.replace("\"\"", "\"");
        }
        SyntaxKind::STRING_START => {
            let text = first_token.text();
            if text.is_empty() {
                return None;
            }
            result.push_str(&text[1..]);

            for token in tokens {
                match token.kind() {
                    SyntaxKind::NEWLINE => {
                        result.push('\n');
                    }
                    SyntaxKind::STRING_PART => {
                        let text = token.text();
                        if let Some(content) = text.strip_prefix('|') {
                            result.push_str(content);
                        }
                    }
                    SyntaxKind::STRING_TAIL => {
                        let text = token.text();
                        if let Some(content) = text.strip_prefix('|') {
                            if let Some(content) = content.strip_suffix('"') {
                                result.push_str(content);
                            }
                        }
                        break;
                    }
                    _ => {}
                }
            }

            result = result.replace("\"\"", "\"");
        }
        _ => return None,
    }

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base_db::{RootQueryDb, SourceDatabase};
    use ide_db::RootDatabaseImpl;
    use vfs::FileId;

    fn parse_method(code: &str) -> SyntaxNode {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId::from_raw(0);
        db.set_file_text(file_id, code);
        let parse = db.parse(file_id);
        let root = parse.syntax_node();

        // Find first method
        root.descendants()
            .find(|n| matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF))
            .expect("No method found in test code")
    }

    #[test]
    fn test_lower_empty_procedure() {
        let method = parse_method("Процедура Тест() КонецПроцедуры");
        let result = lower_method(&method, false);

        assert_eq!(result.body.params.len(), 0);
        // Empty procedure should emit EmptyCodeBlock diagnostic
        assert!(result
            .diagnostics
            .iter()
            .any(|d| matches!(d, BodyDiagnostic::EmptyCodeBlock { .. })));
    }

    #[test]
    fn test_lower_function_without_return() {
        let method = parse_method("Функция Тест() КонецФункции");
        let result = lower_method(&method, true);

        // Function without return should emit FunctionShouldHaveReturn diagnostic
        assert!(result
            .diagnostics
            .iter()
            .any(|d| matches!(d, BodyDiagnostic::FunctionShouldHaveReturn { .. })));
    }

    #[test]
    fn test_lower_function_with_return() {
        let method = parse_method(
            "Функция Тест()
                Возврат 42;
            КонецФункции",
        );
        let result = lower_method(&method, true);

        // Function with return should NOT emit FunctionShouldHaveReturn
        assert!(!result
            .diagnostics
            .iter()
            .any(|d| matches!(d, BodyDiagnostic::FunctionShouldHaveReturn { .. })));
    }

    #[test]
    fn test_lower_procedure_with_params() {
        let method = parse_method("Процедура Тест(А, Знач Б, В = 1) КонецПроцедуры");
        let result = lower_method(&method, false);

        assert_eq!(result.body.params.len(), 3);

        // Check first param
        let param1 = result.body.binding(result.body.params[0]);
        assert_eq!(param1.name.as_str(), "А");
        assert!(!param1.is_val);

        // Check second param (Знач)
        let param2 = result.body.binding(result.body.params[1]);
        assert_eq!(param2.name.as_str(), "Б");
        assert!(param2.is_val);

        // Check third param
        let param3 = result.body.binding(result.body.params[2]);
        assert_eq!(param3.name.as_str(), "В");
        assert!(!param3.is_val);
    }

    #[test]
    fn test_lower_assignment() {
        let method = parse_method(
            "Процедура Тест()
                А = 42;
            КонецПроцедуры",
        );
        let result = lower_method(&method, false);

        assert_eq!(result.body.body_stmts.len(), 1);
        let stmt = result.body.stmt(result.body.body_stmts[0]);
        assert!(matches!(stmt, Stmt::Assign { .. }));
    }

    #[test]
    fn test_lower_self_assign() {
        let method = parse_method(
            "Процедура Тест()
                А = А;
            КонецПроцедуры",
        );
        let result = lower_method(&method, false);

        // Self-assignment should emit diagnostic
        assert!(result.diagnostics.iter().any(|d| matches!(d, BodyDiagnostic::SelfAssign { .. })));
    }

    #[test]
    fn test_lower_if_stmt() {
        let method = parse_method(
            "Процедура Тест()
                Если Истина Тогда
                    А = 1;
                КонецЕсли;
            КонецПроцедуры",
        );
        let result = lower_method(&method, false);

        assert_eq!(result.body.body_stmts.len(), 1);
        let stmt = result.body.stmt(result.body.body_stmts[0]);
        assert!(matches!(stmt, Stmt::If { .. }));
    }

    #[test]
    fn test_sdbl_collected_in_hir() {
        let method = parse_method(
            r#"
Процедура Тест()
    Запрос = "SELECT Ссылка FROM Справочник.Валюты";
    Результат = Запрос.Выполнить();
КонецПроцедуры
"#,
        );
        let result = lower_method(&method, false);

        // Should have collected 1 SDBL query
        assert_eq!(result.body.sdbl_exprs.len(), 1);

        let (expr_id, query_info) = &result.body.sdbl_exprs[0];
        assert!(query_info.is_valid());
        assert!(query_info.query_text.contains("SELECT"));

        // Verify ExprId points to a string literal
        match result.body.expr(*expr_id) {
            Expr::Literal(Literal::String(_)) => {}
            _ => panic!("Expected string literal"),
        }
    }

    #[test]
    fn test_sdbl_multiline_query() {
        let method = parse_method(
            r#"
Функция ПолучитьДанные()
    Запрос = "SELECT
             |    Ссылка,
             |    Наименование
             |FROM Справочник.Валюты";
    Возврат Запрос.Выполнить();
КонецФункции
"#,
        );
        let result = lower_method(&method, true);

        assert_eq!(result.body.sdbl_exprs.len(), 1);

        let (_expr_id, query_info) = &result.body.sdbl_exprs[0];
        assert!(query_info.is_valid());
        // Multiline string should be parsed correctly
        assert!(query_info.query_text.contains("Наименование"));
    }

    #[test]
    fn test_short_strings_ignored() {
        let method = parse_method(
            r#"
Процедура Тест()
    Х = "SELECT";
    Y = "Test";
КонецПроцедуры
"#,
        );
        let result = lower_method(&method, false);

        // Should not collect short strings (< 15 chars)
        assert_eq!(result.body.sdbl_exprs.len(), 0);
    }

    #[test]
    fn test_multiple_queries_in_method() {
        let method = parse_method(
            r#"
Процедура МножественныеЗапросы()
    Запрос1 = "SELECT Ссылка FROM Справочник.Валюты";
    Запрос2 = "ВЫБРАТЬ Наименование ИЗ Справочник.Номенклатура";
    Результат1 = Запрос1.Выполнить();
    Результат2 = Запрос2.Выполнить();
КонецПроцедуры
"#,
        );
        let result = lower_method(&method, false);

        // Should collect both queries
        assert_eq!(result.body.sdbl_exprs.len(), 2);

        assert!(result.body.sdbl_exprs[0].1.query_text.contains("SELECT"));
        assert!(result.body.sdbl_exprs[1].1.query_text.contains("ВЫБРАТЬ"));
    }

    #[test]
    fn test_if_else_duplicated_code_block() {
        let method = parse_method(
            r#"Процедура Тест()
    Если x = 1 Тогда
        А = 1;
        Б = 2;
    Иначе
        А = 1;
        Б = 2;
    КонецЕсли;
КонецПроцедуры"#,
        );
        let result = lower_method(&method, false);

        // Should detect duplicated code blocks
        let diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d, BodyDiagnostic::IfElseDuplicatedCodeBlock { .. }))
            .collect();
        assert_eq!(diags.len(), 1, "Should detect 1 duplicated code block");
    }

    #[test]
    fn test_if_else_different_blocks() {
        let method = parse_method(
            r#"Процедура Тест()
    Если x = 1 Тогда
        А = 1;
    Иначе
        А = 2;
    КонецЕсли;
КонецПроцедуры"#,
        );
        let result = lower_method(&method, false);

        // Should NOT detect duplicated code blocks (different values)
        let diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d, BodyDiagnostic::IfElseDuplicatedCodeBlock { .. }))
            .collect();
        assert_eq!(diags.len(), 0, "Different blocks should not trigger diagnostic");
    }

    #[test]
    fn test_if_elsif_duplicated_code_block() {
        let method = parse_method(
            r#"Процедура Тест()
    Если x = 1 Тогда
        А = 1;
    ИначеЕсли x = 2 Тогда
        А = 1;
    КонецЕсли;
КонецПроцедуры"#,
        );
        let result = lower_method(&method, false);

        // Should detect duplicated code blocks in if/elsif
        let diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d, BodyDiagnostic::IfElseDuplicatedCodeBlock { .. }))
            .collect();
        assert_eq!(diags.len(), 1, "Should detect duplicated if/elsif blocks");
    }

    #[test]
    fn test_if_else_empty_blocks_not_duplicated() {
        let method = parse_method(
            r#"Процедура Тест()
    Если x = 1 Тогда
    Иначе
    КонецЕсли;
КонецПроцедуры"#,
        );
        let result = lower_method(&method, false);

        // Empty blocks should NOT be reported as duplicates
        let diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d, BodyDiagnostic::IfElseDuplicatedCodeBlock { .. }))
            .collect();
        assert_eq!(diags.len(), 0, "Empty blocks should not trigger duplicate diagnostic");
    }

    #[test]
    fn test_if_else_duplicated_range_correct() {
        let code = r#"Процедура Тест()
    Если x = 1 Тогда
        А = 1;
    Иначе
        А = 1;
    КонецЕсли;
КонецПроцедуры"#;
        let method = parse_method(code);
        let result = lower_method(&method, false);

        let diags: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d, BodyDiagnostic::IfElseDuplicatedCodeBlock { .. }))
            .collect();
        assert_eq!(diags.len(), 1);

        // Diagnostic should point to the FIRST block (then-branch)
        if let BodyDiagnostic::IfElseDuplicatedCodeBlock { range } = diags[0] {
            let text = &code[range.start().into()..range.end().into()];
            // The range should cover the STMT_LIST content (А = 1;)
            assert!(text.contains("А = 1"), "Range should cover the duplicated statement");
        }
    }
}
