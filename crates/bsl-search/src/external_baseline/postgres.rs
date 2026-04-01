use crate::domain::{BaselineRef, CorpusId, ExternalBaselineConfig, IndexedDocument, Snapshot};
use crate::error::SearchError;
use crate::ports::{SnapshotCatalog, SnapshotContentStore, SnapshotPublisher};
use postgres::{Client, NoTls, Row};

/// PostgreSQL adapter for centralized baseline storage.
///
/// Expected schema:
/// - `<schema>.snapshots`
///   `id TEXT PRIMARY KEY`
///   `corpus TEXT NOT NULL`
///   `branch TEXT NULL`
///   `commit_sha TEXT NULL`
///   `created_at TIMESTAMPTZ NOT NULL`
/// - `<schema>.snapshot_items`
///   `snapshot_id TEXT NOT NULL`
///   `collection TEXT NOT NULL`
///   `path TEXT NOT NULL`
///   `symbol_name TEXT NOT NULL`
///   `kind TEXT NOT NULL`
///   `line_start INTEGER NOT NULL`
///   `line_end INTEGER NOT NULL`
///   `content_hash TEXT NOT NULL`
/// - `<schema>.content_objects`
///   `content_hash TEXT PRIMARY KEY`
///   `text TEXT NOT NULL`
#[derive(Debug, Clone)]
pub struct PostgresBaselineAdapter {
    config: ExternalBaselineConfig,
    schema: String,
}

impl PostgresBaselineAdapter {
    pub fn new(config: ExternalBaselineConfig) -> Result<Self, SearchError> {
        let schema = config.schema.clone().unwrap_or_else(|| "bsl_search".to_owned());
        validate_identifier(&schema)?;
        Ok(Self { config, schema })
    }

    pub fn config(&self) -> &ExternalBaselineConfig {
        &self.config
    }

    fn connect(&self) -> Result<Client, SearchError> {
        Client::connect(&self.config.connection, NoTls).map_err(SearchError::from)
    }

    fn table(&self, table: &str) -> String {
        format!("{}.{}", self.schema, table)
    }

    fn snapshot_row_to_model(row: Row) -> Snapshot {
        let corpus = CorpusId::from_storage(row.get::<_, String>("corpus"));
        Snapshot::new(row.get::<_, String>("id"), corpus)
    }

    fn latest_snapshot_query(&self, with_filter: &str) -> String {
        format!(
            "SELECT id, corpus
             FROM {}
             WHERE {}
             ORDER BY created_at DESC
             LIMIT 1",
            self.table("snapshots"),
            with_filter
        )
    }

    fn ensure_schema_statements(&self) -> Vec<String> {
        vec![
            format!("CREATE SCHEMA IF NOT EXISTS {}", self.schema),
            format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    id TEXT PRIMARY KEY,
                    corpus TEXT NOT NULL,
                    branch TEXT NULL,
                    commit_sha TEXT NULL,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
                )",
                self.table("snapshots")
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    content_hash TEXT PRIMARY KEY,
                    text TEXT NOT NULL
                )",
                self.table("content_objects")
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    snapshot_id TEXT NOT NULL REFERENCES {}(id) ON DELETE CASCADE,
                    collection TEXT NOT NULL,
                    path TEXT NOT NULL,
                    symbol_name TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL,
                    content_hash TEXT NOT NULL REFERENCES {}(content_hash) ON DELETE RESTRICT
                )",
                self.table("snapshot_items"),
                self.table("snapshots"),
                self.table("content_objects")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_snapshots_corpus_created_at
                 ON {} (corpus, created_at DESC)",
                self.schema,
                self.table("snapshots")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_snapshots_branch_commit
                 ON {} (corpus, branch, commit_sha, created_at DESC)",
                self.schema,
                self.table("snapshots")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_snapshot_items_snapshot_path
                 ON {} (snapshot_id, collection, path)",
                self.schema,
                self.table("snapshot_items")
            ),
        ]
    }
}

impl SnapshotCatalog for PostgresBaselineAdapter {
    fn resolve_baseline(&self, baseline: &BaselineRef) -> Result<Option<Snapshot>, SearchError> {
        let mut client = self.connect()?;
        let corpus = baseline.corpus.as_str();

        let row = if let Some(snapshot_id) = &baseline.snapshot_id {
            let query = format!(
                "SELECT id, corpus
                 FROM {}
                 WHERE id = $1 AND corpus = $2
                 LIMIT 1",
                self.table("snapshots")
            );
            client.query_opt(&query, &[&snapshot_id.0, &corpus])?
        } else if let (Some(branch), Some(commit)) = (&baseline.branch, &baseline.commit) {
            let query =
                self.latest_snapshot_query("corpus = $1 AND branch = $2 AND commit_sha = $3");
            client.query_opt(&query, &[&corpus, branch, commit])?
        } else if let Some(branch) = &baseline.branch {
            let query = self.latest_snapshot_query("corpus = $1 AND branch = $2");
            client.query_opt(&query, &[&corpus, branch])?
        } else if let Some(commit) = &baseline.commit {
            let query = self.latest_snapshot_query("corpus = $1 AND commit_sha = $2");
            client.query_opt(&query, &[&corpus, commit])?
        } else {
            let query = self.latest_snapshot_query("corpus = $1");
            client.query_opt(&query, &[&corpus])?
        };

        Ok(row.map(Self::snapshot_row_to_model))
    }
}

impl SnapshotContentStore for PostgresBaselineAdapter {
    fn load_snapshot_documents(
        &self,
        snapshot: &Snapshot,
    ) -> Result<Vec<IndexedDocument>, SearchError> {
        let mut client = self.connect()?;
        let query = format!(
            "SELECT si.collection,
                    si.path,
                    si.symbol_name,
                    si.kind,
                    si.line_start,
                    si.line_end,
                    co.text,
                    si.content_hash
             FROM {} si
             JOIN {} co ON co.content_hash = si.content_hash
             WHERE si.snapshot_id = $1
             ORDER BY si.collection, si.path, si.line_start, si.line_end, si.symbol_name",
            self.table("snapshot_items"),
            self.table("content_objects")
        );

        let rows = client.query(&query, &[&snapshot.id.0])?;
        Ok(rows
            .into_iter()
            .map(|row| IndexedDocument {
                collection: row.get("collection"),
                path: row.get("path"),
                symbol_name: row.get("symbol_name"),
                kind: row.get("kind"),
                line_start: row.get::<_, i32>("line_start") as u32,
                line_end: row.get::<_, i32>("line_end") as u32,
                text: row.get("text"),
                content_hash: row.get("content_hash"),
            })
            .collect())
    }
}

impl SnapshotPublisher for PostgresBaselineAdapter {
    fn ensure_storage(&self) -> Result<(), SearchError> {
        let mut client = self.connect()?;
        for statement in self.ensure_schema_statements() {
            client.batch_execute(&statement)?;
        }
        Ok(())
    }

    fn publish_snapshot(
        &self,
        snapshot: &Snapshot,
        branch: Option<&str>,
        commit: Option<&str>,
        documents: &[IndexedDocument],
    ) -> Result<(), SearchError> {
        self.ensure_storage()?;

        let mut client = self.connect()?;
        let mut tx = client.transaction()?;

        let upsert_snapshot = format!(
            "INSERT INTO {} (id, corpus, branch, commit_sha)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (id) DO UPDATE SET
                corpus = EXCLUDED.corpus,
                branch = EXCLUDED.branch,
                commit_sha = EXCLUDED.commit_sha",
            self.table("snapshots")
        );
        tx.execute(
            &upsert_snapshot,
            &[&snapshot.id.0, &snapshot.corpus.as_str(), &branch, &commit],
        )?;

        let delete_items =
            format!("DELETE FROM {} WHERE snapshot_id = $1", self.table("snapshot_items"));
        tx.execute(&delete_items, &[&snapshot.id.0])?;

        let upsert_content = format!(
            "INSERT INTO {} (content_hash, text)
             VALUES ($1, $2)
             ON CONFLICT (content_hash) DO UPDATE SET text = EXCLUDED.text",
            self.table("content_objects")
        );
        let insert_item = format!(
            "INSERT INTO {} (
                snapshot_id, collection, path, symbol_name, kind, line_start, line_end, content_hash
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            self.table("snapshot_items")
        );

        for document in documents {
            tx.execute(&upsert_content, &[&document.content_hash, &document.text])?;
            tx.execute(
                &insert_item,
                &[
                    &snapshot.id.0,
                    &document.collection,
                    &document.path,
                    &document.symbol_name,
                    &document.kind,
                    &(document.line_start as i32),
                    &(document.line_end as i32),
                    &document.content_hash,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }
}

fn validate_identifier(identifier: &str) -> Result<(), SearchError> {
    if identifier.is_empty()
        || !identifier.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(SearchError::ExternalBaseline(format!(
            "invalid postgres schema identifier: {identifier}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::PostgresBaselineAdapter;
    use crate::domain::{CorpusId, ExternalBaselineConfig};
    use crate::ports::{SnapshotCatalog, SnapshotPublisher};
    use crate::BaselineRef;

    #[test]
    fn defaults_to_bsl_search_schema() {
        let adapter =
            PostgresBaselineAdapter::new(ExternalBaselineConfig::postgres("postgres://example"))
                .unwrap();

        assert_eq!(adapter.config().schema, None);
        assert_eq!(adapter.table("snapshots"), "bsl_search.snapshots");
    }

    #[test]
    fn schema_validation_rejects_invalid_identifier() {
        let error = PostgresBaselineAdapter::new(
            ExternalBaselineConfig::postgres("postgres://example").with_schema("bad-schema"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("invalid postgres schema"));
    }

    #[test]
    fn connection_errors_surface_from_runtime_queries() {
        let adapter = PostgresBaselineAdapter::new(ExternalBaselineConfig::postgres(
            "postgres://127.0.0.1:1",
        ))
        .unwrap();
        let baseline = BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "snapshot-1");

        let error = adapter.resolve_baseline(&baseline).unwrap_err();
        assert!(error.to_string().contains("postgres"));
    }

    #[test]
    fn connection_errors_surface_from_storage_initialization() {
        let adapter = PostgresBaselineAdapter::new(ExternalBaselineConfig::postgres(
            "postgres://127.0.0.1:1",
        ))
        .unwrap();

        let error = adapter.ensure_storage().unwrap_err();
        assert!(error.to_string().contains("postgres"));
    }
}
