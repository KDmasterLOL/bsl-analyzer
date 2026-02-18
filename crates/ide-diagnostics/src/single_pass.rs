//! Single-pass AST traversal for syntax diagnostics.
//!
//! This module performs ONE traversal of the syntax tree and calls all
//! migrated handlers on each node/token. This provides:
//! - **Performance:** O(n) instead of O(n × handlers)
//! - **Cache locality:** Better CPU cache utilization
//! - **Reduced latency:** Faster diagnostics, less UI flicker
//!
//! Handlers are incrementally migrated from `collect_syntax_diagnostics`.
//! Once all handlers are migrated, `collect_syntax_diagnostics` will be removed.

use crate::{handlers, Diagnostic, DiagnosticsContext};
use syntax::{SyntaxNode, SyntaxToken};

/// Context passed to single-pass handlers during AST traversal.
///
/// This struct holds per-file configuration that handlers need.
/// It's created once per file and passed to all handlers.
pub struct SinglePassContext<'a> {
    pub ctx: &'a DiagnosticsContext<'a>,
}

impl<'a> SinglePassContext<'a> {
    pub fn new(ctx: &'a DiagnosticsContext<'a>) -> Self {
        Self { ctx }
    }
}

/// State accumulated during single-pass traversal for stateful handlers.
///
/// Some handlers need to track state across multiple nodes (e.g., counting
/// occurrences, tracking seen regions). This struct holds that state.
#[derive(Debug, Default)]
pub struct DiagnosticState {
    // Reserved for future stateful handlers:
    // - duplicate_string_literal: HashMap<String, Vec<TextRange>>
    // - duplicate_region: HashMap<String, Vec<TextRange>>
    // - consecutive_empty_lines: usize
}

impl DiagnosticState {
    /// Create new empty state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Finalize and emit diagnostics from accumulated state.
    ///
    /// Called after AST traversal completes. Stateful handlers emit
    /// their diagnostics here based on accumulated data.
    #[allow(clippy::ptr_arg)]
    pub fn finalize(&self, _diagnostics: &mut Vec<Diagnostic>, _ctx: &DiagnosticsContext) {
        // Reserved for stateful handlers that emit diagnostics at the end
    }
}

/// Collect syntax diagnostics using single-pass AST traversal.
///
/// This function performs ONE traversal of the syntax tree and calls all
/// migrated handlers on each node/token. Handlers use:
/// - `check_node(&node, acc, ctx)` for node-based checks
/// - `check_token(&token, acc, ctx)` for token-based checks
///
/// ## Performance
/// Single pass: O(n) instead of O(n × handlers)
/// Expected speedup: 2-5x for files with many handlers
pub fn collect_syntax_single_pass(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Early exit: skip if none of our diagnostics are enabled
    if !ctx.config.any_enabled(crate::runner::SINGLE_PASS_DIAGNOSTICS) {
        return Vec::new();
    }

    let _span = tracing::debug_span!("collect_syntax_single_pass").entered();
    let start = std::time::Instant::now();

    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();
    let state = DiagnosticState::new();
    let sp_ctx = SinglePassContext::new(ctx);

    // Single traversal: visit all nodes and tokens
    for node in root.descendants() {
        // Call node-based handlers
        check_node_handlers(&node, &mut diagnostics, &sp_ctx);

        // Call token-based handlers for tokens in this node
        for element in node.children_with_tokens() {
            if let Some(token) = element.into_token() {
                check_token_handlers(&token, &mut diagnostics, &sp_ctx);
            }
        }
    }

    // Finalize stateful handlers
    state.finalize(&mut diagnostics, ctx);

    let elapsed = start.elapsed();
    tracing::debug!(
        elapsed_ms = elapsed.as_millis(),
        count = diagnostics.len(),
        "Single-pass syntax diagnostics collected"
    );

    diagnostics
}

/// Dispatch to all node-based handlers.
///
/// Each handler checks `ctx.is_disabled_with_metadata()` internally.
#[inline]
fn check_node_handlers(node: &SyntaxNode, acc: &mut Vec<Diagnostic>, sp_ctx: &SinglePassContext) {
    // Migrated handlers (Phase 1):
    handlers::useless_ternary_operator::check_node(node, acc, sp_ctx.ctx);
    handlers::double_negatives::check_node(node, acc, sp_ctx.ctx);
    // Migrated handlers (Phase 2):
    handlers::unknown_preprocessor_symbol::check_node(node, acc, sp_ctx.ctx);
    // Migrated handlers (Phase 4 - from collect_text_diagnostics):
    handlers::bad_words::check_node(node, acc, sp_ctx.ctx);
    handlers::typo::check_node(node, acc, sp_ctx.ctx);
    handlers::nested_ternary_operator::check_node(node, acc, sp_ctx.ctx);
}

/// Dispatch to all token-based handlers.
///
/// Each handler checks `ctx.is_disabled_with_metadata()` internally.
#[inline]
fn check_token_handlers(
    token: &SyntaxToken,
    acc: &mut Vec<Diagnostic>,
    sp_ctx: &SinglePassContext,
) {
    // Migrated handlers (Phase 2):
    handlers::yo_letter_usage::check_token(token, acc, sp_ctx.ctx);
    handlers::magic_date::check_token(token, acc, sp_ctx.ctx);
}
