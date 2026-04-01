//! Search tools: full-text and semantic search across code and documentation.

use crate::baseline::{ConfiguredBaselineStatus, ExternalBaselineSource, ExternalBaselineState};
use bsl_search::{lexical_hits_for_resolved_view, IndexProgress, SearchEngine};
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;
use std::collections::HashSet;
use std::fmt::Write;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tracing::warn;

/// Full-text search across indexed BSL code.
pub fn find_code(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    external_baseline: Option<Arc<ExternalBaselineSource>>,
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

    let hits = if let Some(source) = external_baseline {
        match source.resolve_workspace_view(engine) {
            Ok(Some(view)) => lexical_hits_for_resolved_view(&view, query, limit, Some("code")),
            Ok(None) => engine
                .text_search(query, limit, Some("code"))
                .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?,
            Err(error) => {
                warn!("failed to resolve external baseline view for lexical search: {error}");
                engine
                    .text_search(query, limit, Some("code"))
                    .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?
            }
        }
    } else {
        engine
            .text_search(query, limit, Some("code"))
            .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?
    };

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
    external_baseline: Option<Arc<ExternalBaselineSource>>,
    query: &str,
    limit: usize,
) -> Result<CallToolResult, McpError> {
    let guard =
        engine.lock().map_err(|e| McpError::internal_error(format!("lock error: {e}"), None))?;
    let hits = if let Some(source) = external_baseline {
        match source.resolve_reference_view() {
            Ok(Some(view)) => lexical_hits_for_resolved_view(&view, query, limit, Some("platform")),
            Ok(None) => {
                let Some(engine) = guard.as_ref() else {
                    return Ok(CallToolResult::success(vec![Content::text(
                        "Search index is being built, please try again in a moment.",
                    )]));
                };
                engine
                    .text_search(query, limit, Some("platform"))
                    .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?
            }
            Err(error) => {
                warn!("failed to resolve external reference baseline view for lexical search: {error}");
                let Some(engine) = guard.as_ref() else {
                    return Ok(CallToolResult::success(vec![Content::text(
                        "Search index is being built, please try again in a moment.",
                    )]));
                };
                engine
                    .text_search(query, limit, Some("platform"))
                    .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?
            }
        }
    } else {
        let Some(engine) = guard.as_ref() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "Search index is being built, please try again in a moment.",
            )]));
        };
        engine
            .text_search(query, limit, Some("platform"))
            .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?
    };

    if hits.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
    }

    Ok(CallToolResult::success(vec![Content::text(format_doc_hits(&hits))]))
}

/// Semantic search across platform documentation.
pub fn search_docs(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    _external_baseline: Option<Arc<ExternalBaselineSource>>,
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
    configured_baseline: Option<ConfiguredBaselineStatus>,
    external_baseline: Option<Arc<ExternalBaselineSource>>,
) -> Result<CallToolResult, McpError> {
    let mut out = String::new();

    if let Some(configured_baseline) = configured_baseline.as_ref() {
        let _ = writeln!(out, "Configured baseline:");
        let _ = writeln!(out, "  Backend:  {}", configured_baseline.backend);
        let _ = writeln!(out, "  Select:   {}", configured_baseline.selection);
        let _ = writeln!(
            out,
            "  Status:   {}",
            configured_baseline.issue.as_deref().unwrap_or("ready")
        );
        let _ = writeln!(out);
    }

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
        let workspace_overlay = engine
            .workspace_overlay_stats()
            .map_err(|e| McpError::internal_error(format!("overlay status error: {e}"), None))?;
        if let Some(source) = external_baseline.as_ref() {
            match source.corpus() {
                bsl_search::CorpusId::WorkspaceCode => {
                    let code_lexical_source = match source.resolve_workspace_view(engine) {
                        Ok(Some(_)) => "external baseline + local overlay",
                        Ok(None) => "local sqlite + local overlay",
                        Err(_) => "local sqlite + local overlay",
                    };
                    let _ = writeln!(out, "  Code lexical source: {code_lexical_source}");
                }
                bsl_search::CorpusId::Reference => {
                    let docs_lexical_source = match source.resolve_reference_view() {
                        Ok(Some(_)) => "external baseline",
                        Ok(None) => "local sqlite",
                        Err(_) => "local sqlite",
                    };
                    let _ = writeln!(out, "  Docs lexical source: {docs_lexical_source}");
                    let docs_semantic_source = if semantic {
                        "local semantic cache of external baseline"
                    } else {
                        "not configured (set EMBEDDING_URL)"
                    };
                    let _ = writeln!(out, "  Docs semantic source: {docs_semantic_source}");
                }
                bsl_search::CorpusId::Custom(_) => {}
            }
        } else if workspace_overlay.is_some() {
            let _ = writeln!(out, "  Code lexical source: local sqlite + local overlay");
        } else {
            let _ = writeln!(out, "  Docs lexical source: local sqlite");
            let docs_semantic_source =
                if semantic { "local sqlite" } else { "not configured (set EMBEDDING_URL)" };
            let _ = writeln!(out, "  Docs semantic source: {docs_semantic_source}");
        }

        if let Some(overlay) = workspace_overlay {
            if let Some(view) = engine.resolve_workspace_code_view().map_err(|e| {
                McpError::internal_error(format!("resolved workspace view error: {e}"), None)
            })? {
                let files: HashSet<&str> =
                    view.documents().iter().map(|document| document.path.as_str()).collect();
                let _ = writeln!(out);
                let _ = writeln!(out, "Resolved workspace view: ready");
                let _ = writeln!(out, "  Baseline: {}", format_baseline_ref(view.baseline()));
                let _ = writeln!(out, "  Files:    {}", files.len());
                let _ = writeln!(out, "  Chunks:   {}", view.documents().len());
            }

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

    if let Some(external_baseline) = external_baseline {
        let status = external_baseline.probe_status();
        let _ = writeln!(out);
        let _ = writeln!(out, "External baseline: configured");
        let _ = writeln!(out, "  Backend:  {}", status.backend);
        let _ = writeln!(out, "  Schema:   {}", status.schema);
        let _ = writeln!(out, "  Select:   {}", status.selection);
        match status.state {
            ExternalBaselineState::Ready { snapshot_id, fingerprint, documents, files } => {
                let _ = writeln!(out, "  Status:   ready");
                let _ = writeln!(out, "  Snapshot: {}", snapshot_id);
                let _ = writeln!(out, "  Files:    {}", files);
                let _ = writeln!(out, "  Chunks:   {}", documents);
                if let Some(fingerprint) = fingerprint.as_deref() {
                    let _ = writeln!(out, "  Fingerprint: {}", shorten_fingerprint(fingerprint));
                }
                match external_baseline.corpus() {
                    bsl_search::CorpusId::WorkspaceCode => {
                        if let Some(engine) = guard.as_ref() {
                            match external_baseline.resolve_workspace_view(engine) {
                                Ok(Some(view)) => {
                                    let resolved_files: HashSet<&str> = view
                                        .documents()
                                        .iter()
                                        .map(|document| document.path.as_str())
                                        .collect();
                                    let _ = writeln!(out, "  Resolved view: ready");
                                    let _ =
                                        writeln!(out, "  Resolved files: {}", resolved_files.len());
                                    let _ = writeln!(
                                        out,
                                        "  Resolved chunks: {}",
                                        view.documents().len()
                                    );
                                }
                                Ok(None) => {
                                    let _ = writeln!(out, "  Resolved view: unavailable");
                                }
                                Err(error) => {
                                    let _ = writeln!(out, "  Resolved view: error");
                                    let _ = writeln!(out, "  Resolved error: {}", error);
                                }
                            }
                        }
                    }
                    bsl_search::CorpusId::Reference => {
                        if let Some(local_fingerprint) =
                            external_baseline.local_reference_fingerprint()
                        {
                            let freshness = match fingerprint.as_deref() {
                                Some(shared) if shared == local_fingerprint => "up to date",
                                Some(_) => "stale",
                                None => "unknown",
                            };
                            let _ = writeln!(out, "  Freshness: {}", freshness);
                            let _ = writeln!(
                                out,
                                "  Local fingerprint: {}",
                                shorten_fingerprint(&local_fingerprint)
                            );
                        }
                        match external_baseline.resolve_reference_view() {
                            Ok(Some(view)) => {
                                let resolved_files: HashSet<&str> = view
                                    .documents()
                                    .iter()
                                    .map(|document| document.path.as_str())
                                    .collect();
                                let _ = writeln!(out, "  Resolved view: ready");
                                let _ = writeln!(out, "  Resolved files: {}", resolved_files.len());
                                let _ =
                                    writeln!(out, "  Resolved chunks: {}", view.documents().len());
                            }
                            Ok(None) => {
                                let _ = writeln!(out, "  Resolved view: unavailable");
                            }
                            Err(error) => {
                                let _ = writeln!(out, "  Resolved view: error");
                                let _ = writeln!(out, "  Resolved error: {}", error);
                            }
                        }
                    }
                    bsl_search::CorpusId::Custom(_) => {}
                }
            }
            ExternalBaselineState::Missing => {
                let _ = writeln!(out, "  Status:   not found");
            }
            ExternalBaselineState::Error(error) => {
                let _ = writeln!(out, "  Status:   error");
                let _ = writeln!(out, "  Error:    {}", error);
            }
        }
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

fn format_baseline_ref(baseline: &bsl_search::BaselineRef) -> String {
    if let Some(snapshot_id) = &baseline.snapshot_id {
        return format!("snapshot {}", snapshot_id.0);
    }
    if let (Some(branch), Some(commit)) = (&baseline.branch, &baseline.commit) {
        return format!("branch {branch} @ {commit}");
    }
    if let Some(branch) = &baseline.branch {
        return format!("branch {branch}");
    }
    if let Some(commit) = &baseline.commit {
        return format!("commit {commit}");
    }
    format!("latest {}", baseline.corpus.as_str())
}

fn shorten_fingerprint(fingerprint: &str) -> &str {
    fingerprint.get(..12).unwrap_or(fingerprint)
}

#[cfg(test)]
mod tests {
    use super::{search_docs, search_status, ConfiguredBaselineStatus, ExternalBaselineSource};
    use bsl_search::{
        lexical_hits_for_resolved_view, BaselineRef, CorpusId, Document, IndexProgress,
        IndexedDocument, ResolvedView, SearchEngine,
    };
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

        let result = search_status(
            &Arc::new(Mutex::new(Some(engine))),
            &Arc::new(IndexProgress::default()),
            Some(ConfiguredBaselineStatus {
                backend: "sqlite",
                selection: "local workspace index".to_owned(),
                issue: None,
            }),
            None,
        )
        .unwrap();
        let text = result.content[0].raw.as_text().expect("expected text content").text.as_str();

        assert!(text.contains("Code lexical source: local sqlite + local overlay"));
        assert!(text.contains("Resolved workspace view: ready"));
        assert!(text.contains("Baseline: snapshot local-workspace-baseline"));
        assert!(text.contains("Workspace overlay: enabled"));
        assert!(text.contains("Files:    1"));
        assert!(text.contains("Chunks:   1"));
    }

    #[test]
    fn search_status_shows_external_baseline_probe_errors() {
        let source = ExternalBaselineSource::new(
            bsl_search::ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
            bsl_search::BaselineRef {
                corpus: bsl_search::CorpusId::WorkspaceCode,
                snapshot_id: None,
                branch: Some("main".to_owned()),
                commit: None,
            },
        )
        .unwrap();

        let result = search_status(
            &Arc::new(Mutex::new(None)),
            &Arc::new(IndexProgress::default()),
            Some(ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "branch main".to_owned(),
                issue: None,
            }),
            Some(Arc::new(source)),
        )
        .unwrap();
        let text = result.content[0].raw.as_text().expect("expected text content").text.as_str();

        assert!(text.contains("Configured baseline:"));
        assert!(text.contains("Select:   branch main"));
        assert!(text.contains("External baseline: configured"));
        assert!(text.contains("Backend:  postgres"));
        assert!(text.contains("Status:   error"));
    }

    #[test]
    fn search_status_shows_reference_docs_source() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("reference-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine
            .index_documents(
                "platform",
                "platform://docs",
                b"v1",
                &[Document {
                    title: "Массив / Array".to_owned(),
                    body: "Тип: Массив / Array".to_owned(),
                    kind: "type".to_owned(),
                }],
                None,
            )
            .unwrap();

        let result = search_status(
            &Arc::new(Mutex::new(Some(engine))),
            &Arc::new(IndexProgress::default()),
            Some(ConfiguredBaselineStatus {
                backend: "sqlite",
                selection: "local reference index".to_owned(),
                issue: None,
            }),
            None,
        )
        .unwrap();
        let text = result.content[0].raw.as_text().expect("expected text content").text.as_str();

        assert!(text.contains("Docs lexical source: local sqlite"));
        assert!(text.contains("Docs semantic source: not configured (set EMBEDDING_URL)"));
    }

    #[test]
    fn search_docs_with_external_reference_baseline_uses_standard_semantic_validation() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("reference-search.db");
        let engine = SearchEngine::fts_only(&db_path).unwrap();
        let source = ExternalBaselineSource::new(
            bsl_search::ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
            bsl_search::BaselineRef {
                corpus: bsl_search::CorpusId::Reference,
                snapshot_id: None,
                branch: None,
                commit: None,
            },
        )
        .unwrap();

        let error =
            search_docs(&Arc::new(Mutex::new(Some(engine))), Some(Arc::new(source)), "Массив", 10)
                .unwrap_err();

        assert!(error.message.contains("Semantic search not available"));
        assert!(!error.message.contains("centralized reference baseline"));
    }

    #[test]
    fn resolved_view_lexical_search_returns_exact_match_first() {
        let view = ResolvedView::new(
            BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "snapshot-1"),
            vec![
                IndexedDocument {
                    collection: "code".to_owned(),
                    path: "A.bsl".to_owned(),
                    symbol_name: "НайтиПроцедуру".to_owned(),
                    kind: "procedure".to_owned(),
                    line_start: 1,
                    line_end: 2,
                    text: "body".to_owned(),
                    content_hash: "a".to_owned(),
                },
                IndexedDocument {
                    collection: "code".to_owned(),
                    path: "B.bsl".to_owned(),
                    symbol_name: "Другая".to_owned(),
                    kind: "procedure".to_owned(),
                    line_start: 1,
                    line_end: 2,
                    text: "внутри НайтиПроцедуру".to_owned(),
                    content_hash: "b".to_owned(),
                },
            ],
        );

        let hits = lexical_hits_for_resolved_view(&view, "НайтиПроцедуру", 10, Some("code"));

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].file_path, "A.bsl");
        assert!(hits[0].score > hits[1].score);
    }
}
