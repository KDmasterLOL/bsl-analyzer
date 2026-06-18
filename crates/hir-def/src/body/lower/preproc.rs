use syntax::{SyntaxKind, SyntaxNode};
use text_size::TextRange;

use crate::hir::{PreprocIfStmt, Stmt};

use super::control_flow::is_statement_node;
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

        // Flat region markers are skipped; their statements are siblings here.
        if !is_statement_node(&child) {
            continue;
        }

        if let Some(stmt_id) = lower_stmt(ctx, &child) {
            stmts.push(stmt_id);
        }
    }

    stmts
}
