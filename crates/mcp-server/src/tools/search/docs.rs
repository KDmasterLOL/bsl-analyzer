use super::gating::{
    ensure_reference_baseline_runtime_ready, external_baseline_mcp_error,
    map_reference_baseline_resolution,
};
use super::render::{format_doc_hits, format_lexical_doc_hits, format_semantic_doc_hits};
use crate::baseline::{ConfiguredBaselineStatus, ExternalBaselineService};
use bsl_search::{lexical_hits_for_resolved_view, SearchEngine};
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;
use std::sync::{Arc, Mutex};
use tracing::warn;

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
    use super::{find_docs, search_docs};
    use crate::baseline::{
        ConfiguredBaselineStatus, ExternalBaselineService, RefreshableExternalBaselineSource,
    };
    use bsl_search::{BaselineRef, CorpusId, SearchEngine};
    use rmcp::model::ErrorCode;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    #[test]
    fn search_docs_with_external_reference_baseline_uses_standard_semantic_validation() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("reference-search.db");
        let engine = SearchEngine::fts_only(&db_path).unwrap();
        let source = ExternalBaselineService::for_test(
            RefreshableExternalBaselineSource::for_test(
                bsl_search::ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
                BaselineRef {
                    corpus: CorpusId::Reference,
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
        let engine = SearchEngine::fts_only(&db_path).unwrap();

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
}
