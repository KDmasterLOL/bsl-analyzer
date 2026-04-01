use crate::domain::{
    BaselineRef, ExternalBaselineBackend, ExternalBaselineConfig, IndexedDocument, Snapshot,
};
use crate::error::SearchError;
use crate::ports::{SnapshotCatalog, SnapshotContentStore};

mod postgres;

/// Infrastructure adapter for centralized baseline storage.
///
/// This adapter is a backend selector. The first supported production backend is
/// PostgreSQL. Runtime code should depend on this selector or directly on the
/// storage ports, not on a concrete PostgreSQL client.
#[derive(Debug, Clone)]
pub enum ExternalBaselineAdapter {
    Postgres(postgres::PostgresBaselineAdapter),
}

impl ExternalBaselineAdapter {
    pub fn new(config: ExternalBaselineConfig) -> Result<Self, SearchError> {
        match config.backend {
            ExternalBaselineBackend::Postgres => {
                Ok(Self::Postgres(postgres::PostgresBaselineAdapter::new(config)?))
            }
        }
    }

    pub fn config(&self) -> &ExternalBaselineConfig {
        match self {
            Self::Postgres(adapter) => adapter.config(),
        }
    }
}

impl SnapshotCatalog for ExternalBaselineAdapter {
    fn resolve_baseline(&self, baseline: &BaselineRef) -> Result<Option<Snapshot>, SearchError> {
        match self {
            Self::Postgres(adapter) => adapter.resolve_baseline(baseline),
        }
    }
}

impl SnapshotContentStore for ExternalBaselineAdapter {
    fn load_snapshot_documents(
        &self,
        snapshot: &Snapshot,
    ) -> Result<Vec<IndexedDocument>, SearchError> {
        match self {
            Self::Postgres(adapter) => adapter.load_snapshot_documents(snapshot),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ExternalBaselineAdapter;
    use crate::domain::{CorpusId, ExternalBaselineConfig};

    #[test]
    fn external_adapter_constructs_postgres_backend() {
        let adapter = ExternalBaselineAdapter::new(
            ExternalBaselineConfig::postgres("postgres://example").with_schema("bsl_search"),
        )
        .unwrap();

        assert_eq!(adapter.config().schema.as_deref(), Some("bsl_search"));
        assert_eq!(adapter.config().connection, "postgres://example");
        assert!(matches!(adapter, ExternalBaselineAdapter::Postgres(_)));
    }

    #[test]
    fn invalid_schema_is_rejected_before_runtime_use() {
        let error = ExternalBaselineAdapter::new(
            ExternalBaselineConfig::postgres("postgres://example").with_schema("bad-schema"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("invalid postgres schema"));
        let _ = CorpusId::WorkspaceCode;
    }
}
