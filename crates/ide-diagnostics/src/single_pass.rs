use crate::{handlers, Diagnostic, DiagnosticsContext};
use syntax::{SyntaxNode, SyntaxToken};

pub fn collect_syntax_single_pass(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if !ctx.config.any_enabled(crate::runner::SINGLE_PASS_DIAGNOSTICS) {
        return Vec::new();
    }

    let _span = tracing::debug_span!("collect_syntax_single_pass").entered();
    let start = std::time::Instant::now();

    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        check_node_handlers(&node, &mut diagnostics, ctx);

        for element in node.children_with_tokens() {
            if let Some(token) = element.into_token() {
                check_token_handlers(&token, &mut diagnostics, ctx);
            }
        }
    }

    let elapsed = start.elapsed();
    tracing::debug!(
        elapsed_ms = elapsed.as_millis(),
        count = diagnostics.len(),
        "Single-pass syntax diagnostics collected"
    );

    diagnostics
}

#[inline]
fn check_node_handlers(node: &SyntaxNode, acc: &mut Vec<Diagnostic>, ctx: &DiagnosticsContext) {
    handlers::useless_ternary_operator::check_node(node, acc, ctx);
    handlers::double_negatives::check_node(node, acc, ctx);
    handlers::unknown_preprocessor_symbol::check_node(node, acc, ctx);
    handlers::bad_words::check_node(node, acc, ctx);
    handlers::typo::check_node(node, acc, ctx);
    handlers::nested_ternary_operator::check_node(node, acc, ctx);
}

#[inline]
fn check_token_handlers(token: &SyntaxToken, acc: &mut Vec<Diagnostic>, ctx: &DiagnosticsContext) {
    handlers::yo_letter_usage::check_token(token, acc, ctx);
    handlers::magic_date::check_token(token, acc, ctx);
    handlers::using_hardcode_path::check_token(token, acc, ctx);
}
