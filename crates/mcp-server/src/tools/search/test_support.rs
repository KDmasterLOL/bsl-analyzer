use crate::baseline::{ExternalBaselineService, RefreshableExternalBaselineSource};
use bsl_search::{BaselineRef, CorpusId, LexicalHit, SearchHit, SemanticHit};
use project_model::{SearchPostgresConfig, SearchPostgresCredentialHelperConfig};
use std::sync::Arc;

pub(super) fn retryable_postgres_source() -> Arc<ExternalBaselineService> {
    let postgres = SearchPostgresConfig {
        host: Some("127.0.0.1".to_owned()),
        port: Some(1),
        dbname: Some("bsl_search".to_owned()),
        schema: Some("bsl_search".to_owned()),
        vault_role_base: Some("prod/search/bsl-analyzer".to_owned()),
        credential_helper: SearchPostgresCredentialHelperConfig {
            program: Some("echo".to_owned()),
            args: vec![
                r#"{"protocol":"bsl-analyzer.postgres-helper.v1","ok":true,"url":"postgres://127.0.0.1:1/bsl_search"}"#.to_owned(),
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

pub(super) fn unreachable_workspace_service() -> Arc<ExternalBaselineService> {
    ExternalBaselineService::for_test(
        RefreshableExternalBaselineSource::for_test(
            bsl_search::ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
            BaselineRef {
                corpus: CorpusId::WorkspaceCode,
                snapshot_id: None,
                branch: Some("main".to_owned()),
                commit: None,
            },
        )
        .unwrap(),
    )
}

pub(super) fn lexical_hit(path: &str, symbol_name: &str, rank: f32) -> LexicalHit {
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

pub(super) fn semantic_hit(path: &str, symbol_name: &str, score: f32) -> SemanticHit {
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

pub(super) fn code_hit(file_path: &str, symbol: &str, kind: &str) -> SearchHit {
    SearchHit {
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

#[cfg(test)]
mod tests {
    use super::{
        code_hit, lexical_hit, retryable_postgres_source, semantic_hit,
        unreachable_workspace_service,
    };

    #[test]
    fn fixtures_construct_synthetic_hits_and_baseline_services() {
        assert_eq!(lexical_hit("a.bsl", "A", 1.0).collection, "code");
        assert_eq!(semantic_hit("a.bsl", "A", 1.0).collection, "code");
        assert_eq!(code_hit("a.bsl", "A", "procedure").collection, "code");
        let _ = retryable_postgres_source();
        let _ = unreachable_workspace_service();
    }
}
