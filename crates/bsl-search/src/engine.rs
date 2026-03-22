//! Search engine — ties chunker, embedder, store, and index together.
//!
//! Provides the high-level API for indexing BSL files and platform
//! documentation, and searching through them using FTS5 or semantic
//! similarity. Supports multiple collections within a single database.
//!
//! Embedding generation uses a pool of N concurrent HTTP connections
//! (default 10) to maximize throughput against remote APIs.

use crate::chunker::Chunker;
use crate::context::{enrich_chunk_text, file_path_to_module_path};
use crate::document::Document;
use crate::embedder::{Embedder, EmbedderConfig};
use crate::error::SearchError;
use crate::index::VectorIndex;
use crate::store::Store;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Default number of concurrent embedding connections.
const DEFAULT_CONCURRENCY: usize = 10;

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
    /// Number of concurrent embedding connections (default 10).
    pub concurrency: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            embedder: EmbedderConfig::default(),
            batch_size: 32,
            concurrency: DEFAULT_CONCURRENCY,
        }
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
    concurrency: usize,
}

impl SearchEngine {
    /// Create a search engine with full capabilities (FTS + semantic).
    ///
    /// - `db_path`: path to the SQLite database file
    /// - `config`: search configuration (embedder + batch size + concurrency)
    pub fn new(db_path: &Path, config: SearchConfig) -> Result<Self, SearchError> {
        let store = Store::open(db_path)?;
        let dim = config.embedder.dim.unwrap_or(1024);
        let embedder = Embedder::new(config.embedder);

        // Load existing embeddings from store into HNSW index.
        let data = store.load_all_embeddings(dim)?;
        let index = VectorIndex::build(dim, &data)?;
        info!(vectors = index.len(), dim, "search index loaded");

        Self::ensure_fts(&store)?;

        Ok(Self {
            store,
            embedder: Some(embedder),
            index,
            dim,
            batch_size: config.batch_size,
            concurrency: config.concurrency,
        })
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

        Ok(Self {
            store,
            embedder: None,
            index,
            dim,
            batch_size: 32,
            concurrency: DEFAULT_CONCURRENCY,
        })
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
    /// Uses a pool of N concurrent workers for embedding generation.
    /// Each worker has its own HTTP connection to the embedding API.
    /// The main thread acts as writer — receives completed files and
    /// writes them to SQLite immediately (resumable on interruption).
    ///
    /// If `progress` is provided, workers update it atomically.
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

        let embedder = self.embedder.as_ref().ok_or_else(|| {
            SearchError::Embedder(
                "Cannot generate embeddings: embedder not configured. Set EMBEDDING_URL.".into(),
            )
        })?;

        // First pass: collect files that need reindexing.
        let mut tasks: Vec<FileTask> = Vec::new();
        let mut total_chunks = 0usize;

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
            let texts: Vec<String> =
                chunks.iter().map(|c| enrich_chunk_text(c, &module_path)).collect();

            total_chunks += chunks.len();
            tasks.push(FileTask { rel_path, hash: hash.as_bytes().to_vec(), chunks, texts });
        }

        if tasks.is_empty() {
            info!("no files need reindexing");
            return Ok(0);
        }

        let batch_size = self.batch_size;
        let total_batches: usize = tasks.iter().map(|t| t.texts.len().div_ceil(batch_size)).sum();

        if let Some(p) = &progress {
            p.active.store(true, Ordering::Relaxed);
            p.total_files.store(tasks.len(), Ordering::Relaxed);
            p.total_chunks.store(total_chunks, Ordering::Relaxed);
            p.total_batches.store(total_batches, Ordering::Relaxed);
            p.done_batches.store(0, Ordering::Relaxed);
            p.done_chunks.store(0, Ordering::Relaxed);
        }

        let concurrency = self.concurrency.min(tasks.len());
        info!(
            files = tasks.len(),
            chunks = total_chunks,
            batches = total_batches,
            concurrency,
            "generating embeddings"
        );

        // Set up worker pool.
        let (task_tx, task_rx) = crossbeam_channel::bounded::<FileTask>(concurrency * 2);
        let (result_tx, result_rx) = crossbeam_channel::bounded::<FileResult>(concurrency * 2);

        let workers: Vec<std::thread::JoinHandle<()>> = (0..concurrency)
            .map(|_| {
                let rx = task_rx.clone();
                let tx = result_tx.clone();
                let emb = embedder.clone();
                let bs = batch_size;
                let prog = progress.cloned();

                std::thread::spawn(move || {
                    while let Ok(task) = rx.recv() {
                        let mut embeddings = Vec::with_capacity(task.texts.len());
                        let mut error = None;

                        for batch in task.texts.chunks(bs) {
                            let refs: Vec<&str> = batch.iter().map(|s| s.as_str()).collect();
                            match emb.embed_batch(&refs) {
                                Ok(embs) => {
                                    embeddings.extend(embs);
                                    if let Some(p) = &prog {
                                        p.done_chunks.fetch_add(batch.len(), Ordering::Relaxed);
                                        p.done_batches.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                                Err(e) => {
                                    error = Some(e);
                                    break;
                                }
                            }
                        }

                        let _ = tx.send(FileResult {
                            rel_path: task.rel_path,
                            hash: task.hash,
                            chunks: task.chunks,
                            embeddings: match error {
                                None => Ok(embeddings),
                                Some(e) => Err(e),
                            },
                        });
                    }
                })
            })
            .collect();

        // Drop our copies — workers hold theirs.
        drop(task_rx);
        drop(result_tx);

        // Producer thread sends tasks while main thread reads results.
        // Both channels are bounded, so producer and workers apply
        // backpressure to each other — no deadlock.
        let producer = std::thread::spawn(move || {
            for task in tasks {
                if task_tx.send(task).is_err() {
                    break;
                }
            }
            // task_tx dropped here, closing the channel.
        });

        // Main thread = writer: receive completed files and write to DB.
        let mut indexed = 0usize;
        let mut errors = 0usize;
        while let Ok(result) = result_rx.recv() {
            match result.embeddings {
                Ok(embeddings) => {
                    self.store.reindex_file(
                        &result.rel_path,
                        &result.hash,
                        &result.chunks,
                        Some(&embeddings),
                    )?;
                    indexed += 1;
                    debug!(file = %result.rel_path, chunks = result.chunks.len(), "file indexed");
                }
                Err(e) => {
                    warn!(file = %result.rel_path, "embedding failed after retries, skipping: {e}");
                    errors += 1;
                }
            }
        }

        let _ = producer.join();
        for w in workers {
            let _ = w.join();
        }

        if let Some(p) = &progress {
            p.active.store(false, Ordering::Relaxed);
        }

        // Rebuild HNSW index from all embeddings.
        let data = self.store.load_all_embeddings(self.dim)?;
        self.index = VectorIndex::build(self.dim, &data)?;

        if errors > 0 {
            info!(
                indexed,
                errors,
                total_vectors = self.index.len(),
                "indexing complete with errors"
            );
        } else {
            info!(indexed, total_vectors = self.index.len(), "indexing complete");
        }
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
    ///
    /// If embedder is configured, generates embeddings using a pool of
    /// concurrent workers for parallel API calls.
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
            let texts: Vec<String> = documents.iter().map(|d| d.body.clone()).collect();
            let batch_size = self.batch_size;
            let total_batches = texts.len().div_ceil(batch_size);

            if let Some(p) = progress {
                p.active.store(true, Ordering::Relaxed);
                p.total_files.store(1, Ordering::Relaxed);
                p.total_chunks.store(texts.len(), Ordering::Relaxed);
                p.total_batches.store(total_batches, Ordering::Relaxed);
                p.done_batches.store(0, Ordering::Relaxed);
                p.done_chunks.store(0, Ordering::Relaxed);
            }

            let concurrency = self.concurrency.min(total_batches.max(1));

            // Split texts into owned batches with indices for ordered reassembly.
            let indexed_batches: Vec<(usize, Vec<String>)> =
                texts.chunks(batch_size).enumerate().map(|(i, b)| (i, b.to_vec())).collect();

            let (task_tx, task_rx) =
                crossbeam_channel::bounded::<(usize, Vec<String>)>(concurrency * 2);
            let (result_tx, result_rx) = crossbeam_channel::bounded::<(
                usize,
                Result<Vec<Vec<f32>>, SearchError>,
            )>(concurrency * 2);

            let workers: Vec<std::thread::JoinHandle<()>> = (0..concurrency)
                .map(|_| {
                    let rx = task_rx.clone();
                    let tx = result_tx.clone();
                    let emb = embedder.clone();
                    let prog = progress.cloned();

                    std::thread::spawn(move || {
                        while let Ok((idx, batch)) = rx.recv() {
                            let refs: Vec<&str> = batch.iter().map(|s| s.as_str()).collect();
                            let result = emb.embed_batch(&refs);
                            if let (Ok(_), Some(p)) = (&result, &prog) {
                                p.done_chunks.fetch_add(batch.len(), Ordering::Relaxed);
                                p.done_batches.fetch_add(1, Ordering::Relaxed);
                            }
                            let _ = tx.send((idx, result));
                        }
                    })
                })
                .collect();

            drop(task_rx);
            drop(result_tx);

            // Producer thread sends batches while main thread reads results.
            let producer = std::thread::spawn(move || {
                for (idx, batch) in indexed_batches {
                    if task_tx.send((idx, batch)).is_err() {
                        break;
                    }
                }
            });

            // Collect results and reassemble in order.
            let mut results: Vec<(usize, Vec<Vec<f32>>)> = Vec::with_capacity(total_batches);
            while let Ok((idx, result)) = result_rx.recv() {
                results.push((idx, result?));
            }

            let _ = producer.join();
            for w in workers {
                let _ = w.join();
            }

            if let Some(p) = progress {
                p.active.store(false, Ordering::Relaxed);
            }

            results.sort_by_key(|(i, _)| *i);
            let all_embeddings: Vec<Vec<f32>> =
                results.into_iter().flat_map(|(_, embs)| embs).collect();

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

    /// Count embeddings in a specific collection.
    pub fn embedding_count_by_collection(&self, collection: &str) -> Result<usize, SearchError> {
        self.store.embedding_count_by_collection(collection)
    }

    /// Clear file hashes for a collection (forces re-indexing).
    pub fn clear_file_hashes(&self, collection: &str) -> Result<usize, SearchError> {
        self.store.clear_file_hashes(collection)
    }

    /// Clear file hashes only for files without embeddings.
    pub fn clear_file_hashes_without_embeddings(
        &self,
        collection: &str,
    ) -> Result<usize, SearchError> {
        self.store.clear_file_hashes_without_embeddings(collection)
    }

    /// Remove a file from the index.
    pub fn remove_file(&mut self, rel_path: &str) -> Result<(), SearchError> {
        self.store.remove_file(rel_path)?;
        let data = self.store.load_all_embeddings(self.dim)?;
        self.index = VectorIndex::build(self.dim, &data)?;
        Ok(())
    }
}

// -- Internal types for worker pool communication --

/// Task sent to an embedding worker (one file).
struct FileTask {
    rel_path: String,
    hash: Vec<u8>,
    chunks: Vec<crate::chunker::Chunk>,
    texts: Vec<String>,
}

/// Result from an embedding worker (one file).
struct FileResult {
    rel_path: String,
    hash: Vec<u8>,
    chunks: Vec<crate::chunker::Chunk>,
    embeddings: Result<Vec<Vec<f32>>, SearchError>,
}
