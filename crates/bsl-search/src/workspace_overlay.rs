use crate::domain::{BaselineRef, CorpusId, DocumentPath, IndexedDocument, SearchOverlay};
use crate::embedder::Embedder;
use crate::error::SearchError;
use crate::lexical::lexical_hits_for_documents;
use crate::ports::GraphContextProvider;
use crate::store::Store;
use code_chunk::Chunker;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineHashMode {
    RawFileBytes,
    NormalizedChunks,
}

/// How a refresh treats overlay chunks that lack a cached embedding.
///
/// [`RefreshMode::Embed`] may call the remote embedder to fill missing vectors;
/// [`RefreshMode::ReuseOnly`] never embeds inline (no remote round-trip under the engine lock),
/// so a chunk without a cached vector simply contributes no overlay semantic vector this turn
/// while remaining lexically searchable.
///
/// Every interactive query path is [`RefreshMode::ReuseOnly`]: the engine lock is held there, so
/// embedding must stay off it. The only embedding path is the background warmup, which runs
/// lock-free against a standalone store (see
/// [`crate::SearchEngine::prime_workspace_overlay_standalone`]) rather than through the in-engine
/// refresh; [`RefreshMode::Embed`] therefore remains available for the no-baseline / test refresh
/// paths that legitimately embed inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshMode {
    Embed,
    ReuseOnly,
}

#[derive(Debug, Clone)]
pub struct OverlayVectorDocument {
    pub document: IndexedDocument,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceOverlayIndex {
    pub overlay: SearchOverlay,
    pub hidden_paths: HashSet<String>,
    pub lexical_documents: Vec<IndexedDocument>,
    pub vector_documents: Vec<OverlayVectorDocument>,
}

impl WorkspaceOverlayIndex {
    pub fn is_empty(&self) -> bool {
        self.overlay.changes.is_empty()
    }
}

/// A read-only plan for a manifest-driven overlay refresh, produced off any lock against a
/// standalone store (Phase A) and applied later under the inner cache lock (Phase C).
///
/// Splitting plan/embed/publish keeps the slow remote embed (Phase B) entirely off the engine
/// and inner-cache locks: Phase A only reads files and the warm embedding cache, Phase B embeds
/// the `missing_embeddings` inputs with no lock held, and Phase C merges everything atomically.
#[derive(Debug, Clone)]
pub struct RefreshPlan {
    snapshot_id: String,
    /// Overlay file entries with lexical docs + embedding inputs but no vectors yet; vectors are
    /// assembled in Phase C from the merged embedding cache.
    entries: Vec<(String, PlannedEntry)>,
    hidden_paths: HashSet<String>,
    updated_persisted: HashMap<String, crate::store::PersistedFingerprint>,
    /// Distinct `embedding_key -> embedding input` pairs that have no warm-cache vector; these are
    /// the inputs Phase B embeds. The key is the hash of the embedding input (the semantic key).
    missing_embeddings: HashMap<String, String>,
}

impl RefreshPlan {
    /// The distinct `(embedding_key, embedding_input)` pairs Phase B must embed.
    pub fn missing_embeddings(&self) -> &HashMap<String, String> {
        &self.missing_embeddings
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.hidden_paths.is_empty()
    }

    /// Number of locally-changed files this plan re-embeds. Reported in `search status` so an
    /// agent can see how much of the overlay differed from the baseline.
    pub fn overlay_file_count(&self) -> usize {
        self.entries.len()
    }

    /// Every overlay embedding key referenced by the planned entries. The caller uses this to pull
    /// warm-reused vectors into the published embedding set so Phase C builds complete vectors. The
    /// key is the hash of each chunk's embedding input (the semantic key), matching how the cache
    /// is keyed in [`build_overlay_vectors`].
    pub fn planned_embedding_keys(&self) -> impl Iterator<Item = String> + '_ {
        self.entries.iter().flat_map(|(_, entry)| {
            entry.embedding_inputs.iter().map(|input| overlay_embedding_key(input))
        })
    }
}

#[derive(Debug, Clone)]
struct PlannedEntry {
    fingerprint: FileFingerprint,
    file_hash: Vec<u8>,
    lexical_documents: Vec<IndexedDocument>,
    embedding_inputs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceOverlayStats {
    pub overlay_files: usize,
    pub deleted_files: usize,
    pub hidden_paths: usize,
    pub lexical_chunks: usize,
    pub semantic_chunks: usize,
    pub cached_embeddings: usize,
    pub watcher_mode: bool,
    pub pending_dirty_paths: usize,
}

#[derive(Default)]
pub struct WorkspaceOverlayCache {
    entries: HashMap<String, OverlayFileEntry>,
    hidden_paths: HashSet<String>,
    embedding_cache: HashMap<String, Vec<f32>>,
    /// Watcher-marked paths awaiting re-embed, each tagged with the sequence at which it was last
    /// marked. The sequence lets [`publish_plan`] tell a path superseded by its refresh from one
    /// the watcher re-marked while the lock-free embed was in flight (same path, newer sequence).
    dirty_paths: HashMap<String, u64>,
    dirty_seq: u64,
    watcher_mode: bool,
    initialized: bool,
    /// Optional graph-context provider (dependency-inverted). When set, overlay
    /// (uncommitted-edit) chunks are enriched with their call-graph context before
    /// embedding, matching the local index.
    graph_context_provider: Option<Arc<dyn GraphContextProvider>>,
}

impl std::fmt::Debug for WorkspaceOverlayCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceOverlayCache")
            .field("entries", &self.entries)
            .field("hidden_paths", &self.hidden_paths)
            .field("embedding_cache_len", &self.embedding_cache.len())
            .field("dirty_paths", &self.dirty_paths)
            .field("watcher_mode", &self.watcher_mode)
            .field("initialized", &self.initialized)
            .field("graph_context", &self.graph_context_provider.is_some())
            .finish()
    }
}

impl WorkspaceOverlayCache {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.hidden_paths.clear();
        self.dirty_paths.clear();
        self.initialized = false;
    }

    /// Inject the graph-context provider so overlay chunks are enriched like the
    /// local index. Clears cached entries so they rebuild with context.
    pub fn set_graph_context_provider(&mut self, provider: Arc<dyn GraphContextProvider>) {
        self.graph_context_provider = Some(provider);
        self.entries.clear();
        self.embedding_cache.clear();
        self.initialized = false;
    }

    pub fn enable_watcher_mode(&mut self) {
        self.watcher_mode = true;
    }

    pub fn mark_dirty_path(&mut self, rel_path: impl Into<String>) {
        self.dirty_seq += 1;
        self.dirty_paths.insert(rel_path.into(), self.dirty_seq);
    }

    /// `allow_cold_scan` gates the only expensive operation here: a cold full-tree scan + read +
    /// chunk of every workspace file (`full_refresh_from_manifest`). The background warmup
    /// (`RefreshMode::Embed`) passes `true`; every interactive query/status path passes `false`
    /// so it stays O(cached) under the engine lock and answers from the Postgres baseline until the
    /// warmup (or the watcher's incremental path) populates the overlay. Without this gate a single
    /// query on an unwarmed overlay would block for minutes walking the whole tree.
    pub fn refresh_with_manifest(
        &mut self,
        manifest_fingerprints: &HashMap<String, String>,
        workspace_root: &Path,
        embedder: Option<&Embedder>,
        batch_size: usize,
        store: &Store,
        allow_cold_scan: bool,
    ) -> Result<(), SearchError> {
        if allow_cold_scan {
            if !self.initialized || !self.watcher_mode {
                self.full_refresh_from_manifest(
                    manifest_fingerprints,
                    workspace_root,
                    embedder,
                    batch_size,
                    store,
                )?;
            } else if !self.dirty_paths.is_empty() {
                self.refresh_dirty_paths_from_manifest(
                    manifest_fingerprints,
                    workspace_root,
                    embedder,
                    batch_size,
                )?;
            }
            self.initialized = true;
        } else if self.initialized && !self.dirty_paths.is_empty() {
            // ReuseOnly: never cold-scan. An already-populated cache still applies the cheap
            // watcher-marked dirty-path refresh, but a `!watcher_mode` (polling) cache must NOT
            // re-run the full scan. An uninitialized cache stays empty (and `initialized` stays
            // false) so the next warmup/watcher pass still builds it.
            self.refresh_dirty_paths_from_manifest(
                manifest_fingerprints,
                workspace_root,
                embedder,
                batch_size,
            )?;
        }
        Ok(())
    }

    /// `allow_cold_scan` gates the cold full-tree scan + read + chunk (`full_refresh`). See
    /// [`Self::refresh_with_manifest`] for the rationale: only the background warmup
    /// (`RefreshMode::Embed`) may pay that cost; interactive query/status paths pass `false` and
    /// stay O(cached).
    pub fn refresh(
        &mut self,
        store: &Store,
        workspace_root: &Path,
        embedder: Option<&Embedder>,
        batch_size: usize,
        hash_mode: BaselineHashMode,
        allow_cold_scan: bool,
    ) -> Result<(), SearchError> {
        if allow_cold_scan {
            let baseline_files: HashMap<String, Vec<u8>> =
                store.all_files_in_collection("code")?.into_iter().collect();
            if !self.initialized || !self.watcher_mode {
                self.full_refresh(
                    &baseline_files,
                    workspace_root,
                    embedder,
                    batch_size,
                    hash_mode,
                )?;
            } else if !self.dirty_paths.is_empty() {
                self.refresh_dirty_paths(
                    &baseline_files,
                    workspace_root,
                    embedder,
                    batch_size,
                    hash_mode,
                )?;
            }
            self.initialized = true;
        } else if self.initialized && !self.dirty_paths.is_empty() {
            // ReuseOnly: never cold-scan. Only the cheap dirty-path refresh on an already-populated
            // cache; a `!watcher_mode` (polling) cache must NOT re-run the full scan, and an
            // uninitialized cache stays empty for the warmup/watcher to build later.
            let baseline_files: HashMap<String, Vec<u8>> =
                store.all_files_in_collection("code")?.into_iter().collect();
            self.refresh_dirty_paths(
                &baseline_files,
                workspace_root,
                embedder,
                batch_size,
                hash_mode,
            )?;
        }
        Ok(())
    }

    fn full_refresh(
        &mut self,
        baseline_files: &HashMap<String, Vec<u8>>,
        workspace_root: &Path,
        embedder: Option<&Embedder>,
        batch_size: usize,
        hash_mode: BaselineHashMode,
    ) -> Result<(), SearchError> {
        let workspace_files = scan_workspace_files(workspace_root);
        let mut seen_paths = HashSet::new();
        let mut hidden_paths = HashSet::new();

        for file in workspace_files {
            seen_paths.insert(file.rel_path.clone());
            let baseline_hash = baseline_files.get(&file.rel_path);

            let mut should_remove_cached_entry = false;
            if let Some(entry) = self.entries.get_mut(&file.rel_path) {
                if entry.fingerprint == file.fingerprint {
                    if baseline_hash.is_some_and(|stored_hash| stored_hash == &entry.file_hash) {
                        should_remove_cached_entry = true;
                    } else {
                        if baseline_hash.is_some() {
                            hidden_paths.insert(file.rel_path.clone());
                        }
                        if entry.vector_documents.is_empty() {
                            // ReuseOnly passes `embedder = None`: this attaches any cached
                            // vectors and leaves the rest lexical-only. Embed (warmup) fills
                            // the gaps via the remote embedder.
                            entry.vector_documents = build_overlay_vectors(
                                embedder,
                                batch_size,
                                &entry.lexical_documents,
                                &entry.embedding_inputs,
                                &mut self.embedding_cache,
                            )?;
                        }
                        continue;
                    }
                }
            }
            if should_remove_cached_entry {
                self.entries.remove(&file.rel_path);
                continue;
            }

            let content = match std::fs::read_to_string(&file.abs_path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let file_hash = compute_file_hash(&content, hash_mode);
            if baseline_hash.is_some_and(|stored_hash| stored_hash == &file_hash) {
                self.entries.remove(&file.rel_path);
                continue;
            }

            let provider = self.graph_context_provider.clone();
            let entry = build_overlay_entry(
                &file.rel_path,
                &content,
                file.fingerprint,
                file_hash,
                embedder,
                batch_size,
                &mut self.embedding_cache,
                provider.as_deref(),
            )?;
            if baseline_hash.is_some() {
                hidden_paths.insert(file.rel_path.clone());
            }
            self.entries.insert(file.rel_path, entry);
        }

        self.entries.retain(|path, _| seen_paths.contains(path));

        for rel_path in baseline_files.keys() {
            if !seen_paths.contains(rel_path) {
                hidden_paths.insert(rel_path.clone());
            }
        }

        self.hidden_paths = hidden_paths;
        self.dirty_paths.clear();
        Ok(())
    }

    fn refresh_dirty_paths(
        &mut self,
        baseline_files: &HashMap<String, Vec<u8>>,
        workspace_root: &Path,
        embedder: Option<&Embedder>,
        batch_size: usize,
        hash_mode: BaselineHashMode,
    ) -> Result<(), SearchError> {
        let dirty_paths: Vec<String> = self.dirty_paths.drain().map(|(path, _)| path).collect();

        for rel_path in dirty_paths {
            let baseline_hash = baseline_files.get(&rel_path);
            let abs_path = workspace_root.join(&rel_path);

            if !abs_path.exists() {
                self.entries.remove(&rel_path);
                if baseline_hash.is_some() {
                    self.hidden_paths.insert(rel_path);
                } else {
                    self.hidden_paths.remove(&rel_path);
                }
                continue;
            }

            let metadata = match std::fs::metadata(&abs_path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let fingerprint =
                FileFingerprint { len: metadata.len(), modified: metadata.modified().ok() };

            if let Some(entry) = self.entries.get_mut(&rel_path) {
                if entry.fingerprint == fingerprint {
                    if baseline_hash.is_some_and(|stored_hash| stored_hash == &entry.file_hash) {
                        self.entries.remove(&rel_path);
                        self.hidden_paths.remove(&rel_path);
                    } else {
                        if baseline_hash.is_some() {
                            self.hidden_paths.insert(rel_path.clone());
                        } else {
                            self.hidden_paths.remove(&rel_path);
                        }
                        if entry.vector_documents.is_empty() {
                            // ReuseOnly passes `embedder = None`: this attaches any cached
                            // vectors and leaves the rest lexical-only. Embed (warmup) fills
                            // the gaps via the remote embedder.
                            entry.vector_documents = build_overlay_vectors(
                                embedder,
                                batch_size,
                                &entry.lexical_documents,
                                &entry.embedding_inputs,
                                &mut self.embedding_cache,
                            )?;
                        }
                    }
                    continue;
                }
            }

            let content = match std::fs::read_to_string(&abs_path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let file_hash = compute_file_hash(&content, hash_mode);
            if baseline_hash.is_some_and(|stored_hash| stored_hash == &file_hash) {
                self.entries.remove(&rel_path);
                self.hidden_paths.remove(&rel_path);
                continue;
            }

            let provider = self.graph_context_provider.clone();
            let entry = build_overlay_entry(
                &rel_path,
                &content,
                fingerprint,
                file_hash,
                embedder,
                batch_size,
                &mut self.embedding_cache,
                provider.as_deref(),
            )?;

            if baseline_hash.is_some() {
                self.hidden_paths.insert(rel_path.clone());
            } else {
                self.hidden_paths.remove(&rel_path);
            }
            self.entries.insert(rel_path, entry);
        }

        Ok(())
    }

    fn full_refresh_from_manifest(
        &mut self,
        manifest_fingerprints: &HashMap<String, String>,
        workspace_root: &Path,
        embedder: Option<&Embedder>,
        batch_size: usize,
        store: &Store,
    ) -> Result<(), SearchError> {
        let manifest_snapshot_id = store
            .load_baseline_manifest()
            .ok()
            .flatten()
            .map(|r| r.snapshot_id)
            .unwrap_or_default();
        let persisted = store
            .load_overlay_fingerprint_cache(&manifest_snapshot_id)
            .unwrap_or(None)
            .unwrap_or_default();

        if self.embedding_cache.is_empty() {
            if let Some(embedder) = embedder {
                let model_id = embedder.model();
                let dim = embedder.dim();
                match store.load_overlay_embedding_cache(model_id, dim) {
                    Ok(cached) if !cached.is_empty() => {
                        tracing::info!(
                            model_id,
                            dim,
                            cached_embeddings = cached.len(),
                            "loaded persisted overlay embedding cache"
                        );
                        self.embedding_cache = cached;
                    }
                    _ => {}
                }
            }
        }

        let workspace_files = scan_workspace_files(workspace_root);
        let mut seen_paths = HashSet::new();
        let mut hidden_paths = HashSet::new();
        let mut updated_persisted = HashMap::new();

        for file in &workspace_files {
            seen_paths.insert(file.rel_path.clone());
            let baseline_fingerprint = manifest_fingerprints.get(&file.rel_path);

            let mut should_remove_cached_entry = false;
            if let Some(entry) = self.entries.get_mut(&file.rel_path) {
                if entry.fingerprint == file.fingerprint {
                    let local_fp =
                        fingerprint_overlay_documents(&entry.lexical_documents, &file.rel_path);
                    if baseline_fingerprint.is_some_and(|stored| stored == &local_fp) {
                        should_remove_cached_entry = true;
                    } else {
                        if baseline_fingerprint.is_some() {
                            hidden_paths.insert(file.rel_path.clone());
                        }
                        if entry.vector_documents.is_empty() {
                            // ReuseOnly passes `embedder = None`: this attaches any cached
                            // vectors and leaves the rest lexical-only. Embed (warmup) fills
                            // the gaps via the remote embedder.
                            entry.vector_documents = build_overlay_vectors(
                                embedder,
                                batch_size,
                                &entry.lexical_documents,
                                &entry.embedding_inputs,
                                &mut self.embedding_cache,
                            )?;
                        }
                        continue;
                    }
                }
            }
            if should_remove_cached_entry {
                self.entries.remove(&file.rel_path);
                continue;
            }

            if let Some(cached) = persisted.get(&file.rel_path) {
                if cached.file_size == file.fingerprint.len
                    && fingerprint_mtime_matches(file.fingerprint.modified, cached)
                {
                    updated_persisted.insert(file.rel_path.clone(), cached.clone());

                    if baseline_fingerprint
                        .is_some_and(|stored| stored == &cached.content_fingerprint)
                    {
                        self.entries.remove(&file.rel_path);
                        continue;
                    }
                }
            }

            let content = match std::fs::read_to_string(&file.abs_path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let file_hash = normalized_file_hash_for_content(&content);
            let local_fp = fingerprint_content(&content, &file.rel_path);

            if let Some((secs, nanos)) = mtime_to_secs_nanos(file.fingerprint.modified) {
                updated_persisted.insert(
                    file.rel_path.clone(),
                    crate::store::PersistedFingerprint {
                        file_size: file.fingerprint.len,
                        file_mtime_secs: secs,
                        file_mtime_nanos: nanos,
                        content_fingerprint: local_fp.clone(),
                    },
                );
            }

            if baseline_fingerprint.is_some_and(|stored| stored == &local_fp) {
                self.entries.remove(&file.rel_path);
                continue;
            }

            let provider = self.graph_context_provider.clone();
            let entry = build_overlay_entry(
                &file.rel_path,
                &content,
                file.fingerprint.clone(),
                file_hash,
                embedder,
                batch_size,
                &mut self.embedding_cache,
                provider.as_deref(),
            )?;
            if baseline_fingerprint.is_some() {
                hidden_paths.insert(file.rel_path.clone());
            }
            self.entries.insert(file.rel_path.clone(), entry);
        }

        self.entries.retain(|path, _| seen_paths.contains(path));

        for rel_path in manifest_fingerprints.keys() {
            if !seen_paths.contains(rel_path) {
                hidden_paths.insert(rel_path.clone());
            }
        }

        self.hidden_paths = hidden_paths;
        self.dirty_paths.clear();

        if !updated_persisted.is_empty() {
            if let Err(error) =
                store.save_overlay_fingerprint_cache(&manifest_snapshot_id, &updated_persisted)
            {
                tracing::warn!("failed to persist overlay fingerprint cache: {error}");
            }
        }

        if let Some(embedder) = embedder {
            if !self.embedding_cache.is_empty() {
                if let Err(error) = store.save_overlay_embedding_cache(
                    embedder.model(),
                    embedder.dim(),
                    &self.embedding_cache,
                ) {
                    tracing::warn!("failed to persist overlay embedding cache: {error}");
                }
            }
        }

        Ok(())
    }

    /// Phase A: plan a manifest-driven full refresh without holding any live lock.
    ///
    /// Reads workspace files and the persisted overlay caches through `store` (a standalone
    /// connection) and the supplied read-only `warm_embeddings` clone, decides which files belong
    /// in the overlay, builds their lexical docs and embedding inputs, and collects the distinct
    /// `content_hash -> input` pairs that lack a warm vector. Mutates nothing shared: the result
    /// is a [`RefreshPlan`] applied later by [`Self::publish_plan`].
    pub fn plan_full_refresh_from_manifest(
        manifest_fingerprints: &HashMap<String, String>,
        workspace_root: &Path,
        store: &Store,
        warm_embeddings: &HashMap<String, Vec<f32>>,
        graph_context: Option<&dyn GraphContextProvider>,
    ) -> Result<RefreshPlan, SearchError> {
        let snapshot_id = store
            .load_baseline_manifest()
            .ok()
            .flatten()
            .map(|r| r.snapshot_id)
            .unwrap_or_default();
        let persisted =
            store.load_overlay_fingerprint_cache(&snapshot_id).unwrap_or(None).unwrap_or_default();

        let workspace_files = scan_workspace_files(workspace_root);
        let mut seen_paths = HashSet::new();
        let mut hidden_paths = HashSet::new();
        let mut updated_persisted = HashMap::new();
        let mut entries: Vec<(String, PlannedEntry)> = Vec::new();
        let mut missing_embeddings: HashMap<String, String> = HashMap::new();

        for file in &workspace_files {
            seen_paths.insert(file.rel_path.clone());
            let baseline_fingerprint = manifest_fingerprints.get(&file.rel_path);

            if let Some(cached) = persisted.get(&file.rel_path) {
                if cached.file_size == file.fingerprint.len
                    && fingerprint_mtime_matches(file.fingerprint.modified, cached)
                {
                    updated_persisted.insert(file.rel_path.clone(), cached.clone());

                    if baseline_fingerprint
                        .is_some_and(|stored| stored == &cached.content_fingerprint)
                    {
                        continue;
                    }
                }
            }

            let content = match std::fs::read_to_string(&file.abs_path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let file_hash = normalized_file_hash_for_content(&content);
            let local_fp = fingerprint_content(&content, &file.rel_path);

            if let Some((secs, nanos)) = mtime_to_secs_nanos(file.fingerprint.modified) {
                updated_persisted.insert(
                    file.rel_path.clone(),
                    crate::store::PersistedFingerprint {
                        file_size: file.fingerprint.len,
                        file_mtime_secs: secs,
                        file_mtime_nanos: nanos,
                        content_fingerprint: local_fp.clone(),
                    },
                );
            }

            if baseline_fingerprint.is_some_and(|stored| stored == &local_fp) {
                continue;
            }

            let (lexical_documents, embedding_inputs) =
                build_overlay_documents(&file.rel_path, &content, graph_context);
            for input in &embedding_inputs {
                let key = overlay_embedding_key(input);
                if !warm_embeddings.contains_key(&key) {
                    missing_embeddings.entry(key).or_insert_with(|| input.clone());
                }
            }

            if baseline_fingerprint.is_some() {
                hidden_paths.insert(file.rel_path.clone());
            }
            entries.push((
                file.rel_path.clone(),
                PlannedEntry {
                    fingerprint: file.fingerprint.clone(),
                    file_hash,
                    lexical_documents,
                    embedding_inputs,
                },
            ));
        }

        for rel_path in manifest_fingerprints.keys() {
            if !seen_paths.contains(rel_path) {
                hidden_paths.insert(rel_path.clone());
            }
        }

        Ok(RefreshPlan {
            snapshot_id,
            entries,
            hidden_paths,
            updated_persisted,
            missing_embeddings,
        })
    }

    /// Phase C: apply a [`RefreshPlan`] atomically under the inner cache lock.
    ///
    /// Merges `new_embeddings` (Phase B output) into `embedding_cache`, assembles each planned
    /// entry's vectors from the merged cache, swaps `entries`/`hidden_paths` in one shot (so a
    /// concurrent reader never sees a half-embedded file), then persists the fingerprint and
    /// embedding caches once. The merge is last-writer-wins on the embedding key, which is value
    /// stable because identical embedding input yields an identical embedding.
    pub fn publish_plan(
        &mut self,
        plan: RefreshPlan,
        new_embeddings: HashMap<String, Vec<f32>>,
        dirty_before: &HashMap<String, u64>,
        embedder: Option<&Embedder>,
        store: &Store,
    ) -> Result<(), SearchError> {
        for (embedding_key, embedding) in new_embeddings {
            self.embedding_cache.insert(embedding_key, embedding);
        }

        let mut entries = HashMap::with_capacity(plan.entries.len());
        for (rel_path, planned) in plan.entries {
            // No embedder is passed, so vectors come purely from the merged cache and the batch
            // size is unused; `1` keeps the chunking math well-defined.
            let vector_documents = build_overlay_vectors(
                None,
                1,
                &planned.lexical_documents,
                &planned.embedding_inputs,
                &mut self.embedding_cache,
            )?;
            entries.insert(
                rel_path,
                OverlayFileEntry {
                    fingerprint: planned.fingerprint,
                    file_hash: planned.file_hash,
                    lexical_documents: planned.lexical_documents,
                    vector_documents,
                    embedding_inputs: planned.embedding_inputs,
                },
            );
        }

        self.entries = entries;
        self.hidden_paths = plan.hidden_paths;
        // Clear only the dirty flags this full refresh superseded. A path is superseded only if it
        // has not been re-marked since the snapshot (same sequence): a watcher edit that landed
        // during the lock-free embed bumps the sequence (or adds a new path), so it survives and a
        // later refresh re-embeds it. A blanket clear would silently drop an edit made mid-pass.
        for (rel_path, seq) in dirty_before {
            if self.dirty_paths.get(rel_path) == Some(seq) {
                self.dirty_paths.remove(rel_path);
            }
        }
        self.initialized = true;

        if !plan.updated_persisted.is_empty() {
            if let Err(error) =
                store.save_overlay_fingerprint_cache(&plan.snapshot_id, &plan.updated_persisted)
            {
                tracing::warn!("failed to persist overlay fingerprint cache: {error}");
            }
        }

        if let Some(embedder) = embedder {
            if !self.embedding_cache.is_empty() {
                if let Err(error) = store.save_overlay_embedding_cache(
                    embedder.model(),
                    embedder.dim(),
                    &self.embedding_cache,
                ) {
                    tracing::warn!("failed to persist overlay embedding cache: {error}");
                }
            }
        }

        Ok(())
    }

    /// A read-only clone of the embedding cache for the warmup's lock-free Phase B start.
    pub fn embedding_cache_snapshot(&self) -> HashMap<String, Vec<f32>> {
        self.embedding_cache.clone()
    }

    /// The dirty-path set (path -> mark sequence) as of this call. Captured before a lock-free embed
    /// pass so [`publish_plan`] can clear exactly the flags that pass supersedes, leaving any
    /// re-marked mid-pass intact (their sequence will have advanced).
    pub fn dirty_paths_snapshot(&self) -> HashMap<String, u64> {
        self.dirty_paths.clone()
    }

    fn refresh_dirty_paths_from_manifest(
        &mut self,
        manifest_fingerprints: &HashMap<String, String>,
        workspace_root: &Path,
        embedder: Option<&Embedder>,
        batch_size: usize,
    ) -> Result<(), SearchError> {
        let dirty_paths: Vec<String> = self.dirty_paths.drain().map(|(path, _)| path).collect();

        for rel_path in dirty_paths {
            let baseline_fingerprint = manifest_fingerprints.get(&rel_path);
            let abs_path = workspace_root.join(&rel_path);

            if !abs_path.exists() {
                self.entries.remove(&rel_path);
                if baseline_fingerprint.is_some() {
                    self.hidden_paths.insert(rel_path);
                } else {
                    self.hidden_paths.remove(&rel_path);
                }
                continue;
            }

            let metadata = match std::fs::metadata(&abs_path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let fingerprint =
                FileFingerprint { len: metadata.len(), modified: metadata.modified().ok() };

            if let Some(entry) = self.entries.get_mut(&rel_path) {
                if entry.fingerprint == fingerprint {
                    let local_fp =
                        fingerprint_overlay_documents(&entry.lexical_documents, &rel_path);
                    if baseline_fingerprint.is_some_and(|stored| stored == &local_fp) {
                        self.entries.remove(&rel_path);
                        self.hidden_paths.remove(&rel_path);
                    } else {
                        if baseline_fingerprint.is_some() {
                            self.hidden_paths.insert(rel_path.clone());
                        } else {
                            self.hidden_paths.remove(&rel_path);
                        }
                        if entry.vector_documents.is_empty() {
                            // ReuseOnly passes `embedder = None`: this attaches any cached
                            // vectors and leaves the rest lexical-only. Embed (warmup) fills
                            // the gaps via the remote embedder.
                            entry.vector_documents = build_overlay_vectors(
                                embedder,
                                batch_size,
                                &entry.lexical_documents,
                                &entry.embedding_inputs,
                                &mut self.embedding_cache,
                            )?;
                        }
                    }
                    continue;
                }
            }

            let content = match std::fs::read_to_string(&abs_path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let file_hash = normalized_file_hash_for_content(&content);
            let local_fp = fingerprint_content(&content, &rel_path);
            if baseline_fingerprint.is_some_and(|stored| stored == &local_fp) {
                self.entries.remove(&rel_path);
                self.hidden_paths.remove(&rel_path);
                continue;
            }

            let provider = self.graph_context_provider.clone();
            let entry = build_overlay_entry(
                &rel_path,
                &content,
                fingerprint,
                file_hash,
                embedder,
                batch_size,
                &mut self.embedding_cache,
                provider.as_deref(),
            )?;

            if baseline_fingerprint.is_some() {
                self.hidden_paths.insert(rel_path.clone());
            } else {
                self.hidden_paths.remove(&rel_path);
            }
            self.entries.insert(rel_path, entry);
        }

        Ok(())
    }

    pub fn snapshot(&self) -> WorkspaceOverlayIndex {
        let baseline =
            BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "local-workspace-baseline");
        let mut overlay = SearchOverlay::new(baseline);
        let mut lexical_documents = Vec::new();
        let mut vector_documents = Vec::new();

        let mut entry_paths: Vec<&String> = self.entries.keys().collect();
        entry_paths.sort();
        for rel_path in entry_paths {
            let entry = self.entries.get(rel_path).expect("path collected from map keys");
            overlay.replace_file(
                DocumentPath::new("code", rel_path.clone()),
                entry.lexical_documents.clone(),
            );
            lexical_documents.extend(entry.lexical_documents.clone());
            vector_documents.extend(entry.vector_documents.clone());
        }

        let mut deleted_paths: Vec<&String> =
            self.hidden_paths.iter().filter(|path| !self.entries.contains_key(*path)).collect();
        deleted_paths.sort();
        for rel_path in deleted_paths {
            overlay.delete_file(DocumentPath::new("code", rel_path.clone()));
        }

        WorkspaceOverlayIndex {
            overlay,
            hidden_paths: self.hidden_paths.clone(),
            lexical_documents,
            vector_documents,
        }
    }

    pub fn stats(&self) -> WorkspaceOverlayStats {
        let overlay_files = self.entries.len();
        let hidden_paths = self.hidden_paths.len();
        let deleted_files =
            self.hidden_paths.iter().filter(|path| !self.entries.contains_key(*path)).count();
        let lexical_chunks =
            self.entries.values().map(|entry| entry.lexical_documents.len()).sum::<usize>();
        let semantic_chunks =
            self.entries.values().map(|entry| entry.vector_documents.len()).sum::<usize>();

        WorkspaceOverlayStats {
            overlay_files,
            deleted_files,
            hidden_paths,
            lexical_chunks,
            semantic_chunks,
            cached_embeddings: self.embedding_cache.len(),
            watcher_mode: self.watcher_mode,
            pending_dirty_paths: self.dirty_paths.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

fn fingerprint_mtime_matches(
    mtime: Option<SystemTime>,
    cached: &crate::store::PersistedFingerprint,
) -> bool {
    let Some((secs, nanos)) = mtime_to_secs_nanos(mtime) else {
        return false;
    };
    secs == cached.file_mtime_secs && nanos == cached.file_mtime_nanos
}

fn mtime_to_secs_nanos(mtime: Option<SystemTime>) -> Option<(i64, u32)> {
    let duration = mtime?.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some((duration.as_secs() as i64, duration.subsec_nanos()))
}

#[derive(Debug, Clone)]
struct OverlayFileEntry {
    fingerprint: FileFingerprint,
    file_hash: Vec<u8>,
    lexical_documents: Vec<IndexedDocument>,
    vector_documents: Vec<OverlayVectorDocument>,
    embedding_inputs: Vec<String>,
}

#[derive(Debug, Clone)]
struct WorkspaceFileState {
    rel_path: String,
    abs_path: PathBuf,
    fingerprint: FileFingerprint,
}

fn scan_workspace_files(workspace_root: &Path) -> Vec<WorkspaceFileState> {
    walkdir::WalkDir::new(workspace_root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")))
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            let rel_path = entry
                .path()
                .strip_prefix(workspace_root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .to_string();
            Some(WorkspaceFileState {
                rel_path,
                abs_path: entry.into_path(),
                fingerprint: FileFingerprint {
                    len: metadata.len(),
                    modified: metadata.modified().ok(),
                },
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_overlay_entry(
    rel_path: &str,
    content: &str,
    fingerprint: FileFingerprint,
    file_hash: Vec<u8>,
    embedder: Option<&Embedder>,
    batch_size: usize,
    embedding_cache: &mut HashMap<String, Vec<f32>>,
    graph_context: Option<&dyn GraphContextProvider>,
) -> Result<OverlayFileEntry, SearchError> {
    let (lexical_documents, embedding_inputs) =
        build_overlay_documents(rel_path, content, graph_context);
    let vector_documents = build_overlay_vectors(
        embedder,
        batch_size,
        &lexical_documents,
        &embedding_inputs,
        embedding_cache,
    )?;

    Ok(OverlayFileEntry {
        fingerprint,
        file_hash,
        lexical_documents,
        vector_documents,
        embedding_inputs,
    })
}

fn compute_file_hash(content: &str, hash_mode: BaselineHashMode) -> Vec<u8> {
    match hash_mode {
        BaselineHashMode::RawFileBytes => blake3::hash(content.as_bytes()).as_bytes().to_vec(),
        BaselineHashMode::NormalizedChunks => normalized_file_hash_for_content(content),
    }
}

pub(crate) fn normalized_file_hash_for_content(content: &str) -> Vec<u8> {
    let chunks = Chunker::chunk(content);
    normalized_file_hash_for_chunks(chunks.iter().map(|chunk| {
        (
            chunk.kind.label(),
            chunk.name.as_str(),
            chunk.line_start,
            chunk.line_end,
            chunk.text.as_str(),
        )
    }))
}

pub(crate) fn normalized_file_hash_for_indexed_documents(documents: &[IndexedDocument]) -> Vec<u8> {
    normalized_file_hash_for_chunks(documents.iter().map(|document| {
        (
            document.kind.as_str(),
            document.symbol_name.as_str(),
            document.line_start,
            document.line_end,
            document.text.as_str(),
        )
    }))
}

fn normalized_file_hash_for_chunks<'a>(
    chunks: impl Iterator<Item = (&'a str, &'a str, u32, u32, &'a str)>,
) -> Vec<u8> {
    let mut hasher = blake3::Hasher::new();
    for (kind, name, line_start, line_end, text) in chunks {
        hasher.update(kind.as_bytes());
        hasher.update(&[0]);
        hasher.update(name.as_bytes());
        hasher.update(&[0]);
        hasher.update(&line_start.to_le_bytes());
        hasher.update(&line_end.to_le_bytes());
        hasher.update(text.as_bytes());
        hasher.update(&[0xff]);
    }
    hasher.finalize().as_bytes().to_vec()
}

pub(crate) fn fingerprint_content(content: &str, rel_path: &str) -> String {
    let documents = Chunker::chunk(content);
    let mut hasher = blake3::Hasher::new();
    for chunk in &documents {
        let kind = match chunk.kind {
            code_chunk::ChunkKind::ModuleHeader => "header",
            code_chunk::ChunkKind::Procedure => "procedure",
            code_chunk::ChunkKind::Function => "function",
        };
        let content_hash = blake3::hash(chunk.text.as_bytes()).to_hex().to_string();
        hasher.update("code".as_bytes());
        hasher.update(&[0]);
        hasher.update(rel_path.as_bytes());
        hasher.update(&[0]);
        hasher.update(chunk.name.as_bytes());
        hasher.update(&[0]);
        hasher.update(kind.as_bytes());
        hasher.update(&chunk.line_start.to_le_bytes());
        hasher.update(&chunk.line_end.to_le_bytes());
        hasher.update(content_hash.as_bytes());
        hasher.update(&[0]);
        hasher.update(chunk.text.as_bytes());
        hasher.update(&[0xff]);
    }
    hasher.finalize().to_hex().to_string()
}

pub(crate) fn fingerprint_overlay_documents(
    documents: &[IndexedDocument],
    rel_path: &str,
) -> String {
    let mut hasher = blake3::Hasher::new();
    for document in documents {
        hasher.update(document.collection.as_bytes());
        hasher.update(&[0]);
        hasher.update(rel_path.as_bytes());
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

/// The overlay embedding cache key: the blake3 hash of the exact text that gets embedded. This is
/// the same value as [`crate::document::semantic_key_for_indexed_document`] computed from the
/// document's embedding input, so the overlay reuses vectors by the same identity the baseline and
/// the main chunk index use, rather than by raw chunk text.
fn overlay_embedding_key(embedding_input: &str) -> String {
    blake3::hash(embedding_input.as_bytes()).to_hex().to_string()
}

fn build_overlay_documents(
    rel_path: &str,
    content: &str,
    graph_context: Option<&dyn GraphContextProvider>,
) -> (Vec<IndexedDocument>, Vec<String>) {
    let chunks = Chunker::chunk(content);
    let mut lexical_documents = Vec::with_capacity(chunks.len());
    let mut embedding_inputs = Vec::with_capacity(chunks.len());

    for chunk in &chunks {
        let document = crate::document::indexed_document_for_chunk(rel_path, chunk, graph_context);
        embedding_inputs.push(crate::document::semantic_text_for_indexed_document(&document));
        lexical_documents.push(document);
    }

    (lexical_documents, embedding_inputs)
}

/// Build overlay vectors for a file's chunks.
///
/// In [`RefreshMode::ReuseOnly`] (the interactive query path) `embedder` is `None`: only cached
/// vectors are attached and chunks without one are dropped from the vector set (they remain
/// lexical). In [`RefreshMode::Embed`] (the background warmup) `embedder` is `Some` and missing
/// vectors are embedded inline. Newly embedded vectors are written back to `embedding_cache`.
fn build_overlay_vectors(
    embedder: Option<&Embedder>,
    batch_size: usize,
    documents: &[IndexedDocument],
    embedding_inputs: &[String],
    embedding_cache: &mut HashMap<String, Vec<f32>>,
) -> Result<Vec<OverlayVectorDocument>, SearchError> {
    if documents.is_empty() {
        return Ok(Vec::new());
    }

    let mut vectors: Vec<Option<Vec<f32>>> = vec![None; documents.len()];
    let mut missing_indexes = Vec::new();
    let mut missing_inputs = Vec::new();

    // Key the embedding cache by the hash of the exact text that is embedded (the semantic
    // embedding input), not by the raw chunk-text `content_hash`. Two chunks with identical bodies
    // but different module / symbol / kind / graph context produce different embedding inputs, so
    // they must map to different vectors; keying by `content_hash` would collapse them onto one
    // (and serve a stale vector when only the graph context changed).
    for (idx, _document) in documents.iter().enumerate() {
        let key = overlay_embedding_key(&embedding_inputs[idx]);
        if let Some(embedding) = embedding_cache.get(&key) {
            vectors[idx] = Some(embedding.clone());
        } else {
            missing_indexes.push(idx);
            missing_inputs.push(embedding_inputs[idx].as_str());
        }
    }

    if let Some(embedder) = embedder {
        for (batch_indexes, batch_inputs) in
            missing_indexes.chunks(batch_size.max(1)).zip(missing_inputs.chunks(batch_size.max(1)))
        {
            // The background warmup runs this off any lock, so it can afford the interactive
            // embed; the hot interactive query path never reaches here (it passes `None`).
            let embeddings = embedder.embed_batch_interactive(batch_inputs)?;
            for (idx, embedding) in batch_indexes.iter().copied().zip(embeddings) {
                let key = overlay_embedding_key(&embedding_inputs[idx]);
                embedding_cache.insert(key, embedding.clone());
                vectors[idx] = Some(embedding);
            }
        }
    }

    // ReuseOnly (or a missing inline embed) leaves a chunk without a vector. Such chunks are not
    // emitted as vector documents so they never appear as zero-similarity hits; they still serve
    // lexically through `lexical_documents`.
    Ok(documents
        .iter()
        .cloned()
        .zip(vectors)
        .filter_map(|(document, embedding)| {
            embedding.map(|embedding| OverlayVectorDocument { document, embedding })
        })
        .collect())
}

pub fn lexical_hits(
    overlay: &WorkspaceOverlayIndex,
    query: &str,
    limit: usize,
) -> Vec<crate::engine::SearchHit> {
    lexical_hits_for_documents(overlay.lexical_documents.iter(), query, limit)
}

pub fn semantic_hits(
    overlay: &WorkspaceOverlayIndex,
    query_embedding: &[f32],
    limit: usize,
) -> Vec<crate::engine::SearchHit> {
    let mut hits: Vec<crate::engine::SearchHit> = overlay
        .vector_documents
        .iter()
        .map(|document| crate::engine::SearchHit {
            collection: document.document.collection.clone(),
            file_path: document.document.path.clone(),
            symbol_name: document.document.symbol_name.clone(),
            kind: document.document.kind.clone(),
            text: document.document.text.clone(),
            line_start: document.document.line_start,
            line_end: document.document.line_end,
            score: cosine_similarity(query_embedding, &document.embedding),
        })
        .collect();

    hits.sort_by(|lhs, rhs| rhs.score.total_cmp(&lhs.score));
    hits.truncate(limit);
    hits
}

fn cosine_similarity(lhs: &[f32], rhs: &[f32]) -> f32 {
    if lhs.len() != rhs.len() || lhs.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut lhs_norm = 0.0f32;
    let mut rhs_norm = 0.0f32;

    for (&left, &right) in lhs.iter().zip(rhs.iter()) {
        dot += left * right;
        lhs_norm += left * left;
        rhs_norm += right * right;
    }

    let denom = lhs_norm.sqrt() * rhs_norm.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

#[cfg(test)]
mod tests {
    use super::{
        fingerprint_content, lexical_hits, BaselineHashMode, WorkspaceOverlayCache,
        WorkspaceOverlayStats,
    };
    use crate::store::Store;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn reuse_only_refresh_attaches_no_vectors_when_cache_is_empty() {
        // A refresh with `embedder = None` is the interactive ReuseOnly path: a changed file is
        // lexically searchable immediately, but with nothing cached it gets NO semantic vector
        // this turn (and crucially never calls an embedder). The background warmup is what fills
        // vectors later.
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура ТолькоЛексика()\nКонецПроцедуры").unwrap();

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut manifest = HashMap::new();
        manifest.insert("A.bsl".to_owned(), "different-fingerprint".to_owned());

        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store, true).unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 1);
        assert!(overlay.vector_documents.is_empty(), "ReuseOnly must not embed overlay chunks");
        let hits = lexical_hits(&overlay, "ТолькоЛексика", 10);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn plan_and_publish_attach_only_embedded_vectors() {
        // Phase A plans the refresh and reports the chunk that needs embedding; Phase C publishes
        // a (test-supplied) embedding for it and the snapshot then carries exactly one vector.
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура НужноВложить()\nКонецПроцедуры").unwrap();

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut manifest = HashMap::new();
        manifest.insert("A.bsl".to_owned(), "different-fingerprint".to_owned());

        let warm = HashMap::new();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest, workspace, &store, &warm, None,
        )
        .unwrap();

        let missing = plan.missing_embeddings();
        assert_eq!(missing.len(), 1, "the one changed chunk needs embedding");
        let embedding_key = missing.keys().next().unwrap().clone();

        let mut new_embeddings = HashMap::new();
        new_embeddings.insert(embedding_key, vec![0.1_f32, 0.2, 0.3]);

        let mut cache = WorkspaceOverlayCache::default();
        cache.publish_plan(plan, new_embeddings, &HashMap::new(), None, &store).unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 1);
        assert_eq!(overlay.vector_documents.len(), 1, "the embedded chunk now has a vector");
        assert_eq!(overlay.vector_documents[0].embedding, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn chunks_with_same_text_but_different_module_get_distinct_vectors() {
        // Two files at different module paths hold a byte-identical procedure body. Their raw chunk
        // text (and thus the legacy `content_hash`) is the same, but the embedded text differs (it
        // folds in the module path), so the overlay must key the embedding cache by the embedding
        // input. Keying by raw-text identity would collapse them onto one shared vector.
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let body = "Процедура Делать()\nКонецПроцедуры";
        let dir_a = workspace.join("CommonModules").join("МодульА").join("Ext");
        let dir_b = workspace.join("CommonModules").join("МодульБ").join("Ext");
        fs::create_dir_all(&dir_a).unwrap();
        fs::create_dir_all(&dir_b).unwrap();
        fs::write(dir_a.join("Module.bsl"), body).unwrap();
        fs::write(dir_b.join("Module.bsl"), body).unwrap();

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut manifest = HashMap::new();
        manifest.insert(
            "CommonModules/МодульА/Ext/Module.bsl".to_owned(),
            "different-fingerprint".to_owned(),
        );
        manifest.insert(
            "CommonModules/МодульБ/Ext/Module.bsl".to_owned(),
            "different-fingerprint".to_owned(),
        );

        let warm = HashMap::new();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest, workspace, &store, &warm, None,
        )
        .unwrap();

        // The two chunks share raw text but have distinct embedding inputs, so the plan reports two
        // distinct embedding keys (the bug would report a single collapsed key).
        let missing = plan.missing_embeddings();
        assert_eq!(missing.len(), 2, "same-text chunks in different modules must not collapse");

        // Give each key its own vector; publishing must attach the right vector to each chunk.
        let mut new_embeddings = HashMap::new();
        let mut keys: Vec<String> = missing.keys().cloned().collect();
        keys.sort();
        new_embeddings.insert(keys[0].clone(), vec![1.0_f32, 0.0, 0.0]);
        new_embeddings.insert(keys[1].clone(), vec![0.0_f32, 1.0, 0.0]);

        let mut cache = WorkspaceOverlayCache::default();
        cache.publish_plan(plan, new_embeddings, &HashMap::new(), None, &store).unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.vector_documents.len(), 2, "each chunk keeps its own vector");
        let mut embeddings: Vec<Vec<f32>> =
            overlay.vector_documents.iter().map(|doc| doc.embedding.clone()).collect();
        embeddings.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(embeddings, vec![vec![0.0, 1.0, 0.0], vec![1.0, 0.0, 0.0]]);
    }

    #[test]
    fn publish_clears_only_dirty_paths_superseded_by_the_refresh() {
        // The dirty snapshot is taken before the lock-free embed pass. Publish must clear only
        // those paths; a path the watcher marks DURING the embed window is absent from the
        // snapshot and must survive so a later refresh re-embeds it (a blanket clear would drop it).
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let manifest = HashMap::new();
        let warm = HashMap::new();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest, workspace, &store, &warm, None,
        )
        .unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.mark_dirty_path("before.bsl".to_owned());
        cache.mark_dirty_path("reedited.bsl".to_owned());
        let dirty_before = cache.dirty_paths_snapshot();
        // Watcher activity during the lock-free embed window: a brand-new path, plus a re-edit of a
        // path that was already in the snapshot (its sequence advances).
        cache.mark_dirty_path("during.bsl".to_owned());
        cache.mark_dirty_path("reedited.bsl".to_owned());

        cache.publish_plan(plan, HashMap::new(), &dirty_before, None, &store).unwrap();

        // before.bsl was superseded and cleared; during.bsl (new) and reedited.bsl (re-marked after
        // the snapshot) both survive so a later refresh re-embeds them.
        assert_eq!(cache.stats().pending_dirty_paths, 2);
        let remaining = cache.dirty_paths_snapshot();
        assert!(remaining.contains_key("during.bsl"));
        assert!(remaining.contains_key("reedited.bsl"));
        assert!(!remaining.contains_key("before.bsl"));
    }

    #[test]
    fn overlay_detects_changed_and_deleted_files() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file_a = workspace.join("A.bsl");
        let file_b = workspace.join("B.bsl");
        fs::write(&file_a, "Процедура Старая()\nКонецПроцедуры").unwrap();
        fs::write(&file_b, "Процедура Удаляемая()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("search.db");
        let mut store = Store::open(&db_path).unwrap();
        let chunks_a = crate::Chunker::chunk(&fs::read_to_string(&file_a).unwrap());
        let chunks_b = crate::Chunker::chunk(&fs::read_to_string(&file_b).unwrap());
        let hash_a = blake3::hash(fs::read(&file_a).unwrap().as_slice());
        let hash_b = blake3::hash(fs::read(&file_b).unwrap().as_slice());
        store.reindex_file("A.bsl", hash_a.as_bytes(), &chunks_a, None).unwrap();
        store.reindex_file("B.bsl", hash_b.as_bytes(), &chunks_b, None).unwrap();

        fs::write(&file_a, "Процедура НовоеИмя()\nКонецПроцедуры").unwrap();
        fs::remove_file(&file_b).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        let overlay = cache.snapshot();

        assert!(overlay.hidden_paths.contains("A.bsl"));
        assert!(overlay.hidden_paths.contains("B.bsl"));
        assert_eq!(overlay.lexical_documents.len(), 1);
        assert_eq!(overlay.lexical_documents[0].symbol_name, "НовоеИмя");
    }

    #[test]
    fn lexical_hits_rank_overlay_matches() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура НоваяПроцедура123()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("search.db");
        let store = Store::open(&db_path).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        let overlay = cache.snapshot();

        let hits = lexical_hits(&overlay, "НоваяПроцедура123", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].symbol_name, "НоваяПроцедура123");
    }

    #[test]
    fn refresh_updates_only_changed_overlay_state() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура ВерсияОдин111()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("search.db");
        let store = Store::open(&db_path).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        let first = cache.snapshot();
        assert_eq!(first.lexical_documents[0].symbol_name, "ВерсияОдин111");

        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        let second = cache.snapshot();
        assert_eq!(second.lexical_documents[0].symbol_name, "ВерсияОдин111");

        fs::write(&file, "Процедура ВерсияДва222222()\nКонецПроцедуры").unwrap();
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        let third = cache.snapshot();
        assert_eq!(third.lexical_documents[0].symbol_name, "ВерсияДва222222");
    }

    #[test]
    fn stats_report_overlay_shape() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file_a = workspace.join("A.bsl");
        let file_b = workspace.join("B.bsl");
        fs::write(&file_a, "Процедура Первая()\nКонецПроцедуры").unwrap();
        fs::write(&file_b, "Процедура Вторая()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("search.db");
        let mut store = Store::open(&db_path).unwrap();
        let chunks_a = crate::Chunker::chunk(&fs::read_to_string(&file_a).unwrap());
        let chunks_b = crate::Chunker::chunk(&fs::read_to_string(&file_b).unwrap());
        let hash_a = blake3::hash(fs::read(&file_a).unwrap().as_slice());
        let hash_b = blake3::hash(fs::read(&file_b).unwrap().as_slice());
        store.reindex_file("A.bsl", hash_a.as_bytes(), &chunks_a, None).unwrap();
        store.reindex_file("B.bsl", hash_b.as_bytes(), &chunks_b, None).unwrap();

        fs::write(&file_a, "Процедура Измененная()\nКонецПроцедуры").unwrap();
        fs::remove_file(&file_b).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();

        assert_eq!(
            cache.stats(),
            WorkspaceOverlayStats {
                overlay_files: 1,
                deleted_files: 1,
                hidden_paths: 2,
                lexical_chunks: 1,
                semantic_chunks: 0,
                cached_embeddings: 0,
                watcher_mode: false,
                pending_dirty_paths: 0,
            }
        );
    }

    #[test]
    fn watcher_mode_refreshes_only_marked_paths() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура Базовая()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("search.db");
        let mut store = Store::open(&db_path).unwrap();
        let chunks = crate::Chunker::chunk(&fs::read_to_string(&file).unwrap());
        let hash = blake3::hash(fs::read(&file).unwrap().as_slice());
        store.reindex_file("A.bsl", hash.as_bytes(), &chunks, None).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        assert_eq!(cache.stats().overlay_files, 0);

        fs::write(&file, "Процедура ИзWatcher()\nКонецПроцедуры").unwrap();
        cache.mark_dirty_path("A.bsl");
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 1);
        assert_eq!(overlay.lexical_documents[0].symbol_name, "ИзWatcher");
        assert_eq!(cache.stats().pending_dirty_paths, 0);
        assert!(cache.stats().watcher_mode);
    }

    #[test]
    fn manifest_refresh_treats_all_files_as_new_without_manifest() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура Новая()\nКонецПроцедуры").unwrap();

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        let manifest: HashMap<String, String> = HashMap::new();
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store, true).unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 1);
        assert_eq!(overlay.lexical_documents[0].symbol_name, "Новая");
        assert!(overlay.hidden_paths.is_empty());
    }

    #[test]
    fn manifest_refresh_detects_unchanged_file() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let content = "Процедура Базовая()\nКонецПроцедуры";
        let file = workspace.join("A.bsl");
        fs::write(&file, content).unwrap();

        let fp = fingerprint_content(content, "A.bsl");
        let mut manifest = HashMap::new();
        manifest.insert("A.bsl".to_owned(), fp);

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store, true).unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 0);
        assert!(!overlay.hidden_paths.contains("A.bsl"));
    }

    #[test]
    fn manifest_refresh_detects_modified_file() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура Старая()\nКонецПроцедуры").unwrap();

        let mut manifest = HashMap::new();
        manifest.insert("A.bsl".to_owned(), "different-fingerprint".to_owned());

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store, true).unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 1);
        assert_eq!(overlay.lexical_documents[0].symbol_name, "Старая");
        assert!(overlay.hidden_paths.contains("A.bsl"));
    }

    #[test]
    fn manifest_refresh_detects_deleted_baseline_file() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();

        let mut manifest = HashMap::new();
        manifest.insert("A.bsl".to_owned(), "some-fp".to_owned());
        manifest.insert("B.bsl".to_owned(), "other-fp".to_owned());

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store, true).unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 0);
        assert_eq!(overlay.hidden_paths.len(), 2);
        assert!(overlay.hidden_paths.contains("A.bsl"));
        assert!(overlay.hidden_paths.contains("B.bsl"));
    }

    #[test]
    fn reuse_only_never_cold_scans_an_uninitialized_cache() {
        // A fresh cache (initialized=false, watcher_mode=false) holding files that DIVERGE from the
        // baseline. A `ReuseOnly` (allow_cold_scan=false) refresh must NOT walk the tree: if it did,
        // the divergent file would surface as an overlay entry. So the snapshot stays empty and the
        // cache stays uninitialized — the warmup/watcher is what builds it. The SAME cache with
        // allow_cold_scan=true then DOES scan and populate, proving the gate is the only difference.
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура Локальная()\nКонецПроцедуры").unwrap();

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut manifest = HashMap::new();
        manifest.insert("A.bsl".to_owned(), "different-fingerprint".to_owned());

        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store, false).unwrap();

        let overlay = cache.snapshot();
        assert!(
            overlay.lexical_documents.is_empty(),
            "ReuseOnly over an uninitialized cache must not cold-scan present files"
        );
        assert_eq!(cache.stats().overlay_files, 0);

        // The gate is the only difference: a cold-scan-allowed refresh of the same cache populates.
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store, true).unwrap();
        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 1);
        assert_eq!(overlay.lexical_documents[0].symbol_name, "Локальная");
    }

    #[test]
    fn reuse_only_skips_full_scan_but_applies_dirty_paths_in_polling_mode() {
        // An already-initialized cache in polling mode (watcher_mode=false). A ReuseOnly refresh
        // must NOT re-run the full scan just because it is polling: with no dirty paths the overlay
        // is unchanged, even after a new on-disk file appears that a cold scan would have picked up.
        // A marked dirty path IS still applied (the cheap incremental arm).
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file_a = workspace.join("A.bsl");
        fs::write(&file_a, "Процедура ИзменённаяА()\nКонецПроцедуры").unwrap();

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut manifest = HashMap::new();
        manifest.insert("A.bsl".to_owned(), "different-fingerprint".to_owned());
        manifest.insert("B.bsl".to_owned(), "different-fingerprint".to_owned());

        // Populate the cache once via the cold-scan path so it is initialized.
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store, true).unwrap();
        assert_eq!(cache.snapshot().lexical_documents.len(), 1, "A.bsl is the only overlay entry");
        assert!(!cache.stats().watcher_mode, "polling mode for this scenario");

        // A new baseline-divergent file appears on disk. A ReuseOnly refresh with NO dirty paths
        // must leave the overlay untouched (no full rescan) — B.bsl stays absent.
        let file_b = workspace.join("B.bsl");
        fs::write(&file_b, "Процедура НоваяБ()\nКонецПроцедуры").unwrap();
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store, false).unwrap();
        let overlay = cache.snapshot();
        assert_eq!(
            overlay.lexical_documents.len(),
            1,
            "polling ReuseOnly must not re-scan the tree"
        );
        assert_eq!(overlay.lexical_documents[0].symbol_name, "ИзменённаяА");

        // A marked dirty path IS still picked up by the cheap incremental arm.
        cache.mark_dirty_path("B.bsl");
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store, false).unwrap();
        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 2, "the dirty path is applied incrementally");
        let mut names: Vec<String> =
            overlay.lexical_documents.iter().map(|doc| doc.symbol_name.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["ИзменённаяА".to_owned(), "НоваяБ".to_owned()]);
    }
}
