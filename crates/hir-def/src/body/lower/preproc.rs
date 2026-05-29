use syntax::{SyntaxKind, SyntaxNode};
use text_size::TextRange;

use crate::hir::{PreprocIfStmt, Stmt, StmtIdx};

use super::control_flow::{
    if_all_branches_terminate, is_control_flow_terminator, is_statement_node, stmt_list_terminates,
};
use super::stmt::lower_stmt;
use super::LoweringCtx;

pub(crate) fn lower_preproc_if(ctx: &mut LoweringCtx, node: &SyntaxNode) -> Option<Stmt> {
    let condition_range = node
        .children()
        .find(|n| n.kind() == SyntaxKind::PRE_EXPR)
        .map(|n| n.text_range())
        .unwrap_or_else(|| node.text_range());

    let directive_range = get_directive_header_range(node);

    let then_branch = lower_preproc_branch_stmts(ctx, node);

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

fn get_directive_header_range(node: &SyntaxNode) -> TextRange {
    let text = node.text().to_string();
    if let Some(newline_pos) = text.find('\n') {
        let start = node.text_range().start();
        TextRange::new(start, start + text_size::TextSize::from(newline_pos as u32))
    } else {
        node.text_range()
    }
}

pub(crate) fn lower_region_stmts(
    ctx: &mut LoweringCtx,
    region_node: &SyntaxNode,
) -> (Vec<StmtIdx>, bool) {
    let mut stmts: Vec<StmtIdx> = Vec::new();

    for child in region_node.children() {
        if child.kind() == SyntaxKind::PRE_IF_DIR {
            if let Some(stmt) = lower_preproc_if(ctx, &child) {
                let stmt_id = ctx.alloc_stmt(stmt, child.text_range());
                stmts.push(stmt_id);
            }
            continue;
        }

        if child.kind() == SyntaxKind::PRE_REGION_DIR {
            let (nested_stmts, _nested_terminates) = lower_region_stmts(ctx, &child);
            stmts.extend(nested_stmts);
            continue;
        }

        if !is_statement_node(&child) {
            continue;
        }

        if let Some(stmt_id) = lower_stmt(ctx, &child) {
            stmts.push(stmt_id);
        }
    }

    let terminates = preproc_region_terminates(region_node);

    (stmts, terminates)
}

fn lower_preproc_branch_stmts(
    ctx: &mut LoweringCtx,
    branch_node: &SyntaxNode,
) -> Vec<crate::hir::StmtIdx> {
    use crate::hir::StmtIdx;

    let mut stmts: Vec<StmtIdx> = Vec::new();

    for child in branch_node.children() {
        match child.kind() {
            SyntaxKind::PRE_EXPR | SyntaxKind::PRE_ELSIF_CLAUSE | SyntaxKind::PRE_ELSE_CLAUSE => {
                continue
            }
            _ => {}
        }

        if child.kind() == SyntaxKind::PRE_IF_DIR {
            if let Some(stmt) = lower_preproc_if(ctx, &child) {
                let stmt_id = ctx.alloc_stmt(stmt, child.text_range());
                stmts.push(stmt_id);
            }
            continue;
        }

        if child.kind() == SyntaxKind::PRE_REGION_DIR {
            let (region_stmts, _region_terminates) = lower_region_stmts(ctx, &child);
            stmts.extend(region_stmts);
            continue;
        }

        if !is_statement_node(&child) {
            continue;
        }

        if let Some(stmt_id) = lower_stmt(ctx, &child) {
            stmts.push(stmt_id);
        }
    }

    stmts
}

pub(crate) fn preproc_if_all_branches_terminate(node: &SyntaxNode) -> bool {
    let has_else = node.children().any(|n| n.kind() == SyntaxKind::PRE_ELSE_CLAUSE);
    if !has_else {
        return false;
    }

    if !preproc_branch_terminates(node) {
        return false;
    }

    for elsif in node.children().filter(|n| n.kind() == SyntaxKind::PRE_ELSIF_CLAUSE) {
        if !preproc_branch_terminates(&elsif) {
            return false;
        }
    }

    let else_clause = node.children().find(|n| n.kind() == SyntaxKind::PRE_ELSE_CLAUSE);
    if let Some(else_node) = else_clause {
        if !preproc_branch_terminates(&else_node) {
            return false;
        }
    }

    true
}

fn preproc_branch_terminates(branch: &SyntaxNode) -> bool {
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

pub(crate) fn preproc_region_terminates(region: &SyntaxNode) -> bool {
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
