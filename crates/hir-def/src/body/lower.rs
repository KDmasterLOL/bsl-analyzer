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
}

impl LoweringCtx {
    /// Create a new lowering context.
    pub fn new(is_function: bool) -> Self {
        Self {
            body: Body::new(),
            source_map: BodySourceMap::new(),
            diagnostics: Vec::new(),
            is_function,
            local_vars: FxHashMap::default(),
            used_vars: FxHashSet::default(),
        }
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
            if !self.used_vars.contains(key) {
                self.diagnostics
                    .push(BodyDiagnostic::UnusedVariable { name: name.to_string(), range: *range });
            }
        }
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
    let mut ctx = LoweringCtx::new(is_function);

    // Lower parameters
    if let Some(param_list) = method_node.children().find(|n| n.kind() == SyntaxKind::PARAM_LIST) {
        let params = lower_params(&mut ctx, &param_list);
        ctx.body.params = params.into_boxed_slice();
    }

    // Lower body statements
    if let Some(stmt_list) = method_node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST) {
        let stmts = lower_stmt_list(&mut ctx, &stmt_list);
        ctx.body.body_stmts = stmts.into_boxed_slice();

        // Check for FunctionShouldHaveReturn
        if is_function && !has_return_statement(&stmt_list) {
            // Get function name range for diagnostic
            let name_range = method_node
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| tok.kind() == SyntaxKind::IDENT)
                .map(|tok| tok.text_range())
                .unwrap_or_else(|| method_node.text_range());

            ctx.emit(BodyDiagnostic::FunctionShouldHaveReturn { range: name_range });
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
pub fn lower_module_code(root: &SyntaxNode) -> LowerResult {
    let mut ctx = LoweringCtx::new(false);

    // Find all top-level statements (not in procedures/functions)
    let stmt_kinds = [
        SyntaxKind::ASSIGN_STMT,
        SyntaxKind::CALL_STMT,
        SyntaxKind::IF_STMT,
        SyntaxKind::WHILE_STMT,
        SyntaxKind::FOR_STMT,
        SyntaxKind::FOR_EACH_STMT,
        SyntaxKind::TRY_STMT,
        SyntaxKind::RETURN_STMT,
        SyntaxKind::RAISE_STMT,
        SyntaxKind::EXECUTE_STMT,
        SyntaxKind::GOTO_STMT,
        SyntaxKind::LABEL_STMT,
        SyntaxKind::ADD_HANDLER_STMT,
        SyntaxKind::REMOVE_HANDLER_STMT,
    ];

    let mut stmts = Vec::new();
    for node in root.children() {
        if stmt_kinds.contains(&node.kind()) {
            if let Some(stmt_id) = lower_stmt(&mut ctx, &node) {
                stmts.push(stmt_id);
            }
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

/// Check if a statement list contains at least one return statement.
fn has_return_statement(stmt_list: &SyntaxNode) -> bool {
    stmt_list.descendants().any(|n| n.kind() == SyntaxKind::RETURN_STMT)
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

    let binding = Binding::new(Name::new(name_token.text()), is_val);
    Some(ctx.alloc_binding(binding, name_token.text_range()))
}

/// Lower a statement list.
fn lower_stmt_list(ctx: &mut LoweringCtx, stmt_list: &SyntaxNode) -> Vec<StmtId> {
    let mut stmts = Vec::new();

    for child in stmt_list.children() {
        if let Some(stmt_id) = lower_stmt(ctx, &child) {
            stmts.push(stmt_id);
        }
    }

    stmts
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
        // Register implicit variable if not already declared
        if !ctx.local_vars.contains_key(&key) {
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

    // Then branch (STMT_LIST)
    let then_branch = children
        .next()
        .filter(|n| n.kind() == SyntaxKind::STMT_LIST)
        .map(|n| lower_stmt_list(ctx, &n))
        .unwrap_or_default();

    // Check for empty then branch
    if then_branch.is_empty() {
        if let Some(stmt_list) = node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST) {
            ctx.emit(BodyDiagnostic::EmptyCodeBlock { range: stmt_list.text_range() });
        }
    }

    // Elsif branches
    let mut elsif_branches = Vec::new();
    for elsif in node.children().filter(|n| n.kind() == SyntaxKind::ELSIF_CLAUSE) {
        let mut elsif_children = elsif.children();
        if let Some(cond_node) = elsif_children.next() {
            let cond = lower_expr_node(ctx, &cond_node);
            let body = elsif_children
                .find(|n| n.kind() == SyntaxKind::STMT_LIST)
                .map(|n| lower_stmt_list(ctx, &n))
                .unwrap_or_default();

            // Check for empty elsif branch
            if body.is_empty() {
                if let Some(stmt_list) =
                    elsif.children().find(|n| n.kind() == SyntaxKind::STMT_LIST)
                {
                    ctx.emit(BodyDiagnostic::EmptyCodeBlock { range: stmt_list.text_range() });
                }
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

                stmts.into_boxed_slice()
            })
        });

    Some(Stmt::If {
        condition,
        then_branch: then_branch.into_boxed_slice(),
        elsif_branches: elsif_branches.into_boxed_slice(),
        else_branch,
    })
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

    ctx.alloc_expr(expr, range)
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
            let text = token.text();
            // Remove quotes
            let value = text.trim_start_matches('"').trim_end_matches('"').to_string();
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
    let callee = lower_expr_node(ctx, &callee_node);

    // Arguments
    let args = children
        .find(|n| n.kind() == SyntaxKind::ARG_LIST)
        .map(|arg_list| lower_arg_list(ctx, &arg_list))
        .unwrap_or_default();

    Expr::Call { callee, args: args.into_boxed_slice() }
}

/// Lower argument list.
fn lower_arg_list(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Vec<ExprId> {
    node.children().map(|n| lower_expr_node(ctx, &n)).collect()
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
    if node.children().any(|n| n.kind() == SyntaxKind::ARG_LIST) {
        let args = node
            .children()
            .find(|n| n.kind() == SyntaxKind::ARG_LIST)
            .map(|arg_list| lower_arg_list(ctx, &arg_list))
            .unwrap_or_default();

        Expr::MethodCall { receiver: base, method: field_name, args: args.into_boxed_slice() }
    } else {
        Expr::Field { base, field: field_name }
    }
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
}
