use crate::domain::{
    BaselineRef, CorpusId, ExternalBaselineConfig, IndexedDocument, Snapshot,
    SnapshotPublishMetadata, SnapshotPublishStats,
};
use crate::error::SearchError;
use crate::external_baseline::{
    BaselineCollectionRecord, BaselineSnapshotDetails, BaselineSnapshotRecord,
};
use crate::ports::{SnapshotCatalog, SnapshotContentStore, SnapshotPublisher};
use postgres::{Client, NoTls, Row, Transaction};
use std::collections::{BTreeMap, HashSet};

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
                    file_object_id TEXT NULL,
                    PRIMARY KEY (snapshot_id, collection, path)
                )",
                self.table("snapshot_files"),
                self.table("snapshots")
            ),
            format!(
                "ALTER TABLE {} ADD COLUMN IF NOT EXISTS file_object_id TEXT NULL",
                self.table("snapshot_files")
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
                "CREATE INDEX IF NOT EXISTS idx_{}_snapshot_items_snapshot_path
                 ON {} (snapshot_id, collection, path)",
                self.schema,
                self.table("snapshot_items")
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
                    s.created_at::TEXT AS created_at,
                    COALESCE(
                        (SELECT SUM(sf.document_count) FROM {} sf WHERE sf.snapshot_id = s.id),
                        (SELECT COUNT(*) FROM {} si WHERE si.snapshot_id = s.id),
                        0
                    ) AS documents,
                    COALESCE(
                        (SELECT COUNT(*) FROM {} sf WHERE sf.snapshot_id = s.id),
                        (SELECT COUNT(DISTINCT si.path) FROM {} si WHERE si.snapshot_id = s.id),
                        0
                    ) AS files
             FROM {} s
             WHERE 1 = 1",
            self.table("snapshot_files"),
            self.table("snapshot_items"),
            self.table("snapshot_files"),
            self.table("snapshot_items"),
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
        Ok(rows.into_iter().map(snapshot_record_from_row).collect())
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
                    s.created_at::TEXT AS created_at,
                    COALESCE(
                        (SELECT SUM(sf.document_count) FROM {} sf WHERE sf.snapshot_id = s.id),
                        (SELECT COUNT(*) FROM {} si WHERE si.snapshot_id = s.id),
                        0
                    ) AS documents,
                    COALESCE(
                        (SELECT COUNT(*) FROM {} sf WHERE sf.snapshot_id = s.id),
                        (SELECT COUNT(DISTINCT si.path) FROM {} si WHERE si.snapshot_id = s.id),
                        0
                    ) AS files
             FROM {} s
             WHERE s.id = $1
             LIMIT 1",
            self.table("snapshot_files"),
            self.table("snapshot_items"),
            self.table("snapshot_files"),
            self.table("snapshot_items"),
            self.table("snapshots"),
        );
        let Some(snapshot_row) = client.query_opt(&summary_query, &[&snapshot_id])? else {
            return Ok(None);
        };
        let snapshot = snapshot_record_from_row(snapshot_row);
        let collections = self.snapshot_collection_records(&mut client, snapshot_id)?;

        Ok(Some(BaselineSnapshotDetails { snapshot, collections }))
    }

    fn snapshot_collection_records(
        &self,
        client: &mut Client,
        snapshot_id: &str,
    ) -> Result<Vec<BaselineCollectionRecord>, SearchError> {
        let snapshot_files_query = format!(
            "SELECT collection,
                    COUNT(*) AS files,
                    SUM(document_count) AS documents
             FROM {}
             WHERE snapshot_id = $1
             GROUP BY collection
             ORDER BY collection",
            self.table("snapshot_files")
        );
        let rows = client.query(&snapshot_files_query, &[&snapshot_id])?;
        if !rows.is_empty() {
            return Ok(rows
                .into_iter()
                .map(|row| BaselineCollectionRecord {
                    collection: row.get("collection"),
                    files: row.get::<_, i64>("files") as usize,
                    documents: row.get::<_, i64>("documents") as usize,
                })
                .collect());
        }

        let legacy_query = format!(
            "SELECT collection,
                    COUNT(DISTINCT path) AS files,
                    COUNT(*) AS documents
             FROM {}
             WHERE snapshot_id = $1
             GROUP BY collection
             ORDER BY collection",
            self.table("snapshot_items")
        );
        Ok(client
            .query(&legacy_query, &[&snapshot_id])?
            .into_iter()
            .map(|row| BaselineCollectionRecord {
                collection: row.get("collection"),
                files: row.get::<_, i64>("files") as usize,
                documents: row.get::<_, i64>("documents") as usize,
            })
            .collect())
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
        let shared_query = format!(
            "SELECT sf.collection,
                    sf.path,
                    foi.symbol_name,
                    foi.kind,
                    foi.line_start,
                    foi.line_end,
                    co.text,
                    foi.content_hash
             FROM {} sf
             JOIN {} foi ON foi.file_object_id = sf.file_object_id
             JOIN {} co ON co.content_hash = foi.content_hash
             WHERE sf.snapshot_id = $1 AND sf.file_object_id IS NOT NULL
             ORDER BY sf.collection, sf.path, foi.ordinal",
            self.table("snapshot_files"),
            self.table("file_object_items"),
            self.table("content_objects")
        );
        let shared_rows = client.query(&shared_query, &[&snapshot.id.0])?;
        let shared_documents: Vec<IndexedDocument> =
            shared_rows.into_iter().map(indexed_document_from_row).collect();
        let shared_paths: HashSet<(String, String)> = shared_documents
            .iter()
            .map(|document| (document.collection.clone(), document.path.clone()))
            .collect();

        let legacy_query = format!(
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
        let legacy_documents = client
            .query(&legacy_query, &[&snapshot.id.0])?
            .into_iter()
            .map(indexed_document_from_row)
            .filter(|document| {
                !shared_paths.contains(&(document.collection.clone(), document.path.clone()))
            });

        let mut documents = shared_documents;
        documents.extend(legacy_documents);
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
        let delete_items =
            format!("DELETE FROM {} WHERE snapshot_id = $1", self.table("snapshot_items"));
        tx.execute(&delete_items, &[&snapshot.id.0])?;

        let insert_snapshot_file = format!(
            "INSERT INTO {} (
                snapshot_id, collection, path, file_fingerprint, document_count, file_object_id
             ) VALUES ($1, $2, $3, $4, $5, $6)",
            self.table("snapshot_files")
        );

        let mut stats = SnapshotPublishStats::default();
        for file_group in file_groups {
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

            if !try_insert_file_object(&mut tx, self, &file_object_id, &file_group)? {
                stats.reused_files += 1;
                stats.reused_documents += file_group.documents.len();
                continue;
            }

            stats.written_files += 1;
            stats.written_documents += file_group.documents.len();
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

fn indexed_document_from_row(row: Row) -> IndexedDocument {
    IndexedDocument {
        collection: row.get("collection"),
        path: row.get("path"),
        symbol_name: row.get("symbol_name"),
        kind: row.get("kind"),
        line_start: row.get::<_, i32>("line_start") as u32,
        line_end: row.get::<_, i32>("line_end") as u32,
        text: row.get("text"),
        content_hash: row.get("content_hash"),
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

fn snapshot_record_from_row(row: Row) -> BaselineSnapshotRecord {
    BaselineSnapshotRecord {
        snapshot_id: row.get("id"),
        corpus: row.get("corpus"),
        fingerprint: row.get("fingerprint"),
        parent_snapshot_id: row.get("parent_snapshot_id"),
        branch: row.get("branch"),
        commit: row.get("commit_sha"),
        created_at: row.get("created_at"),
        files: row.get::<_, i64>("files") as usize,
        documents: row.get::<_, i64>("documents") as usize,
    }
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
