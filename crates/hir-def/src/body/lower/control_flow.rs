//! Control flow analysis for return path checking and unreachable code detection.
//!
//! This module provides functions for analyzing control flow in BSL code,
//! including return path analysis and detection of unreachable code.

use syntax::{SyntaxKind, SyntaxNode};
use text_size::TextRange;

/// Check if a statement list contains at least one return statement.
pub(crate) fn has_return_statement(stmt_list: &SyntaxNode) -> bool {
    stmt_list.descendants().any(|n| n.kind() == SyntaxKind::RETURN_STMT)
}

/// Check if function has missing return paths using CFG analysis.
///
/// Returns true if some execution paths don't have explicit return statements.
/// This uses the same CFG analysis as AllFunctionPathMustHaveReturn diagnostic.
pub fn check_missing_return_paths(stmt_list: &SyntaxNode) -> bool {
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

/// Find the first statement after a control flow terminator.
pub(crate) fn find_first_unreachable_stmt(
    stmt_list: &SyntaxNode,
    after_range: TextRange,
) -> Option<TextRange> {
    for child in stmt_list.children() {
        // Skip empty statements - they shouldn't be reported as unreachable
        if child.kind() == SyntaxKind::EMPTY_STMT {
            continue;
        }
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

/// Find the first unreachable node at module root level.
pub(crate) fn find_first_unreachable_at_root(
    root: &SyntaxNode,
    after_range: TextRange,
) -> Option<TextRange> {
    for child in root.children() {
        // Skip empty statements - they shouldn't be reported as unreachable
        if child.kind() == SyntaxKind::EMPTY_STMT {
            continue;
        }
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
