//! Control flow analysis for return path checking and unreachable code detection.
//!
//! This module provides functions for analyzing control flow in BSL code,
//! including return path analysis and detection of unreachable code.

use syntax::{SyntaxKind, SyntaxNode};

/// Result of combined control flow analysis.
pub(crate) struct ControlFlowAnalysis {
    /// Whether the statement list contains at least one return statement.
    pub has_return: bool,
    /// All CALL_STMT nodes found in the statement list (for async call checking).
    pub call_stmts: Vec<SyntaxNode>,
}

/// Perform combined control flow analysis in a single AST traversal.
///
/// This function does a single `descendants()` pass to collect:
/// - Whether any return statement exists (for FunctionShouldHaveReturn check)
/// - All CALL_STMT nodes (for CodeAfterAsyncCall check)
///
/// This avoids two separate tree traversals.
pub(crate) fn analyze_control_flow(stmt_list: &SyntaxNode) -> ControlFlowAnalysis {
    let mut has_return = false;
    let mut call_stmts = Vec::new();

    for node in stmt_list.descendants() {
        match node.kind() {
            SyntaxKind::RETURN_STMT => {
                has_return = true;
            }
            SyntaxKind::CALL_STMT => {
                call_stmts.push(node);
            }
            _ => {}
        }
    }

    ControlFlowAnalysis { has_return, call_stmts }
}

/// Check if a node is a statement (vs whitespace, comments, etc.)
pub(crate) fn is_statement_node(node: &SyntaxNode) -> bool {
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
            | SyntaxKind::EMPTY_STMT
            // ERROR statements are lowered best-effort by
            // `try_lower_recovered_expr_stmt` (see `stmt.rs`) so the editor
            // can reason about expressions the user is still typing. Keeps
            // `stmt_list_terminates` honest too — an ERROR is never itself a
            // control-flow terminator.
            | SyntaxKind::ERROR
    )
}

/// Check if a statement terminates control flow (making subsequent code unreachable).
pub(crate) fn is_control_flow_terminator(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::RETURN_STMT
            | SyntaxKind::RAISE_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::GOTO_STMT
    )
}

/// Check if an if-statement has all branches terminating (with return/raise).
///
/// This returns true only if:
/// 1. The if-statement has an else branch
/// 2. All branches (then, elsif*, else) end with a terminator or another if-all-branches-terminate
pub(crate) fn if_all_branches_terminate(node: &SyntaxNode) -> bool {
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
pub(crate) fn stmt_list_terminates(stmt_list: &SyntaxNode) -> bool {
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
            // Import from preproc module to avoid circular dependency
            super::preproc::preproc_if_all_branches_terminate(&node)
        }
        _ => false,
    }
}
