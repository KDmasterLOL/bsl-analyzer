use crate::baseline::{ConfiguredBaselineStatus, ExternalBaselineService, ExternalBaselineState};
use crate::state::{SemanticRuntimeStatus, WorkspaceSearchMode};
use bsl_search::{
    fuse_rrf, lexical_hits_for_resolved_view, merge_context_for_collection, merge_lexical,
    merge_semantic, IndexProgress, LexicalHit, SearchEngine, SearchError, SearchHit, SemanticHit,
    RRF_K,
};
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;
use serde_json::json;
use std::collections::HashSet;
use std::fmt::Write;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tracing::warn;

const DIRECT_SEARCH_INITIAL_WINDOW_MULTIPLIER: usize = 3;
const DIRECT_SEARCH_MAX_WINDOW_MULTIPLIER: usize = 10;
const DIRECT_SEARCH_MIN_MAX_WINDOW: usize = 100;
const DIRECT_SEARCH_MAX_REFILL_ROUNDS: usize = 4;

/// How much wider than the caller's `limit` each modality is queried before fusion, so a hit
/// ranked just outside `limit` in one modality but boosted by the other can still surface.
const HYBRID_FETCH_MULTIPLIER: usize = 2;

/// The outcome of producing one modality's code hits, separated from presentation so the
/// hybrid path can fuse two modalities. Hard policy/terminal failures stay `Err(McpError)`;
/// these soft states let `hybrid_code` reproduce today's lexical messages and degrade
/// gracefully on a semantic shortfall.
enum CodeHits {
    /// Hits (possibly empty) plus the workspace root for the graph-id bridge.
    Ready { hits: Vec<SearchHit>, workspace_root: Option<std::path::PathBuf> },
    /// The index/overlay is still warming or building — no hits yet, emit `message`.
    Pending(String),
    /// The semantic modality cannot serve this request; `hybrid_code` degrades to lexical.
    Unavailable(SemanticUnavailable),
}

/// Why semantic search could not serve a request — carried so `hybrid_code` can name the
/// reason in its degradation note instead of hard-failing the whole search.
enum SemanticUnavailable {
    NotConfigured,
    RuntimeFailed,
    BaselineNotReady,
    BaselineRequired,
}

impl SemanticUnavailable {
    fn note(&self) -> &'static str {
        match self {
            Self::NotConfigured => "semantic skipped: not configured (set EMBEDDING_URL)",
            Self::RuntimeFailed => "semantic skipped: runtime initialization failed",
            Self::BaselineNotReady => "semantic skipped: PostgreSQL baseline semantic not ready",
            Self::BaselineRequired => "semantic skipped: requires PostgreSQL baseline serving",
        }
    }
}

/// Produce lexical (FTS5) code hits, separated from presentation. Hard policy/terminal
/// failures are `Err`; a still-warming index is `Pending`. Lexical search is always available
/// (it is the baseline), so this never returns `Unavailable`.
fn lexical_code_hits(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    workspace_search_mode: WorkspaceSearchMode,
    configured_baseline: Option<&ConfiguredBaselineStatus>,
    external_baseline: Option<Arc<ExternalBaselineService>>,
    query: &str,
    limit: usize,
) -> Result<CodeHits, McpError> {
    ensure_workspace_search_allowed(configured_baseline)?;
    ensure_workspace_baseline_runtime_ready(
        workspace_search_mode,
        configured_baseline,
        external_baseline.as_ref(),
    )?;
    let guard = match engine.try_lock() {
        Ok(g) => g,
        Err(_) => {
            if let Some(source) = external_baseline {
                match try_direct_lexical_code_no_overlay(&source, query, limit) {
                    DirectResult::Found(hits) => {
                        if hits.is_empty() {
                            return Ok(CodeHits::Pending(
                                "No results found (overlay is warming up, only baseline search available)."
                                    .to_owned(),
                            ));
                        }
                        // Engine lock is busy, so these are external-baseline direct hits
                        // with no reachable workspace root. Module-keyed methods still get a
                        // graph_id (root-independent); a path-fallback hit would be dropped,
                        // which is fine here — baseline paths are relative, not absolute.
                        return Ok(CodeHits::Ready { hits, workspace_root: None });
                    }
                    DirectResult::Terminal(error) => {
                        return Err(external_baseline_mcp_error(&error));
                    }
                    DirectResult::Unavailable => {}
                }
            }
            return Ok(CodeHits::Pending(
                "Search index overlay is warming up, please try again in a moment.".to_owned(),
            ));
        }
    };

    let hits = if let Some(source) = external_baseline {
        match guard.as_ref() {
            Some(engine) => match try_direct_lexical_code(engine, &source, query, limit) {
                DirectResult::Found(hits) => hits,
                DirectResult::Terminal(error) => {
                    return Err(external_baseline_mcp_error(&error));
                }
                DirectResult::Unavailable => match source.resolve_workspace_view(engine) {
                    Ok(Some(view)) => {
                        lexical_hits_for_resolved_view(&view, query, limit, Some("code"))
                    }
                    Ok(None) => engine.text_search(query, limit, Some("code")).map_err(|e| {
                        McpError::internal_error(format!("search error: {e}"), None)
                    })?,
                    Err(error) => {
                        if error.is_terminal() {
                            return Err(external_baseline_mcp_error(&error));
                        }
                        warn!(
                            "failed to resolve external baseline view for lexical search: {error}"
                        );
                        engine.text_search(query, limit, Some("code")).map_err(|e| {
                            McpError::internal_error(format!("search error: {e}"), None)
                        })?
                    }
                },
            },
            None => match try_direct_lexical_code_no_overlay(&source, query, limit) {
                DirectResult::Found(hits) => hits,
                DirectResult::Terminal(error) => {
                    return Err(external_baseline_mcp_error(&error));
                }
                DirectResult::Unavailable => {
                    return Ok(CodeHits::Pending(
                        "Search index is being built, please try again in a moment.".to_owned(),
                    ));
                }
            },
        }
    } else {
        let Some(engine) = guard.as_ref() else {
            return Ok(CodeHits::Pending(
                "Search index is being built, please try again in a moment.".to_owned(),
            ));
        };
        engine
            .text_search(query, limit, Some("code"))
            .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?
    };

    let workspace_root = guard.as_ref().and_then(|e| e.workspace_root()).map(Path::to_path_buf);
    Ok(CodeHits::Ready { hits, workspace_root })
}

/// Produce semantic (pgvector) code hits, separated from presentation. Hard policy/terminal
/// failures are `Err`; a still-warming index is `Pending`; a semantic shortfall that
/// `hybrid_code` can degrade past is `Unavailable`.
fn semantic_code_hits(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    semantic_runtime: &Arc<Mutex<SemanticRuntimeStatus>>,
    workspace_search_mode: WorkspaceSearchMode,
    configured_baseline: Option<&ConfiguredBaselineStatus>,
    external_baseline: Option<Arc<ExternalBaselineService>>,
    query: &str,
    limit: usize,
) -> Result<CodeHits, McpError> {
    ensure_workspace_search_allowed(configured_baseline)?;
    ensure_workspace_baseline_runtime_ready(
        workspace_search_mode.clone(),
        configured_baseline,
        external_baseline.as_ref(),
    )?;
    let semantic_runtime = semantic_runtime
        .lock()
        .map_err(|e| McpError::internal_error(format!("semantic runtime lock error: {e}"), None))?
        .clone();
    let guard = match engine.try_lock() {
        Ok(g) => g,
        Err(_) => {
            return Ok(CodeHits::Pending(
                "Semantic search overlay is warming up. Lexical search is available while the overlay is prepared."
                    .to_owned(),
            ));
        }
    };
    if guard.is_none() {
        return Ok(CodeHits::Pending(
            "Search index is being built, please try again in a moment.".to_owned(),
        ));
    }
    let engine = guard.as_ref().expect("checked above");

    if let SemanticRuntimeStatus::Failed(_) = semantic_runtime {
        return Ok(CodeHits::Unavailable(SemanticUnavailable::RuntimeFailed));
    }

    if !engine.has_semantic() {
        return Ok(CodeHits::Unavailable(SemanticUnavailable::NotConfigured));
    }

    if let Some(source) = external_baseline {
        match try_direct_semantic_code(engine, &source, query, limit) {
            DirectResult::Found(hits) => {
                let workspace_root = engine.workspace_root().map(Path::to_path_buf);
                return Ok(CodeHits::Ready { hits, workspace_root });
            }
            DirectResult::Terminal(error) => {
                return Err(external_baseline_mcp_error(&error));
            }
            DirectResult::Unavailable => {
                if matches!(workspace_search_mode, WorkspaceSearchMode::PostgresRemoteOverlay) {
                    return Ok(CodeHits::Unavailable(SemanticUnavailable::BaselineNotReady));
                }
            }
        }
    }

    if matches!(workspace_search_mode, WorkspaceSearchMode::PostgresRemoteOverlay) {
        return Ok(CodeHits::Unavailable(SemanticUnavailable::BaselineRequired));
    }

    let hits = engine
        .search(query, limit, Some("code"))
        .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?;
    let workspace_root = engine.workspace_root().map(Path::to_path_buf);
    Ok(CodeHits::Ready { hits, workspace_root })
}

/// Append live indexing progress to a "still building" message so the failed `search_code`
/// response carries the same signal as `search(status)`, instead of a flat "try again" that
/// hides whether the build is progressing or stuck.
fn with_index_progress(message: String, progress: &IndexProgress) -> String {
    if progress.is_active() {
        let done_b = progress.done_batches.load(Ordering::Relaxed);
        let total_b = progress.total_batches.load(Ordering::Relaxed);
        format!("{message} (indexing {}% — {done_b}/{total_b} batches)", progress.percent())
    } else {
        message
    }
}

/// The unified code search: run lexical and semantic, fuse by RRF, and degrade to lexical
/// (with a trailing note) when semantic cannot serve. This is what the `search_code` action
/// dispatches to.
// This is the tool-dispatch boundary: each argument is an independent runtime handle or
// per-request value pulled straight from `SharedState`, with no natural sub-grouping that a
// context struct would not make more obscure than the flat list.
#[allow(clippy::too_many_arguments)]
pub fn hybrid_code(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    semantic_runtime: &Arc<Mutex<SemanticRuntimeStatus>>,
    workspace_search_mode: WorkspaceSearchMode,
    configured_baseline: Option<&ConfiguredBaselineStatus>,
    external_baseline: Option<Arc<ExternalBaselineService>>,
    graph_root: Option<&Path>,
    index_progress: &IndexProgress,
    query: &str,
    limit: usize,
) -> Result<CallToolResult, McpError> {
    // Over-fetch each modality so a hit ranked just outside `limit` in one but boosted by the
    // other can still surface after fusion.
    let fetch = limit.saturating_mul(HYBRID_FETCH_MULTIPLIER).max(limit);

    let lexical = lexical_code_hits(
        engine,
        workspace_search_mode.clone(),
        configured_baseline,
        external_baseline.clone(),
        query,
        fetch,
    )?;
    let (lex_hits, workspace_root) = match lexical {
        CodeHits::Ready { hits, workspace_root } => (hits, workspace_root),
        // Lexical is the floor: if it cannot serve yet, the whole search cannot — emit its
        // message unchanged rather than claiming a (nonexistent) lexical-only result.
        CodeHits::Pending(message) => {
            return Ok(CallToolResult::success(vec![Content::text(with_index_progress(
                message,
                index_progress,
            ))]));
        }
        // Lexical search is always available, so it never reports a semantic shortfall; treat
        // it defensively as "still building".
        CodeHits::Unavailable(_) => {
            return Ok(CallToolResult::success(vec![Content::text(with_index_progress(
                "Search index is being built, please try again in a moment.".to_owned(),
                index_progress,
            ))]));
        }
    };

    let semantic = semantic_code_hits(
        engine,
        semantic_runtime,
        workspace_search_mode,
        configured_baseline,
        external_baseline,
        query,
        fetch,
    )?;

    let (mut hits, note): (Vec<SearchHit>, Option<&str>) = match semantic {
        CodeHits::Ready { hits: sem_hits, .. } => {
            (fuse_rrf(&lex_hits, &sem_hits, RRF_K, limit), None)
        }
        CodeHits::Pending(_) => {
            // Semantic overlay still warming — degrade to lexical for this call.
            (lex_hits, Some("semantic skipped: overlay warming up"))
        }
        CodeHits::Unavailable(reason) => (lex_hits, Some(reason.note())),
    };
    hits.truncate(limit);

    if hits.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
    }

    let mut out = format_code_hits(&hits, workspace_root.as_deref(), graph_root);
    if let Some(note) = note {
        // Append AFTER the hit lines — never before — so a client parsing `graph_id:` lines
        // positionally is not shifted.
        let _ = writeln!(out, "-- {note} --");
    }
    Ok(CallToolResult::success(vec![Content::text(out)]))
}

#[derive(Debug)]
enum DirectResult {
    Found(Vec<SearchHit>),
    Unavailable,
    Terminal(bsl_search::SearchError),
}

fn try_direct_lexical_code_no_overlay(
    source: &ExternalBaselineService,
    query: &str,
    limit: usize,
) -> DirectResult {
    let snapshot = match source.resolve_snapshot() {
        Ok(Some((_, s))) => s,
        Ok(None) => return DirectResult::Unavailable,
        Err(e) => {
            if e.is_terminal() {
                warn!("direct lexical (no overlay): terminal snapshot resolution error: {e}");
                return DirectResult::Terminal(e);
            }
            warn!("direct lexical (no overlay): snapshot resolution failed: {e}");
            return DirectResult::Unavailable;
        }
    };
    match source.lexical_search(snapshot.id.0.as_str(), query, Some("code"), limit) {
        Ok(hits) => DirectResult::Found(hits.iter().map(SearchHit::from_lexical).collect()),
        Err(e) => {
            if e.is_terminal() {
                warn!("direct lexical (no overlay): terminal serving query error: {e}");
                return DirectResult::Terminal(e);
            }
            warn!("direct lexical (no overlay): serving query failed: {e}");
            DirectResult::Unavailable
        }
    }
}

fn try_direct_lexical_code(
    engine: &SearchEngine,
    source: &ExternalBaselineService,
    query: &str,
    limit: usize,
) -> DirectResult {
    let snapshot = match source.resolve_snapshot() {
        Ok(Some((_, s))) => s,
        Ok(None) => return DirectResult::Unavailable,
        Err(e) => {
            if e.is_terminal() {
                warn!("direct lexical: terminal snapshot resolution error: {e}");
                return DirectResult::Terminal(e);
            }
            warn!("direct lexical: snapshot resolution failed: {e}");
            return DirectResult::Unavailable;
        }
    };
    let (overlay_hits, hidden_paths) = match engine.workspace_overlay_lexical_hits(query, limit) {
        Ok(r) => r,
        Err(e) => {
            warn!("direct lexical: overlay query failed: {e}");
            return DirectResult::Unavailable;
        }
    };
    let overlay_lexical: Vec<LexicalHit> = overlay_hits.iter().map(SearchHit::to_lexical).collect();
    merge_direct_lexical_with_refill(&overlay_lexical, &hidden_paths, limit, |fetch_limit| {
        source.lexical_search(snapshot.id.0.as_str(), query, Some("code"), fetch_limit)
    })
}

fn try_direct_semantic_code(
    engine: &SearchEngine,
    source: &ExternalBaselineService,
    query: &str,
    limit: usize,
) -> DirectResult {
    let snapshot = match source.resolve_snapshot() {
        Ok(Some((_, s))) => s,
        Ok(None) => return DirectResult::Unavailable,
        Err(e) => {
            if e.is_terminal() {
                warn!("direct semantic: terminal snapshot resolution error: {e}");
                return DirectResult::Terminal(e);
            }
            warn!("direct semantic: snapshot resolution failed: {e}");
            return DirectResult::Unavailable;
        }
    };
    let query_embedding = match engine.embed_query(query) {
        Ok(e) => e,
        Err(e) => {
            warn!("direct semantic: embed_query failed: {e}");
            return DirectResult::Unavailable;
        }
    };
    let Some(model_id) = engine.embedding_model() else {
        return DirectResult::Unavailable;
    };
    let Some(dim) = engine.embedding_dimension() else {
        return DirectResult::Unavailable;
    };
    let (overlay_hits, hidden_paths) = match engine.workspace_overlay_semantic_hits(query, limit) {
        Ok(r) => r,
        Err(e) => {
            warn!("direct semantic: overlay query failed: {e}");
            return DirectResult::Unavailable;
        }
    };
    let overlay_semantic: Vec<SemanticHit> =
        overlay_hits.iter().map(SearchHit::to_semantic).collect();
    merge_direct_semantic_with_refill(&overlay_semantic, &hidden_paths, limit, |fetch_limit| {
        source.semantic_search(
            snapshot.id.0.as_str(),
            &query_embedding,
            model_id,
            dim,
            Some("code"),
            fetch_limit,
        )
    })
}

fn direct_search_initial_window(limit: usize) -> usize {
    limit.max(1).saturating_mul(DIRECT_SEARCH_INITIAL_WINDOW_MULTIPLIER)
}

fn direct_search_max_window(limit: usize) -> usize {
    direct_search_initial_window(limit).max(
        limit.saturating_mul(DIRECT_SEARCH_MAX_WINDOW_MULTIPLIER).max(DIRECT_SEARCH_MIN_MAX_WINDOW),
    )
}

fn merge_direct_lexical_with_refill<F>(
    overlay_hits: &[LexicalHit],
    hidden_paths: &HashSet<String>,
    limit: usize,
    mut fetch_baseline: F,
) -> DirectResult
where
    F: FnMut(usize) -> Result<Vec<LexicalHit>, SearchError>,
{
    let context = merge_context_for_collection(hidden_paths, "code");
    let mut fetch_limit = direct_search_initial_window(limit);
    let max_fetch_limit = direct_search_max_window(limit);
    let mut previous_baseline_count = 0usize;
    let mut best = Vec::new();

    for _ in 0..DIRECT_SEARCH_MAX_REFILL_ROUNDS {
        let baseline_hits = match fetch_baseline(fetch_limit) {
            Ok(hits) => hits,
            Err(e) => {
                if e.is_terminal() {
                    warn!("direct lexical: terminal serving query error: {e}");
                    return DirectResult::Terminal(e);
                }
                warn!("direct lexical: serving query failed: {e}");
                return DirectResult::Unavailable;
            }
        };

        best = merge_lexical(&baseline_hits, overlay_hits, &context, limit)
            .into_iter()
            .map(SearchHit::from_merged)
            .collect();

        if best.len() >= limit {
            return DirectResult::Found(best);
        }

        let baseline_count = baseline_hits.len();
        if baseline_count < fetch_limit || baseline_count <= previous_baseline_count {
            return DirectResult::Found(best);
        }

        previous_baseline_count = baseline_count;
        if fetch_limit >= max_fetch_limit {
            return DirectResult::Found(best);
        }

        let next_fetch_limit = fetch_limit.saturating_mul(2).min(max_fetch_limit);
        if next_fetch_limit == fetch_limit {
            return DirectResult::Found(best);
        }
        fetch_limit = next_fetch_limit;
    }

    DirectResult::Found(best)
}

fn merge_direct_semantic_with_refill<F>(
    overlay_hits: &[SemanticHit],
    hidden_paths: &HashSet<String>,
    limit: usize,
    mut fetch_baseline: F,
) -> DirectResult
where
    F: FnMut(usize) -> Result<Vec<SemanticHit>, SearchError>,
{
    let context = merge_context_for_collection(hidden_paths, "code");
    let mut fetch_limit = direct_search_initial_window(limit);
    let max_fetch_limit = direct_search_max_window(limit);
    let mut previous_baseline_count = 0usize;
    let mut best = Vec::new();

    for _ in 0..DIRECT_SEARCH_MAX_REFILL_ROUNDS {
        let baseline_hits = match fetch_baseline(fetch_limit) {
            Ok(hits) => hits,
            Err(e) => {
                if e.is_terminal() {
                    warn!("direct semantic: terminal serving query error: {e}");
                    return DirectResult::Terminal(e);
                }
                warn!("direct semantic: serving query failed: {e}");
                return DirectResult::Unavailable;
            }
        };

        best = merge_semantic(&baseline_hits, overlay_hits, &context, limit)
            .into_iter()
            .map(SearchHit::from_merged)
            .collect();

        if best.len() >= limit {
            return DirectResult::Found(best);
        }

        let baseline_count = baseline_hits.len();
        if baseline_count < fetch_limit || baseline_count <= previous_baseline_count {
            return DirectResult::Found(best);
        }

        previous_baseline_count = baseline_count;
        if fetch_limit >= max_fetch_limit {
            return DirectResult::Found(best);
        }

        let next_fetch_limit = fetch_limit.saturating_mul(2).min(max_fetch_limit);
        if next_fetch_limit == fetch_limit {
            return DirectResult::Found(best);
        }
        fetch_limit = next_fetch_limit;
    }

    DirectResult::Found(best)
}

pub fn find_docs(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    configured_baseline: Option<&ConfiguredBaselineStatus>,
    external_baseline: Option<Arc<ExternalBaselineService>>,
    query: &str,
    limit: usize,
) -> Result<CallToolResult, McpError> {
    ensure_reference_baseline_runtime_ready(configured_baseline, external_baseline.as_ref())?;
    let guard =
        engine.lock().map_err(|e| McpError::internal_error(format!("lock error: {e}"), None))?;

    if let Some(source) = external_baseline {
        if let Some((_, snapshot)) = map_reference_baseline_resolution(
            configured_baseline,
            source.resolve_snapshot(),
            "failed to resolve external reference baseline snapshot for lexical search",
        )? {
            match source.lexical_search(snapshot.id.0.as_str(), query, Some("platform"), limit) {
                Ok(hits) if !hits.is_empty() => {
                    return Ok(CallToolResult::success(vec![Content::text(
                        format_lexical_doc_hits(&hits),
                    )]));
                }
                Ok(_) => {
                    return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
                }
                Err(error) => {
                    if error.is_terminal() {
                        return Err(external_baseline_mcp_error(&error));
                    }
                    warn!(
                        snapshot_id = snapshot.id.0.as_str(),
                        %error,
                        "direct lexical search failed for external reference baseline, falling back",
                    );
                }
            }

            if let Some(view) = map_reference_baseline_resolution(
                configured_baseline,
                source.resolve_reference_view(),
                "failed to resolve external reference baseline view for lexical search",
            )? {
                let hits = lexical_hits_for_resolved_view(&view, query, limit, Some("platform"));
                if !hits.is_empty() {
                    return Ok(CallToolResult::success(vec![Content::text(format_doc_hits(
                        &hits,
                    ))]));
                }
                return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
            }
        }
    }

    let Some(engine) = guard.as_ref() else {
        return Ok(CallToolResult::success(vec![Content::text(
            "Search index is being built, please try again in a moment.",
        )]));
    };
    let hits = engine
        .text_search(query, limit, Some("platform"))
        .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?;

    if hits.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
    }

    Ok(CallToolResult::success(vec![Content::text(format_doc_hits(&hits))]))
}

pub fn search_docs(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    configured_baseline: Option<&ConfiguredBaselineStatus>,
    external_baseline: Option<Arc<ExternalBaselineService>>,
    query: &str,
    limit: usize,
) -> Result<CallToolResult, McpError> {
    ensure_reference_baseline_runtime_ready(configured_baseline, external_baseline.as_ref())?;
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

    if let Some(source) = external_baseline {
        if let Some((_, snapshot)) = map_reference_baseline_resolution(
            configured_baseline,
            source.resolve_snapshot(),
            "failed to resolve external reference baseline snapshot for semantic search",
        )? {
            let query_embedding = engine
                .embed_query(query)
                .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?;
            let model_id = engine.embedding_model().ok_or_else(|| {
                McpError::internal_error(
                    "search error: semantic model id is unavailable".to_owned(),
                    None,
                )
            })?;
            let dim = engine.embedding_dimension().ok_or_else(|| {
                McpError::internal_error(
                    "search error: embedding dimension is unavailable".to_owned(),
                    None,
                )
            })?;

            match source.semantic_search(
                snapshot.id.0.as_str(),
                &query_embedding,
                model_id,
                dim,
                Some("platform"),
                limit,
            ) {
                Ok(hits) if !hits.is_empty() => {
                    return Ok(CallToolResult::success(vec![Content::text(
                        format_semantic_doc_hits(&hits),
                    )]));
                }
                Ok(_) => {
                    return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
                }
                Err(error) => {
                    if error.is_terminal() {
                        return Err(external_baseline_mcp_error(&error));
                    }
                    warn!(
                        snapshot_id = snapshot.id.0.as_str(),
                        %error,
                        "direct semantic search failed for external reference baseline, falling back",
                    );
                }
            }
        }
    }

    let hits = engine
        .search(query, limit, Some("platform"))
        .map_err(|e| McpError::internal_error(format!("search error: {e}"), None))?;

    if hits.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
    }

    Ok(CallToolResult::success(vec![Content::text(format_doc_hits(&hits))]))
}

pub fn search_status(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    progress: &Arc<IndexProgress>,
    semantic_runtime: &Arc<Mutex<SemanticRuntimeStatus>>,
    workspace_search_mode: WorkspaceSearchMode,
    configured_baseline: Option<ConfiguredBaselineStatus>,
    external_baseline: Option<Arc<ExternalBaselineService>>,
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
        if let Some(support) = configured_baseline.support.as_ref() {
            let _ = writeln!(out, "  Support:  {}", support.state.as_str());
            let _ = writeln!(out, "  Reason:   {}", support.reason);
            let _ = writeln!(
                out,
                "  Policy:   stale after {}d, expire after {}d",
                support.stale_after_days, support.expire_after_days
            );
            if matches!(support.state, project_model::SearchBaselineSupportState::Expired) {
                let _ = writeln!(out, "  Action:   update the branch from develop and restart MCP");
            }
        }
        let _ = writeln!(out);
    }

    let semantic_runtime = semantic_runtime
        .lock()
        .map_err(|e| McpError::internal_error(format!("semantic runtime lock error: {e}"), None))?
        .clone();
    let guard =
        engine.lock().map_err(|e| McpError::internal_error(format!("lock error: {e}"), None))?;

    if let Some(engine) = guard.as_ref() {
        let files = engine.file_count().unwrap_or(0);
        let chunks = engine.chunk_count().unwrap_or(0);
        let vectors = engine.vector_count();
        let semantic = engine.has_semantic();

        let code_vectors = engine.embedding_count_by_collection("code").unwrap_or(0);
        let platform_vectors = engine.embedding_count_by_collection("platform").unwrap_or(0);

        let search_state = match &semantic_runtime {
            SemanticRuntimeStatus::Failed(_) => "ready (semantic runtime failed)",
            _ => "ready",
        };
        let _ = writeln!(out, "Search index: {search_state}");
        let _ = writeln!(out, "  Files:    {files}");
        let _ = writeln!(out, "  Chunks:   {chunks}");
        let _ = writeln!(
            out,
            "  Vectors:  {vectors} (code: {code_vectors}, platform: {platform_vectors})"
        );
        let semantic_status = match &semantic_runtime {
            SemanticRuntimeStatus::Disabled => {
                if semantic {
                    "available".to_owned()
                } else {
                    "not configured (set EMBEDDING_URL)".to_owned()
                }
            }
            SemanticRuntimeStatus::OverlaySyncing => match workspace_search_mode {
                WorkspaceSearchMode::PostgresRemoteOverlay => {
                    "syncing local overlay embeddings against remote baseline".to_owned()
                }
                WorkspaceSearchMode::SqliteLocal => "syncing local semantic index".to_owned(),
            },
            SemanticRuntimeStatus::Ready => match workspace_search_mode {
                WorkspaceSearchMode::PostgresRemoteOverlay => {
                    if semantic {
                        "available (remote baseline semantic + local overlay only)".to_owned()
                    } else {
                        "not configured (set EMBEDDING_URL)".to_owned()
                    }
                }
                WorkspaceSearchMode::SqliteLocal => {
                    if semantic {
                        "available (local sqlite + local overlay)".to_owned()
                    } else {
                        "not configured (set EMBEDDING_URL)".to_owned()
                    }
                }
            },
            SemanticRuntimeStatus::Failed(_) => "failed (inspect status)".to_owned(),
        };
        let _ = writeln!(out, "  Semantic: {semantic_status}");
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
                    let code_semantic_source =
                        match (&semantic_runtime, workspace_search_mode.clone()) {
                            (SemanticRuntimeStatus::Disabled, _) => {
                                "not configured (set EMBEDDING_URL)".to_owned()
                            }
                            (
                                SemanticRuntimeStatus::OverlaySyncing,
                                WorkspaceSearchMode::PostgresRemoteOverlay,
                            ) => "remote baseline semantic + local overlay sync in progress"
                                .to_owned(),
                            (
                                SemanticRuntimeStatus::OverlaySyncing,
                                WorkspaceSearchMode::SqliteLocal,
                            ) => "local sqlite + local overlay sync in progress".to_owned(),
                            (
                                SemanticRuntimeStatus::Ready,
                                WorkspaceSearchMode::PostgresRemoteOverlay,
                            ) => "remote baseline semantic + local overlay only".to_owned(),
                            (SemanticRuntimeStatus::Ready, WorkspaceSearchMode::SqliteLocal) => {
                                if semantic {
                                    "local sqlite + local overlay".to_owned()
                                } else {
                                    "not configured (set EMBEDDING_URL)".to_owned()
                                }
                            }
                            (SemanticRuntimeStatus::Failed(_), _) => "failed".to_owned(),
                        };
                    let _ = writeln!(out, "  Code semantic source: {code_semantic_source}");
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
            let code_semantic_source = if semantic {
                "local sqlite + local overlay"
            } else {
                "not configured (set EMBEDDING_URL)"
            };
            let _ = writeln!(out, "  Code semantic source: {code_semantic_source}");
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
        if let Some(resolved) = status.resolved.as_deref() {
            let _ = writeln!(out, "  Resolved: {}", resolved);
        }
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
        let heading = "Indexing in progress";

        let _ = writeln!(out);
        let _ = writeln!(out, "{heading}: {pct}%");
        let _ = writeln!(out, "  Batches:  {done_b}/{total_b}");
        let _ = writeln!(out, "  Chunks:   {done}/{total}");
    }

    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Note: code snippets are secret-redacted (credential-like values shown as ***); \
         treat snippet text as sanitized, not byte-exact."
    );

    Ok(CallToolResult::success(vec![Content::text(out)]))
}

fn map_reference_baseline_resolution<T>(
    configured_baseline: Option<&ConfiguredBaselineStatus>,
    resolution: Result<Option<T>, SearchError>,
    failure_message: &'static str,
) -> Result<Option<T>, McpError> {
    match resolution {
        Ok(Some(value)) => Ok(Some(value)),
        Ok(None) => {
            if configured_baseline.is_some_and(|baseline| baseline.backend == "postgres") {
                return Err(reference_baseline_unavailable_mcp_error(configured_baseline));
            }
            Ok(None)
        }
        Err(error) => {
            if error.is_terminal() {
                return Err(external_baseline_mcp_error(&error));
            }
            warn!(%error, "{failure_message}");
            Ok(None)
        }
    }
}

fn reference_baseline_unavailable_mcp_error(
    configured_baseline: Option<&ConfiguredBaselineStatus>,
) -> McpError {
    let selection = configured_baseline
        .map(|baseline| baseline.selection.as_str())
        .unwrap_or("configured shared reference baseline");
    McpError::invalid_params(
        format!(
            "Shared reference baseline is unavailable. No published snapshot matched {selection}. Publish the selected baseline, restart MCP, and retry."
        ),
        Some(json!({
            "reasonCode": "baseline_unavailable",
            "selection": selection,
        })),
    )
}

fn external_baseline_mcp_error(error: &SearchError) -> McpError {
    let reason_code = error.reason_code();
    let details = json!({
        "reasonCode": reason_code,
        "details": error.to_string(),
    });

    let invalid_params_reason = matches!(
        reason_code,
        Some(
            "helper_spawn_failed"
                | "helper_timeout"
                | "helper_protocol_error"
                | "helper_rejected"
                | "resolved_target_mismatch"
                | "missing_config"
                | "storage_not_initialized"
                | "schema_version_mismatch"
        )
    );

    if invalid_params_reason {
        return McpError::invalid_params(
            format!("external baseline error: {error}"),
            Some(details),
        );
    }

    McpError::internal_error(format!("external baseline error: {error}"), Some(details))
}

fn ensure_workspace_search_allowed(
    configured_baseline: Option<&ConfiguredBaselineStatus>,
) -> Result<(), McpError> {
    let Some(configured_baseline) = configured_baseline else {
        return Ok(());
    };
    let Some(support) = configured_baseline.support.as_ref() else {
        return Ok(());
    };

    if !configured_baseline.search_is_expired() {
        return Ok(());
    }

    Err(McpError::invalid_params(
        format!(
            "Shared baseline access is expired for this branch. {}. Update the branch from develop, restart MCP, and retry.",
            support.reason
        ),
        Some(json!({
            "reasonCode": "expired_branch",
            "supportState": support.state.as_str(),
            "workspaceBranch": support.workspace_branch,
            "selectedBranch": support.selected_branch,
            "snapshotAgeDays": support.snapshot_age_days,
            "staleAfterDays": support.stale_after_days,
            "expireAfterDays": support.expire_after_days
        })),
    ))
}

fn ensure_workspace_baseline_runtime_ready(
    workspace_search_mode: WorkspaceSearchMode,
    configured_baseline: Option<&ConfiguredBaselineStatus>,
    external_baseline: Option<&Arc<ExternalBaselineService>>,
) -> Result<(), McpError> {
    if matches!(workspace_search_mode, WorkspaceSearchMode::SqliteLocal) {
        return Ok(());
    }
    let Some(configured_baseline) = configured_baseline else {
        return Err(McpError::invalid_params(
            "Postgres workspace mode requires a configured PostgreSQL baseline. Restart MCP in sqlite mode if PostgreSQL is not intended."
                .to_owned(),
            Some(json!({
                "reasonCode": "baseline_required",
            })),
        ));
    };
    if configured_baseline.backend != "postgres" {
        return Ok(());
    }
    if let Some(issue) = configured_baseline.issue.as_deref() {
        return Err(McpError::invalid_params(
            format!(
                "Shared baseline is unavailable. Fix the PostgreSQL baseline configuration, restart MCP, and retry. {issue}"
            ),
            Some(json!({
                "reasonCode": "baseline_unavailable",
                "details": issue,
            })),
        ));
    }
    if external_baseline.is_none() {
        return Err(McpError::invalid_params(
            "Shared baseline is unavailable. Restart MCP after fixing the PostgreSQL baseline configuration and retry."
                .to_owned(),
            Some(json!({
                "reasonCode": "baseline_unavailable",
            })),
        ));
    }
    Ok(())
}

fn ensure_reference_baseline_runtime_ready(
    configured_baseline: Option<&ConfiguredBaselineStatus>,
    external_baseline: Option<&Arc<ExternalBaselineService>>,
) -> Result<(), McpError> {
    let Some(configured_baseline) = configured_baseline else {
        return Ok(());
    };
    if configured_baseline.backend != "postgres" {
        return Ok(());
    }
    if let Some(issue) = configured_baseline.issue.as_deref() {
        return Err(McpError::invalid_params(
            format!(
                "Shared reference baseline is unavailable. Fix the PostgreSQL baseline configuration, restart MCP, and retry. {issue}"
            ),
            Some(json!({
                "reasonCode": "baseline_unavailable",
                "details": issue,
            })),
        ));
    }
    if external_baseline.is_none() {
        return Err(McpError::invalid_params(
            "Shared reference baseline is unavailable. Restart MCP after fixing the PostgreSQL baseline configuration and retry."
                .to_owned(),
            Some(json!({
                "reasonCode": "baseline_unavailable",
            })),
        ));
    }
    Ok(())
}

/// Durable graph id for a code-search hit, when it names a method. Returns `None` for
/// headers and non-method symbols. Module-keyed methods (common/object/manager) resolve
/// regardless of root; form/command/file-module methods fall back to the
/// `method/file/<rel>::<name>` id the graph also mints.
///
/// The call graph keys file paths against `graph_root` (the repo root it was built from), but
/// search hit paths are relative to `engine_root` (the configuration root, e.g. `src/cf`,
/// nested under it). So we re-anchor the hit to an absolute path via `engine_root`, then let
/// [`ide::method_graph_id`] strip `graph_root` back down — yielding the same `src/cf/…` prefix
/// the graph minted, so a form/file method id resolves in `graph` instead of `not_found`.
/// (Module-keyed ids are prefix-independent, so they are unaffected by the re-anchoring.)
fn graph_id_for_hit(
    hit: &SearchHit,
    engine_root: Option<&Path>,
    graph_root: Option<&Path>,
) -> Option<String> {
    if hit.symbol_name.is_empty() {
        return None;
    }
    let kind = hit.kind.to_lowercase();
    let is_method =
        ["proc", "func", "процед", "функц", "метод"].iter().any(|marker| kind.contains(marker));
    if !is_method {
        return None;
    }
    let hit_is_absolute = Path::new(&hit.file_path).is_absolute();
    match engine_root {
        // Engine-relative hit: re-anchor to an absolute path so `method_graph_id` strips the
        // graph root back to the `src/cf/…` rel the graph keyed against.
        Some(root) if !hit_is_absolute => {
            let abs = root.join(&hit.file_path);
            ide::method_graph_id(&abs.to_string_lossy(), &hit.symbol_name, graph_root)
        }
        // Already an absolute hit path: strip the graph root directly.
        _ if hit_is_absolute => ide::method_graph_id(&hit.file_path, &hit.symbol_name, graph_root),
        // Unanchored relative hit (e.g. a remote-baseline direct hit whose workspace root is
        // unreachable): the config-root prefix cannot be reconstructed, so a `method/file/…`
        // fallback id would not resolve in `graph`. Keep only the root-independent module-keyed
        // id and drop the path fallback — a missing id beats a wrong one.
        _ => ide::method_graph_id(&hit.file_path, &hit.symbol_name, None)
            .filter(|id| !id.starts_with("method/file/")),
    }
}

fn format_code_hits(
    hits: &[bsl_search::SearchHit],
    engine_root: Option<&Path>,
    graph_root: Option<&Path>,
) -> String {
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
        // Bridge into the call graph: surface the durable node id so the agent
        // can pivot to `graph` callers/callees/source.
        if let Some(id) = graph_id_for_hit(hit, engine_root, graph_root) {
            let _ = writeln!(out, "  graph_id: {id}");
        }

        // Redact secrets before emitting the source snippet (same safety net as
        // the `graph` tool's source output).
        let snippet = crate::tools::redact::redact_secrets(&hit.text);
        for line in snippet.lines().take(5) {
            let _ = writeln!(out, "  │ {line}");
        }
        let total_lines = snippet.lines().count();
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

fn format_lexical_doc_hits(hits: &[LexicalHit]) -> String {
    let mut out = String::new();

    for (i, hit) in hits.iter().enumerate() {
        let name = if hit.symbol_name.is_empty() { "<header>" } else { &hit.symbol_name };
        let _ = writeln!(
            out,
            "#{} [{:.3}] {}:{}-{} :: {} ({})",
            i + 1,
            hit.rank,
            hit.path,
            hit.line_start + 1,
            hit.line_end,
            name,
            hit.kind,
        );

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

fn format_semantic_doc_hits(hits: &[SemanticHit]) -> String {
    let mut out = String::new();

    for (i, hit) in hits.iter().enumerate() {
        let name = if hit.symbol_name.is_empty() { "<header>" } else { &hit.symbol_name };
        let _ = writeln!(
            out,
            "#{} [{:.3}] {}:{}-{} :: {} ({})",
            i + 1,
            hit.score,
            hit.path,
            hit.line_start + 1,
            hit.line_end,
            name,
            hit.kind,
        );
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
    use super::{
        external_baseline_mcp_error, find_docs, hybrid_code, map_reference_baseline_resolution,
        merge_direct_lexical_with_refill, merge_direct_semantic_with_refill, search_docs,
        search_status, semantic_code_hits, CodeHits, ConfiguredBaselineStatus, DirectResult,
        ExternalBaselineService, SemanticRuntimeStatus, SemanticUnavailable,
    };
    use crate::baseline::RefreshableExternalBaselineSource;
    use crate::state::WorkspaceSearchMode;
    use bsl_search::{
        lexical_hits_for_resolved_view, BaselineRef, CorpusId, Document, IndexProgress,
        IndexedDocument, LexicalHit, ResolvedView, SearchEngine, SearchError, SemanticHit,
    };
    use project_model::{
        ResolvedWorkspaceBaselineSupport, SearchBaselineSupportState, SearchPostgresConfig,
        SearchPostgresCredentialHelperConfig,
    };
    use rmcp::model::ErrorCode;
    use std::collections::HashSet;
    use std::fs;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    fn retryable_postgres_source() -> Arc<ExternalBaselineService> {
        let postgres = SearchPostgresConfig {
            host: Some("127.0.0.1".to_owned()),
            port: Some(1),
            dbname: Some("bsl_search".to_owned()),
            schema: Some("bsl_search".to_owned()),
            vault_role_base: Some("prod/search/bsl-analyzer".to_owned()),
            credential_helper: SearchPostgresCredentialHelperConfig {
                program: Some("python3".to_owned()),
                args: vec![
                    "-c".to_owned(),
                    "import json; print(json.dumps({'protocol':'bsl-analyzer.postgres-helper.v1','ok':True,'url':'postgres://127.0.0.1:1/bsl_search'}))".to_owned(),
                ],
            },
        };
        ExternalBaselineService::for_test(
            RefreshableExternalBaselineSource::for_test_with_refresh_context(
                bsl_search::ExternalBaselineConfig::postgres("postgres://127.0.0.1:1/bsl_search"),
                BaselineRef {
                    corpus: CorpusId::WorkspaceCode,
                    snapshot_id: None,
                    branch: Some("main".to_owned()),
                    commit: None,
                },
                postgres,
            )
            .unwrap(),
        )
    }

    fn lexical_hit(path: &str, symbol_name: &str, rank: f32) -> LexicalHit {
        LexicalHit {
            collection: "code".to_owned(),
            path: path.to_owned(),
            symbol_name: symbol_name.to_owned(),
            kind: "procedure".to_owned(),
            line_start: 1,
            line_end: 10,
            text: format!("procedure {symbol_name}"),
            rank,
        }
    }

    fn semantic_hit(path: &str, symbol_name: &str, score: f32) -> SemanticHit {
        SemanticHit {
            collection: "code".to_owned(),
            path: path.to_owned(),
            symbol_name: symbol_name.to_owned(),
            kind: "procedure".to_owned(),
            line_start: 1,
            line_end: 10,
            score,
        }
    }

    fn code_hit(file_path: &str, symbol: &str, kind: &str) -> bsl_search::SearchHit {
        bsl_search::SearchHit {
            collection: "code".to_owned(),
            file_path: file_path.to_owned(),
            symbol_name: symbol.to_owned(),
            kind: kind.to_owned(),
            text: String::new(),
            line_start: 0,
            line_end: 1,
            score: 1.0,
        }
    }

    #[test]
    fn index_progress_suffix_appended_only_when_active() {
        use std::sync::atomic::Ordering;
        let progress = IndexProgress::new();
        // Inactive → message unchanged (no misleading 0% appended).
        assert_eq!(super::with_index_progress("building".to_owned(), &progress), "building");
        // Active → the percent + batch counts from `search(status)` are surfaced inline.
        progress.active.store(true, Ordering::Relaxed);
        progress.total_chunks.store(100, Ordering::Relaxed);
        progress.done_chunks.store(77, Ordering::Relaxed);
        progress.total_batches.store(10, Ordering::Relaxed);
        progress.done_batches.store(7, Ordering::Relaxed);
        assert_eq!(
            super::with_index_progress("building".to_owned(), &progress),
            "building (indexing 77% — 7/10 batches)",
        );
    }

    #[test]
    fn graph_id_bridges_method_hits_in_modules() {
        // The graph was built from the repo root; the search engine indexes paths relative to
        // the nested configuration root (`src/cf`). These are the two roots the bridge spans.
        let engine_root = std::path::Path::new("/repo/src/cf");
        let graph_root = std::path::Path::new("/repo");

        // A method in a common module gets a durable, prefix-independent graph id — the
        // re-anchoring through the two roots does not perturb a module-keyed id.
        assert_eq!(
            super::graph_id_for_hit(
                &code_hit("CommonModules/Утилиты/Ext/Module.bsl", "ПроверитьИНН", "procedure"),
                Some(engine_root),
                Some(graph_root),
            ),
            Some("method/common/Утилиты/ПроверитьИНН".to_owned()),
        );
        // A non-method symbol gets no id.
        assert_eq!(
            super::graph_id_for_hit(
                &code_hit("CommonModules/Утилиты/Ext/Module.bsl", "МодульнаяПерем", "variable"),
                Some(engine_root),
                Some(graph_root),
            ),
            None,
        );
        // A form-module method falls back to `method/file/<rel>::<name>`, and the rel MUST
        // carry the `src/cf/` prefix the graph minted — otherwise `graph(node)` returns
        // `not_found`. The engine-relative hit path is re-anchored to the graph root to add it.
        assert_eq!(
            super::graph_id_for_hit(
                &code_hit(
                    "Catalogs/Контрагенты/Forms/Форма/Ext/Form/Module.bsl",
                    "ПриОткрытии",
                    "procedure",
                ),
                Some(engine_root),
                Some(graph_root),
            ),
            Some(
                "method/file/src/cf/Catalogs/Контрагенты/Forms/Форма/Ext/Form/Module.bsl::ПриОткрытии"
                    .to_owned()
            ),
        );
        // With no engine root (e.g. a remote-baseline hit whose root is unreachable) the bridge
        // cannot reconstruct the `src/cf/` prefix, so a path-fallback id would not resolve — it
        // is dropped rather than emitted prefix-less and wrong.
        assert_eq!(
            super::graph_id_for_hit(
                &code_hit(
                    "Catalogs/Контрагенты/Forms/Форма/Ext/Form/Module.bsl",
                    "ПриОткрытии",
                    "procedure",
                ),
                None,
                None,
            ),
            None,
        );
        // A module-keyed id is root-independent, so it still resolves even unanchored.
        assert_eq!(
            super::graph_id_for_hit(
                &code_hit("CommonModules/Утилиты/Ext/Module.bsl", "ПроверитьИНН", "procedure"),
                None,
                None,
            ),
            Some("method/common/Утилиты/ПроверитьИНН".to_owned()),
        );
    }

    #[test]
    fn direct_lexical_refill_recovers_results_hidden_by_overlay() {
        let hidden_paths = HashSet::from([
            "src/hidden1.bsl".to_owned(),
            "src/hidden2.bsl".to_owned(),
            "src/hidden3.bsl".to_owned(),
            "src/hidden4.bsl".to_owned(),
            "src/hidden5.bsl".to_owned(),
            "src/hidden6.bsl".to_owned(),
            "src/hidden7.bsl".to_owned(),
            "src/hidden8.bsl".to_owned(),
            "src/hidden9.bsl".to_owned(),
        ]);
        let baseline = vec![
            lexical_hit("src/hidden1.bsl", "Hidden1", 100.0),
            lexical_hit("src/hidden2.bsl", "Hidden2", 99.0),
            lexical_hit("src/hidden3.bsl", "Hidden3", 98.0),
            lexical_hit("src/hidden4.bsl", "Hidden4", 97.0),
            lexical_hit("src/hidden5.bsl", "Hidden5", 96.0),
            lexical_hit("src/hidden6.bsl", "Hidden6", 95.0),
            lexical_hit("src/hidden7.bsl", "Hidden7", 94.0),
            lexical_hit("src/hidden8.bsl", "Hidden8", 93.0),
            lexical_hit("src/hidden9.bsl", "Hidden9", 92.0),
            lexical_hit("src/visible1.bsl", "Visible1", 91.0),
            lexical_hit("src/visible2.bsl", "Visible2", 90.0),
            lexical_hit("src/visible3.bsl", "Visible3", 89.0),
        ];
        let mut requested_limits = Vec::new();

        let result = merge_direct_lexical_with_refill(&[], &hidden_paths, 3, |fetch_limit| {
            requested_limits.push(fetch_limit);
            Ok(baseline.iter().take(fetch_limit).cloned().collect())
        });

        let DirectResult::Found(hits) = result else {
            panic!("expected lexical refill to produce hits");
        };
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits.iter().map(|hit| hit.file_path.as_str()).collect::<Vec<_>>(),
            vec!["src/visible1.bsl", "src/visible2.bsl", "src/visible3.bsl"]
        );
        assert_eq!(requested_limits, vec![9, 18]);
    }

    #[test]
    fn direct_semantic_refill_recovers_results_hidden_by_overlay() {
        let hidden_paths = HashSet::from([
            "src/hidden1.bsl".to_owned(),
            "src/hidden2.bsl".to_owned(),
            "src/hidden3.bsl".to_owned(),
            "src/hidden4.bsl".to_owned(),
            "src/hidden5.bsl".to_owned(),
            "src/hidden6.bsl".to_owned(),
            "src/hidden7.bsl".to_owned(),
            "src/hidden8.bsl".to_owned(),
            "src/hidden9.bsl".to_owned(),
        ]);
        let baseline = vec![
            semantic_hit("src/hidden1.bsl", "Hidden1", 1.00),
            semantic_hit("src/hidden2.bsl", "Hidden2", 0.99),
            semantic_hit("src/hidden3.bsl", "Hidden3", 0.98),
            semantic_hit("src/hidden4.bsl", "Hidden4", 0.97),
            semantic_hit("src/hidden5.bsl", "Hidden5", 0.96),
            semantic_hit("src/hidden6.bsl", "Hidden6", 0.95),
            semantic_hit("src/hidden7.bsl", "Hidden7", 0.94),
            semantic_hit("src/hidden8.bsl", "Hidden8", 0.93),
            semantic_hit("src/hidden9.bsl", "Hidden9", 0.92),
            semantic_hit("src/visible1.bsl", "Visible1", 0.91),
            semantic_hit("src/visible2.bsl", "Visible2", 0.90),
            semantic_hit("src/visible3.bsl", "Visible3", 0.89),
        ];
        let mut requested_limits = Vec::new();

        let result = merge_direct_semantic_with_refill(&[], &hidden_paths, 3, |fetch_limit| {
            requested_limits.push(fetch_limit);
            Ok(baseline.iter().take(fetch_limit).cloned().collect())
        });

        let DirectResult::Found(hits) = result else {
            panic!("expected semantic refill to produce hits");
        };
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits.iter().map(|hit| hit.file_path.as_str()).collect::<Vec<_>>(),
            vec!["src/visible1.bsl", "src/visible2.bsl", "src/visible3.bsl"]
        );
        assert_eq!(requested_limits, vec![9, 18]);
    }

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
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            WorkspaceSearchMode::SqliteLocal,
            Some(ConfiguredBaselineStatus {
                backend: "sqlite",
                selection: "local workspace index".to_owned(),
                issue: None,
                support: None,
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
        let source = ExternalBaselineService::for_test(
            RefreshableExternalBaselineSource::for_test(
                bsl_search::ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
                bsl_search::BaselineRef {
                    corpus: bsl_search::CorpusId::WorkspaceCode,
                    snapshot_id: None,
                    branch: Some("main".to_owned()),
                    commit: None,
                },
            )
            .unwrap(),
        );

        let result = search_status(
            &Arc::new(Mutex::new(None)),
            &Arc::new(IndexProgress::default()),
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            WorkspaceSearchMode::PostgresRemoteOverlay,
            Some(ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "branch main".to_owned(),
                issue: None,
                support: None,
            }),
            Some(source),
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
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            WorkspaceSearchMode::SqliteLocal,
            Some(ConfiguredBaselineStatus {
                backend: "sqlite",
                selection: "local reference index".to_owned(),
                issue: None,
                support: None,
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
        let source = ExternalBaselineService::for_test(
            RefreshableExternalBaselineSource::for_test(
                bsl_search::ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
                bsl_search::BaselineRef {
                    corpus: bsl_search::CorpusId::Reference,
                    snapshot_id: None,
                    branch: None,
                    commit: None,
                },
            )
            .unwrap(),
        );

        let error =
            search_docs(&Arc::new(Mutex::new(Some(engine))), None, Some(source), "Массив", 10)
                .unwrap_err();

        assert!(error.message.contains("Semantic search not available"));
        assert!(!error.message.contains("centralized reference baseline"));
    }

    #[test]
    fn find_docs_rejects_local_fallback_when_reference_postgres_baseline_is_unavailable() {
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

        let error = find_docs(
            &Arc::new(Mutex::new(Some(engine))),
            Some(&ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "latest reference".to_owned(),
                issue: Some("failed to resolve PostgreSQL reader credentials".to_owned()),
                support: None,
            }),
            None,
            "Массив",
            10,
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert!(error.message.contains("Shared reference baseline is unavailable"));
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("reasonCode"))
                .and_then(|value| value.as_str()),
            Some("baseline_unavailable")
        );
    }

    #[test]
    fn search_docs_rejects_reference_postgres_baseline_unavailability_before_semantic_validation() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("reference-search.db");
        let engine = SearchEngine::fts_only(&db_path).unwrap();

        let error = search_docs(
            &Arc::new(Mutex::new(Some(engine))),
            Some(&ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "latest reference".to_owned(),
                issue: Some("failed to resolve PostgreSQL reader credentials".to_owned()),
                support: None,
            }),
            None,
            "Массив",
            10,
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert!(error.message.contains("Shared reference baseline is unavailable"));
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("reasonCode"))
                .and_then(|value| value.as_str()),
            Some("baseline_unavailable")
        );
    }

    #[test]
    fn missing_reference_snapshot_maps_to_baseline_unavailable_error() {
        let error = map_reference_baseline_resolution::<()>(
            Some(&ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "snapshot reference:0.1.104".to_owned(),
                issue: None,
                support: None,
            }),
            Ok(None),
            "test reference snapshot resolution",
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert!(error.message.contains("Shared reference baseline is unavailable"));
        assert!(error.message.contains("snapshot reference:0.1.104"));
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("reasonCode"))
                .and_then(|value| value.as_str()),
            Some("baseline_unavailable")
        );
    }

    #[test]
    fn missing_reference_snapshot_still_allows_local_sqlite_mode() {
        let result = map_reference_baseline_resolution::<()>(
            Some(&ConfiguredBaselineStatus {
                backend: "sqlite",
                selection: "local reference index".to_owned(),
                issue: None,
                support: None,
            }),
            Ok(None),
            "test reference snapshot resolution",
        )
        .unwrap();

        assert!(result.is_none());
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
                    graph_context: None,
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
                    graph_context: None,
                },
            ],
        );

        let hits = lexical_hits_for_resolved_view(&view, "НайтиПроцедуру", 10, Some("code"));

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].file_path, "A.bsl");
        assert!(hits[0].score > hits[1].score);
    }

    #[test]
    fn semantic_core_reports_unavailable_when_runtime_failed() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("workspace-search.db");
        let engine = Arc::new(Mutex::new(Some(SearchEngine::fts_only(&db_path).unwrap())));
        let outcome = semantic_code_hits(
            &engine,
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Failed("overlay sync failed".to_owned()))),
            WorkspaceSearchMode::SqliteLocal,
            None,
            None,
            "обработка проведения документа",
            10,
        )
        .unwrap();

        // A failed semantic runtime is a soft shortfall the hybrid path degrades past, not a
        // hard error.
        assert!(matches!(outcome, CodeHits::Unavailable(SemanticUnavailable::RuntimeFailed)));
    }

    #[test]
    fn hybrid_degrades_to_lexical_with_note_when_semantic_runtime_failed() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        fs::write(workspace.join("CommonModule.bsl"), "Процедура ПроверитьИНН()\nКонецПроцедуры")
            .unwrap();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);

        let result = hybrid_code(
            &Arc::new(Mutex::new(Some(engine))),
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Failed("overlay sync failed".to_owned()))),
            WorkspaceSearchMode::SqliteLocal,
            None,
            None,
            None,
            &IndexProgress::new(),
            "ПроверитьИНН",
            10,
        )
        .unwrap();
        let text = result.content[0].raw.as_text().expect("text content").text.as_str();

        // The lexical hit still comes back...
        assert!(text.contains("ПроверитьИНН"), "{text}");
        // ...with a trailing note explaining semantic was skipped (after the hit lines).
        assert!(text.contains("-- semantic skipped: runtime initialization failed --"), "{text}");
    }

    #[test]
    fn search_status_reports_overlay_sync_for_postgres_mode() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("CommonModule.bsl");
        fs::write(
            &file,
            "Процедура СтараяПроцедура()
КонецПроцедуры",
        )
        .unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);

        let progress = Arc::new(IndexProgress::default());
        progress.active.store(true, Ordering::Relaxed);
        progress.total_chunks.store(200, Ordering::Relaxed);
        progress.done_chunks.store(50, Ordering::Relaxed);
        progress.total_batches.store(20, Ordering::Relaxed);
        progress.done_batches.store(5, Ordering::Relaxed);

        let result = search_status(
            &Arc::new(Mutex::new(Some(engine))),
            &progress,
            &Arc::new(Mutex::new(SemanticRuntimeStatus::OverlaySyncing)),
            WorkspaceSearchMode::PostgresRemoteOverlay,
            Some(ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "branch develop".to_owned(),
                issue: None,
                support: None,
            }),
            None,
        )
        .unwrap();
        let text = result.content[0].raw.as_text().expect("expected text content").text.as_str();

        assert!(text.contains("Search index: ready"));
        assert!(text.contains("Semantic: syncing local overlay embeddings against remote baseline"));
        assert!(text.contains("Indexing in progress: 25%"));
    }

    #[test]
    fn code_search_returns_structured_error_when_workspace_branch_is_expired() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("workspace-search.db");
        let engine = Arc::new(Mutex::new(Some(SearchEngine::fts_only(&db_path).unwrap())));
        let configured = ConfiguredBaselineStatus {
            backend: "postgres",
            selection: "workspace branch feature/demo -> branch develop -> branch vendor".to_owned(),
            issue: None,
            support: Some(ResolvedWorkspaceBaselineSupport {
                state: SearchBaselineSupportState::Expired,
                workspace_branch: Some("feature/demo".to_owned()),
                selected_branch: Some("develop".to_owned()),
                snapshot_age_days: 45,
                stale_after_days: 21,
                expire_after_days: 30,
                reason: "workspace branch 'feature/demo' uses shared baseline branch 'develop' published 45 days ago".to_owned(),
            }),
        };

        let error = hybrid_code(
            &engine,
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            WorkspaceSearchMode::PostgresRemoteOverlay,
            Some(&configured),
            None,
            None,
            &IndexProgress::new(),
            "Процедура",
            10,
        )
        .unwrap_err();

        assert!(error.message.contains("expired"));
        assert!(error.message.contains("Update the branch from develop"));
    }

    #[test]
    fn code_search_rejects_local_fallback_when_postgres_baseline_is_unavailable() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("CommonModule.bsl");
        fs::write(&file, "Процедура ТестоваПроцедура()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);

        let error = hybrid_code(
            &Arc::new(Mutex::new(Some(engine))),
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            WorkspaceSearchMode::PostgresRemoteOverlay,
            Some(&ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "branch main".to_owned(),
                issue: Some("failed to resolve PostgreSQL reader credentials".to_owned()),
                support: None,
            }),
            None,
            None,
            &IndexProgress::new(),
            "ТестоваПроцедура",
            10,
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert!(error.message.contains("Shared baseline is unavailable"));
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("reasonCode"))
                .and_then(|value| value.as_str()),
            Some("baseline_unavailable")
        );
    }

    #[test]
    fn code_search_surfaces_retry_exhausted_external_baseline_errors() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("CommonModule.bsl");
        fs::write(&file, "Процедура ТестоваПроцедура()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);

        let error = hybrid_code(
            &Arc::new(Mutex::new(Some(engine))),
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            WorkspaceSearchMode::PostgresRemoteOverlay,
            Some(&ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "branch main".to_owned(),
                issue: None,
                support: None,
            }),
            Some(retryable_postgres_source()),
            None,
            &IndexProgress::new(),
            "ТестоваПроцедура",
            10,
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
        assert!(error.message.contains("external baseline error"));
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("reasonCode"))
                .and_then(|value| value.as_str()),
            Some("refresh_retry_exhausted")
        );
    }

    #[test]
    fn code_search_surfaces_retry_exhausted_errors_for_empty_queries() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("workspace-search.db");
        let engine = Arc::new(Mutex::new(Some(SearchEngine::fts_only(&db_path).unwrap())));

        let error = hybrid_code(
            &engine,
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            WorkspaceSearchMode::PostgresRemoteOverlay,
            Some(&ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "branch main".to_owned(),
                issue: None,
                support: None,
            }),
            Some(retryable_postgres_source()),
            None,
            &IndexProgress::new(),
            "НесуществующееСлово",
            10,
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("reasonCode"))
                .and_then(|value| value.as_str()),
            Some("refresh_retry_exhausted")
        );
    }

    #[test]
    fn search_docs_falls_back_to_local_sqlite_when_external_semantic_fails() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("reference-search.db");
        let engine = SearchEngine::fts_only(&db_path).unwrap();

        let source = ExternalBaselineService::for_test(
            RefreshableExternalBaselineSource::for_test(
                bsl_search::ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
                BaselineRef {
                    corpus: CorpusId::Reference,
                    snapshot_id: Some(bsl_search::SnapshotId::new("ref:0.1.0")),
                    branch: None,
                    commit: None,
                },
            )
            .unwrap(),
        );

        let result =
            search_docs(&Arc::new(Mutex::new(Some(engine))), None, Some(source), "Массив", 10)
                .unwrap_err();

        assert!(result.message.contains("Semantic search not available"));
    }

    #[test]
    fn external_baseline_storage_error_maps_to_invalid_params() {
        let error = SearchError::StorageNotInitialized { schema: "bsl_search".to_owned() };

        let mcp_error = external_baseline_mcp_error(&error);

        assert_eq!(mcp_error.code, ErrorCode::INVALID_PARAMS);
        assert!(mcp_error.message.contains("external baseline error"));
        assert_eq!(
            mcp_error
                .data
                .as_ref()
                .and_then(|data| data.get("reasonCode"))
                .and_then(|value| value.as_str()),
            Some("storage_not_initialized")
        );
    }

    #[test]
    fn external_baseline_connectivity_error_maps_to_internal_error() {
        let error =
            SearchError::ExternalBaseline("postgres_connect_failed: connection refused".to_owned());

        let mcp_error = external_baseline_mcp_error(&error);

        assert_eq!(mcp_error.code, ErrorCode::INTERNAL_ERROR);
        assert!(mcp_error.message.contains("external baseline error"));
        assert_eq!(
            mcp_error
                .data
                .as_ref()
                .and_then(|data| data.get("reasonCode"))
                .and_then(|value| value.as_str()),
            Some("postgres_connect_failed")
        );
    }
}
