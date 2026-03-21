//! Search engine — ties chunker, embedder, store, and index together.
//!
//! Provides the high-level API for indexing BSL files and platform
//! documentation, and searching through them using FTS5 or semantic
//! similarity. Supports multiple collections within a single database.

use crate::chunker::Chunker;
use crate::context::{enrich_chunk_text, file_path_to_module_path};
use crate::document::Document;
use crate::embedder::{Embedder, EmbedderConfig};
use crate::error::SearchError;
use crate::index::VectorIndex;
use crate::store::Store;
use rayon::prelude::*;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Progress tracker for indexing operations.
///
/// Thread-safe, can be shared between the indexing thread and status queries.
#[derive(Debug, Default)]
pub struct IndexProgress {
    /// Whether indexing is currently in progress.
    pub active: AtomicBool,
    /// Total number of files to process.
    pub total_files: AtomicUsize,
    /// Total number of chunks to embed.
    pub total_chunks: AtomicUsize,
    /// Total number of batches.
    pub total_batches: AtomicUsize,
    /// Number of completed batches.
    pub done_batches: AtomicUsize,
    /// Number of embedded chunks so far.
    pub done_chunks: AtomicUsize,
}

impl IndexProgress {
    /// Create a new progress tracker.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Reset all counters.
    pub fn reset(&self) {
        self.active.store(false, Ordering::Relaxed);
        self.total_files.store(0, Ordering::Relaxed);
        self.total_chunks.store(0, Ordering::Relaxed);
        self.total_batches.store(0, Ordering::Relaxed);
        self.done_batches.store(0, Ordering::Relaxed);
        self.done_chunks.store(0, Ordering::Relaxed);
    }

    /// Completion percentage (0-100).
    pub fn percent(&self) -> usize {
        let total = self.total_chunks.load(Ordering::Relaxed);
        if total == 0 {
            return 0;
        }
        let done = self.done_chunks.load(Ordering::Relaxed);
        (done * 100) / total
    }

    /// Whether indexing is active.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
}

/// Configuration for the search engine.
pub struct SearchConfig {
    /// Embedding API configuration.
    pub embedder: EmbedderConfig,
    /// Maximum batch size for embedding generation.
    pub batch_size: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self { embedder: EmbedderConfig::default(), batch_size: 32 }
    }
}

/// A search result with chunk metadata and similarity score.
#[derive(Debug, Clone)]
pub struct SearchHit {
    /// Collection this result belongs to ("code", "platform").
    pub collection: String,
    /// File path (relative to workspace root, or virtual path for docs).
    pub file_path: String,
    /// Symbol name (procedure/function name, type/method name).
    pub symbol_name: String,
    /// Chunk kind: "procedure", "function", "header", "type", "method", etc.
    pub kind: String,
    /// Source code or documentation text.
    pub text: String,
    /// Line range in the original file (0 for non-file documents).
    pub line_start: u32,
    pub line_end: u32,
    /// Similarity score (0..1, higher is better).
    pub score: f32,
}

/// The search engine: indexes BSL files and documents, performs search.
pub struct SearchEngine {
    store: Store,
    embedder: Option<Embedder>,
    index: VectorIndex,
    dim: usize,
    batch_size: usize,
}

impl SearchEngine {
    /// Create a search engine with full capabilities (FTS + semantic).
    ///
    /// - `db_path`: path to the SQLite database file
    /// - `config`: search configuration (embedder + batch size)
    pub fn new(db_path: &Path, config: SearchConfig) -> Result<Self, SearchError> {
        let store = Store::open(db_path)?;
        let dim = config.embedder.dim.unwrap_or(1024);
        let embedder = Embedder::new(config.embedder);

        // Load existing embeddings from store into HNSW index.
        let data = store.load_all_embeddings(dim)?;
        let index = VectorIndex::build(dim, &data)?;
        info!(vectors = index.len(), dim, "search index loaded");

        Self::ensure_fts(&store)?;

        Ok(Self { store, embedder: Some(embedder), index, dim, batch_size: config.batch_size })
    }

    /// Create a FTS-only search engine (no embedder, no semantic search).
    ///
    /// Suitable for environments without GPU or embedding service.
    /// `find_code` works, `search_code` returns an error.
    pub fn fts_only(db_path: &Path) -> Result<Self, SearchError> {
        let store = Store::open(db_path)?;
        let dim = 1024;
        let index = VectorIndex::new(dim)?;

        Self::ensure_fts(&store)?;

        Ok(Self { store, embedder: None, index, dim, batch_size: 32 })
    }

    /// Auto-populate FTS index from existing chunk data if needed.
    fn ensure_fts(store: &Store) -> Result<(), SearchError> {
        let chunk_count = store.chunk_count()?;
        let fts_count = store.fts_count()?;
        if chunk_count > 0 && fts_count == 0 {
            info!(chunks = chunk_count, "populating FTS index from existing data");
            store.rebuild_fts()?;
        }
        Ok(())
    }

    /// Index all BSL files in a directory with embeddings.
    ///
    /// Walks the directory tree, skips files whose hash hasn't changed,
    /// and generates embeddings for new/modified files.
    ///
    /// If `progress` is provided, updates it with batching stats.
    /// Returns the number of files indexed (new or updated).
    pub fn index_directory(
        &mut self,
        root: &Path,
        progress: Option<&Arc<IndexProgress>>,
    ) -> Result<usize, SearchError> {
        let bsl_files: Vec<std::path::PathBuf> = walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")))
            .map(|e| e.into_path())
            .collect();

        info!(total_files = bsl_files.len(), "scanning BSL files");

        let mut pending_texts = Vec::new();
        let mut pending_chunks_count = Vec::new();
        let mut pending_meta: Vec<(String, Vec<u8>, Vec<crate::chunker::Chunk>)> = Vec::new();

        for file_path in &bsl_files {
            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(e) => {
                    warn!(?file_path, "failed to read file: {e}");
                    continue;
                }
            };

            let hash = blake3::hash(content.as_bytes());
            let rel_path =
                file_path.strip_prefix(root).unwrap_or(file_path).to_string_lossy().to_string();

            // Skip if hash unchanged.
            if let Some(stored_hash) = self.store.file_hash(&rel_path)? {
                if stored_hash == hash.as_bytes() {
                    continue;
                }
            }

            let chunks = Chunker::chunk(&content);
            if chunks.is_empty() {
                continue;
            }

            let module_path = file_path_to_module_path(&rel_path);
            for chunk in &chunks {
                pending_texts.push(enrich_chunk_text(chunk, &module_path));
            }
            pending_chunks_count.push(chunks.len());
            pending_meta.push((rel_path, hash.as_bytes().to_vec(), chunks));
        }

        if pending_texts.is_empty() {
            info!("no files need reindexing");
            return Ok(0);
        }

        let embedder = self.embedder.as_ref().ok_or_else(|| {
            SearchError::Embedder(
                "Cannot generate embeddings: embedder not configured. Set EMBEDDING_URL.".into(),
            )
        })?;

        // Generate embeddings in parallel batches.
        let total_texts = pending_texts.len();
        let batch_size = self.batch_size;
        let batches: Vec<Vec<String>> =
            pending_texts.chunks(batch_size).map(|c| c.to_vec()).collect();

        // Update progress tracker.
        if let Some(p) = &progress {
            p.active.store(true, Ordering::Relaxed);
            p.total_files.store(pending_meta.len(), Ordering::Relaxed);
            p.total_chunks.store(total_texts, Ordering::Relaxed);
            p.total_batches.store(batches.len(), Ordering::Relaxed);
            p.done_batches.store(0, Ordering::Relaxed);
            p.done_chunks.store(0, Ordering::Relaxed);
        }

        info!(
            files = pending_meta.len(),
            chunks = total_texts,
            batches = batches.len(),
            "generating embeddings"
        );

        let batch_results: Vec<Result<Vec<Vec<f32>>, SearchError>> = batches
            .par_iter()
            .map(|batch| {
                let texts: Vec<&str> = batch.iter().map(|s| s.as_str()).collect();
                let result = embedder.embed_batch(&texts);
                if let Some(p) = &progress {
                    let done =
                        p.done_chunks.fetch_add(texts.len(), Ordering::Relaxed) + texts.len();
                    p.done_batches.fetch_add(1, Ordering::Relaxed);
                    debug!(progress = done, total = total_texts, "embedding batch done");
                }
                result
            })
            .collect();

        if let Some(p) = &progress {
            p.active.store(false, Ordering::Relaxed);
        }

        let mut all_embeddings = Vec::with_capacity(total_texts);
        for result in batch_results {
            all_embeddings.extend(result?);
        }

        // Store chunks with embeddings.
        let mut emb_offset = 0;
        for (i, (rel_path, hash, chunks)) in pending_meta.iter().enumerate() {
            let chunk_count = pending_chunks_count[i];
            let chunk_embeddings: Vec<Vec<f32>> =
                all_embeddings[emb_offset..emb_offset + chunk_count].to_vec();

            self.store.reindex_file(rel_path, hash, chunks, Some(&chunk_embeddings))?;

            emb_offset += chunk_count;
        }

        // Rebuild HNSW index from all embeddings.
        let data = self.store.load_all_embeddings(self.dim)?;
        self.index = VectorIndex::build(self.dim, &data)?;

        let indexed = pending_meta.len();
        info!(indexed, total_vectors = self.index.len(), "indexing complete");
        Ok(indexed)
    }

    /// Index BSL files for FTS only (no embeddings).
    ///
    /// Much faster than full indexing — only parses and chunks code.
    /// Returns the number of files indexed (new or updated).
    pub fn index_directory_fts(&mut self, root: &Path) -> Result<usize, SearchError> {
        let bsl_files: Vec<std::path::PathBuf> = walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")))
            .map(|e| e.into_path())
            .collect();

        info!(total_files = bsl_files.len(), "scanning BSL files (FTS-only)");

        let mut indexed = 0;
        for file_path in &bsl_files {
            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(e) => {
                    warn!(?file_path, "failed to read file: {e}");
                    continue;
                }
            };

            let hash = blake3::hash(content.as_bytes());
            let rel_path =
                file_path.strip_prefix(root).unwrap_or(file_path).to_string_lossy().to_string();

            // Skip if hash unchanged.
            if let Some(stored_hash) = self.store.file_hash(&rel_path)? {
                if stored_hash == hash.as_bytes() {
                    continue;
                }
            }

            let chunks = Chunker::chunk(&content);
            if chunks.is_empty() {
                continue;
            }

            // Store chunks without embeddings.
            self.store.reindex_file(&rel_path, hash.as_bytes(), &chunks, None)?;
            indexed += 1;
        }

        info!(indexed, total_chunks = self.store.chunk_count()?, "FTS indexing complete");
        Ok(indexed)
    }

    /// Index documents in a named collection (e.g. platform reference).
    ///
    /// Documents are stored under a virtual file path. Hash-based
    /// deduplication prevents re-indexing unchanged data.
    /// If embedder is configured, generates embeddings for semantic search.
    ///
    /// Returns the number of documents indexed (0 if hash unchanged).
    pub fn index_documents(
        &mut self,
        collection: &str,
        virtual_path: &str,
        version_hash: &[u8],
        documents: &[Document],
        progress: Option<&Arc<IndexProgress>>,
    ) -> Result<usize, SearchError> {
        // Skip if hash unchanged.
        if let Some(stored_hash) = self.store.file_hash(virtual_path)? {
            if stored_hash == version_hash {
                info!(collection, documents = documents.len(), "documents unchanged, skipping");
                return Ok(0);
            }
        }

        info!(collection, documents = documents.len(), "indexing documents");

        if let Some(embedder) = &self.embedder {
            // Generate embeddings for all document bodies.
            let texts: Vec<&str> = documents.iter().map(|d| d.body.as_str()).collect();
            let total_texts = texts.len();
            let batch_size = self.batch_size;
            let batches: Vec<Vec<&str>> = texts.chunks(batch_size).map(|c| c.to_vec()).collect();

            if let Some(p) = progress {
                p.active.store(true, Ordering::Relaxed);
                p.total_files.store(1, Ordering::Relaxed);
                p.total_chunks.store(total_texts, Ordering::Relaxed);
                p.total_batches.store(batches.len(), Ordering::Relaxed);
                p.done_batches.store(0, Ordering::Relaxed);
                p.done_chunks.store(0, Ordering::Relaxed);
            }

            let batch_results: Vec<Result<Vec<Vec<f32>>, SearchError>> = batches
                .par_iter()
                .map(|batch| {
                    let result = embedder.embed_batch(batch);
                    if let Some(p) = progress {
                        p.done_chunks.fetch_add(batch.len(), Ordering::Relaxed);
                        p.done_batches.fetch_add(1, Ordering::Relaxed);
                    }
                    result
                })
                .collect();

            if let Some(p) = progress {
                p.active.store(false, Ordering::Relaxed);
            }

            let mut all_embeddings = Vec::with_capacity(total_texts);
            for result in batch_results {
                all_embeddings.extend(result?);
            }

            self.store.reindex_documents(
                collection,
                virtual_path,
                version_hash,
                documents,
                Some(&all_embeddings),
            )?;

            // Rebuild HNSW index.
            let data = self.store.load_all_embeddings(self.dim)?;
            self.index = VectorIndex::build(self.dim, &data)?;
        } else {
            // FTS-only: store documents without embeddings.
            self.store.reindex_documents(
                collection,
                virtual_path,
                version_hash,
                documents,
                None,
            )?;
        }

        let count = documents.len();
        info!(collection, count, "document indexing complete");
        Ok(count)
    }

    /// Whether semantic search is available (embedder configured).
    pub fn has_semantic(&self) -> bool {
        self.embedder.is_some()
    }

    /// Semantic search, optionally filtered by collection.
    ///
    /// If `collection` is `None`, searches across all collections.
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        collection: Option<&str>,
    ) -> Result<Vec<SearchHit>, SearchError> {
        let embedder = self.embedder.as_ref().ok_or_else(|| {
            SearchError::Embedder(
                "Semantic search not configured. Set EMBEDDING_URL to enable.".into(),
            )
        })?;
        let query_embedding = embedder.embed(query)?;

        // Request extra results to account for collection filtering.
        let fetch_limit = if collection.is_some() { limit * 3 } else { limit };
        let results = self.index.search(&query_embedding, fetch_limit)?;

        let mut hits = Vec::with_capacity(limit);
        for result in results {
            if hits.len() >= limit {
                break;
            }
            if let Some(info) = self.store.chunk_by_id(result.chunk_id)? {
                if let Some(coll) = collection {
                    if info.collection != coll {
                        continue;
                    }
                }
                hits.push(SearchHit {
                    collection: info.collection,
                    file_path: info.file_path,
                    symbol_name: info.symbol_name,
                    kind: info.kind,
                    text: info.text,
                    line_start: info.line_start,
                    line_end: info.line_end,
                    score: result.score,
                });
            }
        }

        Ok(hits)
    }

    /// Full-text search, optionally filtered by collection.
    ///
    /// Uses SQLite FTS5 for lexical matching — good for exact names,
    /// variable references, API calls, and string literals.
    pub fn text_search(
        &self,
        query: &str,
        limit: usize,
        collection: Option<&str>,
    ) -> Result<Vec<SearchHit>, SearchError> {
        let results = self.store.text_search(query, limit, collection)?;

        let mut hits = Vec::with_capacity(results.len());
        for result in results {
            if let Some(info) = self.store.chunk_by_id(result.chunk_id)? {
                // Normalize FTS5 rank (negative, lower = better) to 0..1 score.
                let score = 1.0 / (1.0 - result.rank as f32);
                hits.push(SearchHit {
                    collection: info.collection,
                    file_path: info.file_path,
                    symbol_name: info.symbol_name,
                    kind: info.kind,
                    text: info.text,
                    line_start: info.line_start,
                    line_end: info.line_end,
                    score,
                });
            }
        }

        Ok(hits)
    }

    /// Number of indexed chunks.
    pub fn chunk_count(&self) -> Result<usize, SearchError> {
        self.store.chunk_count()
    }

    /// Number of indexed files.
    pub fn file_count(&self) -> Result<usize, SearchError> {
        self.store.file_count()
    }

    /// Number of vectors in the HNSW index.
    pub fn vector_count(&self) -> usize {
        self.index.len()
    }

    /// Remove a file from the index.
    pub fn remove_file(&mut self, rel_path: &str) -> Result<(), SearchError> {
        self.store.remove_file(rel_path)?;
        let data = self.store.load_all_embeddings(self.dim)?;
        self.index = VectorIndex::build(self.dim, &data)?;
        Ok(())
    }
}
