//! Preprocessor directive handling.
//!
//! This module handles lowering of BSL preprocessor directives (#Если, #Область, etc.)
//! and analyzes them for unreachable code detection.

use syntax::{SyntaxKind, SyntaxNode};
use text_size::TextRange;

use crate::body::BodyDiagnostic;

use super::control_flow::{
    if_all_branches_terminate, is_control_flow_terminator, is_statement_node, stmt_list_terminates,
};
use super::stmt::{lower_stmt, lower_stmt_list_with_unreachable};
use super::LoweringCtx;

/// Process preprocessor `#Если` directive, analyzing each branch for unreachable code.
pub(crate) fn process_preproc_if(ctx: &mut LoweringCtx, node: &SyntaxNode) {
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
pub(crate) fn process_preproc_region(ctx: &mut LoweringCtx, node: &SyntaxNode) {
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

/// Check if a preprocessor #Если directive has all branches terminating.
///
/// For code after #КонецЕсли to be unreachable, ALL branches must terminate:
/// - The main branch (after #Если ... Тогда)
/// - All #ИначеЕсли branches
/// - The #Иначе branch (must exist)
pub(crate) fn preproc_if_all_branches_terminate(node: &SyntaxNode) -> bool {
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

/// Check if a preprocessor region ends with a terminator.
pub(crate) fn preproc_region_terminates(region: &SyntaxNode) -> bool {
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
