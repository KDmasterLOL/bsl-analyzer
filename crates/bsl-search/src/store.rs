use crate::document::Document;
use crate::error::SearchError;
use code_chunk::Chunk;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct Store {
    conn: Connection,
    path: PathBuf,
}

/// The embeddings the vector index is built from (`(chunk_id, vector)` rows) paired with the
/// `embedding_generation` they were read at, as one consistent snapshot.
pub type EmbeddingsSnapshot = (i64, Vec<(i64, Vec<f32>)>);

/// Bumped whenever the embedding text composed by
/// `document::semantic_text_for_indexed_document` changes shape. Stored in the SQLite
/// `user_version` pragma; on mismatch the store clears file hashes so the next index
/// re-embeds everything, rather than mixing old- and new-format vectors in one space
/// (file-hash gating would otherwise keep stale-format embeddings indefinitely).
pub(crate) const EMBED_TEXT_VERSION: i64 = 1;

/// The structural version of the SQLite schema, recorded in the `meta` table — the
/// search-index counterpart to the call graph's `graph_db::SCHEMA_VERSION`. Bump this
/// whenever a table's shape changes in a way the additive `ALTER TABLE` migrations in
/// [`Store::init_schema`] cannot reconcile; on mismatch the derived cache is wiped and
/// rebuilt. Distinct from [`EMBED_TEXT_VERSION`], which only forces a soft re-embed and
/// leaves the schema intact. A pre-versioning database (no `meta` row) is treated as
/// already current — the additive migrations keep it compatible — so upgrading does not
/// trigger a needless full re-index.
const SCHEMA_VERSION: i64 = 1;

impl Store {
    pub fn open(path: &Path) -> Result<Self, SearchError> {
        let conn = Connection::open(path)?;
        let store = Self { conn, path: path.to_path_buf() };
        store.apply_pragmas()?;
        store.migrate_structural_schema()?;
        store.migrate_embed_text_version()?;
        Ok(store)
    }

    /// The database file this store was opened from — the anchor for the sibling persisted
    /// vector-index files (see [`crate::vector_persist`]).
    pub fn db_path(&self) -> &Path {
        &self.path
    }

    /// Connection-level pragmas. Set outside any transaction — `journal_mode` is a no-op
    /// inside one — so the WAL mode that makes [`Self::migrate_structural_schema`]
    /// crash-atomic is actually in force before that transaction runs.
    ///
    /// `busy_timeout` matters once two connections write the same database: the
    /// background embedding pass opens its own connection (WAL: many readers, one
    /// writer) while the overlay watcher keeps writing through the live engine. Without
    /// a timeout a writer that finds the WAL lock held returns `SQLITE_BUSY`
    /// immediately; with it SQLite retries internally for the configured window.
    fn apply_pragmas(&self) -> Result<(), SearchError> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 30000;
             PRAGMA foreign_keys = ON;",
        )?;
        Ok(())
    }

    /// Reconcile the structural schema in a single transaction: wipe a stale cache,
    /// (re)create the current tables, and stamp the version atomically. Under WAL a
    /// crash mid-reconcile rolls back to the prior consistent state, so the next open
    /// never sees a half-wiped database it would mistake for a pre-versioning one (whose
    /// data must be kept). Distinct from [`Self::migrate_embed_text_version`], a soft
    /// re-embed that leaves the schema intact.
    fn migrate_structural_schema(&self) -> Result<(), SearchError> {
        let tx = self.conn.unchecked_transaction()?;
        if let Some(stored) = Self::stored_schema_version(&tx)? {
            if stored != SCHEMA_VERSION {
                tracing::info!(
                    from = stored,
                    to = SCHEMA_VERSION,
                    "search index schema changed; wiping derived cache to rebuild"
                );
                Self::wipe_all_tables(&tx)?;
            }
        }
        Self::create_schema(&tx)?;
        Self::ensure_embedding_generation(&tx, &self.path)?;
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Guarantee the `embedding_generation` counter exists, and invalidate stale vector artifacts
    /// whenever it has to be (re)created. The counter is absent in exactly three cases — a fresh
    /// database, one just wiped by [`Self::wipe_all_tables`] above, or a corrupt one that lost the
    /// row — and in all of them a persisted index/sidecar cannot be trusted against the
    /// freshly-reset counter (the wipe drops the row via `DROP TABLE`, firing no trigger, so a
    /// surviving generation-0 sidecar would otherwise false-accept). So when the row is missing we
    /// remove the artifacts FIRST, fallibly: a sidecar that cannot be deleted aborts the migration
    /// before `tx.commit()` (the transaction rolls back) rather than leave an emptied/reset database
    /// next to a loadable sidecar. Seeding only after a successful removal keeps the counter and the
    /// on-disk artifacts consistent. A normal open (row present) skips all of this.
    fn ensure_embedding_generation(tx: &Connection, db_path: &Path) -> Result<(), SearchError> {
        if Self::read_embedding_generation(tx)? != Self::MISSING_GENERATION {
            return Ok(());
        }
        crate::vector_persist::remove_artifacts(db_path)?;
        tx.execute("INSERT INTO meta (key, value) VALUES ('embedding_generation', '0')", [])?;
        Ok(())
    }

    /// The structural schema version recorded in `meta`, or `None` for a fresh or
    /// pre-versioning database (no `meta` table, or no `schema_version` row).
    fn stored_schema_version(conn: &Connection) -> Result<Option<i64>, SearchError> {
        let has_meta = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
                [],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !has_meta {
            return Ok(None);
        }
        let version = conn
            .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .and_then(|v| v.parse().ok());
        Ok(version)
    }

    /// Drop every table so [`Self::create_schema`] can recreate the current structure.
    /// FTS5 virtual tables are dropped first so their shadow tables vanish before the
    /// generic enumeration runs (dropping a shadow table directly is an error). The
    /// `embedding_generation` triggers are dropped before any table: dropping the parent
    /// `files` table runs its FK `ON DELETE CASCADE` onto `chunks` (foreign keys are ON and
    /// the pragma cannot be toggled inside this transaction), which would otherwise fire
    /// `chunks_gen_del` against an already-dropped `meta` table and abort the wipe.
    fn wipe_all_tables(conn: &Connection) -> Result<(), SearchError> {
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS chunks_gen_ins;
             DROP TRIGGER IF EXISTS chunks_gen_upd;
             DROP TRIGGER IF EXISTS chunks_gen_del;
             DROP TRIGGER IF EXISTS files_gen_del;
             DROP TABLE IF EXISTS chunks_fts; DROP TABLE IF EXISTS overlay_chunks_fts;",
        )?;
        let names: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for name in names {
            conn.execute(&format!("DROP TABLE IF EXISTS \"{name}\""), [])?;
        }
        Ok(())
    }

    /// Force a full re-embed when the embedding-text format has changed since this
    /// database was built (see [`EMBED_TEXT_VERSION`]). A fresh database just records
    /// the current version.
    fn migrate_embed_text_version(&self) -> Result<(), SearchError> {
        let version: i64 = self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != EMBED_TEXT_VERSION {
            let cleared = self.conn.execute("UPDATE files SET hash = zeroblob(0)", [])?;
            self.conn.pragma_update(None, "user_version", EMBED_TEXT_VERSION)?;
            if cleared > 0 {
                tracing::info!(
                    cleared,
                    from = version,
                    to = EMBED_TEXT_VERSION,
                    "embed-text format changed; cleared file hashes to force re-embed"
                );
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self, SearchError> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn, path: PathBuf::from(":memory:") };
        store.apply_pragmas()?;
        store.migrate_structural_schema()?;
        Ok(store)
    }

    fn create_schema(conn: &Connection) -> Result<(), SearchError> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS files (
                id         INTEGER PRIMARY KEY,
                path       TEXT    NOT NULL UNIQUE,
                hash       BLOB   NOT NULL,
                indexed_at INTEGER NOT NULL,
                collection TEXT    NOT NULL DEFAULT 'code'
            );

            CREATE TABLE IF NOT EXISTS chunks (
                id          INTEGER PRIMARY KEY,
                file_id     INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                kind        TEXT    NOT NULL,
                symbol_name TEXT    NOT NULL,
                is_export   INTEGER NOT NULL DEFAULT 0,
                annotations TEXT,
                line_start  INTEGER NOT NULL,
                line_end    INTEGER NOT NULL,
                text        TEXT    NOT NULL,
                embedding   BLOB,
                graph_context TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_file
                ON chunks(file_id);

            CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                symbol_name,
                text,
                tokenize='unicode61'
            );
            ",
        )?;

        let _ = conn
            .execute("ALTER TABLE files ADD COLUMN collection TEXT NOT NULL DEFAULT 'code'", []);
        // Idempotent column add for databases created before graph-enriched embeddings;
        // the error when it already exists is intentionally ignored.
        let _ = conn.execute("ALTER TABLE chunks ADD COLUMN graph_context TEXT", []);

        conn.execute_batch(
            "
            -- Baseline manifest metadata for workspace code.
            -- Stores the selected snapshot identity and manifest snapshot.
            CREATE TABLE IF NOT EXISTS baseline_manifest (
                id              INTEGER PRIMARY KEY CHECK (id = 1),
                snapshot_id     TEXT    NOT NULL,
                fingerprint     TEXT,
                manifest_files  INTEGER NOT NULL DEFAULT 0,
                fetched_at      TEXT    NOT NULL
            );

            CREATE TABLE IF NOT EXISTS baseline_manifest_files (
                collection       TEXT    NOT NULL DEFAULT 'code',
                path             TEXT    NOT NULL,
                file_fingerprint TEXT    NOT NULL,
                PRIMARY KEY (collection, path)
            );

            -- Tombstones for deleted baseline files.
            -- When a baseline file is deleted locally, its path is recorded here
            -- so the merge layer can hide the baseline hit.
            CREATE TABLE IF NOT EXISTS overlay_tombstones (
                path       TEXT    NOT NULL UNIQUE,
                collection TEXT    NOT NULL DEFAULT 'code',
                deleted_at TEXT    NOT NULL
            );

            -- Overlay files: files that are locally modified or new relative to
            -- the baseline manifest. These are separate from the main `files`
            -- table so baseline rows never appear in local storage.
            CREATE TABLE IF NOT EXISTS overlay_files (
                id         INTEGER PRIMARY KEY,
                path       TEXT    NOT NULL UNIQUE,
                hash       BLOB   NOT NULL,
                indexed_at INTEGER NOT NULL,
                collection TEXT    NOT NULL DEFAULT 'code'
            );

            -- Overlay chunks: lexical chunks belonging to overlay files.
            CREATE TABLE IF NOT EXISTS overlay_chunks (
                id          INTEGER PRIMARY KEY,
                file_id     INTEGER NOT NULL REFERENCES overlay_files(id) ON DELETE CASCADE,
                kind        TEXT    NOT NULL,
                symbol_name TEXT    NOT NULL,
                is_export   INTEGER NOT NULL DEFAULT 0,
                annotations TEXT,
                line_start  INTEGER NOT NULL,
                line_end    INTEGER NOT NULL,
                text        TEXT    NOT NULL,
                embedding   BLOB
            );

            CREATE INDEX IF NOT EXISTS idx_overlay_chunks_file
                ON overlay_chunks(file_id);

            -- FTS index for overlay chunks.
            CREATE VIRTUAL TABLE IF NOT EXISTS overlay_chunks_fts USING fts5(
                symbol_name,
                text,
                tokenize='unicode61'
            );

            -- Persisted overlay fingerprint cache: avoids re-reading and
            -- re-hashing unchanged files on MCP server restart.
            CREATE TABLE IF NOT EXISTS overlay_fingerprint_cache (
                path                TEXT NOT NULL PRIMARY KEY,
                collection          TEXT NOT NULL DEFAULT 'code',
                file_size           INTEGER NOT NULL,
                file_mtime_secs     INTEGER NOT NULL,
                file_mtime_nanos    INTEGER NOT NULL,
                content_fingerprint TEXT NOT NULL,
                manifest_snapshot_id TEXT NOT NULL
            );

            -- Persisted overlay embedding cache: avoids re-embedding
            -- unchanged overlay chunks on MCP server restart.
            CREATE TABLE IF NOT EXISTS overlay_embedding_cache (
                content_hash TEXT NOT NULL PRIMARY KEY,
                model_id     TEXT NOT NULL,
                dimension    INTEGER NOT NULL,
                embedding    BLOB NOT NULL
            );
            ",
        )?;

        Self::create_embedding_generation_triggers(conn)?;

        Ok(())
    }

    /// A monotonic counter bumped by triggers on every write that can change the set of
    /// `(chunks.id, chunks.embedding)` rows the vector index is built from. The persisted index's
    /// sidecar records the generation it was built at, so [`crate::vector_persist::try_load`] can
    /// confirm "nothing changed since" with a single-row read instead of scanning every embedding
    /// BLOB (see `embedding_generation` / `load_all_embeddings_with_generation`).
    ///
    /// Coverage (auditable contract): `insert_chunk` and all `reindex_*` inserts fire
    /// `chunks_gen_ins`; the reindex delete-phases and `delete_chunks_for_file` fire `chunks_gen_del`;
    /// `set_chunk_embedding` fires `chunks_gen_upd`; `remove_file` / `clear_collection` delete `files`
    /// rows (and cascade to `chunks`) and fire `files_gen_del`. The `files_gen_del` trigger makes the
    /// counter advance on a file removal regardless of the `recursive_triggers` pragma, so we never
    /// depend on whether an FK cascade fires the `chunks` delete trigger. `upsert_file`,
    /// `clear_file_hashes`, and `migrate_embed_text_version` touch only `files` metadata, not the
    /// indexed embedding set, and intentionally do not bump. Over-bumping is safe (only forces a
    /// rebuild); under-bumping would serve a stale index, so the triggers err toward bumping. A
    /// destructive `wipe_all_tables` resets the counter (DROP TABLE fires no trigger); the counter
    /// row itself is (re)seeded by [`Self::ensure_embedding_generation`], which deletes any stale
    /// persisted artifacts whenever it has to recreate the row so the reset can never match a
    /// pre-wipe sidecar. These triggers reference the `meta` row but do not create it.
    fn create_embedding_generation_triggers(conn: &Connection) -> Result<(), SearchError> {
        conn.execute_batch(
            "
            CREATE TRIGGER IF NOT EXISTS chunks_gen_ins AFTER INSERT ON chunks BEGIN
                UPDATE meta SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)
                WHERE key = 'embedding_generation';
            END;
            CREATE TRIGGER IF NOT EXISTS chunks_gen_upd AFTER UPDATE OF embedding ON chunks BEGIN
                UPDATE meta SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)
                WHERE key = 'embedding_generation';
            END;
            CREATE TRIGGER IF NOT EXISTS chunks_gen_del AFTER DELETE ON chunks BEGIN
                UPDATE meta SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)
                WHERE key = 'embedding_generation';
            END;
            CREATE TRIGGER IF NOT EXISTS files_gen_del AFTER DELETE ON files BEGIN
                UPDATE meta SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)
                WHERE key = 'embedding_generation';
            END;
            ",
        )?;
        Ok(())
    }

    pub fn file_hash(&self, path: &str) -> Result<Option<Vec<u8>>, SearchError> {
        let hash = self
            .conn
            .query_row("SELECT hash FROM files WHERE path = ?1", params![path], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .optional()?;
        Ok(hash)
    }

    pub fn upsert_file(
        &self,
        path: &str,
        hash: &[u8],
        collection: &str,
    ) -> Result<i64, SearchError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.conn.execute(
            "INSERT INTO files (path, hash, indexed_at, collection)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET hash = ?2, indexed_at = ?3, collection = ?4",
            params![path, hash, now, collection],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn remove_file(&self, path: &str) -> Result<(), SearchError> {
        self.conn.execute(
            "DELETE FROM chunks_fts WHERE rowid IN (
                 SELECT c.id FROM chunks c
                 JOIN files f ON f.id = c.file_id
                 WHERE f.path = ?1
             )",
            params![path],
        )?;
        self.conn.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        Ok(())
    }

    pub fn delete_chunks_for_file(&self, file_id: i64) -> Result<(), SearchError> {
        self.conn.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id])?;
        Ok(())
    }

    pub fn insert_chunk(
        &self,
        file_id: i64,
        chunk: &Chunk,
        embedding: Option<&[f32]>,
    ) -> Result<i64, SearchError> {
        let kind_str = match chunk.kind {
            code_chunk::ChunkKind::ModuleHeader => "header",
            code_chunk::ChunkKind::Procedure => "procedure",
            code_chunk::ChunkKind::Function => "function",
        };
        let annotations =
            if chunk.annotations.is_empty() { None } else { Some(chunk.annotations.join(",")) };
        let embedding_blob: Option<Vec<u8>> =
            embedding.map(|e| e.iter().flat_map(|f| f.to_le_bytes()).collect());

        self.conn.execute(
            "INSERT INTO chunks (file_id, kind, symbol_name, is_export, annotations,
                                 line_start, line_end, text, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                file_id,
                kind_str,
                chunk.name,
                chunk.is_export as i32,
                annotations,
                chunk.line_start,
                chunk.line_end,
                chunk.text,
                embedding_blob,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn reindex_file(
        &mut self,
        path: &str,
        hash: &[u8],
        chunks: &[Chunk],
        embeddings: Option<&[Vec<f32>]>,
    ) -> Result<i64, SearchError> {
        self.reindex_file_in_collection(path, hash, "code", chunks, embeddings, None)
    }

    /// As [`Self::reindex_file`], but persists each chunk's graph context (parallel to
    /// `chunks`) so a later reconstruction re-embeds with the same enriched text.
    pub fn reindex_file_with_context(
        &mut self,
        path: &str,
        hash: &[u8],
        chunks: &[Chunk],
        embeddings: Option<&[Vec<f32>]>,
        graph_contexts: Option<&[Option<String>]>,
    ) -> Result<i64, SearchError> {
        self.reindex_file_in_collection(path, hash, "code", chunks, embeddings, graph_contexts)
    }

    pub fn reindex_file_in_collection(
        &mut self,
        path: &str,
        hash: &[u8],
        collection: &str,
        chunks: &[Chunk],
        embeddings: Option<&[Vec<f32>]>,
        graph_contexts: Option<&[Option<String>]>,
    ) -> Result<i64, SearchError> {
        let tx = self.conn.transaction()?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        tx.execute(
            "INSERT INTO files (path, hash, indexed_at, collection)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET hash = ?2, indexed_at = ?3, collection = ?4",
            params![path, hash, now, collection],
        )?;
        let file_id: i64 =
            tx.query_row("SELECT id FROM files WHERE path = ?1", params![path], |row| row.get(0))?;

        tx.execute(
            "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![file_id],
        )?;

        tx.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id])?;

        {
            let mut stmt = tx.prepare(
                "INSERT INTO chunks (file_id, kind, symbol_name, is_export, annotations,
                                     line_start, line_end, text, embedding, graph_context)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            let mut fts_stmt =
                tx.prepare("INSERT INTO chunks_fts(rowid, symbol_name, text) VALUES (?1, ?2, ?3)")?;

            for (i, chunk) in chunks.iter().enumerate() {
                let kind_str = match chunk.kind {
                    code_chunk::ChunkKind::ModuleHeader => "header",
                    code_chunk::ChunkKind::Procedure => "procedure",
                    code_chunk::ChunkKind::Function => "function",
                };
                let annotations = if chunk.annotations.is_empty() {
                    None
                } else {
                    Some(chunk.annotations.join(","))
                };
                let embedding_blob: Option<Vec<u8>> = embeddings
                    .and_then(|embs| embs.get(i))
                    .map(|e| e.iter().flat_map(|f| f.to_le_bytes()).collect());
                let graph_context: Option<&str> =
                    graph_contexts.and_then(|gc| gc.get(i)).and_then(|g| g.as_deref());

                stmt.execute(params![
                    file_id,
                    kind_str,
                    chunk.name,
                    chunk.is_export as i32,
                    annotations,
                    chunk.line_start,
                    chunk.line_end,
                    chunk.text,
                    embedding_blob,
                    graph_context,
                ])?;

                let chunk_id = tx.last_insert_rowid();
                fts_stmt.execute(params![chunk_id, chunk.name, chunk.text])?;
            }
        }

        tx.commit()?;
        Ok(file_id)
    }

    pub fn reindex_documents(
        &mut self,
        collection: &str,
        virtual_path: &str,
        hash: &[u8],
        documents: &[Document],
        embeddings: Option<&[Vec<f32>]>,
    ) -> Result<i64, SearchError> {
        let tx = self.conn.transaction()?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        tx.execute(
            "INSERT INTO files (path, hash, indexed_at, collection)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET hash = ?2, indexed_at = ?3, collection = ?4",
            params![virtual_path, hash, now, collection],
        )?;
        let file_id: i64 =
            tx.query_row("SELECT id FROM files WHERE path = ?1", params![virtual_path], |row| {
                row.get(0)
            })?;

        tx.execute(
            "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![file_id],
        )?;
        tx.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id])?;

        {
            let mut stmt = tx.prepare(
                "INSERT INTO chunks (file_id, kind, symbol_name, is_export, annotations,
                                     line_start, line_end, text, embedding)
                 VALUES (?1, ?2, ?3, 0, NULL, 0, 0, ?4, ?5)",
            )?;
            let mut fts_stmt =
                tx.prepare("INSERT INTO chunks_fts(rowid, symbol_name, text) VALUES (?1, ?2, ?3)")?;

            for (i, doc) in documents.iter().enumerate() {
                let embedding_blob: Option<Vec<u8>> = embeddings
                    .and_then(|embs| embs.get(i))
                    .map(|e| e.iter().flat_map(|f| f.to_le_bytes()).collect());

                stmt.execute(params![file_id, doc.kind, doc.title, doc.body, embedding_blob])?;

                let chunk_id = tx.last_insert_rowid();
                fts_stmt.execute(params![chunk_id, doc.title, doc.body])?;
            }
        }

        tx.commit()?;
        Ok(file_id)
    }

    pub fn reindex_indexed_documents_in_collection(
        &mut self,
        path: &str,
        hash: &[u8],
        collection: &str,
        documents: &[crate::IndexedDocument],
        embeddings: Option<&[Vec<f32>]>,
    ) -> Result<i64, SearchError> {
        let tx = self.conn.transaction()?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        tx.execute(
            "INSERT INTO files (path, hash, indexed_at, collection)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET hash = ?2, indexed_at = ?3, collection = ?4",
            params![path, hash, now, collection],
        )?;
        let file_id: i64 =
            tx.query_row("SELECT id FROM files WHERE path = ?1", params![path], |row| row.get(0))?;

        tx.execute(
            "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![file_id],
        )?;
        tx.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id])?;

        {
            let mut stmt = tx.prepare(
                "INSERT INTO chunks (file_id, kind, symbol_name, is_export, annotations,
                                     line_start, line_end, text, embedding)
                 VALUES (?1, ?2, ?3, 0, NULL, ?4, ?5, ?6, ?7)",
            )?;
            let mut fts_stmt =
                tx.prepare("INSERT INTO chunks_fts(rowid, symbol_name, text) VALUES (?1, ?2, ?3)")?;

            for (idx, document) in documents.iter().enumerate() {
                let embedding_blob: Option<Vec<u8>> = embeddings
                    .and_then(|embs| embs.get(idx))
                    .map(|embedding| embedding.iter().flat_map(|f| f.to_le_bytes()).collect());

                stmt.execute(params![
                    file_id,
                    document.kind,
                    document.symbol_name,
                    document.line_start,
                    document.line_end,
                    document.text,
                    embedding_blob,
                ])?;

                let chunk_id = tx.last_insert_rowid();
                fts_stmt.execute(params![chunk_id, document.symbol_name, document.text])?;
            }
        }

        tx.commit()?;
        Ok(file_id)
    }

    pub fn load_all_embeddings(&self, dim: usize) -> Result<Vec<(i64, Vec<f32>)>, SearchError> {
        Self::read_all_embeddings(&self.conn, dim)
    }

    /// The embeddings the vector index is built from, paired with the `embedding_generation` they
    /// were read at — both captured in one read transaction so the generation exactly describes
    /// this snapshot of the data. The persisted index records this generation; a later cold start
    /// that sees the same generation can trust the index without re-reading every BLOB (a concurrent
    /// writer that bumps the generation during the long HNSW build only makes a later load rebuild).
    pub fn load_all_embeddings_with_generation(
        &self,
        dim: usize,
    ) -> Result<EmbeddingsSnapshot, SearchError> {
        let tx = self.conn.unchecked_transaction()?;
        let generation = Self::read_embedding_generation(&tx)?;
        let data = Self::read_all_embeddings(&tx, dim)?;
        // Read-only: drop the transaction without committing.
        Ok((generation, data))
    }

    /// The current `embedding_generation` counter (O(1) single-row read). `Store::open` always
    /// seeds the row, so a missing row means corrupt/foreign state; it maps to `-1`, a sentinel
    /// that can never equal a real generation (which is `>= 0`, since a fresh build can stamp 0),
    /// so a stale gen-0 sidecar cannot validate against a database whose counter has gone missing.
    pub fn embedding_generation(&self) -> Result<i64, SearchError> {
        Self::read_embedding_generation(&self.conn)
    }

    /// Missing-row sentinel (see [`Self::embedding_generation`]): distinct from every persisted
    /// generation so it never produces a false-accept.
    const MISSING_GENERATION: i64 = -1;

    fn read_embedding_generation(conn: &Connection) -> Result<i64, SearchError> {
        let generation = conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'embedding_generation'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(Self::MISSING_GENERATION);
        Ok(generation)
    }

    fn read_all_embeddings(
        conn: &Connection,
        dim: usize,
    ) -> Result<Vec<(i64, Vec<f32>)>, SearchError> {
        let mut stmt =
            conn.prepare("SELECT id, embedding FROM chunks WHERE embedding IS NOT NULL")?;

        let rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((id, blob))
        })?;

        let mut result = Vec::new();
        for row in rows {
            let (id, blob) = row?;
            if blob.len() == dim * 4 {
                let embedding: Vec<f32> = blob
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();
                result.push((id, embedding));
            }
        }
        Ok(result)
    }

    pub fn chunk_by_id(&self, chunk_id: i64) -> Result<Option<ChunkInfo>, SearchError> {
        let info = self
            .conn
            .query_row(
                "SELECT c.kind, c.symbol_name, c.line_start, c.line_end, c.text,
                        c.annotations, c.is_export, f.path, f.collection
                 FROM chunks c
                 JOIN files f ON f.id = c.file_id
                 WHERE c.id = ?1",
                params![chunk_id],
                |row| {
                    Ok(ChunkInfo {
                        file_path: row.get(7)?,
                        collection: row.get(8)?,
                        kind: row.get(0)?,
                        symbol_name: row.get(1)?,
                        line_start: row.get(2)?,
                        line_end: row.get(3)?,
                        text: row.get(4)?,
                        annotations: row.get::<_, Option<String>>(5)?,
                        is_export: row.get::<_, i32>(6)? != 0,
                    })
                },
            )
            .optional()?;
        Ok(info)
    }

    pub fn all_files(&self) -> Result<Vec<(String, Vec<u8>)>, SearchError> {
        let mut stmt = self.conn.prepare("SELECT path, hash FROM files")?;
        let rows =
            stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn all_files_in_collection(
        &self,
        collection: &str,
    ) -> Result<Vec<(String, Vec<u8>)>, SearchError> {
        let mut stmt = self.conn.prepare("SELECT path, hash FROM files WHERE collection = ?1")?;
        let rows = stmt.query_map(params![collection], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn clear_collection(&self, collection: &str) -> Result<(), SearchError> {
        self.conn.execute(
            "DELETE FROM chunks_fts WHERE rowid IN (
                 SELECT c.id FROM chunks c
                 JOIN files f ON f.id = c.file_id
                 WHERE f.collection = ?1
             )",
            params![collection],
        )?;
        self.conn.execute("DELETE FROM files WHERE collection = ?1", params![collection])?;
        Ok(())
    }

    pub fn chunk_count(&self) -> Result<usize, SearchError> {
        let count: i64 =
            self.conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn load_indexed_documents(
        &self,
        collection: Option<&str>,
    ) -> Result<Vec<crate::IndexedDocument>, SearchError> {
        let query = if collection.is_some() {
            "SELECT f.collection, f.path, c.symbol_name, c.kind, c.line_start, c.line_end, c.text,
                    c.graph_context
             FROM chunks c
             JOIN files f ON f.id = c.file_id
             WHERE f.collection = ?1
             ORDER BY f.collection, f.path, c.line_start, c.line_end, c.symbol_name"
        } else {
            "SELECT f.collection, f.path, c.symbol_name, c.kind, c.line_start, c.line_end, c.text,
                    c.graph_context
             FROM chunks c
             JOIN files f ON f.id = c.file_id
             ORDER BY f.collection, f.path, c.line_start, c.line_end, c.symbol_name"
        };

        let mut stmt = self.conn.prepare(query)?;
        let rows = if let Some(collection) = collection {
            stmt.query_map(params![collection], |row| {
                let text: String = row.get(6)?;
                Ok(crate::IndexedDocument {
                    collection: row.get(0)?,
                    path: row.get(1)?,
                    symbol_name: row.get(2)?,
                    kind: row.get(3)?,
                    line_start: row.get::<_, i64>(4)? as u32,
                    line_end: row.get::<_, i64>(5)? as u32,
                    content_hash: blake3::hash(text.as_bytes()).to_hex().to_string(),
                    text,
                    graph_context: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([], |row| {
                let text: String = row.get(6)?;
                Ok(crate::IndexedDocument {
                    collection: row.get(0)?,
                    path: row.get(1)?,
                    symbol_name: row.get(2)?,
                    kind: row.get(3)?,
                    line_start: row.get::<_, i64>(4)? as u32,
                    line_end: row.get::<_, i64>(5)? as u32,
                    content_hash: blake3::hash(text.as_bytes()).to_hex().to_string(),
                    text,
                    graph_context: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        };

        Ok(rows)
    }

    /// Chunks in `collection` whose embedding has not been computed yet, each paired
    /// with its row id. Powers the fused cold-build's separate embedding phase: the
    /// graph pass writes chunk text + FTS + graph context with a NULL embedding, then
    /// this lists exactly what still needs a vector — so embedding stays decoupled
    /// from the graph build's lifecycle.
    pub fn load_pending_embedding_documents(
        &self,
        collection: &str,
    ) -> Result<Vec<(i64, crate::IndexedDocument)>, SearchError> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, f.collection, f.path, c.symbol_name, c.kind, c.line_start, c.line_end,
                    c.text, c.graph_context
             FROM chunks c
             JOIN files f ON f.id = c.file_id
             WHERE f.collection = ?1 AND c.embedding IS NULL
             ORDER BY f.path, c.line_start, c.line_end, c.symbol_name",
        )?;
        let rows = stmt
            .query_map(params![collection], |row| {
                let id: i64 = row.get(0)?;
                let text: String = row.get(7)?;
                Ok((
                    id,
                    crate::IndexedDocument {
                        collection: row.get(1)?,
                        path: row.get(2)?,
                        symbol_name: row.get(3)?,
                        kind: row.get(4)?,
                        line_start: row.get::<_, i64>(5)? as u32,
                        line_end: row.get::<_, i64>(6)? as u32,
                        content_hash: blake3::hash(text.as_bytes()).to_hex().to_string(),
                        text,
                        graph_context: row.get(8)?,
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Set one chunk's embedding by row id, leaving its text/FTS/context untouched —
    /// the write half of the fused build's embedding phase.
    pub fn set_chunk_embedding(&self, chunk_id: i64, embedding: &[f32]) -> Result<(), SearchError> {
        let blob: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        self.conn
            .execute("UPDATE chunks SET embedding = ?2 WHERE id = ?1", params![chunk_id, blob])?;
        Ok(())
    }

    pub fn text_search(
        &self,
        query: &str,
        limit: usize,
        collection: Option<&str>,
    ) -> Result<Vec<TextSearchResult>, SearchError> {
        let Some(match_query) = crate::lexical::fts5_match_query(query) else {
            return Ok(Vec::new());
        };
        let results = if let Some(coll) = collection {
            let mut stmt = self.conn.prepare(
                "SELECT chunks_fts.rowid, chunks_fts.rank
                 FROM chunks_fts
                 JOIN chunks c ON c.id = chunks_fts.rowid
                 JOIN files f ON f.id = c.file_id
                 WHERE chunks_fts MATCH ?1 AND f.collection = ?2
                 ORDER BY chunks_fts.rank
                 LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![match_query, coll, limit as i64], |row| {
                Ok(TextSearchResult { chunk_id: row.get(0)?, rank: row.get(1)? })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT rowid, rank
                 FROM chunks_fts
                 WHERE chunks_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![match_query, limit as i64], |row| {
                Ok(TextSearchResult { chunk_id: row.get(0)?, rank: row.get(1)? })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        Ok(results)
    }

    pub fn rebuild_fts(&self) -> Result<(), SearchError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM chunks_fts", [])?;
        tx.execute(
            "INSERT INTO chunks_fts(rowid, symbol_name, text)
             SELECT id, symbol_name, text FROM chunks",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn fts_count(&self) -> Result<usize, SearchError> {
        let count: i64 =
            self.conn.query_row("SELECT COUNT(*) FROM chunks_fts", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn file_count(&self) -> Result<usize, SearchError> {
        let count: i64 = self.conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn embedding_count_by_collection(&self, collection: &str) -> Result<usize, SearchError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM chunks c
             JOIN files f ON f.id = c.file_id
             WHERE c.embedding IS NOT NULL AND f.collection = ?1",
            params![collection],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn clear_file_hashes(&self, collection: &str) -> Result<usize, SearchError> {
        let count = self.conn.execute(
            "UPDATE files SET hash = zeroblob(0) WHERE collection = ?1",
            params![collection],
        )?;
        Ok(count)
    }

    pub fn clear_file_hashes_without_embeddings(
        &self,
        collection: &str,
    ) -> Result<usize, SearchError> {
        // Clear the skip hash for any file that has even one un-embedded chunk, not
        // only files with zero embeddings. A partially embedded file (some chunks
        // vectored, some still NULL — e.g. a build interrupted mid-corpus, or the fused
        // cold-build's embedding phase failing after the chunks were written) must be
        // re-indexed in full on the next run; the previous `NOT IN (… IS NOT NULL)`
        // predicate kept such a file's hash and skipped it forever.
        let count = self.conn.execute(
            "UPDATE files SET hash = zeroblob(0)
             WHERE collection = ?1
               AND id IN (
                   SELECT DISTINCT file_id FROM chunks WHERE embedding IS NULL
               )",
            params![collection],
        )?;
        Ok(count)
    }

    pub fn upsert_baseline_manifest(
        &self,
        snapshot_id: &str,
        fingerprint: Option<&str>,
        manifest_files: usize,
    ) -> Result<(), SearchError> {
        let fetched_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.conn.execute(
            "INSERT INTO baseline_manifest (id, snapshot_id, fingerprint, manifest_files, fetched_at)
             VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                 snapshot_id = ?1,
                 fingerprint = ?2,
                 manifest_files = ?3,
                 fetched_at = ?4",
            params![snapshot_id, fingerprint, manifest_files as i64, fetched_at.to_string()],
        )?;
        Ok(())
    }

    pub fn save_baseline_manifest(
        &self,
        manifest: &crate::WorkspaceBaselineManifest,
    ) -> Result<(), SearchError> {
        let fetched_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO baseline_manifest (id, snapshot_id, fingerprint, manifest_files, fetched_at)
             VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                 snapshot_id = ?1,
                 fingerprint = ?2,
                 manifest_files = ?3,
                 fetched_at = ?4",
            params![
                manifest.snapshot_id,
                manifest.snapshot_fingerprint,
                manifest.files.len() as i64,
                fetched_at.to_string()
            ],
        )?;
        tx.execute("DELETE FROM baseline_manifest_files", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO baseline_manifest_files (collection, path, file_fingerprint)
                 VALUES (?1, ?2, ?3)",
            )?;
            for file in &manifest.files {
                stmt.execute(params![file.collection, file.path, file.file_fingerprint])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_baseline_manifest(&self) -> Result<Option<BaselineManifestRecord>, SearchError> {
        let record = self
            .conn
            .query_row(
                "SELECT snapshot_id, fingerprint, manifest_files, fetched_at
             FROM baseline_manifest WHERE id = 1",
                [],
                |row| {
                    Ok(BaselineManifestRecord {
                        snapshot_id: row.get(0)?,
                        fingerprint: row.get(1)?,
                        manifest_files: row.get::<_, i64>(2)? as usize,
                        fetched_at: row.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(record)
    }

    pub fn load_baseline_manifest_fingerprints(
        &self,
        collection: &str,
    ) -> Result<Option<HashMap<String, String>>, SearchError> {
        let has_manifest = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM baseline_manifest WHERE id = 1)",
            [],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !has_manifest {
            return Ok(None);
        }

        let mut stmt = self.conn.prepare(
            "SELECT path, file_fingerprint
             FROM baseline_manifest_files
             WHERE collection = ?1",
        )?;
        let rows = stmt.query_map(params![collection], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut fingerprints = HashMap::new();
        for row in rows {
            let (path, fingerprint) = row?;
            fingerprints.insert(path, fingerprint);
        }
        Ok(Some(fingerprints))
    }

    pub fn clear_baseline_manifest(&self) -> Result<(), SearchError> {
        self.conn.execute("DELETE FROM baseline_manifest_files", [])?;
        self.conn.execute("DELETE FROM baseline_manifest WHERE id = 1", [])?;
        Ok(())
    }

    pub fn insert_overlay_tombstone(
        &self,
        path: &str,
        collection: &str,
    ) -> Result<(), SearchError> {
        let deleted_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.conn.execute(
            "INSERT INTO overlay_tombstones (path, collection, deleted_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET collection = ?2, deleted_at = ?3",
            params![path, collection, deleted_at.to_string()],
        )?;
        Ok(())
    }

    pub fn remove_overlay_tombstone(&self, path: &str) -> Result<(), SearchError> {
        self.conn.execute("DELETE FROM overlay_tombstones WHERE path = ?1", params![path])?;
        Ok(())
    }

    pub fn overlay_tombstone_paths(
        &self,
        collection: &str,
    ) -> Result<HashSet<String>, SearchError> {
        let mut stmt =
            self.conn.prepare("SELECT path FROM overlay_tombstones WHERE collection = ?1")?;
        let rows = stmt.query_map(params![collection], |row| row.get::<_, String>(0))?;
        let mut paths = HashSet::new();
        for row in rows {
            paths.insert(row?);
        }
        Ok(paths)
    }

    pub fn clear_overlay_tombstones(&self, collection: &str) -> Result<(), SearchError> {
        self.conn
            .execute("DELETE FROM overlay_tombstones WHERE collection = ?1", params![collection])?;
        Ok(())
    }

    pub fn upsert_overlay_file_with_chunks(
        &mut self,
        path: &str,
        hash: &[u8],
        collection: &str,
        chunks: &[Chunk],
        embeddings: Option<&[Vec<f32>]>,
    ) -> Result<i64, SearchError> {
        let tx = self.conn.transaction()?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        tx.execute(
            "INSERT INTO overlay_files (path, hash, indexed_at, collection)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET hash = ?2, indexed_at = ?3, collection = ?4",
            params![path, hash, now, collection],
        )?;
        let file_id: i64 =
            tx.query_row("SELECT id FROM overlay_files WHERE path = ?1", params![path], |row| {
                row.get(0)
            })?;

        tx.execute(
            "DELETE FROM overlay_chunks_fts WHERE rowid IN (SELECT id FROM overlay_chunks WHERE file_id = ?1)",
            params![file_id],
        )?;
        tx.execute("DELETE FROM overlay_chunks WHERE file_id = ?1", params![file_id])?;

        {
            let mut stmt = tx.prepare(
                "INSERT INTO overlay_chunks (file_id, kind, symbol_name, is_export, annotations,
                     line_start, line_end, text, embedding)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            let mut fts_stmt = tx.prepare(
                "INSERT INTO overlay_chunks_fts(rowid, symbol_name, text) VALUES (?1, ?2, ?3)",
            )?;

            for (i, chunk) in chunks.iter().enumerate() {
                let kind_str = match chunk.kind {
                    code_chunk::ChunkKind::ModuleHeader => "header",
                    code_chunk::ChunkKind::Procedure => "procedure",
                    code_chunk::ChunkKind::Function => "function",
                };
                let annotations = if chunk.annotations.is_empty() {
                    None
                } else {
                    Some(chunk.annotations.join(","))
                };
                let embedding_blob: Option<Vec<u8>> = embeddings
                    .and_then(|embs| embs.get(i))
                    .map(|e| e.iter().flat_map(|f| f.to_le_bytes()).collect());

                stmt.execute(params![
                    file_id,
                    kind_str,
                    chunk.name,
                    chunk.is_export as i32,
                    annotations,
                    chunk.line_start,
                    chunk.line_end,
                    chunk.text,
                    embedding_blob,
                ])?;

                let chunk_id = tx.last_insert_rowid();
                fts_stmt.execute(params![chunk_id, chunk.name, chunk.text])?;
            }
        }

        tx.commit()?;
        Ok(file_id)
    }

    pub fn remove_overlay_file(&self, path: &str) -> Result<(), SearchError> {
        self.conn.execute(
            "DELETE FROM overlay_chunks_fts WHERE rowid IN (
                 SELECT c.id FROM overlay_chunks c
                 JOIN overlay_files f ON f.id = c.file_id
                 WHERE f.path = ?1
             )",
            params![path],
        )?;
        self.conn.execute("DELETE FROM overlay_files WHERE path = ?1", params![path])?;
        Ok(())
    }

    pub fn overlay_text_search(
        &self,
        query: &str,
        limit: usize,
        collection: Option<&str>,
    ) -> Result<Vec<TextSearchResult>, SearchError> {
        let Some(match_query) = crate::lexical::fts5_match_query(query) else {
            return Ok(Vec::new());
        };
        let results = if let Some(coll) = collection {
            let mut stmt = self.conn.prepare(
                "SELECT overlay_chunks_fts.rowid, overlay_chunks_fts.rank
                 FROM overlay_chunks_fts
                 JOIN overlay_chunks c ON c.id = overlay_chunks_fts.rowid
                 JOIN overlay_files f ON f.id = c.file_id
                 WHERE overlay_chunks_fts MATCH ?1 AND f.collection = ?2
                 ORDER BY overlay_chunks_fts.rank
                 LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![match_query, coll, limit as i64], |row| {
                Ok(TextSearchResult { chunk_id: row.get(0)?, rank: row.get(1)? })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT rowid, rank
                 FROM overlay_chunks_fts
                 WHERE overlay_chunks_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![match_query, limit as i64], |row| {
                Ok(TextSearchResult { chunk_id: row.get(0)?, rank: row.get(1)? })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        Ok(results)
    }

    pub fn overlay_chunks_by_ids(&self, chunk_ids: &[i64]) -> Result<Vec<ChunkInfo>, SearchError> {
        if chunk_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = chunk_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let query = format!(
            "SELECT c.kind, c.symbol_name, c.line_start, c.line_end, c.text,
                    c.annotations, c.is_export, f.path, f.collection
             FROM overlay_chunks c
             JOIN overlay_files f ON f.id = c.file_id
             WHERE c.id IN ({})",
            placeholders
        );
        let mut stmt = self.conn.prepare(&query)?;
        let params_vec: Vec<&dyn rusqlite::ToSql> =
            chunk_ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(rusqlite::params_from_iter(params_vec.iter()), |row| {
            Ok(ChunkInfo {
                file_path: row.get(7)?,
                collection: row.get(8)?,
                kind: row.get(0)?,
                symbol_name: row.get(1)?,
                line_start: row.get(2)?,
                line_end: row.get(3)?,
                text: row.get(4)?,
                annotations: row.get::<_, Option<String>>(5)?,
                is_export: row.get::<_, i32>(6)? != 0,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn load_overlay_embeddings(&self, dim: usize) -> Result<Vec<(i64, Vec<f32>)>, SearchError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, embedding FROM overlay_chunks WHERE embedding IS NOT NULL")?;
        let rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((id, blob))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (id, blob) = row?;
            if blob.len() == dim * 4 {
                let embedding: Vec<f32> = blob
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();
                result.push((id, embedding));
            }
        }
        Ok(result)
    }

    pub fn overlay_file_count(&self, collection: &str) -> Result<usize, SearchError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM overlay_files WHERE collection = ?1",
            params![collection],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn overlay_chunk_count(&self, collection: &str) -> Result<usize, SearchError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM overlay_chunks c
             JOIN overlay_files f ON f.id = c.file_id
             WHERE f.collection = ?1",
            params![collection],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn overlay_tombstone_count(&self, collection: &str) -> Result<usize, SearchError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM overlay_tombstones WHERE collection = ?1",
            params![collection],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn load_overlay_fingerprint_cache(
        &self,
        manifest_snapshot_id: &str,
    ) -> Result<Option<HashMap<String, PersistedFingerprint>>, SearchError> {
        let mut stmt = self.conn.prepare(
            "SELECT path, file_size, file_mtime_secs, file_mtime_nanos, content_fingerprint
             FROM overlay_fingerprint_cache
             WHERE manifest_snapshot_id = ?1",
        )?;
        let rows = stmt.query_map(params![manifest_snapshot_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                PersistedFingerprint {
                    file_size: row.get::<_, i64>(1)? as u64,
                    file_mtime_secs: row.get::<_, i64>(2)?,
                    file_mtime_nanos: row.get::<_, u32>(3)?,
                    content_fingerprint: row.get::<_, String>(4)?,
                },
            ))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (path, entry) = row?;
            map.insert(path, entry);
        }
        if map.is_empty() {
            let any_rows: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM overlay_fingerprint_cache LIMIT 1)",
                [],
                |row| row.get(0),
            )?;
            if any_rows {
                self.clear_overlay_fingerprint_cache()?;
            }
            return Ok(None);
        }
        Ok(Some(map))
    }

    pub fn save_overlay_fingerprint_cache(
        &self,
        manifest_snapshot_id: &str,
        entries: &HashMap<String, PersistedFingerprint>,
    ) -> Result<(), SearchError> {
        self.conn.execute("DELETE FROM overlay_fingerprint_cache", [])?;
        let mut stmt = self.conn.prepare(
            "INSERT INTO overlay_fingerprint_cache
             (path, file_size, file_mtime_secs, file_mtime_nanos, content_fingerprint, manifest_snapshot_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for (path, entry) in entries {
            stmt.execute(params![
                path,
                entry.file_size as i64,
                entry.file_mtime_secs,
                entry.file_mtime_nanos,
                entry.content_fingerprint,
                manifest_snapshot_id,
            ])?;
        }
        Ok(())
    }

    pub fn clear_overlay_fingerprint_cache(&self) -> Result<(), SearchError> {
        self.conn.execute("DELETE FROM overlay_fingerprint_cache", [])?;
        Ok(())
    }

    pub fn load_overlay_embedding_cache(
        &self,
        model_id: &str,
        dimension: usize,
    ) -> Result<HashMap<String, Vec<f32>>, SearchError> {
        let dimension = dimension as i64;
        let mut stmt = self.conn.prepare(
            "SELECT content_hash, embedding
             FROM overlay_embedding_cache
             WHERE model_id = ?1 AND dimension = ?2",
        )?;
        let rows = stmt.query_map(params![model_id, dimension], |row| {
            let hash: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((hash, blob))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (hash, blob) = row?;
            if blob.len() % 4 == 0 {
                let embedding: Vec<f32> = blob
                    .chunks_exact(4)
                    .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                    .collect();
                map.insert(hash, embedding);
            }
        }
        Ok(map)
    }

    pub fn save_overlay_embedding_cache(
        &self,
        model_id: &str,
        dimension: usize,
        entries: &HashMap<String, Vec<f32>>,
    ) -> Result<(), SearchError> {
        let dimension = dimension as i64;
        let mut stmt = self.conn.prepare(
            "INSERT OR REPLACE INTO overlay_embedding_cache
             (content_hash, model_id, dimension, embedding)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (hash, embedding) in entries {
            let blob: Vec<u8> = embedding.iter().flat_map(|v| v.to_le_bytes()).collect();
            stmt.execute(params![hash, model_id, dimension, blob])?;
        }
        Ok(())
    }

    pub fn clear_overlay_embedding_cache(&self) -> Result<(), SearchError> {
        self.conn.execute("DELETE FROM overlay_embedding_cache", [])?;
        Ok(())
    }

    pub fn clear_overlay_state(&self, collection: &str) -> Result<(), SearchError> {
        self.conn.execute(
            "DELETE FROM overlay_chunks_fts WHERE rowid IN (
                 SELECT c.id FROM overlay_chunks c
                 JOIN overlay_files f ON f.id = c.file_id
                 WHERE f.collection = ?1
             )",
            params![collection],
        )?;
        self.conn.execute(
            "DELETE FROM overlay_chunks WHERE file_id IN (
                 SELECT id FROM overlay_files WHERE collection = ?1
             )",
            params![collection],
        )?;
        self.conn
            .execute("DELETE FROM overlay_files WHERE collection = ?1", params![collection])?;
        self.clear_overlay_tombstones(collection)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct TextSearchResult {
    pub chunk_id: i64,
    pub rank: f64,
}

#[derive(Debug, Clone)]
pub struct ChunkInfo {
    pub file_path: String,
    pub collection: String,
    pub kind: String,
    pub symbol_name: String,
    pub line_start: u32,
    pub line_end: u32,
    pub text: String,
    pub annotations: Option<String>,
    pub is_export: bool,
}

#[derive(Debug, Clone)]
pub struct BaselineManifestRecord {
    pub snapshot_id: String,
    pub fingerprint: Option<String>,
    pub manifest_files: usize,
    pub fetched_at: String,
}

#[derive(Debug, Clone)]
pub struct PersistedFingerprint {
    pub file_size: u64,
    pub file_mtime_secs: i64,
    pub file_mtime_nanos: u32,
    pub content_fingerprint: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_chunk::{Chunk, ChunkKind};

    fn sample_chunk(name: &str) -> Chunk {
        Chunk {
            kind: ChunkKind::Procedure,
            name: name.to_owned(),
            is_export: true,
            annotations: vec!["НаСервере".to_owned()],
            line_start: 0,
            line_end: 5,
            text: format!("Процедура {name}()\nКонецПроцедуры"),
        }
    }

    #[test]
    fn create_and_query() {
        let mut store = Store::in_memory().unwrap();
        let hash = blake3::hash(b"test content");

        let file_id =
            store.reindex_file("test.bsl", hash.as_bytes(), &[sample_chunk("Тест")], None).unwrap();

        assert!(file_id > 0);
        assert_eq!(store.file_count().unwrap(), 1);
        assert_eq!(store.chunk_count().unwrap(), 1);
    }

    #[test]
    fn embedding_generation_advances_on_indexed_set_changes() {
        let mut store = Store::in_memory().unwrap();
        // Pin the conservative pragma: the cascade bump must hold even with recursive triggers off.
        store.conn.execute_batch("PRAGMA recursive_triggers = OFF;").unwrap();

        let g0 = store.embedding_generation().unwrap();

        // Insert two chunks -> two INSERT trigger firings.
        store
            .reindex_file("m.bsl", b"h0", &[sample_chunk("Один"), sample_chunk("Два")], None)
            .unwrap();
        let g_after_insert = store.embedding_generation().unwrap();
        assert!(g_after_insert > g0, "insert must advance the generation");

        // In-place embedding update -> UPDATE OF embedding trigger.
        let id: i64 =
            store.conn.query_row("SELECT id FROM chunks LIMIT 1", [], |r| r.get(0)).unwrap();
        store.set_chunk_embedding(id, &[0.1_f32, 0.2, 0.3]).unwrap();
        let g_after_update = store.embedding_generation().unwrap();
        assert!(g_after_update > g_after_insert, "embedding update must advance the generation");

        // A non-embedding column update must NOT advance it (the index is unaffected).
        store
            .conn
            .execute("UPDATE chunks SET line_end = line_end + 1 WHERE id = ?1", params![id])
            .unwrap();
        assert_eq!(
            store.embedding_generation().unwrap(),
            g_after_update,
            "a non-embedding update must not advance the generation"
        );

        // File removal cascades to chunks; `files_gen_del` guarantees an advance regardless of
        // whether the cascade fires the chunk delete trigger.
        store.remove_file("m.bsl").unwrap();
        assert!(
            store.embedding_generation().unwrap() > g_after_update,
            "file removal (cascade delete) must advance the generation"
        );
        assert_eq!(store.chunk_count().unwrap(), 0);
    }

    #[test]
    fn structural_wipe_removes_persisted_vector_artifacts() {
        use crate::index::VectorIndex;

        const DIM: usize = 4;
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("search.db");

        // Seed one embedded chunk and persist a vector index + sidecar beside the DB.
        {
            let mut store = Store::open(&db_path).unwrap();
            let emb = vec![0.1_f32, 0.2, 0.3, 0.4];
            store.reindex_file("f.bsl", b"h0", &[sample_chunk("П")], Some(&[emb])).unwrap();
            let (generation, data) = store.load_all_embeddings_with_generation(DIM).unwrap();
            let index = VectorIndex::build(DIM, &data).unwrap();
            let key = crate::vector_persist::PersistKey {
                db_path: store.db_path(),
                model_id: "test-model",
                dim: DIM,
            };
            crate::vector_persist::persist(&index, &key, generation).unwrap();

            // Simulate a future structural-schema change: stamp a different version so the next
            // open wipes the derived cache (which drops `meta` and resets the generation counter).
            store
                .conn
                .execute("UPDATE meta SET value = '999' WHERE key = 'schema_version'", [])
                .unwrap();
        }

        let usearch = dir.path().join("search.db.usearch");
        let sidecar = dir.path().join("search.db.usearch.json");
        assert!(usearch.exists() && sidecar.exists(), "artifacts persisted before the wipe");

        // Reopening sees the version mismatch, wipes the tables, and must delete the stale
        // artifacts so the reset generation (0) can never match the old sidecar.
        let store = Store::open(&db_path).unwrap();
        assert_eq!(store.chunk_count().unwrap(), 0, "wipe emptied the chunks");
        assert!(!usearch.exists(), "stale index file removed by the wipe");
        assert!(!sidecar.exists(), "stale sidecar removed by the wipe");
        let key = crate::vector_persist::PersistKey {
            db_path: store.db_path(),
            model_id: "test-model",
            dim: DIM,
        };
        assert!(
            crate::vector_persist::try_load(&store, &key).is_none(),
            "no stale index is served over the emptied database"
        );
    }

    #[test]
    fn structural_wipe_aborts_when_stale_sidecar_cannot_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("search.db");

        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file(
                    "f.bsl",
                    b"h0",
                    &[sample_chunk("П")],
                    Some(&[vec![0.1, 0.2, 0.3, 0.4]]),
                )
                .unwrap();
            store
                .conn
                .execute("UPDATE meta SET value = '999' WHERE key = 'schema_version'", [])
                .unwrap();
        }

        // Make the sidecar path un-removable as a plain file by turning it into a (non-empty)
        // directory, so `fs::remove_file` fails with a non-`NotFound` error. The wipe must abort
        // rather than empty the DB while a loadable sidecar survives.
        let sidecar = dir.path().join("search.db.usearch.json");
        std::fs::create_dir_all(sidecar.join("blocker")).unwrap();

        assert!(
            Store::open(&db_path).is_err(),
            "a structural wipe must fail closed when the stale sidecar cannot be removed"
        );

        // Once the obstruction is gone, the wipe proceeds and the DB is reconciled.
        std::fs::remove_dir_all(&sidecar).unwrap();
        let store = Store::open(&db_path).unwrap();
        assert_eq!(store.chunk_count().unwrap(), 0);
    }

    #[test]
    fn missing_generation_row_on_current_schema_invalidates_artifacts() {
        use crate::index::VectorIndex;

        const DIM: usize = 4;
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("search.db");

        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file(
                    "f.bsl",
                    b"h0",
                    &[sample_chunk("П")],
                    Some(&[vec![0.1, 0.2, 0.3, 0.4]]),
                )
                .unwrap();
            let (generation, data) = store.load_all_embeddings_with_generation(DIM).unwrap();
            let index = VectorIndex::build(DIM, &data).unwrap();
            let key = crate::vector_persist::PersistKey {
                db_path: store.db_path(),
                model_id: "test-model",
                dim: DIM,
            };
            crate::vector_persist::persist(&index, &key, generation).unwrap();
            assert!(crate::vector_persist::try_load(&store, &key).is_some());

            // Corruption: the counter row vanishes while the schema version stays current, so no
            // structural wipe runs. The reset counter must not silently come back as 0 and validate
            // the old sidecar (which would serve a possibly-stale index).
            store.conn.execute("DELETE FROM meta WHERE key = 'embedding_generation'", []).unwrap();
        }

        let sidecar = dir.path().join("search.db.usearch.json");
        assert!(sidecar.exists(), "sidecar present before the corrupt reopen");

        let store = Store::open(&db_path).unwrap();
        assert_eq!(store.embedding_generation().unwrap(), 0, "counter reseeded");
        assert!(!sidecar.exists(), "stale sidecar removed when the counter had to be recreated");
        let key = crate::vector_persist::PersistKey {
            db_path: store.db_path(),
            model_id: "test-model",
            dim: DIM,
        };
        assert!(
            crate::vector_persist::try_load(&store, &key).is_none(),
            "no stale index is served after the counter was recreated"
        );
    }

    #[test]
    fn reindex_replaces_chunks() {
        let mut store = Store::in_memory().unwrap();
        let hash1 = blake3::hash(b"v1");
        let hash2 = blake3::hash(b"v2");

        store
            .reindex_file(
                "mod.bsl",
                hash1.as_bytes(),
                &[sample_chunk("Первая"), sample_chunk("Вторая")],
                None,
            )
            .unwrap();
        assert_eq!(store.chunk_count().unwrap(), 2);

        store.reindex_file("mod.bsl", hash2.as_bytes(), &[sample_chunk("Новая")], None).unwrap();
        assert_eq!(store.chunk_count().unwrap(), 1);
        assert_eq!(store.file_count().unwrap(), 1);
    }

    #[test]
    fn file_hash_lookup() {
        let mut store = Store::in_memory().unwrap();
        let hash = blake3::hash(b"content");

        assert!(store.file_hash("test.bsl").unwrap().is_none());

        store.reindex_file("test.bsl", hash.as_bytes(), &[sample_chunk("Тест")], None).unwrap();

        let stored = store.file_hash("test.bsl").unwrap().unwrap();
        assert_eq!(stored, hash.as_bytes());
    }

    #[test]
    fn embeddings_roundtrip() {
        let mut store = Store::in_memory().unwrap();
        let hash = blake3::hash(b"test");
        let embedding = vec![0.1f32, 0.2, 0.3, 0.4];

        store
            .reindex_file(
                "test.bsl",
                hash.as_bytes(),
                &[sample_chunk("Тест")],
                Some(std::slice::from_ref(&embedding)),
            )
            .unwrap();

        let loaded = store.load_all_embeddings(4).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].1, embedding);
    }

    #[test]
    fn chunk_by_id_returns_metadata() {
        let mut store = Store::in_memory().unwrap();
        let hash = blake3::hash(b"test");
        store
            .reindex_file("path/to/module.bsl", hash.as_bytes(), &[sample_chunk("Метод")], None)
            .unwrap();

        assert_eq!(store.chunk_count().unwrap(), 1);

        let chunk_id: i64 =
            store.conn.query_row("SELECT id FROM chunks LIMIT 1", [], |r| r.get(0)).unwrap();

        let info = store.chunk_by_id(chunk_id).unwrap().unwrap();
        assert_eq!(info.file_path, "path/to/module.bsl");
        assert_eq!(info.symbol_name, "Метод");
        assert_eq!(info.kind, "procedure");
        assert!(info.is_export);
        assert_eq!(info.annotations.as_deref(), Some("НаСервере"));
    }

    #[test]
    fn remove_file_cascades() {
        let mut store = Store::in_memory().unwrap();
        let hash = blake3::hash(b"test");
        store
            .reindex_file(
                "test.bsl",
                hash.as_bytes(),
                &[sample_chunk("А"), sample_chunk("Б")],
                None,
            )
            .unwrap();
        assert_eq!(store.chunk_count().unwrap(), 2);

        store.remove_file("test.bsl").unwrap();
        assert_eq!(store.file_count().unwrap(), 0);
        assert_eq!(store.chunk_count().unwrap(), 0);
    }

    #[test]
    fn fts_search_by_symbol_name() {
        let mut store = Store::in_memory().unwrap();
        let hash = blake3::hash(b"test");
        store
            .reindex_file(
                "test.bsl",
                hash.as_bytes(),
                &[sample_chunk("ОбработкаПроведения"), sample_chunk("ПриСозданииНаСервере")],
                None,
            )
            .unwrap();

        let results = store.text_search("ОбработкаПроведения", 10, None).unwrap();
        assert_eq!(results.len(), 1);

        let info = store.chunk_by_id(results[0].chunk_id).unwrap().unwrap();
        assert_eq!(info.symbol_name, "ОбработкаПроведения");
    }

    #[test]
    fn fts_search_by_text_content() {
        let mut store = Store::in_memory().unwrap();
        let hash = blake3::hash(b"test");

        let mut chunk = sample_chunk("Тест");
        chunk.text =
            "Процедура Тест()\n    СообщитьПользователю(\"Привет\");\nКонецПроцедуры".to_owned();

        store.reindex_file("test.bsl", hash.as_bytes(), &[chunk], None).unwrap();

        let results = store.text_search("СообщитьПользователю", 10, None).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn fts_multi_term_query_matches_any_term() {
        let mut store = Store::in_memory().unwrap();

        // One chunk carries only the identifier; the other carries the identifier and the extra
        // words. A multi-word query used to be wrapped as one phrase, matching neither; with the
        // OR fix both surface.
        let mut only_id = sample_chunk("Прочее");
        only_id.text = "Процедура Прочее()\n    ВызватьHTTПМетод();\nКонецПроцедуры".to_owned();
        let mut full = sample_chunk("Отправщик");
        full.text =
            "Процедура Отправщик()\n    ВызватьHTTПМетод(); // отправка запроса\nКонецПроцедуры"
                .to_owned();

        store.reindex_file("a.bsl", b"h0", &[only_id], None).unwrap();
        store.reindex_file("b.bsl", b"h1", &[full], None).unwrap();

        let results = store.text_search("ВызватьHTTПМетод отправка запроса", 10, None).unwrap();
        assert_eq!(results.len(), 2, "OR semantics must surface a chunk matching any term");
    }

    #[test]
    fn fts_dotted_call_term_matches_indexed_code() {
        let mut store = Store::in_memory().unwrap();
        let mut chunk = sample_chunk("Отправщик");
        chunk.text =
            "Процедура Отправщик()\n    КоннекторHTTP.ВызватьМетод();\nКонецПроцедуры".to_owned();
        store.reindex_file("a.bsl", b"h0", &[chunk], None).unwrap();

        // The dotted call is one quoted token; unicode61 makes it an adjacency phrase that still
        // matches the same dotted call in the body.
        let results = store.text_search("КоннекторHTTP.ВызватьМетод()", 10, None).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn fts_punctuation_only_query_is_empty_not_error() {
        let mut store = Store::in_memory().unwrap();
        store.reindex_file("a.bsl", b"h0", &[sample_chunk("Метод")], None).unwrap();
        // No usable term -> empty result, never an FTS5 syntax error.
        assert!(store.text_search("()", 10, None).unwrap().is_empty());
        assert!(store.text_search("   ", 10, None).unwrap().is_empty());
    }

    #[test]
    fn fts_reindex_updates_index() {
        let mut store = Store::in_memory().unwrap();
        let hash1 = blake3::hash(b"v1");
        let hash2 = blake3::hash(b"v2");

        store.reindex_file("test.bsl", hash1.as_bytes(), &[sample_chunk("Старая")], None).unwrap();
        assert_eq!(store.text_search("Старая", 10, None).unwrap().len(), 1);

        store.reindex_file("test.bsl", hash2.as_bytes(), &[sample_chunk("Новая")], None).unwrap();

        assert_eq!(store.text_search("Старая", 10, None).unwrap().len(), 0);
        assert_eq!(store.text_search("Новая", 10, None).unwrap().len(), 1);
    }

    #[test]
    fn fts_remove_file_cleans_index() {
        let mut store = Store::in_memory().unwrap();
        let hash = blake3::hash(b"test");
        store
            .reindex_file("test.bsl", hash.as_bytes(), &[sample_chunk("Удаляемая")], None)
            .unwrap();
        assert_eq!(store.text_search("Удаляемая", 10, None).unwrap().len(), 1);

        store.remove_file("test.bsl").unwrap();
        assert_eq!(store.text_search("Удаляемая", 10, None).unwrap().len(), 0);
    }

    #[test]
    fn load_indexed_documents_filters_by_collection() {
        let mut store = Store::in_memory().unwrap();
        let code = crate::Chunker::chunk("Процедура Код()\nКонецПроцедуры");
        store.reindex_file("A.bsl", b"hash-a", &code, None).unwrap();
        store
            .reindex_documents(
                "platform",
                "platform://docs",
                b"hash-docs",
                &[crate::Document {
                    title: "Строка".to_owned(),
                    body: "Описание".to_owned(),
                    kind: "type".to_owned(),
                }],
                None,
            )
            .unwrap();

        let code_docs = store.load_indexed_documents(Some("code")).unwrap();
        let platform_docs = store.load_indexed_documents(Some("platform")).unwrap();

        assert_eq!(code_docs.len(), 1);
        assert_eq!(code_docs[0].collection, "code");
        assert_eq!(platform_docs.len(), 1);
        assert_eq!(platform_docs[0].collection, "platform");
    }

    #[test]
    fn baseline_manifest_roundtrip() {
        let store = Store::in_memory().unwrap();
        assert!(store.load_baseline_manifest().unwrap().is_none());
        assert!(store.load_baseline_manifest_fingerprints("code").unwrap().is_none());

        let manifest = crate::WorkspaceBaselineManifest {
            snapshot_id: "snap-123".to_owned(),
            snapshot_fingerprint: Some("fp-abc".to_owned()),
            files: vec![
                crate::BaselineManifestFile {
                    collection: "code".to_owned(),
                    path: "src/A.bsl".to_owned(),
                    file_fingerprint: "fp-a".to_owned(),
                    document_count: 1,
                    file_object_id: "obj-a".to_owned(),
                },
                crate::BaselineManifestFile {
                    collection: "code".to_owned(),
                    path: "src/B.bsl".to_owned(),
                    file_fingerprint: "fp-b".to_owned(),
                    document_count: 2,
                    file_object_id: "obj-b".to_owned(),
                },
            ],
        };
        store.save_baseline_manifest(&manifest).unwrap();

        let record = store.load_baseline_manifest().unwrap().unwrap();
        assert_eq!(record.snapshot_id, "snap-123");
        assert_eq!(record.fingerprint, Some("fp-abc".to_owned()));
        assert_eq!(record.manifest_files, 2);

        let fingerprints = store.load_baseline_manifest_fingerprints("code").unwrap().unwrap();
        assert_eq!(fingerprints.len(), 2);
        assert_eq!(fingerprints.get("src/A.bsl").map(String::as_str), Some("fp-a"));
        assert_eq!(fingerprints.get("src/B.bsl").map(String::as_str), Some("fp-b"));

        store.clear_baseline_manifest().unwrap();
        assert!(store.load_baseline_manifest().unwrap().is_none());
        assert!(store.load_baseline_manifest_fingerprints("code").unwrap().is_none());
    }

    #[test]
    fn overlay_tombstone_persistence() {
        let store = Store::in_memory().unwrap();
        assert!(store.overlay_tombstone_paths("code").unwrap().is_empty());

        store.insert_overlay_tombstone("src/A.bsl", "code").unwrap();
        store.insert_overlay_tombstone("src/B.bsl", "code").unwrap();

        let paths = store.overlay_tombstone_paths("code").unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains("src/A.bsl"));
        assert!(paths.contains("src/B.bsl"));

        store.remove_overlay_tombstone("src/A.bsl").unwrap();
        let paths = store.overlay_tombstone_paths("code").unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths.contains("src/B.bsl"));

        store.clear_overlay_tombstones("code").unwrap();
        assert!(store.overlay_tombstone_paths("code").unwrap().is_empty());
    }

    #[test]
    fn overlay_file_with_chunks_roundtrip() {
        let mut store = Store::in_memory().unwrap();
        let hash = blake3::hash(b"overlay content");
        let chunks = vec![sample_chunk("OverlayProc")];

        store
            .upsert_overlay_file_with_chunks(
                "src/Overlay.bsl",
                hash.as_bytes(),
                "code",
                &chunks,
                None,
            )
            .unwrap();

        assert_eq!(store.overlay_file_count("code").unwrap(), 1);
        assert_eq!(store.overlay_chunk_count("code").unwrap(), 1);

        let results = store.overlay_text_search("OverlayProc", 10, Some("code")).unwrap();
        assert_eq!(results.len(), 1);

        store.remove_overlay_file("src/Overlay.bsl").unwrap();
        assert_eq!(store.overlay_file_count("code").unwrap(), 0);
        assert_eq!(store.overlay_chunk_count("code").unwrap(), 0);
        assert_eq!(store.overlay_text_search("OverlayProc", 10, Some("code")).unwrap().len(), 0);
    }

    #[test]
    fn clear_overlay_state_removes_all() {
        let mut store = Store::in_memory().unwrap();
        let hash = blake3::hash(b"overlay");
        store
            .upsert_overlay_file_with_chunks(
                "src/A.bsl",
                hash.as_bytes(),
                "code",
                &[sample_chunk("ProcA")],
                None,
            )
            .unwrap();
        store.insert_overlay_tombstone("src/B.bsl", "code").unwrap();

        store.clear_overlay_state("code").unwrap();
        assert_eq!(store.overlay_file_count("code").unwrap(), 0);
        assert_eq!(store.overlay_chunk_count("code").unwrap(), 0);
        assert_eq!(store.overlay_tombstone_count("code").unwrap(), 0);
    }

    #[test]
    fn overlay_embeddings_roundtrip() {
        let mut store = Store::in_memory().unwrap();
        let hash = blake3::hash(b"overlay");
        let embedding = vec![0.1f32, 0.2, 0.3, 0.4];
        store
            .upsert_overlay_file_with_chunks(
                "src/Emb.bsl",
                hash.as_bytes(),
                "code",
                &[sample_chunk("EmbProc")],
                Some(std::slice::from_ref(&embedding)),
            )
            .unwrap();

        let loaded = store.load_overlay_embeddings(4).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].1, embedding);
    }

    #[test]
    fn reindex_persists_and_loads_graph_context() {
        let mut store = Store::in_memory().unwrap();
        store
            .reindex_file_with_context(
                "A.bsl",
                b"h",
                &[sample_chunk("Делать")],
                None,
                Some(&[Some("Dispatch: server | сервер\nCalls: Иная\n".to_owned())]),
            )
            .unwrap();
        let docs = store.load_indexed_documents(Some("code")).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(
            docs[0].graph_context.as_deref(),
            Some("Dispatch: server | сервер\nCalls: Иная\n")
        );

        // A chunk indexed without context round-trips as `None`.
        store.reindex_file("B.bsl", b"h2", &[sample_chunk("Плейн")], None).unwrap();
        let b = store
            .load_indexed_documents(Some("code"))
            .unwrap()
            .into_iter()
            .find(|d| d.path == "B.bsl")
            .unwrap();
        assert_eq!(b.graph_context, None);
    }

    #[test]
    fn embed_text_version_bump_clears_file_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        {
            let mut store = Store::open(&path).unwrap();
            store.reindex_file("A.bsl", b"realhash", &[sample_chunk("Делать")], None).unwrap();
            assert_eq!(store.file_hash("A.bsl").unwrap().unwrap(), b"realhash");
            // Simulate a database written by an older embed-text format.
            store.conn.pragma_update(None, "user_version", 0i64).unwrap();
        }
        // Reopening with a version mismatch clears the hash, so the next index
        // re-embeds the file under the current format instead of keeping a stale vector.
        let store = Store::open(&path).unwrap();
        assert!(
            store.file_hash("A.bsl").unwrap().unwrap().is_empty(),
            "file hash cleared to force re-embed"
        );

        // A second open at the same version is a no-op (does not re-clear).
        let store = Store::open(&path).unwrap();
        assert!(store.file_hash("A.bsl").unwrap().unwrap().is_empty());
    }

    #[test]
    fn open_stamps_current_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        let store = Store::open(&path).unwrap();
        let stored = Store::stored_schema_version(&store.conn).unwrap();
        assert_eq!(stored, Some(SCHEMA_VERSION));
    }

    #[test]
    fn schema_version_bump_wipes_derived_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        {
            let mut store = Store::open(&path).unwrap();
            store.reindex_file("A.bsl", b"realhash", &[sample_chunk("Делать")], None).unwrap();
            assert_eq!(store.file_count().unwrap(), 1);
            assert_eq!(store.chunk_count().unwrap(), 1);
            // Simulate a database written under an older structural schema.
            store
                .conn
                .execute(
                    "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
                    params![(SCHEMA_VERSION - 1).to_string()],
                )
                .unwrap();
        }
        // Reopening with a structural-version mismatch wipes the cache and rebuilds the
        // current schema; the rows are gone and the version is stamped current.
        let store = Store::open(&path).unwrap();
        assert_eq!(store.file_count().unwrap(), 0);
        assert_eq!(store.chunk_count().unwrap(), 0);
        assert_eq!(Store::stored_schema_version(&store.conn).unwrap(), Some(SCHEMA_VERSION));
    }

    #[test]
    fn pre_versioning_database_is_kept_and_stamped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        {
            let mut store = Store::open(&path).unwrap();
            store.reindex_file("A.bsl", b"realhash", &[sample_chunk("Делать")], None).unwrap();
            // Simulate a database created before schema versioning existed.
            store.conn.execute("DELETE FROM meta WHERE key = 'schema_version'", []).unwrap();
        }
        // A missing version row is treated as already-current: the data survives and the
        // version is stamped, so existing workspaces are not force-reindexed on upgrade.
        let store = Store::open(&path).unwrap();
        assert_eq!(store.file_count().unwrap(), 1);
        assert_eq!(Store::stored_schema_version(&store.conn).unwrap(), Some(SCHEMA_VERSION));
    }
}
