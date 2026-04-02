use crate::domain::{
    BaselineRef, CorpusId, ExternalBaselineConfig, IndexedDocument, Snapshot,
    SnapshotPublishMetadata, SnapshotPublishStats,
};
use crate::error::SearchError;
use crate::external_baseline::{
    BaselineCollectionRecord, BaselineEmbeddingCoverageRecord, BaselineEmbeddingModelRecord,
    BaselineEmbeddingStats, BaselineFileObjectDetails, BaselineFileObjectRecord,
    BaselineFileObjectReference, BaselineGcReport, BaselineSnapshotDetails, BaselineSnapshotRecord,
};
use crate::ports::{SnapshotCatalog, SnapshotContentStore, SnapshotPublisher};
use postgres::{Client, GenericClient, NoTls, Row, Transaction};
use std::collections::{BTreeMap, HashMap, HashSet};

/// PostgreSQL adapter for centralized baseline storage.
///
/// Expected schema:
/// - `<schema>.snapshots`
///   `id TEXT PRIMARY KEY`
///   `corpus TEXT NOT NULL`
///   `fingerprint TEXT NULL`
///   `parent_snapshot_id TEXT NULL`
///   `branch TEXT NULL`
///   `commit_sha TEXT NULL`
///   `created_at TIMESTAMPTZ NOT NULL`
/// - `<schema>.snapshot_files`
///   `snapshot_id TEXT NOT NULL`
///   `collection TEXT NOT NULL`
///   `path TEXT NOT NULL`
///   `file_fingerprint TEXT NOT NULL`
///   `document_count INTEGER NOT NULL`
///   `file_object_id TEXT NULL`
/// - `<schema>.file_objects`
///   `id TEXT PRIMARY KEY`
///   `collection TEXT NOT NULL`
///   `file_fingerprint TEXT NOT NULL`
///   `document_count INTEGER NOT NULL`
/// - `<schema>.file_object_items`
///   `file_object_id TEXT NOT NULL`
///   `ordinal INTEGER NOT NULL`
///   `symbol_name TEXT NOT NULL`
///   `kind TEXT NOT NULL`
///   `line_start INTEGER NOT NULL`
///   `line_end INTEGER NOT NULL`
///   `content_hash TEXT NOT NULL`
/// - `<schema>.snapshot_items`
///   legacy fallback for snapshots published before file-object materialization
/// - `<schema>.content_objects`
///   `content_hash TEXT PRIMARY KEY`
///   `text TEXT NOT NULL`
/// - `<schema>.semantic_embeddings`
///   `embedding_key TEXT NOT NULL`
///   `model_id TEXT NOT NULL`
///   `dimension INTEGER NOT NULL`
///   `embedding BYTEA NOT NULL`
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
                    PRIMARY KEY (file_object_id, ordinal)
                )",
                self.table("file_object_items"),
                self.table("file_objects"),
                self.table("content_objects")
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

    pub fn list_snapshots(
        &self,
        corpus: Option<&str>,
        branch: Option<&str>,
        commit: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BaselineSnapshotRecord>, SearchError> {
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
            let summary = effective_snapshot_summary(&mut client, self, &snapshot.snapshot_id)?;
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
        let summary = effective_snapshot_summary(&mut client, self, snapshot_id)?;
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
        self.ensure_storage()?;
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
        if embedding_keys.is_empty() {
            return Ok(HashMap::new());
        }

        let mut client = self.connect()?;
        let query = format!(
            "SELECT embedding_key, embedding
             FROM {}
             WHERE model_id = $1 AND dimension = $2 AND embedding_key = ANY($3)",
            self.table("semantic_embeddings")
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

    pub fn list_embedding_models(
        &self,
        model_id: Option<&str>,
        dimension: Option<usize>,
    ) -> Result<Vec<BaselineEmbeddingModelRecord>, SearchError> {
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
        let mut client = self.connect()?;
        let active_keys = collect_active_embedding_keys(&mut client, self)?;
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
        let mut client = self.connect()?;
        let active_keys = collect_active_embedding_keys(&mut client, self)?;

        let orphan_file_object_ids = query_string_column(
            &mut client,
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
}

impl SnapshotCatalog for PostgresBaselineAdapter {
    fn resolve_baseline(&self, baseline: &BaselineRef) -> Result<Option<Snapshot>, SearchError> {
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
        let visible_files = materialize_visible_snapshot_files(&mut client, self, &snapshot.id.0)?;
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
                    foi.content_hash
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
        metadata: &SnapshotPublishMetadata,
        documents: &[IndexedDocument],
    ) -> Result<SnapshotPublishStats, SearchError> {
        self.ensure_storage()?;

        if snapshot.parent_id.as_ref().is_some_and(|parent| parent.0 == snapshot.id.0) {
            return Err(SearchError::ExternalBaseline(
                "snapshot cannot reference itself as parent".to_owned(),
            ));
        }

        let mut client = self.connect()?;
        let mut tx = client.transaction()?;
        let file_groups = group_documents_by_file(documents);

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

        let parent_files = if let Some(parent_id) = snapshot.parent_id.as_ref() {
            materialize_visible_snapshot_file_map(&mut tx, self, &parent_id.0)?
        } else {
            BTreeMap::new()
        };

        let insert_snapshot_file = format!(
            "INSERT INTO {} (
                snapshot_id, collection, path, file_fingerprint, document_count, file_object_id
             ) VALUES ($1, $2, $3, $4, $5, $6)",
            self.table("snapshot_files")
        );
        let insert_snapshot_deletion = format!(
            "INSERT INTO {} (snapshot_id, collection, path)
             VALUES ($1, $2, $3)",
            self.table("snapshot_deletions")
        );

        let mut stats = SnapshotPublishStats::default();
        let mut remaining_parent_files = parent_files;
        for file_group in file_groups {
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
            tx.execute(
                &insert_snapshot_file,
                &[
                    &snapshot.id.0,
                    &file_group.collection,
                    &file_group.path,
                    &file_group.file_fingerprint,
                    &(file_group.documents.len() as i32),
                    &file_object_id,
                ],
            )?;

            let _ = try_insert_file_object(&mut tx, self, &file_object_id, &file_group)?;
            stats.written_files += 1;
            stats.written_documents += file_group.documents.len();
        }

        for ((collection, path), _) in remaining_parent_files {
            tx.execute(&insert_snapshot_deletion, &[&snapshot.id.0, &collection, &path])?;
            stats.deleted_files += 1;
        }

        tx.commit()?;
        Ok(stats)
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
}

#[derive(Debug, Clone)]
struct EffectiveSnapshotSummary {
    total_files: usize,
    total_documents: usize,
    collections: Vec<BaselineCollectionRecord>,
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

    let upsert_content = format!(
        "INSERT INTO {} (content_hash, text)
         VALUES ($1, $2)
         ON CONFLICT (content_hash) DO UPDATE SET text = EXCLUDED.text",
        adapter.table("content_objects")
    );
    let insert_item = format!(
        "INSERT INTO {} (
            file_object_id, ordinal, symbol_name, kind, line_start, line_end, content_hash
         ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        adapter.table("file_object_items")
    );
    for (ordinal, document) in file_group.documents.iter().enumerate() {
        tx.execute(&upsert_content, &[&document.content_hash, &document.text])?;
        tx.execute(
            &insert_item,
            &[
                &file_object_id,
                &(ordinal as i32),
                &document.symbol_name,
                &document.kind,
                &(document.line_start as i32),
                &(document.line_end as i32),
                &document.content_hash,
            ],
        )?;
    }
    Ok(true)
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
    client: &mut Client,
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

fn query_string_column(
    client: &mut Client,
    query: &str,
    params: &[&(dyn postgres::types::ToSql + Sync)],
) -> Result<Vec<String>, SearchError> {
    Ok(client.query(query, params)?.into_iter().map(|row| row.get(0)).collect())
}

#[cfg(test)]
mod tests {
    use super::{
        file_object_id_for, fingerprint_file_documents, group_documents_by_file,
        PostgresBaselineAdapter,
    };
    use crate::domain::{CorpusId, ExternalBaselineConfig};
    use crate::ports::{SnapshotCatalog, SnapshotPublisher};
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
        }
    }
}
