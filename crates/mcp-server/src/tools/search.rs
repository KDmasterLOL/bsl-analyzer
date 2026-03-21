//! Search tools: full-text and semantic code search.

use bsl_search::SearchEngine;
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;
use std::fmt::Write;
use std::sync::{Arc, Mutex};

/// Full-text search across indexed BSL code.
pub fn find_code(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    query: &str,
    limit: usize,
) -> Result<CallToolResult, McpError> {
    let guard =
        engine.lock().map_err(|e| McpError::internal_error(format!("lock error: {e}"), None))?;
    let engine = guard.as_ref().ok_or_else(|| {
        McpError::invalid_params("Search index not built. Run indexing first.", None)
    })?;

    let hits = engine
        .text_search(query, limit)
        .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?;

    if hits.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
    }

    Ok(CallToolResult::success(vec![Content::text(format_hits(&hits))]))
}

/// Semantic search across indexed BSL code.
pub fn search_code(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    query: &str,
    limit: usize,
) -> Result<CallToolResult, McpError> {
    let guard =
        engine.lock().map_err(|e| McpError::internal_error(format!("lock error: {e}"), None))?;
    let engine = guard.as_ref().ok_or_else(|| {
        McpError::invalid_params("Search index not built. Run indexing first.", None)
    })?;

    let hits = engine
        .search(query, limit)
        .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?;

    if hits.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
    }

    Ok(CallToolResult::success(vec![Content::text(format_hits(&hits))]))
}

fn format_hits(hits: &[bsl_search::SearchHit]) -> String {
    let mut out = String::new();

    for (i, hit) in hits.iter().enumerate() {
        let name = if hit.symbol_name.is_empty() { "<header>" } else { &hit.symbol_name };
        let _ = writeln!(
            out,
            "#{} [{:.3}] {}:{}-{} :: {} ({})",
            i + 1,
            hit.score,
            hit.file_path,
            hit.line_start + 1,
            hit.line_end,
            name,
            hit.kind,
        );

        // Show first 5 lines of code.
        for line in hit.text.lines().take(5) {
            let _ = writeln!(out, "  │ {line}");
        }
        let total_lines = hit.text.lines().count();
        if total_lines > 5 {
            let _ = writeln!(out, "  │ ... ({} more lines)", total_lines - 5);
        }
        out.push('\n');
    }

    out
}
