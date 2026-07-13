mod acquire;
mod gating;
mod render;
mod status;
#[cfg(test)]
mod test_support;
mod types;

use acquire::{acquire_engine_within, engine_lock_poisoned_error, try_acquire_engine};
use gating::{
    ensure_reference_baseline_runtime_ready, ensure_workspace_baseline_runtime_ready,
    ensure_workspace_search_allowed, external_baseline_mcp_error,
    map_reference_baseline_resolution,
};
#[cfg(test)]
use render::graph_id_for_hit;
use render::{
    format_baseline_ref, format_code_hits, format_doc_hits, format_lexical_doc_hits,
    format_semantic_doc_hits,
};
pub(crate) use status::baseline_warming_not_ready;
use status::search_not_ready;
pub use status::search_status;
#[cfg(test)]
use status::{search_status_with_cap, with_index_progress};
use types::{
    direct_search_initial_window, direct_search_max_window, AcquireFailure, CodeHits,
    DirectResolve, DirectResult, SemanticUnavailable, DIRECT_SEARCH_MAX_REFILL_ROUNDS,
    HYBRID_FETCH_MULTIPLIER,
};

use crate::baseline::{ConfiguredBaselineStatus, ExternalBaselineService};
use crate::state::{SemanticRuntimeStatus, WorkspaceSearchMode};
use bsl_search::{
    fuse_smart, lexical_hits_for_resolved_view, merge_context_for_collection, merge_lexical,
    merge_semantic, FusedHit, IndexProgress, LexicalHit, SearchEngine, SearchError, SearchHit,
    SemanticHit,
};
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;
use std::collections::HashSet;
use std::fmt::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::warn;

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
        workspace_search_mode.clone(),
        configured_baseline,
        external_baseline.as_ref(),
    )?;
    // Reindex dirty overlay paths from the shared resident parse before serving. Runs OFF the
    // engine lock and no-ops when the resident is unavailable.
    crate::state::SharedState::prefetch_resident_overlay(engine);
    let guard = match try_acquire_engine(engine) {
        Ok(g) => g,
        Err(AcquireFailure::Poisoned) => return Err(engine_lock_poisoned_error()),
        Err(AcquireFailure::TimedOut) => {
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
                "Search index is busy (a long operation is holding it); please try again in a moment."
                    .to_owned(),
            ));
        }
    };

    let hits = if let Some(source) = external_baseline {
        match guard.as_ref() {
            Some(engine) => {
                let direct_start = std::time::Instant::now();
                let direct = try_direct_lexical_code(engine, &source, query, limit);
                tracing::debug!(
                    elapsed_ms = direct_start.elapsed().as_millis() as u64,
                    query_len = query.len(),
                    "search.code: try_direct_lexical_code"
                );
                match direct {
                    DirectResult::Found(hits) => hits,
                    DirectResult::Terminal(error) => {
                        return Err(external_baseline_mcp_error(&error));
                    }
                    // Direct baseline serving is unavailable (snapshot, overlay, or a transient
                    // serving-table absence). Do NOT fall back to `resolve_workspace_view`:
                    // that loads the whole baseline corpus under the engine lock and stalls
                    // search past the client timeout on a large remote overlay.
                    //
                    // In PostgresRemoteOverlay mode the local store has no baseline rows, so
                    // local `text_search` would silently return overlay-only or empty results
                    // while the real corpus is unreachable — a misleading "no matches found"
                    // instead of an honest transient state. Surface it as Pending so the caller
                    // retries.
                    //
                    // In SqliteLocal mode the local store IS the full corpus, so `text_search`
                    // is the correct bounded answer.
                    DirectResult::Unavailable => {
                        if matches!(
                            workspace_search_mode,
                            WorkspaceSearchMode::PostgresRemoteOverlay
                        ) {
                            return Ok(CodeHits::Pending(
                                "Baseline lexical serving is temporarily unavailable; \
                                 please retry shortly."
                                    .to_owned(),
                            ));
                        }
                        let fallback_start = std::time::Instant::now();
                        let hits = engine.text_search(query, limit, Some("code")).map_err(|e| {
                            McpError::internal_error(format!("search error: {e}"), None)
                        })?;
                        tracing::debug!(
                            elapsed_ms = fallback_start.elapsed().as_millis() as u64,
                            query_len = query.len(),
                            "search.code: lexical fallback text_search (baseline unavailable)"
                        );
                        hits
                    }
                }
            }
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
    // Reindex dirty overlay paths from the shared resident parse before serving. Runs OFF the
    // engine lock and no-ops when the resident is unavailable.
    crate::state::SharedState::prefetch_resident_overlay(engine);
    let guard = match try_acquire_engine(engine) {
        Ok(g) => g,
        Err(AcquireFailure::Poisoned) => return Err(engine_lock_poisoned_error()),
        Err(AcquireFailure::TimedOut) => {
            return Ok(CodeHits::Pending(
                "Semantic search is busy (a long operation is holding the index). Lexical search is available in the meantime."
                    .to_owned(),
            ));
        }
    };
    {
        let Some(engine) = guard.as_ref() else {
            return Ok(CodeHits::Pending(
                "Search index is being built, please try again in a moment.".to_owned(),
            ));
        };

        if let SemanticRuntimeStatus::Failed(_) = semantic_runtime {
            return Ok(CodeHits::Unavailable(SemanticUnavailable::RuntimeFailed));
        }

        // The fused engine is published before its vectors exist. Degrade to lexical until
        // the background pass swaps in a populated index, rather than searching the empty
        // one and reporting a silent zero.
        if let SemanticRuntimeStatus::Indexing = semantic_runtime {
            return Ok(CodeHits::Pending(
                "RAG semantic index is still building; lexical search is available in the meantime."
                    .to_owned(),
            ));
        }

        if !engine.has_semantic() {
            return Ok(CodeHits::Unavailable(SemanticUnavailable::NotConfigured));
        }

        // Best-effort identity gate, kept under the guard and *before* the embed: the reader's
        // query vectors are only comparable against the baseline's stored vectors if both were
        // produced by the same embedding model/dimension. A mismatch means the embed could never
        // match, so checking it here (cheap) avoids paying for a wasted ~1.4s query embed. On a
        // mismatch, name the exact reason (and the knobs to fix it) instead of silently returning
        // lexical-only. A baseline with no recorded identity, or a read error, falls through to the
        // existing behavior rather than hard-failing.
        if let Some(source) = external_baseline.as_ref() {
            match source.embedding_identity() {
                Ok(Some((baseline_model, baseline_dim))) => {
                    let reader_model = engine.embedding_model().unwrap_or("unset");
                    let reader_dim = engine.embedding_dimension();
                    if reader_model != baseline_model || reader_dim != Some(baseline_dim) {
                        let reader_dim = reader_dim
                            .map(|dim| dim.to_string())
                            .unwrap_or_else(|| "unset".to_owned());
                        let msg = format!(
                            "semantic skipped: this baseline was indexed with model \
                             '{baseline_model}' (dim {baseline_dim}), but the reader is configured \
                             with model '{reader_model}' (dim {reader_dim}); set \
                             EMBEDDING_MODEL/EMBEDDING_DIM (or [search.baseline.embedding] in \
                             bsl-analyzer.toml) to match and restart"
                        );
                        return Ok(CodeHits::Unavailable(SemanticUnavailable::IdentityMismatch(
                            msg,
                        )));
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::debug!(
                        "failed to read baseline embedding identity for validation: {error}"
                    );
                }
            }
        }
    }

    // Capture everything needed from the engine while holding the guard, then drop it so the
    // ~1.4s embed does not serialize every concurrent search on the single engine Mutex.
    // `model_id` and `dim` are captured here so `resolve_direct_semantic` (called lock-free
    // below) can gate baseline readiness without re-acquiring the engine.
    //
    // These captures cannot go stale across the unlocked embed window: the engine's embedding
    // identity (embedder/model/dimension) is fixed for the life of the process — built once from
    // the startup env config and never reconfigured. The only runtime mutation under the engine
    // lock is `set_vector_index` (the background pass swapping in the populated index, built from
    // the same config), which preserves model and dimension. So the captured embedder/model_id/dim
    // stay consistent with the engine the second guard sees. (If a model-reconfiguration path is
    // ever added, re-validate identity under the second guard.)
    let (embedder, workspace_root, model_id, dim) = {
        let engine = guard.as_ref().expect("checked is_none above");
        (
            engine.embedder_clone(),
            engine.workspace_root().map(Path::to_path_buf),
            engine.embedding_model().map(str::to_owned),
            engine.embedding_dimension(),
        )
    };
    drop(guard);

    let Some(embedder) = embedder else {
        return Ok(CodeHits::Unavailable(SemanticUnavailable::NotConfigured));
    };

    // Resolve external baseline readiness BEFORE embedding. The snapshot actor round-trip is
    // cheap and needs no engine lock; the embed (~1.4s) must not fire on a not-ready baseline
    // (it would be wasted, and the caller would receive EmbedderUnavailable instead of the
    // correct BaselineNotReady). `resolve_direct_semantic` uses the model_id/dim captured above.
    let resolved_baseline: Option<DirectResolve> = if let Some(ref source) = external_baseline {
        let resolve_start = std::time::Instant::now();
        let r = resolve_direct_semantic(source, model_id.as_deref(), dim);
        tracing::debug!(
            elapsed_ms = resolve_start.elapsed().as_millis() as u64,
            "search.code: resolve_direct_semantic"
        );
        match r {
            DirectResolve::Terminal(e) => return Err(external_baseline_mcp_error(&e)),
            DirectResolve::Unavailable => {
                // Baseline not ready: the PostgresRemoteOverlay mode has no local fallback.
                if matches!(workspace_search_mode, WorkspaceSearchMode::PostgresRemoteOverlay) {
                    return Ok(CodeHits::Unavailable(SemanticUnavailable::BaselineNotReady));
                }
                None // Non-Postgres: continue to the local path without embedding for baseline.
            }
            r @ DirectResolve::Ready { .. } => Some(r),
        }
    } else {
        // No external baseline: PostgresRemoteOverlay requires one.
        if matches!(workspace_search_mode, WorkspaceSearchMode::PostgresRemoteOverlay) {
            return Ok(CodeHits::Unavailable(SemanticUnavailable::BaselineRequired));
        }
        None
    };

    // Embed lock-free now that readiness is confirmed (either baseline Ready or local path).
    let embed_start = std::time::Instant::now();
    let embed_result = embedder.embed(query);
    tracing::debug!(
        elapsed_ms = embed_start.elapsed().as_millis() as u64,
        query_len = query.len(),
        "search.code: embedder.embed (off-lock)"
    );
    let query_embedding = match embed_result {
        Ok(vector) => vector,
        // The query embed is a request-time call to a remote embedder on the hot path of every
        // search. When it times out or transiently fails, degrade to the lexical hits the caller
        // already has rather than failing the whole tool. A non-embedder error means something
        // structural is broken, which is worth surfacing as a hard error.
        Err(SearchError::Embedder(detail)) => {
            warn!("semantic: query embed failed, degrading to lexical: {detail}");
            return Ok(CodeHits::Unavailable(SemanticUnavailable::EmbedderUnavailable(detail)));
        }
        Err(e) => return Err(McpError::internal_error(format!("search error: {e}"), None)),
    };

    // Re-acquire the lock for the now-fast search. The engine may have changed while unlocked, so
    // re-check the readiness conditions that gate a semantic search.
    let guard = match try_acquire_engine(engine) {
        Ok(g) => g,
        Err(AcquireFailure::Poisoned) => return Err(engine_lock_poisoned_error()),
        Err(AcquireFailure::TimedOut) => {
            return Ok(CodeHits::Pending(
                "Semantic search is busy (a long operation is holding the index). Lexical search is available in the meantime."
                    .to_owned(),
            ));
        }
    };
    let Some(engine) = guard.as_ref() else {
        return Ok(CodeHits::Pending(
            "Search index is being built, please try again in a moment.".to_owned(),
        ));
    };
    if !engine.has_semantic() {
        return Ok(CodeHits::Unavailable(SemanticUnavailable::NotConfigured));
    }

    if let Some(DirectResolve::Ready { snapshot, model_id: ref mid, dim: d }) = resolved_baseline {
        // `external_baseline` is still live (the Arc was not consumed); borrow the service for
        // the search call.
        let source = external_baseline.as_ref().expect("resolved_baseline=Some implies Some");
        let direct_start = std::time::Instant::now();
        let direct =
            run_direct_semantic(engine, source, &snapshot, mid, d, &query_embedding, limit);
        tracing::debug!(
            elapsed_ms = direct_start.elapsed().as_millis() as u64,
            "search.code: run_direct_semantic (under lock)"
        );
        match direct {
            DirectResult::Found(hits) => {
                return Ok(CodeHits::Ready { hits, workspace_root });
            }
            DirectResult::Terminal(error) => {
                return Err(external_baseline_mcp_error(&error));
            }
            DirectResult::Unavailable => {
                if matches!(workspace_search_mode, WorkspaceSearchMode::PostgresRemoteOverlay) {
                    return Ok(CodeHits::Unavailable(SemanticUnavailable::BaselineNotReady));
                }
                // Non-Postgres: fall through to local search.
            }
        }
    }

    if matches!(workspace_search_mode, WorkspaceSearchMode::PostgresRemoteOverlay) {
        return Ok(CodeHits::Unavailable(SemanticUnavailable::BaselineRequired));
    }

    match engine.search_with_embedding(&query_embedding, limit, Some("code")) {
        Ok(hits) => Ok(CodeHits::Ready { hits, workspace_root }),
        Err(e) => Err(McpError::internal_error(format!("search error: {e}"), None)),
    }
}

/// The unified code search: run lexical and semantic, fuse by `fuse_smart` (exact-symbol tier
/// then semantic tail), and degrade to lexical (with a trailing note) when semantic cannot serve.
/// This is what the `search_code` action dispatches to.
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
    max_output_tokens: usize,
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
        // Lexical is the floor: if it cannot serve yet, the whole search cannot — return a
        // structured not-ready envelope (machine status + live counters + retry hint),
        // matching the graph tool, rather than a bare sentence a poller must parse.
        CodeHits::Pending(message) => {
            return Ok(search_not_ready(&message, index_progress));
        }
        // Lexical search is always available, so it never reports a semantic shortfall; treat
        // it defensively as "still building".
        CodeHits::Unavailable(_) => {
            return Ok(search_not_ready(
                "Search index is being built, please try again in a moment.",
                index_progress,
            ));
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

    let (mut hits, note): (Vec<FusedHit>, Option<String>) = match semantic {
        CodeHits::Ready { hits: sem_hits, .. } => {
            (fuse_smart(&lex_hits, &sem_hits, query, limit), None)
        }
        // Semantic could not serve — degrade to lexical-only by fusing against an empty semantic
        // list, so the exact-symbol tier still floats. Surface the precise upstream pending reason
        // (overlay warmup, local RAG indexing, or index build) verbatim rather than collapsing
        // them to one generic note.
        CodeHits::Pending(message) => (fuse_smart(&lex_hits, &[], query, limit), Some(message)),
        CodeHits::Unavailable(reason) => {
            (fuse_smart(&lex_hits, &[], query, limit), Some(reason.note()))
        }
    };
    hits.truncate(limit);

    if hits.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text("No results found.")]));
    }

    // Explain the per-hit modality tag once, up front — a leading line does not shift the
    // per-hit `graph_id:` parsing (which is relative to each `#N` line).
    let mut out = String::from(
        "Modality tag per hit: [L] lexical-only · [S] semantic-only · [L+S] found by both (cross-modal agreement).\n",
    );
    out.push_str(&format_code_hits(
        &hits,
        workspace_root.as_deref(),
        graph_root,
        max_output_tokens,
    ));
    if let Some(note) = note {
        // Append AFTER the hit lines — never before — so a client parsing `graph_id:` lines
        // positionally is not shifted.
        let _ = writeln!(out, "-- {note} --");
    }
    Ok(CallToolResult::success(vec![Content::text(out)]))
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

/// Check whether the external baseline can serve a semantic search — without embedding the query.
///
/// Called lock-free, between the two engine-guard acquisitions in `semantic_code_hits`, so a
/// not-ready baseline aborts before the ~1.4s query embed fires.
fn resolve_direct_semantic(
    source: &ExternalBaselineService,
    model_id: Option<&str>,
    dim: Option<usize>,
) -> DirectResolve {
    let snapshot = match source.resolve_snapshot() {
        Ok(Some((_, s))) => s,
        Ok(None) => return DirectResolve::Unavailable,
        Err(e) => {
            if e.is_terminal() {
                warn!("direct semantic: terminal snapshot resolution error: {e}");
                return DirectResolve::Terminal(e);
            }
            warn!("direct semantic: snapshot resolution failed: {e}");
            return DirectResolve::Unavailable;
        }
    };
    let Some(model_id) = model_id else {
        return DirectResolve::Unavailable;
    };
    let Some(dim) = dim else {
        return DirectResolve::Unavailable;
    };
    DirectResolve::Ready { snapshot, model_id: model_id.to_owned(), dim }
}

/// Execute the external baseline semantic search with a precomputed query vector.
///
/// Called under the engine lock after [`resolve_direct_semantic`] confirmed readiness and the
/// embed completed. The snapshot and model identity were resolved in the lock-free phase and are
/// passed in directly; no second `resolve_snapshot` call is made.
fn run_direct_semantic(
    engine: &SearchEngine,
    source: &ExternalBaselineService,
    snapshot: &bsl_search::Snapshot,
    model_id: &str,
    dim: usize,
    query_embedding: &[f32],
    limit: usize,
) -> DirectResult {
    let (overlay_hits, hidden_paths) =
        match engine.workspace_overlay_semantic_hits_with_embedding(query_embedding, limit) {
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
            query_embedding,
            model_id,
            dim,
            Some("code"),
            fetch_limit,
        )
    })
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
    max_output_tokens: usize,
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
                        format_lexical_doc_hits(&hits, max_output_tokens),
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
                        max_output_tokens,
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

    Ok(CallToolResult::success(vec![Content::text(format_doc_hits(&hits, max_output_tokens))]))
}

pub fn search_docs(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
    configured_baseline: Option<&ConfiguredBaselineStatus>,
    external_baseline: Option<Arc<ExternalBaselineService>>,
    query: &str,
    limit: usize,
    max_output_tokens: usize,
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
            "Semantic search not available. Set EMBEDDING_URL and EMBEDDING_MODEL \
             environment variables and restart. Use find_docs for text search instead.",
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
                        format_semantic_doc_hits(&hits, max_output_tokens),
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

    Ok(CallToolResult::success(vec![Content::text(format_doc_hits(&hits, max_output_tokens))]))
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
    use crate::state::{OverlayWarmupState, WorkspaceSearchMode};
    use bsl_search::{
        lexical_hits_for_resolved_view, BaselineRef, CorpusId, Document, EmbedderConfig, FusedHit,
        IndexProgress, IndexedDocument, LexicalHit, Modality, ResolvedView, SearchConfig,
        SearchEngine, SearchError, SemanticHit,
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
    fn format_code_hits_shows_modality_tag() {
        let hits = vec![
            FusedHit {
                hit: code_hit("CommonModules/М/Ext/Module.bsl", "Оба", "procedure"),
                modality: Modality::Both,
            },
            FusedHit {
                hit: code_hit("CommonModules/М/Ext/Module.bsl", "Лекс", "procedure"),
                modality: Modality::Lexical,
            },
            FusedHit {
                hit: code_hit("CommonModules/М/Ext/Module.bsl", "Сем", "procedure"),
                modality: Modality::Semantic,
            },
        ];
        let out = super::format_code_hits(&hits, None, None, usize::MAX);
        assert!(out.contains("#1 [L+S]"), "both-modality hit tagged L+S: {out}");
        assert!(out.contains("#2 [L]"), "lexical-only hit tagged L: {out}");
        assert!(out.contains("#3 [S]"), "semantic-only hit tagged S: {out}");
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
            OverlayWarmupState::Pending,
            Some(ConfiguredBaselineStatus {
                backend: "sqlite",
                selection: "local workspace index".to_owned(),
                issue: None,
                support: None,
            }),
            None,
            false,
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

    fn unreachable_workspace_service() -> Arc<ExternalBaselineService> {
        ExternalBaselineService::for_test(
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
        )
    }

    #[test]
    fn search_status_shows_external_baseline_probe_errors() {
        let source = unreachable_workspace_service();
        // Status itself makes no PG round-trips: it renders the last completed background
        // probe, so the error state is seeded the way a finished probe would leave it.
        source.seed_status_cache_for_test(
            crate::baseline::ExternalBaselineStatus {
                backend: "postgres",
                schema: "erp".to_owned(),
                selection: "branch main".to_owned(),
                resolved: None,
                state: crate::baseline::ExternalBaselineState::Error(
                    "connection refused".to_owned(),
                ),
            },
            std::time::Duration::from_secs(1),
        );

        let result = search_status(
            &Arc::new(Mutex::new(None)),
            &Arc::new(IndexProgress::default()),
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            WorkspaceSearchMode::PostgresRemoteOverlay,
            OverlayWarmupState::Pending,
            Some(ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "branch main".to_owned(),
                issue: None,
                support: None,
            }),
            Some(source),
            false,
        )
        .unwrap();
        let text = result.content[0].raw.as_text().expect("expected text content").text.as_str();

        assert!(text.contains("Configured baseline:"));
        assert!(text.contains("Select:   branch main"));
        assert!(text.contains("External baseline: configured"));
        assert!(text.contains("Backend:  postgres"));
        assert!(text.contains("Status:   error"));
        assert!(text.contains("Error:    connection refused"));
        assert!(
            text.contains("Probed:   1s ago"),
            "cached render must state the probe age: {text}"
        );
    }

    #[test]
    fn search_status_reports_pending_probe_without_blocking() {
        let source = unreachable_workspace_service();

        // No seeded cache: the first status call must answer immediately (the real probe
        // against the unreachable server takes seconds) and say a probe is in flight,
        // without claiming the baseline is established.
        let started = std::time::Instant::now();
        let result = search_status(
            &Arc::new(Mutex::new(None)),
            &Arc::new(IndexProgress::default()),
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Ready)),
            WorkspaceSearchMode::PostgresRemoteOverlay,
            OverlayWarmupState::Pending,
            Some(ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "branch main".to_owned(),
                issue: None,
                support: None,
            }),
            Some(source),
            false,
        )
        .unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "status must not block on the first probe: {:?}",
            started.elapsed()
        );
        let text = result.content[0].raw.as_text().expect("expected text content").text.as_str();

        assert!(
            text.contains("probing the shared baseline in the background — retry shortly"),
            "pending probe must be announced: {text}"
        );
        assert!(
            text.contains("first background status probe still running"),
            "summary must not claim an established baseline before the first probe: {text}"
        );
        assert!(
            !text.contains("(published index)"),
            "summary must not assert baseline availability while the probe is pending: {text}"
        );
    }

    #[test]
    fn search_status_reports_warming_while_baseline_connect_is_pending() {
        let result = search_status(
            &Arc::new(Mutex::new(None)),
            &Arc::new(IndexProgress::default()),
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            WorkspaceSearchMode::PostgresRemoteOverlay,
            OverlayWarmupState::Pending,
            None,
            None,
            true,
        )
        .unwrap();
        let text = result.content[0].raw.as_text().expect("expected text content").text.as_str();

        assert!(text.contains("Configured baseline:"), "{text}");
        assert!(text.contains("Backend:  postgres"), "{text}");
        assert!(text.contains("connecting to the shared baseline"), "{text}");
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
            OverlayWarmupState::Pending,
            Some(ConfiguredBaselineStatus {
                backend: "sqlite",
                selection: "local reference index".to_owned(),
                issue: None,
                support: None,
            }),
            None,
            false,
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

        let error = search_docs(
            &Arc::new(Mutex::new(Some(engine))),
            None,
            Some(source),
            "Массив",
            10,
            usize::MAX,
        )
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
            usize::MAX,
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
            usize::MAX,
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
    fn semantic_core_reports_pending_when_indexing() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("workspace-search.db");
        let engine = Arc::new(Mutex::new(Some(SearchEngine::fts_only(&db_path).unwrap())));
        let outcome = semantic_code_hits(
            &engine,
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Indexing)),
            WorkspaceSearchMode::SqliteLocal,
            None,
            None,
            "обработка проведения документа",
            10,
        )
        .unwrap();

        // While the background embedding pass fills the published-but-empty index, semantic
        // degrades to lexical via Pending instead of searching the empty index.
        assert!(matches!(outcome, CodeHits::Pending(_)));
    }

    #[test]
    fn semantic_core_degrades_to_lexical_when_query_embed_fails() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("workspace-search.db");
        // The engine has semantic configured, but the embedder points at a closed port so the
        // request-time query embed fails fast (connection refused) — standing in for a remote
        // embedder timeout on the hot path of a SqliteLocal search.
        let config = SearchConfig {
            embedder: EmbedderConfig {
                base_url: "http://127.0.0.1:1".to_owned(),
                model: "test-model".to_owned(),
                dim: Some(8),
                api_key: None,
                provider: None,
            },
            ..SearchConfig::default()
        };
        let engine = Arc::new(Mutex::new(Some(SearchEngine::new(&db_path, config).unwrap())));
        let outcome = semantic_code_hits(
            &engine,
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Ready)),
            WorkspaceSearchMode::SqliteLocal,
            None,
            None,
            "обработка проведения документа",
            10,
        )
        .unwrap();

        // A request-time embed failure is a soft shortfall the hybrid path degrades past (lexical
        // hits still serve), not a hard tool error.
        assert!(matches!(
            outcome,
            CodeHits::Unavailable(SemanticUnavailable::EmbedderUnavailable(_))
        ));
    }

    #[test]
    fn hybrid_serves_lexical_while_semantic_indexing() {
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
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Indexing)),
            WorkspaceSearchMode::SqliteLocal,
            None,
            None,
            None,
            &IndexProgress::new(),
            "ПроверитьИНН",
            10,
            usize::MAX,
        )
        .unwrap();
        let text = result.content[0].raw.as_text().expect("text content").text.as_str();

        // The lexical hit comes back immediately, with the precise indexing-specific note.
        assert!(text.contains("ПроверитьИНН"), "{text}");
        assert!(text.contains("-- RAG semantic index is still building"), "{text}");
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
            usize::MAX,
        )
        .unwrap();
        let text = result.content[0].raw.as_text().expect("text content").text.as_str();

        // The lexical hit still comes back...
        assert!(text.contains("ПроверитьИНН"), "{text}");
        // ...with a trailing note explaining semantic was skipped (after the hit lines).
        assert!(text.contains("-- semantic skipped: runtime initialization failed --"), "{text}");
    }

    #[test]
    fn identity_mismatch_note_surfaces_the_carried_actionable_message() {
        // End-to-end mismatch coverage requires a live Postgres baseline with a recorded
        // `_schema_metadata_` identity, which is left to integration; here we pin that the
        // dynamic variant round-trips its carried, caller-facing message verbatim.
        let message = "semantic skipped: this baseline was indexed with model 'a' (dim 768), \
                       but the reader is configured with model 'b' (dim 1024); set \
                       EMBEDDING_MODEL/EMBEDDING_DIM (or [search.baseline.embedding] in \
                       bsl-analyzer.toml) to match and restart";
        let reason = SemanticUnavailable::IdentityMismatch(message.to_owned());
        assert_eq!(reason.note(), message);
    }

    #[test]
    fn hybrid_degrade_note_follows_hit_lines_and_empty_results_suppress_it() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        fs::write(workspace.join("CommonModule.bsl"), "Процедура ПроверитьИНН()\nКонецПроцедуры")
            .unwrap();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);
        let engine = Arc::new(Mutex::new(Some(engine)));
        let failed = Arc::new(Mutex::new(SemanticRuntimeStatus::Failed("boom".to_owned())));

        // A matching query: the degrade note must appear strictly AFTER the hit line so a client
        // parsing `graph_id:` lines positionally is never shifted by it.
        let hit_result = hybrid_code(
            &engine,
            &failed,
            WorkspaceSearchMode::SqliteLocal,
            None,
            None,
            None,
            &IndexProgress::new(),
            "ПроверитьИНН",
            10,
            usize::MAX,
        )
        .unwrap();
        let text = hit_result.content[0].raw.as_text().expect("text").text.as_str();
        let hit_pos = text.find("ПроверитьИНН").expect("hit line present");
        let note_pos = text.find("-- semantic skipped").expect("note present");
        assert!(note_pos > hit_pos, "note must trail the hit lines: {text}");

        // A query that matches nothing degrades to an empty fused list: "No results found" and the
        // semantic-skip note is suppressed (no dangling note without any hits).
        let empty_result = hybrid_code(
            &engine,
            &failed,
            WorkspaceSearchMode::SqliteLocal,
            None,
            None,
            None,
            &IndexProgress::new(),
            "несуществующийидентификатор",
            10,
            usize::MAX,
        )
        .unwrap();
        let empty_text = empty_result.content[0].raw.as_text().expect("text").text.as_str();
        assert_eq!(empty_text, "No results found.");
        assert!(!empty_text.contains("--"), "no trailing note without hits: {empty_text}");
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
            OverlayWarmupState::Pending,
            Some(ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "branch develop".to_owned(),
                issue: None,
                support: None,
            }),
            None,
            false,
        )
        .unwrap();
        let text = result.content[0].raw.as_text().expect("expected text content").text.as_str();

        assert!(text.contains("Search index: ready"));
        // The top line is honest that a concurrent search_code briefly queues behind the sync
        // (it blocks rather than failing), resolving the "status=ready but search says warming"
        // contradiction without promising an error the blocking contract no longer returns.
        assert!(
            text.contains("overlay syncing") && text.contains("queues behind the sync"),
            "status must flag the transient queue window: {text}",
        );
        assert!(text.contains("Semantic: syncing local overlay embeddings against remote baseline"));
        assert!(text.contains("Indexing in progress: 25%"));
    }

    #[test]
    fn search_status_summary_block_is_self_explanatory() {
        // The overlay/semantic summary lines are driven by the runtime and warmup state, not the
        // engine snapshot, so a `None` engine is enough to exercise them. They must let an agent
        // tell "no local diffs" from "warmup failed" and read back the synced file/chunk counts,
        // without parsing the detailed field list below.
        let postgres_baseline = || {
            Some(ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "branch develop".to_owned(),
                issue: None,
                support: None,
            })
        };
        let run = |warmup: OverlayWarmupState| {
            search_status(
                &Arc::new(Mutex::new(None)),
                &Arc::new(IndexProgress::default()),
                &Arc::new(Mutex::new(SemanticRuntimeStatus::Ready)),
                WorkspaceSearchMode::PostgresRemoteOverlay,
                warmup,
                postgres_baseline(),
                None,
                false,
            )
            .unwrap()
            .content[0]
                .raw
                .as_text()
                .expect("text content")
                .text
                .clone()
        };

        let no_diffs = run(OverlayWarmupState::NoLocalDiffs);
        assert!(no_diffs.starts_with("Summary:"), "summary must lead the output: {no_diffs}");
        assert!(
            no_diffs.contains("served from the branch develop baseline (published index)."),
            "ready+postgres must name the baseline: {no_diffs}"
        );
        assert!(
            no_diffs.contains("working tree matches the baseline"),
            "NoLocalDiffs must explain [S] comes from the baseline: {no_diffs}"
        );

        let failed = run(OverlayWarmupState::Failed("embedder timeout: global".to_owned()));
        assert!(
            failed.contains("warmup failed: embedder timeout: global"),
            "Failed must surface the reason: {failed}"
        );

        let synced = run(OverlayWarmupState::Synced { overlay_files: 2, embedded: 5 });
        assert!(
            synced.contains("2 locally-changed file(s) indexed (5 chunks)"),
            "Synced must report file/chunk counts: {synced}"
        );
    }

    #[test]
    fn search_status_emits_progress_signal_while_building() {
        // Engine not built yet (guard is None) and no active re-index: the status must carry
        // an honest progress signal — a "pending" phase line — never a bare "building" line a
        // poller cannot act on. Live numbers are withheld here because an inactive progress
        // object can hold stale totals (it is never reset()), so we must not print them.
        let engine: Arc<Mutex<Option<SearchEngine>>> = Arc::new(Mutex::new(None));
        let progress = Arc::new(IndexProgress::default());

        let result = search_status(
            &engine,
            &progress,
            &Arc::new(Mutex::new(SemanticRuntimeStatus::Ready)),
            WorkspaceSearchMode::SqliteLocal,
            OverlayWarmupState::Pending,
            None,
            None,
            false,
        )
        .unwrap();
        let text = result.content[0].raw.as_text().expect("text").text.as_str();

        assert!(text.contains("building"), "building state must be labelled: {text}");
        assert!(
            text.contains("Indexing pending: initializing"),
            "a pending phase line must appear while building, not a bare line: {text}"
        );
        // No live counter lines when not actively indexing (would be stale/misleading).
        assert!(
            !text.contains("Indexing in progress"),
            "must not claim active indexing while merely pending: {text}"
        );
    }

    #[test]
    fn hybrid_code_not_ready_returns_structured_envelope() {
        // No engine and no baseline: lexical cannot serve, so the whole search is not ready.
        // It must come back as a structured envelope (machine status + retry hint + live
        // counters) mirroring the graph tool, not a bare sentence a poller has to parse.
        let engine: Arc<Mutex<Option<SearchEngine>>> = Arc::new(Mutex::new(None));
        let runtime = Arc::new(Mutex::new(SemanticRuntimeStatus::Ready));
        let progress = Arc::new(IndexProgress::default());
        progress.active.store(true, Ordering::Relaxed);
        progress.total_chunks.store(100, Ordering::Relaxed);
        progress.done_chunks.store(40, Ordering::Relaxed);
        progress.total_batches.store(10, Ordering::Relaxed);
        progress.done_batches.store(4, Ordering::Relaxed);

        let result = hybrid_code(
            &engine,
            &runtime,
            WorkspaceSearchMode::SqliteLocal,
            None,
            None,
            None,
            &progress,
            "ПроверитьИНН",
            10,
            usize::MAX,
        )
        .unwrap();

        let body = result.structured_content.as_ref().expect("structured not-ready envelope");
        assert_eq!(body["status"], "not_ready");
        assert_eq!(body["retry_after_ms"], 1500);
        assert_eq!(body["progress"]["active"], true);
        assert_eq!(body["progress"]["pct"], 40);
        assert_eq!(body["progress"]["chunks"]["done"], 40);
        assert_eq!(body["progress"]["batches"]["total"], 10);

        // The text block is the JSON mirror (machine-parseable), not prose.
        let text = result.content[0].raw.as_text().expect("text mirror").text.as_str();
        let mirror: serde_json::Value =
            serde_json::from_str(text).expect("text mirror must be valid JSON");
        assert_eq!(&mirror, body, "text mirror must match structuredContent");
    }

    #[test]
    fn hybrid_code_not_ready_omits_counters_when_inactive() {
        // An inactive progress object may hold stale totals (it is never reset()), so a
        // not-ready envelope must carry the `active=false` flag but NO numeric counters —
        // never report a finished/failed attempt's leftover numbers as current progress.
        let engine: Arc<Mutex<Option<SearchEngine>>> = Arc::new(Mutex::new(None));
        let runtime = Arc::new(Mutex::new(SemanticRuntimeStatus::Ready));
        let progress = Arc::new(IndexProgress::default());
        // Simulate stale leftovers from a prior attempt: nonzero totals, but not active.
        progress.total_chunks.store(100, Ordering::Relaxed);
        progress.done_chunks.store(100, Ordering::Relaxed);

        let result = hybrid_code(
            &engine,
            &runtime,
            WorkspaceSearchMode::SqliteLocal,
            None,
            None,
            None,
            &progress,
            "ПроверитьИНН",
            10,
            usize::MAX,
        )
        .unwrap();

        let body = result.structured_content.as_ref().expect("structured not-ready envelope");
        assert_eq!(body["status"], "not_ready");
        assert_eq!(body["progress"]["active"], false);
        assert!(body["progress"]["pct"].is_null(), "no stale pct when inactive: {body}");
        assert!(body["progress"]["chunks"].is_null(), "no stale counters when inactive: {body}");
    }

    #[test]
    fn try_acquire_engine_queues_until_the_lock_frees() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Barrier;
        use std::time::{Duration, Instant};

        let engine: Arc<Mutex<Option<SearchEngine>>> = Arc::new(Mutex::new(None));
        // Uncontended: the guard is acquired immediately.
        assert!(super::try_acquire_engine(&engine).is_ok());

        // A peer holds the lock for a known interval, as an overlay prime or a slow embedding
        // round-trip would. The acquire must QUEUE — block rather than bail with a misleading
        // "warming up" — and then succeed once the holder releases, so a concurrent batch all
        // get real results instead of half of them failing.
        const HOLD: Duration = Duration::from_millis(300);
        let held = engine.lock().unwrap();
        // The barrier guarantees the probe has reached the acquire call while the lock is still
        // held, so a passing test can't be an artifact of the probe starting after the release.
        let gate = Arc::new(Barrier::new(2));
        let entered = Arc::new(AtomicBool::new(false));
        let probe = {
            let engine = Arc::clone(&engine);
            let gate = Arc::clone(&gate);
            let entered = Arc::clone(&entered);
            std::thread::spawn(move || {
                gate.wait();
                entered.store(true, Ordering::SeqCst);
                let started = Instant::now();
                let acquired = super::try_acquire_engine(&engine).is_ok();
                (acquired, started.elapsed())
            })
        };
        gate.wait();
        // Keep holding well past the point the probe is blocking on the lock.
        std::thread::sleep(HOLD);
        assert!(entered.load(Ordering::SeqCst), "probe must reach the acquire under contention");
        drop(held);
        let (acquired, waited) = probe.join().unwrap();
        assert!(acquired, "acquire must succeed once the lock frees");
        // It blocked for ~the hold, not bailed immediately — i.e. it really queued.
        assert!(waited >= HOLD / 2, "acquire returned too fast to have queued: {waited:?}");
    }

    #[test]
    fn acquire_engine_times_out_when_the_lock_stays_held() {
        use std::time::{Duration, Instant};

        let engine: Arc<Mutex<Option<SearchEngine>>> = Arc::new(Mutex::new(None));
        // Held for the whole call with a tiny cap: the acquire must give up as `TimedOut` (the
        // caller degrades to baseline / "busy, retry"), and only after roughly the cap elapsed.
        let held = engine.lock().unwrap();
        let cap = Duration::from_millis(150);
        let started = Instant::now();
        let outcome = super::acquire_engine_within(&engine, cap, Duration::from_millis(10));
        let waited = started.elapsed();
        assert!(matches!(outcome, Err(super::AcquireFailure::TimedOut)));
        assert!(waited >= cap, "must wait out the cap before giving up: {waited:?}");
        drop(held);
    }

    #[test]
    fn acquire_engine_reports_poison_immediately() {
        use std::time::{Duration, Instant};

        let engine: Arc<Mutex<Option<SearchEngine>>> = Arc::new(Mutex::new(None));
        // A holder panics, poisoning the lock. Waiting cannot recover it, so the acquire must
        // report `Poisoned` at once — not spin out the cap — so the caller can surface a hard
        // error instead of "warming up". A large cap proves it returns without waiting it out.
        let poisoner = {
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || {
                let _held = engine.lock().unwrap();
                panic!("poison the engine lock");
            })
        };
        assert!(poisoner.join().is_err());
        let started = Instant::now();
        let outcome = super::acquire_engine_within(
            &engine,
            Duration::from_secs(30),
            Duration::from_millis(10),
        );
        assert!(matches!(outcome, Err(super::AcquireFailure::Poisoned)));
        assert!(started.elapsed() < Duration::from_secs(1), "poison must not block on the cap");
    }

    #[test]
    fn search_status_returns_promptly_with_busy_note_when_engine_lock_is_held() {
        use std::sync::Barrier;
        use std::time::{Duration, Instant};

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("bsl-search.db");
        let engine = Arc::new(Mutex::new(Some(SearchEngine::fts_only(&db_path).unwrap())));

        // A peer thread holds the engine lock for the whole call, as the overlay warmup's publish
        // (or a slow embed) would. `search_status` must not block to the MCP client timeout: with
        // a short acquire cap it gives up on the local snapshot, emits the lock-free baseline
        // section plus a busy note, and returns well within the cap-plus-margin window.
        let gate = Arc::new(Barrier::new(2));
        let holder = {
            let engine = Arc::clone(&engine);
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || {
                let held = engine.lock().unwrap();
                gate.wait();
                std::thread::sleep(Duration::from_millis(300));
                drop(held);
            })
        };
        gate.wait();

        let started = Instant::now();
        let status = super::search_status_with_cap(
            &engine,
            &Arc::new(IndexProgress::default()),
            &Arc::new(Mutex::new(SemanticRuntimeStatus::OverlaySyncing)),
            WorkspaceSearchMode::PostgresRemoteOverlay,
            OverlayWarmupState::Pending,
            Some(ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "branch main".to_owned(),
                issue: None,
                support: None,
            }),
            None,
            false,
            Duration::from_millis(40),
        )
        .unwrap();
        let elapsed = started.elapsed();
        holder.join().unwrap();

        assert!(elapsed < Duration::from_secs(2), "status must not hang on the lock: {elapsed:?}");
        let text = status.content[0].raw.as_text().expect("text content").text.as_str();
        assert!(text.contains("Configured baseline:"), "baseline section present: {text}");
        assert!(text.contains("Local index: busy (overlay syncing)"), "busy note present: {text}");
        // The summary must not claim the index is live while the lock is held: with the engine
        // busy, the lexical line degrades rather than asserting "reflects the current working tree".
        assert!(
            text.contains("Lexical search: temporarily unavailable"),
            "summary lexical line reflects busy engine: {text}"
        );
        assert!(
            !text.contains("Lexical search: reflects the current working tree"),
            "summary must not claim the working tree is live while busy: {text}"
        );
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
            usize::MAX,
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
            usize::MAX,
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
            usize::MAX,
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
            usize::MAX,
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

        let result = search_docs(
            &Arc::new(Mutex::new(Some(engine))),
            None,
            Some(source),
            "Массив",
            10,
            usize::MAX,
        )
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
