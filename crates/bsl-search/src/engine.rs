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
    RefreshMode, RefreshPlan, WorkspaceOverlayCache, WorkspaceOverlayIndex, WorkspaceOverlayStats,
};
use crate::{
    semantic_key_for_indexed_document, semantic_text_for_indexed_document,
    BaselineOverlaySearchService, BaselineRef, CorpusId,
};
use code_chunk::Chunker;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

#[derive(Debug, Default)]
pub struct IndexProgress {
    pub active: AtomicBool,
    pub total_files: AtomicUsize,
    pub total_chunks: AtomicUsize,
    pub total_batches: AtomicUsize,
    pub done_batches: AtomicUsize,
    pub done_chunks: AtomicUsize,
}

impl IndexProgress {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn reset(&self) {
        self.active.store(false, Ordering::Relaxed);
        self.total_files.store(0, Ordering::Relaxed);
        self.total_chunks.store(0, Ordering::Relaxed);
        self.total_batches.store(0, Ordering::Relaxed);
        self.done_batches.store(0, Ordering::Relaxed);
        self.done_chunks.store(0, Ordering::Relaxed);
    }

    pub fn percent(&self) -> usize {
        let total = self.total_chunks.load(Ordering::Relaxed);
        if total == 0 {
            return 0;
        }
        let done = self.done_chunks.load(Ordering::Relaxed);
        (done * 100) / total
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
pub struct SearchConfig {
    pub embedder: EmbedderConfig,
    pub execution: EmbeddingExecutionPolicy,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub collection: String,
    pub file_path: String,
    pub symbol_name: String,
    pub kind: String,
    pub text: String,
    pub line_start: u32,
    pub line_end: u32,
    pub score: f32,
}

impl SearchHit {
    pub fn from_lexical(hit: &crate::domain::LexicalHit) -> Self {
        Self {
            collection: hit.collection.clone(),
            file_path: hit.path.clone(),
            symbol_name: hit.symbol_name.clone(),
            kind: hit.kind.clone(),
            text: hit.text.clone(),
            line_start: hit.line_start,
            line_end: hit.line_end,
            score: hit.rank,
        }
    }

    pub fn to_lexical(&self) -> crate::domain::LexicalHit {
        crate::domain::LexicalHit {
            collection: self.collection.clone(),
            path: self.file_path.clone(),
            symbol_name: self.symbol_name.clone(),
            kind: self.kind.clone(),
            line_start: self.line_start,
            line_end: self.line_end,
            text: self.text.clone(),
            rank: self.score,
        }
    }

    pub fn to_semantic(&self) -> crate::domain::SemanticHit {
        crate::domain::SemanticHit {
            collection: self.collection.clone(),
            path: self.file_path.clone(),
            symbol_name: self.symbol_name.clone(),
            kind: self.kind.clone(),
            line_start: self.line_start,
            line_end: self.line_end,
            score: self.score,
        }
    }

    pub fn from_merged(hit: crate::merge::MergedHit) -> Self {
        Self {
            collection: hit.collection,
            file_path: hit.path,
            symbol_name: hit.symbol_name,
            kind: hit.kind,
            text: hit.text.unwrap_or_default(),
            line_start: hit.line_start,
            line_end: hit.line_end,
            score: hit.score,
        }
    }
}

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
    /// Optional graph-context provider (dependency-inverted via
    /// [`crate::ports::GraphContextProvider`]). When set, code chunks are enriched
    /// with their outbound graph context before embedding. `None` keeps embeddings
    /// graph-free.
    graph_context_provider: Option<Arc<dyn crate::ports::GraphContextProvider>>,
}

impl SearchEngine {
    pub fn new(db_path: &Path, config: SearchConfig) -> Result<Self, SearchError> {
        let SearchConfig { embedder: embedder_config, execution } = config;
        let store = Store::open(db_path)?;
        let dim = embedder_config.dim.unwrap_or(1024);
        let embedder = Embedder::new(embedder_config);

        let index = Self::load_or_build_index(&store, dim, Some(&embedder))?;
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
            graph_context_provider: None,
        })
    }

    /// Load a persisted vector index when it still matches the current embeddings, otherwise
    /// build it from SQLite and persist the result. Rebuilding the HNSW is the dominant cold-
    /// start cost; loading a prebuilt one is ~40x faster (see `examples/bench_vector_index.rs`).
    /// Only a real, model-backed, file-backed engine persists — in-memory and embedder-less
    /// (FTS-only / overlay) engines fall back to a plain build with no sidecar.
    fn load_or_build_index(
        store: &Store,
        dim: usize,
        embedder: Option<&Embedder>,
    ) -> Result<VectorIndex, SearchError> {
        if let Some(key) = Self::persist_key(store, dim, embedder) {
            if let Some(index) = crate::vector_persist::try_load(store, &key) {
                info!(vectors = index.len(), "loaded persisted vector index");
                return Ok(index);
            }
        }
        Self::build_persisted_index(store, dim, embedder)
    }

    /// Build the vector index from SQLite and persist it (best-effort) when persistence applies.
    /// The persisted fingerprint is taken from the SAME `data` snapshot the index is built from —
    /// never a fresh DB read — so the sidecar can never describe a different state than the saved
    /// index. An empty index is not persisted (e.g. before the deferred embedding pass runs).
    fn build_persisted_index(
        store: &Store,
        dim: usize,
        embedder: Option<&Embedder>,
    ) -> Result<VectorIndex, SearchError> {
        let (generation, data) = store.load_all_embeddings_with_generation(dim)?;
        let index = VectorIndex::build(dim, &data)?;
        Self::persist_built(store, dim, embedder, &index, generation);
        Ok(index)
    }

    /// Persist a freshly built `index` stamped with the `embedding_generation` of the snapshot it
    /// was built from. Best-effort and gated: only a model-backed, file-backed engine with a
    /// non-empty index writes a sidecar; in-memory/FTS-only/overlay engines and the pre-embedding
    /// empty state are skipped.
    fn persist_built(
        store: &Store,
        dim: usize,
        embedder: Option<&Embedder>,
        index: &VectorIndex,
        generation: i64,
    ) {
        if index.is_empty() {
            return;
        }
        if let Some(key) = Self::persist_key(store, dim, embedder) {
            if let Err(e) = crate::vector_persist::persist(index, &key, generation) {
                warn!("failed to persist vector index: {e}");
            }
        }
    }

    /// The persistence identity for this engine's index, or `None` when persistence does not
    /// apply (no embedder, or an in-memory database).
    fn persist_key<'a>(
        store: &'a Store,
        dim: usize,
        embedder: Option<&'a Embedder>,
    ) -> Option<crate::vector_persist::PersistKey<'a>> {
        let model_id = embedder?.model();
        if store.db_path() == Path::new(":memory:") {
            return None;
        }
        Some(crate::vector_persist::PersistKey { db_path: store.db_path(), model_id, dim })
    }

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
            graph_context_provider: None,
        })
    }

    pub fn semantic_overlay_only(
        db_path: &Path,
        config: SearchConfig,
    ) -> Result<Self, SearchError> {
        let SearchConfig { embedder: embedder_config, execution } = config;
        let store = Store::open(db_path)?;
        let dim = embedder_config.dim.unwrap_or(1024);
        let embedder = Embedder::new(embedder_config);
        let index = VectorIndex::new(dim)?;

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
            graph_context_provider: None,
        })
    }

    fn ensure_fts(store: &Store) -> Result<(), SearchError> {
        let chunk_count = store.chunk_count()?;
        let fts_count = store.fts_count()?;
        if chunk_count > 0 && fts_count == 0 {
            info!(chunks = chunk_count, "populating FTS index from existing data");
            store.rebuild_fts()?;
        }
        Ok(())
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Inject the graph-context provider (dependency-inverted). Once set, code chunks
    /// indexed afterwards are enriched with their outbound graph context before
    /// embedding. Idempotent; pass-through to the indexing paths.
    pub fn set_graph_context_provider(
        &mut self,
        provider: Arc<dyn crate::ports::GraphContextProvider>,
    ) {
        if let Ok(mut cache) = self.workspace_overlay_cache.lock() {
            cache.set_graph_context_provider(provider.clone());
        }
        self.graph_context_provider = Some(provider);
    }

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

            let provider = self.graph_context_provider.as_deref();
            let docs: Vec<crate::IndexedDocument> = chunks
                .iter()
                .map(|c| crate::document::indexed_document_for_chunk(&rel_path, c, provider))
                .collect();
            let texts: Vec<String> =
                docs.iter().map(crate::document::semantic_text_for_indexed_document).collect();
            let graph_contexts: Vec<Option<String>> =
                docs.iter().map(|d| d.graph_context.clone()).collect();

            total_chunks += chunks.len();
            tasks.push(FileTask {
                rel_path,
                hash: hash.as_bytes().to_vec(),
                chunks,
                texts,
                graph_contexts,
            });
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
                            graph_contexts: task.graph_contexts,
                            embeddings: match error {
                                None => Ok(embeddings),
                                Some(e) => Err(e),
                            },
                        });
                    }
                })
            })
            .collect();

        drop(task_rx);
        drop(result_tx);

        let producer = std::thread::spawn(move || {
            for task in tasks {
                if task_tx.send(task).is_err() {
                    break;
                }
            }
        });

        let mut indexed = 0usize;
        let mut errors = 0usize;
        while let Ok(result) = result_rx.recv() {
            match result.embeddings {
                Ok(embeddings) => {
                    self.store.reindex_file_with_context(
                        &result.rel_path,
                        &result.hash,
                        &result.chunks,
                        Some(&embeddings),
                        Some(&result.graph_contexts),
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

        self.index = Self::build_persisted_index(&self.store, self.dim, self.embedder.as_ref())?;

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

    /// Ingest one file's chunks produced by the fused graph pass: writes chunk text,
    /// FTS rows, and the per-chunk graph context with NO embedding (filled later by
    /// [`Self::embed_pending_chunks_standalone`]), and records the file hash so an
    /// unchanged file is skipped next run. Chunks and contexts originate in the graph
    /// build, so no
    /// parsing or graph round-trip happens here — this is purely the storage write.
    pub fn ingest_fused_file(
        &mut self,
        rel_path: &str,
        hash: &[u8],
        chunks: &[crate::Chunk],
        graph_contexts: &[Option<String>],
    ) -> Result<(), SearchError> {
        self.store.reindex_file_with_context(rel_path, hash, chunks, None, Some(graph_contexts))?;
        Ok(())
    }

    /// Run the fused embedding pass against a database without holding a live
    /// [`SearchEngine`]: opens its own connection (WAL — concurrent readers, single
    /// writer) so the engine's outer mutex stays free for lexical search during the
    /// long HTTP-bound embed. Returns the freshly built [`VectorIndex`] for the caller
    /// to swap into the live engine via [`Self::set_vector_index`].
    pub fn embed_pending_chunks_standalone(
        db_path: &Path,
        config: &SearchConfig,
        progress: Option<&Arc<IndexProgress>>,
    ) -> Result<VectorIndex, SearchError> {
        let store = Store::open(db_path)?;
        let dim = config.embedder.dim.unwrap_or(1024);
        let embedder = Embedder::new(config.embedder.clone());
        // `run_embedding_pass` persists the built index from this background thread (it owns the
        // standalone store), NOT after the caller's `set_vector_index` swap which holds the live
        // engine lock — so the ~1.5s save never blocks concurrent search and the swap is instant.
        Self::run_embedding_pass(
            &store,
            &embedder,
            dim,
            config.execution.batch_size(),
            config.execution.concurrency(),
            progress,
        )
    }

    /// Atomically swap the in-memory vector index of a live engine. Brief operation
    /// held under the engine's outer mutex (the same lock semantic queries take while
    /// reading `self.index`), so a concurrent reader sees either the old or the new
    /// index, never a torn one.
    pub fn set_vector_index(&mut self, index: VectorIndex) {
        self.index = index;
    }

    /// Core of the fused embedding phase, free of any borrow on a live engine so it can
    /// run against either `self.store` or a standalone connection. Reads the `code`
    /// chunks still missing an embedding, embeds their semantic text concurrently,
    /// updates each row, then builds and returns the vector index.
    fn run_embedding_pass(
        store: &Store,
        embedder: &Embedder,
        dim: usize,
        batch_size: usize,
        concurrency: usize,
        progress: Option<&Arc<IndexProgress>>,
    ) -> Result<VectorIndex, SearchError> {
        let pending = store.load_pending_embedding_documents("code")?;
        if pending.is_empty() {
            let (generation, data) = store.load_all_embeddings_with_generation(dim)?;
            let index = VectorIndex::build(dim, &data)?;
            Self::persist_built(store, dim, Some(embedder), &index, generation);
            return Ok(index);
        }

        let items: Vec<(i64, String)> = pending
            .into_iter()
            .map(|(id, doc)| (id, crate::document::semantic_text_for_indexed_document(&doc)))
            .collect();
        let total = items.len();

        let total_batches = total.div_ceil(batch_size);
        if let Some(p) = &progress {
            p.active.store(true, Ordering::Relaxed);
            p.total_files.store(0, Ordering::Relaxed);
            p.total_chunks.store(total, Ordering::Relaxed);
            p.total_batches.store(total_batches, Ordering::Relaxed);
            p.done_batches.store(0, Ordering::Relaxed);
            p.done_chunks.store(0, Ordering::Relaxed);
        }

        let concurrency = concurrency.min(total_batches.max(1));
        info!(chunks = total, batches = total_batches, concurrency, "embedding fused chunks");

        // Fan batches of (chunk_id, text) out to embedder workers; the main thread
        // applies each batch's vectors (SQLite is single-writer). Nothing larger than
        // one batch is held per worker, so peak RAM stays bounded by the batch size.
        let (task_tx, task_rx) = crossbeam_channel::bounded::<Vec<(i64, String)>>(concurrency * 2);
        #[allow(clippy::type_complexity)]
        let (result_tx, result_rx) = crossbeam_channel::bounded::<
            Result<Vec<(i64, Vec<f32>)>, SearchError>,
        >(concurrency * 2);

        let workers: Vec<std::thread::JoinHandle<()>> = (0..concurrency)
            .map(|_| {
                let rx = task_rx.clone();
                let tx = result_tx.clone();
                let emb = embedder.clone();
                let prog = progress.cloned();
                std::thread::spawn(move || {
                    while let Ok(batch) = rx.recv() {
                        let refs: Vec<&str> = batch.iter().map(|(_, t)| t.as_str()).collect();
                        let out = match emb.embed_batch(&refs) {
                            Ok(embs) => {
                                if let Some(p) = &prog {
                                    p.done_chunks.fetch_add(batch.len(), Ordering::Relaxed);
                                    p.done_batches.fetch_add(1, Ordering::Relaxed);
                                }
                                Ok(batch.iter().map(|(id, _)| *id).zip(embs).collect())
                            }
                            Err(e) => Err(e),
                        };
                        if tx.send(out).is_err() {
                            break;
                        }
                    }
                })
            })
            .collect();

        drop(task_rx);
        drop(result_tx);

        let producer = {
            let batches: Vec<Vec<(i64, String)>> =
                items.chunks(batch_size).map(<[(i64, String)]>::to_vec).collect();
            std::thread::spawn(move || {
                for batch in batches {
                    if task_tx.send(batch).is_err() {
                        break;
                    }
                }
            })
        };

        let mut embedded = 0usize;
        let mut errors = 0usize;
        while let Ok(result) = result_rx.recv() {
            match result {
                Ok(pairs) => {
                    for (id, emb) in pairs {
                        store.set_chunk_embedding(id, &emb)?;
                        embedded += 1;
                    }
                }
                Err(e) => {
                    warn!("embedding batch failed after retries, skipping: {e}");
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

        let (generation, data) = store.load_all_embeddings_with_generation(dim)?;
        let index = VectorIndex::build(dim, &data)?;
        Self::persist_built(store, dim, Some(embedder), &index, generation);

        info!(embedded, errors, total_vectors = index.len(), "fused embedding complete");
        Ok(index)
    }

    /// Index workspace files for *deferred* embedding: chunk each changed file, attach
    /// its graph context via the configured provider, and persist chunk + FTS rows with a
    /// NULL embedding. The vectors are filled later by
    /// [`Self::embed_pending_chunks_standalone`], which reads back the stored graph
    /// context. Unlike [`Self::index_directory_fts`] this preserves graph context, so the
    /// deferred embeddings are graph-enriched whenever a provider is set — matching what
    /// the synchronous [`Self::index_directory`] would have produced, without blocking on
    /// the embed. Returns the number of files (re)indexed.
    pub fn index_directory_deferred(&mut self, root: &Path) -> Result<usize, SearchError> {
        let bsl_files: Vec<std::path::PathBuf> = walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")))
            .map(|e| e.into_path())
            .collect();

        info!(total_files = bsl_files.len(), "scanning BSL files (deferred embedding)");

        let provider = self.graph_context_provider.as_deref();
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

            if let Some(stored_hash) = self.store.file_hash(&rel_path)? {
                if stored_hash == hash.as_bytes() {
                    continue;
                }
            }

            let chunks = Chunker::chunk(&content);
            if chunks.is_empty() {
                continue;
            }

            let graph_contexts: Vec<Option<String>> = chunks
                .iter()
                .map(|c| {
                    crate::document::indexed_document_for_chunk(&rel_path, c, provider)
                        .graph_context
                })
                .collect();

            self.store.reindex_file_with_context(
                &rel_path,
                hash.as_bytes(),
                &chunks,
                None,
                Some(&graph_contexts),
            )?;
            indexed += 1;
        }

        info!(indexed, total_chunks = self.store.chunk_count()?, "deferred indexing complete");
        Ok(indexed)
    }

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

            if let Some(stored_hash) = self.store.file_hash(&rel_path)? {
                if stored_hash == hash.as_bytes() {
                    continue;
                }
            }

            let chunks = Chunker::chunk(&content);
            if chunks.is_empty() {
                continue;
            }

            self.store.reindex_file(&rel_path, hash.as_bytes(), &chunks, None)?;
            indexed += 1;
        }

        info!(indexed, total_chunks = self.store.chunk_count()?, "FTS indexing complete");
        Ok(indexed)
    }

    pub fn index_documents(
        &mut self,
        collection: &str,
        virtual_path: &str,
        version_hash: &[u8],
        documents: &[Document],
        progress: Option<&Arc<IndexProgress>>,
    ) -> Result<usize, SearchError> {
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

            let producer = std::thread::spawn(move || {
                for (idx, batch) in indexed_batches {
                    if task_tx.send((idx, batch)).is_err() {
                        break;
                    }
                }
            });

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

            self.index =
                Self::build_persisted_index(&self.store, self.dim, self.embedder.as_ref())?;
        } else {
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

    pub fn has_semantic(&self) -> bool {
        self.embedder.is_some()
    }

    pub fn embedding_model(&self) -> Option<&str> {
        self.embedder.as_ref().map(Embedder::model)
    }

    pub fn embedding_dimension(&self) -> Option<usize> {
        self.embedder.as_ref().map(Embedder::dim)
    }

    pub fn embed_query(&self, query: &str) -> Result<Vec<f32>, SearchError> {
        let embedder = self.embedder.as_ref().ok_or_else(|| {
            SearchError::Index(
                "semantic search not available: configure EMBEDDING_URL to enable embeddings"
                    .to_owned(),
            )
        })?;
        embedder.embed(query)
    }

    pub fn workspace_root(&self) -> Option<&std::path::Path> {
        self.workspace_root.as_deref()
    }

    pub fn set_workspace_root(&mut self, workspace_root: impl Into<std::path::PathBuf>) {
        self.workspace_root = Some(workspace_root.into());
        if let Ok(mut cache) = self.workspace_overlay_cache.lock() {
            cache.clear();
        }
    }

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

    pub fn workspace_overlay_stats(&self) -> Result<Option<WorkspaceOverlayStats>, SearchError> {
        let Some(workspace_root) = &self.workspace_root else {
            return Ok(None);
        };
        let mut cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        if let Some(manifest_fingerprints) =
            self.store.load_baseline_manifest_fingerprints("code")?
        {
            cache.refresh_with_manifest(
                &manifest_fingerprints,
                workspace_root,
                None,
                self.batch_size,
                &self.store,
            )?;
        } else {
            cache.refresh(
                &self.store,
                workspace_root,
                None,
                self.batch_size,
                self.workspace_baseline_hash_mode,
            )?;
        }
        Ok(Some(cache.stats()))
    }

    /// In-engine overlay prime that may embed inline (holds the engine lock for its duration).
    /// Reserved for the no-baseline / local paths and tests; the PostgresRemoteOverlay warmup must
    /// NOT use this (it would serialize all search behind a multi-minute embed) and instead drives
    /// the lock-free [`Self::prime_workspace_overlay_standalone`] + [`Self::publish_workspace_overlay`].
    pub fn prime_workspace_overlay(&self) -> Result<(), SearchError> {
        if self.workspace_root.is_none() {
            return Ok(());
        }
        let _ = self.workspace_overlay_snapshot(RefreshMode::Embed)?;
        Ok(())
    }

    /// The embedder configuration of this engine, if semantic search is configured. The warmup
    /// thread clones this under a brief lock so it can build a standalone embedder for the
    /// lock-free embedding pass.
    pub fn embedder_config(&self) -> Option<EmbedderConfig> {
        self.embedder.as_ref().map(Embedder::config)
    }

    /// The path of this engine's SQLite database, for reopening a standalone connection off-lock.
    pub fn db_path(&self) -> &Path {
        self.store.db_path()
    }

    /// The injected graph-context provider, cloned for the standalone overlay prime so its
    /// embeddings are graph-enriched exactly like an in-engine refresh.
    pub fn graph_context_provider(&self) -> Option<Arc<dyn crate::ports::GraphContextProvider>> {
        self.graph_context_provider.clone()
    }

    /// A read-only clone of the overlay embedding cache, for the warmup's lock-free Phase B start.
    pub fn workspace_overlay_embedding_cache_snapshot(
        &self,
    ) -> Result<HashMap<String, Vec<f32>>, SearchError> {
        let cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        Ok(cache.embedding_cache_snapshot())
    }

    /// Phase A + B of the lock-free overlay warmup: plan the manifest-driven refresh against a
    /// freshly reopened standalone [`Store`] (Phase A, read-only), then embed the missing chunks
    /// via the remote embedder with no engine/inner lock held (Phase B). Returns the plan and the
    /// embeddings it produced for [`Self::publish_workspace_overlay`] (Phase C) to merge in.
    ///
    /// `Store` is `!Sync`, so this opens its own connection from `db_path` rather than borrowing
    /// the live engine's store. Newly embedded vectors are persisted to that standalone store at
    /// the end of Phase B so a crash mid-warmup does not throw away embedding work already paid
    /// for; Phase C persists the merged live cache once more.
    pub fn prime_workspace_overlay_standalone(
        db_path: &Path,
        embedder_config: EmbedderConfig,
        workspace_root: &Path,
        warm_embeddings: HashMap<String, Vec<f32>>,
        graph_provider: Option<Arc<dyn crate::ports::GraphContextProvider>>,
    ) -> Result<(RefreshPlan, HashMap<String, Vec<f32>>), SearchError> {
        let batch_size = EmbeddingExecutionPolicy::default().batch_size();
        let store = Store::open(db_path)?;
        let embedder = Embedder::new(embedder_config);

        // Seed the warm cache from the persisted overlay embedding cache so a restart reuses
        // vectors already paid for instead of re-embedding everything.
        let mut warm_embeddings = warm_embeddings;
        if warm_embeddings.is_empty() {
            match store.load_overlay_embedding_cache(embedder.model(), embedder.dim()) {
                Ok(cached) if !cached.is_empty() => {
                    info!(
                        model_id = embedder.model(),
                        dim = embedder.dim(),
                        cached_embeddings = cached.len(),
                        "loaded persisted overlay embedding cache for standalone prime"
                    );
                    warm_embeddings = cached;
                }
                _ => {}
            }
        }

        let manifest_fingerprints =
            store.load_baseline_manifest_fingerprints("code")?.unwrap_or_default();

        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest_fingerprints,
            workspace_root,
            &store,
            &warm_embeddings,
            graph_provider.as_deref(),
        )?;

        let mut new_embeddings = Self::embed_missing_overlay_chunks(
            &store,
            &embedder,
            plan.missing_embeddings(),
            batch_size,
        )?;

        // Include the warm-reused vectors for the plan's chunks in the published set so Phase C
        // builds complete vectors regardless of the live cache's state (it may be empty on a
        // fresh engine). `content_hash` is value stable, so this is a no-op merge for chunks the
        // live cache already holds.
        for content_hash in plan.planned_content_hashes() {
            if !new_embeddings.contains_key(content_hash) {
                if let Some(embedding) = warm_embeddings.get(content_hash) {
                    new_embeddings.insert(content_hash.clone(), embedding.clone());
                }
            }
        }

        Ok((plan, new_embeddings))
    }

    /// Phase B: embed the plan's missing `content_hash -> input` pairs in batches off any lock,
    /// persisting each batch's vectors to the standalone `store` as it lands so a mid-pass crash
    /// keeps the progress already paid for.
    fn embed_missing_overlay_chunks(
        store: &Store,
        embedder: &Embedder,
        missing: &HashMap<String, String>,
        batch_size: usize,
    ) -> Result<HashMap<String, Vec<f32>>, SearchError> {
        if missing.is_empty() {
            return Ok(HashMap::new());
        }

        let pairs: Vec<(&String, &String)> = missing.iter().collect();
        let mut new_embeddings = HashMap::with_capacity(missing.len());

        for batch in pairs.chunks(batch_size.max(1)) {
            let inputs: Vec<&str> = batch.iter().map(|(_, input)| input.as_str()).collect();
            let embeddings = embedder.embed_batch_interactive(&inputs)?;

            let mut batch_persist = HashMap::with_capacity(batch.len());
            for ((content_hash, _), embedding) in batch.iter().zip(embeddings) {
                batch_persist.insert((*content_hash).clone(), embedding.clone());
                new_embeddings.insert((*content_hash).clone(), embedding);
            }
            // Persist to the standalone store (NOT the live engine) so partial progress survives
            // a mid-pass failure. The shared live cache is touched only once, in Phase C.
            if let Err(error) =
                store.save_overlay_embedding_cache(embedder.model(), embedder.dim(), &batch_persist)
            {
                tracing::warn!("failed to persist overlay embedding batch: {error}");
            }
        }

        Ok(new_embeddings)
    }

    /// Phase C: merge the plan and Phase-B embeddings into the live overlay cache under a brief
    /// inner-cache lock, swapping the entry/hidden-path set atomically so a concurrent reader
    /// never sees a half-embedded file. Never holds the lock across an embed.
    pub fn publish_workspace_overlay(
        &self,
        plan: RefreshPlan,
        new_embeddings: HashMap<String, Vec<f32>>,
        dirty_before: &HashMap<String, u64>,
    ) -> Result<(), SearchError> {
        let mut cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        cache.publish_plan(plan, new_embeddings, dirty_before, self.embedder.as_ref(), &self.store)
    }

    /// Snapshot the overlay dirty-path set (path -> mark sequence). Taken under the cache lock
    /// before the warmup's lock-free embed pass so [`Self::publish_workspace_overlay`] clears only
    /// the flags that pass supersedes, never one the watcher re-marked while the embed was in flight.
    pub fn workspace_overlay_dirty_paths_snapshot(
        &self,
    ) -> Result<HashMap<String, u64>, SearchError> {
        let cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        Ok(cache.dirty_paths_snapshot())
    }

    pub fn workspace_overlay_lexical_hits(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<(Vec<SearchHit>, HashSet<String>), SearchError> {
        if self.workspace_root.is_none() {
            return Ok((Vec::new(), HashSet::new()));
        }
        let overlay = self.workspace_overlay_snapshot(RefreshMode::ReuseOnly)?;
        if overlay.is_empty() {
            return Ok((Vec::new(), HashSet::new()));
        }
        let hits = lexical_hits(&overlay, query, limit);
        Ok((hits, overlay.hidden_paths.clone()))
    }

    pub fn workspace_overlay_semantic_hits(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<(Vec<SearchHit>, HashSet<String>), SearchError> {
        if self.workspace_root.is_none() {
            return Ok((Vec::new(), HashSet::new()));
        }
        let Some(embedder) = &self.embedder else {
            return Ok((Vec::new(), HashSet::new()));
        };
        // ReuseOnly: the refresh attaches only already-cached overlay vectors and never embeds
        // inline on this hot, lock-held query path. Chunks lacking a cached vector contribute no
        // overlay semantic hit this turn (they remain lexical); the background warmup is the only
        // place that embeds overlay chunks. The embedder is still used below for the query vector.
        let overlay = self.workspace_overlay_snapshot(RefreshMode::ReuseOnly)?;
        if overlay.is_empty() {
            return Ok((Vec::new(), HashSet::new()));
        }
        let query_embedding = embedder.embed(query)?;
        let hits = semantic_hits(&overlay, &query_embedding, limit);
        Ok((hits, overlay.hidden_paths.clone()))
    }

    pub fn resolve_workspace_code_view(&self) -> Result<Option<ResolvedView>, SearchError> {
        self.resolve_workspace_code_view_with(
            BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "local-workspace-baseline"),
            LocalStoreBaselineAdapter::workspace_code(&self.store),
            LocalStoreBaselineAdapter::workspace_code(&self.store),
        )
    }

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

        let overlay = self.workspace_overlay_snapshot(RefreshMode::ReuseOnly)?;
        let mut overlay = overlay.overlay;
        overlay.baseline = baseline.clone();
        let service =
            BaselineOverlaySearchService::new(catalog, content_store, InMemoryResolvedViewResolver);

        service.resolve_view(baseline, overlay)
    }

    pub fn resolve_workspace_code_view_from_documents(
        &self,
        baseline: BaselineRef,
        baseline_documents: Vec<crate::IndexedDocument>,
    ) -> Result<Option<ResolvedView>, SearchError> {
        if self.workspace_root.is_none() {
            return Ok(None);
        }

        let overlay = self.workspace_overlay_snapshot(RefreshMode::ReuseOnly)?;
        let mut overlay = overlay.overlay;
        overlay.baseline = baseline.clone();

        InMemoryResolvedViewResolver.resolve(baseline, baseline_documents, overlay).map(Some)
    }

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

        // ReuseOnly: reuse cached overlay vectors only, never embed inline under the engine lock.
        let overlay = self.workspace_overlay_snapshot(RefreshMode::ReuseOnly)?;
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
                // FTS5 bm25 `rank` is negative and *smaller is better*. Map it to a [0,1) score
                // that *increases* with relevance so any later descending re-sort (the overlay
                // merge in `text_search_with_workspace_overlay`) keeps the strongest match first
                // rather than inverting it.
                let score = 1.0 - 1.0 / (1.0 - result.rank as f32);
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
        let overlay = self.workspace_overlay_snapshot(RefreshMode::ReuseOnly)?;
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

    /// Refresh and snapshot the workspace overlay.
    ///
    /// `mode` selects how chunks without a cached vector are treated: [`RefreshMode::ReuseOnly`]
    /// (every interactive query) reuses only cached vectors and never embeds inline under the
    /// engine lock; [`RefreshMode::Embed`] (the background warmup) may embed missing vectors.
    /// In [`RefreshMode::Embed`] the engine's own embedder is supplied; in [`RefreshMode::ReuseOnly`]
    /// no embedder is passed down, so the refresh stays off the network.
    fn workspace_overlay_snapshot(
        &self,
        mode: RefreshMode,
    ) -> Result<WorkspaceOverlayIndex, SearchError> {
        let workspace_root = self
            .workspace_root
            .as_ref()
            .ok_or_else(|| SearchError::Index("workspace root is not configured".to_owned()))?;
        let embedder = match mode {
            RefreshMode::Embed => self.embedder.as_ref(),
            RefreshMode::ReuseOnly => None,
        };
        let mut cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        if let Some(manifest_fingerprints) =
            self.store.load_baseline_manifest_fingerprints("code")?
        {
            cache.refresh_with_manifest(
                &manifest_fingerprints,
                workspace_root,
                embedder,
                self.batch_size,
                &self.store,
            )?;
        } else {
            cache.refresh(
                &self.store,
                workspace_root,
                embedder,
                self.batch_size,
                self.workspace_baseline_hash_mode,
            )?;
        }
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

        self.index = Self::build_persisted_index(&self.store, self.dim, self.embedder.as_ref())?;
        Ok(indexed)
    }

    pub fn chunk_count(&self) -> Result<usize, SearchError> {
        self.store.chunk_count()
    }

    pub fn file_count(&self) -> Result<usize, SearchError> {
        self.store.file_count()
    }

    pub fn vector_count(&self) -> usize {
        self.index.len()
    }

    pub fn embedding_count_by_collection(&self, collection: &str) -> Result<usize, SearchError> {
        self.store.embedding_count_by_collection(collection)
    }

    pub fn load_indexed_documents(
        &self,
        collection: Option<&str>,
    ) -> Result<Vec<crate::IndexedDocument>, SearchError> {
        self.store.load_indexed_documents(collection)
    }

    pub fn clear_file_hashes(&self, collection: &str) -> Result<usize, SearchError> {
        self.store.clear_file_hashes(collection)
    }

    pub fn clear_file_hashes_without_embeddings(
        &self,
        collection: &str,
    ) -> Result<usize, SearchError> {
        self.store.clear_file_hashes_without_embeddings(collection)
    }

    pub fn remove_file(&mut self, rel_path: &str) -> Result<(), SearchError> {
        self.store.remove_file(rel_path)?;
        self.index = Self::build_persisted_index(&self.store, self.dim, self.embedder.as_ref())?;
        Ok(())
    }
}

struct FileTask {
    rel_path: String,
    hash: Vec<u8>,
    chunks: Vec<code_chunk::Chunk>,
    texts: Vec<String>,
    /// Per-chunk graph context (parallel to `chunks`), persisted so a later
    /// reconstruction-from-storage re-embeds with the same enriched text.
    graph_contexts: Vec<Option<String>>,
}

struct FileResult {
    rel_path: String,
    hash: Vec<u8>,
    chunks: Vec<code_chunk::Chunk>,
    graph_contexts: Vec<Option<String>>,
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
    fn index_directory_deferred_preserves_graph_context_without_embedding() {
        struct StubProvider;
        impl crate::ports::GraphContextProvider for StubProvider {
            fn graph_context(
                &self,
                _rel_path: &str,
                symbol_name: &str,
                _kind: &str,
            ) -> Option<String> {
                Some(format!("calls: {symbol_name}_helper"))
            }
        }

        let dir = tempdir().unwrap();
        let workspace = dir.path();
        fs::write(workspace.join("CommonModule.bsl"), "Процедура Тест()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_graph_context_provider(std::sync::Arc::new(StubProvider));

        let indexed = engine.index_directory_deferred(workspace).unwrap();
        assert_eq!(indexed, 1);

        // Chunks are written with graph context but no vectors yet — the deferred
        // background pass embeds the stored, already-enriched text.
        assert_eq!(engine.vector_count(), 0);
        let pending = engine.store().load_pending_embedding_documents("code").unwrap();
        let method = pending
            .iter()
            .find(|(_, doc)| doc.symbol_name == "Тест")
            .expect("method chunk should be pending embedding");
        assert_eq!(method.1.graph_context.as_deref(), Some("calls: Тест_helper"));
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
                    graph_context: None,
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
                    graph_context: None,
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
    fn workspace_overlay_stats_use_persisted_manifest_without_hiding_unchanged_files() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("CommonModule.bsl");
        fs::write(&file, "Процедура БазоваяПроцедура()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        engine
            .store()
            .save_baseline_manifest(&crate::WorkspaceBaselineManifest {
                snapshot_id: "snap-1".to_owned(),
                snapshot_fingerprint: Some("fp-1".to_owned()),
                files: vec![crate::BaselineManifestFile {
                    collection: "code".to_owned(),
                    path: "CommonModule.bsl".to_owned(),
                    file_fingerprint: crate::workspace_overlay::fingerprint_content(
                        "Процедура БазоваяПроцедура()\nКонецПроцедуры",
                        "CommonModule.bsl",
                    ),
                    document_count: 1,
                    file_object_id: "obj-1".to_owned(),
                }],
            })
            .unwrap();

        let stats = engine.workspace_overlay_stats().unwrap().unwrap();
        assert_eq!(stats.overlay_files, 0);
        assert_eq!(stats.deleted_files, 0);
        assert_eq!(stats.hidden_paths, 0);
    }

    #[test]
    fn workspace_overlay_lexical_hits_use_persisted_manifest_for_modified_file() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("CommonModule.bsl");
        fs::write(&file, "Процедура ЛокальнаяПроцедура()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        engine
            .store()
            .save_baseline_manifest(&crate::WorkspaceBaselineManifest {
                snapshot_id: "snap-1".to_owned(),
                snapshot_fingerprint: Some("fp-1".to_owned()),
                files: vec![crate::BaselineManifestFile {
                    collection: "code".to_owned(),
                    path: "CommonModule.bsl".to_owned(),
                    file_fingerprint: "different-fingerprint".to_owned(),
                    document_count: 1,
                    file_object_id: "obj-1".to_owned(),
                }],
            })
            .unwrap();

        let (hits, hidden_paths) =
            engine.workspace_overlay_lexical_hits("ЛокальнаяПроцедура", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].symbol_name, "ЛокальнаяПроцедура");
        assert!(hidden_paths.contains("CommonModule.bsl"));
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

    #[test]
    fn interactive_overlay_semantic_does_not_embed_overlay_chunks_when_vectors_absent() {
        use crate::embedder::EmbedderConfig;
        use std::time::{Duration, Instant};

        // An overlay engine wired to an unreachable embedder. The interactive overlay refresh is
        // ReuseOnly: with no cached vectors it must NOT embed the changed file's chunks inline
        // (that would hit the dead embedder and stall the lock-held query). The overlay still
        // refreshes lexically, and the call returns promptly rather than blocking on an embed.
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("CommonModule.bsl");
        fs::write(&file, "Процедура ЛокальнаяПравка()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let config = crate::SearchConfig {
            embedder: EmbedderConfig {
                base_url: "http://127.0.0.1:1".to_owned(),
                model: "test-model".to_owned(),
                dim: Some(3),
                api_key: None,
                provider: None,
            },
            execution: crate::EmbeddingExecutionPolicy::default(),
        };
        let mut engine = SearchEngine::semantic_overlay_only(&db_path, config).unwrap();
        engine.set_workspace_root(workspace);
        engine
            .store()
            .save_baseline_manifest(&crate::WorkspaceBaselineManifest {
                snapshot_id: "snap-1".to_owned(),
                snapshot_fingerprint: Some("fp-1".to_owned()),
                files: vec![crate::BaselineManifestFile {
                    collection: "code".to_owned(),
                    path: "CommonModule.bsl".to_owned(),
                    file_fingerprint: "different-fingerprint".to_owned(),
                    document_count: 1,
                    file_object_id: "obj-1".to_owned(),
                }],
            })
            .unwrap();

        // Lexical overlay still sees the change without any embedding round-trip.
        let (lexical, _hidden) =
            engine.workspace_overlay_lexical_hits("ЛокальнаяПравка", 10).unwrap();
        assert_eq!(lexical.len(), 1);

        // The semantic overlay query embeds only the QUERY (fast connection-refused on a dead
        // endpoint), never the overlay chunks. Either way it returns quickly; it must not stall
        // trying to embed the uncached chunk. The result is allowed to be an error (query embed
        // failed), but it must come back fast.
        let started = Instant::now();
        let _ = engine.workspace_overlay_semantic_hits("ЛокальнаяПравка", 10);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "ReuseOnly query must not block on inline overlay embedding"
        );
    }
}
