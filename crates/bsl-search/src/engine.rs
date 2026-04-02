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
use crate::local_baseline::LocalStoreBaselineAdapter;
use crate::ports::{SnapshotCatalog, SnapshotContentStore};
use crate::publish::EmbeddingExecutionPolicy;
use crate::resolver::{InMemoryResolvedViewResolver, ResolvedView};
use crate::store::Store;
use crate::workspace_overlay::{
    lexical_hits, normalized_file_hash_for_indexed_documents, semantic_hits, BaselineHashMode,
    WorkspaceOverlayCache, WorkspaceOverlayIndex, WorkspaceOverlayStats,
};
use crate::{
    semantic_key_for_indexed_document, semantic_text_for_indexed_document,
    BaselineOverlaySearchService, BaselineRef, CorpusId,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
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
#[derive(Default)]
pub struct SearchConfig {
    /// Embedding API configuration.
    pub embedder: EmbedderConfig,
    /// Execution policy for embedding generation.
    pub execution: EmbeddingExecutionPolicy,
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
    workspace_root: Option<std::path::PathBuf>,
    workspace_overlay_cache: Mutex<WorkspaceOverlayCache>,
    workspace_baseline_hash_mode: BaselineHashMode,
}

impl SearchEngine {
    /// Create a search engine with full capabilities (FTS + semantic).
    ///
    /// - `db_path`: path to the SQLite database file
    /// - `config`: search configuration (embedder + batch size + concurrency)
    pub fn new(db_path: &Path, config: SearchConfig) -> Result<Self, SearchError> {
        let SearchConfig { embedder: embedder_config, execution } = config;
        let store = Store::open(db_path)?;
        let dim = embedder_config.dim.unwrap_or(1024);
        let embedder = Embedder::new(embedder_config);

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
            batch_size: execution.batch_size(),
            concurrency: execution.concurrency(),
            workspace_root: None,
            workspace_overlay_cache: Mutex::new(WorkspaceOverlayCache::default()),
            workspace_baseline_hash_mode: BaselineHashMode::RawFileBytes,
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
            batch_size: EmbeddingExecutionPolicy::default().batch_size(),
            concurrency: EmbeddingExecutionPolicy::default().concurrency(),
            workspace_root: None,
            workspace_overlay_cache: Mutex::new(WorkspaceOverlayCache::default()),
            workspace_baseline_hash_mode: BaselineHashMode::RawFileBytes,
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

    pub fn embedding_model(&self) -> Option<&str> {
        self.embedder.as_ref().map(Embedder::model)
    }

    pub fn embedding_dimension(&self) -> Option<usize> {
        self.embedder.as_ref().map(Embedder::dim)
    }

    /// Attach a workspace source root for building a local overlay view.
    pub fn set_workspace_root(&mut self, workspace_root: impl Into<std::path::PathBuf>) {
        self.workspace_root = Some(workspace_root.into());
        if let Ok(mut cache) = self.workspace_overlay_cache.lock() {
            cache.clear();
        }
    }

    /// Enable watcher-driven overlay refresh mode.
    pub fn enable_workspace_watcher_mode(&mut self) {
        if let Ok(mut cache) = self.workspace_overlay_cache.lock() {
            cache.enable_watcher_mode();
        }
    }

    pub fn set_workspace_baseline_hash_mode(&mut self, hash_mode: BaselineHashMode) {
        self.workspace_baseline_hash_mode = hash_mode;
        if let Ok(mut cache) = self.workspace_overlay_cache.lock() {
            cache.clear();
        }
    }

    /// Mark one workspace file as dirty for the overlay cache.
    ///
    /// The path can be absolute inside the configured workspace root or already relative.
    pub fn mark_workspace_path_dirty(
        &self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<bool, SearchError> {
        let workspace_root = match &self.workspace_root {
            Some(root) => root,
            None => return Ok(false),
        };
        let path = path.as_ref();
        let rel_path = if path.is_absolute() {
            match path.strip_prefix(workspace_root) {
                Ok(rel) => rel,
                Err(_) => return Ok(false),
            }
        } else {
            path
        };

        if !rel_path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")) {
            return Ok(false);
        }

        let rel_path = rel_path.to_string_lossy().to_string();
        let mut cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        cache.enable_watcher_mode();
        cache.mark_dirty_path(rel_path);
        Ok(true)
    }

    /// Get current workspace overlay statistics.
    ///
    /// Performs a lightweight refresh using file metadata and hashes, but does not
    /// force semantic embedding generation for status queries.
    pub fn workspace_overlay_stats(&self) -> Result<Option<WorkspaceOverlayStats>, SearchError> {
        let Some(workspace_root) = &self.workspace_root else {
            return Ok(None);
        };
        let mut cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        cache.refresh(
            &self.store,
            workspace_root,
            None,
            self.batch_size,
            self.workspace_baseline_hash_mode,
        )?;
        Ok(Some(cache.stats()))
    }

    /// Materialize the current workspace code view from the persisted local
    /// baseline plus the live workspace overlay.
    ///
    /// This does not replace the current search runtime yet. It provides a
    /// real runtime integration point for the baseline + overlay architecture
    /// so diagnostics and future backend switching can reuse the same flow.
    pub fn resolve_workspace_code_view(&self) -> Result<Option<ResolvedView>, SearchError> {
        self.resolve_workspace_code_view_with(
            BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "local-workspace-baseline"),
            LocalStoreBaselineAdapter::workspace_code(&self.store),
            LocalStoreBaselineAdapter::workspace_code(&self.store),
        )
    }

    /// Materialize the current workspace code view against an arbitrary
    /// baseline source.
    ///
    /// This is the bridge between the local live overlay and pluggable
    /// baseline backends such as SQLite today and PostgreSQL later.
    pub fn resolve_workspace_code_view_with<C, S>(
        &self,
        baseline: BaselineRef,
        catalog: C,
        content_store: S,
    ) -> Result<Option<ResolvedView>, SearchError>
    where
        C: SnapshotCatalog,
        S: SnapshotContentStore,
    {
        if self.workspace_root.is_none() {
            return Ok(None);
        }

        let overlay = self.workspace_overlay_snapshot(None)?;
        let mut overlay = overlay.overlay;
        overlay.baseline = baseline.clone();
        let service =
            BaselineOverlaySearchService::new(catalog, content_store, InMemoryResolvedViewResolver);

        service.resolve_view(baseline, overlay)
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
        if collection == Some("code") {
            if let Some(overlay_hits) = self.search_with_workspace_overlay(query, limit)? {
                return Ok(overlay_hits);
            }
        }

        self.search_persisted(query, limit, collection)
    }

    fn search_persisted(
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

    fn search_with_workspace_overlay(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Option<Vec<SearchHit>>, SearchError> {
        if self.workspace_root.is_none() {
            return Ok(None);
        }
        let Some(embedder) = &self.embedder else {
            return Ok(None);
        };

        let overlay = self.workspace_overlay_snapshot(Some(embedder))?;
        if overlay.is_empty() {
            return Ok(None);
        }

        let query_embedding = embedder.embed(query)?;
        let mut combined = self.search_persisted(query, limit * 3, Some("code"))?;
        combined.retain(|hit| !overlay.hidden_paths.contains(&hit.file_path));
        combined.extend(semantic_hits(&overlay, &query_embedding, limit));
        combined.sort_by(|lhs, rhs| rhs.score.total_cmp(&lhs.score));
        combined.truncate(limit);
        Ok(Some(combined))
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
        if collection == Some("code") {
            if let Some(overlay_hits) = self.text_search_with_workspace_overlay(query, limit)? {
                return Ok(overlay_hits);
            }
        }

        self.text_search_persisted(query, limit, collection)
    }

    fn text_search_persisted(
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

    fn text_search_with_workspace_overlay(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Option<Vec<SearchHit>>, SearchError> {
        let Some(workspace_root) = &self.workspace_root else {
            return Ok(None);
        };

        let _ = workspace_root;
        let overlay = self.workspace_overlay_snapshot(None)?;
        if overlay.is_empty() {
            return Ok(None);
        }

        let mut combined = self.text_search_persisted(query, limit * 3, Some("code"))?;
        combined.retain(|hit| !overlay.hidden_paths.contains(&hit.file_path));
        combined.extend(lexical_hits(&overlay, query, limit));
        combined.sort_by(|lhs, rhs| rhs.score.total_cmp(&lhs.score));
        combined.truncate(limit);
        Ok(Some(combined))
    }

    fn workspace_overlay_snapshot(
        &self,
        embedder: Option<&Embedder>,
    ) -> Result<WorkspaceOverlayIndex, SearchError> {
        let workspace_root = self
            .workspace_root
            .as_ref()
            .ok_or_else(|| SearchError::Index("workspace root is not configured".to_owned()))?;
        let mut cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        cache.refresh(
            &self.store,
            workspace_root,
            embedder,
            self.batch_size,
            self.workspace_baseline_hash_mode,
        )?;
        Ok(cache.snapshot())
    }

    pub fn sync_indexed_documents_in_collection(
        &mut self,
        collection: &str,
        documents: &[crate::IndexedDocument],
        progress: Option<&Arc<IndexProgress>>,
    ) -> Result<usize, SearchError> {
        self.sync_indexed_documents_in_collection_with_embeddings(
            collection, documents, None, progress,
        )
    }

    pub fn sync_indexed_documents_in_collection_with_embeddings(
        &mut self,
        collection: &str,
        documents: &[crate::IndexedDocument],
        shared_embeddings: Option<&HashMap<String, Vec<f32>>>,
        progress: Option<&Arc<IndexProgress>>,
    ) -> Result<usize, SearchError> {
        use std::collections::{BTreeMap, HashSet};

        let mut grouped = BTreeMap::<String, Vec<crate::IndexedDocument>>::new();
        for document in documents {
            grouped.entry(document.path.clone()).or_default().push(document.clone());
        }

        let desired_paths: HashSet<&str> = grouped.keys().map(String::as_str).collect();
        for (existing_path, _) in self.store.all_files_in_collection(collection)? {
            if !desired_paths.contains(existing_path.as_str()) {
                self.store.remove_file(&existing_path)?;
            }
        }

        let total_chunks = documents.len();
        if let Some(p) = progress {
            p.active.store(true, Ordering::Relaxed);
            p.total_files.store(grouped.len(), Ordering::Relaxed);
            p.total_chunks.store(total_chunks, Ordering::Relaxed);
            p.total_batches.store(total_chunks.div_ceil(self.batch_size.max(1)), Ordering::Relaxed);
            p.done_batches.store(0, Ordering::Relaxed);
            p.done_chunks.store(0, Ordering::Relaxed);
        }

        let mut indexed = 0usize;
        for (path, mut file_documents) in grouped {
            file_documents.sort_by(|lhs, rhs| {
                (lhs.line_start, lhs.line_end, lhs.symbol_name.as_str()).cmp(&(
                    rhs.line_start,
                    rhs.line_end,
                    rhs.symbol_name.as_str(),
                ))
            });

            let file_hash = normalized_file_hash_for_indexed_documents(&file_documents);
            if self.store.file_hash(&path)?.as_deref() == Some(file_hash.as_slice()) {
                continue;
            }

            let embeddings = if let Some(embedder) = &self.embedder {
                let mut vectors = vec![Vec::<f32>::new(); file_documents.len()];
                let mut missing_indices = Vec::new();
                let mut missing_texts = Vec::new();

                for (idx, document) in file_documents.iter().enumerate() {
                    let embedding_key = semantic_key_for_indexed_document(document);
                    if let Some(shared_embedding) =
                        shared_embeddings.and_then(|items| items.get(&embedding_key))
                    {
                        vectors[idx] = shared_embedding.clone();
                        if let Some(p) = progress {
                            p.done_chunks.fetch_add(1, Ordering::Relaxed);
                        }
                    } else {
                        missing_indices.push(idx);
                        missing_texts.push(semantic_text_for_indexed_document(document));
                    }
                }

                let mut cursor = 0usize;
                for batch in missing_texts.chunks(self.batch_size.max(1)) {
                    let refs = batch.iter().map(String::as_str).collect::<Vec<_>>();
                    let batch_vectors = embedder.embed_batch(&refs)?;
                    if let Some(p) = progress {
                        p.done_chunks.fetch_add(batch.len(), Ordering::Relaxed);
                        p.done_batches.fetch_add(1, Ordering::Relaxed);
                    }

                    for (offset, embedding) in batch_vectors.into_iter().enumerate() {
                        let idx = missing_indices[cursor + offset];
                        vectors[idx] = embedding;
                    }
                    cursor += batch.len();
                }

                Some(vectors)
            } else {
                None
            };

            self.store.reindex_indexed_documents_in_collection(
                &path,
                &file_hash,
                collection,
                &file_documents,
                embeddings.as_deref(),
            )?;
            indexed += 1;
        }

        if let Some(p) = progress {
            p.active.store(false, Ordering::Relaxed);
        }

        let data = self.store.load_all_embeddings(self.dim)?;
        self.index = VectorIndex::build(self.dim, &data)?;
        Ok(indexed)
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

    /// Export indexed documents, optionally filtered by collection.
    pub fn load_indexed_documents(
        &self,
        collection: Option<&str>,
    ) -> Result<Vec<crate::IndexedDocument>, SearchError> {
        self.store.load_indexed_documents(collection)
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

#[cfg(test)]
mod tests {
    use super::SearchEngine;
    use crate::ports::{SnapshotCatalog, SnapshotContentStore};
    use crate::{BaselineRef, CorpusId, IndexedDocument, SearchError, Snapshot};
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::fs;
    use tempfile::tempdir;

    #[derive(Default)]
    struct TestCatalog {
        snapshots: HashMap<String, Snapshot>,
    }

    impl SnapshotCatalog for TestCatalog {
        fn resolve_baseline(
            &self,
            baseline: &BaselineRef,
        ) -> Result<Option<Snapshot>, SearchError> {
            let id = baseline.snapshot_id.as_ref().map(|id| id.0.as_str()).unwrap_or_default();
            Ok(self.snapshots.get(id).cloned())
        }
    }

    #[derive(Default)]
    struct TestContentStore {
        documents: HashMap<String, Vec<IndexedDocument>>,
    }

    impl SnapshotContentStore for TestContentStore {
        fn load_snapshot_documents(
            &self,
            snapshot: &Snapshot,
        ) -> Result<Vec<IndexedDocument>, SearchError> {
            Ok(self.documents.get(&snapshot.id.0).cloned().unwrap_or_default())
        }
    }

    #[test]
    fn text_search_sees_workspace_overlay_without_reindex() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("CommonModule.bsl");
        fs::write(&file, "Процедура СтараяПроцедура()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);

        fs::write(&file, "Процедура НоваяПроцедура()\nКонецПроцедуры").unwrap();

        let hits = engine.text_search("НоваяПроцедура", 10, Some("code")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].symbol_name, "НоваяПроцедура");
    }

    #[test]
    fn text_search_hides_deleted_baseline_file() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("CommonModule.bsl");
        fs::write(&file, "Процедура УдаляемаяПроцедура()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);

        fs::remove_file(&file).unwrap();

        let hits = engine.text_search("УдаляемаяПроцедура", 10, Some("code")).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn workspace_overlay_stats_report_changed_files() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("CommonModule.bsl");
        fs::write(&file, "Процедура СтараяПроцедура()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);

        fs::write(&file, "Процедура НоваяПроцедура()\nКонецПроцедуры").unwrap();

        let stats = engine.workspace_overlay_stats().unwrap().unwrap();
        assert_eq!(stats.overlay_files, 1);
        assert_eq!(stats.deleted_files, 0);
        assert_eq!(stats.hidden_paths, 1);
        assert_eq!(stats.lexical_chunks, 1);
    }

    #[test]
    fn resolved_workspace_view_combines_local_baseline_with_overlay() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let changed = workspace.join("ChangedModule.bsl");
        let stable = workspace.join("StableModule.bsl");
        fs::write(&changed, "Процедура СтараяПроцедура()\nКонецПроцедуры").unwrap();
        fs::write(&stable, "Процедура СтабильнаяПроцедура()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);

        fs::write(&changed, "Процедура НоваяПроцедура()\nКонецПроцедуры").unwrap();

        let view = engine.resolve_workspace_code_view().unwrap().unwrap();
        let symbols: HashSet<&str> =
            view.documents().iter().map(|document| document.symbol_name.as_str()).collect();

        assert!(symbols.contains("НоваяПроцедура"));
        assert!(symbols.contains("СтабильнаяПроцедура"));
        assert!(!symbols.contains("СтараяПроцедура"));
    }

    #[test]
    fn resolved_workspace_view_can_target_explicit_baseline_snapshot() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let changed = workspace.join("ChangedModule.bsl");
        fs::write(&changed, "Процедура ЛокальнаяВерсия()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);

        fs::write(&changed, "Процедура OverlayВерсия()\nКонецПроцедуры").unwrap();

        let baseline = BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "external-main");
        let snapshot = Snapshot::new("external-main", CorpusId::WorkspaceCode);
        let mut catalog = TestCatalog::default();
        catalog.snapshots.insert(snapshot.id.0.clone(), snapshot.clone());

        let mut content_store = TestContentStore::default();
        content_store.documents.insert(
            snapshot.id.0.clone(),
            vec![
                IndexedDocument {
                    collection: "code".to_owned(),
                    path: "ChangedModule.bsl".to_owned(),
                    symbol_name: "БазоваяВерсия".to_owned(),
                    kind: "procedure".to_owned(),
                    line_start: 1,
                    line_end: 2,
                    text: "базовая".to_owned(),
                    content_hash: "base-changed".to_owned(),
                },
                IndexedDocument {
                    collection: "code".to_owned(),
                    path: "StableModule.bsl".to_owned(),
                    symbol_name: "СтабильноИзBaseline".to_owned(),
                    kind: "procedure".to_owned(),
                    line_start: 1,
                    line_end: 2,
                    text: "stable".to_owned(),
                    content_hash: "base-stable".to_owned(),
                },
            ],
        );

        let view = engine
            .resolve_workspace_code_view_with(baseline, catalog, content_store)
            .unwrap()
            .unwrap();
        let symbols: HashSet<&str> =
            view.documents().iter().map(|document| document.symbol_name.as_str()).collect();

        assert!(symbols.contains("OverlayВерсия"));
        assert!(symbols.contains("СтабильноИзBaseline"));
        assert!(!symbols.contains("БазоваяВерсия"));
    }

    #[test]
    fn watcher_mode_applies_dirty_file_updates() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("CommonModule.bsl");
        fs::write(&file, "Процедура СтараяПроцедура()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);
        engine.enable_workspace_watcher_mode();

        let initial = engine.workspace_overlay_stats().unwrap().unwrap();
        assert!(initial.watcher_mode);
        assert_eq!(initial.overlay_files, 0);

        fs::write(&file, "Процедура ОбновленаЧерезWatcher()\nКонецПроцедуры").unwrap();
        assert!(engine.mark_workspace_path_dirty(&file).unwrap());

        let hits = engine.text_search("ОбновленаЧерезWatcher", 10, Some("code")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].symbol_name, "ОбновленаЧерезWatcher");
    }
}
