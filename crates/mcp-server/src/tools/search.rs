//! Search tools: full-text and semantic search across code and documentation.

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
    if guard.is_none() {
        return Ok(CallToolResult::success(vec![Content::text(
            "Search index is being built, please try again in a moment.",
        )]));
    }
    let engine = guard.as_ref().expect("checked above");

    let hits = engine
        .text_search(query, limit, Some("code"))
        .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?;

    if hits.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
    }

    Ok(CallToolResult::success(vec![Content::text(format_code_hits(&hits))]))
}

/// Semantic search across indexed BSL code.
pub fn search_code(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    query: &str,
    limit: usize,
) -> Result<CallToolResult, McpError> {
    let guard =
        engine.lock().map_err(|e| McpError::internal_error(format!("lock error: {e}"), None))?;
    if guard.is_none() {
        return Ok(CallToolResult::success(vec![Content::text(
            "Search index is being built, please try again in a moment.",
        )]));
    }
    let engine = guard.as_ref().expect("checked above");

    if !engine.has_semantic() {
        return Err(McpError::invalid_params(
            "Semantic search not available. Set EMBEDDING_URL environment variable \
             and restart. Use find_code for text search instead.",
            None,
        ));
    }

    let hits = engine
        .search(query, limit, Some("code"))
        .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?;

    if hits.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
    }

    Ok(CallToolResult::success(vec![Content::text(format_code_hits(&hits))]))
}

/// Full-text search across platform documentation (types, methods, global functions).
pub fn find_docs(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    query: &str,
    limit: usize,
) -> Result<CallToolResult, McpError> {
    let guard =
        engine.lock().map_err(|e| McpError::internal_error(format!("lock error: {e}"), None))?;
    if guard.is_none() {
        return Ok(CallToolResult::success(vec![Content::text(
            "Search index is being built, please try again in a moment.",
        )]));
    }
    let engine = guard.as_ref().expect("checked above");

    let hits = engine
        .text_search(query, limit, Some("platform"))
        .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?;

    if hits.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
    }

    Ok(CallToolResult::success(vec![Content::text(format_doc_hits(&hits))]))
}

/// Semantic search across platform documentation.
pub fn search_docs(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    query: &str,
    limit: usize,
) -> Result<CallToolResult, McpError> {
    let guard =
        engine.lock().map_err(|e| McpError::internal_error(format!("lock error: {e}"), None))?;
    if guard.is_none() {
        return Ok(CallToolResult::success(vec![Content::text(
            "Search index is being built, please try again in a moment.",
        )]));
    }
    let engine = guard.as_ref().expect("checked above");

    if !engine.has_semantic() {
        return Err(McpError::invalid_params(
            "Semantic search not available. Set EMBEDDING_URL environment variable \
             and restart. Use find_docs for text search instead.",
            None,
        ));
    }

    let hits = engine
        .search(query, limit, Some("platform"))
        .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?;

    if hits.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
    }

    Ok(CallToolResult::success(vec![Content::text(format_doc_hits(&hits))]))
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

        let code_vectors = engine.embedding_count_by_collection("code").unwrap_or(0);
        let platform_vectors = engine.embedding_count_by_collection("platform").unwrap_or(0);

        let _ = writeln!(out, "Search index: ready");
        let _ = writeln!(out, "  Files:    {files}");
        let _ = writeln!(out, "  Chunks:   {chunks}");
        let _ = writeln!(
            out,
            "  Vectors:  {vectors} (code: {code_vectors}, platform: {platform_vectors})"
        );
        let _ = writeln!(
            out,
            "  Semantic: {}",
            if semantic { "available" } else { "not configured (set EMBEDDING_URL)" }
        );
        let _ = writeln!(out, "  FTS:      {}", if chunks > 0 { "available" } else { "empty" });
        let _ = writeln!(out, "  Collections: code, platform");

        if let Some(overlay) = engine
            .workspace_overlay_stats()
            .map_err(|e| McpError::internal_error(format!("overlay status error: {e}"), None))?
        {
            let _ = writeln!(out);
            let _ = writeln!(out, "Workspace overlay: enabled");
            let _ = writeln!(out, "  Files:    {}", overlay.overlay_files);
            let _ = writeln!(out, "  Deleted:  {}", overlay.deleted_files);
            let _ = writeln!(out, "  Hidden:   {}", overlay.hidden_paths);
            let _ = writeln!(out, "  Chunks:   {}", overlay.lexical_chunks);
            let _ = writeln!(out, "  Semantic: {}", overlay.semantic_chunks);
            let _ = writeln!(out, "  Cached embeddings: {}", overlay.cached_embeddings);
            let _ = writeln!(
                out,
                "  Watcher mode: {}",
                if overlay.watcher_mode { "enabled" } else { "polling" }
            );
            let _ = writeln!(out, "  Pending dirty paths: {}", overlay.pending_dirty_paths);
        }
    } else {
        let _ = writeln!(out, "Search index: building (background initialization in progress)");
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

fn format_code_hits(hits: &[bsl_search::SearchHit]) -> String {
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

fn format_doc_hits(hits: &[bsl_search::SearchHit]) -> String {
    let mut out = String::new();

    for (i, hit) in hits.iter().enumerate() {
        let _ = writeln!(out, "#{} [{:.3}] {} ({})", i + 1, hit.score, hit.symbol_name, hit.kind,);

        // Show first 5 lines of documentation.
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

#[cfg(test)]
mod tests {
    use super::search_status;
    use bsl_search::{IndexProgress, SearchEngine};
    use std::fs;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    #[test]
    fn search_status_shows_workspace_overlay_section() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("CommonModule.bsl");
        fs::write(&file, "Процедура СтараяПроцедура()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);

        fs::write(&file, "Процедура НоваяПроцедура()\nКонецПроцедуры").unwrap();

        let result =
            search_status(&Arc::new(Mutex::new(Some(engine))), &Arc::new(IndexProgress::default()))
                .unwrap();
        let text = result.content[0].raw.as_text().expect("expected text content").text.as_str();

        assert!(text.contains("Workspace overlay: enabled"));
        assert!(text.contains("Files:    1"));
        assert!(text.contains("Chunks:   1"));
    }
}
