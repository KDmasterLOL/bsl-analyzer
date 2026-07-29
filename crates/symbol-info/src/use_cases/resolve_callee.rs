use syntax::{SyntaxKind, SyntaxNode, SyntaxToken, TextSize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveParam {
    pub index: usize,
}

/// The argument slot the cursor sits in, for a position inside a call's or a
/// constructor's argument list.
///
/// Purely syntactic: which callee that argument list belongs to is inference's
/// answer, asked through `hir::Semantics::call_binding_at`. Classifying the
/// callee from identifier text — a plural as a metadata collection, a receiver's
/// spelling as a platform type — resolved names their owner had already taken,
/// because the text carries no scope.
pub fn resolve_callee_at(root: &SyntaxNode, offset: TextSize) -> Option<ActiveParam> {
    let token = root.token_at_offset(offset).left_biased()?;

    let arg_list = find_arg_list(&token)?;
    if is_on_closing_paren(&token, &arg_list) {
        return None;
    }
    if !is_call_argument_list(&arg_list) {
        return None;
    }

    Some(ActiveParam { index: count_commas_before(&arg_list, offset) })
}

/// Whether this argument list belongs to a call or a constructor, as opposed to
/// a parenthesised expression the parser shaped the same way.
fn is_call_argument_list(arg_list: &SyntaxNode) -> bool {
    arg_list
        .parent()
        .is_some_and(|p| matches!(p.kind(), SyntaxKind::CALL_EXPR | SyntaxKind::NEW_EXPR))
}

fn find_arg_list(token: &SyntaxToken) -> Option<SyntaxNode> {
    token.parent_ancestors().find(|node| node.kind() == SyntaxKind::ARG_LIST)
}

fn is_on_closing_paren(token: &SyntaxToken, arg_list: &SyntaxNode) -> bool {
    if token.kind() == SyntaxKind::R_PAREN {
        if let Some(parent) = token.parent() {
            return parent == *arg_list || parent.parent().as_ref() == Some(arg_list);
        }
    }
    false
}

fn count_commas_before(arg_list: &SyntaxNode, offset: TextSize) -> usize {
    let mut count = 0;
    for child in arg_list.children_with_tokens() {
        if child.text_range().start() >= offset {
            break;
        }
        if child.kind() == SyntaxKind::COMMA {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
#[path = "resolve_callee_tests.rs"]
mod tests;
