use crate::{handlers, BodyContext, Diagnostic};
use hir::LocalRange;
use syntax::{SyntaxNode, SyntaxToken};

/// One walk over the body's nodes and tokens for every single-pass check.
/// For module-level code the walk skips method subtrees, which their own
/// bodies cover, so each node of the file is visited exactly once.
pub fn collect_body_single_pass(ctx: &BodyContext, acc: &mut Vec<Diagnostic<LocalRange>>) {
    if !ctx.config.any_enabled(crate::runner::SINGLE_PASS_DIAGNOSTICS) {
        return;
    }

    let _span = tracing::debug_span!("collect_body_single_pass").entered();

    for node in ctx.nodes() {
        check_node_handlers(&node, acc, ctx);

        for element in node.children_with_tokens() {
            if let Some(token) = element.into_token() {
                check_token_handlers(&token, acc, ctx);
            }
        }
    }
}

#[inline]
fn check_node_handlers(
    node: &SyntaxNode,
    acc: &mut Vec<Diagnostic<LocalRange>>,
    ctx: &BodyContext,
) {
    handlers::useless_ternary_operator::check_node(node, acc, ctx);
    handlers::double_negatives::check_node(node, acc, ctx);
    handlers::unknown_preprocessor_symbol::check_node(node, acc, ctx);
    handlers::bad_words::check_node(node, acc, ctx);
    handlers::typo::check_node(node, acc, ctx);
    handlers::nested_ternary_operator::check_node(node, acc, ctx);
}

#[inline]
fn check_token_handlers(
    token: &SyntaxToken,
    acc: &mut Vec<Diagnostic<LocalRange>>,
    ctx: &BodyContext,
) {
    handlers::yo_letter_usage::check_token(token, acc, ctx);
    handlers::magic_date::check_token(token, acc, ctx);
    handlers::using_hardcode_path::check_token(token, acc, ctx);
}
