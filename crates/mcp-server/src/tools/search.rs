//! Search tools: full-text and semantic code search.

use bsl_search::{IndexProgress, SearchEngine};
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;
use std::fmt::Write;
use std::sync::atomic::Ordering;
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
    let engine = guard
        .as_ref()
        .ok_or_else(|| McpError::invalid_params("Search index not initialized.", None))?;

    if !engine.has_semantic() {
        return Err(McpError::invalid_params(
            "Semantic search not available. Set EMBEDDING_URL environment variable \
             and restart. Use find_code for text search instead.",
            None,
        ));
    }

    let hits = engine
        .search(query, limit)
        .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?;

    if hits.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
    }

    Ok(CallToolResult::success(vec![Content::text(format_hits(&hits))]))
}

/// Search index status and indexing progress.
pub fn search_status(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    progress: &Arc<IndexProgress>,
) -> Result<CallToolResult, McpError> {
    let mut out = String::new();

    let guard =
        engine.lock().map_err(|e| McpError::internal_error(format!("lock error: {e}"), None))?;

    if let Some(engine) = guard.as_ref() {
        let files = engine.file_count().unwrap_or(0);
        let chunks = engine.chunk_count().unwrap_or(0);
        let vectors = engine.vector_count();
        let semantic = engine.has_semantic();

        let _ = writeln!(out, "Search index: ready");
        let _ = writeln!(out, "  Files:    {files}");
        let _ = writeln!(out, "  Chunks:   {chunks}");
        let _ = writeln!(out, "  Vectors:  {vectors}");
        let _ = writeln!(
            out,
            "  Semantic: {}",
            if semantic { "available" } else { "not configured (set EMBEDDING_URL)" }
        );
        let _ = writeln!(out, "  FTS:      {}", if chunks > 0 { "available" } else { "empty" });
    } else {
        let _ = writeln!(out, "Search index: not initialized");
    }

    if progress.is_active() {
        let total = progress.total_chunks.load(Ordering::Relaxed);
        let done = progress.done_chunks.load(Ordering::Relaxed);
        let total_b = progress.total_batches.load(Ordering::Relaxed);
        let done_b = progress.done_batches.load(Ordering::Relaxed);
        let pct = progress.percent();

        let _ = writeln!(out);
        let _ = writeln!(out, "Indexing in progress: {pct}%");
        let _ = writeln!(out, "  Batches:  {done_b}/{total_b}");
        let _ = writeln!(out, "  Chunks:   {done}/{total}");
    }

    Ok(CallToolResult::success(vec![Content::text(out)]))
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
