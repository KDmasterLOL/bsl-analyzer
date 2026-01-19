//! Diagnostic runner helpers.
//!
//! This module provides helper functions for running diagnostics.

use crate::{handlers, Diagnostic, DiagnosticsContext};

/// Helper to run a diagnostic and log if it's slow (>80ms).
pub fn run_diagnostic<F>(
    name: &'static str,
    ctx: &DiagnosticsContext,
    check_fn: F,
) -> Vec<Diagnostic>
where
    F: FnOnce(&DiagnosticsContext) -> Vec<Diagnostic>,
{
    let start = std::time::Instant::now();
    let _span = tracing::debug_span!("diagnostic", name = name).entered();

    let result = check_fn(ctx);

    let elapsed = start.elapsed();
    if elapsed.as_millis() > 80 {
        tracing::warn!(
            diagnostic = name,
            elapsed_ms = elapsed.as_millis(),
            count = result.len(),
            "Slow diagnostic"
        );
    }

    result
}

/// Collect text-based diagnostics in a single AST pass.
///
/// This function performs ONE traversal of the syntax tree and calls all text-based
/// diagnostics on each node. This is much faster than calling each diagnostic separately.
///
/// Pattern from rust-analyzer: crates/ide-diagnostics/src/lib.rs:336-352
pub fn collect_text_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    diagnostics.extend(handlers::consecutive_empty_lines::check(ctx));
    diagnostics.extend(handlers::line_length::check(ctx));
    diagnostics.extend(handlers::commented_code::check(ctx));

    for node in root.descendants() {
        handlers::bad_words::check_node(&node, &mut diagnostics, ctx);
    }

    diagnostics
}
