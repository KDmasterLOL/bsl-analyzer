use crate::document::Document;
use crate::embedder::{Embedder, EmbedderConfig};
use crate::error::SearchError;
use crate::index::VectorIndex;
use crate::local_baseline::LocalStoreBaselineAdapter;
use crate::ports::{ModuleSnapshot, ModuleSnapshotSource, SnapshotCatalog, SnapshotContentStore};
use crate::publish::EmbeddingExecutionPolicy;
use crate::resolver::{InMemoryResolvedViewResolver, ResolvedView};
use crate::store::Store;
use crate::workspace_overlay::{
    lexical_hits, normalized_file_hash_for_indexed_documents, semantic_hits, BaselineHashMode,
    PublicationBaseline, PublishOutcome, RefreshMode, RefreshPlan, WorkspaceOverlayCache,
    WorkspaceOverlayIndex, WorkspaceOverlayStats,
};
use crate::workspace_roots::{FileKey, WorkspaceRoots, CONFIGURATION_ROOT_ID};
use crate::{
    semantic_key_for_indexed_document, semantic_text_for_indexed_document,
    BaselineOverlaySearchService, BaselineRef, CorpusId,
};
use code_chunk::Chunker;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
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
    /// The source root the file belongs to; see [`crate::DocumentPath::root_id`].
    pub root_id: String,
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
            root_id: hit.root_id.clone(),
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
            root_id: self.root_id.clone(),
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
            root_id: self.root_id.clone(),
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
            root_id: hit.root_id,
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

// Test seam: force the vector eviction of a removal to count as failed, so a test can assert
// that a removal whose vectors stayed in the live index is not reported as a success — and
// that the store row it is selected by outlives the failure. The live index rejects nothing
// on its own: removing an id it does not hold is a no-op there.
#[cfg(test)]
thread_local! {
    // Thread-local on purpose: tests run in parallel, and a process-wide flag would fail
    // removals in whichever unrelated test happened to be running at the time.
    static FORCE_VECTOR_REMOVE_ERROR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub struct SearchEngine {
    store: Store,
    embedder: Option<Embedder>,
    index: VectorIndex,
    dim: usize,
    batch_size: usize,
    concurrency: usize,
    workspace_roots: Option<WorkspaceRoots>,
    workspace_overlay_cache: Mutex<WorkspaceOverlayCache>,
    workspace_baseline_hash_mode: BaselineHashMode,
    /// Whether this engine serves an EXTERNAL (remote) baseline through the persisted
    /// manifest. The manifest is a persistent warm-cache that deliberately survives a mode
    /// switch, so its mere presence proves nothing: every manifest-path dispatch and every
    /// baseline-evidence decision must consult this flag, not the table.
    serves_external_baseline: bool,
    /// Optional graph-context provider (dependency-inverted via
    /// [`crate::ports::GraphContextProvider`]). When set, code chunks are enriched
    /// with their outbound graph context before embedding. `None` keeps embeddings
    /// graph-free.
    graph_context_provider: Option<Arc<dyn crate::ports::GraphContextProvider>>,
    /// Optional resident-host snapshot source (dependency-inverted via
    /// [`crate::ports::ModuleSnapshotSource`]). When set, the overlay's incremental reindex
    /// chunks the resident's shared parse instead of parsing the file itself. `None` keeps the
    /// pure disk read+parse path.
    module_snapshot_source: Option<Arc<dyn ModuleSnapshotSource>>,
}

/// The overlay retry driver's condition signals, read without side effects by
/// [`SearchEngine::workspace_overlay_retry_signals`]. Any nonzero/true field means the
/// overlay owes another Embed pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayRetrySignals {
    pub initialized: bool,
    pub needs_full_rescan: bool,
    pub pending_dirty_paths: usize,
    pub unembedded_entries: usize,
    pub unread_keys: usize,
}

impl OverlayRetrySignals {
    /// Whether any signal demands a pass: the first pass has not happened, removals were
    /// withheld or a persist failed, marks await re-embedding, entries lack vectors, or
    /// proven-present files stayed unread.
    pub fn demands_a_pass(&self) -> bool {
        !self.initialized
            || self.needs_full_rescan
            || self.pending_dirty_paths > 0
            || self.unembedded_entries > 0
            || self.unread_keys > 0
    }
}

/// Outcome of [`SearchEngine::refresh_dirty_contexts`]: how many context-dirty paths
/// were processed (marks cleared) and how many chunks had their context re-rendered
/// (and embedding cleared) as a result.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ContextRefreshStats {
    pub paths_cleared: usize,
    pub chunks_updated: usize,
    /// Chunks whose live embedding was cleared (set NULL) as part of the re-render, so
    /// the caller knows a background re-embed pass is warranted. Equal to
    /// `chunks_updated` today (every re-render clears the embedding), tracked separately
    /// so the "kick the embed pass" decision reads an explicit signal, not a coincidence.
    pub cleared_embeddings: usize,
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
            workspace_roots: None,
            workspace_overlay_cache: Mutex::new(WorkspaceOverlayCache::default()),
            workspace_baseline_hash_mode: BaselineHashMode::RawFileBytes,
            serves_external_baseline: false,
            graph_context_provider: None,
            module_snapshot_source: None,
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
            workspace_roots: None,
            workspace_overlay_cache: Mutex::new(WorkspaceOverlayCache::default()),
            workspace_baseline_hash_mode: BaselineHashMode::RawFileBytes,
            serves_external_baseline: false,
            graph_context_provider: None,
            module_snapshot_source: None,
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
            workspace_roots: None,
            workspace_overlay_cache: Mutex::new(WorkspaceOverlayCache::default()),
            workspace_baseline_hash_mode: BaselineHashMode::RawFileBytes,
            serves_external_baseline: false,
            graph_context_provider: None,
            module_snapshot_source: None,
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

    /// Inject the resident-host snapshot source (dependency-inverted). Once set, the overlay's
    /// incremental reindex prefers the resident's shared parse. Does not touch cached entries:
    /// the source changes only HOW a file is read+parsed, never the chunk output.
    pub fn set_module_snapshot_source(&mut self, source: Arc<dyn ModuleSnapshotSource>) {
        self.module_snapshot_source = Some(source);
    }

    /// The injected resident-host snapshot source, cloned so the orchestrator can prefetch
    /// snapshots OFF the engine lock (the resident read must never overlap the engine lock).
    pub fn module_snapshot_source(&self) -> Option<Arc<dyn ModuleSnapshotSource>> {
        self.module_snapshot_source.clone()
    }

    /// The overlay paths currently marked dirty, so the caller can prefetch resident snapshots
    /// for them off-lock and feed them back through [`Self::reindex_dirty_from_snapshots`].
    pub fn workspace_overlay_dirty_paths(&self) -> Result<Vec<FileKey>, SearchError> {
        let cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        Ok(cache.dirty_paths_list())
    }

    /// How many overlay entries have been built from a resident-provided shared parse since the
    /// engine's workspace root was set. Observability for the resident-fed reindex (proves the
    /// shared-parse path fired, e.g. in a regression test).
    pub fn workspace_overlay_resident_fed_count(&self) -> Result<usize, SearchError> {
        let cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        Ok(cache.resident_fed_count())
    }

    /// Reindex the dirty overlay paths using prefetched resident snapshots (shared parse) where
    /// available, disk-reading the rest. The `snapshots` map is prefetched by the caller with no
    /// engine lock held, so this method — which does take the engine's overlay-cache lock — never
    /// touches the resident host, keeping the resident and engine locks strictly disjoint.
    pub fn reindex_dirty_from_snapshots(
        &self,
        snapshots: &HashMap<FileKey, ModuleSnapshot>,
    ) -> Result<(), SearchError> {
        let Some(roots) = &self.workspace_roots else {
            return Ok(());
        };
        let mut cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        cache.reindex_dirty_from_snapshots(
            roots,
            &self.store,
            self.serves_external_baseline,
            self.batch_size,
            self.workspace_baseline_hash_mode,
            snapshots,
        )
    }

    pub fn index_directory(
        &mut self,
        root: &Path,
        progress: Option<&Arc<IndexProgress>>,
    ) -> Result<usize, SearchError> {
        let bsl_files: Vec<std::path::PathBuf> = walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| bsl_conventions::has_extension(e.path(), bsl_conventions::BSL_EXTENSION))
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

            if let Some(stored_hash) = self.store.file_hash(CONFIGURATION_ROOT_ID, &rel_path)? {
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
                .map(|c| {
                    crate::document::indexed_document_for_chunk(
                        &FileKey::configuration(&rel_path),
                        c,
                        provider,
                    )
                })
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
                        CONFIGURATION_ROOT_ID,
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
        key: &FileKey,
        hash: &[u8],
        chunks: &[crate::Chunk],
        graph_contexts: &[Option<String>],
    ) -> Result<(), SearchError> {
        self.store.reindex_file_with_context(
            &key.root_id,
            &key.path,
            hash,
            chunks,
            None,
            Some(graph_contexts),
        )?;
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
        should_continue: Option<&(dyn Fn() -> bool + Sync)>,
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
            should_continue,
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
        should_continue: Option<&(dyn Fn() -> bool + Sync)>,
    ) -> Result<VectorIndex, SearchError> {
        let pending = store.load_pending_embedding_documents("code")?;
        if pending.is_empty() {
            let (generation, data) = store.load_all_embeddings_with_generation(dim)?;
            let index = VectorIndex::build(dim, &data)?;
            // The sidecar is a shared artifact like any other; a caller that may no longer
            // write leaves it to whoever may.
            if should_continue.is_none_or(|keep_going| keep_going()) {
                Self::persist_built(store, dim, Some(embedder), &index, generation);
            }
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
        let mut stopped = false;
        while let Ok(result) = result_rx.recv() {
            // Asked between batches, never inside one: a pass over a large configuration runs
            // for hours, and the caller's right to write may not outlive it.
            if should_continue.is_some_and(|keep_going| !keep_going()) {
                stopped = true;
                break;
            }
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
        // Closed before the joins in every path: a worker parked on a send into a channel
        // nobody reads any more would never finish, and the stop path leaves exactly that.
        drop(result_rx);

        let _ = producer.join();
        for w in workers {
            let _ = w.join();
        }
        if let Some(p) = &progress {
            p.active.store(false, Ordering::Relaxed);
        }

        let (generation, data) = store.load_all_embeddings_with_generation(dim)?;
        let index = VectorIndex::build(dim, &data)?;
        // Asked once more before the sidecar: a takeover landing after the last batch would
        // otherwise still leave this pass's index description behind for the new owner.
        let stopped = stopped || should_continue.is_some_and(|keep_going| !keep_going());
        if stopped {
            // The vectors already written stay — they were written while the caller still had
            // the right to. What is skipped is the persisted sidecar, the one artifact a
            // stopped pass would leave behind for whoever writes this database next; the index
            // itself is still returned, so this process keeps answering semantic queries from
            // what it has.
            warn!(embedded, errors, "embedding pass stopped early; sidecar not persisted");
            return Ok(index);
        }
        Self::persist_built(store, dim, Some(embedder), &index, generation);

        info!(embedded, errors, total_vectors = index.len(), "fused embedding complete");
        Ok(index)
    }

    /// The files a boot ingest must write, each under the key the store knows it by.
    ///
    /// With a root table configured, the universe is EVERY registered root, walked once through
    /// the shared source-set walk, and the key is decided by the same attribution every other
    /// path uses — the longest matching prefix, not the root the walk entered through. Keying by
    /// the entered root would give a file under a configuration that some extension contains a
    /// second row under that extension's id. De-duplication by key is what keeps one file one
    /// row when roots nest and the walk reaches it twice.
    ///
    /// Without a table the caller is not a workspace daemon but a one-shot indexer (the baseline
    /// publisher, a reference corpus), and the old contract stands: walk the given directory and
    /// key everything as the configuration.
    fn boot_ingest_files(&self, root: &Path) -> Vec<(FileKey, std::path::PathBuf)> {
        let walked: Option<Vec<std::path::PathBuf>> = self
            .workspace_roots
            .as_ref()
            .map(|roots| roots.entries().map(|(_, declared)| declared.to_path_buf()).collect());
        self.boot_ingest_files_over(root, walked.as_deref())
    }

    /// The same projection over a chosen subset of the roots to WALK. Attribution still consults
    /// the whole table: roots may nest, so a file found while walking one root can belong to
    /// another, and keying it by the root the walk entered through would give it a second row.
    fn boot_ingest_files_over(
        &self,
        root: &Path,
        walk: Option<&[std::path::PathBuf]>,
    ) -> Vec<(FileKey, std::path::PathBuf)> {
        let Some(roots) = self.workspace_roots.as_ref() else {
            return walkdir::WalkDir::new(root)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    bsl_conventions::has_extension(e.path(), bsl_conventions::BSL_EXTENSION)
                })
                .map(|e| {
                    let path = e.into_path();
                    let rel =
                        path.strip_prefix(root).unwrap_or(&path).to_string_lossy().to_string();
                    (FileKey::configuration(rel), path)
                })
                .collect();
        };
        let owned: Vec<std::path::PathBuf>;
        let declared: &[std::path::PathBuf] = match walk {
            Some(walk) => walk,
            None => {
                owned = roots.entries().map(|(_, declared)| declared.to_path_buf()).collect();
                &owned
            }
        };
        let set = project_model::SourceSet::scan(declared);
        let mut seen = HashSet::new();
        let mut files = Vec::new();
        for file in &set.files {
            if file.role != project_model::FileRole::Source {
                continue;
            }
            let Some(key) = roots.root_of(&file.walked, &file.canonical) else {
                continue;
            };
            if !seen.insert(key.clone()) {
                continue;
            }
            files.push((key, file.walked.clone()));
        }
        files
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
        let bsl_files = self.boot_ingest_files(root);

        info!(total_files = bsl_files.len(), "scanning BSL files (deferred embedding)");

        let provider = self.graph_context_provider.as_deref();
        let mut indexed = 0;
        for (key, file_path) in &bsl_files {
            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(e) => {
                    warn!(?file_path, "failed to read file: {e}");
                    continue;
                }
            };

            let hash = blake3::hash(content.as_bytes());
            let rel_path = key.path.clone();

            let had_prior = match self.store.file_hash(&key.root_id, &rel_path)? {
                Some(stored_hash) => {
                    if stored_hash == hash.as_bytes() {
                        continue;
                    }
                    true
                }
                None => false,
            };

            let chunks = Chunker::chunk(&content);
            if chunks.is_empty() {
                // The content changed (its hash mismatched above) but now yields no chunks — the
                // file was gutted to comments/blank while the daemon was down. Any prior chunks are
                // now stale; leaving them makes a Clean boot false-clean (the vanished symbol is
                // served forever), and the deletion reconcile does NOT cover this — the file still
                // EXISTS on disk, so it is never "gone". Remove the stored rows. A file that was
                // never indexed has nothing to remove and must not gain a spurious zero-chunk row,
                // so only prior-stored files are touched.
                if had_prior {
                    self.store.remove_file(&key.root_id, &rel_path, "code")?;
                    indexed += 1;
                }
                continue;
            }

            let graph_contexts: Vec<Option<String>> = chunks
                .iter()
                .map(|c| {
                    crate::document::indexed_document_for_chunk(key, c, provider).graph_context
                })
                .collect();

            self.store.reindex_file_with_context(
                &key.root_id,
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
        let bsl_files = self.boot_ingest_files(root);
        self.ingest_files_fts(&bsl_files)
    }

    /// Index only the registered roots that have no rows at all yet.
    ///
    /// A warm store skips re-indexing to keep a restart cheap, but "warm" is a per-ROOT fact: a
    /// root declared while the daemon was down has nothing stored, and skipping it would leave it
    /// out of the index until someone edited a file in it. Roots that already have rows are not
    /// walked, read or hashed here, so the restart stays as cheap as it was.
    pub fn index_unindexed_roots_fts(&mut self) -> Result<usize, SearchError> {
        let indexed_roots: HashSet<String> = self
            .store
            .all_files_in_collection("code")?
            .into_iter()
            .map(|(key, _hash)| key.root_id)
            .collect();
        let Some(roots) = self.workspace_roots.as_ref() else { return Ok(0) };
        // Only the unindexed roots are WALKED: a warm root must not be traversed, canonicalised
        // or stat-ed at all, or the per-root skip would cost exactly what it exists to avoid.
        let cold: Vec<std::path::PathBuf> = roots
            .entries()
            .filter(|(id, _)| !indexed_roots.contains(*id))
            .map(|(_, declared)| declared.to_path_buf())
            .collect();
        if cold.is_empty() {
            return Ok(0);
        }
        let files: Vec<(FileKey, std::path::PathBuf)> = self
            .boot_ingest_files_over(Path::new(""), Some(&cold))
            .into_iter()
            .filter(|(key, _)| !indexed_roots.contains(&key.root_id))
            .collect();
        if files.is_empty() {
            return Ok(0);
        }
        self.ingest_files_fts(&files)
    }

    fn ingest_files_fts(
        &mut self,
        bsl_files: &[(FileKey, std::path::PathBuf)],
    ) -> Result<usize, SearchError> {
        info!(total_files = bsl_files.len(), "scanning BSL files (FTS-only)");

        let mut indexed = 0;
        for (key, file_path) in bsl_files {
            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(e) => {
                    warn!(?file_path, "failed to read file: {e}");
                    continue;
                }
            };

            let hash = blake3::hash(content.as_bytes());
            let rel_path = key.path.clone();

            let had_prior = match self.store.file_hash(&key.root_id, &rel_path)? {
                Some(stored_hash) => {
                    if stored_hash == hash.as_bytes() {
                        continue;
                    }
                    true
                }
                None => false,
            };

            let chunks = Chunker::chunk(&content);
            if chunks.is_empty() {
                // The content changed (its hash mismatched above) but now yields no chunks — the
                // file was gutted to comments/blank while the daemon was down. Any prior chunks are
                // now stale; leaving them makes a Clean boot false-clean (the vanished symbol is
                // served forever), and the deletion reconcile does NOT cover this — the file still
                // EXISTS on disk, so it is never "gone". Remove the stored rows. A file that was
                // never indexed has nothing to remove and must not gain a spurious zero-chunk row,
                // so only prior-stored files are touched.
                if had_prior {
                    self.store.remove_file(&key.root_id, &rel_path, "code")?;
                    indexed += 1;
                }
                continue;
            }

            self.store.reindex_file(&key.root_id, &rel_path, hash.as_bytes(), &chunks, None)?;
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
        if let Some(stored_hash) = self.store.file_hash(CONFIGURATION_ROOT_ID, virtual_path)? {
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

    /// The CONFIGURATION root: the directory every stored path with the reserved
    /// configuration id is spelled against, and the base a caller resolves a hit's relative path
    /// with. Deliberately not the workspace directory of the root table — a configuration
    /// commonly sits in a subdirectory of the project, and the table's workspace exists to make
    /// root identifiers relative, not to resolve paths.
    pub fn configuration_root(&self) -> Option<&std::path::Path> {
        self.workspace_roots.as_ref().and_then(WorkspaceRoots::configuration)
    }

    /// The engine's root table, for a caller that must scan or resolve keys off
    /// the engine lock (the standalone overlay prime).
    pub fn workspace_roots(&self) -> Option<&WorkspaceRoots> {
        self.workspace_roots.as_ref()
    }

    /// Point the engine at a workspace whose only source root is the workspace
    /// directory itself. Every file found under it is the configuration's, which
    /// is what a caller with no project model to consult can honestly say.
    pub fn set_workspace_root(&mut self, workspace_root: impl Into<std::path::PathBuf>) {
        let workspace_root = workspace_root.into();
        let (roots, _) = WorkspaceRoots::build(&workspace_root, &workspace_root, &[]);
        self.set_workspace_roots(roots);
    }

    pub fn set_workspace_roots(&mut self, roots: WorkspaceRoots) {
        self.workspace_roots = Some(roots);
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
        let Some(key) = self.workspace_file_key(path.as_ref()) else {
            return Ok(false);
        };
        let mut cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        cache.enable_watcher_mode();
        cache.mark_dirty_path(key);
        Ok(true)
    }

    /// The store key of a workspace `.bsl` file, or `None` when it is not a
    /// `.bsl` or lies outside every registered root. Shared by the workspace
    /// point-update entry points.
    ///
    /// A relative path is taken as workspace-relative — that is the only reading
    /// available, and the callers that pass one have already stripped the
    /// workspace prefix themselves. The canonical spelling is what attribution
    /// ranks roots by, which is why [`WorkspaceRoots::root_of`] takes two.
    fn workspace_file_key(&self, path: &Path) -> Option<FileKey> {
        let roots = self.workspace_roots.as_ref()?;
        if !bsl_conventions::has_extension(path, bsl_conventions::BSL_EXTENSION) {
            return None;
        }
        // A relative path is spelled against the CONFIGURATION root: that is how every stored
        // path with the reserved id is spelled, and it is the prefix callers strip before
        // handing one over. The table's workspace exists to make root identifiers relative and
        // is a directory higher whenever the configuration sits in a subdirectory.
        let walked = if path.is_absolute() {
            path.to_path_buf()
        } else {
            roots.configuration().unwrap_or_else(|| roots.workspace()).join(path)
        };
        let canonical = crate::workspace_roots::canonical_spelling(&walked);
        // A `.bsl`-spelled link may resolve to a non-source target — by role, or by not being
        // a regular file at all (a directory spelled `.bsl`). A key under such a target's root
        // would be one that is FORBIDDEN to exist (the walk drops such files), so canonical
        // attribution is meaningless there; the walked spelling is the only key the file could
        // ever have been indexed under — the key a removal must reach. A GONE target still
        // attributes canonically: it was a file if it was anything, and the tombstone path
        // needs the last known spelling.
        let target_is_source = project_model::file_role(&canonical)
            == project_model::FileRole::Source
            && match std::fs::metadata(&canonical) {
                Ok(metadata) => metadata.is_file(),
                Err(_) => true,
            };
        if target_is_source {
            roots.root_of(&walked, &canonical)
        } else {
            roots.root_of_declared(&walked)
        }
    }

    /// Mark one workspace `.bsl` file's stored graph context stale, so a later
    /// reindex/embed pass re-renders it. Cheap metadata write (a side-table upsert, no
    /// chunk mutation, so the vector sidecar is not invalidated). Returns whether the
    /// path was a workspace `.bsl`.
    pub fn mark_workspace_path_context_dirty(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<bool, SearchError> {
        let Some(key) = self.workspace_file_key(path.as_ref()) else {
            return Ok(false);
        };
        self.store.mark_context_dirty("code", &key.root_id, &key.path)?;
        Ok(true)
    }

    /// Mark every indexed workspace file context-dirty (a configuration-root descriptor
    /// changed: conservatively assume any module's context could shift). Returns the
    /// number of files marked.
    pub fn mark_workspace_context_dirty(&self) -> Result<usize, SearchError> {
        Ok(self.store.mark_collection_context_dirty("code")?.0)
    }

    /// [`Self::mark_workspace_context_dirty`] for a caller that consumes the batch in the same
    /// breath: the rows are stamped at `stamp_seq` — the bound of the build the re-render will
    /// run against — so that same bound clears exactly this batch, and a file carrying a
    /// fresher drift keeps its own mark instead of being swept up by it. Returns the number of
    /// rows written.
    pub fn mark_workspace_context_dirty_at(&self, stamp_seq: i64) -> Result<usize, SearchError> {
        self.store.mark_collection_context_dirty_at("code", stamp_seq)
    }

    /// The set of paths currently marked context-dirty in `collection`.
    pub fn context_dirty_paths(&self, collection: &str) -> Result<HashSet<FileKey>, SearchError> {
        self.store.context_dirty_paths(collection)
    }

    /// A handle to the highest context-dirty mark seq this store has observed. The graph
    /// layer reads it at build start to bound which marks that build's publish may consume;
    /// the stamps themselves are allocated by the database. See [`Store::mark_seq_handle`].
    pub fn mark_seq_handle(&self) -> Arc<AtomicI64> {
        self.store.mark_seq_handle()
    }

    /// Remove one workspace `.bsl` file after a local deletion, closing every path a
    /// stale hit could survive:
    /// - drops its `files` row and cascaded `chunks`/FTS rows from the store;
    /// - writes an overlay tombstone so a baseline (Postgres-mode) hit for the same path
    ///   cannot resurrect it;
    /// - marks the path dirty in the in-memory overlay cache so a cached entry stops
    ///   serving stale hits on the next refresh (`refresh_dirty_paths` hides a gone file);
    /// - evicts exactly the deleted chunks' vectors from the live index incrementally.
    ///
    /// The store deletion bumps `embedding_generation` (via the delete triggers), so the
    /// persisted vector sidecar already invalidates and a cold start rebuilds — this path
    /// deliberately does NOT reload every embedding or re-persist the sidecar. Returns
    /// whether the path was a workspace `.bsl`.
    pub fn remove_workspace_path(&mut self, path: impl AsRef<Path>) -> Result<bool, SearchError> {
        let Some(key) = self.workspace_file_key(path.as_ref()) else {
            return Ok(false);
        };
        self.remove_workspace_key(&key)?;
        Ok(true)
    }

    /// [`Self::remove_workspace_path`] for a caller that already holds the store
    /// key. A key read back from the store must NOT be re-attributed: its path is
    /// relative to its own root, and re-deriving it from the workspace would hand
    /// an extension's file to the configuration and leave the real row in place.
    pub fn remove_workspace_key(&mut self, key: &FileKey) -> Result<(), SearchError> {
        // A manifest this engine cannot read is not evidence of "no baseline copy": guessing
        // `false` would skip the hiding that stops the copy from being served.
        let has_baseline =
            self.dispatched_manifest_fingerprints()?.is_some_and(|m| m.contains_key(key));
        self.remove_workspace_key_with(key, has_baseline)
    }

    /// [`Self::remove_workspace_key`] with the baseline evidence already resolved, so a batch
    /// caller loads the manifest once instead of once per key.
    ///
    /// The store row goes LAST, after every step that can fail. That row is what a reconcile
    /// selects the key by, so dropping it first would turn any later failure into a silent
    /// loss: the retry mark reaches the overlay alone, and the chunk ids the vector eviction
    /// needs come from the row itself. Leaving it in place instead makes the very next
    /// reconcile pick the key up exactly where this pass left it.
    fn remove_workspace_key_with(
        &mut self,
        key: &FileKey,
        has_baseline: bool,
    ) -> Result<(), SearchError> {
        // The retry obligation comes FIRST: every operation below can fail, and returning
        // early without the mark would leave no signal for the point path — while the rows
        // still tell the old story. A poisoned lock is a dead process, not a key state, but
        // it does mean the obligation was not recorded, so it is not a success either.
        {
            let mut cache = self.workspace_overlay_cache.lock().map_err(|e| {
                SearchError::Index(format!("workspace overlay cache lock error: {e}"))
            })?;
            cache.enable_watcher_mode();
            cache.mark_dirty_path(key.clone());
        }
        // Collected before the row goes, because the row is where they live.
        let chunk_ids = self.store.chunk_ids_for_file("code", &key.root_id, &key.path)?;
        for id in chunk_ids {
            self.index.remove(id)?;
        }
        #[cfg(test)]
        if FORCE_VECTOR_REMOVE_ERROR.with(std::cell::Cell::get) {
            return Err(SearchError::Index("forced vector removal failure".to_owned()));
        }
        self.store.insert_overlay_tombstone(&key.root_id, &key.path, "code")?;
        // The dead file's fingerprint row must not survive it: the dirty mark dies with the
        // process, and a namesake recreated at the same (len, mtime, canonical) would inherit
        // the "verified" claim across a restart.
        self.store.delete_overlay_fingerprint_entries(std::slice::from_ref(key))?;
        {
            // The deletion is proven, so drop the overlay entry at once — the point refresh
            // would read a root that vanished WITH the file as "unreachable, retry" and leave
            // a ghost entry. The mark still re-checks the disk: if the event lied, the next
            // point pass republishes the live file.
            let mut cache = self.workspace_overlay_cache.lock().map_err(|e| {
                SearchError::Index(format!("workspace overlay cache lock error: {e}"))
            })?;
            cache.remove_known_deleted(key, has_baseline);
        }
        self.store.remove_file(&key.root_id, &key.path, "code")?;
        Ok(())
    }

    /// One reading of every carrier that can still know about a workspace file, taken once
    /// per operation: each carrier costs a load, and asking per key would turn a reconcile
    /// into a query per stored file.
    ///
    /// A carrier that cannot be read is left EMPTY rather than guessed at, and the two cases
    /// mean different things. The manifest is empty whenever this engine does not serve an
    /// external baseline — its rows deliberately survive a mode switch, so a local engine
    /// must not read them as evidence. The overlay is empty only if its lock is poisoned,
    /// which is a dead process rather than a key state.
    fn carrier_keys(&self) -> Result<crate::key_carriers::CarrierKeys, SearchError> {
        let mut carriers = crate::key_carriers::CarrierKeys {
            store_rows: self
                .store
                .all_files_in_collection("code")?
                .into_iter()
                .map(|(key, _hash)| key)
                .collect(),
            ..Default::default()
        };
        if let Ok(cache) = self.workspace_overlay_cache.lock() {
            let (entries, unread) = cache.known_keys();
            carriers.overlay_entries = entries;
            carriers.unread = unread;
        }
        let snapshot_id = self
            .store
            .load_baseline_manifest()
            .ok()
            .flatten()
            .map(|r| r.snapshot_id)
            .unwrap_or_default();
        carriers.fingerprints = self
            .store
            .load_overlay_fingerprint_cache(&snapshot_id)?
            .unwrap_or_default()
            .into_keys()
            .collect();
        carriers.manifest =
            self.dispatched_manifest_fingerprints()?.unwrap_or_default().into_keys().collect();
        Ok(carriers)
    }

    /// Reconcile the workspace `code` collection against the set of `.bsl` files actually
    /// present on disk (`present_abs`, absolute paths from a fresh walk): every key no longer
    /// present is removed via [`Self::remove_workspace_key_with`] (tombstone + overlay dirty +
    /// incremental vector eviction). This closes the gap where a file deleted during a lost
    /// watch window (change-hub overflow or a structural subtree rescan) keeps its rows and
    /// vectors forever, because the ordinary drift path only marks files that still exist.
    ///
    /// Candidates come from EVERY carrier (see [`Self::carrier_keys`]), not from the store
    /// rows alone: those rows are a snapshot of the boot walk, so a file indexed afterwards
    /// has no row at all, and against a remote baseline there are no local rows whatsoever.
    /// Bounded O(known keys) and driven only on the rare rescan branch; the caller walks the
    /// tree OUTSIDE the engine lock and passes the result here. Returns the number of removed
    /// keys.
    pub fn reconcile_workspace_files(
        &mut self,
        present_abs: &HashSet<std::path::PathBuf>,
    ) -> Result<usize, SearchError> {
        if self.workspace_roots.is_none() {
            return Ok(0);
        }
        // The present files under the same keying the `code` collection uses, so
        // a file of one root never answers for the same relative path in another.
        let present: HashSet<FileKey> =
            present_abs.iter().filter_map(|p| self.workspace_file_key(p)).collect();
        let carriers = self.carrier_keys()?;
        // A manifest-only key survives its own removal — the row belongs to someone else's
        // corpus and only its hiding is ours to write — so without this the next reconcile
        // would select it again, and every pass would report a removal that changes nothing.
        // Read once, and only for that case: hiding elsewhere proves absence from disk, not
        // a settled key (a clean full pass hides a baseline key while its row lives on).
        let hidden = match self.workspace_overlay_cache.lock() {
            Ok(cache) => cache.hidden_keys(),
            Err(error) => {
                tracing::warn!("failed to read overlay hidings for a reconcile: {error}");
                HashSet::new()
            }
        };
        // Sorted so a batch removes in a stable order regardless of hash iteration.
        let mut candidates: Vec<FileKey> = carriers.all_keys().into_iter().collect();
        candidates.sort();
        let mut removed = 0;
        let mut failed = 0;
        let mut first_error = None;
        for key in candidates {
            if present.contains(&key) {
                continue;
            }
            if carriers.manifest_is_sole_carrier(&key) && hidden.contains(&key) {
                continue;
            }
            let has_baseline = carriers.manifest.contains(&key);
            // A key that cannot be removed does not cost the rest of the batch its pass: each
            // key's carriers are independent, and aborting here would strand every key after
            // the first fault until some later rescan happens to run.
            match self.remove_workspace_key_with(&key, has_baseline) {
                Ok(()) => removed += 1,
                Err(error) => {
                    tracing::warn!(
                        root = %key.root_id,
                        path = %key.path,
                        "failed to reconcile a deleted file out of the index: {error}"
                    );
                    failed += 1;
                    first_error.get_or_insert(error);
                }
            }
        }
        if let Some(error) = first_error {
            return Err(SearchError::Index(format!(
                "reconcile removed {removed} keys and failed on {failed}; first failure: {error}"
            )));
        }
        Ok(removed)
    }

    /// Re-render the stored `graph_context` of every chunk whose owning file was marked
    /// context-dirty (a metadata `.xml` it owns changed), using the freshly published
    /// graph. Only chunks whose context actually changed are rewritten, and only those
    /// have their embedding cleared (NULL) so the existing NULL-embedding embed machinery
    /// re-embeds them; an unchanged context clears the mark and touches nothing. The mark
    /// is cleared for a successfully processed path (an orphan mark for a file no longer in
    /// the store clears too), but a path whose render FAILED keeps its mark so the next
    /// publish retries it. Callers pass the provider built from the just-published graph;
    /// with no graph there is no provider and nothing is called, so marks simply persist.
    ///
    /// `seq_bound` is the mark sequence captured when the publishing build STARTED (see
    /// [`Store::mark_seq_handle`]): only marks at or below it are read and cleared. A drift
    /// that landed after the build started carries a higher `seq` and is left untouched —
    /// its mark is not cleared against a graph that predates it, and a re-mark of an
    /// in-flight path survives the bounded clear. Pass [`i64::MAX`] to consume every mark
    /// (an unbounded caller, e.g. a graph with no wired mark-seq source).
    pub fn refresh_dirty_contexts(
        &self,
        provider: &dyn crate::ports::GraphContextProvider,
        seq_bound: i64,
    ) -> Result<ContextRefreshStats, SearchError> {
        let mut stats = ContextRefreshStats::default();
        for key in self.store.context_dirty_paths_bounded("code", seq_bound)? {
            // A render error for ANY method of this path keeps the mark: the failure is
            // transient (the graph DB could not be read), so the next publish must retry
            // the whole path rather than clearing it against a half-failed render. A
            // legitimate `Ok(None)` (a method with no graph presence, or a file entirely
            // gone from the graph) is not an error and clears normally.
            let mut render_failed = false;
            for (id, symbol_name, kind, stored) in
                self.store.chunks_with_context_for_file("code", &key.root_id, &key.path)?
            {
                match provider.try_graph_context(&key.path, &symbol_name, &kind) {
                    Ok(rendered) => {
                        if rendered.as_deref() != stored.as_deref() {
                            self.store.set_chunk_graph_context(id, rendered.as_deref())?;
                            self.store.clear_chunk_embedding(id)?;
                            stats.chunks_updated += 1;
                            stats.cleared_embeddings += 1;
                        }
                    }
                    Err(e) => {
                        render_failed = true;
                        tracing::warn!(
                            root = %key.root_id,
                            path = %key.path,
                            method = %symbol_name,
                            "graph context render failed; keeping dirty mark for retry: {e}"
                        );
                    }
                }
            }
            if render_failed {
                continue;
            }
            self.store.clear_context_dirty_bounded("code", &key.root_id, &key.path, seq_bound)?;
            stats.paths_cleared += 1;
        }
        Ok(stats)
    }

    /// Declare whether this engine serves an external (remote) baseline. `false`
    /// additionally clears the persisted overlay fingerprint rows: a row claims "verified
    /// against the manifest", and the raw local mode can neither re-verify nor honour that
    /// claim — a file changed at the same stat during the local period would be suppressed by
    /// the inherited row after a switch back. Rows live only under the mode that wrote them.
    pub fn set_serves_external_baseline(&mut self, serves: bool) -> Result<(), SearchError> {
        self.serves_external_baseline = serves;
        if !serves {
            self.store.clear_overlay_fingerprint_cache()?;
        }
        Ok(())
    }

    /// The manifest fingerprints IF this engine serves an external baseline, `None` otherwise.
    /// Every manifest-vs-raw dispatch goes through here: the persisted manifest is a
    /// warm-cache that survives a mode switch, and dispatching on its presence would pin a
    /// local engine to another mode's baseline.
    fn dispatched_manifest_fingerprints(
        &self,
    ) -> Result<Option<HashMap<FileKey, String>>, SearchError> {
        if !self.serves_external_baseline {
            return Ok(None);
        }
        self.store.load_baseline_manifest_fingerprints("code")
    }

    /// How many overlay keys are proven present but unread — the durable retry signal that
    /// outlives the bounded point budget (see `WorkspaceOverlayCache::unread_keys_count`).
    pub fn workspace_overlay_unread_count(&self) -> Result<usize, SearchError> {
        let cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        Ok(cache.unread_keys_count())
    }

    /// The retry driver's condition signals, read STRICTLY without side effects: unlike
    /// [`Self::workspace_overlay_stats`], no refresh runs — a condition check that drained
    /// marks or touched the store would itself violate the ownership discipline it serves.
    pub fn workspace_overlay_retry_signals(&self) -> Result<OverlayRetrySignals, SearchError> {
        let cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        Ok(OverlayRetrySignals {
            initialized: cache.is_initialized(),
            needs_full_rescan: cache.needs_full_rescan(),
            pending_dirty_paths: cache.dirty_paths_snapshot().len(),
            unembedded_entries: cache.unembedded_entry_count(),
            unread_keys: cache.unread_keys_count(),
        })
    }

    pub fn workspace_overlay_stats(&self) -> Result<Option<WorkspaceOverlayStats>, SearchError> {
        let Some(roots) = &self.workspace_roots else {
            return Ok(None);
        };
        let mut cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        // `search status` is a read-only display; it must never trigger the cold full-tree scan,
        // so it uses the same non-cold-scan path as interactive queries.
        if let Some(manifest_fingerprints) = self.dispatched_manifest_fingerprints()? {
            cache.refresh_with_manifest(
                &manifest_fingerprints,
                roots,
                None,
                self.batch_size,
                &self.store,
                false,
            )?;
        } else {
            cache.refresh(
                &self.store,
                roots,
                None,
                self.batch_size,
                self.workspace_baseline_hash_mode,
                false,
            )?;
        }
        Ok(Some(cache.stats()))
    }

    /// Whether the overlay's last full publication ran over an incomplete scan and withheld its
    /// removals, so only a future clean full scan can catch up. Read-only: takes the cache lock,
    /// refreshes nothing.
    pub fn workspace_overlay_needs_full_rescan(&self) -> Result<bool, SearchError> {
        let cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        Ok(cache.needs_full_rescan())
    }

    /// In-engine overlay prime that may embed inline (holds the engine lock for its duration).
    /// Reserved for the no-baseline / local paths and tests; the PostgresRemoteOverlay warmup must
    /// NOT use this (it would serialize all search behind a multi-minute embed) and instead drives
    /// the lock-free [`Self::prime_workspace_overlay_standalone`] + [`Self::publish_workspace_overlay`].
    pub fn prime_workspace_overlay(&self) -> Result<(), SearchError> {
        if self.workspace_roots.is_none() {
            return Ok(());
        }
        let _ = self.workspace_overlay_snapshot(RefreshMode::Embed)?;
        Ok(())
    }

    /// Mark the workspace overlay initialized with zero entries, WITHOUT a disk scan. The caller
    /// must have proven the SQLite store was just reconciled with disk at boot (a fused parse
    /// ingest, or an `index_directory_deferred`/`index_directory_fts` walk+hash re-ingest), so the
    /// overlay baseline already equals the working tree and a prime would find no diffs. Zero cost,
    /// zero RAM — and, unlike a prime, robust to how the boot hashed files, because it asserts the
    /// reconciled invariant directly rather than re-deriving it. Flips the same `initialized` flag a
    /// prime would, so the resident-fed incremental reindex (inert until initialized) goes live.
    pub fn initialize_workspace_overlay_clean(&self) -> Result<(), SearchError> {
        if self.workspace_roots.is_none() {
            return Ok(());
        }
        let mut cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        cache.mark_initialized_clean();
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
        roots: &WorkspaceRoots,
        warm_embeddings: HashMap<String, Vec<f32>>,
        graph_provider: Option<Arc<dyn crate::ports::GraphContextProvider>>,
        should_continue: &dyn Fn() -> bool,
        distrusted: &HashSet<FileKey>,
    ) -> Result<(RefreshPlan, HashMap<String, Vec<f32>>), SearchError> {
        let batch_size = EmbeddingExecutionPolicy::default().batch_size();
        // `open_existing`, not `open`: this standalone pass runs while another daemon may own
        // the workspace, and the migrating constructor could wipe and recreate the owner's
        // tables on a schema mismatch. A pass has no business migrating anything.
        let store = Store::open_existing(db_path)?;
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
            roots,
            &store,
            &warm_embeddings,
            graph_provider.as_deref(),
            distrusted,
        )?;

        let mut new_embeddings = Self::embed_missing_overlay_chunks(
            &store,
            &embedder,
            plan.missing_embeddings(),
            batch_size,
            should_continue,
        )?;

        // Include the warm-reused vectors for the plan's chunks in the published set so Phase C
        // builds complete vectors regardless of the live cache's state (it may be empty on a
        // fresh engine). The embedding key is value stable, so this is a no-op merge for chunks
        // the live cache already holds.
        for embedding_key in plan.planned_embedding_keys() {
            if let std::collections::hash_map::Entry::Vacant(slot) =
                new_embeddings.entry(embedding_key)
            {
                if let Some(embedding) = warm_embeddings.get(slot.key()) {
                    slot.insert(embedding.clone());
                }
            }
        }

        Ok((plan, new_embeddings))
    }

    /// Phase B: embed the plan's missing `embedding_key -> input` pairs in batches off any lock,
    /// persisting each batch's vectors to the standalone `store` as it lands so a mid-pass crash
    /// keeps the progress already paid for.
    fn embed_missing_overlay_chunks(
        store: &Store,
        embedder: &Embedder,
        missing: &HashMap<String, String>,
        batch_size: usize,
        should_continue: &dyn Fn() -> bool,
    ) -> Result<HashMap<String, Vec<f32>>, SearchError> {
        if missing.is_empty() {
            return Ok(HashMap::new());
        }

        let pairs: Vec<(&String, &String)> = missing.iter().collect();
        let mut new_embeddings = HashMap::with_capacity(missing.len());

        for batch in pairs.chunks(batch_size.max(1)) {
            // Checked between batches, like the collection embed pass: each batch persists
            // vectors to the shared store, and a caller that lost the workspace lease must
            // stop writing over the new owner's rows.
            if !should_continue() {
                return Err(SearchError::Embedder(
                    "overlay embed pass stopped: workspace ownership lost".to_owned(),
                ));
            }
            let inputs: Vec<&str> = batch.iter().map(|(_, input)| input.as_str()).collect();
            let embeddings = embedder.embed_batch_interactive(&inputs)?;

            let mut batch_persist = HashMap::with_capacity(batch.len());
            for ((embedding_key, _), embedding) in batch.iter().zip(embeddings) {
                batch_persist.insert((*embedding_key).clone(), embedding.clone());
                new_embeddings.insert((*embedding_key).clone(), embedding);
            }
            // Re-checked AFTER the embed round-trip too: ownership (or the driver itself) may
            // have gone away while the request was in flight, and the save below writes the
            // shared table. The residual sub-batch window is the accepted vector-row trade —
            // a re-embed replaces a vector, unlike a fingerprint row it cannot lie durably.
            if !should_continue() {
                return Err(SearchError::Embedder(
                    "overlay embed pass stopped: workspace ownership lost".to_owned(),
                ));
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
    /// Returns how many marked keys the plan's gate skipped unread — see
    /// [`WorkspaceOverlayCache::publish_plan`].
    pub fn publish_workspace_overlay(
        &self,
        plan: RefreshPlan,
        new_embeddings: HashMap<String, Vec<f32>>,
        baseline: &PublicationBaseline,
    ) -> Result<PublishOutcome, SearchError> {
        let mut cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        cache.publish_plan(plan, new_embeddings, baseline, self.embedder.as_ref(), &self.store)
    }

    /// The atomic pre-plan snapshot (live marks + freshness fence) a planned publication is
    /// judged against; captured under the cache lock before the lock-free Phase A/B.
    pub fn workspace_overlay_publication_baseline(
        &self,
    ) -> Result<PublicationBaseline, SearchError> {
        let cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        Ok(cache.publication_baseline())
    }

    /// Snapshot the overlay dirty-path set (path -> mark sequence). Taken under the cache lock
    /// before the warmup's lock-free embed pass so [`Self::publish_workspace_overlay`] clears only
    /// the flags that pass supersedes, never one the watcher re-marked while the embed was in flight.
    pub fn workspace_overlay_dirty_paths_snapshot(
        &self,
    ) -> Result<HashMap<FileKey, u64>, SearchError> {
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
    ) -> Result<(Vec<SearchHit>, HashSet<FileKey>), SearchError> {
        if self.workspace_roots.is_none() {
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
    ) -> Result<(Vec<SearchHit>, HashSet<FileKey>), SearchError> {
        if self.workspace_roots.is_none() {
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

    /// Overlay semantic hits from a caller-supplied query vector (embedded off the engine lock),
    /// so the direct/Postgres path embeds once instead of re-embedding here.
    pub fn workspace_overlay_semantic_hits_with_embedding(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<(Vec<SearchHit>, HashSet<FileKey>), SearchError> {
        if self.workspace_roots.is_none() {
            return Ok((Vec::new(), HashSet::new()));
        }
        if self.embedder.is_none() {
            return Ok((Vec::new(), HashSet::new()));
        }
        let overlay = self.workspace_overlay_snapshot(RefreshMode::ReuseOnly)?;
        if overlay.is_empty() {
            return Ok((Vec::new(), HashSet::new()));
        }
        let hits = semantic_hits(&overlay, query_embedding, limit);
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
        if self.workspace_roots.is_none() {
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
        if self.workspace_roots.is_none() {
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

    /// Clone the configured embedder (rebuilds its HTTP agents from config), so the request path
    /// can embed the query *without* holding the engine lock. `None` when semantic is unconfigured.
    pub fn embedder_clone(&self) -> Option<Embedder> {
        self.embedder.clone()
    }

    /// Run a code search from a query vector embedded by the caller (off the engine lock), instead
    /// of embedding inline. Mirrors [`SearchEngine::search`] minus the embed step.
    pub fn search_with_embedding(
        &self,
        query_embedding: &[f32],
        limit: usize,
        collection: Option<&str>,
    ) -> Result<Vec<SearchHit>, SearchError> {
        if collection == Some("code") {
            if let Some(overlay_hits) =
                self.search_with_workspace_overlay_embedding(query_embedding, limit)?
            {
                return Ok(overlay_hits);
            }
        }

        self.search_persisted_with_embedding(query_embedding, limit, collection)
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
        self.search_persisted_with_embedding(&query_embedding, limit, collection)
    }

    /// The persisted-search body after the query has already been embedded, so callers that
    /// embed once (the overlay merge, the lock-free request path) need not embed again.
    fn search_persisted_with_embedding(
        &self,
        query_embedding: &[f32],
        limit: usize,
        collection: Option<&str>,
    ) -> Result<Vec<SearchHit>, SearchError> {
        let fetch_limit = if collection.is_some() { limit * 3 } else { limit };
        let results = self.index.search(query_embedding, fetch_limit)?;

        let ids: Vec<i64> = results.iter().map(|result| result.chunk_id).collect();
        let infos = self.store.chunks_by_ids(&ids)?;

        let mut hits = Vec::with_capacity(limit);
        for result in results {
            if hits.len() >= limit {
                break;
            }
            if let Some(info) = infos.get(&result.chunk_id).cloned() {
                if let Some(coll) = collection {
                    if info.collection != coll {
                        continue;
                    }
                }
                hits.push(SearchHit {
                    collection: info.collection,
                    root_id: info.root_id,
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
        if self.workspace_roots.is_none() {
            return Ok(None);
        }
        let Some(embedder) = &self.embedder else {
            return Ok(None);
        };
        // ReuseOnly: reuse cached overlay vectors only, never embed inline under the engine lock.
        // Snapshot before embedding so an empty overlay returns `None` without paying for a query
        // embed the persisted fallback would only repeat.
        let overlay = self.workspace_overlay_snapshot(RefreshMode::ReuseOnly)?;
        if overlay.is_empty() {
            return Ok(None);
        }
        let query_embedding = embedder.embed(query)?;
        let mut combined =
            self.search_persisted_with_embedding(&query_embedding, limit * 3, Some("code"))?;
        combined.retain(|hit| {
            !overlay.hidden_paths.contains(&FileKey::new(&hit.root_id, &hit.file_path))
        });
        combined.extend(semantic_hits(&overlay, &query_embedding, limit));
        combined.sort_by(|lhs, rhs| rhs.score.total_cmp(&lhs.score));
        combined.truncate(limit);
        Ok(Some(combined))
    }

    /// The overlay-merged code search after the query has already been embedded, so the request
    /// path can embed once off the engine lock and the persisted fetch never re-embeds.
    fn search_with_workspace_overlay_embedding(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Option<Vec<SearchHit>>, SearchError> {
        if self.workspace_roots.is_none() {
            return Ok(None);
        }

        // ReuseOnly: reuse cached overlay vectors only, never embed inline under the engine lock.
        let overlay = self.workspace_overlay_snapshot(RefreshMode::ReuseOnly)?;
        if overlay.is_empty() {
            return Ok(None);
        }

        let mut combined =
            self.search_persisted_with_embedding(query_embedding, limit * 3, Some("code"))?;
        combined.retain(|hit| {
            !overlay.hidden_paths.contains(&FileKey::new(&hit.root_id, &hit.file_path))
        });
        combined.extend(semantic_hits(&overlay, query_embedding, limit));
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

        let ids: Vec<i64> = results.iter().map(|result| result.chunk_id).collect();
        let infos = self.store.chunks_by_ids(&ids)?;

        let mut hits = Vec::with_capacity(results.len());
        for result in results {
            if let Some(info) = infos.get(&result.chunk_id).cloned() {
                // FTS5 bm25 `rank` is negative and *smaller is better*. Map it to a [0,1) score
                // that *increases* with relevance so any later descending re-sort (the overlay
                // merge in `text_search_with_workspace_overlay`) keeps the strongest match first
                // rather than inverting it.
                let score = 1.0 - 1.0 / (1.0 - result.rank as f32);
                hits.push(SearchHit {
                    collection: info.collection,
                    root_id: info.root_id,
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
        if self.workspace_roots.is_none() {
            return Ok(None);
        }

        let overlay = self.workspace_overlay_snapshot(RefreshMode::ReuseOnly)?;
        if overlay.is_empty() {
            return Ok(None);
        }

        let mut combined = self.text_search_persisted(query, limit * 3, Some("code"))?;
        combined.retain(|hit| {
            !overlay.hidden_paths.contains(&FileKey::new(&hit.root_id, &hit.file_path))
        });
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
        let roots = self
            .workspace_roots
            .as_ref()
            .ok_or_else(|| SearchError::Index("workspace root is not configured".to_owned()))?;
        let embedder = match mode {
            RefreshMode::Embed => self.embedder.as_ref(),
            RefreshMode::ReuseOnly => None,
        };
        // Only the background warmup (Embed) may pay for a cold full-tree scan under the lock.
        // Interactive query paths (ReuseOnly) must stay O(cached) — see `WorkspaceOverlayCache::refresh`.
        let allow_cold_scan = matches!(mode, RefreshMode::Embed);
        let mut cache = self
            .workspace_overlay_cache
            .lock()
            .map_err(|e| SearchError::Index(format!("workspace overlay cache lock error: {e}")))?;
        if let Some(manifest_fingerprints) = self.dispatched_manifest_fingerprints()? {
            cache.refresh_with_manifest(
                &manifest_fingerprints,
                roots,
                embedder,
                self.batch_size,
                &self.store,
                allow_cold_scan,
            )?;
        } else {
            cache.refresh(
                &self.store,
                roots,
                embedder,
                self.batch_size,
                self.workspace_baseline_hash_mode,
                allow_cold_scan,
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

        let mut grouped = BTreeMap::<FileKey, Vec<crate::IndexedDocument>>::new();
        for document in documents {
            grouped
                .entry(FileKey::new(&document.root_id, &document.path))
                .or_default()
                .push(document.clone());
        }

        let desired: HashSet<&FileKey> = grouped.keys().collect();
        for (existing, _) in self.store.all_files_in_collection(collection)? {
            if !desired.contains(&existing) {
                self.store.remove_file(&existing.root_id, &existing.path, collection)?;
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
        for (key, mut file_documents) in grouped {
            file_documents.sort_by(|lhs, rhs| {
                (lhs.line_start, lhs.line_end, lhs.symbol_name.as_str()).cmp(&(
                    rhs.line_start,
                    rhs.line_end,
                    rhs.symbol_name.as_str(),
                ))
            });

            let file_hash = normalized_file_hash_for_indexed_documents(&file_documents);
            if self.store.file_hash(&key.root_id, &key.path)?.as_deref()
                == Some(file_hash.as_slice())
            {
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
                &key.root_id,
                &key.path,
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

    pub fn remove_file(&mut self, rel_path: &str, collection: &str) -> Result<(), SearchError> {
        self.store.remove_file(CONFIGURATION_ROOT_ID, rel_path, collection)?;
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
    use super::{SearchEngine, FORCE_VECTOR_REMOVE_ERROR};
    use crate::key_carriers::KeyCarrier;
    use crate::ports::{SnapshotCatalog, SnapshotContentStore};
    use crate::workspace_overlay::RefreshMode;
    use crate::workspace_roots::{FileKey, CONFIGURATION_ROOT_ID};
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

        // The warmup (Embed) builds the overlay; interactive queries (ReuseOnly) never cold-scan.
        engine.prime_workspace_overlay().unwrap();

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

        // The warmup (Embed) builds the overlay; interactive queries (ReuseOnly) never cold-scan.
        engine.prime_workspace_overlay().unwrap();

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

        // The warmup (Embed) builds the overlay; `search status` (ReuseOnly) never cold-scans.
        engine.prime_workspace_overlay().unwrap();

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

        // The warmup (Embed) builds the overlay; the resolved view reads it via ReuseOnly.
        engine.prime_workspace_overlay().unwrap();

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

        // The warmup (Embed) builds the overlay; the resolved view reads it via ReuseOnly.
        engine.prime_workspace_overlay().unwrap();

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
                    root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
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
                    root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
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
        engine.set_serves_external_baseline(true).unwrap();
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
        engine.set_serves_external_baseline(true).unwrap();
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

        // The warmup (Embed) builds the overlay; interactive queries (ReuseOnly) never cold-scan.
        engine.prime_workspace_overlay().unwrap();

        let (hits, hidden_paths) =
            engine.workspace_overlay_lexical_hits("ЛокальнаяПроцедура", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].symbol_name, "ЛокальнаяПроцедура");
        assert!(hidden_paths.contains(&FileKey::configuration("CommonModule.bsl")));
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
        // The warmup (Embed) initializes the overlay; afterwards the watcher's dirty-path marks are
        // applied incrementally on the next ReuseOnly query without any cold full-tree scan.
        engine.prime_workspace_overlay().unwrap();

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
    fn search_with_embedding_uses_precomputed_vector_without_network() {
        use crate::embedder::EmbedderConfig;
        use crate::{Chunk, ChunkKind, Store};

        // Populate a file-backed store with two chunks carrying distinct stored vectors, so the
        // engine builds a real vector index from them. The embedder points at an unreachable URL:
        // the embedding-free search paths must never call it, so the query resolves offline.
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("bsl-search.db");

        let chunk = |name: &str| Chunk {
            kind: ChunkKind::Procedure,
            name: name.to_owned(),
            is_export: true,
            annotations: vec![],
            line_start: 0,
            line_end: 1,
            text: format!("Процедура {name}()\nКонецПроцедуры"),
        };
        let vec_a = vec![1.0f32, 0.0, 0.0];
        let vec_b = vec![0.0f32, 1.0, 0.0];
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file(
                    CONFIGURATION_ROOT_ID,
                    "a.bsl",
                    b"ha",
                    &[chunk("Альфа")],
                    Some(std::slice::from_ref(&vec_a)),
                )
                .unwrap();
            store
                .reindex_file(
                    CONFIGURATION_ROOT_ID,
                    "b.bsl",
                    b"hb",
                    &[chunk("Бета")],
                    Some(std::slice::from_ref(&vec_b)),
                )
                .unwrap();
        }

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
        let engine = SearchEngine::new(&db_path, config).unwrap();

        // Querying with chunk A's own vector ranks A first; with B's vector, B first. This
        // exercises `search_with_embedding` -> `search_persisted_with_embedding` (the batched
        // `chunks_by_ids` lookup) end to end with no embed round-trip.
        let hits_a = engine.search_with_embedding(&vec_a, 5, None).unwrap();
        assert_eq!(hits_a.first().map(|h| h.symbol_name.as_str()), Some("Альфа"));

        let hits_b = engine.search_with_embedding(&vec_b, 5, None).unwrap();
        assert_eq!(hits_b.first().map(|h| h.symbol_name.as_str()), Some("Бета"));
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
        engine.set_serves_external_baseline(true).unwrap();
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

        // Populate the overlay lexically the way the lock-free warmup does — plan the refresh and
        // publish it with NO embeddings (the embed step failed against the dead endpoint). This is
        // the exact failed-semantic-warmup state the fix targets: the overlay carries lexical docs
        // but no vectors, and interactive queries must answer from it without ever cold-scanning or
        // embedding inline.
        let manifest =
            engine.store().load_baseline_manifest_fingerprints("code").unwrap().unwrap_or_default();
        let plan =
            crate::workspace_overlay::WorkspaceOverlayCache::plan_full_refresh_from_manifest(
                &manifest,
                &crate::WorkspaceRoots::build(workspace, workspace, &[]).0,
                engine.store(),
                &std::collections::HashMap::new(),
                None,
                &std::collections::HashSet::new(),
            )
            .unwrap();
        engine
            .publish_workspace_overlay(
                plan,
                std::collections::HashMap::new(),
                &engine.workspace_overlay_publication_baseline().unwrap(),
            )
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

    /// The consumer for the context-dirty marks: `refresh_dirty_contexts` re-renders each
    /// dirty file's chunks against the provider, rewrites only those whose context
    /// changed (clearing their embedding so the embed machinery re-embeds), leaves
    /// unchanged ones alone, and clears every processed mark. Without this consumer the
    /// marks are write-only and `.xml` edits re-render nothing.
    #[test]
    fn refresh_dirty_contexts_rerenders_changed_context_and_clears_marks() {
        use crate::{Chunk, ChunkKind, Store};

        struct Stub;
        impl crate::ports::GraphContextProvider for Stub {
            fn graph_context(&self, _rel: &str, symbol_name: &str, _kind: &str) -> Option<String> {
                match symbol_name {
                    "Изменённая" => Some("новый контекст".to_owned()),
                    "Стабильная" => Some("тот же контекст".to_owned()),
                    _ => None,
                }
            }
        }

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("bsl-search.db");
        let chunk = |name: &str| Chunk {
            kind: ChunkKind::Procedure,
            name: name.to_owned(),
            is_export: true,
            annotations: vec![],
            line_start: 0,
            line_end: 1,
            text: format!("Процедура {name}()\nКонецПроцедуры"),
        };
        let vec = vec![1.0f32, 0.0, 0.0];
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file_with_context(
                    CONFIGURATION_ROOT_ID,
                    "Owned.bsl",
                    b"h1",
                    &[chunk("Изменённая")],
                    Some(std::slice::from_ref(&vec)),
                    Some(&[Some("старый контекст".to_owned())]),
                )
                .unwrap();
            store
                .reindex_file_with_context(
                    CONFIGURATION_ROOT_ID,
                    "Stable.bsl",
                    b"h2",
                    &[chunk("Стабильная")],
                    Some(std::slice::from_ref(&vec)),
                    Some(&[Some("тот же контекст".to_owned())]),
                )
                .unwrap();
        }

        let engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.store().mark_context_dirty("code", CONFIGURATION_ROOT_ID, "Owned.bsl").unwrap();
        engine.store().mark_context_dirty("code", CONFIGURATION_ROOT_ID, "Stable.bsl").unwrap();
        let gen_before = engine.store().embedding_generation().unwrap();

        let stats = engine.refresh_dirty_contexts(&Stub, i64::MAX).unwrap();
        assert_eq!(stats.paths_cleared, 2, "both marked paths are processed");
        assert_eq!(stats.chunks_updated, 1, "only the file whose context changed is rewritten");
        assert_eq!(stats.cleared_embeddings, 1, "the one rewritten chunk had its embedding NULLed");

        // Every mark is cleared.
        assert!(engine.context_dirty_paths("code").unwrap().is_empty());

        // The changed context is rewritten; the stable one is untouched.
        let docs = engine.store().load_indexed_documents(Some("code")).unwrap();
        let changed = docs.iter().find(|d| d.symbol_name == "Изменённая").unwrap();
        assert_eq!(changed.graph_context.as_deref(), Some("новый контекст"));
        let stable = docs.iter().find(|d| d.symbol_name == "Стабильная").unwrap();
        assert_eq!(stable.graph_context.as_deref(), Some("тот же контекст"));

        // Only the changed chunk had its embedding cleared (→ pending re-embed), which
        // bumped the vector generation; the stable chunk kept its vector.
        assert!(engine.store().embedding_generation().unwrap() > gen_before);
        let pending = engine.store().load_pending_embedding_documents("code").unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].1.symbol_name, "Изменённая");
    }

    /// A render FAILURE (transient — the graph DB could not be read) must NOT clear the
    /// path's dirty mark: the next graph publish has to retry it. A legitimate `Ok(None)`
    /// still clears. Without keeping the mark, a one-off graph-read error would silently
    /// drop the `.xml` edit's re-render forever.
    #[test]
    fn refresh_dirty_contexts_keeps_the_mark_when_render_fails() {
        use crate::{Chunk, ChunkKind, Store};

        struct Failing;
        impl crate::ports::GraphContextProvider for Failing {
            fn graph_context(&self, _rel: &str, _sym: &str, _kind: &str) -> Option<String> {
                None
            }
            fn try_graph_context(
                &self,
                _rel: &str,
                _sym: &str,
                _kind: &str,
            ) -> Result<Option<String>, crate::GraphContextError> {
                Err(crate::GraphContextError("graph db unreadable".to_owned()))
            }
        }

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("bsl-search.db");
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file_with_context(
                    CONFIGURATION_ROOT_ID,
                    "Owned.bsl",
                    b"h1",
                    &[Chunk {
                        kind: ChunkKind::Procedure,
                        name: "П".to_owned(),
                        is_export: true,
                        annotations: vec![],
                        line_start: 0,
                        line_end: 1,
                        text: "Процедура П()\nКонецПроцедуры".to_owned(),
                    }],
                    None,
                    Some(&[Some("старый контекст".to_owned())]),
                )
                .unwrap();
        }

        let engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.store().mark_context_dirty("code", CONFIGURATION_ROOT_ID, "Owned.bsl").unwrap();

        let stats = engine.refresh_dirty_contexts(&Failing, i64::MAX).unwrap();
        assert_eq!(stats.paths_cleared, 0, "a failed render clears no path");
        assert_eq!(stats.chunks_updated, 0);
        assert_eq!(stats.cleared_embeddings, 0);
        assert!(
            engine
                .context_dirty_paths("code")
                .unwrap()
                .contains(&FileKey::configuration("Owned.bsl")),
            "the mark survives a render failure so the next publish retries it",
        );
    }

    /// A mark stamped AFTER a build captured its start-seq is not consumed by that build's
    /// publish (its seq exceeds the bound) and IS consumed by the next build (whose start-seq
    /// covers it). This is the race a stale `.xml` drift lands in: it must not be cleared
    /// against a graph that predates it. Reverting the `seq <= seq_bound` bound on the read
    /// (consuming every mark) makes the later mark vanish in the first round and this fails.
    #[test]
    fn refresh_bounded_by_start_seq_excludes_later_marks_and_consumes_them_next_round() {
        struct NoContext;
        impl crate::ports::GraphContextProvider for NoContext {
            fn graph_context(&self, _rel: &str, _sym: &str, _kind: &str) -> Option<String> {
                None
            }
        }

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("bsl-search.db");
        let engine = SearchEngine::fts_only(&db_path).unwrap();

        // A build captures its start-seq AFTER the first drift marked A, but BEFORE a second
        // drift marks B (as if B's `.xml` landed while this build was already reading disk).
        engine.store().mark_context_dirty("code", CONFIGURATION_ROOT_ID, "A.bsl").unwrap();
        let build_start_seq = engine.mark_seq_handle().load(std::sync::atomic::Ordering::SeqCst);
        engine.store().mark_context_dirty("code", CONFIGURATION_ROOT_ID, "B.bsl").unwrap();
        let next_build_seq = engine.mark_seq_handle().load(std::sync::atomic::Ordering::SeqCst);
        assert!(next_build_seq > build_start_seq, "the later mark got a higher seq");

        // The build's publish consumes only A (seq <= its start-seq); B is left for a later
        // build so it is never cleared against this pre-drift graph.
        let stats = engine.refresh_dirty_contexts(&NoContext, build_start_seq).unwrap();
        assert_eq!(stats.paths_cleared, 1, "only the mark at or below the bound is consumed");
        let dirty = engine.context_dirty_paths("code").unwrap();
        assert!(
            !dirty.contains(&FileKey::configuration("A.bsl")),
            "A was within the bound and is cleared"
        );
        assert!(
            dirty.contains(&FileKey::configuration("B.bsl")),
            "B was stamped after build start and survives"
        );

        // The next build's start-seq covers B, so its publish consumes it.
        let stats = engine.refresh_dirty_contexts(&NoContext, next_build_seq).unwrap();
        assert_eq!(stats.paths_cleared, 1, "the follow-up build consumes the deferred mark");
        assert!(
            engine.context_dirty_paths("code").unwrap().is_empty(),
            "every mark is consumed once a build's start-seq covers it",
        );
    }

    /// A structural rescan reconciles the store against disk: a file deleted during a lost
    /// watch window (hub overflow / subtree removal) — absent from the freshly walked set —
    /// is removed (FTS rows dropped, live vector evicted, tombstone written); a file still
    /// present is untouched. Without this, a deleted file lingers in the index forever.
    #[test]
    fn reconcile_workspace_files_removes_stored_but_gone_files() {
        use crate::embedder::EmbedderConfig;
        use crate::{Chunk, ChunkKind, Store};

        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let db_path = workspace.join("bsl-search.db");
        let chunk = |name: &str| Chunk {
            kind: ChunkKind::Procedure,
            name: name.to_owned(),
            is_export: true,
            annotations: vec![],
            line_start: 0,
            line_end: 1,
            text: format!("Процедура {name}()\nКонецПроцедуры"),
        };
        let vec_a = vec![1.0f32, 0.0, 0.0];
        let vec_b = vec![0.0f32, 1.0, 0.0];
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file(
                    CONFIGURATION_ROOT_ID,
                    "Gone.bsl",
                    b"ha",
                    &[chunk("Ушедшая")],
                    Some(std::slice::from_ref(&vec_a)),
                )
                .unwrap();
            store
                .reindex_file(
                    CONFIGURATION_ROOT_ID,
                    "Kept.bsl",
                    b"hb",
                    &[chunk("Оставшаяся")],
                    Some(std::slice::from_ref(&vec_b)),
                )
                .unwrap();
        }

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
        let mut engine = SearchEngine::new(&db_path, config).unwrap();
        engine.set_workspace_root(workspace);
        assert_eq!(engine.file_count().unwrap(), 2, "both files indexed");

        // The rescan walked only the surviving file; `Gone.bsl` is absent from disk.
        let mut present = std::collections::HashSet::new();
        present.insert(workspace.join("Kept.bsl"));

        let removed = engine.reconcile_workspace_files(&present).unwrap();
        assert_eq!(removed, 1, "exactly the stored-but-gone file is reconciled out");

        assert_eq!(engine.file_count().unwrap(), 1, "only the surviving file remains");
        assert!(
            engine.text_search("Ушедшая", 10, Some("code")).unwrap().is_empty(),
            "the gone file no longer appears in FTS results",
        );
        assert!(
            !engine.text_search("Оставшаяся", 10, Some("code")).unwrap().is_empty(),
            "the surviving file is intact",
        );
        assert!(
            engine
                .store()
                .overlay_tombstone_paths("code")
                .unwrap()
                .contains(&FileKey::configuration("Gone.bsl")),
            "a tombstone blocks a baseline hit from resurrecting the gone file",
        );
        // The gone file's vector answers nothing; the survivor's still does.
        let hits = engine.search_with_embedding(&vec_a, 5, None).unwrap();
        assert!(
            hits.iter().all(|h| h.symbol_name != "Ушедшая"),
            "the reconciled file's vector is evicted from the live index: {hits:?}",
        );
    }

    /// Indexed files with their store rows, as a boot walk would have left them. Seeded in
    /// ONE call: the collection sync is a whole-collection operation, so a second call would
    /// evict what the first one wrote.
    fn seed_rows(engine: &mut SearchEngine, paths: &[&str]) {
        let documents: Vec<IndexedDocument> = paths
            .iter()
            .map(|path| IndexedDocument {
                collection: "code".to_owned(),
                root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
                path: (*path).to_owned(),
                symbol_name: "П".to_owned(),
                kind: "procedure".to_owned(),
                line_start: 0,
                line_end: 1,
                text: "Процедура П()\nКонецПроцедуры".to_owned(),
                content_hash: "h".to_owned(),
                graph_context: None,
            })
            .collect();
        engine.sync_indexed_documents_in_collection("code", &documents, None).unwrap();
    }

    /// The store row is what a reconcile sees a key by, so it must be the LAST thing a
    /// removal drops: a failure after it would leave nothing to select the key again, and
    /// the retry mark reaches the overlay only. Checked on the tombstone, whose write has
    /// always been fallible and has always run after the row.
    #[test]
    fn a_denied_tombstone_leaves_the_store_row_as_evidence() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        seed_rows(&mut engine, &["Removed.bsl"]);

        let saboteur = rusqlite::Connection::open(&db_path).unwrap();
        saboteur
            .execute_batch(
                "CREATE TRIGGER deny_tombstone BEFORE INSERT ON overlay_tombstones \
                 BEGIN SELECT RAISE(FAIL, 'deny'); END;",
            )
            .unwrap();

        assert!(engine.reconcile_workspace_files(&HashSet::new()).is_err(), "the denial surfaces");
        assert_eq!(engine.file_count().unwrap(), 1, "the row survives as evidence for a retry");

        saboteur.execute_batch("DROP TRIGGER deny_tombstone").unwrap();
        assert_eq!(
            engine.reconcile_workspace_files(&HashSet::new()).unwrap(),
            1,
            "once the fault clears, the key is still there to remove",
        );
    }

    /// Retracting the fingerprint row used to be best effort: its failure was logged and the
    /// removal reported success, leaving a row that claims the file was verified — enough for
    /// a namesake at the same size and mtime to inherit the claim across a restart.
    #[test]
    fn a_denied_fingerprint_retraction_fails_the_removal() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        seed_rows(&mut engine, &["Removed.bsl"]);
        engine
            .store()
            .save_overlay_fingerprint_cache(
                "",
                &HashMap::from([(
                    FileKey::configuration("Removed.bsl"),
                    crate::store::PersistedFingerprint {
                        file_size: 7,
                        file_mtime_secs: 1,
                        file_mtime_nanos: 0,
                        content_fingerprint: "fp".to_owned(),
                        canonical: workspace.join("Removed.bsl").display().to_string(),
                    },
                )]),
            )
            .unwrap();

        let saboteur = rusqlite::Connection::open(&db_path).unwrap();
        saboteur
            .execute_batch(
                "CREATE TRIGGER deny_fp_delete BEFORE DELETE ON overlay_fingerprint_cache \
                 BEGIN SELECT RAISE(FAIL, 'deny'); END;",
            )
            .unwrap();

        assert!(
            engine.reconcile_workspace_files(&HashSet::new()).is_err(),
            "a carrier left populated is not a successful removal",
        );
        assert_eq!(engine.file_count().unwrap(), 1, "the row survives as evidence for a retry");
    }

    /// Evicting the dead file's vectors is the one step whose retry dies with the store row:
    /// the chunk ids come from that row, and the dirty mark only ever reaches the overlay.
    #[test]
    fn a_failed_vector_eviction_fails_the_removal_and_keeps_the_row() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        seed_rows(&mut engine, &["Removed.bsl"]);

        FORCE_VECTOR_REMOVE_ERROR.with(|flag| flag.set(true));
        let outcome = engine.reconcile_workspace_files(&HashSet::new());
        FORCE_VECTOR_REMOVE_ERROR.with(|flag| flag.set(false));

        assert!(outcome.is_err(), "a vector left in the live index is not a successful removal");
        assert_eq!(engine.file_count().unwrap(), 1, "the row survives as evidence for a retry");
        assert_eq!(
            engine.reconcile_workspace_files(&HashSet::new()).unwrap(),
            1,
            "the retry finds the key exactly where the failed pass left it",
        );
    }

    /// A manifest that cannot be read is not evidence that there is no baseline copy. Taken
    /// as one, the removal would skip the hiding and the copy would keep being served — while
    /// the caller was told the file is gone.
    #[test]
    fn an_unreadable_manifest_fails_the_removal() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("Removed.bsl");
        fs::write(&file, "Процедура П()\nКонецПроцедуры").unwrap();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        seed_rows(&mut engine, &["Removed.bsl"]);
        engine
            .store()
            .save_baseline_manifest(&crate::WorkspaceBaselineManifest {
                snapshot_id: "snap".to_owned(),
                snapshot_fingerprint: Some("fp".to_owned()),
                files: vec![crate::BaselineManifestFile {
                    collection: "code".to_owned(),
                    path: "Removed.bsl".to_owned(),
                    file_fingerprint: "fp-file".to_owned(),
                    document_count: 1,
                    file_object_id: "obj-1".to_owned(),
                }],
            })
            .unwrap();
        engine.set_serves_external_baseline(true).unwrap();

        let saboteur = rusqlite::Connection::open(&db_path).unwrap();
        saboteur.execute_batch("DROP TABLE baseline_manifest_files").unwrap();

        fs::remove_file(&file).unwrap();
        assert!(
            engine.remove_workspace_path(&file).is_err(),
            "a removal that could not weigh the baseline is not a success",
        );
    }

    /// A reconcile is a batch: one key it cannot remove must not cost the others their pass.
    /// The failure is still reported — it just no longer strands the tail.
    #[test]
    fn a_reconcile_batch_outlives_a_failing_key() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        seed_rows(&mut engine, &["AFailing.bsl", "BHealthy.bsl"]);

        // Denied for exactly one key, so the batch has both a failing and a healthy member.
        let saboteur = rusqlite::Connection::open(&db_path).unwrap();
        saboteur
            .execute_batch(
                "CREATE TRIGGER deny_one_tombstone BEFORE INSERT ON overlay_tombstones \
                 WHEN NEW.path = 'AFailing.bsl' BEGIN SELECT RAISE(FAIL, 'deny'); END;",
            )
            .unwrap();

        assert!(engine.reconcile_workspace_files(&HashSet::new()).is_err(), "the failure is told");
        assert_eq!(
            engine.file_count().unwrap(),
            1,
            "the healthy key was removed despite the earlier failure",
        );
        assert!(
            engine
                .store()
                .all_files_in_collection("code")
                .unwrap()
                .iter()
                .any(|(key, _)| key.path == "AFailing.bsl"),
            "and the failing one is the one left behind",
        );
    }

    /// A file indexed AFTER boot lives in the overlay alone — the store rows stop growing
    /// once the daemon is up — so a reconcile that walks the rows cannot see it, and its
    /// entry keeps serving a file that is gone from disk.
    #[test]
    fn a_reconcile_removes_a_key_known_only_to_the_overlay() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("AfterBoot.bsl");
        fs::write(&file, "Процедура ПослеСтарта()\nКонецПроцедуры").unwrap();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        engine.initialize_workspace_overlay_clean().unwrap();
        assert!(engine.mark_workspace_path_dirty(&file).unwrap());
        engine.workspace_overlay_snapshot(RefreshMode::ReuseOnly).unwrap();
        let key = FileKey::configuration("AfterBoot.bsl");
        assert_eq!(engine.file_count().unwrap(), 0, "no boot walk ever wrote a row");
        assert!(
            engine.carrier_keys().unwrap().carriers_of(&key).contains(&KeyCarrier::OverlayEntry),
            "the overlay is the only carrier that knows this file",
        );

        fs::remove_file(&file).unwrap();
        let removed = engine.reconcile_workspace_files(&HashSet::new()).unwrap();

        assert_eq!(removed, 1, "the overlay-only key is reconciled out");
        assert!(
            engine.carrier_keys().unwrap().carriers_of(&key).is_empty(),
            "no carrier still knows it",
        );
    }

    /// The fingerprint row outlives its entry: an entry that matched the baseline is dropped
    /// while its row stays behind asserting the file was verified. Left alone, that row lets
    /// a namesake recreated at the same size and mtime inherit the claim across a restart.
    #[test]
    fn a_reconcile_removes_a_key_known_only_to_the_fingerprint_row() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        let key = FileKey::configuration("OnlyFingerprint.bsl");
        engine
            .store()
            .save_overlay_fingerprint_cache(
                "",
                &HashMap::from([(
                    key.clone(),
                    crate::store::PersistedFingerprint {
                        file_size: 7,
                        file_mtime_secs: 1,
                        file_mtime_nanos: 0,
                        content_fingerprint: "fp".to_owned(),
                        canonical: workspace.join("OnlyFingerprint.bsl").display().to_string(),
                    },
                )]),
            )
            .unwrap();
        assert_eq!(
            engine.carrier_keys().unwrap().carriers_of(&key),
            vec![KeyCarrier::FingerprintRow],
            "the fingerprint row is the only carrier",
        );

        let removed = engine.reconcile_workspace_files(&HashSet::new()).unwrap();

        assert_eq!(removed, 1, "the fingerprint-only key is reconciled out");
        assert!(
            engine.carrier_keys().unwrap().carriers_of(&key).is_empty(),
            "no carrier still knows it",
        );
    }

    /// Against a remote baseline the local rows are cleared on boot, so the manifest is the
    /// only carrier there is. A reconcile blind to it removes nothing at all, and a file
    /// deleted locally keeps arriving from the baseline.
    #[test]
    fn a_reconcile_hides_a_key_known_only_to_the_served_manifest() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        engine
            .store()
            .save_baseline_manifest(&crate::WorkspaceBaselineManifest {
                snapshot_id: "snap".to_owned(),
                snapshot_fingerprint: Some("fp".to_owned()),
                files: vec![crate::BaselineManifestFile {
                    collection: "code".to_owned(),
                    path: "Deleted.bsl".to_owned(),
                    file_fingerprint: "fp-file".to_owned(),
                    document_count: 1,
                    file_object_id: "obj-1".to_owned(),
                }],
            })
            .unwrap();
        engine.set_serves_external_baseline(true).unwrap();
        engine.initialize_workspace_overlay_clean().unwrap();
        let key = FileKey::configuration("Deleted.bsl");

        let removed = engine.reconcile_workspace_files(&HashSet::new()).unwrap();

        assert_eq!(removed, 1, "the manifest-only key is reconciled out");
        assert!(
            engine.workspace_overlay_cache.lock().unwrap().hidden_keys().contains(&key),
            "removing a manifest-only key is expressed by hiding its baseline copy",
        );
    }

    /// The manifest deliberately survives a mode switch, so its rows prove nothing to an
    /// engine that does not serve it. Reading them anyway would make a local engine remove
    /// ghosts of another mode's corpus.
    #[test]
    fn an_inherited_manifest_yields_no_candidates_in_the_local_mode() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        engine
            .store()
            .save_baseline_manifest(&crate::WorkspaceBaselineManifest {
                snapshot_id: "stale-snap".to_owned(),
                snapshot_fingerprint: Some("fp".to_owned()),
                files: vec![crate::BaselineManifestFile {
                    collection: "code".to_owned(),
                    path: "Ghost.bsl".to_owned(),
                    file_fingerprint: "fp-file".to_owned(),
                    document_count: 1,
                    file_object_id: "obj-1".to_owned(),
                }],
            })
            .unwrap();
        engine.initialize_workspace_overlay_clean().unwrap();

        assert_eq!(
            engine.reconcile_workspace_files(&HashSet::new()).unwrap(),
            0,
            "an inherited manifest is not evidence in the local mode",
        );
        // The positive control: the very same rows DO yield a candidate once served, so the
        // assertion above cannot be satisfied by never reading the manifest at all.
        engine.set_serves_external_baseline(true).unwrap();
        assert_eq!(
            engine.reconcile_workspace_files(&HashSet::new()).unwrap(),
            1,
            "served, the same manifest yields its key",
        );
    }

    /// Removing a manifest-only key cannot delete its row — the manifest is a snapshot of
    /// someone else's corpus — so the key survives its own removal. Re-selecting it every
    /// time would grow the removal count and the retry obligations without a single change
    /// to what search serves.
    #[test]
    fn a_second_reconcile_over_an_unchanged_tree_removes_nothing() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        engine
            .store()
            .save_baseline_manifest(&crate::WorkspaceBaselineManifest {
                snapshot_id: "snap".to_owned(),
                snapshot_fingerprint: Some("fp".to_owned()),
                files: vec![crate::BaselineManifestFile {
                    collection: "code".to_owned(),
                    path: "Deleted.bsl".to_owned(),
                    file_fingerprint: "fp-file".to_owned(),
                    document_count: 1,
                    file_object_id: "obj-1".to_owned(),
                }],
            })
            .unwrap();
        engine.set_serves_external_baseline(true).unwrap();
        engine.initialize_workspace_overlay_clean().unwrap();

        assert_eq!(engine.reconcile_workspace_files(&HashSet::new()).unwrap(), 1);
        assert_eq!(
            engine.reconcile_workspace_files(&HashSet::new()).unwrap(),
            0,
            "the second pass has nothing left to do",
        );
    }

    /// Hiding proves a file is absent from disk, NOT that its carriers were cleared: a clean
    /// full pass hides a baseline key it did not see without touching the store row. Treating
    /// a hidden key as already settled would leave that row, its chunks and its vectors alive
    /// for good.
    #[test]
    fn a_hidden_key_whose_row_survives_is_still_a_candidate() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("Hidden.bsl");
        fs::write(&file, "Процедура Спрятанная()\nКонецПроцедуры").unwrap();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);
        let key = FileKey::configuration("Hidden.bsl");
        assert_eq!(engine.file_count().unwrap(), 1, "the boot walk wrote its row");

        // A clean full pass over a tree the file has left: it hides the baseline key and
        // leaves the row exactly as it was.
        fs::remove_file(&file).unwrap();
        engine.workspace_overlay_snapshot(RefreshMode::Embed).unwrap();
        assert!(
            engine.workspace_overlay_cache.lock().unwrap().hidden_keys().contains(&key),
            "the clean pass hid the vanished baseline key",
        );
        assert_eq!(engine.file_count().unwrap(), 1, "the row survived the pass untouched");

        assert_eq!(
            engine.reconcile_workspace_files(&HashSet::new()).unwrap(),
            1,
            "hiding does not excuse the row from reconciliation",
        );
        assert_eq!(engine.file_count().unwrap(), 0, "the row is gone");
    }

    /// The reconcile grew its candidate set, so what it must NOT do grew with it: a file
    /// present on disk stays, however many carriers know about it.
    #[test]
    fn a_reconcile_keeps_an_overlay_only_file_that_is_still_on_disk() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("Alive.bsl");
        fs::write(&file, "Процедура Живая()\nКонецПроцедуры").unwrap();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        engine.initialize_workspace_overlay_clean().unwrap();
        assert!(engine.mark_workspace_path_dirty(&file).unwrap());
        engine.workspace_overlay_snapshot(RefreshMode::ReuseOnly).unwrap();
        let key = FileKey::configuration("Alive.bsl");

        let present = HashSet::from([file.clone()]);
        assert_eq!(
            engine.reconcile_workspace_files(&present).unwrap(),
            0,
            "a file the walk found is never removed",
        );
        assert!(
            !engine.carrier_keys().unwrap().carriers_of(&key).is_empty(),
            "its overlay entry is intact",
        );
    }

    /// A workspace removal writes an overlay tombstone so a baseline (Postgres-mode) hit
    /// for the same path cannot resurrect the locally-deleted file.
    #[test]
    fn remove_workspace_path_tombstones_so_a_baseline_hit_cannot_resurrect() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        engine
            .sync_indexed_documents_in_collection(
                "code",
                &[IndexedDocument {
                    collection: "code".to_owned(),
                    root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
                    path: "Removed.bsl".to_owned(),
                    symbol_name: "П".to_owned(),
                    kind: "procedure".to_owned(),
                    line_start: 0,
                    line_end: 1,
                    text: "Процедура П()\nКонецПроцедуры".to_owned(),
                    content_hash: "h".to_owned(),
                    graph_context: None,
                }],
                None,
            )
            .unwrap();

        assert!(engine.remove_workspace_path(workspace.join("Removed.bsl")).unwrap());

        let tombstones = engine.store().overlay_tombstone_paths("code").unwrap();
        assert!(
            tombstones.contains(&FileKey::configuration("Removed.bsl")),
            "the deleted path is tombstoned so a baseline hit stays hidden: {tombstones:?}",
        );
    }

    /// Two roots whose declared nesting is the reverse of their canonical one: an
    /// outer root reached through an alias, and an inner root registered under the
    /// alias's real path. A file deleted there cannot be canonicalized, and ranking
    /// the roots by their declared spellings alone would pick the outer one — so the
    /// removal would tombstone a key nobody ever wrote and leave the real row serving
    /// a dead hit.
    #[cfg(unix)]
    #[test]
    fn a_deletion_reached_through_an_alias_removes_the_row_the_file_lived_under() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let workspace = dir.path();
        let configuration = workspace.join("cf");
        fs::create_dir_all(&configuration).unwrap();
        let outer = outside.path().join("outer");
        let inner = outer.join("inner");
        fs::create_dir_all(&inner).unwrap();
        let alias = workspace.join("alias");
        std::os::unix::fs::symlink(&outer, &alias).unwrap();

        let file = inner.join("X.bsl");
        fs::write(&file, "Процедура П()\nКонецПроцедуры").unwrap();

        let mut engine = SearchEngine::fts_only(&workspace.join("bsl-search.db")).unwrap();
        let (roots, rejected) = crate::WorkspaceRoots::build(
            workspace,
            &configuration,
            &[alias.clone(), inner.clone()],
        );
        assert!(rejected.is_empty(), "both roots register: {rejected:?}");
        engine.set_workspace_roots(roots);

        // The file is stored under the root it physically lives in, which is what
        // the walk would have attributed it to.
        let lived_under =
            engine.workspace_file_key(&file).expect("a live file attributes to its own root");
        engine.store().upsert_file(&lived_under.root_id, &lived_under.path, b"h", "code").unwrap();
        assert_eq!(engine.file_count().unwrap(), 1);

        fs::remove_file(&file).unwrap();
        assert!(engine.remove_workspace_path(alias.join("inner/X.bsl")).unwrap());

        assert_eq!(
            engine.file_count().unwrap(),
            0,
            "the row of the root the file lived under is the one removed",
        );
    }

    /// A workspace removal marks the path dirty in the in-memory overlay cache, so a
    /// cached overlay entry for the deleted file stops serving stale hits on the next
    /// query's refresh.
    #[test]
    fn remove_workspace_path_drops_the_cached_overlay_entry() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("Ext.bsl");
        fs::write(&file, "Процедура Живая()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);
        engine.enable_workspace_watcher_mode();
        engine.prime_workspace_overlay().unwrap();

        // Edit the file so the overlay caches an entry for it, then confirm it serves.
        fs::write(&file, "Процедура ЖиваяПравка()\nКонецПроцедуры").unwrap();
        assert!(engine.mark_workspace_path_dirty(&file).unwrap());
        assert_eq!(
            engine.text_search("ЖиваяПравка", 10, Some("code")).unwrap().len(),
            1,
            "the edited file is served from the overlay cache",
        );

        // Delete the file and drive the removal branch.
        fs::remove_file(&file).unwrap();
        assert!(engine.remove_workspace_path(&file).unwrap());

        // The removal marked the path dirty; the next query's overlay refresh sees it gone
        // and drops the cached entry, so the stale hit disappears.
        assert!(
            engine.text_search("ЖиваяПравка", 10, Some("code")).unwrap().is_empty(),
            "the removed file no longer serves a stale overlay hit",
        );
    }

    /// A workspace removal evicts exactly the deleted chunks' vectors from the live index
    /// incrementally — it does NOT reload every embedding and rebuild the index. The live
    /// index keeps its count (a tombstone); a full reload would have shrunk it to the one
    /// surviving vector.
    #[test]
    fn remove_workspace_path_evicts_vectors_incrementally_without_full_reload() {
        use crate::embedder::EmbedderConfig;
        use crate::{Chunk, ChunkKind, Store};

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("bsl-search.db");
        let chunk = |name: &str| Chunk {
            kind: ChunkKind::Procedure,
            name: name.to_owned(),
            is_export: true,
            annotations: vec![],
            line_start: 0,
            line_end: 1,
            text: format!("Процедура {name}()\nКонецПроцедуры"),
        };
        let vec_a = vec![1.0f32, 0.0, 0.0];
        let vec_b = vec![0.0f32, 1.0, 0.0];
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file(
                    CONFIGURATION_ROOT_ID,
                    "a.bsl",
                    b"ha",
                    &[chunk("Альфа")],
                    Some(std::slice::from_ref(&vec_a)),
                )
                .unwrap();
            store
                .reindex_file(
                    CONFIGURATION_ROOT_ID,
                    "b.bsl",
                    b"hb",
                    &[chunk("Бета")],
                    Some(std::slice::from_ref(&vec_b)),
                )
                .unwrap();
        }

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
        let mut engine = SearchEngine::new(&db_path, config).unwrap();
        engine.set_workspace_root(dir.path());
        assert_eq!(engine.index.len(), 2, "both vectors load at construction");

        assert!(engine.remove_workspace_path(dir.path().join("a.bsl")).unwrap());

        // Incremental eviction keeps the live count (tombstone); a full reload would have
        // rebuilt the index to exactly the one surviving vector.
        assert_eq!(
            engine.index.len(),
            2,
            "removal evicts incrementally; it does not reload every embedding",
        );
        // The evicted vector no longer answers even its own query; the survivor still does.
        let hits_a = engine.search_with_embedding(&vec_a, 5, None).unwrap();
        assert!(
            hits_a.iter().all(|h| h.symbol_name != "Альфа"),
            "the removed file's vector is gone from the live index: {hits_a:?}",
        );
        let hits_b = engine.search_with_embedding(&vec_b, 5, None).unwrap();
        assert_eq!(hits_b.first().map(|h| h.symbol_name.as_str()), Some("Бета"));
    }

    /// A `.bsl`-spelled link resolving to a non-source target keeps its key under the WALKED
    /// spelling: a key under the target's root is forbidden to exist (the walk drops such
    /// files), and the walked key is the only one the file could have been indexed under — the
    /// one a removal must reach.
    #[cfg(unix)]
    #[test]
    fn a_link_to_a_non_source_target_keys_by_its_walked_spelling() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let configuration = workspace.join("cf");
        let extension = workspace.join("cfe");
        fs::create_dir_all(&configuration).unwrap();
        fs::create_dir_all(&extension).unwrap();
        let target = extension.join("Target.txt");
        fs::write(&target, "не исходник").unwrap();
        let alias = configuration.join("Alias.bsl");
        std::os::unix::fs::symlink(&target, &alias).unwrap();

        let mut engine = SearchEngine::fts_only(&workspace.join("bsl-search.db")).unwrap();
        let (roots, _) = crate::WorkspaceRoots::build(
            workspace,
            &configuration,
            std::slice::from_ref(&extension),
        );
        engine.set_workspace_roots(roots);
        let key = engine.workspace_file_key(&alias).expect("the walked spelling is a .bsl");
        assert_eq!(
            (key.root_id.as_str(), key.path.as_str()),
            (crate::CONFIGURATION_ROOT_ID, "Alias.bsl"),
            "attribution must not follow the non-source target into its root"
        );
    }

    /// Walked-spelling attribution must rank the DECLARED roots: for a root declared through a
    /// link, the walked path also lies under the enclosing root's canonical spelling, and a
    /// canonical-ranked lookup would hand the key to the wrong root — missing the row the
    /// removal must reach.
    #[cfg(unix)]
    #[test]
    fn a_non_source_target_under_a_linked_root_keys_by_the_declared_root() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let workspace = dir.path();
        let configuration = workspace.join("cf");
        fs::create_dir_all(&configuration).unwrap();
        let real_ext = outside.path().join("ext");
        fs::create_dir_all(&real_ext).unwrap();
        let ext_link = configuration.join("ext");
        std::os::unix::fs::symlink(&real_ext, &ext_link).unwrap();
        let source = outside.path().join("Source.bsl");
        fs::write(&source, "Процедура Настоящая()\nКонецПроцедуры").unwrap();
        let alias = real_ext.join("Alias.bsl");
        std::os::unix::fs::symlink(&source, &alias).unwrap();

        let mut engine = SearchEngine::fts_only(&workspace.join("bsl-search.db")).unwrap();
        let (roots, _) = crate::WorkspaceRoots::build(
            workspace,
            &configuration,
            std::slice::from_ref(&ext_link),
        );
        engine.set_workspace_roots(roots);
        let walked_alias = ext_link.join("Alias.bsl");
        let old_key = engine.workspace_file_key(&walked_alias).unwrap();
        engine.store().upsert_file(&old_key.root_id, &old_key.path, b"h", "code").unwrap();

        let foreign = outside.path().join("Foreign.txt");
        fs::write(&foreign, "не исходник").unwrap();
        fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&foreign, &alias).unwrap();
        assert!(engine.remove_workspace_path(&walked_alias).unwrap());
        assert_eq!(
            engine.file_count().unwrap(),
            0,
            "the removal must reach the key the file was indexed under"
        );
    }

    /// A directory spelled `.bsl` must not pass for a source target: extension-only role
    /// classification would attribute the key to the DIRECTORY'S root, and the stale row under
    /// the walked key would never be reached.
    #[cfg(unix)]
    #[test]
    fn a_directory_target_spelled_bsl_keys_by_the_walked_spelling() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let workspace = dir.path();
        let configuration = workspace.join("cf");
        let extension = workspace.join("cfe");
        fs::create_dir_all(&configuration).unwrap();
        fs::create_dir_all(&extension).unwrap();
        let source = outside.path().join("Source.bsl");
        fs::write(&source, "Процедура Настоящая()\nКонецПроцедуры").unwrap();
        let alias = configuration.join("Alias.bsl");
        std::os::unix::fs::symlink(&source, &alias).unwrap();

        let mut engine = SearchEngine::fts_only(&workspace.join("bsl-search.db")).unwrap();
        let (roots, _) = crate::WorkspaceRoots::build(
            workspace,
            &configuration,
            std::slice::from_ref(&extension),
        );
        engine.set_workspace_roots(roots);
        let old_key = engine.workspace_file_key(&alias).unwrap();
        engine.store().upsert_file(&old_key.root_id, &old_key.path, b"h", "code").unwrap();

        fs::create_dir(extension.join("Target.bsl")).unwrap();
        fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(extension.join("Target.bsl"), &alias).unwrap();
        assert!(engine.remove_workspace_path(&alias).unwrap());
        assert_eq!(
            engine.file_count().unwrap(),
            0,
            "a live directory target is not a source; the walked key owns the row"
        );
    }

    /// A deletion PROVEN by the removal channel must drop the cached overlay entry even when
    /// the whole root vanished with the file: the point refresh would read the dead root as
    /// "unreachable, retry" and leave a ghost entry serving hits forever.
    #[test]
    fn removing_a_file_under_a_vanished_root_drops_its_overlay_entry() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let configuration = workspace.join("cf");
        fs::create_dir_all(&configuration).unwrap();
        fs::write(configuration.join("A.bsl"), "Процедура Локальная()\nКонецПроцедуры").unwrap();

        let mut engine = SearchEngine::fts_only(&workspace.join("bsl-search.db")).unwrap();
        let (roots, _) = crate::WorkspaceRoots::build(workspace, &configuration, &[]);
        engine.set_workspace_roots(roots);
        engine.prime_workspace_overlay().unwrap();
        assert_eq!(engine.text_search("Локальная", 10, Some("code")).unwrap().len(), 1);

        fs::rename(&configuration, workspace.join("cf.saved")).unwrap();
        engine.remove_workspace_path(configuration.join("A.bsl")).unwrap();
        let hits = engine.text_search("Локальная", 10, Some("code")).unwrap();
        assert!(hits.is_empty(), "a proven removal must not leave a ghost entry: {hits:?}");
    }

    /// The proven-removal channel must retract the persisted fingerprint row too: the dirty
    /// mark dies with the process, and a namesake recreated at the same `(len, mtime,
    /// canonical)` would inherit the dead file's "verified" claim across a restart.
    #[test]
    fn a_proven_removal_retracts_the_fingerprint_row() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура Локальная()\nКонецПроцедуры").unwrap();

        let mut engine = SearchEngine::fts_only(&workspace.join("bsl-search.db")).unwrap();
        let (roots, _) = crate::WorkspaceRoots::build(workspace, workspace, &[]);
        engine.set_workspace_roots(roots);
        engine.prime_workspace_overlay().unwrap();
        let key = engine.workspace_file_key(&file).unwrap();
        engine
            .store()
            .save_overlay_fingerprint_cache(
                "",
                &HashMap::from([(
                    key.clone(),
                    crate::store::PersistedFingerprint {
                        file_size: 1,
                        file_mtime_secs: 2,
                        file_mtime_nanos: 3,
                        content_fingerprint: "fp".to_owned(),
                        canonical: "/spelled".to_owned(),
                    },
                )]),
            )
            .unwrap();

        fs::remove_file(&file).unwrap();
        engine.remove_workspace_path(&file).unwrap();
        assert!(
            !engine
                .store()
                .load_overlay_fingerprint_cache("")
                .unwrap_or(None)
                .unwrap_or_default()
                .contains_key(&key),
            "the dead file's row must not vouch for a future namesake"
        );
    }

    /// A symlink spelled `.bsl` whose target is not a BSL source is not a source
    /// file: the graph walk drops it because the roles of the two spellings
    /// disagree, and the overlay must agree with that universe.
    #[cfg(unix)]
    #[test]
    fn probe_a_symlink_to_a_non_bsl_target_is_not_indexed() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let configuration = workspace.join("cf");
        let extension = workspace.join("cfe");
        fs::create_dir_all(&configuration).unwrap();
        fs::create_dir_all(&extension).unwrap();
        let target = extension.join("Target.txt");
        fs::write(&target, "Процедура ТолькоЧерезСсылку()\nКонецПроцедуры").unwrap();
        let alias = configuration.join("Alias.bsl");
        std::os::unix::fs::symlink(&target, &alias).unwrap();

        let mut engine = SearchEngine::fts_only(&workspace.join("bsl-search.db")).unwrap();
        let (roots, _) = crate::WorkspaceRoots::build(
            workspace,
            &configuration,
            std::slice::from_ref(&extension),
        );
        engine.set_workspace_roots(roots);
        engine.prime_workspace_overlay().unwrap();
        let before = engine.text_search("ТолькоЧерезСсылку", 10, Some("code")).unwrap();
        assert!(before.is_empty(), "a .txt is not a BSL source file: {before:?}");
    }

    /// End-to-end through a REAL walk: a subtree that loses read permission
    /// makes the scan unclean, and the indexed file inside it survives instead
    /// of being read as deleted. The policy and the walker are each tested on
    /// their own; this leg catches the adapter between them dropping a counter.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_subtree_does_not_erase_its_indexed_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let closed = workspace.join("closed");
        fs::create_dir(&closed).unwrap();
        fs::write(closed.join("Hidden.bsl"), "Процедура ЗаЗакрытымКаталогом()\nКонецПроцедуры")
            .unwrap();

        let mut engine = SearchEngine::fts_only(&workspace.join("bsl-search.db")).unwrap();
        let (roots, _) = crate::WorkspaceRoots::build(workspace, workspace, &[]);
        engine.set_workspace_roots(roots);
        engine.prime_workspace_overlay().unwrap();
        let hits = engine.text_search("ЗаЗакрытымКаталогом", 10, Some("code")).unwrap();
        assert_eq!(hits.len(), 1, "the file is indexed while readable");

        fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&closed).is_ok() {
            // Running as root: permissions cannot make the subtree unreadable.
            fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        let rescan = engine.prime_workspace_overlay();
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).unwrap();
        rescan.unwrap();

        let hits = engine.text_search("ЗаЗакрытымКаталогом", 10, Some("code")).unwrap();
        assert_eq!(hits.len(), 1, "an unreadable subtree is not evidence of deletion");
        assert!(
            engine.workspace_overlay_needs_full_rescan().unwrap(),
            "the unclean prime leaves the overlay waiting for a clean rescan"
        );
    }

    /// A cold overlay prime is exactly one walk of the workspace, and an
    /// initialized watcher-mode engine performs none: the walk count is the
    /// observable proving every overlay pass shares the one common scan instead
    /// of a private traversal with its own symlink and error policy.
    #[test]
    fn a_cold_prime_walks_the_workspace_exactly_once() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        fs::write(workspace.join("M.bsl"), "Процедура Раз()\nКонецПроцедуры").unwrap();

        let mut engine = SearchEngine::fts_only(&workspace.join("bsl-search.db")).unwrap();
        let (roots, _) = crate::WorkspaceRoots::build(workspace, workspace, &[]);
        engine.set_workspace_roots(roots);

        let before = project_model::source_set::scans_performed_on_thread();
        engine.prime_workspace_overlay().unwrap();
        let walked = project_model::source_set::scans_performed_on_thread() - before;
        assert_eq!(walked, 1, "one cold prime is one walk");

        engine.enable_workspace_watcher_mode();
        let before = project_model::source_set::scans_performed_on_thread();
        engine.prime_workspace_overlay().unwrap();
        let walked = project_model::source_set::scans_performed_on_thread() - before;
        assert_eq!(walked, 0, "an initialized watcher-mode cache must not rescan");
    }

    /// The removal's retry obligation (the dirty mark) must be set BEFORE the fallible store
    /// operations: an early failure would otherwise leave no signal anywhere while the rows
    /// still tell the old story.
    #[test]
    fn a_failed_removal_still_leaves_the_retry_mark() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace);
        engine
            .sync_indexed_documents_in_collection(
                "code",
                &[IndexedDocument {
                    collection: "code".to_owned(),
                    root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
                    path: "Removed.bsl".to_owned(),
                    symbol_name: "П".to_owned(),
                    kind: "procedure".to_owned(),
                    line_start: 0,
                    line_end: 1,
                    text: "Процедура П()\nКонецПроцедуры".to_owned(),
                    content_hash: "h".to_owned(),
                    graph_context: None,
                }],
                None,
            )
            .unwrap();

        let saboteur = rusqlite::Connection::open(&db_path).unwrap();
        saboteur
            .execute_batch(
                "CREATE TRIGGER deny_tombstone BEFORE INSERT ON overlay_tombstones \
                 BEGIN SELECT RAISE(FAIL, 'deny'); END;",
            )
            .unwrap();
        let result = engine.remove_workspace_path(workspace.join("Removed.bsl"));
        assert!(result.is_err(), "the denied tombstone surfaces as an error");
        assert!(
            engine
                .workspace_overlay_dirty_paths_snapshot()
                .unwrap()
                .contains_key(&FileKey::configuration("Removed.bsl")),
            "the retry mark was set before the store failed"
        );
    }

    /// An INHERITED manifest (a warm-cache left by a Postgres period) is not baseline
    /// evidence for a LOCAL engine: a removal must not hide anything on its account.
    #[test]
    fn a_local_removal_ignores_an_inherited_manifest() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("Removed.bsl");
        let content = "Процедура П()\nКонецПроцедуры";
        fs::write(&file, content).unwrap();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);
        engine
            .store()
            .save_baseline_manifest(&crate::WorkspaceBaselineManifest {
                snapshot_id: "stale-snap".to_owned(),
                snapshot_fingerprint: Some("fp".to_owned()),
                files: vec![crate::BaselineManifestFile {
                    collection: "code".to_owned(),
                    path: "Removed.bsl".to_owned(),
                    file_fingerprint: crate::workspace_overlay::fingerprint_content(
                        content,
                        "Removed.bsl",
                    ),
                    document_count: 1,
                    file_object_id: "obj-1".to_owned(),
                }],
            })
            .unwrap();

        fs::remove_file(&file).unwrap();
        assert!(engine.remove_workspace_path(workspace.join("Removed.bsl")).unwrap());
        let stats = engine.workspace_overlay_stats().unwrap().unwrap();
        assert_eq!(stats.deleted_files, 0, "an inherited manifest proves no baseline copy to hide");
    }

    /// A LOCAL engine's dirty-path refresh reads its edits against the local store rows, not
    /// against an inherited manifest: an edit that happens to equal the STALE manifest
    /// fingerprint must still become an overlay entry.
    #[test]
    fn a_local_point_refresh_ignores_an_inherited_manifest() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("CommonModule.bsl");
        let local = "Процедура Локальная()\nКонецПроцедуры";
        let edited = "Процедура Правка()\nКонецПроцедуры";
        fs::write(&file, local).unwrap();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.index_directory_fts(workspace).unwrap();
        engine.set_workspace_root(workspace);
        engine.enable_workspace_watcher_mode();
        engine.prime_workspace_overlay().unwrap();
        engine
            .store()
            .save_baseline_manifest(&crate::WorkspaceBaselineManifest {
                snapshot_id: "stale-snap".to_owned(),
                snapshot_fingerprint: Some("fp".to_owned()),
                files: vec![crate::BaselineManifestFile {
                    collection: "code".to_owned(),
                    path: "CommonModule.bsl".to_owned(),
                    file_fingerprint: crate::workspace_overlay::fingerprint_content(
                        edited,
                        "CommonModule.bsl",
                    ),
                    document_count: 1,
                    file_object_id: "obj-1".to_owned(),
                }],
            })
            .unwrap();

        fs::write(&file, edited).unwrap();
        assert!(engine.mark_workspace_path_dirty(&file).unwrap());
        let stats = engine.workspace_overlay_stats().unwrap().unwrap();
        assert_eq!(
            stats.overlay_files, 1,
            "the edit differs from the LOCAL baseline and must serve as an overlay entry \
             even though it equals the stale manifest fingerprint"
        );
    }

    /// Declaring the local mode clears inherited fingerprint rows: they claim "verified
    /// against the manifest", and the raw mode can neither honour nor refresh that claim —
    /// a same-stat edit during the local period would be suppressed by the row after a
    /// switch back to the remote mode.
    #[test]
    fn declaring_the_local_mode_clears_inherited_fingerprint_rows() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let db_path = workspace.join("bsl-search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine
            .store()
            .save_overlay_fingerprint_cache(
                "snap-1",
                &HashMap::from([(
                    FileKey::configuration("A.bsl"),
                    crate::store::PersistedFingerprint {
                        file_size: 1,
                        file_mtime_secs: 1,
                        file_mtime_nanos: 0,
                        content_fingerprint: "fp".to_owned(),
                        canonical: String::new(),
                    },
                )]),
            )
            .unwrap();

        engine.set_serves_external_baseline(false).unwrap();
        let rows = engine
            .store()
            .load_overlay_fingerprint_cache("snap-1")
            .unwrap_or(None)
            .unwrap_or_default();
        assert!(rows.is_empty(), "the local mode owns no manifest-verified rows");
    }
    /// A warm root must not be walked when only the cold ones need ingesting: the per-root skip
    /// exists to keep a restart cheap, and walking everything and filtering afterwards spends
    /// exactly what it was meant to save. Attribution still consults the whole table, so a file
    /// found under one root but owned by another keeps its owner's key.
    #[test]
    fn a_subset_walk_visits_only_the_roots_it_was_given() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().join("ws");
        let configuration = workspace.join("cf");
        let extension = dir.path().join("outside-ext");
        fs::create_dir_all(&configuration).unwrap();
        fs::create_dir_all(&extension).unwrap();
        fs::write(configuration.join("Тёплый.bsl"), "Процедура Первая()\nКонецПроцедуры").unwrap();
        fs::write(extension.join("Холодный.bsl"), "Процедура Вторая()\nКонецПроцедуры").unwrap();

        let mut engine = SearchEngine::fts_only(&dir.path().join("search.db")).unwrap();
        let (roots, _) = crate::WorkspaceRoots::build(
            &workspace,
            &configuration,
            std::slice::from_ref(&extension),
        );
        engine.set_workspace_roots(roots);

        let all = engine.boot_ingest_files(&configuration);
        assert_eq!(all.len(), 2, "the full walk covers both roots: {all:?}");

        let cold_only = engine.boot_ingest_files_over(
            std::path::Path::new(""),
            Some(std::slice::from_ref(&extension)),
        );
        let names: Vec<String> = cold_only.iter().map(|(key, _)| key.path.clone()).collect();
        assert_eq!(names, vec!["Холодный.bsl".to_owned()], "only the given root is walked");
    }
    /// A relative path handed to the engine is spelled against the CONFIGURATION root — that is
    /// how every stored path with the reserved id is spelled, and what callers strip before
    /// handing one over (the graph bridge strips the configuration prefix). Resolving it against
    /// the table's workspace instead points one directory too high whenever the configuration
    /// sits in a subdirectory, and the key is then silently not found: the mark is dropped and
    /// the stale graph context is served on.
    #[test]
    fn a_relative_path_is_spelled_against_the_configuration_root() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().join("ws");
        let configuration = workspace.join("src").join("cf");
        let module = configuration.join("CommonModules").join("Б").join("Ext");
        fs::create_dir_all(&module).unwrap();
        fs::write(module.join("Module.bsl"), "Процедура Первая()\nКонецПроцедуры").unwrap();

        let mut engine = SearchEngine::fts_only(&dir.path().join("search.db")).unwrap();
        let (roots, _) = crate::WorkspaceRoots::build(&workspace, &configuration, &[]);
        engine.set_workspace_roots(roots);
        engine.index_directory_fts(&configuration).unwrap();
        assert_eq!(engine.file_count().unwrap(), 1, "the fixture indexes the module");

        let marked = engine
            .mark_workspace_path_context_dirty("CommonModules/Б/Ext/Module.bsl")
            .expect("marking a workspace path is not an error");
        assert!(marked, "a configuration-relative path resolves to its stored key");
    }
}
