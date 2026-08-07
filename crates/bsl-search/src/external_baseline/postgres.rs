use crate::domain::{
    BaselineRef, CorpusId, ExternalBaselineConfig, IndexedDocument, LexicalHit, SemanticHit,
    Snapshot, SnapshotPublishMetadata, SnapshotPublishStats,
};
use crate::error::SearchError;
use crate::external_baseline::{
    BaselineCollectionRecord, BaselineEmbeddingCoverageRecord, BaselineEmbeddingModelRecord,
    BaselineEmbeddingStats, BaselineFileObjectDetails, BaselineFileObjectRecord,
    BaselineFileObjectReference, BaselineGcReport, BaselineSnapshotDetails, BaselineSnapshotRecord,
    SemanticPublishPhase, SemanticPublishProgress,
};
use crate::ports::{
    BaselineLexicalSearch, BaselineSemanticSearch, SnapshotCatalog, SnapshotContentStore,
    SnapshotPublisher, WorkspaceBaselineManifestStore,
};
use crate::workspace_roots::CONFIGURATION_ROOT_ID;
use postgres::{GenericClient, NoTls, Row, Transaction};
use r2d2_postgres::{r2d2::Pool, PostgresConnectionManager};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SCHEMA_METADATA_TABLE: &str = "_schema_metadata_";
const EMBEDDING_MODEL_SETTING: &str = "embedding_model";
const EMBEDDING_DIMENSION_SETTING: &str = "embedding_dimension";
const REQUIRED_STORAGE_TABLES: &[&str] = &[
    SCHEMA_METADATA_TABLE,
    "snapshots",
    "snapshot_files",
    "snapshot_deletions",
    "snapshot_heads",
    "content_objects",
    "semantic_embeddings",
    "file_objects",
    "file_object_items",
    "serving_lexical",
];

type PgPooledConnection = r2d2_postgres::r2d2::PooledConnection<PostgresConnectionManager<NoTls>>;

const CONTENT_OBJECT_BATCH_SIZE: usize = 5_000;
const FILE_OBJECT_ITEM_BATCH_SIZE: usize = 2_000;
const SNAPSHOT_FILE_BATCH_SIZE: usize = 2_000;
const SNAPSHOT_DELETION_BATCH_SIZE: usize = 5_000;
const SERVING_LEXICAL_BATCH_SIZE: usize = 2_000;
const SERVING_SEMANTIC_BATCH_SIZE: usize = 256;
const SEMANTIC_PUBLICATION_COMPLETE_PREFIX: &str = "semantic_publication_complete:";

/// Readiness re-verification period. A successful check is trusted for this long, then
/// re-run, so the typed `StorageNotInitialized` / `SchemaVersionMismatch` errors still
/// surface (with bounded delay) if the schema is dropped or migrated mid-process — callers
/// branch on those exact error kinds and never treat them as retryable.
const STORAGE_READINESS_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct PostgresBaselineAdapter {
    config: ExternalBaselineConfig,
    schema: String,
    pool: Pool<PostgresConnectionManager<NoTls>>,
    storage_verified_at: Arc<Mutex<Option<Instant>>>,
}

impl PostgresBaselineAdapter {
    pub fn new(config: ExternalBaselineConfig) -> Result<Self, SearchError> {
        let schema = config.schema.clone().unwrap_or_else(|| "bsl_search".to_owned());
        validate_identifier(&schema)?;
        let pg_config = config.connection.parse().map_err(|err: postgres::Error| {
            SearchError::ExternalBaseline(format!("invalid connection string: {err}"))
        })?;
        let manager = PostgresConnectionManager::new(pg_config, NoTls);
        let pool = Pool::builder()
            .max_size(4)
            .connection_timeout(std::time::Duration::from_secs(5))
            .build_unchecked(manager);
        Ok(Self { config, schema, pool, storage_verified_at: Arc::new(Mutex::new(None)) })
    }

    pub fn config(&self) -> &ExternalBaselineConfig {
        &self.config
    }

    fn connect(&self) -> Result<PgPooledConnection, SearchError> {
        self.pool.get().map_err(|err| {
            let reason = pool_connection_reason_code(&err.to_string());
            SearchError::ExternalBaseline(format!(
                "{reason}: failed to get pooled connection: {err}"
            ))
        })
    }

    fn table(&self, table: &str) -> String {
        format!("{}.{}", self.schema, table)
    }

    fn storage_table_exists(
        &self,
        client: &mut impl GenericClient,
        table_name: &str,
    ) -> Result<bool, SearchError> {
        Ok(client
            .query_opt(
                "SELECT 1 FROM information_schema.tables
                 WHERE table_schema = $1 AND table_name = $2
                 LIMIT 1",
                &[&self.schema, &table_name],
            )?
            .is_some())
    }

    fn lexical_serving_rows_exist(
        &self,
        client: &mut impl GenericClient,
        snapshot_id: &str,
        collection: Option<&str>,
    ) -> Result<bool, SearchError> {
        let mut sql =
            format!("SELECT 1 FROM {} WHERE snapshot_id = $1", self.table("serving_lexical"));
        let snapshot_id = snapshot_id.to_owned();
        let collection = collection.map(ToOwned::to_owned);
        let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> = vec![&snapshot_id];
        if let Some(collection) = collection.as_ref() {
            sql.push_str(&format!(" AND collection = ${}", params.len() + 1));
            params.push(collection);
        }
        sql.push_str(" LIMIT 1");
        Ok(client.query_opt(&sql, &params)?.is_some())
    }

    fn semantic_serving_rows_exist(
        &self,
        client: &mut impl GenericClient,
        snapshot_id: &str,
        model_id: &str,
        dimension: i32,
        collection: Option<&str>,
    ) -> Result<bool, SearchError> {
        let mut sql = format!(
            "SELECT 1 FROM {} WHERE snapshot_id = $1 AND model_id = $2 AND dimension = $3",
            self.table("serving_semantic")
        );
        let snapshot_id = snapshot_id.to_owned();
        let model_id = model_id.to_owned();
        let collection = collection.map(ToOwned::to_owned);
        let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> =
            vec![&snapshot_id, &model_id, &dimension];
        if let Some(collection) = collection.as_ref() {
            sql.push_str(&format!(" AND collection = ${}", params.len() + 1));
            params.push(collection);
        }
        sql.push_str(" LIMIT 1");
        Ok(client.query_opt(&sql, &params)?.is_some())
    }

    pub fn check_storage_readiness(&self) -> Result<(), SearchError> {
        // Nearly every public method starts with this check; without the TTL cache each
        // call pays information_schema roundtrips on the shared 4-connection pool.
        // Only success is cached — failures re-check on every call so `migrate_storage`
        // heals a not-ready schema without waiting out the TTL.
        if let Ok(verified_at) = self.storage_verified_at.lock() {
            if verified_at.is_some_and(|at| at.elapsed() < STORAGE_READINESS_TTL) {
                return Ok(());
            }
        }

        let mut client = self.connect()?;
        let present_tables: i64 = client
            .query_one(
                "SELECT COUNT(DISTINCT table_name) FROM information_schema.tables
                 WHERE table_schema = $1 AND table_name = ANY($2)",
                &[&self.schema, &REQUIRED_STORAGE_TABLES],
            )?
            .get(0);
        if present_tables as usize != REQUIRED_STORAGE_TABLES.len() {
            return Err(SearchError::StorageNotInitialized { schema: self.schema.clone() });
        }

        let version = {
            let row = client.query_opt(
                &format!(
                    "SELECT value::INTEGER FROM {} WHERE setting = 'schema_version' LIMIT 1",
                    self.table(SCHEMA_METADATA_TABLE)
                ),
                &[],
            )?;
            row.map(|r| r.get::<_, i32>(0))
        };
        if let Some(version) = version {
            if version != crate::error::SCHEMA_VERSION_CURRENT {
                return Err(SearchError::SchemaVersionMismatch {
                    expected: crate::error::SCHEMA_VERSION_CURRENT,
                    actual: Some(version),
                });
            }
        } else {
            return Err(SearchError::StorageNotInitialized { schema: self.schema.clone() });
        }

        if let Ok(mut verified_at) = self.storage_verified_at.lock() {
            *verified_at = Some(Instant::now());
        }
        Ok(())
    }

    pub fn get_schema_version(&self) -> Result<Option<i32>, SearchError> {
        let mut client = self.connect()?;
        let row = client.query_opt(
            &format!(
                "SELECT value::INTEGER FROM {} WHERE setting = 'schema_version' LIMIT 1",
                self.table(SCHEMA_METADATA_TABLE)
            ),
            &[],
        )?;
        Ok(row.map(|r| r.get::<_, i32>(0)))
    }

    fn write_schema_version(&self, tx: &mut Transaction<'_>) -> Result<(), SearchError> {
        let schema_version = crate::error::SCHEMA_VERSION_CURRENT.to_string();
        tx.execute(
            &format!(
                "INSERT INTO {} (setting, value)
                 VALUES ('schema_version', $1::TEXT)
                 ON CONFLICT (setting) DO UPDATE SET value = EXCLUDED.value",
                self.table(SCHEMA_METADATA_TABLE),
            ),
            &[&schema_version],
        )?;
        Ok(())
    }

    pub fn read_embedding_identity(&self) -> Result<Option<(String, usize)>, SearchError> {
        let mut client = self.connect()?;
        let model = client
            .query_opt(
                &format!(
                    "SELECT value FROM {} WHERE setting = $1 LIMIT 1",
                    self.table(SCHEMA_METADATA_TABLE)
                ),
                &[&EMBEDDING_MODEL_SETTING],
            )?
            .map(|row| row.get::<_, String>(0));
        let dimension = client
            .query_opt(
                &format!(
                    "SELECT value::INTEGER FROM {} WHERE setting = $1 LIMIT 1",
                    self.table(SCHEMA_METADATA_TABLE)
                ),
                &[&EMBEDDING_DIMENSION_SETTING],
            )?
            .map(|row| row.get::<_, i32>(0));
        match (model, dimension) {
            (Some(model), Some(dimension)) => Ok(Some((model, dimension as usize))),
            _ => Ok(None),
        }
    }

    pub fn ensure_embedding_identity(
        &self,
        model_id: &str,
        dimension: usize,
    ) -> Result<(), SearchError> {
        let mut client = self.connect()?;
        let mut tx = client.transaction()?;
        // Claim the identity if unset, atomically. `DO NOTHING` (not `DO UPDATE`) means
        // the first writer wins; a concurrent first publish blocks on the row lock and,
        // once the winner commits, its own insert becomes a no-op. The read-back below
        // therefore reflects the single committed identity for every writer, so two racing
        // first publishes with different models can't both succeed.
        let claim = format!(
            "INSERT INTO {} (setting, value)
             VALUES ($1, $2)
             ON CONFLICT (setting) DO NOTHING",
            self.table(SCHEMA_METADATA_TABLE)
        );
        tx.execute(&claim, &[&EMBEDDING_MODEL_SETTING, &model_id])?;
        tx.execute(&claim, &[&EMBEDDING_DIMENSION_SETTING, &dimension.to_string()])?;

        let recorded_model: String = tx
            .query_one(
                &format!(
                    "SELECT value FROM {} WHERE setting = $1",
                    self.table(SCHEMA_METADATA_TABLE)
                ),
                &[&EMBEDDING_MODEL_SETTING],
            )?
            .get(0);
        let recorded_dimension: i32 = tx
            .query_one(
                &format!(
                    "SELECT value::INTEGER FROM {} WHERE setting = $1",
                    self.table(SCHEMA_METADATA_TABLE)
                ),
                &[&EMBEDDING_DIMENSION_SETTING],
            )?
            .get(0);
        tx.commit()?;

        let recorded_dimension = recorded_dimension as usize;
        if recorded_model != model_id || recorded_dimension != dimension {
            let schema = &self.schema;
            return Err(SearchError::ExternalBaseline(format!(
                "embedding identity mismatch for schema '{schema}': baseline uses \
                 model '{recorded_model}' (dim {recorded_dimension}); refusing to publish with \
                 model '{model_id}' (dim {dimension}). A shared baseline must use one \
                 embedding model."
            )));
        }
        Ok(())
    }

    fn snapshot_row_to_model(row: Row) -> Snapshot {
        let corpus = CorpusId::from_storage(row.get::<_, String>("corpus"));
        let mut snapshot = Snapshot::new(row.get::<_, String>("id"), corpus);
        snapshot.fingerprint = row.get("fingerprint");
        snapshot.parent_id =
            row.get::<_, Option<String>>("parent_snapshot_id").map(crate::SnapshotId::new);
        snapshot
    }

    fn latest_snapshot_query(&self, with_filter: &str) -> String {
        format!(
            "SELECT id, corpus, fingerprint, parent_snapshot_id
             FROM {}
             WHERE {}
             ORDER BY created_at DESC
             LIMIT 1",
            self.table("snapshots"),
            with_filter
        )
    }

    /// Gives a carrier the root column on schemas published before roots existed.
    ///
    /// `CONFIGURATION_ROOT_ID` is the empty string precisely so this backfill is free: every
    /// pre-root row belongs to the configuration, so the default is already its true value and
    /// not one row is rewritten.
    fn add_root_id_column(&self, table: &str) -> String {
        format!(
            "ALTER TABLE {} ADD COLUMN IF NOT EXISTS root_id TEXT NOT NULL DEFAULT ''",
            self.table(table)
        )
    }

    /// Brings a carrier's primary key to `columns`, deciding by the catalog rather than by name.
    ///
    /// The name of a constraint guarantees nothing about its columns — a schema created before
    /// roots carries `..._pkey` over a two-part key — so the composition is read from
    /// `pg_constraint` and the rebuild happens only when it actually differs. That is what makes
    /// re-running the migration a no-op instead of a fresh lock on the largest tables.
    fn enforce_primary_key(&self, table: &str, columns: &[&str]) -> String {
        let qualified = self.table(table);
        format!(
            "DO $$
             DECLARE
                 target_columns TEXT[] := {};
                 actual_columns TEXT[];
                 key_name TEXT;
                 relation OID := to_regclass('{qualified}');
             BEGIN
                 IF relation IS NULL THEN
                     RETURN;
                 END IF;
                 SELECT c.conname, array_agg(a.attname ORDER BY k.ord)
                   INTO key_name, actual_columns
                   FROM pg_constraint c
                   CROSS JOIN LATERAL unnest(c.conkey) WITH ORDINALITY AS k(attnum, ord)
                   JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = k.attnum
                  WHERE c.conrelid = relation AND c.contype = 'p'
                  GROUP BY c.conname;
                 IF actual_columns IS DISTINCT FROM target_columns THEN
                     IF key_name IS NOT NULL THEN
                         EXECUTE format('ALTER TABLE {qualified} DROP CONSTRAINT %I', key_name);
                     END IF;
                     EXECUTE 'ALTER TABLE {qualified} ADD PRIMARY KEY {}';
                 END IF;
             END $$",
            sql_text_array(columns),
            sql_column_list(columns),
        )
    }

    /// Same decision for a secondary index, and for the same reason made explicit here.
    ///
    /// These indexes are declared with `CREATE INDEX IF NOT EXISTS` under a fixed name, so on a
    /// live database a re-declaration with new columns is skipped BY NAME and the index silently
    /// keeps its old composition. Nothing about the answers changes when that happens — only the
    /// cost of the query — which is why it would otherwise never be noticed.
    fn enforce_index(&self, index: &str, table: &str, columns: &[&str]) -> String {
        let qualified = self.table(table);
        let index_name = format!("idx_{}_{index}", self.schema);
        let qualified_index = format!("{}.{index_name}", self.schema);
        format!(
            "DO $$
             DECLARE
                 target_columns TEXT[] := {};
                 actual_columns TEXT[];
                 relation OID := to_regclass('{qualified_index}');
             BEGIN
                 IF relation IS NOT NULL THEN
                     SELECT array_agg(a.attname ORDER BY k.ord)
                       INTO actual_columns
                       FROM pg_index i
                       CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS k(attnum, ord)
                       JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum
                      WHERE i.indexrelid = relation;
                 END IF;
                 IF actual_columns IS DISTINCT FROM target_columns THEN
                     EXECUTE 'DROP INDEX IF EXISTS {qualified_index}';
                     EXECUTE 'CREATE INDEX {index_name} ON {qualified} {}';
                 END IF;
             END $$",
            sql_text_array(columns),
            sql_column_list(columns),
        )
    }

    fn ensure_schema_statements(&self) -> Vec<String> {
        vec![
            format!("CREATE SCHEMA IF NOT EXISTS {}", self.schema),
            format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    setting TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                )",
                self.table(SCHEMA_METADATA_TABLE)
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    id TEXT PRIMARY KEY,
                    corpus TEXT NOT NULL,
                    fingerprint TEXT NULL,
                    parent_snapshot_id TEXT NULL,
                    branch TEXT NULL,
                    commit_sha TEXT NULL,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
                )",
                self.table("snapshots")
            ),
            format!(
                "ALTER TABLE {} ADD COLUMN IF NOT EXISTS fingerprint TEXT NULL",
                self.table("snapshots")
            ),
            format!(
                "ALTER TABLE {} ADD COLUMN IF NOT EXISTS parent_snapshot_id TEXT NULL",
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
                    embedding_key TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    dimension INTEGER NOT NULL,
                    embedding BYTEA NOT NULL,
                    PRIMARY KEY (embedding_key, model_id, dimension)
                )",
                self.table("semantic_embeddings")
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    id TEXT PRIMARY KEY,
                    collection TEXT NOT NULL,
                    file_fingerprint TEXT NOT NULL,
                    document_count INTEGER NOT NULL
                )",
                self.table("file_objects")
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    file_object_id TEXT NOT NULL REFERENCES {}(id) ON DELETE CASCADE,
                    ordinal INTEGER NOT NULL,
                    symbol_name TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL,
                    content_hash TEXT NOT NULL REFERENCES {}(content_hash) ON DELETE RESTRICT,
                    graph_context TEXT,
                    PRIMARY KEY (file_object_id, ordinal)
                )",
                self.table("file_object_items"),
                self.table("file_objects"),
                self.table("content_objects")
            ),
            // Idempotent column add for central databases created before
            // graph-enriched embeddings; pre-existing rows keep NULL (no context,
            // matching their pre-enrichment embeddings).
            format!(
                "ALTER TABLE {} ADD COLUMN IF NOT EXISTS graph_context TEXT",
                self.table("file_object_items")
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    snapshot_id TEXT NOT NULL REFERENCES {}(id) ON DELETE CASCADE,
                    collection TEXT NOT NULL,
                    root_id TEXT NOT NULL DEFAULT '',
                    path TEXT NOT NULL,
                    file_fingerprint TEXT NOT NULL,
                    document_count INTEGER NOT NULL,
                    file_object_id TEXT NOT NULL REFERENCES {}(id) ON DELETE RESTRICT,
                    PRIMARY KEY (snapshot_id, collection, root_id, path)
                )",
                self.table("snapshot_files"),
                self.table("snapshots"),
                self.table("file_objects")
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    snapshot_id TEXT NOT NULL REFERENCES {}(id) ON DELETE CASCADE,
                    collection TEXT NOT NULL,
                    root_id TEXT NOT NULL DEFAULT '',
                    path TEXT NOT NULL,
                    PRIMARY KEY (snapshot_id, collection, root_id, path)
                )",
                self.table("snapshot_deletions"),
                self.table("snapshots"),
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    snapshot_id TEXT NOT NULL REFERENCES {}(id) ON DELETE CASCADE,
                    collection TEXT NOT NULL,
                    root_id TEXT NOT NULL DEFAULT '',
                    path TEXT NOT NULL,
                    ordinal INTEGER NOT NULL,
                    symbol_name TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL,
                    text TEXT NOT NULL,
                    tsv TSVECTOR NOT NULL,
                    PRIMARY KEY (snapshot_id, collection, root_id, path, ordinal)
                )",
                self.table("serving_lexical"),
                self.table("snapshots"),
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    corpus TEXT NOT NULL,
                    branch TEXT NOT NULL,
                    snapshot_id TEXT NOT NULL REFERENCES {}(id) ON DELETE CASCADE,
                    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                    PRIMARY KEY (corpus, branch)
                )",
                self.table("snapshot_heads"),
                self.table("snapshots"),
            ),
            // The root reaches the three mandatory carriers of file identity. Order matters:
            // the column exists before anything keys by it.
            self.add_root_id_column("snapshot_files"),
            self.add_root_id_column("snapshot_deletions"),
            self.add_root_id_column("serving_lexical"),
            self.enforce_primary_key(
                "snapshot_files",
                &["snapshot_id", "collection", "root_id", "path"],
            ),
            self.enforce_primary_key(
                "snapshot_deletions",
                &["snapshot_id", "collection", "root_id", "path"],
            ),
            self.enforce_primary_key(
                "serving_lexical",
                &["snapshot_id", "collection", "root_id", "path", "ordinal"],
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
            self.enforce_index(
                "snapshot_files_snapshot_path",
                "snapshot_files",
                &["snapshot_id", "collection", "root_id", "path"],
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_snapshot_files_object
                 ON {} (file_object_id)",
                self.schema,
                self.table("snapshot_files")
            ),
            self.enforce_index(
                "snapshot_deletions_snapshot_path",
                "snapshot_deletions",
                &["snapshot_id", "collection", "root_id", "path"],
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_serving_lexical_tsv
                 ON {} USING GIN (tsv)",
                self.schema,
                self.table("serving_lexical")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_serving_lexical_snapshot_id
                 ON {} (snapshot_id)",
                self.schema,
                self.table("serving_lexical")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_file_objects_fingerprint
                 ON {} (collection, file_fingerprint)",
                self.schema,
                self.table("file_objects")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_file_object_items_object
                 ON {} (file_object_id, ordinal)",
                self.schema,
                self.table("file_object_items")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_semantic_embeddings_model_dim
                 ON {} (model_id, dimension)",
                self.schema,
                self.table("semantic_embeddings")
            ),
        ]
    }

    fn pgvector_schema_statements(&self) -> Vec<String> {
        vec![
            "CREATE EXTENSION IF NOT EXISTS vector".to_owned(),
            format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    snapshot_id TEXT NOT NULL REFERENCES {}(id) ON DELETE CASCADE,
                    collection TEXT NOT NULL,
                    path TEXT NOT NULL,
                    ordinal INTEGER NOT NULL,
                    symbol_name TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL,
                    model_id TEXT NOT NULL,
                    dimension INTEGER NOT NULL,
                    embedding vector NOT NULL,
                    PRIMARY KEY (snapshot_id, model_id, collection, path, ordinal)
                )",
                self.table("serving_semantic"),
                self.table("snapshots"),
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_serving_semantic_snapshot_model
                 ON {} (snapshot_id, model_id)",
                self.schema,
                self.table("serving_semantic")
            ),
        ]
    }

    pub fn list_snapshots(
        &self,
        corpus: Option<&str>,
        branch: Option<&str>,
        commit: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BaselineSnapshotRecord>, SearchError> {
        self.check_storage_readiness()?;
        let mut client = self.connect()?;
        let limit = limit.clamp(1, 200) as i64;
        let corpus = corpus.map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned);
        let branch = branch.map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned);
        let commit = commit.map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned);

        let mut query = format!(
            "SELECT s.id,
                    s.corpus,
                    s.fingerprint,
                    s.parent_snapshot_id,
                    s.branch,
                    s.commit_sha,
                    s.created_at::TEXT AS created_at
             FROM {} s
             WHERE 1 = 1",
            self.table("snapshots"),
        );

        let mut params = Vec::<&(dyn postgres::types::ToSql + Sync)>::new();
        if let Some(corpus) = corpus.as_ref() {
            query.push_str(&format!(" AND s.corpus = ${}", params.len() + 1));
            params.push(corpus);
        }
        if let Some(branch) = branch.as_ref() {
            query.push_str(&format!(" AND s.branch = ${}", params.len() + 1));
            params.push(branch);
        }
        if let Some(commit) = commit.as_ref() {
            query.push_str(&format!(" AND s.commit_sha = ${}", params.len() + 1));
            params.push(commit);
        }
        query.push_str(" ORDER BY s.created_at DESC");
        query.push_str(&format!(" LIMIT ${}", params.len() + 1));
        params.push(&limit);

        let rows = client.query(&query, &params)?;
        let mut snapshots: Vec<BaselineSnapshotRecord> =
            rows.into_iter().map(snapshot_record_from_metadata_row).collect();

        // Effective file/document totals for every listed snapshot in one round-trip, rather than
        // walking each snapshot's ancestry and summarising it separately.
        let seed_ids: Vec<String> = snapshots.iter().map(|s| s.snapshot_id.clone()).collect();
        let totals = effective_snapshot_totals_batch(&mut *client, self, &seed_ids)?;
        for snapshot in &mut snapshots {
            let (files, documents) = totals.get(&snapshot.snapshot_id).copied().unwrap_or((0, 0));
            snapshot.files = files;
            snapshot.documents = documents;
        }
        Ok(snapshots)
    }

    pub fn snapshot_details(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<BaselineSnapshotDetails>, SearchError> {
        self.check_storage_readiness()?;
        let mut client = self.connect()?;

        let summary_query = format!(
            "SELECT s.id,
                    s.corpus,
                    s.fingerprint,
                    s.parent_snapshot_id,
                    s.branch,
                    s.commit_sha,
                    s.created_at::TEXT AS created_at
             FROM {} s
             WHERE s.id = $1
             LIMIT 1",
            self.table("snapshots"),
        );
        let Some(snapshot_row) = client.query_opt(&summary_query, &[&snapshot_id])? else {
            return Ok(None);
        };
        let mut snapshot = snapshot_record_from_metadata_row(snapshot_row);
        let summary = effective_snapshot_summary(&mut *client, self, snapshot_id)?;
        snapshot.files = summary.total_files;
        snapshot.documents = summary.total_documents;
        let collections = summary.collections;

        Ok(Some(BaselineSnapshotDetails { snapshot, collections }))
    }

    pub fn list_file_objects(
        &self,
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BaselineFileObjectRecord>, SearchError> {
        self.check_storage_readiness()?;
        let mut client = self.connect()?;
        let limit = limit.clamp(1, 200) as i64;
        let collection =
            collection.map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned);

        let mut query = format!(
            "SELECT fo.id,
                    fo.collection,
                    fo.file_fingerprint,
                    fo.document_count,
                    COUNT(DISTINCT sf.snapshot_id) AS snapshots
             FROM {} fo
             LEFT JOIN {} sf ON sf.file_object_id = fo.id
             WHERE 1 = 1",
            self.table("file_objects"),
            self.table("snapshot_files"),
        );
        let mut params = Vec::<&(dyn postgres::types::ToSql + Sync)>::new();
        if let Some(collection) = collection.as_ref() {
            query.push_str(&format!(" AND fo.collection = ${}", params.len() + 1));
            params.push(collection);
        }
        query.push_str(
            " GROUP BY fo.id, fo.collection, fo.file_fingerprint, fo.document_count
              ORDER BY snapshots DESC, fo.collection, fo.id",
        );
        query.push_str(&format!(" LIMIT ${}", params.len() + 1));
        params.push(&limit);

        Ok(client.query(&query, &params)?.into_iter().map(file_object_record_from_row).collect())
    }

    pub fn file_object_details(
        &self,
        file_object_id: &str,
    ) -> Result<Option<BaselineFileObjectDetails>, SearchError> {
        self.check_storage_readiness()?;
        let mut client = self.connect()?;
        let summary_query = format!(
            "SELECT fo.id,
                    fo.collection,
                    fo.file_fingerprint,
                    fo.document_count,
                    COUNT(DISTINCT sf.snapshot_id) AS snapshots
             FROM {} fo
             LEFT JOIN {} sf ON sf.file_object_id = fo.id
             WHERE fo.id = $1
             GROUP BY fo.id, fo.collection, fo.file_fingerprint, fo.document_count
             LIMIT 1",
            self.table("file_objects"),
            self.table("snapshot_files"),
        );
        let Some(file_object_row) = client.query_opt(&summary_query, &[&file_object_id])? else {
            return Ok(None);
        };

        let references_query = format!(
            "SELECT snapshot_id, path
             FROM {}
             WHERE file_object_id = $1
             ORDER BY snapshot_id, path",
            self.table("snapshot_files")
        );
        let references = client
            .query(&references_query, &[&file_object_id])?
            .into_iter()
            .map(|row| BaselineFileObjectReference {
                snapshot_id: row.get("snapshot_id"),
                root_id: CONFIGURATION_ROOT_ID.to_owned(),
                path: row.get("path"),
            })
            .collect();

        Ok(Some(BaselineFileObjectDetails {
            file_object: file_object_record_from_row(file_object_row),
            references,
        }))
    }

    pub fn store_embeddings(
        &self,
        model_id: &str,
        dimension: usize,
        embeddings: &[(String, Vec<f32>)],
    ) -> Result<BaselineEmbeddingStats, SearchError> {
        self.check_storage_readiness()?;
        if embeddings.is_empty() {
            return Ok(BaselineEmbeddingStats { stored: 0, reused: 0 });
        }

        let mut client = self.connect()?;
        let mut tx = client.transaction()?;
        let insert = format!(
            "INSERT INTO {} (embedding_key, model_id, dimension, embedding)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (embedding_key, model_id, dimension) DO NOTHING",
            self.table("semantic_embeddings")
        );

        let mut stored = 0usize;
        let mut seen = HashSet::new();
        for (embedding_key, embedding) in embeddings {
            if !seen.insert(embedding_key.as_str()) {
                continue;
            }
            let blob: Vec<u8> = embedding.iter().flat_map(|value| value.to_le_bytes()).collect();
            let inserted =
                tx.execute(&insert, &[&embedding_key, &model_id, &(dimension as i32), &blob])?;
            stored += inserted as usize;
        }

        tx.commit()?;
        Ok(BaselineEmbeddingStats { stored, reused: seen.len().saturating_sub(stored) })
    }

    pub fn load_embeddings(
        &self,
        embedding_keys: &[String],
        model_id: &str,
        dimension: usize,
    ) -> Result<HashMap<String, Vec<f32>>, SearchError> {
        self.check_storage_readiness()?;
        let mut client = self.connect()?;
        load_embeddings_from_client(&mut *client, self, embedding_keys, model_id, dimension)
    }

    pub fn list_embedding_models(
        &self,
        model_id: Option<&str>,
        dimension: Option<usize>,
    ) -> Result<Vec<BaselineEmbeddingModelRecord>, SearchError> {
        self.check_storage_readiness()?;
        let mut client = self.connect()?;
        let model_id =
            model_id.map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned);
        let dimension = dimension.map(|value| value as i32);

        let mut query = format!(
            "SELECT model_id, dimension, COUNT(*) AS embeddings
             FROM {}
             WHERE 1 = 1",
            self.table("semantic_embeddings")
        );
        let mut params = Vec::<&(dyn postgres::types::ToSql + Sync)>::new();
        if let Some(model_id) = model_id.as_ref() {
            query.push_str(&format!(" AND model_id = ${}", params.len() + 1));
            params.push(model_id);
        }
        if let Some(dimension) = dimension.as_ref() {
            query.push_str(&format!(" AND dimension = ${}", params.len() + 1));
            params.push(dimension);
        }
        query.push_str(" GROUP BY model_id, dimension ORDER BY model_id, dimension");

        Ok(client
            .query(&query, &params)?
            .into_iter()
            .map(|row| BaselineEmbeddingModelRecord {
                model_id: row.get("model_id"),
                dimension: row.get::<_, i32>("dimension") as usize,
                embeddings: row.get::<_, i64>("embeddings") as usize,
            })
            .collect())
    }

    pub fn embedding_coverage(
        &self,
        model_id: Option<&str>,
        dimension: Option<usize>,
    ) -> Result<Vec<BaselineEmbeddingCoverageRecord>, SearchError> {
        self.check_storage_readiness()?;
        let mut client = self.connect()?;
        let active_keys = collect_active_embedding_keys(&mut *client, self)?;
        let active_payloads = active_keys.len();
        let models = self.list_embedding_models(model_id, dimension)?;
        if models.is_empty() {
            return Ok(Vec::new());
        }

        let model_id =
            model_id.map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned);
        let dimension = dimension.map(|value| value as i32);
        let mut query = format!(
            "SELECT embedding_key, model_id, dimension
             FROM {}
             WHERE 1 = 1",
            self.table("semantic_embeddings")
        );
        let mut params = Vec::<&(dyn postgres::types::ToSql + Sync)>::new();
        if let Some(model_id) = model_id.as_ref() {
            query.push_str(&format!(" AND model_id = ${}", params.len() + 1));
            params.push(model_id);
        }
        if let Some(dimension) = dimension.as_ref() {
            query.push_str(&format!(" AND dimension = ${}", params.len() + 1));
            params.push(dimension);
        }

        let mut covered = HashMap::<(String, usize), HashSet<String>>::new();
        for row in client.query(&query, &params)? {
            let embedding_key: String = row.get("embedding_key");
            if !active_keys.contains(&embedding_key) {
                continue;
            }
            let model_id: String = row.get("model_id");
            let dimension = row.get::<_, i32>("dimension") as usize;
            covered.entry((model_id, dimension)).or_default().insert(embedding_key);
        }

        Ok(models
            .into_iter()
            .map(|model| {
                let covered_payloads = covered
                    .remove(&(model.model_id.clone(), model.dimension))
                    .map(|keys| keys.len())
                    .unwrap_or(0);
                BaselineEmbeddingCoverageRecord {
                    model_id: model.model_id,
                    dimension: model.dimension,
                    active_payloads,
                    covered_payloads,
                    embeddings: model.embeddings,
                }
            })
            .collect())
    }

    pub fn garbage_collect(&self, execute: bool) -> Result<BaselineGcReport, SearchError> {
        self.check_storage_readiness()?;
        let mut client = self.connect()?;
        let active_keys = collect_active_embedding_keys(&mut *client, self)?;

        let orphan_file_object_ids = query_string_column(
            &mut *client,
            &format!(
                "SELECT fo.id
                 FROM {} fo
                 LEFT JOIN {} sf ON sf.file_object_id = fo.id
                 WHERE sf.file_object_id IS NULL
                 ORDER BY fo.id",
                self.table("file_objects"),
                self.table("snapshot_files"),
            ),
            &[],
        )?;

        let orphan_file_object_item_rows = client.query(
            &format!(
                "SELECT foi.file_object_id
                 FROM {} foi
                 LEFT JOIN {} fo ON fo.id = foi.file_object_id
                 WHERE fo.id IS NULL
                 ORDER BY foi.file_object_id, foi.ordinal",
                self.table("file_object_items"),
                self.table("file_objects"),
            ),
            &[],
        )?;
        let orphan_file_object_items = orphan_file_object_item_rows.len();
        let orphan_file_object_item_ids = orphan_file_object_item_rows
            .into_iter()
            .map(|row| row.get::<_, String>("file_object_id"))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        let orphan_semantic_embedding_rows = client.query(
            &format!(
                "SELECT embedding_key
                 FROM {}
                 ORDER BY embedding_key",
                self.table("semantic_embeddings")
            ),
            &[],
        )?;
        let orphan_semantic_embeddings = orphan_semantic_embedding_rows
            .iter()
            .filter(|row| !active_keys.contains(row.get::<_, String>("embedding_key").as_str()))
            .count();
        let orphan_semantic_embedding_keys = orphan_semantic_embedding_rows
            .into_iter()
            .map(|row| row.get::<_, String>("embedding_key"))
            .filter(|key| !active_keys.contains(key))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        let mut report = BaselineGcReport {
            orphan_file_objects: orphan_file_object_ids.len(),
            orphan_file_object_items,
            orphan_semantic_embeddings,
            deleted_file_objects: 0,
            deleted_file_object_items: 0,
            deleted_semantic_embeddings: 0,
        };

        if !execute {
            return Ok(report);
        }

        let mut tx = client.transaction()?;
        if !orphan_file_object_item_ids.is_empty() {
            let delete = format!(
                "DELETE FROM {}
                 WHERE file_object_id = ANY($1)",
                self.table("file_object_items")
            );
            report.deleted_file_object_items =
                tx.execute(&delete, &[&orphan_file_object_item_ids])? as usize;
        }
        if !orphan_file_object_ids.is_empty() {
            let delete = format!(
                "DELETE FROM {}
                 WHERE id = ANY($1)",
                self.table("file_objects")
            );
            report.deleted_file_objects = tx.execute(&delete, &[&orphan_file_object_ids])? as usize;
        }
        if !orphan_semantic_embedding_keys.is_empty() {
            let delete = format!(
                "DELETE FROM {}
                 WHERE embedding_key = ANY($1)",
                self.table("semantic_embeddings")
            );
            report.deleted_semantic_embeddings =
                tx.execute(&delete, &[&orphan_semantic_embedding_keys])? as usize;
        }
        tx.commit()?;

        Ok(report)
    }

    pub fn populate_serving_semantic(
        &self,
        snapshot_id: &str,
        model_id: &str,
        dimension: usize,
    ) -> Result<usize, SearchError> {
        self.populate_serving_semantic_with_progress(snapshot_id, model_id, dimension, None)
    }

    pub fn populate_serving_semantic_with_progress(
        &self,
        snapshot_id: &str,
        model_id: &str,
        dimension: usize,
        progress: Option<&dyn Fn(SemanticPublishProgress)>,
    ) -> Result<usize, SearchError> {
        self.check_storage_readiness()?;
        let mut client = self.connect()?;
        if !self.storage_table_exists(&mut *client, "serving_semantic")? {
            return Err(SearchError::ExternalBaseline(
                "serving_semantic_unavailable: semantic serving table is not initialized"
                    .to_owned(),
            ));
        }

        let planning_started = Instant::now();
        let parent_snapshot_id = snapshot_parent_id(&mut *client, self, snapshot_id)?;
        let current_snapshot_files = load_snapshot_file_rows(&mut *client, self, snapshot_id)?;
        let deleted_paths = load_snapshot_deletion_keys(&mut *client, self, snapshot_id)?;
        let parent_complete = match parent_snapshot_id.as_deref() {
            Some(parent_snapshot_id) => semantic_publication_complete(
                &mut *client,
                self,
                parent_snapshot_id,
                model_id,
                dimension,
            )?,
            None => false,
        };
        let plan = semantic_publish_plan(
            parent_snapshot_id.clone(),
            parent_complete,
            &current_snapshot_files,
            &deleted_paths,
        );
        let strategy = plan.strategy.clone();
        let phase_count = semantic_publish_phase_count(&strategy);
        if let Some(on_progress) = progress {
            on_progress(SemanticPublishProgress::Plan {
                strategy: strategy.label().to_owned(),
                changed_files: plan.changed_paths.len(),
                deleted_paths: plan.deleted_paths.len(),
                parent_snapshot_id: strategy.parent_snapshot_id().map(str::to_owned),
                phase_count,
            });
        }

        let mut timings = SemanticPublishPhaseTimings {
            ancestry_materialization: planning_started.elapsed(),
            ..SemanticPublishPhaseTimings::default()
        };

        let (prepared_rows, missing_embeddings) = match &strategy {
            SemanticPublishStrategy::CurrentSnapshotOnly => {
                let recompute_started = Instant::now();
                if let Some(on_progress) = progress {
                    on_progress(SemanticPublishProgress::PhaseStarted {
                        phase: SemanticPublishPhase::PrepareRows,
                        phase_index: 1,
                        phase_count,
                        detail: format!(
                            "Preparing semantic rows for {} changed files",
                            plan.changed_paths.len()
                        ),
                    });
                }
                let prepared = prepare_semantic_rows_for_files(
                    &mut *client,
                    self,
                    &current_snapshot_files,
                    model_id,
                    dimension,
                )?;
                timings.changed_recompute = recompute_started.elapsed();
                if let Some(on_progress) = progress {
                    on_progress(SemanticPublishProgress::PhaseCompleted {
                        phase: SemanticPublishPhase::PrepareRows,
                        phase_index: 1,
                        phase_count,
                        elapsed: timings.changed_recompute,
                        output_rows: prepared.rows.len(),
                    });
                }
                (prepared.rows, prepared.missing_embeddings)
            }
            SemanticPublishStrategy::IncrementalFromParent { .. } => {
                let recompute_started = Instant::now();
                if let Some(on_progress) = progress {
                    on_progress(SemanticPublishProgress::PhaseStarted {
                        phase: SemanticPublishPhase::PrepareRows,
                        phase_index: 1,
                        phase_count,
                        detail: format!(
                            "Preparing semantic rows for {} changed files",
                            plan.changed_paths.len()
                        ),
                    });
                }
                let prepared = prepare_semantic_rows_for_files(
                    &mut *client,
                    self,
                    &current_snapshot_files,
                    model_id,
                    dimension,
                )?;
                timings.changed_recompute = recompute_started.elapsed();
                if let Some(on_progress) = progress {
                    on_progress(SemanticPublishProgress::PhaseCompleted {
                        phase: SemanticPublishPhase::PrepareRows,
                        phase_index: 1,
                        phase_count,
                        elapsed: timings.changed_recompute,
                        output_rows: prepared.rows.len(),
                    });
                }
                (prepared.rows, prepared.missing_embeddings)
            }
            SemanticPublishStrategy::FullRebuild => {
                let rebuild_started = Instant::now();
                if let Some(on_progress) = progress {
                    on_progress(SemanticPublishProgress::PhaseStarted {
                        phase: SemanticPublishPhase::PrepareRows,
                        phase_index: 1,
                        phase_count,
                        detail: "Recomputing semantic rows for the full visible snapshot"
                            .to_owned(),
                    });
                }
                let visible_files =
                    materialize_visible_snapshot_files(&mut *client, self, snapshot_id)?;
                timings.ancestry_materialization = rebuild_started.elapsed();

                let recompute_started = Instant::now();
                let prepared = prepare_semantic_rows_for_files(
                    &mut *client,
                    self,
                    &visible_files,
                    model_id,
                    dimension,
                )?;
                timings.changed_recompute = recompute_started.elapsed();
                if let Some(on_progress) = progress {
                    on_progress(SemanticPublishProgress::PhaseCompleted {
                        phase: SemanticPublishPhase::PrepareRows,
                        phase_index: 1,
                        phase_count,
                        elapsed: timings.ancestry_materialization + timings.changed_recompute,
                        output_rows: prepared.rows.len(),
                    });
                }
                (prepared.rows, prepared.missing_embeddings)
            }
        };

        let changed_rows = prepared_rows.len();
        let changed_files = plan.changed_paths.len();
        let final_sync_started = Instant::now();
        let mut tx = client.transaction()?;
        clear_semantic_publication_complete(&mut tx, self, snapshot_id, model_id, dimension)?;
        delete_serving_semantic_rows(&mut tx, self, snapshot_id, model_id, dimension)?;

        let copied_rows = match &strategy {
            SemanticPublishStrategy::IncrementalFromParent { parent_snapshot_id } => {
                let copy_started = Instant::now();
                if let Some(on_progress) = progress {
                    on_progress(SemanticPublishProgress::PhaseStarted {
                        phase: SemanticPublishPhase::CopyParentRows,
                        phase_index: 2,
                        phase_count,
                        detail: format!(
                            "Copying unchanged rows from parent snapshot {}",
                            parent_snapshot_id
                        ),
                    });
                }
                let copied = copy_parent_serving_semantic_rows(
                    &mut tx,
                    self,
                    snapshot_id,
                    parent_snapshot_id,
                    model_id,
                    dimension,
                )?;
                timings.parent_copy = copy_started.elapsed();
                if let Some(on_progress) = progress {
                    on_progress(SemanticPublishProgress::PhaseCompleted {
                        phase: SemanticPublishPhase::CopyParentRows,
                        phase_index: 2,
                        phase_count,
                        elapsed: timings.parent_copy,
                        output_rows: copied,
                    });
                }
                copied
            }
            _ => 0,
        };
        if let Some(on_progress) = progress {
            on_progress(SemanticPublishProgress::PhaseStarted {
                phase: SemanticPublishPhase::WriteServingRows,
                phase_index: phase_count,
                phase_count,
                detail: format!(
                    "Writing {} changed rows into serving_semantic",
                    prepared_rows.len()
                ),
            });
        }
        let inserted_rows = insert_serving_semantic_rows(
            &mut tx,
            self,
            snapshot_id,
            model_id,
            dimension,
            &prepared_rows,
        )?;
        if missing_embeddings == 0 {
            mark_semantic_publication_complete(&mut tx, self, snapshot_id, model_id, dimension)?;
        }
        tx.commit()?;
        timings.final_sync = final_sync_started.elapsed();
        if let Some(on_progress) = progress {
            on_progress(SemanticPublishProgress::PhaseCompleted {
                phase: SemanticPublishPhase::WriteServingRows,
                phase_index: phase_count,
                phase_count,
                elapsed: timings.final_sync,
                output_rows: inserted_rows,
            });
            on_progress(SemanticPublishProgress::Completed {
                total_rows: copied_rows + inserted_rows,
                copied_rows,
                inserted_rows,
                missing_embeddings,
                total_elapsed: planning_started.elapsed(),
            });
        }

        log_semantic_publish_phase_timings(
            snapshot_id,
            model_id,
            dimension,
            &strategy,
            &timings,
            &SemanticPublishLogStats {
                copied_rows,
                inserted_rows,
                missing_embeddings,
                changed_files,
                changed_rows,
                deleted_paths: plan.deleted_paths.len(),
            },
        );
        Ok(copied_rows + inserted_rows)
    }
}

impl SnapshotCatalog for PostgresBaselineAdapter {
    fn resolve_baseline(&self, baseline: &BaselineRef) -> Result<Option<Snapshot>, SearchError> {
        self.check_storage_readiness()?;
        let mut client = self.connect()?;
        let corpus = baseline.corpus.as_str();

        let row = if let Some(snapshot_id) = &baseline.snapshot_id {
            let query = format!(
                "SELECT id, corpus, fingerprint, parent_snapshot_id
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
            let head_query = format!(
                "SELECT s.id, s.corpus, s.fingerprint, s.parent_snapshot_id
                 FROM {} h
                 JOIN {} s ON s.id = h.snapshot_id
                 WHERE h.corpus = $1 AND h.branch = $2
                 LIMIT 1",
                self.table("snapshot_heads"),
                self.table("snapshots"),
            );
            let row = client.query_opt(&head_query, &[&corpus, branch]).ok().flatten();
            if row.is_some() {
                row
            } else {
                let fallback = self.latest_snapshot_query("corpus = $1 AND branch = $2");
                client.query_opt(&fallback, &[&corpus, branch])?
            }
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
        self.check_storage_readiness()?;
        let mut client = self.connect()?;
        let visible_files = materialize_visible_snapshot_files(&mut *client, self, &snapshot.id.0)?;
        if visible_files.is_empty() {
            return Ok(Vec::new());
        }

        let file_object_ids =
            visible_files.iter().map(|file| file.file_object_id.clone()).collect::<Vec<_>>();
        let items_query = format!(
            "SELECT foi.file_object_id,
                    foi.symbol_name,
                    foi.kind,
                    foi.line_start,
                    foi.line_end,
                    co.text,
                    foi.content_hash,
                    foi.graph_context
             FROM {} foi
             JOIN {} co ON co.content_hash = foi.content_hash
             WHERE foi.file_object_id = ANY($1)
             ORDER BY foi.file_object_id, foi.ordinal",
            self.table("file_object_items"),
            self.table("content_objects")
        );
        let mut items_by_file_object = HashMap::<String, Vec<FileObjectItem>>::new();
        for row in client.query(&items_query, &[&file_object_ids])? {
            let file_object_id: String = row.get("file_object_id");
            items_by_file_object.entry(file_object_id).or_default().push(FileObjectItem {
                symbol_name: row.get("symbol_name"),
                kind: row.get("kind"),
                line_start: row.get::<_, i32>("line_start") as u32,
                line_end: row.get::<_, i32>("line_end") as u32,
                text: row.get("text"),
                content_hash: row.get("content_hash"),
                graph_context: row.get("graph_context"),
            });
        }

        let mut documents = Vec::new();
        for file in visible_files {
            if let Some(items) = items_by_file_object.get(&file.file_object_id) {
                for item in items {
                    documents.push(IndexedDocument {
                        collection: file.collection.clone(),
                        // The published corpus is built from the configuration
                        // root alone, so its rows carry no other identity.
                        root_id: CONFIGURATION_ROOT_ID.to_owned(),
                        path: file.path.clone(),
                        symbol_name: item.symbol_name.clone(),
                        kind: item.kind.clone(),
                        line_start: item.line_start,
                        line_end: item.line_end,
                        text: item.text.clone(),
                        content_hash: item.content_hash.clone(),
                        graph_context: item.graph_context.clone(),
                    });
                }
            }
        }
        documents.sort_by(|lhs, rhs| {
            (
                lhs.collection.as_str(),
                lhs.path.as_str(),
                lhs.line_start,
                lhs.line_end,
                lhs.symbol_name.as_str(),
            )
                .cmp(&(
                    rhs.collection.as_str(),
                    rhs.path.as_str(),
                    rhs.line_start,
                    rhs.line_end,
                    rhs.symbol_name.as_str(),
                ))
        });
        Ok(documents)
    }
}

impl BaselineLexicalSearch for PostgresBaselineAdapter {
    fn lexical_search_baseline(
        &self,
        snapshot_id: &str,
        query: &str,
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LexicalHit>, SearchError> {
        // OR the per-term `plainto_tsquery`s instead of passing the whole query to a single
        // `plainto_tsquery` (which ANDs every lexeme): a multi-word query must surface a chunk
        // that contains any term, with `ts_rank` lifting chunks that contain more of them. Each
        // term is a bound parameter, so no tsquery operator can be injected.
        let terms: Vec<String> =
            crate::lexical::query_terms(query).into_iter().map(str::to_owned).collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }

        self.check_storage_readiness()?;
        let collection =
            collection.map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned);
        let limit = limit.clamp(1, 200) as i64;
        let mut client = self.connect()?;

        let snapshot_id = snapshot_id.to_owned();
        let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> = vec![&snapshot_id];
        let tsquery_expr = format!(
            "({})",
            terms
                .iter()
                .map(|term| {
                    params.push(term);
                    format!("plainto_tsquery('simple', ${})", params.len())
                })
                .collect::<Vec<_>>()
                .join(" || ")
        );
        let mut sql = format!(
            "SELECT collection,
                    path,
                    symbol_name,
                    kind,
                    line_start,
                    line_end,
                    text,
                    ts_rank(tsv, {tsquery_expr}, 32) AS rank
             FROM {}
             WHERE snapshot_id = $1
               AND tsv @@ {tsquery_expr}",
            self.table("serving_lexical")
        );

        if let Some(collection) = collection.as_ref() {
            sql.push_str(&format!(" AND collection = ${}", params.len() + 1));
            params.push(collection);
        }
        sql.push_str(" ORDER BY rank DESC, collection, path, ordinal");
        sql.push_str(&format!(" LIMIT ${}", params.len() + 1));
        params.push(&limit);

        let rows = client.query(&sql, &params)?;
        if rows.is_empty()
            && !self.lexical_serving_rows_exist(
                &mut *client,
                &snapshot_id,
                collection.as_deref(),
            )?
        {
            return Err(SearchError::ExternalBaseline(
                "serving_lexical_unavailable: snapshot has no serving rows for lexical search"
                    .to_owned(),
            ));
        }

        Ok(rows
            .into_iter()
            .map(|row| LexicalHit {
                collection: row.get("collection"),
                root_id: CONFIGURATION_ROOT_ID.to_owned(),
                path: row.get("path"),
                symbol_name: row.get("symbol_name"),
                kind: row.get("kind"),
                line_start: row.get::<_, i32>("line_start") as u32,
                line_end: row.get::<_, i32>("line_end") as u32,
                text: row.get("text"),
                rank: row.get::<_, f32>("rank"),
            })
            .collect())
    }
}

impl BaselineSemanticSearch for PostgresBaselineAdapter {
    fn semantic_search_baseline(
        &self,
        snapshot_id: &str,
        query_embedding: &[f32],
        model_id: &str,
        dimension: usize,
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SemanticHit>, SearchError> {
        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }

        self.check_storage_readiness()?;
        let collection =
            collection.map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned);
        let limit = limit.clamp(1, 200) as i64;
        let dimension = dimension as i32;
        let snapshot_id = snapshot_id.to_owned();
        let model_id = model_id.to_owned();
        let vector_text = format!(
            "[{}]",
            query_embedding.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
        );

        let mut sql = format!(
            "SELECT collection,
                    path,
                    symbol_name,
                    kind,
                    line_start,
                    line_end,
                    1.0 - (embedding <=> $1::text::vector) AS score
             FROM {}
             WHERE snapshot_id = $2 AND model_id = $3 AND dimension = $4",
            self.table("serving_semantic")
        );

        let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> =
            vec![&vector_text, &snapshot_id, &model_id, &dimension];
        if let Some(collection) = collection.as_ref() {
            sql.push_str(&format!(" AND collection = ${}", params.len() + 1));
            params.push(collection);
        }
        sql.push_str(&format!(
            " ORDER BY embedding <=> $1::text::vector LIMIT ${}",
            params.len() + 1
        ));
        params.push(&limit);

        let mut client = self.connect()?;
        if !self.storage_table_exists(&mut *client, "serving_semantic")? {
            return Err(SearchError::ExternalBaseline(
                "serving_semantic_unavailable: semantic serving table is not initialized"
                    .to_owned(),
            ));
        }

        let rows = client.query(&sql, &params)?;
        if rows.is_empty()
            && !self.semantic_serving_rows_exist(
                &mut *client,
                &snapshot_id,
                &model_id,
                dimension,
                collection.as_deref(),
            )?
        {
            return Err(SearchError::ExternalBaseline(
                "serving_semantic_unavailable: snapshot has no semantic serving rows".to_owned(),
            ));
        }

        Ok(rows
            .into_iter()
            .map(|row| SemanticHit {
                collection: row.get("collection"),
                root_id: CONFIGURATION_ROOT_ID.to_owned(),
                path: row.get("path"),
                symbol_name: row.get("symbol_name"),
                kind: row.get("kind"),
                line_start: row.get::<_, i32>("line_start") as u32,
                line_end: row.get::<_, i32>("line_end") as u32,
                score: row.get::<_, f64>("score") as f32,
            })
            .collect())
    }
}

impl WorkspaceBaselineManifestStore for PostgresBaselineAdapter {
    fn load_baseline_manifest(
        &self,
        snapshot_id: &str,
    ) -> Result<crate::ports::WorkspaceBaselineManifest, SearchError> {
        self.check_storage_readiness()?;
        let mut client = self.connect()?;

        let meta_query = format!(
            "SELECT id, fingerprint FROM {} WHERE id = $1 LIMIT 1",
            self.table("snapshots")
        );
        let Some(meta_row) = client.query_opt(&meta_query, &[&snapshot_id])? else {
            return Err(SearchError::ExternalBaseline(format!(
                "snapshot '{}' not found for manifest loading",
                snapshot_id
            )));
        };
        let snapshot_fingerprint: Option<String> = meta_row.get("fingerprint");

        let visible_files = materialize_visible_snapshot_files(&mut *client, self, snapshot_id)?;

        let files = visible_files
            .into_iter()
            .filter(|f| f.collection == "code")
            .map(|f| crate::ports::BaselineManifestFile {
                collection: f.collection,
                path: f.path,
                file_fingerprint: f.file_fingerprint,
                document_count: f.document_count,
                file_object_id: f.file_object_id,
            })
            .collect();

        Ok(crate::ports::WorkspaceBaselineManifest {
            snapshot_id: snapshot_id.to_owned(),
            snapshot_fingerprint,
            files,
        })
    }
}

impl PostgresBaselineAdapter {
    pub fn migrate_storage(&self) -> Result<(), SearchError> {
        let mut client = self.connect()?;
        let statements = self.ensure_schema_statements();
        self.migrate_structure(&mut client, &statements)?;

        // The optional half stays outside and stays tolerant: a database without the `vector`
        // extension is fully usable for everything but semantic serving, which refuses by name.
        for statement in self.pgvector_schema_statements() {
            if let Err(e) = client.batch_execute(&statement) {
                tracing::warn!("pgvector DDL skipped (semantic serving will be unavailable): {e}");
                break;
            }
        }

        Ok(())
    }

    /// Applies the mandatory half of the migration as one transaction, version included.
    ///
    /// Changing a primary key is not idempotent in the middle: between `DROP CONSTRAINT` and
    /// `ADD PRIMARY KEY` the table has no key at all, and an interrupted run would leave a state
    /// a retry cannot repair. PostgreSQL keeps DDL transactional, so one transaction removes
    /// that state entirely.
    ///
    /// The version is stamped inside the same transaction rather than after it. Stamped after,
    /// a rolled-back structural change would still be followed by version 2 on an unmigrated
    /// schema, `Ok` from this function, and "storage is ready" printed at the operator — a false
    /// ready, and a baseline switched off for every consumer.
    fn migrate_structure(
        &self,
        client: &mut PgPooledConnection,
        statements: &[String],
    ) -> Result<(), SearchError> {
        let mut tx = client.transaction()?;
        for statement in statements {
            tx.batch_execute(statement)?;
        }
        self.write_schema_version(&mut tx)?;
        tx.commit()?;
        Ok(())
    }
}

impl SnapshotPublisher for PostgresBaselineAdapter {
    fn publish_snapshot(
        &self,
        snapshot: &Snapshot,
        metadata: &SnapshotPublishMetadata,
        documents: &[IndexedDocument],
    ) -> Result<SnapshotPublishStats, SearchError> {
        if snapshot.parent_id.as_ref().is_some_and(|parent| parent.0 == snapshot.id.0) {
            return Err(SearchError::ExternalBaseline(
                "snapshot cannot reference itself as parent".to_owned(),
            ));
        }

        // Ahead of the readiness check, which connects: what this schema cannot store must be
        // refused without touching the database at all.
        ensure_the_schema_can_store_these_roots(documents)?;

        self.check_storage_readiness()?;

        let mut phase_timings = SnapshotPublishPhaseTimings::default();
        let grouping_started = Instant::now();
        let file_groups = group_documents_by_file(documents);
        phase_timings.grouping = grouping_started.elapsed();

        let mut client = self.connect()?;
        let mut tx = client.transaction()?;

        let upsert_snapshot = format!(
            "INSERT INTO {} (id, corpus, fingerprint, parent_snapshot_id, branch, commit_sha)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE SET
                corpus = EXCLUDED.corpus,
                fingerprint = EXCLUDED.fingerprint,
                parent_snapshot_id = EXCLUDED.parent_snapshot_id,
                branch = EXCLUDED.branch,
                commit_sha = EXCLUDED.commit_sha",
            self.table("snapshots")
        );
        tx.execute(
            &upsert_snapshot,
            &[
                &snapshot.id.0,
                &snapshot.corpus.as_str(),
                &snapshot.fingerprint,
                &snapshot.parent_id.as_ref().map(|value| value.0.as_str()),
                &metadata.branch.as_deref(),
                &metadata.commit.as_deref(),
            ],
        )?;

        let delete_snapshot_files =
            format!("DELETE FROM {} WHERE snapshot_id = $1", self.table("snapshot_files"));
        tx.execute(&delete_snapshot_files, &[&snapshot.id.0])?;
        let delete_snapshot_deletions =
            format!("DELETE FROM {} WHERE snapshot_id = $1", self.table("snapshot_deletions"));
        tx.execute(&delete_snapshot_deletions, &[&snapshot.id.0])?;
        invalidate_semantic_publication_for_snapshot(&mut tx, self, &snapshot.id.0)?;

        let parent_files = if let Some(parent_id) = snapshot.parent_id.as_ref() {
            materialize_visible_snapshot_file_map(&mut tx, self, &parent_id.0)?
        } else {
            BTreeMap::new()
        };

        let mut stats = SnapshotPublishStats::default();
        let mut remaining_parent_files = parent_files;
        let mut snapshot_file_rows = Vec::new();
        let mut snapshot_deletion_rows = Vec::new();
        let normalized_content_started = Instant::now();
        for file_group in &file_groups {
            let file_key = (
                file_group.collection.clone(),
                file_group.root_id.clone(),
                file_group.path.clone(),
            );
            let parent_entry = remaining_parent_files.remove(&file_key);
            if parent_entry
                .as_ref()
                .is_some_and(|parent| parent.file_fingerprint == file_group.file_fingerprint)
            {
                stats.reused_files += 1;
                stats.reused_documents += file_group.documents.len();
                continue;
            }

            let file_object_id =
                file_object_id_for(&file_group.collection, &file_group.file_fingerprint);
            let inserted = try_insert_file_object(&mut tx, self, &file_object_id, file_group)?;
            snapshot_file_rows.push(SnapshotFileRow {
                snapshot_id: snapshot.id.0.clone(),
                collection: file_group.collection.clone(),
                path: file_group.path.clone(),
                file_fingerprint: file_group.file_fingerprint.clone(),
                document_count: file_group.documents.len() as i32,
                file_object_id,
            });
            stats.written_files += 1;
            stats.written_documents += file_group.documents.len();
            if !inserted {
                tracing::trace!(
                    collection = %file_group.collection,
                    path = %file_group.path,
                    documents = file_group.documents.len(),
                    "reused existing file object during snapshot publish"
                );
            }
        }

        // The root is dropped here because `snapshot_deletions` has no column for it yet. That
        // loses nothing today: a corpus carrying roots is refused before it reaches this
        // function, so every key here is the configuration's.
        for ((collection, _root_id, path), _) in remaining_parent_files {
            snapshot_deletion_rows.push(SnapshotDeletionRow {
                snapshot_id: snapshot.id.0.clone(),
                collection,
                path,
            });
            stats.deleted_files += 1;
        }

        insert_snapshot_file_rows(&mut tx, self, &snapshot_file_rows)?;
        insert_snapshot_deletion_rows(&mut tx, self, &snapshot_deletion_rows)?;
        phase_timings.normalized_content = normalized_content_started.elapsed();

        let lexical_started = Instant::now();
        replace_serving_lexical_rows(&mut tx, self, &snapshot.id.0, &file_groups)?;
        phase_timings.lexical_rebuild = lexical_started.elapsed();

        let final_activation_started = Instant::now();
        if let Some(branch) = metadata.branch.as_deref() {
            let upsert_head = format!(
                "INSERT INTO {heads} (corpus, branch, snapshot_id, updated_at)
                 VALUES ($1, $2, $3, NOW())
                 ON CONFLICT (corpus, branch) DO UPDATE SET
                    snapshot_id = EXCLUDED.snapshot_id,
                    updated_at = NOW()
                 WHERE (SELECT created_at FROM {snaps} WHERE id = {heads}.snapshot_id)
                       < (SELECT created_at FROM {snaps} WHERE id = EXCLUDED.snapshot_id)",
                heads = self.table("snapshot_heads"),
                snaps = self.table("snapshots"),
            );
            tx.execute(&upsert_head, &[&snapshot.corpus.as_str(), &branch, &snapshot.id.0])?;
        }

        tx.commit()?;
        phase_timings.final_activation = final_activation_started.elapsed();
        log_snapshot_publish_phase_timings(snapshot, &phase_timings, &stats, documents.len());
        Ok(stats)
    }
}

#[derive(Debug, Clone, Default)]
struct SnapshotPublishPhaseTimings {
    grouping: Duration,
    normalized_content: Duration,
    lexical_rebuild: Duration,
    final_activation: Duration,
}

#[derive(Debug, Clone, Default)]
struct SemanticPublishPhaseTimings {
    ancestry_materialization: Duration,
    parent_copy: Duration,
    changed_recompute: Duration,
    final_sync: Duration,
}

#[derive(Debug, Clone, Default)]
struct SemanticPublishLogStats {
    copied_rows: usize,
    inserted_rows: usize,
    missing_embeddings: usize,
    changed_files: usize,
    changed_rows: usize,
    deleted_paths: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticPublishPlan {
    strategy: SemanticPublishStrategy,
    changed_paths: BTreeSet<(String, String)>,
    deleted_paths: BTreeSet<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SemanticPublishStrategy {
    CurrentSnapshotOnly,
    IncrementalFromParent { parent_snapshot_id: String },
    FullRebuild,
}

impl SemanticPublishStrategy {
    fn label(&self) -> &'static str {
        match self {
            Self::CurrentSnapshotOnly => "current_snapshot_only",
            Self::IncrementalFromParent { .. } => "incremental_copy_forward",
            Self::FullRebuild => "full_rebuild",
        }
    }

    fn parent_snapshot_id(&self) -> Option<&str> {
        match self {
            Self::IncrementalFromParent { parent_snapshot_id } => Some(parent_snapshot_id.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct PublishedFileGroup {
    collection: String,
    root_id: String,
    path: String,
    file_fingerprint: String,
    documents: Vec<IndexedDocument>,
}

#[derive(Debug, Clone)]
struct VisibleSnapshotFile {
    collection: String,
    /// The source root this file belongs to. The schema has no column for it yet, so every
    /// row read from it is the configuration's — which is precisely what the schema asserts
    /// today, and why the constant is honest here rather than a placeholder.
    root_id: String,
    path: String,
    file_fingerprint: String,
    document_count: usize,
    file_object_id: String,
}

/// What makes one visible file distinct from another within a snapshot's ancestry:
/// collection, the source root, and the path relative to that root.
///
/// The root is part of it because an extension repeats the configuration's directory layout,
/// so the same relative path names a different file under each root. Without it a deletion
/// recorded for one root shadows the live file of another.
type VisibleFileKey = (String, String, String);

impl VisibleSnapshotFile {
    fn key(&self) -> VisibleFileKey {
        (self.collection.clone(), self.root_id.clone(), self.path.clone())
    }
}

#[derive(Debug, Clone)]
struct FileObjectItem {
    symbol_name: String,
    kind: String,
    line_start: u32,
    line_end: u32,
    text: String,
    content_hash: String,
    graph_context: Option<String>,
}

#[derive(Debug, Clone)]
struct ContentObjectRow {
    content_hash: String,
    text: String,
}

#[derive(Debug, Clone)]
struct FileObjectItemRow {
    file_object_id: String,
    ordinal: i32,
    symbol_name: String,
    kind: String,
    line_start: i32,
    line_end: i32,
    content_hash: String,
    graph_context: Option<String>,
}

#[derive(Debug, Clone)]
struct SnapshotFileRow {
    snapshot_id: String,
    collection: String,
    path: String,
    file_fingerprint: String,
    document_count: i32,
    file_object_id: String,
}

#[derive(Debug, Clone)]
struct SnapshotDeletionRow {
    snapshot_id: String,
    collection: String,
    path: String,
}

#[derive(Debug, Clone)]
struct ServingLexicalRow {
    snapshot_id: String,
    collection: String,
    path: String,
    ordinal: i32,
    symbol_name: String,
    kind: String,
    line_start: i32,
    line_end: i32,
    text: String,
}

#[derive(Debug, Clone)]
struct PendingSemanticRow {
    collection: String,
    path: String,
    ordinal: i32,
    symbol_name: String,
    kind: String,
    line_start: i32,
    line_end: i32,
    embedding_key: String,
}

#[derive(Debug, Clone)]
struct ServingSemanticRow {
    collection: String,
    path: String,
    ordinal: i32,
    symbol_name: String,
    kind: String,
    line_start: i32,
    line_end: i32,
    vector_text: String,
}

#[derive(Debug, Clone, Default)]
struct PreparedSemanticRows {
    rows: Vec<ServingSemanticRow>,
    missing_embeddings: usize,
}

#[derive(Debug, Clone)]
struct EffectiveSnapshotSummary {
    total_files: usize,
    total_documents: usize,
    collections: Vec<BaselineCollectionRecord>,
}

fn load_embeddings_from_client(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
    embedding_keys: &[String],
    model_id: &str,
    dimension: usize,
) -> Result<HashMap<String, Vec<f32>>, SearchError> {
    if embedding_keys.is_empty() {
        return Ok(HashMap::new());
    }

    let query = format!(
        "SELECT embedding_key, embedding
         FROM {}
         WHERE model_id = $1 AND dimension = $2 AND embedding_key = ANY($3)",
        adapter.table("semantic_embeddings")
    );
    let rows = client.query(&query, &[&model_id, &(dimension as i32), &embedding_keys])?;

    let mut result = HashMap::new();
    for row in rows {
        let key: String = row.get("embedding_key");
        let blob: Vec<u8> = row.get("embedding");
        if blob.len() != dimension * 4 {
            continue;
        }
        let embedding = blob
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .collect::<Vec<_>>();
        result.insert(key, embedding);
    }
    Ok(result)
}

fn semantic_publication_setting(snapshot_id: &str, model_id: &str, dimension: usize) -> String {
    format!("{SEMANTIC_PUBLICATION_COMPLETE_PREFIX}{snapshot_id}:{model_id}:{dimension}")
}

fn semantic_publication_complete(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
    model_id: &str,
    dimension: usize,
) -> Result<bool, SearchError> {
    let setting = semantic_publication_setting(snapshot_id, model_id, dimension);
    Ok(client
        .query_opt(
            &format!(
                "SELECT 1 FROM {} WHERE setting = $1 AND value = 'complete' LIMIT 1",
                adapter.table(SCHEMA_METADATA_TABLE)
            ),
            &[&setting],
        )?
        .is_some())
}

fn clear_semantic_publication_complete(
    tx: &mut Transaction<'_>,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
    model_id: &str,
    dimension: usize,
) -> Result<(), SearchError> {
    let setting = semantic_publication_setting(snapshot_id, model_id, dimension);
    tx.execute(
        &format!("DELETE FROM {} WHERE setting = $1", adapter.table(SCHEMA_METADATA_TABLE)),
        &[&setting],
    )?;
    Ok(())
}

fn mark_semantic_publication_complete(
    tx: &mut Transaction<'_>,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
    model_id: &str,
    dimension: usize,
) -> Result<(), SearchError> {
    let setting = semantic_publication_setting(snapshot_id, model_id, dimension);
    let value = "complete".to_owned();
    tx.execute(
        &format!(
            "INSERT INTO {} (setting, value)
             VALUES ($1, $2)
             ON CONFLICT (setting) DO UPDATE SET value = EXCLUDED.value",
            adapter.table(SCHEMA_METADATA_TABLE)
        ),
        &[&setting, &value],
    )?;
    Ok(())
}

fn invalidate_semantic_publication_for_snapshot(
    tx: &mut Transaction<'_>,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
) -> Result<(), SearchError> {
    let settings_prefix = format!("{SEMANTIC_PUBLICATION_COMPLETE_PREFIX}{snapshot_id}:%");
    tx.execute(
        &format!("DELETE FROM {} WHERE setting LIKE $1", adapter.table(SCHEMA_METADATA_TABLE)),
        &[&settings_prefix],
    )?;
    if adapter.storage_table_exists(tx, "serving_semantic")? {
        tx.execute(
            &format!("DELETE FROM {} WHERE snapshot_id = $1", adapter.table("serving_semantic")),
            &[&snapshot_id],
        )?;
    }
    Ok(())
}

fn snapshot_parent_id(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
) -> Result<Option<String>, SearchError> {
    let query = format!(
        "SELECT parent_snapshot_id FROM {} WHERE id = $1 LIMIT 1",
        adapter.table("snapshots")
    );
    let Some(row) = client.query_opt(&query, &[&snapshot_id])? else {
        return Err(SearchError::ExternalBaseline(format!(
            "snapshot '{}' was not found",
            snapshot_id
        )));
    };
    Ok(row.get("parent_snapshot_id"))
}

fn semantic_publish_phase_count(strategy: &SemanticPublishStrategy) -> usize {
    match strategy {
        SemanticPublishStrategy::IncrementalFromParent { .. } => 3,
        SemanticPublishStrategy::CurrentSnapshotOnly | SemanticPublishStrategy::FullRebuild => 2,
    }
}

fn semantic_publish_strategy(
    parent_snapshot_id: Option<String>,
    parent_complete: bool,
) -> SemanticPublishStrategy {
    match parent_snapshot_id {
        Some(parent_snapshot_id) if parent_complete => {
            SemanticPublishStrategy::IncrementalFromParent { parent_snapshot_id }
        }
        Some(_) => SemanticPublishStrategy::FullRebuild,
        None => SemanticPublishStrategy::CurrentSnapshotOnly,
    }
}

fn semantic_publish_plan(
    parent_snapshot_id: Option<String>,
    parent_complete: bool,
    current_snapshot_files: &[VisibleSnapshotFile],
    deleted_paths: &[(String, String)],
) -> SemanticPublishPlan {
    SemanticPublishPlan {
        strategy: semantic_publish_strategy(parent_snapshot_id, parent_complete),
        changed_paths: current_snapshot_files
            .iter()
            .map(|file| (file.collection.clone(), file.path.clone()))
            .collect(),
        deleted_paths: deleted_paths.iter().cloned().collect(),
    }
}

fn load_snapshot_file_rows(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
) -> Result<Vec<VisibleSnapshotFile>, SearchError> {
    let query = format!(
        "SELECT collection, path, file_fingerprint, document_count, file_object_id
         FROM {}
         WHERE snapshot_id = $1
         ORDER BY collection, path",
        adapter.table("snapshot_files")
    );
    Ok(client
        .query(&query, &[&snapshot_id])?
        .into_iter()
        .map(|row| VisibleSnapshotFile {
            collection: row.get("collection"),
            root_id: CONFIGURATION_ROOT_ID.to_owned(),
            path: row.get("path"),
            file_fingerprint: row.get("file_fingerprint"),
            document_count: row.get::<_, i32>("document_count") as usize,
            file_object_id: row.get("file_object_id"),
        })
        .collect())
}

/// `['a', 'b']` as a PostgreSQL text array, for comparing against a catalog composition.
fn sql_text_array(columns: &[&str]) -> String {
    let items: Vec<String> = columns.iter().map(|column| format!("'{column}'")).collect();
    format!("ARRAY[{}]", items.join(", "))
}

/// `['a', 'b']` as the parenthesised column list of a key or index declaration.
fn sql_column_list(columns: &[&str]) -> String {
    format!("({})", columns.join(", "))
}

fn load_snapshot_deletion_keys(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
) -> Result<Vec<(String, String)>, SearchError> {
    let query = format!(
        "SELECT collection, path FROM {} WHERE snapshot_id = $1 ORDER BY collection, path",
        adapter.table("snapshot_deletions")
    );
    Ok(client
        .query(&query, &[&snapshot_id])?
        .into_iter()
        .map(|row| (row.get("collection"), row.get("path")))
        .collect())
}

fn prepare_semantic_rows_for_files(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
    visible_files: &[VisibleSnapshotFile],
    model_id: &str,
    dimension: usize,
) -> Result<PreparedSemanticRows, SearchError> {
    if visible_files.is_empty() {
        return Ok(PreparedSemanticRows::default());
    }

    let file_object_ids =
        visible_files.iter().map(|file| file.file_object_id.clone()).collect::<Vec<_>>();
    let file_meta = visible_files
        .iter()
        .map(|file| (file.file_object_id.clone(), (file.collection.clone(), file.path.clone())))
        .collect::<HashMap<_, _>>();
    let items_query = format!(
        "SELECT foi.file_object_id, foi.ordinal, foi.symbol_name, foi.kind,
                foi.line_start, foi.line_end, foi.graph_context, co.text
         FROM {} foi
         JOIN {} co ON co.content_hash = foi.content_hash
         WHERE foi.file_object_id = ANY($1)
         ORDER BY foi.file_object_id, foi.ordinal",
        adapter.table("file_object_items"),
        adapter.table("content_objects"),
    );
    let item_rows = client.query(&items_query, &[&file_object_ids])?;

    let mut pending_rows = Vec::with_capacity(item_rows.len());
    let mut embedding_keys = Vec::with_capacity(item_rows.len());
    let mut seen_keys = HashSet::with_capacity(item_rows.len());
    for row in item_rows {
        let file_object_id: String = row.get("file_object_id");
        let Some((collection, path)) = file_meta.get(&file_object_id) else {
            continue;
        };
        let graph_context: Option<String> = row.get("graph_context");
        let embedding_key = crate::document::semantic_key_from_parts(
            path,
            row.get("kind"),
            row.get("symbol_name"),
            graph_context.as_deref().unwrap_or(""),
            row.get("text"),
        );
        if seen_keys.insert(embedding_key.clone()) {
            embedding_keys.push(embedding_key.clone());
        }
        pending_rows.push(PendingSemanticRow {
            collection: collection.clone(),
            path: path.clone(),
            ordinal: row.get("ordinal"),
            symbol_name: row.get("symbol_name"),
            kind: row.get("kind"),
            line_start: row.get("line_start"),
            line_end: row.get("line_end"),
            embedding_key,
        });
    }

    let embeddings =
        load_embeddings_from_client(client, adapter, &embedding_keys, model_id, dimension)?;
    let mut prepared_rows = PreparedSemanticRows::default();
    prepared_rows.rows.reserve(pending_rows.len());
    for pending_row in pending_rows {
        if let Some(embedding) = embeddings.get(&pending_row.embedding_key) {
            prepared_rows.rows.push(ServingSemanticRow {
                collection: pending_row.collection,
                path: pending_row.path,
                ordinal: pending_row.ordinal,
                symbol_name: pending_row.symbol_name,
                kind: pending_row.kind,
                line_start: pending_row.line_start,
                line_end: pending_row.line_end,
                vector_text: format_pgvector_text(embedding),
            });
        } else {
            prepared_rows.missing_embeddings += 1;
        }
    }
    Ok(prepared_rows)
}

fn delete_serving_semantic_rows(
    tx: &mut Transaction<'_>,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
    model_id: &str,
    dimension: usize,
) -> Result<(), SearchError> {
    tx.execute(
        &format!(
            "DELETE FROM {} WHERE snapshot_id = $1 AND model_id = $2 AND dimension = $3",
            adapter.table("serving_semantic")
        ),
        &[&snapshot_id, &model_id, &(dimension as i32)],
    )?;
    Ok(())
}

fn copy_parent_serving_semantic_rows(
    tx: &mut Transaction<'_>,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
    parent_snapshot_id: &str,
    model_id: &str,
    dimension: usize,
) -> Result<usize, SearchError> {
    let query = format!(
        "INSERT INTO {} (
            snapshot_id, collection, path, ordinal, symbol_name, kind,
            line_start, line_end, model_id, dimension, embedding
         )
         SELECT $1, parent.collection, parent.path, parent.ordinal, parent.symbol_name, parent.kind,
                parent.line_start, parent.line_end, parent.model_id, parent.dimension, parent.embedding
         FROM {} parent
         WHERE parent.snapshot_id = $2
           AND parent.model_id = $3
           AND parent.dimension = $4
           AND NOT EXISTS (
               SELECT 1 FROM {} sf
               WHERE sf.snapshot_id = $1
                 AND sf.collection = parent.collection
                 AND sf.path = parent.path
           )
           AND NOT EXISTS (
               SELECT 1 FROM {} sd
               WHERE sd.snapshot_id = $1
                 AND sd.collection = parent.collection
                 AND sd.path = parent.path
           )",
        adapter.table("serving_semantic"),
        adapter.table("serving_semantic"),
        adapter.table("snapshot_files"),
        adapter.table("snapshot_deletions"),
    );
    Ok(tx.execute(&query, &[&snapshot_id, &parent_snapshot_id, &model_id, &(dimension as i32)])?
        as usize)
}

fn insert_serving_semantic_rows(
    tx: &mut Transaction<'_>,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
    model_id: &str,
    dimension: usize,
    rows: &[ServingSemanticRow],
) -> Result<usize, SearchError> {
    if rows.is_empty() {
        return Ok(0);
    }

    let dimension = dimension as i32;
    let mut inserted = 0usize;
    for batch in rows.chunks(SERVING_SEMANTIC_BATCH_SIZE) {
        let mut values = Vec::with_capacity(batch.len());
        let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> =
            Vec::with_capacity(batch.len() * 11);
        for (index, row) in batch.iter().enumerate() {
            let base = index * 11;
            values.push(format!(
                "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}::text::vector)",
                base + 1,
                base + 2,
                base + 3,
                base + 4,
                base + 5,
                base + 6,
                base + 7,
                base + 8,
                base + 9,
                base + 10,
                base + 11
            ));
            params.push(&snapshot_id);
            params.push(&row.collection);
            params.push(&row.path);
            params.push(&row.ordinal);
            params.push(&row.symbol_name);
            params.push(&row.kind);
            params.push(&row.line_start);
            params.push(&row.line_end);
            params.push(&model_id);
            params.push(&dimension);
            params.push(&row.vector_text);
        }
        let query = format!(
            "INSERT INTO {} (
                snapshot_id, collection, path, ordinal, symbol_name, kind,
                line_start, line_end, model_id, dimension, embedding
             ) VALUES {}",
            adapter.table("serving_semantic"),
            values.join(", ")
        );
        inserted += tx.execute(&query, &params)? as usize;
    }
    Ok(inserted)
}

fn format_pgvector_text(embedding: &[f32]) -> String {
    format!("[{}]", embedding.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(","))
}

/// Whether this schema can hold the identities the corpus carries.
///
/// A file's identity is the pair `(root_id, path)`, but these tables key it by
/// `(collection, path)` alone. So a corpus with roots does not merely lose a label on the
/// way in: [`group_documents_by_file`] would MERGE two files that share a relative path
/// across roots — the ordinary case, since an extension repeats the configuration's
/// layout — into one row carrying the chunks of both. Nothing downstream can tell that
/// apart from one large file.
///
/// The refusal therefore comes before any statement runs, and before the connection: a
/// half-merged snapshot is worse than no snapshot, and the caller can act on a named error.
fn ensure_the_schema_can_store_these_roots(
    documents: &[IndexedDocument],
) -> Result<(), SearchError> {
    let roots: BTreeSet<&str> = documents
        .iter()
        .map(|document| document.root_id.as_str())
        .filter(|root_id| *root_id != CONFIGURATION_ROOT_ID)
        .collect();
    if roots.is_empty() {
        return Ok(());
    }
    Err(SearchError::ExternalBaseline(format!(
        "refusing to publish a corpus whose files belong to source roots this baseline schema \
         cannot store: {}. Rows here are keyed by (collection, path) with no room for a root, \
         so files sharing a relative path across roots would be published merged into one. \
         Publish the configuration alone until the schema carries roots.",
        roots.into_iter().collect::<Vec<_>>().join(", ")
    )))
}

fn group_documents_by_file(documents: &[IndexedDocument]) -> Vec<PublishedFileGroup> {
    let mut grouped = BTreeMap::<VisibleFileKey, Vec<IndexedDocument>>::new();
    for document in documents {
        grouped
            .entry((document.collection.clone(), document.root_id.clone(), document.path.clone()))
            .or_default()
            .push(document.clone());
    }

    grouped
        .into_iter()
        .map(|((collection, root_id, path), mut documents)| {
            documents.sort_by(|lhs, rhs| {
                (
                    lhs.line_start,
                    lhs.line_end,
                    lhs.symbol_name.as_str(),
                    lhs.kind.as_str(),
                    lhs.content_hash.as_str(),
                )
                    .cmp(&(
                        rhs.line_start,
                        rhs.line_end,
                        rhs.symbol_name.as_str(),
                        rhs.kind.as_str(),
                        rhs.content_hash.as_str(),
                    ))
            });
            let file_fingerprint = fingerprint_file_documents(&documents);
            PublishedFileGroup { collection, root_id, path, file_fingerprint, documents }
        })
        .collect()
}

fn fingerprint_file_documents(documents: &[IndexedDocument]) -> String {
    let mut hasher = blake3::Hasher::new();
    for document in documents {
        hasher.update(document.collection.as_bytes());
        hasher.update(&[0]);
        hasher.update(document.path.as_bytes());
        hasher.update(&[0]);
        hasher.update(document.symbol_name.as_bytes());
        hasher.update(&[0]);
        hasher.update(document.kind.as_bytes());
        hasher.update(&document.line_start.to_le_bytes());
        hasher.update(&document.line_end.to_le_bytes());
        hasher.update(document.content_hash.as_bytes());
        hasher.update(&[0]);
        hasher.update(document.text.as_bytes());
        hasher.update(&[0]);
        // graph_context is folded into the embedding text (and thus the embedding
        // key), so a context-only change must invalidate file-object reuse — else
        // the reused row keeps stale context and its recomputed key no longer
        // matches the freshly stored embedding.
        match document.graph_context.as_deref() {
            Some(context) => {
                hasher.update(&[1]);
                hasher.update(context.as_bytes());
            }
            None => {
                hasher.update(&[0]);
            }
        }
        hasher.update(&[0xff]);
    }
    hasher.finalize().to_hex().to_string()
}

fn file_object_id_for(collection: &str, file_fingerprint: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(collection.as_bytes());
    hasher.update(&[0]);
    hasher.update(file_fingerprint.as_bytes());
    hasher.finalize().to_hex().to_string()
}

fn try_insert_file_object(
    tx: &mut Transaction<'_>,
    adapter: &PostgresBaselineAdapter,
    file_object_id: &str,
    file_group: &PublishedFileGroup,
) -> Result<bool, SearchError> {
    let insert_file_object = format!(
        "INSERT INTO {} (id, collection, file_fingerprint, document_count)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (id) DO NOTHING",
        adapter.table("file_objects")
    );
    let inserted = tx.execute(
        &insert_file_object,
        &[
            &file_object_id,
            &file_group.collection,
            &file_group.file_fingerprint,
            &(file_group.documents.len() as i32),
        ],
    )?;
    if inserted == 0 {
        return Ok(false);
    }

    let mut content_rows = Vec::with_capacity(file_group.documents.len());
    let mut item_rows = Vec::with_capacity(file_group.documents.len());
    for (ordinal, document) in file_group.documents.iter().enumerate() {
        content_rows.push(ContentObjectRow {
            content_hash: document.content_hash.clone(),
            text: document.text.clone(),
        });
        item_rows.push(FileObjectItemRow {
            file_object_id: file_object_id.to_owned(),
            ordinal: ordinal as i32,
            symbol_name: document.symbol_name.clone(),
            kind: document.kind.clone(),
            line_start: document.line_start as i32,
            line_end: document.line_end as i32,
            content_hash: document.content_hash.clone(),
            graph_context: document.graph_context.clone(),
        });
    }

    upsert_content_objects(tx, adapter, &content_rows)?;
    insert_file_object_items(tx, adapter, &item_rows)?;
    Ok(true)
}

fn unique_content_object_rows(
    rows: &[ContentObjectRow],
) -> Result<Vec<&ContentObjectRow>, SearchError> {
    let mut seen = HashMap::with_capacity(rows.len());
    let mut unique_rows = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some(existing) = seen.get(row.content_hash.as_str()) {
            if existing != &row.text {
                return Err(SearchError::ExternalBaseline(format!(
                    "content hash collision within publish batch: hash {} maps to multiple texts",
                    row.content_hash
                )));
            }
            continue;
        }

        seen.insert(row.content_hash.as_str(), row.text.as_str());
        unique_rows.push(row);
    }
    Ok(unique_rows)
}

fn upsert_content_objects(
    tx: &mut Transaction<'_>,
    adapter: &PostgresBaselineAdapter,
    rows: &[ContentObjectRow],
) -> Result<(), SearchError> {
    if rows.is_empty() {
        return Ok(());
    }

    let unique_rows = unique_content_object_rows(rows)?;
    for batch in unique_rows.chunks(CONTENT_OBJECT_BATCH_SIZE) {
        let mut values = Vec::with_capacity(batch.len());
        let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> =
            Vec::with_capacity(batch.len() * 2);
        for (index, row) in batch.iter().enumerate() {
            let base = index * 2;
            values.push(format!("(${}, ${})", base + 1, base + 2));
            params.push(&row.content_hash);
            params.push(&row.text);
        }
        let query = format!(
            "INSERT INTO {} (content_hash, text) VALUES {} \
             ON CONFLICT (content_hash) DO UPDATE SET text = EXCLUDED.text",
            adapter.table("content_objects"),
            values.join(", ")
        );
        tx.execute(&query, &params)?;
    }

    Ok(())
}

fn insert_file_object_items(
    tx: &mut Transaction<'_>,
    adapter: &PostgresBaselineAdapter,
    rows: &[FileObjectItemRow],
) -> Result<(), SearchError> {
    if rows.is_empty() {
        return Ok(());
    }

    for batch in rows.chunks(FILE_OBJECT_ITEM_BATCH_SIZE) {
        let mut values = Vec::with_capacity(batch.len());
        let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> =
            Vec::with_capacity(batch.len() * 8);
        for (index, row) in batch.iter().enumerate() {
            let base = index * 8;
            values.push(format!(
                "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                base + 1,
                base + 2,
                base + 3,
                base + 4,
                base + 5,
                base + 6,
                base + 7,
                base + 8
            ));
            params.push(&row.file_object_id);
            params.push(&row.ordinal);
            params.push(&row.symbol_name);
            params.push(&row.kind);
            params.push(&row.line_start);
            params.push(&row.line_end);
            params.push(&row.content_hash);
            params.push(&row.graph_context);
        }
        let query = format!(
            "INSERT INTO {} (
                file_object_id, ordinal, symbol_name, kind, line_start, line_end, content_hash,
                graph_context
             ) VALUES {}",
            adapter.table("file_object_items"),
            values.join(", ")
        );
        tx.execute(&query, &params)?;
    }

    Ok(())
}

fn insert_snapshot_file_rows(
    tx: &mut Transaction<'_>,
    adapter: &PostgresBaselineAdapter,
    rows: &[SnapshotFileRow],
) -> Result<(), SearchError> {
    if rows.is_empty() {
        return Ok(());
    }

    for batch in rows.chunks(SNAPSHOT_FILE_BATCH_SIZE) {
        let mut values = Vec::with_capacity(batch.len());
        let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> =
            Vec::with_capacity(batch.len() * 6);
        for (index, row) in batch.iter().enumerate() {
            let base = index * 6;
            values.push(format!(
                "(${}, ${}, ${}, ${}, ${}, ${})",
                base + 1,
                base + 2,
                base + 3,
                base + 4,
                base + 5,
                base + 6
            ));
            params.push(&row.snapshot_id);
            params.push(&row.collection);
            params.push(&row.path);
            params.push(&row.file_fingerprint);
            params.push(&row.document_count);
            params.push(&row.file_object_id);
        }
        let query = format!(
            "INSERT INTO {} (
                snapshot_id, collection, path, file_fingerprint, document_count, file_object_id
             ) VALUES {}",
            adapter.table("snapshot_files"),
            values.join(", ")
        );
        tx.execute(&query, &params)?;
    }

    Ok(())
}

fn insert_snapshot_deletion_rows(
    tx: &mut Transaction<'_>,
    adapter: &PostgresBaselineAdapter,
    rows: &[SnapshotDeletionRow],
) -> Result<(), SearchError> {
    if rows.is_empty() {
        return Ok(());
    }

    for batch in rows.chunks(SNAPSHOT_DELETION_BATCH_SIZE) {
        let mut values = Vec::with_capacity(batch.len());
        let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> =
            Vec::with_capacity(batch.len() * 3);
        for (index, row) in batch.iter().enumerate() {
            let base = index * 3;
            values.push(format!("(${}, ${}, ${})", base + 1, base + 2, base + 3));
            params.push(&row.snapshot_id);
            params.push(&row.collection);
            params.push(&row.path);
        }
        let query = format!(
            "INSERT INTO {} (snapshot_id, collection, path) VALUES {}",
            adapter.table("snapshot_deletions"),
            values.join(", ")
        );
        tx.execute(&query, &params)?;
    }

    Ok(())
}

fn replace_serving_lexical_rows(
    tx: &mut Transaction<'_>,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
    file_groups: &[PublishedFileGroup],
) -> Result<(), SearchError> {
    let delete = format!("DELETE FROM {} WHERE snapshot_id = $1", adapter.table("serving_lexical"));
    tx.execute(&delete, &[&snapshot_id])?;

    let mut rows = Vec::new();
    for file_group in file_groups {
        for (ordinal, document) in file_group.documents.iter().enumerate() {
            rows.push(ServingLexicalRow {
                snapshot_id: snapshot_id.to_owned(),
                collection: file_group.collection.clone(),
                path: file_group.path.clone(),
                ordinal: ordinal as i32,
                symbol_name: document.symbol_name.clone(),
                kind: document.kind.clone(),
                line_start: document.line_start as i32,
                line_end: document.line_end as i32,
                text: document.text.clone(),
            });
        }
    }

    for batch in rows.chunks(SERVING_LEXICAL_BATCH_SIZE) {
        let mut values = Vec::with_capacity(batch.len());
        let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> =
            Vec::with_capacity(batch.len() * 9);
        for (index, row) in batch.iter().enumerate() {
            let base = index * 9;
            values.push(format!(
                "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, to_tsvector('simple', ${}))",
                base + 1,
                base + 2,
                base + 3,
                base + 4,
                base + 5,
                base + 6,
                base + 7,
                base + 8,
                base + 9,
                base + 9
            ));
            params.push(&row.snapshot_id);
            params.push(&row.collection);
            params.push(&row.path);
            params.push(&row.ordinal);
            params.push(&row.symbol_name);
            params.push(&row.kind);
            params.push(&row.line_start);
            params.push(&row.line_end);
            params.push(&row.text);
        }
        let query = format!(
            "INSERT INTO {} (
                snapshot_id, collection, path, ordinal, symbol_name, kind,
                line_start, line_end, text, tsv
             ) VALUES {}",
            adapter.table("serving_lexical"),
            values.join(", ")
        );
        tx.execute(&query, &params)?;
    }
    Ok(())
}

fn log_snapshot_publish_phase_timings(
    snapshot: &Snapshot,
    timings: &SnapshotPublishPhaseTimings,
    stats: &SnapshotPublishStats,
    total_documents: usize,
) {
    tracing::info!(
        snapshot_id = %snapshot.id.0,
        corpus = %snapshot.corpus.as_str(),
        total_documents,
        grouping_ms = timings.grouping.as_millis() as u64,
        normalized_content_ms = timings.normalized_content.as_millis() as u64,
        lexical_rebuild_ms = timings.lexical_rebuild.as_millis() as u64,
        final_activation_ms = timings.final_activation.as_millis() as u64,
        written_files = stats.written_files,
        reused_files = stats.reused_files,
        deleted_files = stats.deleted_files,
        written_documents = stats.written_documents,
        reused_documents = stats.reused_documents,
        "postgres snapshot publish phase timings"
    );
}

fn log_semantic_publish_phase_timings(
    snapshot_id: &str,
    model_id: &str,
    dimension: usize,
    strategy: &SemanticPublishStrategy,
    timings: &SemanticPublishPhaseTimings,
    stats: &SemanticPublishLogStats,
) {
    let parent_snapshot_id = match strategy {
        SemanticPublishStrategy::IncrementalFromParent { parent_snapshot_id } => {
            parent_snapshot_id.as_str()
        }
        _ => "-",
    };
    tracing::info!(
        snapshot_id,
        model_id,
        dimension,
        strategy = strategy.label(),
        parent_snapshot_id,
        ancestry_materialization_ms = timings.ancestry_materialization.as_millis() as u64,
        parent_copy_ms = timings.parent_copy.as_millis() as u64,
        changed_recompute_ms = timings.changed_recompute.as_millis() as u64,
        final_sync_ms = timings.final_sync.as_millis() as u64,
        copied_rows = stats.copied_rows,
        inserted_rows = stats.inserted_rows,
        missing_embeddings = stats.missing_embeddings,
        changed_files = stats.changed_files,
        changed_rows = stats.changed_rows,
        deleted_paths = stats.deleted_paths,
        "postgres semantic publish phase timings"
    );
    if stats.missing_embeddings > 0 {
        tracing::warn!(
            snapshot_id,
            model_id,
            dimension,
            missing_embeddings = stats.missing_embeddings,
            strategy = strategy.label(),
            "semantic publish completed without readiness marker because embeddings were missing"
        );
    }
}

fn pool_connection_reason_code(message: &str) -> &'static str {
    let message = message.to_ascii_lowercase();
    if message.contains("password authentication failed")
        || message.contains("authentication failed")
        || message.contains("invalid password")
        || message.contains("saslauth")
    {
        "postgres_auth_failed"
    } else {
        "postgres_connect_failed"
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

fn snapshot_record_from_metadata_row(row: Row) -> BaselineSnapshotRecord {
    BaselineSnapshotRecord {
        snapshot_id: row.get("id"),
        corpus: row.get("corpus"),
        fingerprint: row.get("fingerprint"),
        parent_snapshot_id: row.get("parent_snapshot_id"),
        branch: row.get("branch"),
        commit: row.get("commit_sha"),
        created_at: row.get("created_at"),
        files: 0,
        documents: 0,
    }
}

fn file_object_record_from_row(row: Row) -> BaselineFileObjectRecord {
    BaselineFileObjectRecord {
        file_object_id: row.get("id"),
        collection: row.get("collection"),
        fingerprint: row.get("file_fingerprint"),
        documents: row.get::<_, i32>("document_count") as usize,
        snapshots: row.get::<_, i64>("snapshots") as usize,
    }
}

fn collect_active_embedding_keys(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
) -> Result<HashSet<String>, SearchError> {
    let shared_query = format!(
        "SELECT sf.path,
                foi.kind,
                foi.symbol_name,
                foi.graph_context,
                co.text
         FROM {} sf
         JOIN {} foi ON foi.file_object_id = sf.file_object_id
         JOIN {} co ON co.content_hash = foi.content_hash
         WHERE sf.file_object_id IS NOT NULL",
        adapter.table("snapshot_files"),
        adapter.table("file_object_items"),
        adapter.table("content_objects")
    );

    let mut active_keys = HashSet::new();
    for row in client.query(&shared_query, &[])? {
        active_keys.insert(semantic_key_for_semantic_row(row));
    }
    Ok(active_keys)
}

/// Counts visible files per collection with one server-side aggregate instead of
/// materializing every visible `snapshot_files` row over the wire (25k+ rows on large
/// corpora) just to count them in Rust. The aggregate mirrors the visibility rules of
/// `materialize_visible_snapshot_file_map`: the first occurrence of a `(collection, path)`
/// key in child-first ancestry order wins, and on a position tie a deletion shadows a file
/// published by the same snapshot (`is_deletion DESC`).
fn effective_snapshot_summary(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
) -> Result<EffectiveSnapshotSummary, SearchError> {
    let ancestry = snapshot_ancestry_ids(client, adapter, snapshot_id)?;
    let query = format!(
        "WITH entries AS (
             SELECT collection, path, document_count, FALSE AS is_deletion,
                    array_position($1::TEXT[], snapshot_id) AS ancestry_position
             FROM {files}
             WHERE snapshot_id = ANY($1)
             UNION ALL
             SELECT collection, path, 0 AS document_count, TRUE AS is_deletion,
                    array_position($1::TEXT[], snapshot_id) AS ancestry_position
             FROM {deletions}
             WHERE snapshot_id = ANY($1)
         ),
         winners AS (
             SELECT DISTINCT ON (collection, path) collection, document_count, is_deletion
             FROM entries
             ORDER BY collection, path, ancestry_position, is_deletion DESC
         )
         SELECT collection,
                COUNT(*)::BIGINT AS files,
                COALESCE(SUM(document_count), 0)::BIGINT AS documents
         FROM winners
         WHERE NOT is_deletion
         GROUP BY collection",
        files = adapter.table("snapshot_files"),
        deletions = adapter.table("snapshot_deletions"),
    );

    let mut total_files = 0usize;
    let mut total_documents = 0usize;
    let mut collections = Vec::new();
    for row in client.query(&query, &[&ancestry])? {
        let files = row.get::<_, i64>("files").max(0) as usize;
        let documents = row.get::<_, i64>("documents").max(0) as usize;
        total_files += files;
        total_documents += documents;
        collections.push(BaselineCollectionRecord {
            collection: row.get("collection"),
            files,
            documents,
        });
    }
    // Byte-order sort in Rust, not SQL ORDER BY: the materialized path grouped through a
    // BTreeMap and SQL collation order can disagree with it on non-ASCII names.
    collections.sort_by(|left, right| left.collection.cmp(&right.collection));

    Ok(EffectiveSnapshotSummary { total_files, total_documents, collections })
}

/// Per-seed effective (visible) file/document totals for a batch of snapshots, computed in one
/// round-trip instead of walking each snapshot's ancestry and summarising it separately (which
/// `effective_snapshot_summary` does per call — `list_snapshots` over N snapshots of depth D would
/// otherwise issue `1 + N*(D+1)` queries). A `RECURSIVE` CTE expands every seed's ancestry keyed by
/// the seed it belongs to, so the child-first visibility rule (`DISTINCT ON (seed, collection, path)`
/// by ancestry depth, a same-position deletion shadowing its file) matches the single-snapshot path.
/// Only totals are returned: `list_snapshots` does not use the per-collection breakdown.
///
/// The delicate error semantics of `snapshot_ancestry_ids` are preserved: a parent that points at a
/// missing snapshot and a parent chain that cycles are both surfaced as the same errors, emitted as
/// discriminated rows in the same result so a corrupt chain still fails the listing instead of
/// silently under-counting. Seeds with no visible files simply have no aggregate row (totals 0/0).
fn effective_snapshot_totals_batch(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
    seed_ids: &[String],
) -> Result<HashMap<String, (usize, usize)>, SearchError> {
    if seed_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let query = format!(
        "WITH RECURSIVE chain(seed_id, snapshot_id, next_parent, depth, missing) AS (
             SELECT id, id, parent_snapshot_id, 0, FALSE
             FROM {snapshots}
             WHERE id = ANY($1)
             UNION ALL
             SELECT c.seed_id, c.next_parent, s.parent_snapshot_id, c.depth + 1, s.id IS NULL
             FROM chain c
             LEFT JOIN {snapshots} s ON s.id = c.next_parent
             WHERE c.next_parent IS NOT NULL
         ) CYCLE snapshot_id SET is_cycle USING cycle_path,
         entries AS (
             SELECT c.seed_id, f.collection, f.path, f.document_count,
                    FALSE AS is_deletion, c.depth AS ancestry_position
             FROM chain c
             JOIN {files} f ON f.snapshot_id = c.snapshot_id
             WHERE NOT c.is_cycle AND NOT c.missing
             UNION ALL
             SELECT c.seed_id, d.collection, d.path, 0,
                    TRUE AS is_deletion, c.depth AS ancestry_position
             FROM chain c
             JOIN {deletions} d ON d.snapshot_id = c.snapshot_id
             WHERE NOT c.is_cycle AND NOT c.missing
         ),
         winners AS (
             SELECT DISTINCT ON (seed_id, collection, path) seed_id, document_count, is_deletion
             FROM entries
             ORDER BY seed_id, collection, path, ancestry_position, is_deletion DESC
         ),
         totals AS (
             SELECT seed_id,
                    COUNT(*)::BIGINT AS files,
                    COALESCE(SUM(document_count), 0)::BIGINT AS documents
             FROM winners
             WHERE NOT is_deletion
             GROUP BY seed_id
         ),
         failures AS (
             SELECT DISTINCT seed_id, snapshot_id AS offending, 'cycle' AS reason
             FROM chain WHERE is_cycle
             UNION
             SELECT DISTINCT seed_id, snapshot_id AS offending, 'missing' AS reason
             FROM chain WHERE missing
         )
         SELECT 'total'::TEXT AS row_kind, seed_id, files, documents,
                NULL::TEXT AS offending, NULL::TEXT AS reason
         FROM totals
         UNION ALL
         SELECT 'failure'::TEXT AS row_kind, seed_id, NULL::BIGINT, NULL::BIGINT, offending, reason
         FROM failures",
        snapshots = adapter.table("snapshots"),
        files = adapter.table("snapshot_files"),
        deletions = adapter.table("snapshot_deletions"),
    );

    let rows = client.query(&query, &[&seed_ids])?;
    let mut totals = HashMap::with_capacity(seed_ids.len());
    let mut failures: HashMap<String, (String, String)> = HashMap::new();
    for row in rows {
        let row_kind: String = row.get("row_kind");
        if row_kind == "failure" {
            failures.insert(row.get("seed_id"), (row.get("offending"), row.get("reason")));
            continue;
        }
        let files = row.get::<_, i64>("files").max(0) as usize;
        let documents = row.get::<_, i64>("documents").max(0) as usize;
        totals.insert(row.get::<_, String>("seed_id"), (files, documents));
    }

    // A corrupt ancestry chain fails the whole listing, as the per-snapshot path did. Report the
    // first seed in listing order that is corrupt so the error is deterministic when several seeds
    // are corrupt at once (the batch result set has no inherent order).
    for seed_id in seed_ids {
        if let Some((offending, reason)) = failures.get(seed_id) {
            return Err(SearchError::ExternalBaseline(match reason.as_str() {
                "cycle" => format!("snapshot parent chain contains cycle at '{offending}'"),
                _ => format!("snapshot '{offending}' was not found"),
            }));
        }
    }
    Ok(totals)
}

fn materialize_visible_snapshot_file_map(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
) -> Result<BTreeMap<VisibleFileKey, VisibleSnapshotFile>, SearchError> {
    let ancestry = snapshot_ancestry_ids(client, adapter, snapshot_id)?;
    let file_query = format!(
        "SELECT snapshot_id, collection, path, file_fingerprint, document_count, file_object_id
         FROM {}
         WHERE snapshot_id = ANY($1)",
        adapter.table("snapshot_files")
    );
    let deletion_query = format!(
        "SELECT snapshot_id, collection, path
         FROM {}
         WHERE snapshot_id = ANY($1)",
        adapter.table("snapshot_deletions")
    );

    let mut files_by_snapshot = HashMap::<String, Vec<VisibleSnapshotFile>>::new();
    for row in client.query(&file_query, &[&ancestry])? {
        let snapshot_id: String = row.get("snapshot_id");
        files_by_snapshot.entry(snapshot_id).or_default().push(VisibleSnapshotFile {
            collection: row.get("collection"),
            root_id: CONFIGURATION_ROOT_ID.to_owned(),
            path: row.get("path"),
            file_fingerprint: row.get("file_fingerprint"),
            document_count: row.get::<_, i32>("document_count") as usize,
            file_object_id: row.get("file_object_id"),
        });
    }
    let mut deletions_by_snapshot = HashMap::<String, Vec<VisibleFileKey>>::new();
    for row in client.query(&deletion_query, &[&ancestry])? {
        let snapshot_id: String = row.get("snapshot_id");
        deletions_by_snapshot.entry(snapshot_id).or_default().push((
            row.get("collection"),
            CONFIGURATION_ROOT_ID.to_owned(),
            row.get("path"),
        ));
    }

    Ok(fold_visible_files(&ancestry, &mut files_by_snapshot, &mut deletions_by_snapshot))
}

/// Which file of an ancestry is the one a snapshot serves, walking child-first.
///
/// A pure function over rows the caller already read, because this is where the whole
/// visibility rule lives: the first occurrence of a key wins, and a deletion recorded closer
/// to the child shadows every ancestor's file under that same key. Reachable only through a
/// live database in its caller, it would be observable nowhere else — and a rule this central
/// must be answerable without one.
fn fold_visible_files(
    ancestry: &[String],
    files_by_snapshot: &mut HashMap<String, Vec<VisibleSnapshotFile>>,
    deletions_by_snapshot: &mut HashMap<String, Vec<VisibleFileKey>>,
) -> BTreeMap<VisibleFileKey, VisibleSnapshotFile> {
    let mut seen_paths = HashSet::<VisibleFileKey>::new();
    let mut visible_files = BTreeMap::<VisibleFileKey, VisibleSnapshotFile>::new();
    for snapshot_id in ancestry {
        if let Some(deletions) = deletions_by_snapshot.remove(snapshot_id) {
            for key in deletions {
                seen_paths.insert(key);
            }
        }
        if let Some(files) = files_by_snapshot.remove(snapshot_id) {
            for file in files {
                let key = file.key();
                if seen_paths.insert(key.clone()) {
                    visible_files.insert(key, file);
                }
            }
        }
    }
    visible_files
}

fn materialize_visible_snapshot_files(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
) -> Result<Vec<VisibleSnapshotFile>, SearchError> {
    Ok(materialize_visible_snapshot_file_map(client, adapter, snapshot_id)?.into_values().collect())
}

fn snapshot_ancestry_ids(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
) -> Result<Vec<String>, SearchError> {
    let query = format!(
        "SELECT id, parent_snapshot_id
         FROM {}
         WHERE id = $1
         LIMIT 1",
        adapter.table("snapshots")
    );

    let mut ancestry = Vec::new();
    let mut seen = HashSet::new();
    let mut current = Some(snapshot_id.to_owned());
    while let Some(snapshot_id) = current.take() {
        if !seen.insert(snapshot_id.clone()) {
            return Err(SearchError::ExternalBaseline(format!(
                "snapshot parent chain contains cycle at '{}'",
                snapshot_id
            )));
        }
        let Some(row) = client.query_opt(&query, &[&snapshot_id])? else {
            return Err(SearchError::ExternalBaseline(format!(
                "snapshot '{}' was not found",
                snapshot_id
            )));
        };
        ancestry.push(row.get::<_, String>("id"));
        current = row.get("parent_snapshot_id");
    }

    Ok(ancestry)
}

fn semantic_key_for_semantic_row(row: Row) -> String {
    let path: String = row.get("path");
    let kind: String = row.get("kind");
    let symbol_name: String = row.get("symbol_name");
    let graph_context: Option<String> = row.get("graph_context");
    let text: String = row.get("text");
    crate::document::semantic_key_from_parts(
        &path,
        &kind,
        &symbol_name,
        graph_context.as_deref().unwrap_or(""),
        &text,
    )
}

fn query_string_column(
    client: &mut impl GenericClient,
    query: &str,
    params: &[&(dyn postgres::types::ToSql + Sync)],
) -> Result<Vec<String>, SearchError> {
    Ok(client.query(query, params)?.into_iter().map(|row| row.get(0)).collect())
}

#[cfg(test)]
mod tests {
    use super::{
        effective_snapshot_totals_batch, file_object_id_for, fingerprint_file_documents,
        fold_visible_files, group_documents_by_file, materialize_visible_snapshot_files,
        semantic_publish_phase_count, semantic_publish_plan, semantic_publish_strategy,
        unique_content_object_rows, ContentObjectRow, EffectiveSnapshotSummary,
        PostgresBaselineAdapter, SemanticPublishPlan, SemanticPublishStrategy, VisibleFileKey,
        VisibleSnapshotFile, CONFIGURATION_ROOT_ID,
    };
    use crate::domain::{CorpusId, ExternalBaselineConfig, Snapshot, SnapshotPublishMetadata};
    use crate::external_baseline::BaselineCollectionRecord;
    use crate::ports::{
        BaselineLexicalSearch, BaselineSemanticSearch, SnapshotCatalog, SnapshotPublisher,
    };
    use crate::{BaselineRef, IndexedDocument};
    use std::collections::HashMap;

    /// Two facts hold this file together, and neither is visible from where the other lives.
    ///
    /// The garbage collector rebuilds the set of live embedding keys from `snapshot_files.path`
    /// with no root at all (`collect_active_embedding_keys`), and that is CORRECT only because
    /// the embedding key ignores the root by construction. Fold the root into the key recipe
    /// and the collector starts calling live vectors orphans — silently, and the payment is
    /// their deletion.
    ///
    /// So the recipe is pinned here too, next to the storage that depends on it, and not only
    /// in `document.rs` where it is written.
    #[test]
    fn the_serving_side_key_recipe_ignores_the_root() {
        let source = include_str!("postgres.rs");
        let production = source.split("\n#[cfg(test)]\nmod tests {").next().unwrap_or(source);
        let recipe = production
            .split_once("fn semantic_key_for_semantic_row")
            .expect("the serving-side key recomputation moved; this gate scans what it can find")
            .1;
        let recipe = recipe.split_once("\nfn ").map(|(body, _)| body).unwrap_or(recipe);

        assert!(
            !recipe.contains("root_id"),
            "recomputing a stored row's embedding key must not read the root: the collector \
             that decides which vectors are live cannot see it, and would delete what it \
             cannot match"
        );
    }

    /// A file is a `(collection, root_id, path)`, so the same relative path under two roots is
    /// two files with two fingerprints — not one file whose chunks got merged.
    #[test]
    fn two_roots_sharing_a_relative_path_are_two_published_files() {
        const PATH: &str = "CommonModules/Общий/Ext/Module.bsl";
        let configuration =
            indexed_document("code", PATH, "Общий", 1, "hash-конфигурации", "текст");
        let extension = IndexedDocument {
            root_id: "Расш".to_owned(),
            content_hash: "hash-расширения".to_owned(),
            ..configuration.clone()
        };

        let groups = group_documents_by_file(&[configuration, extension]);

        assert_eq!(
            groups.iter().map(|group| group.root_id.as_str()).collect::<Vec<_>>(),
            vec![CONFIGURATION_ROOT_ID, "Расш"],
            "merging them would publish one row holding the chunks of two different files"
        );
    }

    fn deletion_key(root_id: &str, path: &str) -> VisibleFileKey {
        visible_file(root_id, path).key()
    }

    fn visible_file(root_id: &str, path: &str) -> VisibleSnapshotFile {
        VisibleSnapshotFile {
            collection: "code".to_owned(),
            root_id: root_id.to_owned(),
            path: path.to_owned(),
            file_fingerprint: format!("fp-{root_id}-{path}"),
            document_count: 1,
            file_object_id: format!("obj-{root_id}-{path}"),
        }
    }

    /// The whole visibility rule in one place: a deletion recorded by a child shadows the
    /// ancestor's file under the SAME key — and an extension's file is not the
    /// configuration's, however identical their relative paths look.
    #[test]
    fn a_deletion_shadows_only_its_own_root() {
        const PATH: &str = "CommonModules/Общий/Ext/Module.bsl";
        let ancestry = vec!["child".to_owned(), "parent".to_owned()];
        let mut files = HashMap::from([(
            "parent".to_owned(),
            vec![visible_file(CONFIGURATION_ROOT_ID, PATH), visible_file("Расш", PATH)],
        )]);
        let mut deletions = HashMap::from([("child".to_owned(), vec![deletion_key("Расш", PATH)])]);

        let visible = fold_visible_files(&ancestry, &mut files, &mut deletions);

        assert_eq!(
            visible.values().map(|file| file.root_id.as_str()).collect::<Vec<_>>(),
            vec![CONFIGURATION_ROOT_ID],
            "deleting the extension's file must leave the configuration's alone: they are two \
             different files that merely share a relative path"
        );
    }

    /// The positive control for the rule above: without it, an implementation that shadows
    /// nothing at all passes just as well.
    #[test]
    fn a_deletion_shadows_the_file_of_its_own_root() {
        const PATH: &str = "CommonModules/Общий/Ext/Module.bsl";
        let ancestry = vec!["child".to_owned(), "parent".to_owned()];
        let mut files = HashMap::from([(
            "parent".to_owned(),
            vec![visible_file(CONFIGURATION_ROOT_ID, PATH), visible_file("Расш", PATH)],
        )]);
        let mut deletions =
            HashMap::from([("child".to_owned(), vec![deletion_key(CONFIGURATION_ROOT_ID, PATH)])]);

        let visible = fold_visible_files(&ancestry, &mut files, &mut deletions);

        assert_eq!(
            visible.values().map(|file| file.root_id.as_str()).collect::<Vec<_>>(),
            vec!["Расш"],
            "a deletion must still shadow the file it names"
        );
    }

    /// The other half of the rule, and the half no deletion takes part in: with the same key
    /// published twice, the row closer to the child wins. An ancestor's row is an older
    /// publication of that same file, so serving it would hand out a fingerprint and a file
    /// object the corpus has already replaced.
    #[test]
    fn the_file_closer_to_the_child_wins_over_its_ancestors() {
        const PATH: &str = "CommonModules/Общий/Ext/Module.bsl";
        let ancestry = vec!["child".to_owned(), "parent".to_owned()];
        let republished = VisibleSnapshotFile {
            file_fingerprint: "fp-republished".to_owned(),
            ..visible_file(CONFIGURATION_ROOT_ID, PATH)
        };
        let mut files = HashMap::from([
            ("child".to_owned(), vec![republished]),
            ("parent".to_owned(), vec![visible_file(CONFIGURATION_ROOT_ID, PATH)]),
        ]);
        let mut deletions = HashMap::new();

        let visible = fold_visible_files(&ancestry, &mut files, &mut deletions);

        assert_eq!(
            visible.values().map(|file| file.file_fingerprint.as_str()).collect::<Vec<_>>(),
            vec!["fp-republished"],
            "the ancestor's row must not overwrite the republished file: ancestry is walked \
             child-first precisely so the newest publication of a key is the visible one"
        );
    }

    /// Every mandatory carrier of file identity keys it by the root, and none of them still
    /// declares the two-part key.
    ///
    /// Looped over the carriers rather than written for one of them: the three are edited by
    /// hand in three separate statements, and a node that keys two of three leaves the third
    /// silently merging an extension's file into the configuration's.
    #[test]
    fn every_mandatory_carrier_keys_the_file_by_its_root() {
        const CARRIERS: [(&str, &str); 3] = [
            ("snapshot_files", "PRIMARY KEY (snapshot_id, collection, root_id, path)"),
            ("snapshot_deletions", "PRIMARY KEY (snapshot_id, collection, root_id, path)"),
            ("serving_lexical", "PRIMARY KEY (snapshot_id, collection, root_id, path, ordinal)"),
        ];
        let adapter =
            PostgresBaselineAdapter::new(ExternalBaselineConfig::postgres("postgres://example"))
                .unwrap();
        let statements = adapter.ensure_schema_statements();

        for (table, expected_key) in CARRIERS {
            let qualified = adapter.table(table);
            let owned: Vec<&String> =
                statements.iter().filter(|statement| statement.contains(&qualified)).collect();
            assert!(!owned.is_empty(), "no statement mentions {qualified}");

            assert!(
                owned.iter().any(|statement| statement.contains(expected_key)),
                "{table} must declare {expected_key}"
            );
            assert!(
                owned
                    .iter()
                    .any(|statement| statement.contains("ADD COLUMN IF NOT EXISTS root_id")),
                "{table} must gain root_id on schemas that predate roots"
            );
            assert!(
                !owned
                    .iter()
                    .any(|statement| statement
                        .contains("PRIMARY KEY (snapshot_id, collection, path")),
                "{table} still declares the rootless key, so two roots collapse into one row"
            );
        }
    }

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
        assert!(error.to_string().contains("connection"));
    }

    /// This schema keys a file by `(collection, path)` alone, so a corpus that also carries
    /// roots has no place to put them: two files sharing a relative path across roots would
    /// merge into one group holding the chunks of both. Refusing is the honest answer, and it
    /// has to come before anything is written — a partial merge is worse than no publish.
    ///
    /// Observed offline, against an address nothing listens on: an implementation that wrote
    /// first and refused afterwards would have to connect first, and there is no connection to
    /// be had. The positive control is the same call with a root-free corpus, which must reach
    /// the network and fail there — without it the assertion passes under any unconditional
    /// error.
    #[test]
    fn a_corpus_with_roots_is_refused_before_the_connection_is_made() {
        let adapter = PostgresBaselineAdapter::new(ExternalBaselineConfig::postgres(
            "postgres://127.0.0.1:1",
        ))
        .unwrap();
        let snapshot = Snapshot::new("snap-1", CorpusId::WorkspaceCode);
        let metadata = SnapshotPublishMetadata::default();
        let configuration =
            indexed_document("code", "CommonModules/Общий/Ext/Module.bsl", "Общий", 1, "h", "t");
        let extension =
            IndexedDocument { root_id: "Расширение".to_owned(), ..configuration.clone() };

        let refused = adapter
            .publish_snapshot(&snapshot, &metadata, std::slice::from_ref(&extension))
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("Расширение") && !refused.contains("connection"),
            "the corpus must be refused for its roots, and refused before the adapter goes \
             anywhere near the database: {refused}"
        );

        let reached_the_network = adapter
            .publish_snapshot(&snapshot, &metadata, std::slice::from_ref(&configuration))
            .unwrap_err()
            .to_string();
        assert!(
            reached_the_network.contains("connection"),
            "positive control: a root-free corpus must get past the refusal and fail on the \
             unreachable address, or the assertion above holds under any error at all: \
             {reached_the_network}"
        );
    }

    #[test]
    fn connection_errors_surface_from_storage_migration() {
        let adapter = PostgresBaselineAdapter::new(ExternalBaselineConfig::postgres(
            "postgres://127.0.0.1:1",
        ))
        .unwrap();

        let error = adapter.migrate_storage().unwrap_err();
        assert!(error.to_string().contains("connection"));
    }

    #[test]
    fn connection_errors_surface_from_baseline_lexical_search() {
        let adapter = PostgresBaselineAdapter::new(ExternalBaselineConfig::postgres(
            "postgres://127.0.0.1:1",
        ))
        .unwrap();

        let error = adapter.lexical_search_baseline("snapshot-1", "Найти", None, 10).unwrap_err();
        assert!(error.to_string().contains("connection"));
    }

    #[test]
    fn lexical_search_returns_empty_for_blank_query() {
        let adapter = PostgresBaselineAdapter::new(ExternalBaselineConfig::postgres(
            "postgres://127.0.0.1:1",
        ))
        .unwrap();

        let result = adapter.lexical_search_baseline("snapshot-1", "", None, 10).unwrap();
        assert!(result.is_empty());

        let result = adapter.lexical_search_baseline("snapshot-1", "   ", None, 10).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn semantic_search_returns_empty_for_empty_embedding() {
        let adapter = PostgresBaselineAdapter::new(ExternalBaselineConfig::postgres(
            "postgres://127.0.0.1:1",
        ))
        .unwrap();

        let result =
            adapter.semantic_search_baseline("snapshot-1", &[], "model-1", 1024, None, 10).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn connection_errors_surface_from_semantic_search() {
        let adapter = PostgresBaselineAdapter::new(ExternalBaselineConfig::postgres(
            "postgres://127.0.0.1:1",
        ))
        .unwrap();

        let error = adapter
            .semantic_search_baseline("snapshot-1", &[0.1, 0.2, 0.3], "model-1", 3, None, 10)
            .unwrap_err();
        assert!(error.to_string().contains("connection"));
    }

    #[test]
    fn semantic_publish_phase_count_matches_strategy() {
        assert_eq!(semantic_publish_phase_count(&SemanticPublishStrategy::CurrentSnapshotOnly), 2);
        assert_eq!(semantic_publish_phase_count(&SemanticPublishStrategy::FullRebuild), 2);
        assert_eq!(
            semantic_publish_phase_count(&SemanticPublishStrategy::IncrementalFromParent {
                parent_snapshot_id: "parent-1".to_owned()
            }),
            3
        );
    }

    #[test]
    fn semantic_publish_strategy_exposes_parent_snapshot_id_only_for_incremental_reuse() {
        assert_eq!(SemanticPublishStrategy::CurrentSnapshotOnly.parent_snapshot_id(), None);
        assert_eq!(SemanticPublishStrategy::FullRebuild.parent_snapshot_id(), None);
        assert_eq!(
            SemanticPublishStrategy::IncrementalFromParent {
                parent_snapshot_id: "parent-1".to_owned()
            }
            .parent_snapshot_id(),
            Some("parent-1")
        );
    }

    #[test]
    fn semantic_publish_requires_completion_marker_for_parent_reuse() {
        assert_eq!(
            semantic_publish_strategy(Some("parent-1".to_owned()), false),
            SemanticPublishStrategy::FullRebuild
        );
        assert_eq!(
            semantic_publish_strategy(Some("parent-1".to_owned()), true),
            SemanticPublishStrategy::IncrementalFromParent {
                parent_snapshot_id: "parent-1".to_owned()
            }
        );
    }

    #[test]
    fn semantic_publish_plan_tracks_changed_and_deleted_paths_for_incremental_reuse() {
        let plan = semantic_publish_plan(
            Some("parent-1".to_owned()),
            true,
            &[
                visible_snapshot_file("code", "src/New.bsl"),
                visible_snapshot_file("code", "src/Renamed.bsl"),
            ],
            &[("code".to_owned(), "src/Old.bsl".to_owned())],
        );

        assert_eq!(
            plan,
            SemanticPublishPlan {
                strategy: SemanticPublishStrategy::IncrementalFromParent {
                    parent_snapshot_id: "parent-1".to_owned()
                },
                changed_paths: [
                    ("code".to_owned(), "src/New.bsl".to_owned()),
                    ("code".to_owned(), "src/Renamed.bsl".to_owned())
                ]
                .into_iter()
                .collect(),
                deleted_paths: [("code".to_owned(), "src/Old.bsl".to_owned())]
                    .into_iter()
                    .collect(),
            }
        );
    }

    #[test]
    fn semantic_publish_plan_falls_back_to_full_rebuild_without_parent_marker() {
        let plan = semantic_publish_plan(
            Some("parent-1".to_owned()),
            false,
            &[visible_snapshot_file("code", "src/Changed.bsl")],
            &[("code".to_owned(), "src/Deleted.bsl".to_owned())],
        );

        assert_eq!(plan.strategy, SemanticPublishStrategy::FullRebuild);
        assert_eq!(plan.changed_paths.len(), 1);
        assert_eq!(plan.deleted_paths.len(), 1);
    }

    #[test]
    fn semantic_publish_uses_current_snapshot_only_without_parent() {
        assert_eq!(
            semantic_publish_strategy(None, false),
            SemanticPublishStrategy::CurrentSnapshotOnly
        );
    }

    #[test]
    fn unique_content_object_rows_deduplicates_repeated_hashes() {
        let rows = vec![
            ContentObjectRow { content_hash: "hash-a".to_owned(), text: "text-a".to_owned() },
            ContentObjectRow { content_hash: "hash-a".to_owned(), text: "text-a".to_owned() },
            ContentObjectRow { content_hash: "hash-b".to_owned(), text: "text-b".to_owned() },
        ];

        let unique_rows = unique_content_object_rows(&rows).unwrap();

        assert_eq!(unique_rows.len(), 2);
        assert_eq!(unique_rows[0].content_hash, "hash-a");
        assert_eq!(unique_rows[1].content_hash, "hash-b");
    }

    #[test]
    fn unique_content_object_rows_rejects_conflicting_text_for_same_hash() {
        let rows = vec![
            ContentObjectRow { content_hash: "hash-a".to_owned(), text: "text-a".to_owned() },
            ContentObjectRow { content_hash: "hash-a".to_owned(), text: "text-b".to_owned() },
        ];

        let error = unique_content_object_rows(&rows).unwrap_err();

        assert!(error.to_string().contains("content hash collision within publish batch"));
    }

    #[test]
    fn group_documents_by_file_merges_same_path_and_sorts_chunks() {
        let documents = vec![
            indexed_document("code", "src/A.bsl", "B", 20, "hash-b", "text-b"),
            indexed_document("code", "src/A.bsl", "A", 10, "hash-a", "text-a"),
            indexed_document("code", "src/B.bsl", "C", 5, "hash-c", "text-c"),
        ];

        let groups = group_documents_by_file(&documents);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].collection, "code");
        assert_eq!(groups[0].path, "src/A.bsl");
        assert_eq!(groups[0].documents.len(), 2);
        assert_eq!(groups[0].documents[0].symbol_name, "A");
        assert_eq!(groups[0].documents[1].symbol_name, "B");
    }

    #[test]
    fn file_fingerprint_changes_when_chunk_content_changes() {
        let documents = vec![indexed_document("code", "src/A.bsl", "A", 10, "hash-a", "text-a")];
        let changed =
            vec![indexed_document("code", "src/A.bsl", "A", 10, "hash-a", "changed-text")];

        assert_ne!(fingerprint_file_documents(&documents), fingerprint_file_documents(&changed));
    }

    /// The manifest carries the fingerprint computed here, and the working-tree
    /// side recomputes its own to decide whether a file differs from the
    /// baseline. Disagreeing recipes make every file read as locally changed,
    /// so the whole corpus lives as an overlay delta.
    ///
    /// The expected value is pinned from THIS side. Published manifests were
    /// computed with this recipe, so it is the one that cannot move; a check
    /// that merely compared the two live functions would be satisfied just as
    /// well by changing the publisher, which would leave every published
    /// manifest mismatched and this check green.
    #[test]
    fn the_working_tree_recipe_reproduces_the_published_fingerprint() {
        const PUBLISHED: &str = "4e5ca8d990e5af2764397a6700dc0e46f66141881870dcf946a0829a334e3804";
        let rel_path = "CommonModules/Модуль/Ext/Module.bsl";
        let content = "Процедура Первая() Экспорт\nКонецПроцедуры\n\n\
                       Функция Вторая() Экспорт\n\tВозврат 1;\nКонецФункции\n";
        // Built through the shared chunk→document builder the overlay uses, so
        // the two recipes are fed the very same documents and only the recipes
        // themselves are under test.
        let documents: Vec<IndexedDocument> = code_chunk::Chunker::chunk(content)
            .iter()
            .map(|chunk| {
                crate::document::indexed_document_for_chunk(
                    &crate::FileKey::configuration(rel_path),
                    chunk,
                    None,
                )
            })
            .collect();
        assert!(documents.len() > 1, "the fixture must exercise more than one chunk");

        assert_eq!(
            fingerprint_file_documents(&documents),
            PUBLISHED,
            "the published recipe must not move: manifests already carry it"
        );
        assert_eq!(
            crate::workspace_overlay::fingerprint_overlay_documents(&documents, rel_path),
            PUBLISHED,
            "the overlay's document recipe must reproduce the published fingerprint"
        );
        assert_eq!(
            crate::workspace_overlay::fingerprint_content(content, rel_path),
            PUBLISHED,
            "the from-disk recipe must reproduce the published fingerprint"
        );
    }

    /// The behaviour the byte-level pin above exists for: a working tree that
    /// *is* the published snapshot must report nothing local at all.
    ///
    /// The manifest is produced by the publisher's own pipeline — index the
    /// directory, load the documents, group them by file — instead of by
    /// calling the working-tree recipe and comparing it with itself. That is
    /// what makes this able to fail: with the two recipes apart, an untouched
    /// checkout reports every one of its files as a local change.
    #[test]
    fn a_working_tree_equal_to_the_snapshot_has_no_overlay() {
        let workspace_dir = tempfile::tempdir().unwrap();
        let publisher_dir = tempfile::tempdir().unwrap();
        let workspace = workspace_dir.path();
        let file = workspace.join("CommonModule.bsl");
        std::fs::write(&file, "Процедура Базовая() Экспорт\nКонецПроцедуры\n").unwrap();
        // Two methods on one physical line: their chunks share a line span, so
        // the publisher's sort puts them in a different order than the chunker
        // returns them. Ordinary code cannot show that — its line numbers rise
        // with position, and both orders coincide.
        std::fs::write(
            workspace.join("OneLine.bsl"),
            "Процедура Б() Экспорт КонецПроцедуры Процедура А() Экспорт КонецПроцедуры\n",
        )
        .unwrap();

        let mut publisher =
            crate::SearchEngine::fts_only(&publisher_dir.path().join("baseline.db")).unwrap();
        publisher.index_directory_fts(workspace).unwrap();
        let published = publisher.load_indexed_documents(Some("code")).unwrap();
        let groups = group_documents_by_file(&published);
        assert_eq!(groups.len(), 2, "the fixture publishes both files");

        let manifest = crate::WorkspaceBaselineManifest {
            snapshot_id: "snap-1".to_owned(),
            snapshot_fingerprint: Some("fp-1".to_owned()),
            files: groups
                .iter()
                .map(|group| crate::BaselineManifestFile {
                    collection: group.collection.clone(),
                    path: group.path.clone(),
                    file_fingerprint: group.file_fingerprint.clone(),
                    document_count: group.documents.len(),
                    file_object_id: file_object_id_for(&group.collection, &group.file_fingerprint),
                })
                .collect(),
        };

        // A fresh engine per measurement, primed explicitly. `stats` alone never
        // walks the tree — it is the read-only status path — so on its own it
        // reports "no local changes" for an edited file just as happily as for
        // an untouched one.
        let overlay_files = |db: &str| {
            let mut engine = crate::SearchEngine::fts_only(&publisher_dir.path().join(db)).unwrap();
            engine.set_workspace_root(workspace);
            engine.set_serves_external_baseline(true).unwrap();
            engine.store().save_baseline_manifest(&manifest).unwrap();
            engine.prime_workspace_overlay().unwrap();
            engine.workspace_overlay_stats().unwrap().unwrap().overlay_files
        };

        assert_eq!(overlay_files("clean.db"), 0, "an untouched checkout has no local changes");

        // Zero above means nothing on its own — an overlay that never scanned
        // reports the same. Editing the very file the manifest describes must
        // move it.
        std::fs::write(&file, "Процедура Базовая() Экспорт\n\tВозврат;\nКонецПроцедуры\n").unwrap();
        assert_eq!(overlay_files("edited.db"), 1, "an edited file is a local change");
    }

    #[test]
    fn file_object_id_is_stable_for_same_collection_and_fingerprint() {
        let id_a = file_object_id_for("code", "abc");
        let id_b = file_object_id_for("code", "abc");
        let id_c = file_object_id_for("platform", "abc");

        assert_eq!(id_a, id_b);
        assert_ne!(id_a, id_c);
    }

    fn indexed_document(
        collection: &str,
        path: &str,
        symbol_name: &str,
        line_start: u32,
        content_hash: &str,
        text: &str,
    ) -> IndexedDocument {
        IndexedDocument {
            collection: collection.to_owned(),
            root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
            path: path.to_owned(),
            symbol_name: symbol_name.to_owned(),
            kind: "procedure".to_owned(),
            line_start,
            line_end: line_start + 1,
            text: text.to_owned(),
            content_hash: content_hash.to_owned(),
            graph_context: None,
        }
    }

    fn visible_snapshot_file(collection: &str, path: &str) -> VisibleSnapshotFile {
        VisibleSnapshotFile {
            collection: collection.to_owned(),
            root_id: CONFIGURATION_ROOT_ID.to_owned(),
            path: path.to_owned(),
            file_fingerprint: format!("fp:{collection}:{path}"),
            document_count: 1,
            file_object_id: format!("fo:{collection}:{path}"),
        }
    }

    /// Drops the throwaway test schema even when an assertion panics mid-test.
    /// Raises the pre-root shape of the schema with raw SQL.
    ///
    /// It cannot be built from the node's own steps: after this node the only producer of DDL
    /// always emits the root column, so a legacy schema has to be written by hand or the
    /// migration is never observed doing anything.
    fn raise_pre_root_schema(adapter: &PostgresBaselineAdapter, schema: &str) {
        let mut client = adapter.connect().unwrap();
        client
            .batch_execute(&format!(
                "CREATE SCHEMA IF NOT EXISTS {schema};
                 CREATE TABLE {schema}._schema_metadata_ (
                     setting TEXT PRIMARY KEY,
                     value TEXT NOT NULL
                 );
                 INSERT INTO {schema}._schema_metadata_ VALUES ('schema_version', '1');
                 CREATE TABLE {schema}.snapshots (
                     id TEXT PRIMARY KEY,
                     corpus TEXT NOT NULL,
                     fingerprint TEXT NULL,
                     parent_snapshot_id TEXT NULL,
                     branch TEXT NULL,
                     commit_sha TEXT NULL,
                     created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
                 );
                 CREATE TABLE {schema}.file_objects (
                     id TEXT PRIMARY KEY,
                     collection TEXT NOT NULL,
                     file_fingerprint TEXT NOT NULL,
                     document_count INTEGER NOT NULL
                 );
                 CREATE TABLE {schema}.snapshot_files (
                     snapshot_id TEXT NOT NULL REFERENCES {schema}.snapshots(id) ON DELETE CASCADE,
                     collection TEXT NOT NULL,
                     path TEXT NOT NULL,
                     file_fingerprint TEXT NOT NULL,
                     document_count INTEGER NOT NULL,
                     file_object_id TEXT NOT NULL REFERENCES {schema}.file_objects(id),
                     PRIMARY KEY (snapshot_id, collection, path)
                 );
                 CREATE TABLE {schema}.snapshot_deletions (
                     snapshot_id TEXT NOT NULL REFERENCES {schema}.snapshots(id) ON DELETE CASCADE,
                     collection TEXT NOT NULL,
                     path TEXT NOT NULL,
                     PRIMARY KEY (snapshot_id, collection, path)
                 );
                 CREATE TABLE {schema}.serving_lexical (
                     snapshot_id TEXT NOT NULL REFERENCES {schema}.snapshots(id) ON DELETE CASCADE,
                     collection TEXT NOT NULL,
                     path TEXT NOT NULL,
                     ordinal INTEGER NOT NULL,
                     symbol_name TEXT NOT NULL,
                     kind TEXT NOT NULL,
                     line_start INTEGER NOT NULL,
                     line_end INTEGER NOT NULL,
                     text TEXT NOT NULL,
                     tsv TSVECTOR NOT NULL,
                     PRIMARY KEY (snapshot_id, collection, path, ordinal)
                 );
                 CREATE INDEX idx_{schema}_snapshot_files_snapshot_path
                     ON {schema}.snapshot_files (snapshot_id, collection, path);
                 CREATE INDEX idx_{schema}_snapshot_deletions_snapshot_path
                     ON {schema}.snapshot_deletions (snapshot_id, collection, path);"
            ))
            .unwrap();
    }

    fn primary_key_columns(adapter: &PostgresBaselineAdapter, table: &str) -> Vec<String> {
        let mut client = adapter.connect().unwrap();
        client
            .query_one(
                "SELECT array_agg(a.attname::text ORDER BY k.ord)
                   FROM pg_constraint c
                   CROSS JOIN LATERAL unnest(c.conkey) WITH ORDINALITY AS k(attnum, ord)
                   JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = k.attnum
                  WHERE c.conrelid = to_regclass($1) AND c.contype = 'p'",
                &[&table],
            )
            .unwrap()
            .get::<_, Option<Vec<String>>>(0)
            .unwrap_or_default()
    }

    fn index_columns(adapter: &PostgresBaselineAdapter, index: &str) -> Vec<String> {
        let mut client = adapter.connect().unwrap();
        client
            .query_one(
                "SELECT array_agg(a.attname::text ORDER BY k.ord)
                   FROM pg_index i
                   CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS k(attnum, ord)
                   JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum
                  WHERE i.indexrelid = to_regclass($1)",
                &[&index],
            )
            .unwrap()
            .get::<_, Option<Vec<String>>>(0)
            .unwrap_or_default()
    }

    fn relation_oid(adapter: &PostgresBaselineAdapter, relation: &str) -> u32 {
        let mut client = adapter.connect().unwrap();
        client
            .query_one("SELECT to_regclass($1)::oid::int8", &[&relation])
            .unwrap()
            .get::<_, i64>(0) as u32
    }

    fn constraint_oid(adapter: &PostgresBaselineAdapter, table: &str) -> u32 {
        let mut client = adapter.connect().unwrap();
        client
            .query_one(
                "SELECT c.oid::int8 FROM pg_constraint c
                  WHERE c.conrelid = to_regclass($1) AND c.contype = 'p'",
                &[&table],
            )
            .unwrap()
            .get::<_, i64>(0) as u32
    }

    fn unique_schema(prefix: &str) -> String {
        let unique =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        format!("bsl_{prefix}_{}_{unique}", std::process::id())
    }

    /// The catalog after migration agrees with the generated SQL — for the keys AND for the two
    /// secondary indexes.
    ///
    /// The indexes are not padding. They are declared `CREATE INDEX IF NOT EXISTS` under a fixed
    /// name, so on a live database a re-declaration is skipped by name and the index keeps its
    /// old composition; nothing about the answers changes, only their cost, so that failure is
    /// invisible forever unless it is caught here.
    #[test]
    #[ignore = "requires a live Postgres; set BSL_TEST_PG_URL and run with --ignored"]
    fn migrating_a_pre_root_schema_keys_every_carrier_and_index_by_root() {
        let url = std::env::var("BSL_TEST_PG_URL")
            .expect("BSL_TEST_PG_URL must point to a live Postgres to run this test");
        let schema = unique_schema("preroot");
        let adapter = PostgresBaselineAdapter::new(
            ExternalBaselineConfig::postgres(url).with_schema(&schema),
        )
        .unwrap();
        let _schema_guard = TestSchemaGuard { adapter: adapter.clone(), schema: schema.clone() };
        raise_pre_root_schema(&adapter, &schema);

        assert_eq!(
            primary_key_columns(&adapter, &format!("{schema}.snapshot_files")),
            vec!["snapshot_id", "collection", "path"],
            "the fixture must start from the pre-root shape, or the migration is never observed"
        );

        adapter.migrate_storage().unwrap();

        for (table, expected) in [
            ("snapshot_files", vec!["snapshot_id", "collection", "root_id", "path"]),
            ("snapshot_deletions", vec!["snapshot_id", "collection", "root_id", "path"]),
            ("serving_lexical", vec!["snapshot_id", "collection", "root_id", "path", "ordinal"]),
        ] {
            assert_eq!(
                primary_key_columns(&adapter, &format!("{schema}.{table}")),
                expected,
                "{table} did not reach the rooted key"
            );
        }
        for table in ["snapshot_files", "snapshot_deletions"] {
            assert_eq!(
                index_columns(&adapter, &format!("{schema}.idx_{schema}_{table}_snapshot_path")),
                vec!["snapshot_id", "collection", "root_id", "path"],
                "the secondary index of {table} kept its pre-root composition"
            );
        }
    }

    /// An interrupted migration leaves the original key, not a half-migrated schema.
    ///
    /// What this actually gates is atomicity: applying the statements outside the transaction
    /// turns it red, because the earlier ones have already keyed the table by the time the last
    /// one fails. The version assertion below is a consistency check and NOT a second gate — an
    /// abort inside the statement loop never reaches the version write in any arrangement, so it
    /// would hold even if the stamp were moved back outside the transaction.
    #[test]
    #[ignore = "requires a live Postgres; set BSL_TEST_PG_URL and run with --ignored"]
    fn an_interrupted_migration_leaves_the_original_key() {
        let url = std::env::var("BSL_TEST_PG_URL")
            .expect("BSL_TEST_PG_URL must point to a live Postgres to run this test");
        let schema = unique_schema("interrupted");
        let adapter = PostgresBaselineAdapter::new(
            ExternalBaselineConfig::postgres(url).with_schema(&schema),
        )
        .unwrap();
        let _schema_guard = TestSchemaGuard { adapter: adapter.clone(), schema: schema.clone() };
        raise_pre_root_schema(&adapter, &schema);

        let mut statements = adapter.ensure_schema_statements();
        statements.push("SELECT 1 / 0".to_owned());
        let mut client = adapter.connect().unwrap();
        let error = adapter.migrate_structure(&mut client, &statements).unwrap_err();

        // Identified by SQLSTATE, not by message text: `SearchError::Postgres` renders as a bare
        // "db error" and keeps the server's own message in its source — where it arrives in the
        // server's language, so matching on English would pass or fail by locale.
        let code = std::error::Error::source(&error)
            .and_then(|source| source.downcast_ref::<postgres::Error>())
            .and_then(|pg| pg.code())
            .cloned();
        assert_eq!(
            code,
            Some(postgres::error::SqlState::DIVISION_BY_ZERO),
            "the interruption must surface, not be swallowed: {error}"
        );
        assert_eq!(
            primary_key_columns(&adapter, &format!("{schema}.snapshot_files")),
            vec!["snapshot_id", "collection", "path"],
            "a rolled-back migration must leave the original key"
        );
        assert_eq!(
            adapter.get_schema_version().unwrap(),
            Some(1),
            "a rolled-back migration must not claim the new version"
        );
    }

    /// Re-running the migration is a genuine no-op, proved by object identity.
    ///
    /// Composition alone cannot prove it: an implementation that unconditionally drops and
    /// recreates already-correct keys shows the same final composition — while taking the heavy
    /// lock on the largest tables of the schema on every `admin migrate`.
    #[test]
    #[ignore = "requires a live Postgres; set BSL_TEST_PG_URL and run with --ignored"]
    fn migrating_an_already_rooted_schema_rebuilds_nothing() {
        let url = std::env::var("BSL_TEST_PG_URL")
            .expect("BSL_TEST_PG_URL must point to a live Postgres to run this test");
        let schema = unique_schema("noop");
        let adapter = PostgresBaselineAdapter::new(
            ExternalBaselineConfig::postgres(url).with_schema(&schema),
        )
        .unwrap();
        let _schema_guard = TestSchemaGuard { adapter: adapter.clone(), schema: schema.clone() };
        raise_pre_root_schema(&adapter, &schema);
        adapter.migrate_storage().unwrap();

        let carriers = ["snapshot_files", "snapshot_deletions", "serving_lexical"];
        let indexes = ["snapshot_files", "snapshot_deletions"];
        let keys_before: Vec<u32> = carriers
            .iter()
            .map(|table| constraint_oid(&adapter, &format!("{schema}.{table}")))
            .collect();
        let indexes_before: Vec<u32> = indexes
            .iter()
            .map(|table| {
                relation_oid(&adapter, &format!("{schema}.idx_{schema}_{table}_snapshot_path"))
            })
            .collect();

        adapter.migrate_storage().unwrap();

        let keys_after: Vec<u32> = carriers
            .iter()
            .map(|table| constraint_oid(&adapter, &format!("{schema}.{table}")))
            .collect();
        let indexes_after: Vec<u32> = indexes
            .iter()
            .map(|table| {
                relation_oid(&adapter, &format!("{schema}.idx_{schema}_{table}_snapshot_path"))
            })
            .collect();

        assert_eq!(keys_before, keys_after, "a primary key was rebuilt on an unchanged schema");
        assert_eq!(
            indexes_before, indexes_after,
            "a secondary index was rebuilt on an unchanged schema"
        );
    }

    /// Existing rows survive the migration without being rewritten.
    ///
    /// The third assertion is the one the invariant exists for: the first two are green for an
    /// implementation that ran `UPDATE … SET root_id = root_id` over every row, and on a corpus
    /// the size of ERP that is the difference between a cheap migration and a full rewrite.
    #[test]
    #[ignore = "requires a live Postgres; set BSL_TEST_PG_URL and run with --ignored"]
    fn migration_backfills_the_root_without_rewriting_a_row() {
        let url = std::env::var("BSL_TEST_PG_URL")
            .expect("BSL_TEST_PG_URL must point to a live Postgres to run this test");
        let schema = unique_schema("backfill");
        let adapter = PostgresBaselineAdapter::new(
            ExternalBaselineConfig::postgres(url).with_schema(&schema),
        )
        .unwrap();
        let _schema_guard = TestSchemaGuard { adapter: adapter.clone(), schema: schema.clone() };
        raise_pre_root_schema(&adapter, &schema);
        {
            let mut client = adapter.connect().unwrap();
            client
                .batch_execute(&format!(
                    "INSERT INTO {schema}.snapshots (id, corpus) VALUES ('snap-1', 'code');
                     INSERT INTO {schema}.file_objects VALUES ('obj-1', 'code', 'fp-1', 1);
                     INSERT INTO {schema}.snapshot_files
                         VALUES ('snap-1', 'code', 'src/A.bsl', 'fp-1', 1, 'obj-1');"
                ))
                .unwrap();
        }
        let row_version = |adapter: &PostgresBaselineAdapter| -> String {
            let mut client = adapter.connect().unwrap();
            client
                .query_one(&format!("SELECT xmin::text FROM {schema}.snapshot_files"), &[])
                .unwrap()
                .get(0)
        };
        let before = row_version(&adapter);

        adapter.migrate_storage().unwrap();

        let mut client = adapter.connect().unwrap();
        let (path, root_id): (String, String) = {
            let row = client
                .query_one(&format!("SELECT path, root_id FROM {schema}.snapshot_files"), &[])
                .unwrap();
            (row.get(0), row.get(1))
        };
        assert_eq!(path, "src/A.bsl", "the row must survive the migration");
        assert_eq!(root_id, CONFIGURATION_ROOT_ID, "a pre-root row belongs to the configuration");
        assert_eq!(before, row_version(&adapter), "the migration rewrote the row");

        client
            .batch_execute(&format!("UPDATE {schema}.snapshot_files SET document_count = 2"))
            .unwrap();
        assert_ne!(
            before,
            row_version(&adapter),
            "positive control: a real rewrite must move the row version, or this gate is blind"
        );
    }

    struct TestSchemaGuard {
        adapter: PostgresBaselineAdapter,
        schema: String,
    }

    impl Drop for TestSchemaGuard {
        fn drop(&mut self) {
            if let Ok(mut client) = self.adapter.connect() {
                let _ =
                    client.batch_execute(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema));
            }
        }
    }

    fn materialized_summary(
        adapter: &PostgresBaselineAdapter,
        snapshot_id: &str,
    ) -> EffectiveSnapshotSummary {
        let mut client = adapter.connect().unwrap();
        let visible = materialize_visible_snapshot_files(&mut *client, adapter, snapshot_id)
            .unwrap_or_else(|error| panic!("materialize {snapshot_id}: {error}"));
        let mut documents = 0usize;
        let mut by_collection = std::collections::BTreeMap::<String, (usize, usize)>::new();
        for file in &visible {
            documents += file.document_count;
            let entry = by_collection.entry(file.collection.clone()).or_default();
            entry.0 += 1;
            entry.1 += file.document_count;
        }
        let collections = by_collection
            .into_iter()
            .map(|(collection, (files, documents))| BaselineCollectionRecord {
                collection,
                files,
                documents,
            })
            .collect();
        EffectiveSnapshotSummary {
            total_files: visible.len(),
            total_documents: documents,
            collections,
        }
    }

    /// Parity oracle for the server-side summary aggregate: every snapshot's counts must
    /// equal a recount over the materialized visible-file map, which stays the semantic
    /// reference for resolve and publish paths.
    #[test]
    #[ignore = "requires a live Postgres; set BSL_TEST_PG_URL and run with --ignored"]
    fn snapshot_summary_aggregate_matches_materialized_visibility_on_live_postgres() {
        let url = std::env::var("BSL_TEST_PG_URL")
            .expect("BSL_TEST_PG_URL must point to a live Postgres to run this test");
        let unique =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
        let schema = format!("bsl_parity_{}_{unique}", std::process::id());
        let adapter = PostgresBaselineAdapter::new(
            ExternalBaselineConfig::postgres(url).with_schema(&schema),
        )
        .unwrap();
        let _schema_guard = TestSchemaGuard { adapter: adapter.clone(), schema: schema.clone() };
        adapter.migrate_storage().unwrap();

        let corpus = CorpusId::WorkspaceCode;
        let metadata = SnapshotPublishMetadata::default();
        let changed_a = [
            indexed_document("code", "src/A.bsl", "A1", 10, "hash-a1-v2", "text-a1-v2"),
            indexed_document("code", "src/A.bsl", "A2", 20, "hash-a2-v2", "text-a2-v2"),
            indexed_document("code", "src/A.bsl", "A3", 30, "hash-a3", "text-a3"),
        ];

        adapter
            .publish_snapshot(
                &Snapshot::new("snap-1", corpus.clone()),
                &metadata,
                &[
                    indexed_document("code", "src/A.bsl", "A1", 10, "hash-a1", "text-a1"),
                    indexed_document("code", "src/A.bsl", "A2", 20, "hash-a2", "text-a2"),
                    indexed_document("code", "src/B.bsl", "B", 10, "hash-b", "text-b"),
                    indexed_document("platform", "std/P.bsl", "P", 10, "hash-p", "text-p"),
                ],
            )
            .unwrap();
        // B and P are not republished: the publish path records them as deletions.
        adapter
            .publish_snapshot(
                &Snapshot::new("snap-2", corpus.clone()).with_parent("snap-1"),
                &metadata,
                &[
                    changed_a[0].clone(),
                    changed_a[1].clone(),
                    changed_a[2].clone(),
                    indexed_document("code", "src/C.bsl", "C", 10, "hash-c", "text-c"),
                ],
            )
            .unwrap();
        // A is byte-identical to snap-2, so it is reused (visible via the parent row only);
        // P returns after an ancestor deletion; C drops out.
        adapter
            .publish_snapshot(
                &Snapshot::new("snap-3", corpus.clone()).with_parent("snap-2"),
                &metadata,
                &[
                    changed_a[0].clone(),
                    changed_a[1].clone(),
                    changed_a[2].clone(),
                    indexed_document("code", "src/D.bsl", "D1", 10, "hash-d1", "text-d1"),
                    indexed_document("code", "src/D.bsl", "D2", 20, "hash-d2", "text-d2"),
                    indexed_document("platform", "std/P.bsl", "P", 10, "hash-p", "text-p"),
                ],
            )
            .unwrap();
        // An empty publish over a non-empty parent turns every visible file into a deletion.
        adapter
            .publish_snapshot(
                &Snapshot::new("snap-4", corpus.clone()).with_parent("snap-3"),
                &metadata,
                &[],
            )
            .unwrap();
        adapter
            .publish_snapshot(&Snapshot::new("root-empty", corpus.clone()), &metadata, &[])
            .unwrap();

        // A file row and a deletion row for the same key in the same snapshot cannot come
        // out of the publish path; the deletion must win on the ancestry-position tie.
        {
            let mut client = adapter.connect().unwrap();
            client
                .execute(
                    &format!(
                        "INSERT INTO {} (id, collection, file_fingerprint, document_count)
                         VALUES ($1, $2, $3, $4)",
                        adapter.table("file_objects")
                    ),
                    &[&"adversarial-fo", &"code", &"adversarial-fp", &7i32],
                )
                .unwrap();
            client
                .execute(
                    &format!(
                        "INSERT INTO {} (snapshot_id, collection, path, file_fingerprint,
                                         document_count, file_object_id)
                         VALUES ($1, $2, $3, $4, $5, $6)",
                        adapter.table("snapshot_files")
                    ),
                    &[
                        &"snap-3",
                        &"code",
                        &"src/Shadowed.bsl",
                        &"adversarial-fp",
                        &7i32,
                        &"adversarial-fo",
                    ],
                )
                .unwrap();
            client
                .execute(
                    &format!(
                        "INSERT INTO {} (snapshot_id, collection, path) VALUES ($1, $2, $3)",
                        adapter.table("snapshot_deletions")
                    ),
                    &[&"snap-3", &"code", &"src/Shadowed.bsl"],
                )
                .unwrap();
        }

        struct SnapshotExpectation {
            snapshot_id: &'static str,
            files: usize,
            documents: usize,
            collections: &'static [(&'static str, usize, usize)],
        }
        let expectations = [
            SnapshotExpectation {
                snapshot_id: "snap-1",
                files: 3,
                documents: 4,
                collections: &[("code", 2, 3), ("platform", 1, 1)],
            },
            SnapshotExpectation {
                snapshot_id: "snap-2",
                files: 2,
                documents: 4,
                collections: &[("code", 2, 4)],
            },
            SnapshotExpectation {
                snapshot_id: "snap-3",
                files: 3,
                documents: 6,
                collections: &[("code", 2, 5), ("platform", 1, 1)],
            },
            SnapshotExpectation { snapshot_id: "snap-4", files: 0, documents: 0, collections: &[] },
            SnapshotExpectation {
                snapshot_id: "root-empty",
                files: 0,
                documents: 0,
                collections: &[],
            },
        ];
        for expectation in &expectations {
            let snapshot_id = expectation.snapshot_id;
            let details = adapter
                .snapshot_details(snapshot_id)
                .unwrap()
                .unwrap_or_else(|| panic!("snapshot {snapshot_id} must exist"));
            let materialized = materialized_summary(&adapter, snapshot_id);
            assert_eq!(
                materialized.total_files, expectation.files,
                "{snapshot_id}: materialized files"
            );
            assert_eq!(
                materialized.total_documents, expectation.documents,
                "{snapshot_id}: materialized documents"
            );
            assert_eq!(
                details.snapshot.files, materialized.total_files,
                "{snapshot_id}: aggregate files diverge from materialization"
            );
            assert_eq!(
                details.snapshot.documents, materialized.total_documents,
                "{snapshot_id}: aggregate documents diverge from materialization"
            );
            assert_eq!(
                details.collections, materialized.collections,
                "{snapshot_id}: aggregate collections diverge from materialization"
            );
            let expected_collections: Vec<BaselineCollectionRecord> = expectation
                .collections
                .iter()
                .map(|(collection, files, documents)| BaselineCollectionRecord {
                    collection: (*collection).to_owned(),
                    files: *files,
                    documents: *documents,
                })
                .collect();
            assert_eq!(details.collections, expected_collections, "{snapshot_id}: collections");
        }

        let listed = adapter.list_snapshots(None, None, None, 10).unwrap();
        assert_eq!(listed.len(), expectations.len(), "listing must cover every snapshot");
        for record in &listed {
            let materialized = materialized_summary(&adapter, &record.snapshot_id);
            assert_eq!(
                record.files, materialized.total_files,
                "{}: listed files diverge from materialization",
                record.snapshot_id
            );
            assert_eq!(
                record.documents, materialized.total_documents,
                "{}: listed documents diverge from materialization",
                record.snapshot_id
            );
        }
    }

    /// The batched totals path reimplements ancestry walking in SQL; it must surface the same
    /// cycle / missing-parent errors as the iterative `snapshot_ancestry_ids`, and pick the error
    /// deterministically (first corrupt seed in listing order) when several seeds are corrupt.
    #[test]
    #[ignore = "requires a live Postgres; set BSL_TEST_PG_URL and run with --ignored"]
    fn snapshot_totals_batch_surfaces_corrupt_ancestry_on_live_postgres() {
        let url = std::env::var("BSL_TEST_PG_URL")
            .expect("BSL_TEST_PG_URL must point to a live Postgres to run this test");
        let unique =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
        let schema = format!("bsl_corrupt_{}_{unique}", std::process::id());
        let adapter = PostgresBaselineAdapter::new(
            ExternalBaselineConfig::postgres(url).with_schema(&schema),
        )
        .unwrap();
        let _schema_guard = TestSchemaGuard { adapter: adapter.clone(), schema: schema.clone() };
        adapter.migrate_storage().unwrap();

        // A healthy snapshot, plus two corrupt ones inserted directly: the publish path cannot
        // produce a self-cycle or a parent pointing at a non-existent snapshot.
        adapter
            .publish_snapshot(
                &Snapshot::new("good", CorpusId::WorkspaceCode),
                &SnapshotPublishMetadata::default(),
                &[indexed_document("code", "src/A.bsl", "A", 5, "hash-a", "text-a")],
            )
            .unwrap();
        {
            let mut client = adapter.connect().unwrap();
            let insert = format!(
                "INSERT INTO {} (id, corpus, parent_snapshot_id) VALUES ($1, $2, $3)",
                adapter.table("snapshots")
            );
            client.execute(&insert, &[&"cyc", &"workspace-code", &"cyc"]).unwrap();
            client.execute(&insert, &[&"dang", &"workspace-code", &"ghost"]).unwrap();
        }

        let mut client = adapter.connect().unwrap();
        let seed = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<String>>();

        let healthy =
            effective_snapshot_totals_batch(&mut *client, &adapter, &seed(&["good"])).unwrap();
        let reference = materialized_summary(&adapter, "good");
        assert_eq!(
            healthy.get("good").copied(),
            Some((reference.total_files, reference.total_documents)),
            "healthy seed totals must match the materialized reference",
        );

        let cycle = effective_snapshot_totals_batch(&mut *client, &adapter, &seed(&["cyc"]))
            .expect_err("self-cycle must error");
        assert!(cycle.to_string().contains("cycle at 'cyc'"), "unexpected cycle error: {cycle}");

        let missing = effective_snapshot_totals_batch(&mut *client, &adapter, &seed(&["dang"]))
            .expect_err("dangling parent must error");
        assert!(
            missing.to_string().contains("'ghost' was not found"),
            "unexpected missing error: {missing}",
        );

        // The first corrupt seed in listing order decides the error, regardless of result-set order.
        let cyc_first =
            effective_snapshot_totals_batch(&mut *client, &adapter, &seed(&["cyc", "dang"]))
                .expect_err("mixed corrupt batch must error");
        assert!(cyc_first.to_string().contains("cycle at 'cyc'"), "cyc-first: {cyc_first}");
        let dang_first =
            effective_snapshot_totals_batch(&mut *client, &adapter, &seed(&["dang", "cyc"]))
                .expect_err("mixed corrupt batch must error");
        assert!(
            dang_first.to_string().contains("'ghost' was not found"),
            "dang-first: {dang_first}",
        );

        // A healthy seed alongside a corrupt one still fails the whole batch.
        let mixed =
            effective_snapshot_totals_batch(&mut *client, &adapter, &seed(&["good", "dang"]))
                .expect_err("healthy + corrupt must error");
        assert!(mixed.to_string().contains("'ghost' was not found"), "good+dang: {mixed}");
    }
}
