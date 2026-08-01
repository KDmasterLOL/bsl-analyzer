use crate::domain::{BaselineRef, CorpusId, DocumentPath, IndexedDocument, SearchOverlay};
use crate::embedder::Embedder;
use crate::error::SearchError;
use crate::lexical::lexical_hits_for_documents;
use crate::ports::{GraphContextProvider, ModuleSnapshot};
use crate::store::Store;
use crate::workspace_roots::{FileKey, WorkspaceRoots};
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
    pub hidden_paths: HashSet<FileKey>,
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
    entries: Vec<(FileKey, PlannedEntry)>,
    hidden_paths: HashSet<FileKey>,
    updated_persisted: HashMap<FileKey, crate::store::PersistedFingerprint>,
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
    entries: HashMap<FileKey, OverlayFileEntry>,
    hidden_paths: HashSet<FileKey>,
    embedding_cache: HashMap<String, Vec<f32>>,
    /// Watcher-marked paths awaiting re-embed, each tagged with the sequence at which it was last
    /// marked. The sequence lets [`publish_plan`] tell a path superseded by its refresh from one
    /// the watcher re-marked while the lock-free embed was in flight (same path, newer sequence).
    dirty_paths: HashMap<FileKey, u64>,
    dirty_seq: u64,
    /// Consecutive refresh-failure count per retained dirty path. A path whose stat/read fails is
    /// re-marked dirty (so the next refresh retries) with its count bumped; after
    /// [`MAX_DIRTY_REFRESH_FAILURES`] it is dropped from the dirty set with a warning rather than
    /// retried forever. A fresh [`Self::mark_dirty_path`] (a new watcher event) clears the count,
    /// and a successful refresh drops the entry, so the count is strictly consecutive.
    dirty_failures: HashMap<FileKey, u32>,
    watcher_mode: bool,
    initialized: bool,
    /// How many overlay entries have been (re)built from a resident-provided shared parse, rather
    /// than a self-parsed disk read. A cumulative observability counter — proves the resident-fed
    /// path actually fires — reset only by [`Self::clear`].
    resident_fed_count: usize,
    /// Optional graph-context provider (dependency-inverted). When set, overlay
    /// (uncommitted-edit) chunks are enriched with their call-graph context before
    /// embedding, matching the local index.
    graph_context_provider: Option<Arc<dyn GraphContextProvider>>,
}

/// The stored raw-bytes baseline for a dirty-path refresh: per-path stored hashes plus the recipe
/// to recompute a file's hash. Bundled so [`WorkspaceOverlayCache::refresh_dirty_paths`] stays
/// within the argument-count lint.
struct RawBaseline<'a> {
    files: &'a HashMap<FileKey, Vec<u8>>,
    hash_mode: BaselineHashMode,
}

/// The baseline a snapshot-fed dirty reindex resolves through the store before it touches the dirty
/// set. Owning the loaded value (rather than dispatching inline) lets the fallible store reads run
/// FIRST, so a store error propagates with every dirty flag still intact.
enum DirtyBaseline {
    Manifest(HashMap<FileKey, String>),
    Raw(HashMap<FileKey, Vec<u8>>),
}

/// Consecutive stat/read failures tolerated for a retained dirty path before it is dropped from
/// the dirty set (with a warning). Bounds the per-query retry of a permanently-unreadable path
/// (a deleted file, a path shaped like a `.bsl` that is really a directory) to a fixed budget;
/// strictly better than the pre-S2 behaviour, which silently dropped a path on its FIRST failure.
/// A later watcher event for the same path re-marks it fresh and resets the count.
const MAX_DIRTY_REFRESH_FAILURES: u32 = 3;

impl std::fmt::Debug for WorkspaceOverlayCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceOverlayCache")
            .field("entries", &self.entries)
            .field("hidden_paths", &self.hidden_paths)
            .field("embedding_cache_len", &self.embedding_cache.len())
            .field("dirty_paths", &self.dirty_paths)
            .field("dirty_failures", &self.dirty_failures)
            .field("watcher_mode", &self.watcher_mode)
            .field("initialized", &self.initialized)
            .field("resident_fed_count", &self.resident_fed_count)
            .field("graph_context", &self.graph_context_provider.is_some())
            .finish()
    }
}

impl WorkspaceOverlayCache {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.hidden_paths.clear();
        self.dirty_paths.clear();
        self.dirty_failures.clear();
        self.resident_fed_count = 0;
        self.initialized = false;
    }

    /// Mark the overlay initialized with no entries: the caller has proven the store this overlay
    /// fronts was just reconciled with disk, so nothing differs from the baseline and a full disk
    /// scan (a prime) would build zero entries anyway. This is the zero-scan, zero-RAM equivalent
    /// of that prime. Until the overlay is initialized the incremental reindex is inert
    /// ([`Self::reindex_dirty_from_snapshots`] no-ops on `!initialized`), so this is what unblocks
    /// the resident-fed path; from here the watcher marks and the reindex serve fresh edits.
    pub fn mark_initialized_clean(&mut self) {
        self.entries.clear();
        self.hidden_paths.clear();
        self.dirty_paths.clear();
        self.dirty_failures.clear();
        self.initialized = true;
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

    pub fn mark_dirty_path(&mut self, key: FileKey) {
        // A fresh watcher event is a clean slate: clear any consecutive-failure count so a path
        // that failed to refresh before, then genuinely changed, gets the full retry budget again.
        self.dirty_failures.remove(&key);
        self.dirty_seq += 1;
        self.dirty_paths.insert(key, self.dirty_seq);
    }

    /// Re-mark a path whose refresh failed (stat/read error), carrying its consecutive-failure
    /// count. Past [`MAX_DIRTY_REFRESH_FAILURES`] the path is dropped from the dirty set with a
    /// warning instead of retried forever; a later [`Self::mark_dirty_path`] resets it.
    fn retain_dirty_after_failure(&mut self, key: FileKey, prior_failures: u32, reason: &str) {
        let failures = prior_failures + 1;
        if failures >= MAX_DIRTY_REFRESH_FAILURES {
            tracing::warn!(
                root = %key.root_id,
                path = %key.path,
                reason,
                failures,
                "dropping overlay dirty path after repeated refresh failures; a later change \
                 re-marks it fresh"
            );
            return;
        }
        self.dirty_failures.insert(key.clone(), failures);
        self.dirty_seq += 1;
        self.dirty_paths.insert(key, self.dirty_seq);
    }

    /// The paths currently marked dirty (awaiting reindex), for a caller that prefetches
    /// resident snapshots off-lock before feeding them back via
    /// [`Self::reindex_dirty_from_snapshots`].
    pub fn dirty_paths_list(&self) -> Vec<FileKey> {
        self.dirty_paths.keys().cloned().collect()
    }

    /// Reindex the currently-dirty paths, chunking a resident-provided parse where the
    /// snapshot's text matches disk and reading+parsing from disk otherwise. Runs with no
    /// embedder (the interactive `ReuseOnly` discipline: lexical immediately, vectors from the
    /// background pass) and never cold-scans. A no-op until the overlay has been initialized,
    /// so a path marked before the first full refresh is left for that refresh to pick up.
    pub fn reindex_dirty_from_snapshots(
        &mut self,
        roots: &WorkspaceRoots,
        store: &Store,
        batch_size: usize,
        hash_mode: BaselineHashMode,
        snapshots: &HashMap<FileKey, ModuleSnapshot>,
    ) -> Result<(), SearchError> {
        if !self.initialized || self.dirty_paths.is_empty() {
            return Ok(());
        }
        // Process ONLY the prefetched snapshot paths that are still dirty; every other dirty path
        // stays in the set, served by the query's own lazy disk refresh and by later prefetches.
        // The prefetch already capped how many snapshots it fetched, so this bounds the
        // under-lock apply to that same per-query budget (no unbounded reindex here).
        let keys: Vec<FileKey> =
            snapshots.keys().filter(|key| self.dirty_paths.contains_key(*key)).cloned().collect();
        if keys.is_empty() {
            return Ok(());
        }
        // Load the baseline through the fallible store reads BEFORE clearing any dirty flag. A
        // store-wide error here (a schema/manifest read that fails) is NOT a per-path fault: it must
        // leave every prefetched path dirty — with its consecutive-failure budget untouched — so a
        // later prefetch retries it, rather than silently dropping stale overlay entries that no
        // query would ever revisit. Removing the keys first (then hitting `?`) would strand them:
        // neither reindexed nor dirty. The budget is reserved for genuine per-path stat/read
        // failures inside the refresh body (see `retain_dirty_after_failure`); charging a transient
        // store error to it would let a few store hiccups exhaust MAX_DIRTY_REFRESH_FAILURES and
        // drop many healthy paths at once. So the keys leave the dirty set only once the baseline is
        // in hand and each path's per-path refresh owns its outcome.
        let baseline = match store.load_baseline_manifest_fingerprints("code")? {
            Some(manifest_fingerprints) => DirtyBaseline::Manifest(manifest_fingerprints),
            None => {
                DirtyBaseline::Raw(store.all_files_in_collection("code")?.into_iter().collect())
            }
        };

        for key in &keys {
            self.dirty_paths.remove(key);
        }

        match baseline {
            DirtyBaseline::Manifest(manifest_fingerprints) => self
                .refresh_dirty_paths_from_manifest(
                    keys,
                    &manifest_fingerprints,
                    roots,
                    None,
                    batch_size,
                    snapshots,
                )?,
            DirtyBaseline::Raw(baseline_files) => self.refresh_dirty_paths(
                keys,
                RawBaseline { files: &baseline_files, hash_mode },
                roots,
                None,
                batch_size,
                snapshots,
            )?,
        }
        Ok(())
    }

    /// `allow_cold_scan` gates the only expensive operation here: a cold full-tree scan + read +
    /// chunk of every workspace file (`full_refresh_from_manifest`). The background warmup
    /// (`RefreshMode::Embed`) passes `true`; every interactive query/status path passes `false`
    /// so it stays O(cached) under the engine lock and answers from the Postgres baseline until the
    /// warmup (or the watcher's incremental path) populates the overlay. Without this gate a single
    /// query on an unwarmed overlay would block for minutes walking the whole tree.
    pub fn refresh_with_manifest(
        &mut self,
        manifest_fingerprints: &HashMap<FileKey, String>,
        roots: &WorkspaceRoots,
        embedder: Option<&Embedder>,
        batch_size: usize,
        store: &Store,
        allow_cold_scan: bool,
    ) -> Result<(), SearchError> {
        if allow_cold_scan {
            if !self.initialized || !self.watcher_mode {
                self.full_refresh_from_manifest(
                    manifest_fingerprints,
                    roots,
                    embedder,
                    batch_size,
                    store,
                )?;
            } else if !self.dirty_paths.is_empty() {
                let dirty: Vec<FileKey> = self.dirty_paths.drain().map(|(key, _)| key).collect();
                self.refresh_dirty_paths_from_manifest(
                    dirty,
                    manifest_fingerprints,
                    roots,
                    embedder,
                    batch_size,
                    &HashMap::new(),
                )?;
            }
            self.initialized = true;
        } else if self.initialized && !self.dirty_paths.is_empty() {
            // ReuseOnly: never cold-scan. An already-populated cache still applies the cheap
            // watcher-marked dirty-path refresh, but a `!watcher_mode` (polling) cache must NOT
            // re-run the full scan. An uninitialized cache stays empty (and `initialized` stays
            // false) so the next warmup/watcher pass still builds it.
            let dirty: Vec<FileKey> = self.dirty_paths.drain().map(|(key, _)| key).collect();
            self.refresh_dirty_paths_from_manifest(
                dirty,
                manifest_fingerprints,
                roots,
                embedder,
                batch_size,
                &HashMap::new(),
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
        roots: &WorkspaceRoots,
        embedder: Option<&Embedder>,
        batch_size: usize,
        hash_mode: BaselineHashMode,
        allow_cold_scan: bool,
    ) -> Result<(), SearchError> {
        if allow_cold_scan {
            let baseline_files: HashMap<FileKey, Vec<u8>> =
                store.all_files_in_collection("code")?.into_iter().collect();
            if !self.initialized || !self.watcher_mode {
                self.full_refresh(&baseline_files, roots, embedder, batch_size, hash_mode)?;
            } else if !self.dirty_paths.is_empty() {
                let dirty: Vec<FileKey> = self.dirty_paths.drain().map(|(key, _)| key).collect();
                self.refresh_dirty_paths(
                    dirty,
                    RawBaseline { files: &baseline_files, hash_mode },
                    roots,
                    embedder,
                    batch_size,
                    &HashMap::new(),
                )?;
            }
            self.initialized = true;
        } else if self.initialized && !self.dirty_paths.is_empty() {
            // ReuseOnly: never cold-scan. Only the cheap dirty-path refresh on an already-populated
            // cache; a `!watcher_mode` (polling) cache must NOT re-run the full scan, and an
            // uninitialized cache stays empty for the warmup/watcher to build later.
            let baseline_files: HashMap<FileKey, Vec<u8>> =
                store.all_files_in_collection("code")?.into_iter().collect();
            let dirty: Vec<FileKey> = self.dirty_paths.drain().map(|(key, _)| key).collect();
            self.refresh_dirty_paths(
                dirty,
                RawBaseline { files: &baseline_files, hash_mode },
                roots,
                embedder,
                batch_size,
                &HashMap::new(),
            )?;
        }
        Ok(())
    }

    fn full_refresh(
        &mut self,
        baseline_files: &HashMap<FileKey, Vec<u8>>,
        roots: &WorkspaceRoots,
        embedder: Option<&Embedder>,
        batch_size: usize,
        hash_mode: BaselineHashMode,
    ) -> Result<(), SearchError> {
        let workspace_files = scan_workspace_files(roots);
        let mut seen_keys = HashSet::new();
        let mut hidden_paths = HashSet::new();

        for file in workspace_files {
            seen_keys.insert(file.key.clone());
            let baseline_hash = baseline_files.get(&file.key);

            let mut should_remove_cached_entry = false;
            if let Some(entry) = self.entries.get_mut(&file.key) {
                if entry.fingerprint == file.fingerprint {
                    if baseline_hash.is_some_and(|stored_hash| stored_hash == &entry.file_hash) {
                        should_remove_cached_entry = true;
                    } else {
                        if baseline_hash.is_some() {
                            hidden_paths.insert(file.key.clone());
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
                self.entries.remove(&file.key);
                continue;
            }

            let content = match std::fs::read_to_string(&file.abs_path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let file_hash = compute_file_hash(&content, hash_mode);
            if baseline_hash.is_some_and(|stored_hash| stored_hash == &file_hash) {
                self.entries.remove(&file.key);
                continue;
            }

            let provider = self.graph_context_provider.clone();
            let entry = build_overlay_entry(
                &file.key,
                &content,
                file.fingerprint,
                file_hash,
                embedder,
                batch_size,
                &mut self.embedding_cache,
                provider.as_deref(),
                None,
            )?;
            if baseline_hash.is_some() {
                hidden_paths.insert(file.key.clone());
            }
            self.entries.insert(file.key, entry);
        }

        self.entries.retain(|key, _| seen_keys.contains(key));

        for key in baseline_files.keys() {
            if !seen_keys.contains(key) {
                hidden_paths.insert(key.clone());
            }
        }

        self.hidden_paths = hidden_paths;
        self.dirty_paths.clear();
        Ok(())
    }

    fn refresh_dirty_paths(
        &mut self,
        dirty_keys: Vec<FileKey>,
        baseline: RawBaseline<'_>,
        roots: &WorkspaceRoots,
        embedder: Option<&Embedder>,
        batch_size: usize,
        snapshots: &HashMap<FileKey, ModuleSnapshot>,
    ) -> Result<(), SearchError> {
        let RawBaseline { files: baseline_files, hash_mode } = baseline;
        // A path whose stat/read transiently fails is re-marked dirty (carrying its
        // consecutive-failure count) so the next refresh retries it — bounded by
        // [`MAX_DIRTY_REFRESH_FAILURES`] — rather than being silently dropped.
        let mut retry: Vec<(FileKey, u32, &'static str)> = Vec::new();

        for key in dirty_keys {
            // Removing the count here clears it on success (the common path) and hands the prior
            // value to `retain_dirty_after_failure` on failure, keeping the streak consecutive.
            let prior_failures = self.dirty_failures.remove(&key).unwrap_or(0);
            let baseline_hash = baseline_files.get(&key);
            // A key whose root is no longer registered resolves to nothing; it
            // is treated exactly like a file gone from disk, which is what it is
            // for this overlay.
            let abs_path = roots.resolve(&key);

            if !abs_path.as_ref().is_some_and(|path| path.exists()) {
                self.entries.remove(&key);
                if baseline_hash.is_some() {
                    self.hidden_paths.insert(key);
                } else {
                    self.hidden_paths.remove(&key);
                }
                continue;
            }
            let abs_path = abs_path.expect("existence was just checked on Some");

            let metadata = match std::fs::metadata(&abs_path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    retry.push((key, prior_failures, "stat failed"));
                    continue;
                }
            };
            let fingerprint =
                FileFingerprint { len: metadata.len(), modified: metadata.modified().ok() };

            if let Some(entry) = self.entries.get_mut(&key) {
                if entry.fingerprint == fingerprint {
                    if baseline_hash.is_some_and(|stored_hash| stored_hash == &entry.file_hash) {
                        self.entries.remove(&key);
                        self.hidden_paths.remove(&key);
                    } else {
                        if baseline_hash.is_some() {
                            self.hidden_paths.insert(key.clone());
                        } else {
                            self.hidden_paths.remove(&key);
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
                Err(_) => {
                    retry.push((key, prior_failures, "read failed"));
                    continue;
                }
            };
            let file_hash = compute_file_hash(&content, hash_mode);
            if baseline_hash.is_some_and(|stored_hash| stored_hash == &file_hash) {
                self.entries.remove(&key);
                self.hidden_paths.remove(&key);
                continue;
            }

            let provider = self.graph_context_provider.clone();
            let parse_root = resident_parse_root(snapshots, &key, &content);
            if parse_root.is_some() {
                self.resident_fed_count += 1;
            }
            let entry = build_overlay_entry(
                &key,
                &content,
                fingerprint,
                file_hash,
                embedder,
                batch_size,
                &mut self.embedding_cache,
                provider.as_deref(),
                parse_root,
            )?;

            if baseline_hash.is_some() {
                self.hidden_paths.insert(key.clone());
            } else {
                self.hidden_paths.remove(&key);
            }
            self.entries.insert(key, entry);
        }

        for (key, prior_failures, reason) in retry {
            self.retain_dirty_after_failure(key, prior_failures, reason);
        }
        Ok(())
    }

    fn full_refresh_from_manifest(
        &mut self,
        manifest_fingerprints: &HashMap<FileKey, String>,
        roots: &WorkspaceRoots,
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

        let workspace_files = scan_workspace_files(roots);
        let mut seen_keys = HashSet::new();
        let mut hidden_paths = HashSet::new();
        let mut updated_persisted = HashMap::new();

        for file in &workspace_files {
            seen_keys.insert(file.key.clone());
            let baseline_fingerprint = manifest_fingerprints.get(&file.key);

            let mut should_remove_cached_entry = false;
            if let Some(entry) = self.entries.get_mut(&file.key) {
                if entry.fingerprint == file.fingerprint {
                    let local_fp =
                        fingerprint_overlay_documents(&entry.lexical_documents, &file.key.path);
                    if baseline_fingerprint.is_some_and(|stored| stored == &local_fp) {
                        should_remove_cached_entry = true;
                    } else {
                        if baseline_fingerprint.is_some() {
                            hidden_paths.insert(file.key.clone());
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
                self.entries.remove(&file.key);
                continue;
            }

            if let Some(cached) = persisted.get(&file.key) {
                if cached.file_size == file.fingerprint.len
                    && fingerprint_mtime_matches(file.fingerprint.modified, cached)
                {
                    updated_persisted.insert(file.key.clone(), cached.clone());

                    if baseline_fingerprint
                        .is_some_and(|stored| stored == &cached.content_fingerprint)
                    {
                        self.entries.remove(&file.key);
                        continue;
                    }
                }
            }

            let content = match std::fs::read_to_string(&file.abs_path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let file_hash = normalized_file_hash_for_content(&content);
            let local_fp = fingerprint_content(&content, &file.key.path);

            if let Some((secs, nanos)) = mtime_to_secs_nanos(file.fingerprint.modified) {
                updated_persisted.insert(
                    file.key.clone(),
                    crate::store::PersistedFingerprint {
                        file_size: file.fingerprint.len,
                        file_mtime_secs: secs,
                        file_mtime_nanos: nanos,
                        content_fingerprint: local_fp.clone(),
                    },
                );
            }

            if baseline_fingerprint.is_some_and(|stored| stored == &local_fp) {
                self.entries.remove(&file.key);
                continue;
            }

            let provider = self.graph_context_provider.clone();
            let entry = build_overlay_entry(
                &file.key,
                &content,
                file.fingerprint.clone(),
                file_hash,
                embedder,
                batch_size,
                &mut self.embedding_cache,
                provider.as_deref(),
                None,
            )?;
            if baseline_fingerprint.is_some() {
                hidden_paths.insert(file.key.clone());
            }
            self.entries.insert(file.key.clone(), entry);
        }

        self.entries.retain(|key, _| seen_keys.contains(key));

        for key in manifest_fingerprints.keys() {
            if !seen_keys.contains(key) {
                hidden_paths.insert(key.clone());
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
        manifest_fingerprints: &HashMap<FileKey, String>,
        roots: &WorkspaceRoots,
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

        let workspace_files = scan_workspace_files(roots);
        let mut seen_keys = HashSet::new();
        let mut hidden_paths = HashSet::new();
        let mut updated_persisted = HashMap::new();
        let mut entries: Vec<(FileKey, PlannedEntry)> = Vec::new();
        let mut missing_embeddings: HashMap<String, String> = HashMap::new();

        for file in &workspace_files {
            seen_keys.insert(file.key.clone());
            let baseline_fingerprint = manifest_fingerprints.get(&file.key);

            if let Some(cached) = persisted.get(&file.key) {
                if cached.file_size == file.fingerprint.len
                    && fingerprint_mtime_matches(file.fingerprint.modified, cached)
                {
                    updated_persisted.insert(file.key.clone(), cached.clone());

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
            let local_fp = fingerprint_content(&content, &file.key.path);

            if let Some((secs, nanos)) = mtime_to_secs_nanos(file.fingerprint.modified) {
                updated_persisted.insert(
                    file.key.clone(),
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
                build_overlay_documents(&file.key, &content, graph_context, None);
            for input in &embedding_inputs {
                let key = overlay_embedding_key(input);
                if !warm_embeddings.contains_key(&key) {
                    missing_embeddings.entry(key).or_insert_with(|| input.clone());
                }
            }

            if baseline_fingerprint.is_some() {
                hidden_paths.insert(file.key.clone());
            }
            entries.push((
                file.key.clone(),
                PlannedEntry {
                    fingerprint: file.fingerprint.clone(),
                    file_hash,
                    lexical_documents,
                    embedding_inputs,
                },
            ));
        }

        for key in manifest_fingerprints.keys() {
            if !seen_keys.contains(key) {
                hidden_paths.insert(key.clone());
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
        dirty_before: &HashMap<FileKey, u64>,
        embedder: Option<&Embedder>,
        store: &Store,
    ) -> Result<(), SearchError> {
        for (embedding_key, embedding) in new_embeddings {
            self.embedding_cache.insert(embedding_key, embedding);
        }

        let mut entries = HashMap::with_capacity(plan.entries.len());
        for (key, planned) in plan.entries {
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
                key,
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
        for (key, seq) in dirty_before {
            if self.dirty_paths.get(key) == Some(seq) {
                self.dirty_paths.remove(key);
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
    pub fn dirty_paths_snapshot(&self) -> HashMap<FileKey, u64> {
        self.dirty_paths.clone()
    }

    fn refresh_dirty_paths_from_manifest(
        &mut self,
        dirty_keys: Vec<FileKey>,
        manifest_fingerprints: &HashMap<FileKey, String>,
        roots: &WorkspaceRoots,
        embedder: Option<&Embedder>,
        batch_size: usize,
        snapshots: &HashMap<FileKey, ModuleSnapshot>,
    ) -> Result<(), SearchError> {
        // A path whose stat/read transiently fails is re-marked dirty (carrying its
        // consecutive-failure count) so the next refresh retries it — bounded by
        // [`MAX_DIRTY_REFRESH_FAILURES`] — rather than being silently dropped.
        let mut retry: Vec<(FileKey, u32, &'static str)> = Vec::new();

        for key in dirty_keys {
            // Removing the count here clears it on success (the common path) and hands the prior
            // value to `retain_dirty_after_failure` on failure, keeping the streak consecutive.
            let prior_failures = self.dirty_failures.remove(&key).unwrap_or(0);
            let baseline_fingerprint = manifest_fingerprints.get(&key);
            // A key whose root is no longer registered resolves to nothing; it
            // is treated exactly like a file gone from disk, which is what it is
            // for this overlay.
            let abs_path = roots.resolve(&key);

            if !abs_path.as_ref().is_some_and(|path| path.exists()) {
                self.entries.remove(&key);
                if baseline_fingerprint.is_some() {
                    self.hidden_paths.insert(key);
                } else {
                    self.hidden_paths.remove(&key);
                }
                continue;
            }
            let abs_path = abs_path.expect("existence was just checked on Some");

            let metadata = match std::fs::metadata(&abs_path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    retry.push((key, prior_failures, "stat failed"));
                    continue;
                }
            };
            let fingerprint =
                FileFingerprint { len: metadata.len(), modified: metadata.modified().ok() };

            if let Some(entry) = self.entries.get_mut(&key) {
                if entry.fingerprint == fingerprint {
                    let local_fp =
                        fingerprint_overlay_documents(&entry.lexical_documents, &key.path);
                    if baseline_fingerprint.is_some_and(|stored| stored == &local_fp) {
                        self.entries.remove(&key);
                        self.hidden_paths.remove(&key);
                    } else {
                        if baseline_fingerprint.is_some() {
                            self.hidden_paths.insert(key.clone());
                        } else {
                            self.hidden_paths.remove(&key);
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
                Err(_) => {
                    retry.push((key, prior_failures, "read failed"));
                    continue;
                }
            };
            let file_hash = normalized_file_hash_for_content(&content);
            let local_fp = fingerprint_content(&content, &key.path);
            if baseline_fingerprint.is_some_and(|stored| stored == &local_fp) {
                self.entries.remove(&key);
                self.hidden_paths.remove(&key);
                continue;
            }

            let provider = self.graph_context_provider.clone();
            let parse_root = resident_parse_root(snapshots, &key, &content);
            if parse_root.is_some() {
                self.resident_fed_count += 1;
            }
            let entry = build_overlay_entry(
                &key,
                &content,
                fingerprint,
                file_hash,
                embedder,
                batch_size,
                &mut self.embedding_cache,
                provider.as_deref(),
                parse_root,
            )?;

            if baseline_fingerprint.is_some() {
                self.hidden_paths.insert(key.clone());
            } else {
                self.hidden_paths.remove(&key);
            }
            self.entries.insert(key, entry);
        }

        for (key, prior_failures, reason) in retry {
            self.retain_dirty_after_failure(key, prior_failures, reason);
        }
        Ok(())
    }

    /// How many overlay entries have been built from a resident-provided shared parse (rather than
    /// a self-parsed disk read) since the last [`Self::clear`]. Observability for the resident-fed
    /// incremental reindex — a nonzero value proves the shared-parse path actually fired.
    pub fn resident_fed_count(&self) -> usize {
        self.resident_fed_count
    }

    /// The consecutive-failure count recorded for a dirty path, or `0` when none is tracked. Lets a
    /// test assert that a store-wide error left a path's retry budget untouched.
    #[cfg(test)]
    fn dirty_failure_count(&self, key: &FileKey) -> u32 {
        self.dirty_failures.get(key).copied().unwrap_or(0)
    }

    pub fn snapshot(&self) -> WorkspaceOverlayIndex {
        let baseline =
            BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "local-workspace-baseline");
        let mut overlay = SearchOverlay::new(baseline);
        let mut lexical_documents = Vec::new();
        let mut vector_documents = Vec::new();

        let mut entry_keys: Vec<&FileKey> = self.entries.keys().collect();
        entry_keys.sort();
        for key in entry_keys {
            let entry = self.entries.get(key).expect("key collected from map keys");
            overlay.replace_file(
                DocumentPath::new("code", &key.root_id, &key.path),
                entry.lexical_documents.clone(),
            );
            lexical_documents.extend(entry.lexical_documents.clone());
            vector_documents.extend(entry.vector_documents.clone());
        }

        let mut deleted_keys: Vec<&FileKey> =
            self.hidden_paths.iter().filter(|key| !self.entries.contains_key(*key)).collect();
        deleted_keys.sort();
        for key in deleted_keys {
            overlay.delete_file(DocumentPath::new("code", &key.root_id, &key.path));
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
            self.hidden_paths.iter().filter(|key| !self.entries.contains_key(*key)).count();
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
    key: FileKey,
    abs_path: PathBuf,
    fingerprint: FileFingerprint,
}

/// Every `.bsl` file of every registered root, each under the key it is stored
/// by. Roots may nest, so a file reached from two of them is enumerated twice
/// and attributed to the same owner both times; de-duplicating by key is what
/// keeps one file one entry.
fn scan_workspace_files(roots: &WorkspaceRoots) -> Vec<WorkspaceFileState> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for (_, root) in roots.entries() {
        for state in scan_one_root(roots, root) {
            if seen.insert(state.key.clone()) {
                files.push(state);
            }
        }
    }
    files
}

fn scan_one_root(roots: &WorkspaceRoots, root: &Path) -> Vec<WorkspaceFileState> {
    // The canonical spelling is taken ONCE for the root and each file's derived
    // from it, rather than per file: the walk does not follow directory links, so
    // everything it reaches is physically under this root and the only aliasing
    // that can occur is in the root's own path. A walk that starts following
    // links must supply the canonical spelling itself — deriving it here would
    // then name a file that is not where the walk actually went.
    let root_canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")))
        .filter_map(|entry| {
            let walked = entry.path();
            // Following the link: the read below reads the target, so the target
            // is what the fingerprint must describe. A dangling link fails here
            // and is dropped, which is right — there is no file to index.
            let metadata = std::fs::metadata(walked).ok()?;
            // A symlinked FILE is still yielded (only directory links are not
            // descended into), and it may point out of this root entirely. Its
            // canonical spelling has to be taken for real; deriving it from the
            // root would name a file that is not the one being read, and the
            // point-update path — which canonicalizes in full — would then
            // attribute the same file to a different root.
            let canonical = if entry.path_is_symlink() {
                std::fs::canonicalize(walked).ok()?
            } else {
                match walked.strip_prefix(root) {
                    Ok(rel) => root_canonical.join(rel),
                    Err(_) => walked.to_path_buf(),
                }
            };
            let key = roots.root_of(walked, &canonical)?;
            Some(WorkspaceFileState {
                key,
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
    key: &FileKey,
    content: &str,
    fingerprint: FileFingerprint,
    file_hash: Vec<u8>,
    embedder: Option<&Embedder>,
    batch_size: usize,
    embedding_cache: &mut HashMap<String, Vec<f32>>,
    graph_context: Option<&dyn GraphContextProvider>,
    parse_root: Option<&syntax::SyntaxNode>,
) -> Result<OverlayFileEntry, SearchError> {
    let (lexical_documents, embedding_inputs) =
        build_overlay_documents(key, content, graph_context, parse_root);
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

/// Both recipes below reproduce the publisher's per-file fingerprint byte for
/// byte, because the only thing they are ever compared against is the value a
/// published manifest carries. A recipe of their own would mean no file in the
/// working tree ever matches the baseline, and the whole corpus would live as
/// an overlay delta.
///
/// Where the publisher folds in each document's graph context, these write its
/// "absent" marker unconditionally. That is not an omission: the published
/// corpus is indexed with no graph context provider at all, so a local document
/// enriched with context describes the same text as the context-free one the
/// snapshot holds. Hashing the enrichment here could only ever report a file
/// whose text matches the snapshot as locally changed.
pub(crate) fn fingerprint_content(content: &str, rel_path: &str) -> String {
    let chunks = Chunker::chunk(content);
    let mut documents: Vec<(u32, u32, &str, &str, String, &str)> = chunks
        .iter()
        .map(|chunk| {
            let kind = match chunk.kind {
                code_chunk::ChunkKind::ModuleHeader => "header",
                code_chunk::ChunkKind::Procedure => "procedure",
                code_chunk::ChunkKind::Function => "function",
            };
            let content_hash = blake3::hash(chunk.text.as_bytes()).to_hex().to_string();
            (
                chunk.line_start,
                chunk.line_end,
                chunk.name.as_str(),
                kind,
                content_hash,
                chunk.text.as_str(),
            )
        })
        .collect();
    sort_like_the_publisher(&mut documents, |lhs, rhs| {
        (lhs.0, lhs.1, lhs.2, lhs.3, lhs.4.as_str()).cmp(&(
            rhs.0,
            rhs.1,
            rhs.2,
            rhs.3,
            rhs.4.as_str(),
        ))
    });

    let mut hasher = blake3::Hasher::new();
    for (line_start, line_end, name, kind, content_hash, text) in &documents {
        hasher.update("code".as_bytes());
        hasher.update(&[0]);
        hasher.update(rel_path.as_bytes());
        hasher.update(&[0]);
        hasher.update(name.as_bytes());
        hasher.update(&[0]);
        hasher.update(kind.as_bytes());
        hasher.update(&line_start.to_le_bytes());
        hasher.update(&line_end.to_le_bytes());
        hasher.update(content_hash.as_bytes());
        hasher.update(&[0]);
        hasher.update(text.as_bytes());
        hasher.update(&[0]);
        hasher.update(&[0]);
        hasher.update(&[0xff]);
    }
    hasher.finalize().to_hex().to_string()
}

/// The publisher hashes a file's chunks in sorted order, while a chunker returns
/// them in source order. For ordinary code the two coincide — line numbers rise
/// with position — and they part ways as soon as several chunks share a line
/// span, as two one-line methods written on one physical line do. Ordering
/// locally by the publisher's key is what makes the recipes agree for those
/// files too.
fn sort_like_the_publisher<T>(documents: &mut [T], compare: impl Fn(&T, &T) -> std::cmp::Ordering) {
    documents.sort_by(compare);
}

pub(crate) fn fingerprint_overlay_documents(
    documents: &[IndexedDocument],
    rel_path: &str,
) -> String {
    let mut ordered: Vec<&IndexedDocument> = documents.iter().collect();
    sort_like_the_publisher(&mut ordered, |lhs, rhs| {
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

    let mut hasher = blake3::Hasher::new();
    for document in ordered {
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
        hasher.update(&[0]);
        hasher.update(&[0]);
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

/// The shared syntax tree to chunk `content` with, when the resident snapshot for `rel_path`
/// holds byte-identical text. A mismatch (the file changed on disk after the resident parsed
/// it) falls back to `None` so the caller parses `content` itself, keeping chunk output and
/// the stored hash pinned to the exact bytes on disk.
fn resident_parse_root<'a>(
    snapshots: &'a HashMap<FileKey, ModuleSnapshot>,
    key: &FileKey,
    content: &str,
) -> Option<&'a syntax::SyntaxNode> {
    snapshots
        .get(key)
        .filter(|snapshot| snapshot.text.as_ref() == content)
        .map(|snapshot| &snapshot.root)
}

fn build_overlay_documents(
    key: &FileKey,
    content: &str,
    graph_context: Option<&dyn GraphContextProvider>,
    parse_root: Option<&syntax::SyntaxNode>,
) -> (Vec<IndexedDocument>, Vec<String>) {
    // When the resident host already parsed this exact text, chunk its shared syntax tree
    // instead of parsing `content` again (`chunk_parsed` is byte-parity-tested against
    // `chunk`). `content` still drives every text/offset/hash decision, so the chunk output
    // and the stored hash are identical to the pure-disk path.
    let chunks = match parse_root {
        Some(root) => Chunker::chunk_parsed(root, content),
        None => Chunker::chunk(content),
    };
    let mut lexical_documents = Vec::with_capacity(chunks.len());
    let mut embedding_inputs = Vec::with_capacity(chunks.len());

    for chunk in &chunks {
        let document = crate::document::indexed_document_for_chunk(key, chunk, graph_context);
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
            root_id: document.document.root_id.clone(),
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
        build_overlay_documents, fingerprint_content, lexical_hits, BaselineHashMode,
        WorkspaceOverlayCache, WorkspaceOverlayStats, MAX_DIRTY_REFRESH_FAILURES,
    };
    use crate::store::Store;
    use crate::workspace_roots::{FileKey, WorkspaceRoots, CONFIGURATION_ROOT_ID};
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    /// A workspace whose only source root is the workspace directory itself —
    /// the shape every test here works in unless it says otherwise.
    fn single_root(workspace: &Path) -> WorkspaceRoots {
        WorkspaceRoots::build(workspace, workspace, &[]).0
    }

    /// The store key of a configuration file at `path`.
    fn key(path: &str) -> FileKey {
        FileKey::configuration(path)
    }

    /// Chunk output through the resident-shared parse must be byte-identical to the pure
    /// disk-read+parse path — for a UTF-8 BOM, CRLF line endings, and a method large enough
    /// to cross the 32 KiB chunk-split threshold.
    #[test]
    fn snapshot_chunking_matches_disk_for_edge_cases() {
        let large_body: String = (0..4000).map(|i| format!("    П = П + {i};\n")).collect();
        let large = format!("Процедура Большая() Экспорт\n{large_body}КонецПроцедуры\n");
        let cases = [
            "\u{feff}Процедура СБом()\nКонецПроцедуры\n".to_owned(),
            "Процедура СRLF()\r\nВозврат;\r\nКонецПроцедуры\r\n".to_owned(),
            large,
        ];
        for content in &cases {
            let root = parser::parse(content).syntax_node();
            let (disk_docs, disk_inputs) =
                build_overlay_documents(&key("M.bsl"), content, None, None);
            let (snap_docs, snap_inputs) =
                build_overlay_documents(&key("M.bsl"), content, None, Some(&root));

            assert!(!disk_docs.is_empty(), "fixture must produce at least one chunk");
            assert_eq!(disk_docs.len(), snap_docs.len(), "chunk count must match");
            for (disk, snap) in disk_docs.iter().zip(&snap_docs) {
                assert_eq!(disk.symbol_name, snap.symbol_name);
                assert_eq!(disk.kind, snap.kind);
                assert_eq!(disk.line_start, snap.line_start);
                assert_eq!(disk.line_end, snap.line_end);
                assert_eq!(disk.text, snap.text);
                assert_eq!(disk.content_hash, snap.content_hash);
            }
            assert_eq!(disk_inputs, snap_inputs, "embedding inputs must match");
        }
        // The large fixture genuinely crosses the split threshold, so parity is checked with
        // more than one chunk in play.
        assert!(
            build_overlay_documents(&key("M.bsl"), &cases[2], None, None).0.len() > 1,
            "the large fixture must exercise the 32 KiB split"
        );
    }

    /// A dirty path whose read transiently fails (here a directory shaped like a `.bsl`, so
    /// `metadata` succeeds but `read_to_string` errors) must stay in the dirty set for the next
    /// refresh, rather than being silently dropped. Restoring the pre-fix `continue`-drop in
    /// `refresh_dirty_paths_from_manifest` makes this assertion fail.
    #[test]
    fn dirty_path_survives_a_read_failure() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let store = Store::open(&workspace.join("search.db")).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        let manifest: HashMap<FileKey, String> = HashMap::new();
        // A full refresh initializes the cache so the next refresh takes the incremental branch.
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, true)
            .unwrap();

        fs::create_dir(workspace.join("Broken.bsl")).unwrap();
        cache.mark_dirty_path(key("Broken.bsl"));
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, false)
            .unwrap();

        assert_eq!(
            cache.stats().pending_dirty_paths,
            1,
            "a read-failed dirty path must be retained for the next refresh"
        );
    }

    /// A path that fails to refresh on every attempt is retained for exactly
    /// [`MAX_DIRTY_REFRESH_FAILURES`] attempts, then dropped from the dirty set (with a warning)
    /// so it stops being retried forever. A fresh `mark_dirty_path` clears the streak, giving the
    /// path the full retry budget again. Removing the bookkeeping (unconditionally re-marking)
    /// makes the drop never happen; removing the reset makes the fresh mark not restore it.
    #[test]
    fn dirty_path_dropped_after_max_consecutive_failures_and_reset_by_fresh_mark() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let store = Store::open(&workspace.join("search.db")).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        let manifest: HashMap<FileKey, String> = HashMap::new();
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, true)
            .unwrap();

        // A directory shaped like a `.bsl`: `metadata` succeeds, `read_to_string` always fails.
        fs::create_dir(workspace.join("Broken.bsl")).unwrap();
        cache.mark_dirty_path(key("Broken.bsl"));

        // The first K-1 refreshes keep retrying: the path stays dirty.
        for _ in 0..(MAX_DIRTY_REFRESH_FAILURES - 1) {
            cache
                .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, false)
                .unwrap();
            assert_eq!(
                cache.stats().pending_dirty_paths,
                1,
                "the path is retained while under the failure budget"
            );
        }
        // The K-th consecutive failure drops it.
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, false)
            .unwrap();
        assert_eq!(
            cache.stats().pending_dirty_paths,
            0,
            "the path is dropped after exactly MAX_DIRTY_REFRESH_FAILURES failures"
        );

        // A fresh watcher event resets the streak: the path survives the budget again.
        cache.mark_dirty_path(key("Broken.bsl"));
        for _ in 0..(MAX_DIRTY_REFRESH_FAILURES - 1) {
            cache
                .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, false)
                .unwrap();
            assert_eq!(
                cache.stats().pending_dirty_paths,
                1,
                "a fresh mark reset the consecutive-failure count"
            );
        }
    }

    /// A store-wide error while resolving the baseline for a snapshot-fed reindex must leave every
    /// prefetched-but-unprocessed path dirty, with its per-path failure budget untouched — so a
    /// later prefetch retries it instead of stranding stale overlay entries no query would revisit.
    /// The pre-fix code cleared the dirty flags BEFORE the fallible store read, so on error the
    /// paths were neither reindexed nor dirty; restoring that ordering makes the retained-count
    /// assertion fail. Because the store error is not a per-path fault, it must NOT be charged to
    /// `MAX_DIRTY_REFRESH_FAILURES` (else a few store hiccups would drop many healthy paths at once).
    #[test]
    fn store_error_during_reindex_keeps_paths_dirty_without_charging_failure_budget() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let store = Store::open(&workspace.join("search.db")).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        let manifest: HashMap<FileKey, String> = HashMap::new();
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, true)
            .unwrap();

        // A directory shaped like a `.bsl`: `metadata` succeeds, `read_to_string` always fails, so
        // one healthy reindex records a genuine per-path failure (budget = 1). That seeded count is
        // what the store-error reindex below must leave untouched.
        fs::create_dir(workspace.join("Broken.bsl")).unwrap();
        cache.mark_dirty_path(key("Broken.bsl"));

        let content = "Процедура П()\nКонецПроцедуры\n";
        let root = parser::parse(content).syntax_node();
        let mut snapshots = HashMap::new();
        snapshots.insert(
            key("Broken.bsl"),
            crate::ports::ModuleSnapshot { text: std::sync::Arc::from(content), root },
        );

        cache
            .reindex_dirty_from_snapshots(
                &single_root(workspace),
                &store,
                32,
                BaselineHashMode::NormalizedChunks,
                &snapshots,
            )
            .unwrap();
        assert_eq!(cache.stats().pending_dirty_paths, 1, "the read-failed path stays dirty");
        assert_eq!(
            cache.dirty_failure_count(&key("Broken.bsl")),
            1,
            "one genuine per-path failure"
        );

        // Drop the manifest tables through a second connection so the next reindex fails at the
        // baseline read (`load_baseline_manifest_fingerprints`) before it processes any path.
        {
            let raw = rusqlite::Connection::open(store.db_path()).unwrap();
            raw.execute_batch(
                "PRAGMA foreign_keys = OFF;
                 DROP TABLE IF EXISTS baseline_manifest_files;
                 DROP TABLE IF EXISTS baseline_manifest;",
            )
            .unwrap();
        }

        let result = cache.reindex_dirty_from_snapshots(
            &single_root(workspace),
            &store,
            32,
            BaselineHashMode::NormalizedChunks,
            &snapshots,
        );
        assert!(result.is_err(), "the dropped baseline table must surface as a store error");
        assert_eq!(
            cache.stats().pending_dirty_paths,
            1,
            "a store-wide error must not strand the prefetched path (still dirty)"
        );
        assert_eq!(
            cache.dirty_failure_count(&key("Broken.bsl")),
            1,
            "a store-wide error must not consume the per-path retry budget"
        );
    }

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
        manifest.insert(key("A.bsl"), "different-fingerprint".to_owned());

        let mut cache = WorkspaceOverlayCache::default();
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, true)
            .unwrap();

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
        manifest.insert(key("A.bsl"), "different-fingerprint".to_owned());

        let warm = HashMap::new();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &single_root(workspace),
            &store,
            &warm,
            None,
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
            key("CommonModules/МодульА/Ext/Module.bsl"),
            "different-fingerprint".to_owned(),
        );
        manifest.insert(
            key("CommonModules/МодульБ/Ext/Module.bsl"),
            "different-fingerprint".to_owned(),
        );

        let warm = HashMap::new();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &single_root(workspace),
            &store,
            &warm,
            None,
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
            &manifest,
            &single_root(workspace),
            &store,
            &warm,
            None,
        )
        .unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.mark_dirty_path(key("before.bsl"));
        cache.mark_dirty_path(key("reedited.bsl"));
        let dirty_before = cache.dirty_paths_snapshot();
        // Watcher activity during the lock-free embed window: a brand-new path, plus a re-edit of a
        // path that was already in the snapshot (its sequence advances).
        cache.mark_dirty_path(key("during.bsl"));
        cache.mark_dirty_path(key("reedited.bsl"));

        cache.publish_plan(plan, HashMap::new(), &dirty_before, None, &store).unwrap();

        // before.bsl was superseded and cleared; during.bsl (new) and reedited.bsl (re-marked after
        // the snapshot) both survive so a later refresh re-embeds them.
        assert_eq!(cache.stats().pending_dirty_paths, 2);
        let remaining = cache.dirty_paths_snapshot();
        assert!(remaining.contains_key(&key("during.bsl")));
        assert!(remaining.contains_key(&key("reedited.bsl")));
        assert!(!remaining.contains_key(&key("before.bsl")));
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
        store
            .reindex_file(CONFIGURATION_ROOT_ID, "A.bsl", hash_a.as_bytes(), &chunks_a, None)
            .unwrap();
        store
            .reindex_file(CONFIGURATION_ROOT_ID, "B.bsl", hash_b.as_bytes(), &chunks_b, None)
            .unwrap();

        fs::write(&file_a, "Процедура НовоеИмя()\nКонецПроцедуры").unwrap();
        fs::remove_file(&file_b).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache
            .refresh(
                &store,
                &single_root(workspace),
                None,
                32,
                BaselineHashMode::RawFileBytes,
                true,
            )
            .unwrap();
        let overlay = cache.snapshot();

        assert!(overlay.hidden_paths.contains(&key("A.bsl")));
        assert!(overlay.hidden_paths.contains(&key("B.bsl")));
        assert_eq!(overlay.lexical_documents.len(), 1);
        assert_eq!(overlay.lexical_documents[0].symbol_name, "НовоеИмя");
    }

    /// A `cfe` extension repeats the configuration's layout, so the same relative
    /// path exists under both roots at once. Each copy must reach the overlay as
    /// its own entry, and each hit must say which root it came from — a
    /// path-keyed overlay collapses them into one and silently loses a file.
    #[test]
    fn the_same_relative_path_under_two_roots_stays_two_overlay_entries() {
        const MODULE: &str = "CommonModules/М/Ext/Module.bsl";
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let configuration = workspace.join("cf");
        let extension = workspace.join("cfe/one");
        for (root, symbol) in [(&configuration, "ИзКонфигурации"), (&extension, "ИзРасширения")]
        {
            let file = root.join(MODULE);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, format!("Процедура {symbol}()\nКонецПроцедуры")).unwrap();
        }
        let (roots, rejected) =
            WorkspaceRoots::build(workspace, &configuration, std::slice::from_ref(&extension));
        assert!(rejected.is_empty(), "the extension sits beside the configuration");

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        let overlay = cache.snapshot();

        assert_eq!(
            overlay.overlay.changes.len(),
            2,
            "one relative path under two roots is two files, not one"
        );
        let mut owners: Vec<(String, String)> = overlay
            .lexical_documents
            .iter()
            .map(|d| (d.root_id.clone(), d.symbol_name.clone()))
            .collect();
        owners.sort();
        assert_eq!(
            owners,
            vec![
                (CONFIGURATION_ROOT_ID.to_owned(), "ИзКонфигурации".to_owned()),
                ("cfe/one".to_owned(), "ИзРасширения".to_owned()),
            ],
            "each document carries the root it was found under"
        );
    }

    /// Both copies must survive the merge too: the fusion key and the dedup key
    /// are independent, and either one keyed by path alone drops a hit.
    #[test]
    fn two_roots_with_one_relative_path_give_two_hits() {
        const MODULE: &str = "CommonModules/М/Ext/Module.bsl";
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let configuration = workspace.join("cf");
        let extension = workspace.join("cfe/one");
        for root in [&configuration, &extension] {
            let file = root.join(MODULE);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, "Процедура ОбщееИмя()\nКонецПроцедуры").unwrap();
        }
        let (roots, _) =
            WorkspaceRoots::build(workspace, &configuration, std::slice::from_ref(&extension));

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        let overlay = cache.snapshot();

        let hits = lexical_hits(&overlay, "ОбщееИмя", 10);
        assert_eq!(hits.len(), 2, "identical symbol at one relative path in two roots");
        let mut roots_of_hits: Vec<&str> = hits.iter().map(|h| h.root_id.as_str()).collect();
        roots_of_hits.sort();
        assert_eq!(roots_of_hits, vec![CONFIGURATION_ROOT_ID, "cfe/one"]);
        for hit in &hits {
            assert_eq!(hit.file_path, MODULE, "the path stays relative to its own root");
        }
    }

    /// A root reached through an alias inside the configuration: the walk arrives
    /// by the declared spelling, but the files belong to the extension the alias
    /// points at. Attributing by the walked spelling alone would hand them to the
    /// configuration, whose subtree the alias sits in.
    #[cfg(unix)]
    #[test]
    fn a_root_declared_through_an_alias_keeps_its_own_files() {
        const MODULE: &str = "CommonModules/М/Ext/Module.bsl";
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let workspace = dir.path();
        let configuration = workspace.join("cf");
        fs::create_dir_all(&configuration).unwrap();
        let real = outside.path().join("ext");
        let file = real.join(MODULE);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "Процедура ЗаСсылкой()\nКонецПроцедуры").unwrap();
        let alias = configuration.join("Linked");
        std::os::unix::fs::symlink(&real, &alias).unwrap();

        let (roots, rejected) =
            WorkspaceRoots::build(workspace, &configuration, std::slice::from_ref(&alias));
        assert!(rejected.is_empty(), "only the alias is inside the configuration, not the root");

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        let overlay = cache.snapshot();

        assert_eq!(overlay.lexical_documents.len(), 1, "the aliased root's file is indexed");
        let document = &overlay.lexical_documents[0];
        assert_eq!(document.symbol_name, "ЗаСсылкой");
        assert_ne!(
            document.root_id, CONFIGURATION_ROOT_ID,
            "the file belongs to the extension, not to the configuration the alias sits in"
        );
        assert_eq!(document.path, MODULE, "keyed relative to its own root");
    }

    /// A `.bsl` that is a symlink into another root belongs to the root it
    /// physically lives in. Attributing it by the walked spelling would give one
    /// file two entries and put the walk at odds with the point-update path,
    /// which resolves the link in full.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_file_belongs_to_the_root_it_lives_in() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let configuration = workspace.join("cf");
        let extension = workspace.join("cfe/one");
        fs::create_dir_all(&configuration).unwrap();
        fs::create_dir_all(&extension).unwrap();
        let target = extension.join("Target.bsl");
        fs::write(&target, "Процедура ЖивётВРасширении()\nКонецПроцедуры").unwrap();
        std::os::unix::fs::symlink(&target, configuration.join("Alias.bsl")).unwrap();

        let (roots, _) =
            WorkspaceRoots::build(workspace, &configuration, std::slice::from_ref(&extension));
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        let documents = cache.snapshot().lexical_documents;

        assert_eq!(documents.len(), 1, "one file is one entry: {documents:?}");
        assert_eq!(
            (documents[0].root_id.as_str(), documents[0].path.as_str()),
            ("cfe/one", "Target.bsl"),
            "the root it lives in owns it, not the one holding the alias"
        );
    }

    /// The fingerprint of a symlinked `.bsl` must describe the file whose bytes
    /// are read — the target. Stat'ing the link instead reports the link's own
    /// length and mtime, which do not move when the target is edited, so the
    /// edit would be invisible to every later refresh.
    #[cfg(unix)]
    #[test]
    fn editing_a_symlink_target_is_seen_through_the_link() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let workspace = dir.path();
        let target = outside.path().join("Настоящий.bsl");
        fs::write(&target, "Процедура Старая()\nКонецПроцедуры").unwrap();
        std::os::unix::fs::symlink(&target, workspace.join("Ссылка.bsl")).unwrap();

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let roots = single_root(workspace);
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        assert_eq!(cache.snapshot().lexical_documents[0].symbol_name, "Старая");

        fs::write(&target, "Процедура Новая()\nКонецПроцедуры").unwrap();
        cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        let documents = cache.snapshot().lexical_documents;

        assert_eq!(documents.len(), 1);
        assert_eq!(
            documents[0].symbol_name, "Новая",
            "an edit behind the link must move the fingerprint"
        );
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
        cache
            .refresh(
                &store,
                &single_root(workspace),
                None,
                32,
                BaselineHashMode::RawFileBytes,
                true,
            )
            .unwrap();
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
        cache
            .refresh(
                &store,
                &single_root(workspace),
                None,
                32,
                BaselineHashMode::RawFileBytes,
                true,
            )
            .unwrap();
        let first = cache.snapshot();
        assert_eq!(first.lexical_documents[0].symbol_name, "ВерсияОдин111");

        cache
            .refresh(
                &store,
                &single_root(workspace),
                None,
                32,
                BaselineHashMode::RawFileBytes,
                true,
            )
            .unwrap();
        let second = cache.snapshot();
        assert_eq!(second.lexical_documents[0].symbol_name, "ВерсияОдин111");

        fs::write(&file, "Процедура ВерсияДва222222()\nКонецПроцедуры").unwrap();
        cache
            .refresh(
                &store,
                &single_root(workspace),
                None,
                32,
                BaselineHashMode::RawFileBytes,
                true,
            )
            .unwrap();
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
        store
            .reindex_file(CONFIGURATION_ROOT_ID, "A.bsl", hash_a.as_bytes(), &chunks_a, None)
            .unwrap();
        store
            .reindex_file(CONFIGURATION_ROOT_ID, "B.bsl", hash_b.as_bytes(), &chunks_b, None)
            .unwrap();

        fs::write(&file_a, "Процедура Измененная()\nКонецПроцедуры").unwrap();
        fs::remove_file(&file_b).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache
            .refresh(
                &store,
                &single_root(workspace),
                None,
                32,
                BaselineHashMode::RawFileBytes,
                true,
            )
            .unwrap();

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
        store.reindex_file(CONFIGURATION_ROOT_ID, "A.bsl", hash.as_bytes(), &chunks, None).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache
            .refresh(
                &store,
                &single_root(workspace),
                None,
                32,
                BaselineHashMode::RawFileBytes,
                true,
            )
            .unwrap();
        assert_eq!(cache.stats().overlay_files, 0);

        fs::write(&file, "Процедура ИзWatcher()\nКонецПроцедуры").unwrap();
        cache.mark_dirty_path(key("A.bsl"));
        cache
            .refresh(
                &store,
                &single_root(workspace),
                None,
                32,
                BaselineHashMode::RawFileBytes,
                true,
            )
            .unwrap();

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
        let manifest: HashMap<FileKey, String> = HashMap::new();
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, true)
            .unwrap();

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
        manifest.insert(key("A.bsl"), fp);

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, true)
            .unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 0);
        assert!(!overlay.hidden_paths.contains(&key("A.bsl")));
    }

    #[test]
    fn manifest_refresh_detects_modified_file() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура Старая()\nКонецПроцедуры").unwrap();

        let mut manifest = HashMap::new();
        manifest.insert(key("A.bsl"), "different-fingerprint".to_owned());

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, true)
            .unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 1);
        assert_eq!(overlay.lexical_documents[0].symbol_name, "Старая");
        assert!(overlay.hidden_paths.contains(&key("A.bsl")));
    }

    #[test]
    fn manifest_refresh_detects_deleted_baseline_file() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();

        let mut manifest = HashMap::new();
        manifest.insert(key("A.bsl"), "some-fp".to_owned());
        manifest.insert(key("B.bsl"), "other-fp".to_owned());

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, true)
            .unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 0);
        assert_eq!(overlay.hidden_paths.len(), 2);
        assert!(overlay.hidden_paths.contains(&key("A.bsl")));
        assert!(overlay.hidden_paths.contains(&key("B.bsl")));
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
        manifest.insert(key("A.bsl"), "different-fingerprint".to_owned());

        let mut cache = WorkspaceOverlayCache::default();
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, false)
            .unwrap();

        let overlay = cache.snapshot();
        assert!(
            overlay.lexical_documents.is_empty(),
            "ReuseOnly over an uninitialized cache must not cold-scan present files"
        );
        assert_eq!(cache.stats().overlay_files, 0);

        // The gate is the only difference: a cold-scan-allowed refresh of the same cache populates.
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, true)
            .unwrap();
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
        manifest.insert(key("A.bsl"), "different-fingerprint".to_owned());
        manifest.insert(key("B.bsl"), "different-fingerprint".to_owned());

        // Populate the cache once via the cold-scan path so it is initialized.
        let mut cache = WorkspaceOverlayCache::default();
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, true)
            .unwrap();
        assert_eq!(cache.snapshot().lexical_documents.len(), 1, "A.bsl is the only overlay entry");
        assert!(!cache.stats().watcher_mode, "polling mode for this scenario");

        // A new baseline-divergent file appears on disk. A ReuseOnly refresh with NO dirty paths
        // must leave the overlay untouched (no full rescan) — B.bsl stays absent.
        let file_b = workspace.join("B.bsl");
        fs::write(&file_b, "Процедура НоваяБ()\nКонецПроцедуры").unwrap();
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, false)
            .unwrap();
        let overlay = cache.snapshot();
        assert_eq!(
            overlay.lexical_documents.len(),
            1,
            "polling ReuseOnly must not re-scan the tree"
        );
        assert_eq!(overlay.lexical_documents[0].symbol_name, "ИзменённаяА");

        // A marked dirty path IS still picked up by the cheap incremental arm.
        cache.mark_dirty_path(key("B.bsl"));
        cache
            .refresh_with_manifest(&manifest, &single_root(workspace), None, 32, &store, false)
            .unwrap();
        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 2, "the dirty path is applied incrementally");
        let mut names: Vec<String> =
            overlay.lexical_documents.iter().map(|doc| doc.symbol_name.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["ИзменённаяА".to_owned(), "НоваяБ".to_owned()]);
    }
}
