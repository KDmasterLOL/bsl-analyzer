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

    /// Total number of chunks in the store.
    pub fn chunk_count(&self) -> Result<usize, SearchError> {
        let count: i64 =
            self.conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
        Ok(count as usize)
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
            let rows = stmt.query_map(params![query, coll, limit as i64], |row| {
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
            let rows = stmt.query_map(params![query, limit as i64], |row| {
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
}
