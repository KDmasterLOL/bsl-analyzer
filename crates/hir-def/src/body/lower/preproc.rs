//! Preprocessor directive handling.
//!
//! This module handles lowering of BSL preprocessor directives (#Если, #Область, etc.)
//! and analyzes them for unreachable code detection.

use syntax::{SyntaxKind, SyntaxNode};
use text_size::TextRange;

use crate::hir::{PreprocIfStmt, Stmt, StmtIdx};

use super::control_flow::{
    if_all_branches_terminate, is_control_flow_terminator, is_statement_node, stmt_list_terminates,
};
use super::stmt::lower_stmt;
use super::LoweringCtx;

/// Lower preprocessor `#Если` directive to `Stmt::PreprocIf`.
///
/// Creates HIR representation that preserves preprocessor structure for CFG analysis.
/// Also detects unreachable code within preprocessor branches.
///
/// Note: Unlike normal code blocks, preprocessor content doesn't have a STMT_LIST wrapper.
/// Statements are direct children of PRE_IF_DIR/PRE_ELSIF_CLAUSE/PRE_ELSE_CLAUSE.
pub(crate) fn lower_preproc_if(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    // Get condition range (PRE_EXPR)
    let condition_range = node
        .children()
        .find(|n| n.kind() == SyntaxKind::PRE_EXPR)
        .map(|n| n.text_range())
        .unwrap_or_else(|| node.text_range());

    // Get directive range (full #Если ... Тогда line)
    let directive_range = get_directive_header_range(node);

    // Then branch - statements are direct children of PRE_IF_DIR
    // (not wrapped in STMT_LIST like normal code blocks)
    let then_branch = lower_preproc_branch_stmts(ctx, node);

    // Elsif branches
    let mut elsif_branches = Vec::new();
    for elsif in node.children().filter(|n| n.kind() == SyntaxKind::PRE_ELSIF_CLAUSE) {
        let elsif_cond_range = elsif
            .children()
            .find(|n| n.kind() == SyntaxKind::PRE_EXPR)
            .map(|n| n.text_range())
            .unwrap_or_else(|| elsif.text_range());

        let elsif_directive_range = get_directive_header_range(&elsif);
        let elsif_body = lower_preproc_branch_stmts(ctx, &elsif);
        elsif_branches.push((
            elsif_cond_range,
            elsif_directive_range,
            elsif_body.into_boxed_slice(),
        ));
    }

    // Else branch
    let else_branch = node
        .children()
        .find(|n| n.kind() == SyntaxKind::PRE_ELSE_CLAUSE)
        .map(|else_node| lower_preproc_branch_stmts(ctx, &else_node).into_boxed_slice());

    Some(Stmt::PreprocIf(Box::new(PreprocIfStmt {
        condition_range,
        directive_range,
        full_range: node.text_range(),
        then_branch: then_branch.into_boxed_slice(),
        elsif_branches: elsif_branches.into_boxed_slice(),
        else_branch,
    })))
}

/// Get the range of the directive header (e.g., `#Если Клиент Тогда` or `#ИначеЕсли Сервер Тогда`).
///
/// This finds the first newline in the node and returns the range up to it,
/// or the whole node range if no newline found.
fn get_directive_header_range(node: &SyntaxNode) -> TextRange {
    let text = node.text().to_string();
    if let Some(newline_pos) = text.find('\n') {
        let start = node.text_range().start();
        TextRange::new(start, start + text_size::TextSize::from(newline_pos as u32))
    } else {
        node.text_range()
    }
}

/// Lower statements from a `#Область` directive.
///
/// Unlike `#Если`, `#Область` is transparent for control flow - statements inside
/// are lowered and added to the parent body. The region boundary doesn't affect
/// statement sequencing.
///
/// Returns the lowered statements and whether the region terminates (ends with return/raise).
pub(crate) fn lower_region_stmts(
    ctx: &mut LoweringCtx,
    region_node: &SyntaxNode,
) -> (Vec<StmtIdx>, bool) {
    // Track 2 Phase C §3.4: `EmptyRegion` is now classified at
    // `RegionTree` construction time (`RegionData::is_empty`); the
    // §3.4 handler reads `ctx.region_tree()` directly. Lowering no
    // longer emits a `BodyDiagnostic::EmptyRegion`.
    let mut stmts: Vec<StmtIdx> = Vec::new();

    for child in region_node.children() {
        // Handle nested preprocessor directives
        if child.kind() == SyntaxKind::PRE_IF_DIR {
            if let Some(stmt) = lower_preproc_if(ctx, &child) {
                let stmt_id = ctx.alloc_stmt(stmt, child.text_range());
                stmts.push(stmt_id);
            }
            continue;
        }

        // Handle nested regions (recursively)
        if child.kind() == SyntaxKind::PRE_REGION_DIR {
            let (nested_stmts, _nested_terminates) = lower_region_stmts(ctx, &child);
            stmts.extend(nested_stmts);
            continue;
        }

        // Skip non-statement nodes
        if !is_statement_node(&child) {
            continue;
        }

        // Lower the statement
        if let Some(stmt_id) = lower_stmt(ctx, &child) {
            stmts.push(stmt_id);
        }
    }

    // Determine if region terminates (last statement is a terminator)
    let terminates = preproc_region_terminates(region_node);

    (stmts, terminates)
}

/// Lower statements from a preprocessor branch node.
///
/// Preprocessor content doesn't use STMT_LIST - statements are direct children.
/// This function iterates over children, lowers statements, and detects unreachable code.
fn lower_preproc_branch_stmts(
    ctx: &mut LoweringCtx,
    branch_node: &SyntaxNode,
) -> Vec<crate::hir::StmtIdx> {
    use crate::hir::StmtIdx;

    let mut stmts: Vec<StmtIdx> = Vec::new();

    for child in branch_node.children() {
        // Skip preprocessor-specific nodes (condition, nested clauses)
        match child.kind() {
            SyntaxKind::PRE_EXPR | SyntaxKind::PRE_ELSIF_CLAUSE | SyntaxKind::PRE_ELSE_CLAUSE => {
                continue
            }
            _ => {}
        }

        // Handle nested preprocessor directives
        if child.kind() == SyntaxKind::PRE_IF_DIR {
            if let Some(stmt) = lower_preproc_if(ctx, &child) {
                let stmt_id = ctx.alloc_stmt(stmt, child.text_range());
                stmts.push(stmt_id);
            }
            continue;
        }

        if child.kind() == SyntaxKind::PRE_REGION_DIR {
            // Lower statements from region (adds them to body)
            let (region_stmts, _region_terminates) = lower_region_stmts(ctx, &child);
            stmts.extend(region_stmts);
            continue;
        }

        // Skip non-statement nodes
        if !is_statement_node(&child) {
            continue;
        }

        // Lower the statement
        if let Some(stmt_id) = lower_stmt(ctx, &child) {
            stmts.push(stmt_id);
        }
    }

    stmts
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
