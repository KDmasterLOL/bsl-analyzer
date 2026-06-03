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
use postgres::{GenericClient, NoTls, Row, Transaction};
use r2d2_postgres::{r2d2::Pool, PostgresConnectionManager};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::{Duration, Instant};

const SCHEMA_METADATA_TABLE: &str = "_schema_metadata_";
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

#[derive(Debug, Clone)]
pub struct PostgresBaselineAdapter {
    config: ExternalBaselineConfig,
    schema: String,
    pool: Pool<PostgresConnectionManager<NoTls>>,
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
        Ok(Self { config, schema, pool })
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
        let mut client = self.connect()?;
        for table_name in REQUIRED_STORAGE_TABLES {
            let exists = client.query_opt(
                "SELECT 1 FROM information_schema.tables
                 WHERE table_schema = $1 AND table_name = $2
                 LIMIT 1",
                &[&self.schema, table_name],
            )?;
            if exists.is_none() {
                return Err(SearchError::StorageNotInitialized { schema: self.schema.clone() });
            }
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
                    path TEXT NOT NULL,
                    file_fingerprint TEXT NOT NULL,
                    document_count INTEGER NOT NULL,
                    file_object_id TEXT NOT NULL REFERENCES {}(id) ON DELETE RESTRICT,
                    PRIMARY KEY (snapshot_id, collection, path)
                )",
                self.table("snapshot_files"),
                self.table("snapshots"),
                self.table("file_objects")
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    snapshot_id TEXT NOT NULL REFERENCES {}(id) ON DELETE CASCADE,
                    collection TEXT NOT NULL,
                    path TEXT NOT NULL,
                    PRIMARY KEY (snapshot_id, collection, path)
                )",
                self.table("snapshot_deletions"),
                self.table("snapshots"),
            ),
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
                    text TEXT NOT NULL,
                    tsv TSVECTOR NOT NULL,
                    PRIMARY KEY (snapshot_id, collection, path, ordinal)
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
                "CREATE INDEX IF NOT EXISTS idx_{}_snapshot_files_snapshot_path
                 ON {} (snapshot_id, collection, path)",
                self.schema,
                self.table("snapshot_files")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_snapshot_files_object
                 ON {} (file_object_id)",
                self.schema,
                self.table("snapshot_files")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_snapshot_deletions_snapshot_path
                 ON {} (snapshot_id, collection, path)",
                self.schema,
                self.table("snapshot_deletions")
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
        let mut snapshots = Vec::with_capacity(rows.len());
        for row in rows {
            let mut snapshot = snapshot_record_from_metadata_row(row);
            let summary = effective_snapshot_summary(&mut *client, self, &snapshot.snapshot_id)?;
            snapshot.files = summary.total_files;
            snapshot.documents = summary.total_documents;
            snapshots.push(snapshot);
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
        let query_text = query.trim();
        if query_text.is_empty() {
            return Ok(Vec::new());
        }

        self.check_storage_readiness()?;
        let collection =
            collection.map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned);
        let limit = limit.clamp(1, 200) as i64;
        let mut client = self.connect()?;
        let mut sql = format!(
            "SELECT collection,
                    path,
                    symbol_name,
                    kind,
                    line_start,
                    line_end,
                    text,
                    ts_rank(tsv, plainto_tsquery('simple', $2), 32) AS rank
             FROM {}
             WHERE snapshot_id = $1
               AND tsv @@ plainto_tsquery('simple', $2)",
            self.table("serving_lexical")
        );

        let snapshot_id = snapshot_id.to_owned();
        let query_text = query_text.to_owned();
        let mut params: Vec<&(dyn postgres::types::ToSql + Sync)> = vec![&snapshot_id, &query_text];
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
        for statement in self.ensure_schema_statements() {
            client.batch_execute(&statement)?;
        }
        for statement in self.pgvector_schema_statements() {
            if let Err(e) = client.batch_execute(&statement) {
                tracing::warn!("pgvector DDL skipped (semantic serving will be unavailable): {e}");
                break;
            }
        }

        let mut tx = client.transaction()?;
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
            let file_key = (file_group.collection.clone(), file_group.path.clone());
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

        for ((collection, path), _) in remaining_parent_files {
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
    path: String,
    file_fingerprint: String,
    documents: Vec<IndexedDocument>,
}

#[derive(Debug, Clone)]
struct VisibleSnapshotFile {
    collection: String,
    path: String,
    file_fingerprint: String,
    document_count: usize,
    file_object_id: String,
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
            path: row.get("path"),
            file_fingerprint: row.get("file_fingerprint"),
            document_count: row.get::<_, i32>("document_count") as usize,
            file_object_id: row.get("file_object_id"),
        })
        .collect())
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
                foi.line_start, foi.line_end, co.text
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
        let embedding_key = semantic_key_for_document(
            path,
            row.get("kind"),
            row.get("symbol_name"),
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

fn group_documents_by_file(documents: &[IndexedDocument]) -> Vec<PublishedFileGroup> {
    let mut grouped = BTreeMap::<(String, String), Vec<IndexedDocument>>::new();
    for document in documents {
        grouped
            .entry((document.collection.clone(), document.path.clone()))
            .or_default()
            .push(document.clone());
    }

    grouped
        .into_iter()
        .map(|((collection, path), mut documents)| {
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
            PublishedFileGroup { collection, path, file_fingerprint, documents }
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

fn effective_snapshot_summary(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
) -> Result<EffectiveSnapshotSummary, SearchError> {
    let visible_files = materialize_visible_snapshot_files(client, adapter, snapshot_id)?;
    let mut documents = 0usize;
    let mut by_collection = BTreeMap::<String, (usize, usize)>::new();
    for file in &visible_files {
        documents += file.document_count;
        let entry = by_collection.entry(file.collection.clone()).or_default();
        entry.0 += 1;
        entry.1 += file.document_count;
    }

    Ok(EffectiveSnapshotSummary {
        total_files: visible_files.len(),
        total_documents: documents,
        collections: by_collection
            .into_iter()
            .map(|(collection, (files, documents))| BaselineCollectionRecord {
                collection,
                files,
                documents,
            })
            .collect(),
    })
}

fn materialize_visible_snapshot_file_map(
    client: &mut impl GenericClient,
    adapter: &PostgresBaselineAdapter,
    snapshot_id: &str,
) -> Result<BTreeMap<(String, String), VisibleSnapshotFile>, SearchError> {
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
            path: row.get("path"),
            file_fingerprint: row.get("file_fingerprint"),
            document_count: row.get::<_, i32>("document_count") as usize,
            file_object_id: row.get("file_object_id"),
        });
    }
    let mut deletions_by_snapshot = HashMap::<String, Vec<(String, String)>>::new();
    for row in client.query(&deletion_query, &[&ancestry])? {
        let snapshot_id: String = row.get("snapshot_id");
        deletions_by_snapshot
            .entry(snapshot_id)
            .or_default()
            .push((row.get("collection"), row.get("path")));
    }

    let mut seen_paths = HashSet::<(String, String)>::new();
    let mut visible_files = BTreeMap::<(String, String), VisibleSnapshotFile>::new();
    for snapshot_id in ancestry {
        if let Some(deletions) = deletions_by_snapshot.remove(&snapshot_id) {
            for key in deletions {
                seen_paths.insert(key);
            }
        }
        if let Some(files) = files_by_snapshot.remove(&snapshot_id) {
            for file in files {
                let key = (file.collection.clone(), file.path.clone());
                if seen_paths.insert(key.clone()) {
                    visible_files.insert(key, file);
                }
            }
        }
    }

    Ok(visible_files)
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
    let payload = format!(
        "Path: {}\nKind: {}\nSymbol: {}\n{}",
        row.get::<_, String>("path"),
        row.get::<_, String>("kind"),
        row.get::<_, String>("symbol_name"),
        row.get::<_, String>("text"),
    );
    blake3::hash(payload.as_bytes()).to_hex().to_string()
}

fn semantic_key_for_document(path: &str, kind: &str, symbol_name: &str, text: &str) -> String {
    let payload = format!("Path: {path}\nKind: {kind}\nSymbol: {symbol_name}\n{text}");
    blake3::hash(payload.as_bytes()).to_hex().to_string()
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
        file_object_id_for, fingerprint_file_documents, group_documents_by_file,
        semantic_publish_phase_count, semantic_publish_plan, semantic_publish_strategy,
        unique_content_object_rows, ContentObjectRow, PostgresBaselineAdapter, SemanticPublishPlan,
        SemanticPublishStrategy, VisibleSnapshotFile,
    };
    use crate::domain::{CorpusId, ExternalBaselineConfig};
    use crate::ports::{BaselineLexicalSearch, BaselineSemanticSearch, SnapshotCatalog};
    use crate::{BaselineRef, IndexedDocument};

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
            path: path.to_owned(),
            file_fingerprint: format!("fp:{collection}:{path}"),
            document_count: 1,
            file_object_id: format!("fo:{collection}:{path}"),
        }
    }
}
