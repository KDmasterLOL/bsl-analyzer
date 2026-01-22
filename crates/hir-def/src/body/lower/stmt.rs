//! Statement lowering.
//!
//! This module handles lowering of BSL statements from AST to HIR.

use syntax::{SyntaxKind, SyntaxNode};
use text_size::TextRange;

use crate::body::BodyDiagnostic;
use crate::hir::{Binding, BindingIdx, Expr, Stmt, StmtIdx};
use crate::{Name, StmtId};
use cfg_types::IdConversion;

use super::control_flow::{
    find_first_unreachable_stmt, if_all_branches_terminate, is_control_flow_terminator,
    is_statement_node,
};
use super::diagnostics::{
    check_commit_transaction_in_try, check_duplicated_code_blocks, extend_range_with_semicolon,
    is_global_begin_transaction_call, is_global_commit_transaction_call, is_inside_try_body,
};
use super::expr::{exprs_are_equal, lower_expr_node};
use super::preproc::{
    preproc_if_all_branches_terminate, preproc_region_terminates, process_preproc_if,
    process_preproc_region,
};
use super::LoweringCtx;

/// Find the range for IF/THEN header (from IF to THEN keyword).
fn find_if_then_range(if_stmt: &SyntaxNode) -> TextRange {
    let mut start = None;
    let mut end = None;

    for token in if_stmt.descendants_with_tokens().filter_map(|el| el.into_token()) {
        if matches!(token.kind(), SyntaxKind::KW_IF) && start.is_none() {
            start = Some(token.text_range().start());
        }
        if matches!(token.kind(), SyntaxKind::KW_THEN) && start.is_some() && end.is_none() {
            end = Some(token.text_range().end());
            break;
        }
    }

    match (start, end) {
        (Some(s), Some(e)) => TextRange::new(s, e),
        _ => if_stmt.text_range(),
    }
}

/// Find the range for ELSIF/THEN header (from ELSIF to THEN keyword).
fn find_elsif_then_range(elsif_clause: &SyntaxNode) -> TextRange {
    let mut start = None;
    let mut end = None;

    for token in elsif_clause.descendants_with_tokens().filter_map(|el| el.into_token()) {
        if matches!(token.kind(), SyntaxKind::KW_ELSIF) && start.is_none() {
            start = Some(token.text_range().start());
        }
        if matches!(token.kind(), SyntaxKind::KW_THEN) && start.is_some() && end.is_none() {
            end = Some(token.text_range().end());
            break;
        }
    }

    match (start, end) {
        (Some(s), Some(e)) => TextRange::new(s, e),
        _ => elsif_clause.text_range(),
    }
}

/// Find the range for ELSE keyword.
fn find_else_range(else_clause: &SyntaxNode) -> TextRange {
    for token in else_clause.descendants_with_tokens().filter_map(|el| el.into_token()) {
        if matches!(token.kind(), SyntaxKind::KW_ELSE) {
            return token.text_range();
        }
    }
    else_clause.text_range()
}

/// Count boolean operations (AND/OR) in an expression.
///
/// This matches Java's `Trees.findAllRuleNodes(expression, BSLParser.RULE_boolOperation).size()`
/// Complexity = number of boolean operations + 1
fn count_bool_operations(expr_node: &SyntaxNode) -> usize {
    let mut count = 0;

    // Traverse all descendants looking for BINARY_EXPR with AND/OR
    for node in expr_node.descendants() {
        if node.kind() == SyntaxKind::BINARY_EXPR {
            // Check if it has AND or OR operator
            let has_bool_op = node.children_with_tokens().any(|child| {
                child
                    .as_token()
                    .is_some_and(|tok| matches!(tok.kind(), SyntaxKind::KW_AND | SyntaxKind::KW_OR))
            });
            if has_bool_op {
                count += 1;
            }
        }
    }

    count
}

/// Get condition range, trimming trailing whitespace.
///
/// Java ANTLR parser doesn't include trailing whitespace in expression nodes,
/// but Rowan CST includes all tokens. This trims trailing whitespace to match Java.
fn get_condition_range(expr_node: &SyntaxNode) -> TextRange {
    let text = expr_node.text().to_string();
    let trimmed = text.trim_end();
    let trimmed_len = trimmed.len();
    let original_len = text.len();

    if trimmed_len == original_len {
        expr_node.text_range()
    } else {
        let start = expr_node.text_range().start();
        let end = start + text_size::TextSize::from(trimmed_len as u32);
        TextRange::new(start, end)
    }
}

/// Check condition complexity and emit diagnostic if too complex.
///
/// Default max complexity is 3 (hardcoded here as we don't have config during lowering).
/// The actual config check happens in from_hir().
fn check_condition_complexity(ctx: &mut LoweringCtx, condition_node: &SyntaxNode) {
    // Default max complexity (matches Java default)
    const DEFAULT_MAX_COMPLEXITY: usize = 3;

    let bool_op_count = count_bool_operations(condition_node);
    let complexity = bool_op_count + 1;

    // Always emit if complexity > default threshold
    // from_hir() will check against user config and filter if needed
    if complexity > DEFAULT_MAX_COMPLEXITY {
        let range = get_condition_range(condition_node);
        ctx.emit(BodyDiagnostic::IfConditionComplexity {
            complexity,
            max_complexity: DEFAULT_MAX_COMPLEXITY,
            range,
        });
    }
}

/// Normalize condition text for comparison.
///
/// Matches Java behavior:
/// - Remove whitespace (except inside string literals)
/// - Convert to lowercase (except inside string literals)
/// - String literals remain case-sensitive
fn normalize_condition(condition: &str) -> String {
    let mut result = String::new();
    let mut in_string = false;

    for ch in condition.chars() {
        // Track string literal boundaries
        if ch == '"' {
            in_string = !in_string;
            result.push(ch);
            continue;
        }

        // Skip whitespace (unless inside string)
        if ch.is_whitespace() && !in_string {
            continue;
        }

        // Inside string: preserve case and all characters
        if in_string {
            result.push(ch);
        } else {
            // Outside string: convert to lowercase (case-insensitive identifiers)
            result.push(ch.to_lowercase().next().unwrap_or(ch));
        }
    }

    result
}

/// Check for duplicated conditions in if/elsif chain.
///
/// Detects when an elsif condition is identical to a previous if/elsif condition.
/// Reports diagnostics on duplicate occurrences (not the first one).
fn check_duplicated_conditions(ctx: &mut LoweringCtx, condition_nodes: &[SyntaxNode]) {
    // Early exit: need at least 2 conditions (if + elsif) to have potential duplicates
    if condition_nodes.len() < 2 {
        return;
    }

    use std::collections::HashMap;

    // Map: normalized condition text -> list of (index, node)
    let mut condition_map: HashMap<String, Vec<(usize, &SyntaxNode)>> = HashMap::new();

    // Group conditions by normalized text
    for (i, node) in condition_nodes.iter().enumerate() {
        let text = node.text().to_string();
        let normalized = normalize_condition(&text);
        condition_map.entry(normalized).or_default().push((i, node));
    }

    // Report diagnostics for duplicated conditions
    for (_normalized_text, occurrences) in condition_map {
        if occurrences.len() > 1 {
            // Report diagnostic on each duplicate (not the first one)
            // This matches Java behavior: first occurrence is the reference
            for (_idx, node) in occurrences.iter().skip(1) {
                let range = node.text_range();
                ctx.emit(BodyDiagnostic::IfElseDuplicatedCondition {
                    first_occurrence_index: occurrences[0].0,
                    range,
                });
            }
        }
    }
}

/// Find the range for WHILE/DO header (from WHILE to DO keyword).
fn find_while_do_range(while_stmt: &SyntaxNode) -> TextRange {
    let mut start = None;
    let mut end = None;

    for token in while_stmt.descendants_with_tokens().filter_map(|el| el.into_token()) {
        if matches!(token.kind(), SyntaxKind::KW_WHILE) && start.is_none() {
            start = Some(token.text_range().start());
        }
        if matches!(token.kind(), SyntaxKind::KW_DO) && start.is_some() && end.is_none() {
            end = Some(token.text_range().end());
            break;
        }
    }

    match (start, end) {
        (Some(s), Some(e)) => TextRange::new(s, e),
        _ => while_stmt.text_range(),
    }
}

/// Find the range for FOR/DO header (from FOR to DO keyword).
fn find_for_do_range(for_stmt: &SyntaxNode) -> TextRange {
    let mut start = None;
    let mut end = None;

    for token in for_stmt.descendants_with_tokens().filter_map(|el| el.into_token()) {
        if matches!(token.kind(), SyntaxKind::KW_FOR) && start.is_none() {
            start = Some(token.text_range().start());
        }
        if matches!(token.kind(), SyntaxKind::KW_DO) && start.is_some() && end.is_none() {
            end = Some(token.text_range().end());
            break;
        }
    }

    match (start, end) {
        (Some(s), Some(e)) => TextRange::new(s, e),
        _ => for_stmt.text_range(),
    }
}

/// Find the range for FOREACH header (from FOR to DO keyword).
/// ForEach uses "Для Каждого X Из Y Цикл" / "For Each X In Y Do"
fn find_foreach_do_range(foreach_stmt: &SyntaxNode) -> TextRange {
    let mut start = None;
    let mut end = None;

    for token in foreach_stmt.descendants_with_tokens().filter_map(|el| el.into_token()) {
        if matches!(token.kind(), SyntaxKind::KW_FOR) && start.is_none() {
            start = Some(token.text_range().start());
        }
        if matches!(token.kind(), SyntaxKind::KW_DO) && start.is_some() && end.is_none() {
            end = Some(token.text_range().end());
            break;
        }
    }

    match (start, end) {
        (Some(s), Some(e)) => TextRange::new(s, e),
        _ => foreach_stmt.text_range(),
    }
}

/// Lower parameter list.
pub(crate) fn lower_params(ctx: &mut LoweringCtx, param_list: &SyntaxNode) -> Vec<BindingIdx> {
    let mut params = Vec::new();

    for param in param_list.children().filter(|n| n.kind() == SyntaxKind::PARAM) {
        if let Some(binding_id) = lower_param(ctx, &param) {
            params.push(binding_id);
        }
    }

    params
}

/// Lower a single parameter.
fn lower_param(ctx: &mut LoweringCtx, param: &SyntaxNode) -> Option<BindingIdx> {
    let name_token = param
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)?;

    let is_val = param
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::KW_VAL);

    // Register parameter name to distinguish from module names in qualified calls
    ctx.register_param(name_token.text());

    // Track by-ref parameters for FunctionOutParameter diagnostic
    if !is_val {
        ctx.by_ref_param_names.insert(name_token.text().to_lowercase());
    }

    // Check for default value expression
    let default_value = param
        .children()
        .find(|n| n.kind() == SyntaxKind::EXPR)
        .map(|expr_node| lower_expr_node(ctx, &expr_node));

    // Create binding with or without default value
    let binding = if let Some(default_expr_id) = default_value {
        Binding::with_default(Name::new(name_token.text()), is_val, default_expr_id)
    } else {
        Binding::new(Name::new(name_token.text()), is_val)
    };

    let binding_id = ctx.alloc_binding(binding, name_token.text_range());

    // Track by-value parameters for RewriteMethodParameter diagnostic
    if is_val {
        ctx.by_value_params.insert(name_token.text().to_lowercase(), binding_id);
    }

    Some(binding_id)
}

/// Lower a statement list.
///
/// Also detects unreachable code after control flow statements (return, raise, break, continue)
/// and after if-else where all branches terminate.
pub(crate) fn lower_stmt_list(ctx: &mut LoweringCtx, stmt_list: &SyntaxNode) -> Vec<StmtIdx> {
    lower_stmt_list_with_unreachable(ctx, stmt_list, true)
}

/// Lower a statement list with optional unreachable code detection.
///
/// The `emit_diagnostics` parameter controls whether to emit unreachable code diagnostics.
/// This is useful for recursive processing where we want to collect statements but not
/// emit duplicate diagnostics.
pub(super) fn lower_stmt_list_with_unreachable(
    ctx: &mut LoweringCtx,
    stmt_list: &SyntaxNode,
    emit_diagnostics: bool,
) -> Vec<StmtIdx> {
    let mut stmts = Vec::new();
    let mut unreachable_start: Option<TextRange> = None;
    let mut unreachable_end: Option<TextRange> = None;

    // Track pending BeginTransaction node for BeginTransactionBeforeTryCatch diagnostic
    let mut pending_begin_transaction: Option<SyntaxNode> = None;

    // Track nesting level for CommitTransactionOutsideTryCatch diagnostic
    // We only check at top level of method body (not inside nested try-catch)
    let is_top_level = !is_inside_try_body(stmt_list);

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

            // CommitTransactionOutsideTryCatch: Check for CommitTransaction outside try-catch
            if is_top_level && is_global_commit_transaction_call(&child) {
                let extended_range = extend_range_with_semicolon(&child, child.text_range());
                ctx.emit(BodyDiagnostic::CommitTransactionOutsideTryCatch {
                    range: extended_range,
                });
            }
        }

        // Lower the statement
        if let Some(stmt_id) = lower_stmt(ctx, &child) {
            stmts.push(stmt_id);

            // Track statement line for OneStatementPerLine diagnostic
            // Exclude: EMPTY_STMT (handled in lower_stmt), preprocessor, parse errors
            if emit_diagnostics && !should_skip_one_statement_per_line(&child) {
                ctx.track_statement_line(child.text_range());
            }

            // Check for missing semicolon (SemicolonPresence diagnostic)
            if emit_diagnostics
                && !should_skip_semicolon_check(&child)
                && !has_trailing_semicolon(&child)
            {
                let range = get_last_meaningful_token_range(&child);
                ctx.emit(BodyDiagnostic::MissingSemicolon { range });
            }

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

/// Lower a single statement.
pub(crate) fn lower_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<StmtIdx> {
    let range = node.text_range();
    let kind = node.kind();

    let stmt = match kind {
        SyntaxKind::ASSIGN_STMT => lower_assign_stmt(ctx, node),
        SyntaxKind::CALL_STMT => lower_call_stmt(ctx, node),
        SyntaxKind::RETURN_STMT => lower_return_stmt(ctx, node),
        SyntaxKind::IF_STMT => lower_if_stmt(ctx, node),
        SyntaxKind::WHILE_STMT => lower_while_stmt(ctx, node),
        SyntaxKind::FOR_STMT => lower_for_stmt(ctx, node),
        SyntaxKind::FOR_EACH_STMT => lower_for_each_stmt(ctx, node),
        SyntaxKind::TRY_STMT => lower_try_stmt(ctx, node),
        SyntaxKind::VAR_DEF => lower_var_decl(ctx, node),
        SyntaxKind::RAISE_STMT => lower_raise_stmt(ctx, node),
        SyntaxKind::BREAK_STMT => Some(Stmt::Break),
        SyntaxKind::CONTINUE_STMT => Some(Stmt::Continue),
        SyntaxKind::GOTO_STMT => lower_goto_stmt(node),
        SyntaxKind::LABEL_STMT => lower_label_stmt(node),
        SyntaxKind::EXECUTE_STMT => lower_execute_stmt(ctx, node),
        SyntaxKind::ADD_HANDLER_STMT => lower_add_handler_stmt(ctx, node),
        SyntaxKind::REMOVE_HANDLER_STMT => lower_remove_handler_stmt(ctx, node),
        SyntaxKind::EMPTY_STMT => {
            // Check if parent or siblings contain ERROR nodes
            // (Java: !Trees.treeContainsErrors(previousNode))
            let has_error = node
                .parent()
                .map(|p| p.children().any(|c| c.kind() == SyntaxKind::ERROR))
                .unwrap_or(false);

            if !has_error {
                ctx.emit(BodyDiagnostic::EmptyStatement { range });
            }

            return None;
        }
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
    let target_name = if let Expr::Path(name) = ctx.body.expr_idx(target) {
        Some((name.clone(), get_target_range(&target_node)))
    } else {
        None
    };
    if let Some((ref name, range)) = target_name {
        let key = name.as_str().to_lowercase();

        // Emit potential CommonModuleAssign for later validation in from_hir().
        // This will be filtered by metadata check - only names matching CommonModule names
        // will produce actual diagnostics.
        ctx.emit(BodyDiagnostic::CommonModuleAssign {
            variable_name: name.as_str().to_string(),
            range,
        });

        // Check for FunctionOutParameter diagnostic
        // Functions should not modify by-reference parameters
        if ctx.is_function && ctx.by_ref_param_names.contains(&key) {
            ctx.emit(BodyDiagnostic::FunctionOutParameter {
                name: name.as_str().to_string(),
                range,
            });
        }

        // Check for RewriteMethodParameter diagnostic
        // Emit for assignments to byValue parameters - validation with reaching defs happens in from_hir()
        if let Some(&param_id) = ctx.by_value_params.get(&key) {
            // Convert typed BindingIdx to opaque BindingId for diagnostic
            let opaque_param_id = cfg_types::BindingId::from_idx(param_id);
            ctx.emit(BodyDiagnostic::RewriteMethodParameter {
                param_id: opaque_param_id,
                stmt_id: StmtId::from_raw(la_arena::RawIdx::from(0)), // Placeholder - will find via range in handler
                stmt_range: node.text_range(), // Full statement range for BodySourceMap lookup
                ident_range: range, // Identifier range for diagnostic display (Java compatibility)
            });
        }
    }

    // Second child should be value expression (or EXPR wrapper)
    let value_node = children.next()?;
    let value = lower_expr_node(ctx, &value_node);

    // Check for self-assignment (a = a, obj.field = obj.field)
    if exprs_are_equal(&ctx.body, target, value) {
        ctx.emit(BodyDiagnostic::SelfAssign { range: node.text_range() });
    }

    // Track Query/QueryBuilder/ReportBuilder assignments for CreateQueryInCycle diagnostic
    if let Some((target_name, _)) = target_name {
        use super::QueryVarType;

        // Check if value is "New Query()" or similar
        if let Expr::New { type_name: Some(type_name), .. } = ctx.body.expr_idx(value) {
            let type_str = type_name.as_str().to_lowercase();
            let query_type = match type_str.as_str() {
                "запрос" | "query" => QueryVarType::Query,
                "построительзапроса" | "querybuilder" => {
                    QueryVarType::QueryBuilder
                }
                "построительотчета" | "reportbuilder" => {
                    QueryVarType::ReportBuilder
                }
                _ => QueryVarType::Undefined,
            };
            ctx.register_query_var(target_name.as_str().to_string(), query_type);
        } else if let Expr::Path(source_name) = ctx.body.expr_idx(value) {
            // Handle: Запрос2 = Запрос (copy query reference)
            if let Some(source_type) = ctx.get_query_var_type(source_name.as_str()) {
                ctx.register_query_var(target_name.as_str().to_string(), source_type);
            }
        }
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
    if !ctx.is_function && value.is_some() {
        ctx.emit(BodyDiagnostic::ProcedureReturnsValue { range: node.text_range() });
    }

    ctx.return_statements.push(node.text_range());

    Some(Stmt::Return { value })
}

/// Lower if statement.
fn lower_if_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let mut children = node.children().peekable();

    // Condition (first EXPR or expression node)
    let condition_node = children.next()?;

    // Check if condition complexity for IfConditionComplexity diagnostic
    // Complexity = number of boolean operations + 1
    // We emit with actual complexity and let from_hir() check against config
    check_condition_complexity(ctx, &condition_node);

    let condition = lower_expr_node(ctx, &condition_node);

    // Collect all condition nodes for duplicate condition detection
    let mut condition_nodes: Vec<SyntaxNode> = Vec::new();
    condition_nodes.push(condition_node.clone());

    // Collect all branch STMT_LIST nodes for duplicate detection
    let mut branch_nodes: Vec<SyntaxNode> = Vec::new();

    // Then branch (STMT_LIST)
    let then_stmt_list = children.next().filter(|n| n.kind() == SyntaxKind::STMT_LIST);
    let then_branch = then_stmt_list.as_ref().map(|n| lower_stmt_list(ctx, n)).unwrap_or_default();

    // Check for empty then branch
    if then_branch.is_empty() && then_stmt_list.is_some() {
        let range = find_if_then_range(node);
        ctx.emit(BodyDiagnostic::EmptyCodeBlock { range });
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
            // Check elsif condition complexity
            check_condition_complexity(ctx, &cond_node);

            // Collect elsif condition for duplicate detection
            condition_nodes.push(cond_node.clone());

            let cond = lower_expr_node(ctx, &cond_node);
            let stmt_list_node = elsif_children.find(|n| n.kind() == SyntaxKind::STMT_LIST);
            let body = stmt_list_node.as_ref().map(|n| lower_stmt_list(ctx, n)).unwrap_or_default();

            // Check for empty elsif branch
            if body.is_empty() && stmt_list_node.is_some() {
                let range = find_elsif_then_range(&elsif);
                ctx.emit(BodyDiagnostic::EmptyCodeBlock { range });
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
                    let range = find_else_range(&else_clause);
                    ctx.emit(BodyDiagnostic::EmptyCodeBlock { range });
                }

                // Add else branch to branch_nodes for duplicate detection
                branch_nodes.push(n.clone());

                stmts.into_boxed_slice()
            })
        });

    // Check for duplicated code blocks
    check_duplicated_code_blocks(ctx, &branch_nodes);

    // Check for duplicated conditions
    check_duplicated_conditions(ctx, &condition_nodes);

    // Check for elsif without else (IfElseIfEndsWithElse diagnostic)
    // Must have elsif but no else
    if !elsif_branches.is_empty() && else_branch.is_none() {
        // Find КонецЕсли/EndIf token for the diagnostic range
        if let Some(endif_token) = node
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == SyntaxKind::KW_END_IF)
        {
            let range = endif_token.text_range();
            ctx.emit(BodyDiagnostic::IfElseIfEndsWithElse { range });
        }
    }

    Some(Stmt::If(Box::new(crate::hir::IfStmt {
        condition,
        then_branch: then_branch.into_boxed_slice(),
        elsif_branches: elsif_branches.into_boxed_slice(),
        else_branch,
    })))
}

/// Lower while statement.
fn lower_while_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let mut children = node.children();

    let condition_node = children.next()?;
    let condition = lower_expr_node(ctx, &condition_node);

    // Enter loop scope for CreateQueryInCycle diagnostic
    ctx.enter_loop();

    let body = children
        .find(|n| n.kind() == SyntaxKind::STMT_LIST)
        .map(|n| {
            let stmts = lower_stmt_list(ctx, &n);

            // Check for empty while body
            if stmts.is_empty() {
                let range = find_while_do_range(node);
                ctx.emit(BodyDiagnostic::EmptyCodeBlock { range });
            }

            stmts.into_boxed_slice()
        })
        .unwrap_or_default();

    // Leave loop scope
    ctx.leave_loop();

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

    // Enter loop scope for CreateQueryInCycle diagnostic
    ctx.enter_loop();

    let body = node
        .children()
        .find(|n| n.kind() == SyntaxKind::STMT_LIST)
        .map(|n| {
            let stmts = lower_stmt_list(ctx, &n);

            // Check for empty for body
            if stmts.is_empty() {
                let range = find_for_do_range(node);
                ctx.emit(BodyDiagnostic::EmptyCodeBlock { range });
            }

            stmts.into_boxed_slice()
        })
        .unwrap_or_default();

    // Leave loop scope
    ctx.leave_loop();

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

    let var = ctx.alloc_binding(Binding::var(name), range);

    // Collection is the first expression child
    let collection_node = node.children().find(|n| {
        matches!(
            n.kind(),
            SyntaxKind::EXPR
                | SyntaxKind::CALL_EXPR
                | SyntaxKind::FIELD_EXPR
                | SyntaxKind::INDEX_EXPR
        )
    });

    let collection = collection_node
        .as_ref()
        .map(|n| lower_expr_node(ctx, n))
        .unwrap_or_else(|| ctx.missing_expr());

    // Extract collection text for diagnostic message (preserve original AST text)
    let collection_text = collection_node
        .as_ref()
        .map(|n| n.text().to_string())
        .unwrap_or_else(|| String::from("<unknown>"));

    // Enter loop scope for CreateQueryInCycle diagnostic
    ctx.enter_loop();

    // Enter ForEach context for DeletingCollectionItem diagnostic
    ctx.enter_foreach(collection, collection_text);

    let body = node
        .children()
        .find(|n| n.kind() == SyntaxKind::STMT_LIST)
        .map(|n| {
            let stmts = lower_stmt_list(ctx, &n);

            // Check for empty for-each body
            if stmts.is_empty() {
                let range = find_foreach_do_range(node);
                ctx.emit(BodyDiagnostic::EmptyCodeBlock { range });
            }

            stmts.into_boxed_slice()
        })
        .unwrap_or_default();

    // Leave ForEach context
    ctx.leave_foreach();

    // Leave loop scope
    ctx.leave_loop();

    Some(Stmt::ForEach { var, collection, body })
}

/// Lower try statement.
fn lower_try_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    // Check CommitTransaction placement within this try-catch
    let violations = check_commit_transaction_in_try(node);
    for (commit_node, _violation) in violations {
        let extended_range = extend_range_with_semicolon(&commit_node, commit_node.text_range());
        ctx.emit(BodyDiagnostic::CommitTransactionOutsideTryCatch { range: extended_range });
    }

    let body = node
        .children()
        .find(|n| n.kind() == SyntaxKind::STMT_LIST)
        .map(|n| lower_stmt_list(ctx, &n).into_boxed_slice())
        .unwrap_or_default();

    let except = node
        .children()
        .find(|n| n.kind() == SyntaxKind::EXCEPT_CLAUSE)
        .and_then(|except_clause| {
            except_clause
                .children()
                .find(|n| n.kind() == SyntaxKind::STMT_LIST)
                .map(|n| lower_stmt_list(ctx, &n).into_boxed_slice())
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
fn lower_goto_stmt(node: &SyntaxNode) -> Option<Stmt> {
    let label_token = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)?;

    Some(Stmt::Goto(Name::new(label_token.text())))
}

/// Lower label statement.
fn lower_label_stmt(node: &SyntaxNode) -> Option<Stmt> {
    let label_token = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)?;

    Some(Stmt::Label(Name::new(label_token.text())))
}

/// Lower execute statement.
fn lower_execute_stmt(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    // Execute statement is forbidden on server (security risk)
    if !ctx.is_client_only {
        let range = node.text_range();
        ctx.emit(BodyDiagnostic::ExecuteExternalCode { range });
    }

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

/// Check if a statement should be skipped for OneStatementPerLine diagnostic.
///
/// Matches Java behavior - excludes:
/// 1. Empty statements (standalone `;`)
/// 2. Statements containing preprocessor directives
/// 3. Statements with parse errors
pub(crate) fn should_skip_one_statement_per_line(node: &SyntaxNode) -> bool {
    // 1. Empty statement (EMPTY_STMT) - already filtered by lower_stmt returning None
    // But we double-check here in case called directly
    if node.kind() == SyntaxKind::EMPTY_STMT {
        return true;
    }

    // 2. Contains preprocessor directive (check for any PRE_* nodes)
    if node.descendants().any(|n| {
        matches!(
            n.kind(),
            SyntaxKind::PRE_IF_DIR
                | SyntaxKind::PRE_ELSIF_CLAUSE
                | SyntaxKind::PRE_ELSE_CLAUSE
                | SyntaxKind::PRE_REGION_DIR
                | SyntaxKind::PRE_DELETE_DIR
                | SyntaxKind::PRE_INSERT_DIR
        )
    }) {
        return true;
    }

    // 3. Contains parse error
    if node.descendants().any(|n| n.kind() == SyntaxKind::ERROR) {
        return true;
    }

    false
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

        // Register local variable to distinguish from module names in qualified calls
        ctx.register_local_var(name.clone(), range);

        let binding_id = ctx.alloc_binding(Binding::var(name), range);
        bindings.push(binding_id);
    }

    if bindings.is_empty() {
        return None;
    }

    Some(Stmt::VarDecl { bindings: bindings.into_boxed_slice() })
}

/// Check if a statement should be skipped for SemicolonPresence diagnostic.
///
/// Skip:
/// - EMPTY_STMT (this is the semicolon itself)
/// - LABEL_STMT (~Label: doesn't require semicolon)
/// - Statements with parse errors
pub(super) fn should_skip_semicolon_check(node: &SyntaxNode) -> bool {
    if matches!(node.kind(), SyntaxKind::EMPTY_STMT | SyntaxKind::LABEL_STMT) {
        return true;
    }
    node.descendants().any(|n| n.kind() == SyntaxKind::ERROR)
}

/// Check if a statement has a trailing semicolon token.
///
/// IMPORTANT: In our parser, SEMICOLON is consumed AFTER m.complete() for most
/// statements (ASSIGN_STMT, CALL_STMT, etc.), so it's NOT a child of the statement.
/// It becomes the next sibling in STMT_LIST.
///
/// For compound statements (IF_STMT, WHILE_STMT, etc.), SEMICOLON is consumed
/// BEFORE m.complete(), so it's a DIRECT child token of the statement node.
///
/// This function checks both cases.
pub(super) fn has_trailing_semicolon(node: &SyntaxNode) -> bool {
    // First check next sibling (for simple statements like ASSIGN_STMT)
    // Skip whitespace/newlines to find the actual next token
    use syntax::NodeOrToken;
    let mut next = node.next_sibling_or_token();
    while let Some(element) = next {
        match element {
            NodeOrToken::Token(ref token) => {
                if token.kind() == SyntaxKind::SEMICOLON {
                    return true;
                }
                if !matches!(token.kind(), SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE) {
                    // Found non-whitespace token that's not semicolon, stop searching
                    break;
                }
            }
            NodeOrToken::Node(_) => {
                // Found next node (next statement), stop searching siblings
                break;
            }
        }
        next = element.next_sibling_or_token();
    }

    // Then check direct children tokens (for compound statements like IF_STMT)
    // We only check direct children, NOT descendants, because descendants would
    // find semicolons inside nested branches (e.g., "А = 0;" inside IF body)
    node.children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|t| t.kind() == SyntaxKind::SEMICOLON)
}

/// Get the range of the last meaningful token in a statement.
///
/// Skips whitespace, newlines, and comments.
/// Uses descendants_with_tokens() to find tokens at any depth.
/// Used for SemicolonPresence diagnostic to match Java behavior (ctx.getStop()).
pub(super) fn get_last_meaningful_token_range(node: &SyntaxNode) -> TextRange {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| {
            !matches!(t.kind(), SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT)
        })
        .last()
        .map(|t| t.text_range())
        .unwrap_or_else(|| node.text_range())
}
