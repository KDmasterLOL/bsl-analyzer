//! SQLite-backed persistent storage for search index.
//!
//! Stores file hashes, code chunks, embedding vectors, and platform
//! documentation. Supports multiple collections (e.g. "code", "platform")
//! within a single database. Survives process restarts and supports
//! incremental updates.

use crate::chunker::Chunk;
use crate::document::Document;
use crate::error::SearchError;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Persistent store for search index data.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open or create a search index database at the given path.
    pub fn open(path: &Path) -> Result<Self, SearchError> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Create an in-memory store (for testing).
    #[cfg(test)]
    pub fn in_memory() -> Result<Self, SearchError> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), SearchError> {
        self.conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;

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
                embedding   BLOB
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

        // Migration: add collection column to existing databases.
        let _ = self
            .conn
            .execute("ALTER TABLE files ADD COLUMN collection TEXT NOT NULL DEFAULT 'code'", []);

        // Overlay-only tables: baseline metadata, overlay tombstones, and
        // overlay-specific file/chunk storage. These are created alongside the
        // existing schema but are only populated when a Postgres baseline is
        // configured.
        self.conn.execute_batch(
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
            ",
        )?;

        Ok(())
    }

    /// Get the stored hash for a file path, if any.
    pub fn file_hash(&self, path: &str) -> Result<Option<Vec<u8>>, SearchError> {
        let hash = self
            .conn
            .query_row("SELECT hash FROM files WHERE path = ?1", params![path], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .optional()?;
        Ok(hash)
    }

    /// Insert or update a file record.
    /// Returns the file id.
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

    /// Remove a file and all its chunks (CASCADE) and FTS entries.
    pub fn remove_file(&self, path: &str) -> Result<(), SearchError> {
        // Clean FTS entries before CASCADE deletes chunks.
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

    /// Delete all chunks belonging to a file.
    pub fn delete_chunks_for_file(&self, file_id: i64) -> Result<(), SearchError> {
        self.conn.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id])?;
        Ok(())
    }

    /// Insert a chunk for a file. Returns the chunk id.
    pub fn insert_chunk(
        &self,
        file_id: i64,
        chunk: &Chunk,
        embedding: Option<&[f32]>,
    ) -> Result<i64, SearchError> {
        let kind_str = match chunk.kind {
            crate::chunker::ChunkKind::ModuleHeader => "header",
            crate::chunker::ChunkKind::Procedure => "procedure",
            crate::chunker::ChunkKind::Function => "function",
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

    /// Reindex a file: delete old chunks, insert new ones.
    /// Runs in a single transaction for atomicity.
    pub fn reindex_file(
        &mut self,
        path: &str,
        hash: &[u8],
        chunks: &[Chunk],
        embeddings: Option<&[Vec<f32>]>,
    ) -> Result<i64, SearchError> {
        self.reindex_file_in_collection(path, hash, "code", chunks, embeddings)
    }

    /// Reindex a file within a specific collection.
    pub fn reindex_file_in_collection(
        &mut self,
        path: &str,
        hash: &[u8],
        collection: &str,
        chunks: &[Chunk],
        embeddings: Option<&[Vec<f32>]>,
    ) -> Result<i64, SearchError> {
        let tx = self.conn.transaction()?;

        // Upsert file.
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

        // Delete old FTS entries for this file's chunks.
        tx.execute(
            "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![file_id],
        )?;

        // Delete old chunks.
        tx.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id])?;

        // Insert new chunks and sync FTS index.
        {
            let mut stmt = tx.prepare(
                "INSERT INTO chunks (file_id, kind, symbol_name, is_export, annotations,
                                     line_start, line_end, text, embedding)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            let mut fts_stmt =
                tx.prepare("INSERT INTO chunks_fts(rowid, symbol_name, text) VALUES (?1, ?2, ?3)")?;

            for (i, chunk) in chunks.iter().enumerate() {
                let kind_str = match chunk.kind {
                    crate::chunker::ChunkKind::ModuleHeader => "header",
                    crate::chunker::ChunkKind::Procedure => "procedure",
                    crate::chunker::ChunkKind::Function => "function",
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

    /// Reindex documents in a collection (e.g. platform reference).
    ///
    /// Documents are stored as chunks under a virtual file path.
    /// Uses the same FTS and embedding infrastructure as code chunks.
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

        // Delete old FTS entries.
        tx.execute(
            "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_id = ?1)",
            params![file_id],
        )?;
        tx.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id])?;

        // Insert documents as chunks.
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

    /// Load all embeddings with their chunk ids for building the HNSW index.
    /// Returns (chunk_id, embedding) pairs.
    pub fn load_all_embeddings(&self, dim: usize) -> Result<Vec<(i64, Vec<f32>)>, SearchError> {
        let mut stmt =
            self.conn.prepare("SELECT id, embedding FROM chunks WHERE embedding IS NOT NULL")?;

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

    /// Get chunk metadata by id (for returning search results).
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

    /// Get all indexed file paths with their hashes.
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

    /// Get all indexed file paths with their hashes for a single collection.
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

    /// Remove all persisted rows for one collection from the primary code/docs tables.
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

    /// Total number of chunks in the store.
    pub fn chunk_count(&self) -> Result<usize, SearchError> {
        let count: i64 =
            self.conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Load indexed documents, optionally filtered by collection.
    pub fn load_indexed_documents(
        &self,
        collection: Option<&str>,
    ) -> Result<Vec<crate::IndexedDocument>, SearchError> {
        let query = if collection.is_some() {
            "SELECT f.collection, f.path, c.symbol_name, c.kind, c.line_start, c.line_end, c.text
             FROM chunks c
             JOIN files f ON f.id = c.file_id
             WHERE f.collection = ?1
             ORDER BY f.collection, f.path, c.line_start, c.line_end, c.symbol_name"
        } else {
            "SELECT f.collection, f.path, c.symbol_name, c.kind, c.line_start, c.line_end, c.text
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        };

        Ok(rows)
    }

    /// Full-text search across chunk symbol names and text.
    ///
    /// If `collection` is `Some`, only searches within that collection.
    /// Returns chunk ids with FTS5 rank scores, ordered by relevance.
    pub fn text_search(
        &self,
        query: &str,
        limit: usize,
        collection: Option<&str>,
    ) -> Result<Vec<TextSearchResult>, SearchError> {
        // Wrap query in double quotes for FTS5 to treat it as a literal phrase.
        // This prevents FTS5 syntax errors on dots, parentheses, and other special chars
        // (e.g. "БонусныеБаллы.Остатки" or "СообщитьПользователю()").
        let escaped = format!("\"{}\"", query.replace('"', "\"\""));
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
            let rows = stmt.query_map(params![escaped, coll, limit as i64], |row| {
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
            let rows = stmt.query_map(params![escaped, limit as i64], |row| {
                Ok(TextSearchResult { chunk_id: row.get(0)?, rank: row.get(1)? })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        Ok(results)
    }

    /// Rebuild the FTS5 index from existing chunk data.
    ///
    /// Clears and repopulates the standalone FTS5 table from chunks.
    /// Useful after schema migration or if the index gets out of sync.
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

    /// Check if the FTS index is populated.
    pub fn fts_count(&self) -> Result<usize, SearchError> {
        let count: i64 =
            self.conn.query_row("SELECT COUNT(*) FROM chunks_fts", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Total number of indexed files.
    pub fn file_count(&self) -> Result<usize, SearchError> {
        let count: i64 = self.conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Count chunks with embeddings in a specific collection.
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

    /// Clear file hashes for a collection so they get re-indexed.
    pub fn clear_file_hashes(&self, collection: &str) -> Result<usize, SearchError> {
        let count = self.conn.execute(
            "UPDATE files SET hash = zeroblob(0) WHERE collection = ?1",
            params![collection],
        )?;
        Ok(count)
    }

    /// Clear file hashes only for files that have no embeddings in their chunks.
    ///
    /// This allows `index_directory` to re-process only files that lack
    /// embeddings, without touching files already fully indexed.
    pub fn clear_file_hashes_without_embeddings(
        &self,
        collection: &str,
    ) -> Result<usize, SearchError> {
        let count = self.conn.execute(
            "UPDATE files SET hash = zeroblob(0)
             WHERE collection = ?1
               AND id NOT IN (
                   SELECT DISTINCT file_id FROM chunks WHERE embedding IS NOT NULL
               )",
            params![collection],
        )?;
        Ok(count)
    }

    // -----------------------------------------------------------------------
    // Baseline manifest metadata persistence
    // -----------------------------------------------------------------------

    /// Persist the selected baseline manifest metadata for workspace code.
    /// This is the only baseline state stored locally — no code documents.
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

    /// Returns the stored baseline manifest metadata, if any.
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

    /// Clear the stored baseline manifest metadata.
    pub fn clear_baseline_manifest(&self) -> Result<(), SearchError> {
        self.conn.execute("DELETE FROM baseline_manifest_files", [])?;
        self.conn.execute("DELETE FROM baseline_manifest WHERE id = 1", [])?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Overlay tombstones
    // -----------------------------------------------------------------------

    /// Record a tombstone for a deleted baseline file.
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

    /// Remove a tombstone (e.g. when a previously deleted file is restored).
    pub fn remove_overlay_tombstone(&self, path: &str) -> Result<(), SearchError> {
        self.conn.execute("DELETE FROM overlay_tombstones WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// Returns all tombstone paths for a collection.
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

    /// Clear all tombstones for a collection.
    pub fn clear_overlay_tombstones(&self, collection: &str) -> Result<(), SearchError> {
        self.conn
            .execute("DELETE FROM overlay_tombstones WHERE collection = ?1", params![collection])?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Overlay files and chunks
    // -----------------------------------------------------------------------

    /// Upsert an overlay file with its chunks and FTS entries atomically.
    /// Returns the overlay file id.
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

        // Delete old FTS entries and chunks for this overlay file.
        tx.execute(
            "DELETE FROM overlay_chunks_fts WHERE rowid IN (SELECT id FROM overlay_chunks WHERE file_id = ?1)",
            params![file_id],
        )?;
        tx.execute("DELETE FROM overlay_chunks WHERE file_id = ?1", params![file_id])?;

        // Insert new chunks and sync FTS.
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
                    crate::chunker::ChunkKind::ModuleHeader => "header",
                    crate::chunker::ChunkKind::Procedure => "procedure",
                    crate::chunker::ChunkKind::Function => "function",
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

    /// Remove an overlay file and all its chunks (CASCADE + FTS).
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

    /// Overlay FTS search: returns chunk ids with rank scores.
    pub fn overlay_text_search(
        &self,
        query: &str,
        limit: usize,
        collection: Option<&str>,
    ) -> Result<Vec<TextSearchResult>, SearchError> {
        let escaped = format!("\"{}\"", query.replace('"', "\"\""));
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
            let rows = stmt.query_map(params![escaped, coll, limit as i64], |row| {
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
            let rows = stmt.query_map(params![escaped, limit as i64], |row| {
                Ok(TextSearchResult { chunk_id: row.get(0)?, rank: row.get(1)? })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        Ok(results)
    }

    /// Load overlay chunks by chunk ids (for returning search results).
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

    /// Load all overlay embeddings for building the HNSW index.
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

    /// Count of overlay files in a collection.
    pub fn overlay_file_count(&self, collection: &str) -> Result<usize, SearchError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM overlay_files WHERE collection = ?1",
            params![collection],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Count of overlay chunks in a collection.
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

    /// Count of overlay tombstones in a collection.
    pub fn overlay_tombstone_count(&self, collection: &str) -> Result<usize, SearchError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM overlay_tombstones WHERE collection = ?1",
            params![collection],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Clear all overlay state (files, chunks, tombstones) for a collection.
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

/// Full-text search result with chunk id and relevance rank.
#[derive(Debug, Clone)]
pub struct TextSearchResult {
    /// Chunk id (matches the SQLite chunks.id).
    pub chunk_id: i64,
    /// FTS5 rank score (lower is more relevant).
    pub rank: f64,
}

/// Chunk metadata returned from search results.
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

/// Baseline manifest metadata persisted locally.
#[derive(Debug, Clone)]
pub struct BaselineManifestRecord {
    pub snapshot_id: String,
    pub fingerprint: Option<String>,
    pub manifest_files: usize,
    pub fetched_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunker::{Chunk, ChunkKind};

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

        // No embeddings stored, so nothing loaded.
        assert_eq!(store.chunk_count().unwrap(), 1);

        // Get chunk id via a direct query.
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
    fn fts_reindex_updates_index() {
        let mut store = Store::in_memory().unwrap();
        let hash1 = blake3::hash(b"v1");
        let hash2 = blake3::hash(b"v2");

        store.reindex_file("test.bsl", hash1.as_bytes(), &[sample_chunk("Старая")], None).unwrap();
        assert_eq!(store.text_search("Старая", 10, None).unwrap().len(), 1);

        store.reindex_file("test.bsl", hash2.as_bytes(), &[sample_chunk("Новая")], None).unwrap();

        // Old name gone, new name found.
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

    // -----------------------------------------------------------------------
    // Overlay-only store tests
    // -----------------------------------------------------------------------

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

        // FTS search should find the overlay chunk.
        let results = store.overlay_text_search("OverlayProc", 10, Some("code")).unwrap();
        assert_eq!(results.len(), 1);

        // Remove the overlay file.
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
}
