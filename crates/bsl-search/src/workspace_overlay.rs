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
    /// See [`ScannedFiles::unreadable`]: how much of the tree the planning scan could not read.
    scan_unreadable: usize,
    /// See [`ScannedFiles::canonical_fallbacks`]: files whose physical spelling is unknown.
    scan_canonical_fallbacks: usize,
    /// Every key the planning scan saw, whether or not it produced a planned entry. Publication
    /// needs the distinction: a seen key without an entry was PROVEN baseline-equal, an unseen key
    /// was proven nothing.
    seen_keys: HashSet<FileKey>,
    /// Seen files whose read failed during planning: proven present, contents unknown.
    read_failures: HashSet<FileKey>,
    /// Seen files the planning phase skipped by a persisted-row hit — never read. The publish
    /// must not consume a live dirty mark for such a key: planning runs off-lock and cannot
    /// see the marks, and a mark is positive evidence the row must not have been trusted.
    gate_skipped: HashSet<FileKey>,
}

impl RefreshPlan {
    /// Whether the planning scan may speak for the whole tree — same verdict as
    /// [`ScannedFiles::clean`]. A caller reporting the warmup outcome must not call an unclean
    /// plan's emptiness "no local diffs": the scan may simply not have seen the diffs.
    pub fn scan_is_clean(&self) -> bool {
        self.scan_unreadable == 0 && self.scan_canonical_fallbacks == 0
    }

    /// How much of the tree the planning scan could not read (see
    /// [`project_model::SourceSet::unreadable`]).
    pub fn scan_unreadable(&self) -> usize {
        self.scan_unreadable
    }

    /// How many files the planning scan walked without a physical spelling (see
    /// [`project_model::SourceSet::canonical_fallbacks`]).
    pub fn scan_canonical_fallbacks(&self) -> usize {
        self.scan_canonical_fallbacks
    }

    /// How many seen files the planning phase failed to read: proven present, contents unknown.
    pub fn read_failure_count(&self) -> usize {
        self.read_failures.len()
    }

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
    /// The last full publication ran over a scan that could not vouch for the whole tree, so its
    /// removals were withheld and only a future CLEAN full scan can catch up. The dispatchers
    /// honour it by taking the full-scan arm even in watcher mode; a clean full publication
    /// clears it. (A read failure of a seen file is NOT this: the file list was complete, and the
    /// per-key dirty mark already drives the retry.)
    full_rescan_pending: bool,
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

/// The manifest baseline for a dirty-path refresh: the published fingerprints plus the store whose
/// persisted fingerprint rows the refresh must keep truthful. Bundled so
/// [`WorkspaceOverlayCache::refresh_dirty_paths_from_manifest`] stays within the argument-count
/// lint.
struct ManifestBaseline<'a> {
    fingerprints: &'a HashMap<FileKey, String>,
    store: &'a Store,
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
        // The withheld removals belonged to the state just discarded; the next build starts
        // from nothing and owes no catch-up scan for it.
        self.full_rescan_pending = false;
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
        // The caller has PROVEN the store equals the disk, which is exactly what the pending
        // rescan existed to re-establish.
        self.full_rescan_pending = false;
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

    /// Drop a key's overlay entry for a deletion PROVEN by the caller (its store rows are
    /// already gone). The point refresh cannot be trusted to settle this one: when the whole
    /// root vanished together with the file, it reads the dead root as "unreachable, retry"
    /// and would leave a ghost entry serving hits forever. The caller keeps the dirty mark, so
    /// if the deletion event lied and the file is alive, the next point pass republishes it.
    pub fn remove_known_deleted(&mut self, key: &FileKey) {
        self.entries.remove(key);
        self.hidden_paths.remove(key);
    }

    /// Whether the last full publication ran over an incomplete scan and withheld its removals,
    /// leaving the overlay waiting for a clean full scan to catch up. The dispatchers already
    /// honour this on the next cold-scan-allowed refresh; the accessor is for a caller deciding
    /// whether to drive one.
    pub fn needs_full_rescan(&self) -> bool {
        self.full_rescan_pending
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
                    ManifestBaseline { fingerprints: &manifest_fingerprints, store },
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
            if !self.initialized || !self.watcher_mode || self.full_rescan_pending {
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
                    ManifestBaseline { fingerprints: manifest_fingerprints, store },
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
                ManifestBaseline { fingerprints: manifest_fingerprints, store },
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
            if !self.initialized || !self.watcher_mode || self.full_rescan_pending {
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
        let scanned = scan_workspace_files(roots);
        self.full_refresh_scanned(baseline_files, scanned, embedder, batch_size, hash_mode)
    }

    fn full_refresh_scanned(
        &mut self,
        baseline_files: &HashMap<FileKey, Vec<u8>>,
        scanned: ScannedFiles,
        embedder: Option<&Embedder>,
        batch_size: usize,
        hash_mode: BaselineHashMode,
    ) -> Result<(), SearchError> {
        let scan_is_clean = scanned.clean();
        let workspace_files = scanned.files;
        let mut seen_keys = HashSet::new();
        let mut hidden_paths = HashSet::new();
        let mut read_failures: HashSet<FileKey> = HashSet::new();

        for file in workspace_files {
            seen_keys.insert(file.key.clone());
            let baseline_hash = baseline_files.get(&file.key);

            let mut should_remove_cached_entry = false;
            let key_is_marked = self.dirty_paths.contains_key(&file.key);
            if let Some(entry) = self.entries.get_mut(&file.key) {
                // A marked key skips the equal-fingerprint gate: the mark is positive evidence
                // the fingerprint must not be trusted (an edit can leave (len, mtime,
                // canonical) unchanged), so the file is re-read below.
                if !key_is_marked && entry.fingerprint == file.fingerprint {
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
                Err(_) => {
                    // Proven present, contents unknown: the key must stay dirty so a later
                    // refresh retries it.
                    read_failures.insert(file.key.clone());
                    continue;
                }
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

        if scan_is_clean {
            self.entries.retain(|key, _| seen_keys.contains(key));
            for key in baseline_files.keys() {
                if !seen_keys.contains(key) {
                    hidden_paths.insert(key.clone());
                }
            }
            // A failed read proves nothing about the baseline, so the key's prior hiding
            // survives the whole-replace — un-hiding would serve two versions at once.
            for key in &read_failures {
                if self.hidden_paths.contains(key) {
                    hidden_paths.insert(key.clone());
                }
            }
            self.hidden_paths = hidden_paths;
        } else {
            self.merge_partial_hidden(&seen_keys, &read_failures, &hidden_paths);
        }
        self.finish_full_publication(scan_is_clean, &seen_keys, &read_failures);
        Ok(())
    }

    /// The hidden-path merge of an unclean full publication: only a SEEN key may change its
    /// hiding — the scan proved something about it — while an unseen key keeps its prior state,
    /// because absence from an incomplete scan proves nothing. A seen-but-unread key is skipped
    /// too: un-hiding on a failed read would serve the baseline and the stale overlay both.
    fn merge_partial_hidden(
        &mut self,
        seen_keys: &HashSet<FileKey>,
        read_failures: &HashSet<FileKey>,
        fresh_hidden: &HashSet<FileKey>,
    ) {
        for key in seen_keys {
            if read_failures.contains(key) {
                continue;
            }
            if fresh_hidden.contains(key) {
                self.hidden_paths.insert(key.clone());
            } else {
                self.hidden_paths.remove(key);
            }
        }
    }

    /// The shared tail of an in-place full publication: dirty marks are cleared ONLY for keys the
    /// pass actually processed — a key it failed to read goes into the bounded retry budget, and
    /// a key an unclean scan never saw stays dirty because nothing about it was proven — and the
    /// rescan flag records whether removals were withheld. Each failed read is warned about HERE,
    /// by the publication itself: the budget helper only warns when the budget runs out, and a
    /// full publication that could not read a seen file must be visible in the log at once.
    fn finish_full_publication(
        &mut self,
        scan_is_clean: bool,
        seen_keys: &HashSet<FileKey>,
        read_failures: &HashSet<FileKey>,
    ) {
        if scan_is_clean {
            self.dirty_paths.retain(|key, _| read_failures.contains(key));
        } else {
            self.dirty_paths
                .retain(|key, _| !seen_keys.contains(key) || read_failures.contains(key));
        }
        // A successful publication of a key ends its failure streak: the budget counts
        // CONSECUTIVE failures, and without this reset unrelated failures spread over time
        // would add up to a drop.
        for key in seen_keys {
            if !read_failures.contains(key) {
                self.dirty_failures.remove(key);
            }
        }
        for key in read_failures {
            tracing::warn!(
                root = %key.root_id,
                path = %key.path,
                "full overlay refresh could not read a seen file; keeping its previous version \
                 and retrying"
            );
            let prior_failures = self.dirty_failures.remove(key).unwrap_or(0);
            self.dirty_paths.remove(key);
            self.retain_dirty_after_failure(key.clone(), prior_failures, "read failed");
        }
        self.full_rescan_pending = !scan_is_clean;
    }

    /// Remove a point-refresh key that is PROVABLY gone: the entry goes, and the baseline copy
    /// (when there is one) is hidden, exactly as a clean full scan would settle it.
    fn remove_point_entry(&mut self, key: FileKey, has_baseline: bool) {
        self.entries.remove(&key);
        if has_baseline {
            self.hidden_paths.insert(key);
        } else {
            self.hidden_paths.remove(&key);
        }
    }

    /// Retract one key's persisted fingerprint row: its "verified against the manifest" claim
    /// did not survive whatever the caller just observed. Returns whether the retraction
    /// LANDED — a caller about to consume the dirty mark must keep it on failure, or the stale
    /// row outlives the process with nothing left to retry it.
    fn retract_fingerprint_row(store: &Store, key: &FileKey) -> bool {
        match store.delete_overlay_fingerprint_entries(std::slice::from_ref(key)) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!("failed to retract overlay fingerprint row: {error}");
                false
            }
        }
    }

    /// Persist the fingerprint rows a full publication verified. A row asserts "this file was
    /// verified against the manifest", so the table must end up holding exactly what THIS pass
    /// proved: the replace-save runs even over an unclean scan — an unseen file may have changed
    /// at the same size and mtime, and its surviving row would skip the re-read after a restart;
    /// losing a row costs one extra read on the next clean pass, a false hit costs a lost edit —
    /// and a read-failed key's row is retracted explicitly, because with nothing else updated on
    /// a clean scan the emptiness guard would keep the whole stale table.
    fn persist_fingerprint_rows(
        store: &Store,
        snapshot_id: &str,
        updated: &HashMap<FileKey, crate::store::PersistedFingerprint>,
        scan_is_clean: bool,
        read_failures: &HashSet<FileKey>,
    ) {
        if !updated.is_empty() || !scan_is_clean {
            if let Err(error) = store.save_overlay_fingerprint_cache(snapshot_id, updated) {
                tracing::warn!("failed to persist overlay fingerprint cache: {error}");
            }
        }
        if !read_failures.is_empty() {
            let keys: Vec<FileKey> = read_failures.iter().cloned().collect();
            if let Err(error) = store.delete_overlay_fingerprint_entries(&keys) {
                tracing::warn!("failed to retract overlay fingerprint rows: {error}");
            }
        }
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
        // [`MAX_DIRTY_REFRESH_FAILURES`] — rather than being silently dropped. The re-mark is
        // settled AT the failure: a later error in the same batch aborts through `?`, and the
        // caller has already drained the dirty set, so a deferred re-mark would be lost.
        for key in dirty_keys {
            // Removing the count here clears it on success (the common path) and hands the prior
            // value to `retain_dirty_after_failure` on failure, keeping the streak consecutive.
            let prior_failures = self.dirty_failures.remove(&key).unwrap_or(0);
            let baseline_hash = baseline_files.get(&key);
            // A key whose root is no longer registered resolves to nothing; that is a change of
            // composition, not a filesystem error, and it settles like a deletion.
            let Some(abs_path) = roots.resolve(&key) else {
                self.remove_point_entry(key, baseline_hash.is_some());
                continue;
            };

            let metadata = match std::fs::metadata(&abs_path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    if point_target_is_absent(&error, &abs_path) && root_is_reachable(roots, &key) {
                        self.remove_point_entry(key, baseline_hash.is_some());
                    } else {
                        self.retain_dirty_after_failure(key, prior_failures, "stat failed");
                    }
                    continue;
                }
            };
            let fingerprint = FileFingerprint {
                len: metadata.len(),
                modified: metadata.modified().ok(),
                canonical: crate::workspace_roots::canonical_spelling(&abs_path),
            };
            // A live target that is not a source file is positive evidence the SOURCE file is
            // gone: the walk never yields a file whose two spellings disagree on role, so a
            // clean full scan would remove this entry — the point path settles it the same way.
            // The same goes for a non-regular target spelled `.bsl` (a directory, a FIFO): the
            // walk yields regular files only, and reading a FIFO would even block.
            if !metadata.is_file()
                || project_model::file_role(&fingerprint.canonical)
                    != project_model::FileRole::Source
            {
                self.remove_point_entry(key, baseline_hash.is_some());
                continue;
            }

            // No equal-fingerprint fast path here: every key in this loop carries a dirty
            // mark, and the mark is positive evidence the fingerprint must not be trusted —
            // an edit at unchanged (len, mtime, canonical) would otherwise be consumed
            // silently. The price is one read per honest no-op watcher ping.
            let content = match std::fs::read_to_string(&abs_path) {
                Ok(content) => content,
                Err(_) => {
                    self.retain_dirty_after_failure(key, prior_failures, "read failed");
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
        let scanned = scan_workspace_files(roots);
        self.full_refresh_from_manifest_scanned(
            manifest_fingerprints,
            scanned,
            embedder,
            batch_size,
            store,
        )
    }

    fn full_refresh_from_manifest_scanned(
        &mut self,
        manifest_fingerprints: &HashMap<FileKey, String>,
        scanned: ScannedFiles,
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

        let scan_is_clean = scanned.clean();
        let workspace_files = scanned.files;
        let mut seen_keys = HashSet::new();
        let mut hidden_paths = HashSet::new();
        let mut updated_persisted = HashMap::new();
        let mut read_failures: HashSet<FileKey> = HashSet::new();

        for file in &workspace_files {
            seen_keys.insert(file.key.clone());
            let baseline_fingerprint = manifest_fingerprints.get(&file.key);

            let mut should_remove_cached_entry = false;
            let key_is_marked = self.dirty_paths.contains_key(&file.key);
            if let Some(entry) = self.entries.get_mut(&file.key) {
                // A marked key skips the equal-fingerprint gate: the mark is positive evidence
                // the fingerprint must not be trusted (an edit can leave (len, mtime,
                // canonical) unchanged), so the file is re-read below.
                if !key_is_marked && entry.fingerprint == file.fingerprint {
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
                if !key_is_marked
                    && cached.file_size == file.fingerprint.len
                    && fingerprint_mtime_matches(file.fingerprint.modified, cached)
                    && fingerprint_canonical_matches(&file.fingerprint, cached)
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
                Err(_) => {
                    // Proven present, contents unknown: the key must stay dirty so a later
                    // refresh retries it.
                    read_failures.insert(file.key.clone());
                    continue;
                }
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
                        canonical: file.fingerprint.canonical.to_string_lossy().into_owned(),
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

        if scan_is_clean {
            self.entries.retain(|key, _| seen_keys.contains(key));
            for key in manifest_fingerprints.keys() {
                if !seen_keys.contains(key) {
                    hidden_paths.insert(key.clone());
                }
            }
            // A failed read proves nothing about the baseline, so the key's prior hiding
            // survives the whole-replace — un-hiding would serve two versions at once.
            for key in &read_failures {
                if self.hidden_paths.contains(key) {
                    hidden_paths.insert(key.clone());
                }
            }
            self.hidden_paths = hidden_paths;
        } else {
            self.merge_partial_hidden(&seen_keys, &read_failures, &hidden_paths);
        }
        self.finish_full_publication(scan_is_clean, &seen_keys, &read_failures);

        Self::persist_fingerprint_rows(
            store,
            &manifest_snapshot_id,
            &updated_persisted,
            scan_is_clean,
            &read_failures,
        );

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
        let scanned = scan_workspace_files(roots);
        Self::plan_full_refresh_from_manifest_scanned(
            manifest_fingerprints,
            scanned,
            store,
            warm_embeddings,
            graph_context,
        )
    }

    fn plan_full_refresh_from_manifest_scanned(
        manifest_fingerprints: &HashMap<FileKey, String>,
        scanned: ScannedFiles,
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

        let scan_unreadable = scanned.unreadable;
        let scan_canonical_fallbacks = scanned.canonical_fallbacks;
        let workspace_files = scanned.files;
        let mut seen_keys = HashSet::new();
        let mut hidden_paths = HashSet::new();
        let mut updated_persisted = HashMap::new();
        let mut entries: Vec<(FileKey, PlannedEntry)> = Vec::new();
        let mut missing_embeddings: HashMap<String, String> = HashMap::new();
        let mut read_failures: HashSet<FileKey> = HashSet::new();
        let mut gate_skipped: HashSet<FileKey> = HashSet::new();

        for file in &workspace_files {
            seen_keys.insert(file.key.clone());
            let baseline_fingerprint = manifest_fingerprints.get(&file.key);

            if let Some(cached) = persisted.get(&file.key) {
                if cached.file_size == file.fingerprint.len
                    && fingerprint_mtime_matches(file.fingerprint.modified, cached)
                    && fingerprint_canonical_matches(&file.fingerprint, cached)
                {
                    updated_persisted.insert(file.key.clone(), cached.clone());

                    if baseline_fingerprint
                        .is_some_and(|stored| stored == &cached.content_fingerprint)
                    {
                        gate_skipped.insert(file.key.clone());
                        continue;
                    }
                }
            }

            let content = match std::fs::read_to_string(&file.abs_path) {
                Ok(content) => content,
                Err(_) => {
                    // A seen-but-unread file is proven present with unknown contents; the
                    // publication keeps its prior state and retries, so the plan must carry the
                    // failure rather than silently plan nothing for the key.
                    read_failures.insert(file.key.clone());
                    continue;
                }
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
                        canonical: file.fingerprint.canonical.to_string_lossy().into_owned(),
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

        // Absence from an incomplete scan proves nothing, so only a clean scan may read "not
        // seen" as "deleted from disk" and hide the baseline copy.
        if scan_unreadable == 0 && scan_canonical_fallbacks == 0 {
            for key in manifest_fingerprints.keys() {
                if !seen_keys.contains(key) {
                    hidden_paths.insert(key.clone());
                }
            }
        }

        Ok(RefreshPlan {
            snapshot_id,
            entries,
            hidden_paths,
            updated_persisted,
            missing_embeddings,
            scan_unreadable,
            scan_canonical_fallbacks,
            seen_keys,
            read_failures,
            gate_skipped,
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
        let scan_is_clean = plan.scan_is_clean();
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

        if scan_is_clean {
            // The whole-replace must still carry over what the plan could not read: a failed
            // read proves nothing, so the prior entry keeps serving and its hiding stays —
            // dropping either would serve the baseline version of contents nobody verified.
            for key in &plan.read_failures {
                if let Some(prior) = self.entries.remove(key) {
                    entries.insert(key.clone(), prior);
                }
            }
            let mut hidden_paths = plan.hidden_paths;
            for key in &plan.read_failures {
                if self.hidden_paths.contains(key) {
                    hidden_paths.insert(key.clone());
                }
            }
            self.entries = entries;
            self.hidden_paths = hidden_paths;
        } else {
            // The plan may not speak for what its scan did not see, so it publishes as a merge:
            // planned entries land, a seen key the plan proved baseline-equal (no entry, no read
            // failure) is removed and un-hidden, and everything unseen keeps its prior state.
            let planned_keys: HashSet<FileKey> = entries.keys().cloned().collect();
            for (key, entry) in entries {
                self.entries.insert(key, entry);
            }
            for key in &plan.seen_keys {
                if plan.read_failures.contains(key) {
                    continue;
                }
                if !planned_keys.contains(key) {
                    self.entries.remove(key);
                }
                if plan.hidden_paths.contains(key) {
                    self.hidden_paths.insert(key.clone());
                } else {
                    self.hidden_paths.remove(key);
                }
            }
        }
        // Clear only the dirty flags this full refresh superseded. A path is superseded only if it
        // has not been re-marked since the snapshot (same sequence): a watcher edit that landed
        // during the lock-free embed bumps the sequence (or adds a new path), so it survives and a
        // later refresh re-embeds it. A blanket clear would silently drop an edit made mid-pass.
        // On top of the sequence check, only a PROCESSED key is superseded: a key the pass failed
        // to read stays dirty for the retry, and a key an unclean scan never saw stays dirty
        // because nothing about it was proven.
        for (key, seq) in dirty_before {
            if self.dirty_paths.get(key) != Some(seq) {
                continue;
            }
            // A gate-skipped key was never read: its mark (set before the plan was even
            // built) is evidence the trusted row was stale, and consuming it here would lose
            // the edit with nothing left to deliver it. The mark survives, the stale-row
            // filter below retracts the row, and the next point pass re-reads the file.
            let processed = !plan.read_failures.contains(key)
                && !plan.gate_skipped.contains(key)
                && (scan_is_clean || plan.seen_keys.contains(key));
            if processed {
                self.dirty_paths.remove(key);
            }
        }
        self.full_rescan_pending = !scan_is_clean;
        self.initialized = true;

        // A successful publication of a key ends its failure streak (the budget counts
        // CONSECUTIVE failures), while a failed read below re-enters it.
        for key in &plan.seen_keys {
            if !plan.read_failures.contains(key) {
                self.dirty_failures.remove(key);
            }
        }
        // The warn belongs HERE and not in the planning phase: phase A publishes nothing, and
        // the message must stand next to the fact that an incomplete result went live.
        for key in &plan.read_failures {
            tracing::warn!(
                root = %key.root_id,
                path = %key.path,
                "published overlay plan could not read a seen file; keeping its previous \
                 version and retrying"
            );
            let prior_failures = self.dirty_failures.remove(key).unwrap_or(0);
            self.dirty_paths.remove(key);
            self.retain_dirty_after_failure(key.clone(), prior_failures, "read failed");
        }

        // The plan's row snapshot is as old as its phase A; a key whose dirty mark is alive NOW
        // was re-marked or failed since, and its stale row must not go back in as "verified" —
        // the mark dies with the process, the row would survive the restart and contradict it.
        let mut updated_persisted = plan.updated_persisted;
        let mut stale_rows: Vec<FileKey> = Vec::new();
        updated_persisted.retain(|key, _| {
            let live = self.dirty_paths.contains_key(key);
            if live {
                stale_rows.push(key.clone());
            }
            !live
        });
        Self::persist_fingerprint_rows(
            store,
            &plan.snapshot_id,
            &updated_persisted,
            scan_is_clean,
            &plan.read_failures,
        );
        // The filtered rows must be RETRACTED, not merely unsaved: with the filtered map empty
        // the guarded replace-save does not run at all, and the stale row would survive intact.
        if !stale_rows.is_empty() {
            if let Err(error) = store.delete_overlay_fingerprint_entries(&stale_rows) {
                tracing::warn!("failed to retract overlay fingerprint rows: {error}");
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
        baseline: ManifestBaseline<'_>,
        roots: &WorkspaceRoots,
        embedder: Option<&Embedder>,
        batch_size: usize,
        snapshots: &HashMap<FileKey, ModuleSnapshot>,
    ) -> Result<(), SearchError> {
        let ManifestBaseline { fingerprints: manifest_fingerprints, store } = baseline;
        // A path whose stat/read transiently fails is re-marked dirty (carrying its
        // consecutive-failure count) so the next refresh retries it — bounded by
        // [`MAX_DIRTY_REFRESH_FAILURES`] — rather than being silently dropped. Both obligations
        // of a failure — the retry re-mark and the retraction of the key's persisted fingerprint
        // row (it claims "verified", the mark dies with the process, the row would survive the
        // restart) — are settled AT the failure: a later error in the same batch aborts through
        // `?`, and the caller has already drained the dirty set.
        for key in dirty_keys {
            // Removing the count here clears it on success (the common path) and hands the prior
            // value to `retain_dirty_after_failure` on failure, keeping the streak consecutive.
            let prior_failures = self.dirty_failures.remove(&key).unwrap_or(0);
            let baseline_fingerprint = manifest_fingerprints.get(&key);
            // A key whose root is no longer registered resolves to nothing; that is a change of
            // composition, not a filesystem error, and it settles like a deletion.
            let Some(abs_path) = roots.resolve(&key) else {
                if !Self::retract_fingerprint_row(store, &key) {
                    self.retain_dirty_after_failure(
                        key.clone(),
                        prior_failures,
                        "row retraction failed",
                    );
                }
                self.remove_point_entry(key, baseline_fingerprint.is_some());
                continue;
            };

            let metadata = match std::fs::metadata(&abs_path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    // Whichever way this settles, the row's "verified" claim did not survive
                    // the failed stat.
                    let retracted = Self::retract_fingerprint_row(store, &key);
                    if point_target_is_absent(&error, &abs_path) && root_is_reachable(roots, &key) {
                        if !retracted {
                            self.retain_dirty_after_failure(
                                key.clone(),
                                prior_failures,
                                "row retraction failed",
                            );
                        }
                        self.remove_point_entry(key, baseline_fingerprint.is_some());
                    } else {
                        self.retain_dirty_after_failure(key, prior_failures, "stat failed");
                    }
                    continue;
                }
            };
            let fingerprint = FileFingerprint {
                len: metadata.len(),
                modified: metadata.modified().ok(),
                canonical: crate::workspace_roots::canonical_spelling(&abs_path),
            };
            // A live target that is not a source file is positive evidence the SOURCE file is
            // gone (see the raw twin above) — as is a non-regular target spelled `.bsl`; the
            // row's "verified" claim goes with the entry.
            if !metadata.is_file()
                || project_model::file_role(&fingerprint.canonical)
                    != project_model::FileRole::Source
            {
                if !Self::retract_fingerprint_row(store, &key) {
                    self.retain_dirty_after_failure(
                        key.clone(),
                        prior_failures,
                        "row retraction failed",
                    );
                }
                self.remove_point_entry(key, baseline_fingerprint.is_some());
                continue;
            }

            // No equal-fingerprint fast path here: every key in this loop carries a dirty
            // mark, and the mark is positive evidence the fingerprint must not be trusted —
            // an edit at unchanged (len, mtime, canonical) would otherwise be consumed
            // silently. The read path below also settles the persisted row either way.
            let content = match std::fs::read_to_string(&abs_path) {
                Ok(content) => content,
                Err(_) => {
                    // The mark is retained either way, so a failed retraction is retried too.
                    let _ = Self::retract_fingerprint_row(store, &key);
                    self.retain_dirty_after_failure(key, prior_failures, "read failed");
                    continue;
                }
            };
            // The successful read produced FRESHER knowledge than the persisted row, so its
            // "verified" claim no longer stands whatever the branches below decide: at an
            // unchanged (len, mtime, canonical) the old row would suppress this very result
            // after a restart. A failed retraction keeps the mark for a retried one.
            if !Self::retract_fingerprint_row(store, &key) {
                self.retain_dirty_after_failure(
                    key.clone(),
                    prior_failures,
                    "row retraction failed",
                );
            }
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
    /// The physical spelling of what was actually read. Without it a link retargeted onto a
    /// file with the same `(len, mtime)` keeps passing for unchanged, and the old target is
    /// served forever; only the identical target may stay invisible.
    canonical: PathBuf,
}

/// Whether a persisted row's physical spelling matches the file just stat'ed. An empty stored
/// spelling is a row from before the column existed: never a match, so the file is re-read and
/// the re-save fills the spelling in — old rows heal themselves. Both sides compare through the
/// same lossy conversion, so a non-UTF-8 path stays equal to itself.
fn fingerprint_canonical_matches(
    fingerprint: &FileFingerprint,
    cached: &crate::store::PersistedFingerprint,
) -> bool {
    if cached.canonical.is_empty() || cached.canonical.contains('\u{FFFD}') {
        // Empty: a row from before the column existed. Replacement characters: the lossy
        // conversion collapses every invalid byte onto one char, so two DIFFERENT non-UTF-8
        // targets could pass for each other — such a row is never trusted, only re-read.
        return false;
    }
    *cached.canonical == *fingerprint.canonical.to_string_lossy()
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

/// Everything one full scan of the registered roots saw: the files under their
/// store keys, plus how much of the tree the walk could NOT vouch for. The
/// counters travel with the files because they qualify them — a consumer that
/// reconciles a store against `files` must know whether the list may speak for
/// the whole tree.
#[derive(Debug, Default)]
struct ScannedFiles {
    files: Vec<WorkspaceFileState>,
    /// See [`project_model::SourceSet::unreadable`]: coverage is incomplete.
    unreadable: usize,
    /// See [`project_model::SourceSet::loops`]: benign, coverage complete.
    loops: usize,
    /// See [`project_model::SourceSet::dangling`]: benign, coverage complete.
    dangling: usize,
    /// See [`project_model::SourceSet::canonical_fallbacks`]: identity is degraded.
    canonical_fallbacks: usize,
}

impl ScannedFiles {
    /// Whether `files` may speak for the whole tree — the same verdict as
    /// [`project_model::SourceSet::clean`]: loops and dangling links leave
    /// coverage complete, so they deliberately do not count.
    fn clean(&self) -> bool {
        self.unreadable == 0 && self.canonical_fallbacks == 0
    }
}

/// Every source file of every registered root, each under the key it is stored
/// by, from the one shared workspace walk.
fn scan_workspace_files(roots: &WorkspaceRoots) -> ScannedFiles {
    let declared: Vec<PathBuf> = roots.entries().map(|(_, root)| root.to_path_buf()).collect();
    let set = project_model::SourceSet::scan(&declared);
    let scanned = scanned_files_from(roots, &set);
    if !scanned.clean() {
        tracing::warn!(
            unreadable = scanned.unreadable,
            canonical_fallbacks = scanned.canonical_fallbacks,
            "workspace overlay scan did not cover the whole tree"
        );
    }
    tracing::debug!(
        files = scanned.files.len(),
        unreadable = scanned.unreadable,
        loops = scanned.loops,
        dangling = scanned.dangling,
        canonical_fallbacks = scanned.canonical_fallbacks,
        "workspace overlay scan"
    );
    scanned
}

/// Whether a failed `metadata` call PROVES the point-refresh target is gone. `NotFound` and
/// `NotADirectory` (a parent directory replaced by a file) are the kernel saying so outright; a
/// symlink cycle anywhere in the path is proven by walking the chain — the shared walk
/// classifies such a file as a benign loop and never yields it, so the point answer must agree
/// with the full scan. Anything else — permissions, I/O, or a merely LONG link chain behind
/// which a live file sits (a bare `ELOOP` proves nothing) — reads as a live file the caller
/// keeps and retries.
fn point_target_is_absent(error: &std::io::Error, path: &Path) -> bool {
    matches!(error.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory)
        || project_model::path_crosses_a_link_cycle(path)
}

/// Whether the registered root a key belongs to exists at all. The full walk classifies a
/// failed ROOT as incomplete coverage, never as deletions of its files — so a point refresh
/// that got `NotFound` under an unreachable root must not read it as the file's removal either.
fn root_is_reachable(roots: &WorkspaceRoots, key: &FileKey) -> bool {
    roots
        .entries()
        .find(|(id, _)| *id == key.root_id)
        // Follow the root if it is a link: the inode of the link itself proves nothing about
        // the tree behind it, and a dangling or cycled root link is the walk's "unreadable
        // root", not the file's deletion.
        .is_some_and(|(_, root)| std::fs::metadata(root).is_ok())
}

/// The pure projection of a walk result into overlay terms. Roots may nest, so
/// a file reached from two of them is enumerated twice and attributed to the
/// same owner both times; de-duplicating by key is what keeps one file one
/// entry. A file whose key cannot be built (a root that is itself a file) is
/// dropped, as it always was.
fn scanned_files_from(roots: &WorkspaceRoots, set: &project_model::SourceSet) -> ScannedFiles {
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
        files.push(WorkspaceFileState {
            key,
            abs_path: file.walked.clone(),
            fingerprint: FileFingerprint {
                len: file.metadata.len(),
                modified: file.metadata.modified().ok(),
                canonical: file.canonical.clone(),
            },
        });
    }
    ScannedFiles {
        files,
        unreadable: set.unreadable,
        loops: set.loops,
        dangling: set.dangling,
        canonical_fallbacks: set.canonical_fallbacks,
    }
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

    /// A dirty path whose read transiently fails (here invalid UTF-8 in a regular `.bsl`, so
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

        fs::write(workspace.join("Broken.bsl"), [0xff, 0xfe]).unwrap();
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

        // Invalid UTF-8 in a regular `.bsl`: `metadata` succeeds, `read_to_string` always fails.
        fs::write(workspace.join("Broken.bsl"), [0xff, 0xfe]).unwrap();
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

        // Invalid UTF-8 in a regular `.bsl`: `metadata` succeeds, `read_to_string` always fails, so
        // one healthy reindex records a genuine per-path failure (budget = 1). That seeded count is
        // what the store-error reindex below must leave untouched.
        fs::write(workspace.join("Broken.bsl"), [0xff, 0xfe]).unwrap();
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

    /// A directory symlink inside a root is part of the workspace universe: the
    /// graph walk follows directory links, and an overlay scan that does not
    /// silently serves a different set of files than every other consumer.
    #[cfg(unix)]
    #[test]
    fn a_module_behind_a_directory_link_reaches_the_overlay() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let workspace = dir.path();
        let real = outside.path().join("shared");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("M.bsl"), "Процедура ЗаКаталожнойСсылкой()\nКонецПроцедуры").unwrap();
        std::os::unix::fs::symlink(&real, workspace.join("linked")).unwrap();

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let roots = single_root(workspace);
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        let documents = cache.snapshot().lexical_documents;

        assert_eq!(documents.len(), 1, "the linked subtree belongs to the universe");
        assert_eq!(documents[0].symbol_name, "ЗаКаталожнойСсылкой");
    }

    /// A hand-built scan result: the files the walk "saw", each stat'ed from
    /// disk, plus the completeness counters under test.
    fn scanned_with(
        seen: &[(&FileKey, &Path)],
        unreadable: usize,
        canonical_fallbacks: usize,
    ) -> super::ScannedFiles {
        let files = seen
            .iter()
            .map(|(key, path)| {
                let metadata = fs::metadata(path).unwrap();
                super::WorkspaceFileState {
                    key: (*key).clone(),
                    abs_path: path.to_path_buf(),
                    fingerprint: super::FileFingerprint {
                        len: metadata.len(),
                        modified: metadata.modified().ok(),
                        canonical: crate::workspace_roots::canonical_spelling(path),
                    },
                }
            })
            .collect();
        super::ScannedFiles { files, unreadable, loops: 0, dangling: 0, canonical_fallbacks }
    }

    /// A workspace with one baseline-divergent file (an overlay entry hiding its
    /// baseline) and one baseline-equal file (no entry, nothing hidden), fully
    /// refreshed once so the cache holds that state.
    fn cache_with_edited_and_clean(
        workspace: &Path,
    ) -> (WorkspaceOverlayCache, HashMap<FileKey, Vec<u8>>) {
        fs::write(workspace.join("Edited.bsl"), "Процедура Изменённая()\nКонецПроцедуры").unwrap();
        let clean_content = "Процедура Прежняя()\nКонецПроцедуры";
        fs::write(workspace.join("Clean.bsl"), clean_content).unwrap();
        let baseline = HashMap::from([
            (key("Edited.bsl"), b"baseline-differs".to_vec()),
            (
                key("Clean.bsl"),
                super::compute_file_hash(clean_content, BaselineHashMode::RawFileBytes),
            ),
        ]);
        let mut cache = WorkspaceOverlayCache::default();
        cache
            .full_refresh(
                &baseline,
                &single_root(workspace),
                None,
                32,
                BaselineHashMode::RawFileBytes,
            )
            .unwrap();
        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 1, "only the edited file diverges");
        assert!(overlay.hidden_paths.contains(&key("Edited.bsl")));
        assert!(!overlay.hidden_paths.contains(&key("Clean.bsl")));
        (cache, baseline)
    }

    /// An unclean scan proves nothing about what it did not see: the unseen
    /// entry survives, its baseline stays hidden, and the unseen baseline-equal
    /// file does not become hidden — absence is not evidence of deletion.
    #[test]
    fn an_unclean_scan_keeps_the_unseen_entry_and_hides_nothing_new() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let (mut cache, baseline) = cache_with_edited_and_clean(workspace);

        cache
            .full_refresh_scanned(
                &baseline,
                scanned_with(&[], 1, 0),
                None,
                32,
                BaselineHashMode::RawFileBytes,
            )
            .unwrap();
        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 1, "the unseen entry survives");
        assert!(overlay.hidden_paths.contains(&key("Edited.bsl")), "its baseline stays hidden");
        assert!(
            !overlay.hidden_paths.contains(&key("Clean.bsl")),
            "an unseen live file must not become hidden"
        );
        assert!(cache.needs_full_rescan(), "withheld removals demand a clean rescan");
    }

    /// The same protection on the manifest-driven full refresh — an independent
    /// implementation of the same reconciliation.
    #[test]
    fn an_unclean_scan_keeps_the_unseen_entry_on_the_manifest_path() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let store = Store::open(&workspace.join("search.db")).unwrap();
        fs::write(workspace.join("Edited.bsl"), "Процедура Изменённая()\nКонецПроцедуры").unwrap();
        let clean_content = "Процедура Прежняя()\nКонецПроцедуры";
        fs::write(workspace.join("Clean.bsl"), clean_content).unwrap();
        let manifest = HashMap::from([
            (key("Edited.bsl"), "manifest-differs".to_owned()),
            (key("Clean.bsl"), super::fingerprint_content(clean_content, "Clean.bsl")),
        ]);
        let mut cache = WorkspaceOverlayCache::default();
        cache
            .full_refresh_from_manifest(&manifest, &single_root(workspace), None, 32, &store)
            .unwrap();
        assert_eq!(cache.snapshot().lexical_documents.len(), 1);
        assert!(cache.snapshot().hidden_paths.contains(&key("Edited.bsl")));

        cache
            .full_refresh_from_manifest_scanned(
                &manifest,
                scanned_with(&[], 1, 0),
                None,
                32,
                &store,
            )
            .unwrap();
        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 1, "the unseen entry survives");
        assert!(overlay.hidden_paths.contains(&key("Edited.bsl")), "its baseline stays hidden");
        assert!(
            !overlay.hidden_paths.contains(&key("Clean.bsl")),
            "an unseen live file must not become hidden"
        );
        assert!(cache.needs_full_rescan());
    }

    /// The planned path publishes through a merge when the scan was unclean: the
    /// unmatched prior entry and the prior hidings survive, while a seen key the
    /// plan proved baseline-equal is removed and un-hidden.
    #[test]
    fn an_unclean_plan_merges_instead_of_replacing() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let store = Store::open(&workspace.join("search.db")).unwrap();
        fs::write(workspace.join("Edited.bsl"), "Процедура Изменённая()\nКонецПроцедуры").unwrap();
        let returned = workspace.join("Returned.bsl");
        fs::write(&returned, "Процедура Вернулась()\nКонецПроцедуры").unwrap();
        let manifest = HashMap::from([
            (key("Edited.bsl"), "manifest-differs".to_owned()),
            (key("Returned.bsl"), "manifest-differs-too".to_owned()),
        ]);
        let roots = single_root(workspace);
        let mut cache = WorkspaceOverlayCache::default();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        cache.publish_plan(plan, HashMap::new(), &HashMap::new(), None, &store).unwrap();
        assert_eq!(cache.snapshot().lexical_documents.len(), 2, "both files diverge");

        // `Returned.bsl` goes back to its baseline; the unclean scan sees ONLY it.
        fs::write(&returned, "Процедура ПоБазлайну()\nКонецПроцедуры").unwrap();
        let manifest = HashMap::from([
            (key("Edited.bsl"), "manifest-differs".to_owned()),
            (
                key("Returned.bsl"),
                super::fingerprint_content(
                    "Процедура ПоБазлайну()\nКонецПроцедуры",
                    "Returned.bsl",
                ),
            ),
        ]);
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest_scanned(
            &manifest,
            scanned_with(&[(&key("Returned.bsl"), &returned)], 1, 0),
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        cache.publish_plan(plan, HashMap::new(), &HashMap::new(), None, &store).unwrap();
        let overlay = cache.snapshot();
        let names: Vec<&str> =
            overlay.lexical_documents.iter().map(|d| d.symbol_name.as_str()).collect();
        assert_eq!(names, vec!["Изменённая"], "unseen entry kept, baseline-equal seen key removed");
        assert!(
            overlay.hidden_paths.contains(&key("Edited.bsl")),
            "the unseen hiding survives the merge"
        );
        assert!(
            !overlay.hidden_paths.contains(&key("Returned.bsl")),
            "a seen key proven baseline-equal is un-hidden"
        );
        assert!(cache.needs_full_rescan());
    }

    /// A degraded identity (`canonical_fallbacks`) is the same "may not speak
    /// for the tree" verdict as an unreadable subtree.
    #[test]
    fn canonical_fallbacks_also_withhold_removals() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let (mut cache, baseline) = cache_with_edited_and_clean(workspace);

        cache
            .full_refresh_scanned(
                &baseline,
                scanned_with(&[], 0, 1),
                None,
                32,
                BaselineHashMode::RawFileBytes,
            )
            .unwrap();
        assert_eq!(cache.snapshot().lexical_documents.len(), 1, "the unseen entry survives");
        assert!(cache.needs_full_rescan());
    }

    /// Loops and dangling links leave coverage complete: a scan with only those
    /// is clean, so a genuinely-deleted file is removed and its baseline hidden,
    /// exactly as before.
    #[test]
    fn loops_and_dangling_links_do_not_withhold_removals() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let (mut cache, baseline) = cache_with_edited_and_clean(workspace);

        let benign = super::ScannedFiles {
            files: Vec::new(),
            unreadable: 0,
            loops: 2,
            dangling: 3,
            canonical_fallbacks: 0,
        };
        cache
            .full_refresh_scanned(&baseline, benign, None, 32, BaselineHashMode::RawFileBytes)
            .unwrap();
        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 0, "a clean scan removes the deleted entry");
        assert!(
            overlay.hidden_paths.contains(&key("Edited.bsl")),
            "the deleted file's baseline is hidden"
        );
        assert!(
            overlay.hidden_paths.contains(&key("Clean.bsl")),
            "every baseline key absent from a clean scan is hidden"
        );
        assert!(!cache.needs_full_rescan(), "a clean scan leaves nothing to catch up");
    }

    /// Positive evidence still acts during an unclean publication: a seen file
    /// back at its baseline is removed and un-hidden, a seen changed file is
    /// re-chunked.
    #[test]
    fn an_unclean_scan_still_applies_what_it_saw() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let returned = workspace.join("Returned.bsl");
        let changed = workspace.join("Changed.bsl");
        fs::write(&returned, "Процедура Вернулась()\nКонецПроцедуры").unwrap();
        fs::write(&changed, "Процедура Старая()\nКонецПроцедуры").unwrap();
        let baseline_content = "Процедура ПоБазлайну()\nКонецПроцедуры";
        let baseline = HashMap::from([
            (
                key("Returned.bsl"),
                super::compute_file_hash(baseline_content, BaselineHashMode::RawFileBytes),
            ),
            (key("Changed.bsl"), b"baseline-differs".to_vec()),
        ]);
        let mut cache = WorkspaceOverlayCache::default();
        cache
            .full_refresh(
                &baseline,
                &single_root(workspace),
                None,
                32,
                BaselineHashMode::RawFileBytes,
            )
            .unwrap();
        assert_eq!(cache.snapshot().lexical_documents.len(), 2, "both diverge at first");

        fs::write(&returned, baseline_content).unwrap();
        fs::write(&changed, "Процедура Новая()\nКонецПроцедуры").unwrap();
        cache
            .full_refresh_scanned(
                &baseline,
                scanned_with(
                    &[(&key("Returned.bsl"), &returned), (&key("Changed.bsl"), &changed)],
                    1,
                    0,
                ),
                None,
                32,
                BaselineHashMode::RawFileBytes,
            )
            .unwrap();
        let overlay = cache.snapshot();
        let names: Vec<&str> =
            overlay.lexical_documents.iter().map(|d| d.symbol_name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Новая"],
            "the returned file is removed, the changed one re-chunked"
        );
        assert!(
            !overlay.hidden_paths.contains(&key("Returned.bsl")),
            "returning to baseline un-hides it even on an unclean scan"
        );
        assert!(overlay.hidden_paths.contains(&key("Changed.bsl")));
    }

    /// After an unclean publication the dirty set is EXACTLY the unprocessed
    /// keys: a successfully-refreshed seen key is cleared, a seen-but-unread key
    /// stays, an unseen key stays. All three sides matter: keeping everything
    /// would re-process healthy keys forever, clearing everything loses the
    /// stale ones.
    #[test]
    fn an_unclean_full_refresh_clears_exactly_the_processed_dirty_keys() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let processed = workspace.join("Processed.bsl");
        fs::write(&processed, "Процедура Обработана()\nКонецПроцедуры").unwrap();
        // A directory shaped like a `.bsl`: stat succeeds, reading fails.
        let broken = workspace.join("Broken.bsl");
        fs::create_dir(&broken).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.mark_dirty_path(key("Processed.bsl"));
        cache.mark_dirty_path(key("Broken.bsl"));
        cache.mark_dirty_path(key("Unseen.bsl"));
        cache
            .full_refresh_scanned(
                &HashMap::new(),
                scanned_with(
                    &[(&key("Processed.bsl"), &processed), (&key("Broken.bsl"), &broken)],
                    1,
                    0,
                ),
                None,
                32,
                BaselineHashMode::RawFileBytes,
            )
            .unwrap();
        let mut dirty: Vec<String> =
            cache.dirty_paths_snapshot().keys().map(|k| k.path.clone()).collect();
        dirty.sort();
        assert_eq!(
            dirty,
            vec!["Broken.bsl".to_owned(), "Unseen.bsl".to_owned()],
            "exactly the unread and the unseen keys stay dirty"
        );
    }

    /// The same exactness on the manifest-driven full refresh.
    #[test]
    fn an_unclean_manifest_refresh_clears_exactly_the_processed_dirty_keys() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let processed = workspace.join("Processed.bsl");
        fs::write(&processed, "Процедура Обработана()\nКонецПроцедуры").unwrap();
        let broken = workspace.join("Broken.bsl");
        fs::create_dir(&broken).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.mark_dirty_path(key("Processed.bsl"));
        cache.mark_dirty_path(key("Broken.bsl"));
        cache.mark_dirty_path(key("Unseen.bsl"));
        cache
            .full_refresh_from_manifest_scanned(
                &HashMap::new(),
                scanned_with(
                    &[(&key("Processed.bsl"), &processed), (&key("Broken.bsl"), &broken)],
                    1,
                    0,
                ),
                None,
                32,
                &store,
            )
            .unwrap();
        let mut dirty: Vec<String> =
            cache.dirty_paths_snapshot().keys().map(|k| k.path.clone()).collect();
        dirty.sort();
        assert_eq!(dirty, vec!["Broken.bsl".to_owned(), "Unseen.bsl".to_owned()]);
    }

    /// And on the planned path: `publish_plan` clears a superseded dirty flag
    /// only for keys the plan actually processed.
    #[test]
    fn an_unclean_published_plan_clears_exactly_the_processed_dirty_keys() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let processed = workspace.join("Processed.bsl");
        fs::write(&processed, "Процедура Обработана()\nКонецПроцедуры").unwrap();
        let broken = workspace.join("Broken.bsl");
        fs::create_dir(&broken).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.mark_dirty_path(key("Processed.bsl"));
        cache.mark_dirty_path(key("Broken.bsl"));
        cache.mark_dirty_path(key("Unseen.bsl"));
        let dirty_before = cache.dirty_paths_snapshot();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest_scanned(
            &HashMap::new(),
            scanned_with(
                &[(&key("Processed.bsl"), &processed), (&key("Broken.bsl"), &broken)],
                1,
                0,
            ),
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        cache.publish_plan(plan, HashMap::new(), &dirty_before, None, &store).unwrap();
        let mut dirty: Vec<String> =
            cache.dirty_paths_snapshot().keys().map(|k| k.path.clone()).collect();
        dirty.sort();
        assert_eq!(dirty, vec!["Broken.bsl".to_owned(), "Unseen.bsl".to_owned()]);
    }

    /// Field-by-field transfer of every walk counter through the adapter: an
    /// end-to-end stand can only make `unreadable` non-zero (permissions), so a
    /// dropped counter would otherwise be invisible.
    #[test]
    fn the_mapping_carries_every_walk_counter() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("M.bsl");
        fs::write(&file, "Процедура П()\nКонецПроцедуры").unwrap();
        let set = project_model::SourceSet {
            files: vec![project_model::WalkedFile {
                root: dir.path().to_path_buf(),
                role: project_model::FileRole::Source,
                walked: file.clone(),
                canonical: file.clone(),
                metadata: fs::metadata(&file).unwrap(),
            }],
            unreadable: 2,
            loops: 3,
            dangling: 4,
            canonicalizations: 6,
            canonical_fallbacks: 5,
        };
        let scanned = super::scanned_files_from(&single_root(dir.path()), &set);
        assert_eq!(scanned.files.len(), 1);
        assert_eq!(
            (scanned.unreadable, scanned.loops, scanned.dangling, scanned.canonical_fallbacks),
            (2, 3, 4, 5),
            "each counter crosses the adapter unchanged"
        );
    }

    /// A pending rescan forces the full-scan arm through BOTH dispatchers even
    /// in initialized watcher mode, and a clean full scan clears it; with the
    /// flag down, watcher mode keeps not rescanning.
    #[test]
    fn a_pending_rescan_forces_the_full_arm_of_the_raw_dispatcher() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        fs::write(workspace.join("A.bsl"), "Процедура П()\nКонецПроцедуры").unwrap();
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let roots = single_root(workspace);
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();

        cache
            .full_refresh_scanned(
                &HashMap::new(),
                scanned_with(&[], 1, 0),
                None,
                32,
                BaselineHashMode::RawFileBytes,
            )
            .unwrap();
        assert!(cache.needs_full_rescan());
        let before = project_model::source_set::scans_performed_on_thread();
        cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        assert_eq!(
            project_model::source_set::scans_performed_on_thread() - before,
            1,
            "the pending flag forces the full arm"
        );
        assert!(!cache.needs_full_rescan(), "the clean full scan caught up");
        let before = project_model::source_set::scans_performed_on_thread();
        cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        assert_eq!(
            project_model::source_set::scans_performed_on_thread() - before,
            0,
            "flag down, watcher mode: no rescan"
        );
    }

    /// The manifest dispatcher is an independent condition; the flag must force
    /// it too.
    #[test]
    fn a_pending_rescan_forces_the_full_arm_of_the_manifest_dispatcher() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        fs::write(workspace.join("A.bsl"), "Процедура П()\nКонецПроцедуры").unwrap();
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let roots = single_root(workspace);
        let manifest: HashMap<FileKey, String> = HashMap::new();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();

        cache
            .full_refresh_from_manifest_scanned(
                &manifest,
                scanned_with(&[], 1, 0),
                None,
                32,
                &store,
            )
            .unwrap();
        assert!(cache.needs_full_rescan());
        let before = project_model::source_set::scans_performed_on_thread();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
        assert_eq!(
            project_model::source_set::scans_performed_on_thread() - before,
            1,
            "the pending flag forces the full arm"
        );
        assert!(!cache.needs_full_rescan());
        let before = project_model::source_set::scans_performed_on_thread();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
        assert_eq!(project_model::source_set::scans_performed_on_thread() - before, 0);
    }

    /// The flag is raised by an unclean publication of EVERY full path and
    /// cleared by a clean one — a flag wired to one path of three would leave
    /// its callers blind.
    #[test]
    fn every_unclean_full_publication_raises_the_rescan_flag() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let roots = single_root(workspace);
        let manifest: HashMap<FileKey, String> = HashMap::new();
        let mut cache = WorkspaceOverlayCache::default();

        cache
            .full_refresh_scanned(
                &HashMap::new(),
                scanned_with(&[], 1, 0),
                None,
                32,
                BaselineHashMode::RawFileBytes,
            )
            .unwrap();
        assert!(cache.needs_full_rescan(), "raw full refresh");
        cache
            .full_refresh(&HashMap::new(), &roots, None, 32, BaselineHashMode::RawFileBytes)
            .unwrap();
        assert!(!cache.needs_full_rescan(), "a clean raw refresh clears it");

        cache
            .full_refresh_from_manifest_scanned(
                &manifest,
                scanned_with(&[], 0, 1),
                None,
                32,
                &store,
            )
            .unwrap();
        assert!(cache.needs_full_rescan(), "manifest full refresh");
        cache.full_refresh_from_manifest(&manifest, &roots, None, 32, &store).unwrap();
        assert!(!cache.needs_full_rescan(), "a clean manifest refresh clears it");

        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest_scanned(
            &manifest,
            scanned_with(&[], 1, 0),
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        cache.publish_plan(plan, HashMap::new(), &HashMap::new(), None, &store).unwrap();
        assert!(cache.needs_full_rescan(), "published unclean plan");
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        cache.publish_plan(plan, HashMap::new(), &HashMap::new(), None, &store).unwrap();
        assert!(!cache.needs_full_rescan(), "a published clean plan clears it");
    }

    /// Runs `f` under a thread-local subscriber capturing WARN-and-up output; returns the
    /// closure's result and the captured lines. `bsl-search` sets no global dispatcher, so the
    /// scoped default reliably owns every event the closure emits.
    fn warns_during<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
        use std::sync::{Arc, Mutex};
        #[derive(Clone, Default)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(data);
                Ok(data.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buf {
            type Writer = Buf;
            fn make_writer(&'a self) -> Buf {
                self.clone()
            }
        }
        let buf = Buf::default();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_writer(buf.clone())
            .without_time()
            .finish();
        let result = tracing::subscriber::with_default(subscriber, f);
        let bytes = buf.0.lock().unwrap().clone();
        (result, String::from_utf8_lossy(&bytes).lines().map(str::to_owned).collect())
    }

    /// Closes `path` with mode 000 and reports whether that actually revokes access —
    /// under root it cannot, and the caller skips the scenario.
    #[cfg(unix)]
    fn deny_access(path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
        if path.is_dir() {
            fs::read_dir(path).is_err()
        } else {
            fs::read(path).is_err()
        }
    }

    #[cfg(unix)]
    fn restore_access(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { 0o755 } else { 0o644 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    /// A seen file whose read fails keeps its previous entry and its hiding, gets a retryable
    /// dirty mark, and the publication says so out loud — the old silent `continue` stranded
    /// the stale entry with no trace and no retry.
    #[cfg(unix)]
    #[test]
    fn a_read_failure_during_full_refresh_keeps_the_entry_and_warns() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let edited = workspace.join("Edited.bsl");
        fs::write(&edited, "Процедура Изменённая()\nКонецПроцедуры").unwrap();
        let baseline = HashMap::from([(key("Edited.bsl"), b"baseline-differs".to_vec())]);
        let roots = single_root(workspace);
        let mut cache = WorkspaceOverlayCache::default();
        cache.full_refresh(&baseline, &roots, None, 32, BaselineHashMode::RawFileBytes).unwrap();
        assert_eq!(cache.snapshot().lexical_documents[0].symbol_name, "Изменённая");
        assert!(cache.snapshot().hidden_paths.contains(&key("Edited.bsl")));

        // Two steps: first MOVE the fingerprint (a bare chmod leaves `(len, mtime)` alone and
        // the equal-fingerprint branch would skip the read entirely), then revoke access.
        fs::write(&edited, "Процедура ИзменённаяЕщёРаз()\nКонецПроцедуры").unwrap();
        if !deny_access(&edited) {
            return;
        }
        let (result, warns) = warns_during(|| {
            cache.full_refresh(&baseline, &roots, None, 32, BaselineHashMode::RawFileBytes)
        });
        restore_access(&edited);
        result.unwrap();

        let overlay = cache.snapshot();
        assert_eq!(
            overlay.lexical_documents[0].symbol_name, "Изменённая",
            "the previous version survives the failed read"
        );
        assert!(overlay.hidden_paths.contains(&key("Edited.bsl")), "its hiding survives");
        assert!(
            cache.dirty_paths_snapshot().contains_key(&key("Edited.bsl")),
            "the key is marked for a bounded retry"
        );
        assert!(warns.iter().any(|l| l.contains("Edited.bsl")), "the publication warns: {warns:?}");
    }

    /// The same protection on the manifest-driven full refresh.
    #[cfg(unix)]
    #[test]
    fn a_read_failure_during_manifest_refresh_keeps_the_entry_and_warns() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let edited = workspace.join("Edited.bsl");
        fs::write(&edited, "Процедура Изменённая()\nКонецПроцедуры").unwrap();
        let manifest = HashMap::from([(key("Edited.bsl"), "manifest-differs".to_owned())]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.full_refresh_from_manifest(&manifest, &roots, None, 32, &store).unwrap();
        assert_eq!(cache.snapshot().lexical_documents[0].symbol_name, "Изменённая");

        fs::write(&edited, "Процедура ИзменённаяЕщёРаз()\nКонецПроцедуры").unwrap();
        if !deny_access(&edited) {
            return;
        }
        let (result, warns) =
            warns_during(|| cache.full_refresh_from_manifest(&manifest, &roots, None, 32, &store));
        restore_access(&edited);
        result.unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents[0].symbol_name, "Изменённая");
        assert!(overlay.hidden_paths.contains(&key("Edited.bsl")));
        assert!(cache.dirty_paths_snapshot().contains_key(&key("Edited.bsl")));
        assert!(warns.iter().any(|l| l.contains("Edited.bsl")), "{warns:?}");
    }

    /// On the planned path the failure happens in phase A, which publishes nothing — so phase A
    /// stays silent and `publish_plan` emits exactly one warn per failed key, next to the fact
    /// that an incomplete result went live. The prior entry and its hiding survive the
    /// whole-replace publication.
    #[cfg(unix)]
    #[test]
    fn a_read_failure_survives_a_published_plan_with_one_warn() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let edited = workspace.join("Edited.bsl");
        fs::write(&edited, "Процедура Изменённая()\nКонецПроцедуры").unwrap();
        let manifest = HashMap::from([(key("Edited.bsl"), "manifest-differs".to_owned())]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        cache.publish_plan(plan, HashMap::new(), &HashMap::new(), None, &store).unwrap();
        assert_eq!(cache.snapshot().lexical_documents[0].symbol_name, "Изменённая");
        assert!(cache.snapshot().hidden_paths.contains(&key("Edited.bsl")));

        fs::write(&edited, "Процедура ИзменённаяЕщёРаз()\nКонецПроцедуры").unwrap();
        if !deny_access(&edited) {
            return;
        }
        let (plan, plan_warns) = warns_during(|| {
            WorkspaceOverlayCache::plan_full_refresh_from_manifest(
                &manifest,
                &roots,
                &store,
                &HashMap::new(),
                None,
            )
            .unwrap()
        });
        assert!(
            !plan_warns.iter().any(|l| l.contains("Edited.bsl")),
            "phase A publishes nothing and stays silent: {plan_warns:?}"
        );
        let (result, publish_warns) = warns_during(|| {
            cache.publish_plan(plan, HashMap::new(), &HashMap::new(), None, &store)
        });
        restore_access(&edited);
        result.unwrap();

        let overlay = cache.snapshot();
        assert_eq!(
            overlay.lexical_documents[0].symbol_name, "Изменённая",
            "the entry survives the publication"
        );
        assert!(overlay.hidden_paths.contains(&key("Edited.bsl")), "its hiding survives");
        assert!(cache.dirty_paths_snapshot().contains_key(&key("Edited.bsl")));
        assert_eq!(
            publish_warns.iter().filter(|l| l.contains("Edited.bsl")).count(),
            1,
            "exactly one warn, at the moment the incomplete result goes live: {publish_warns:?}"
        );
    }

    /// The persisted fingerprint row of a read-failed file claims "verified against the
    /// manifest" — after a failed read the claim must be retracted even when NOTHING else was
    /// read (an empty update map used to skip the save entirely), and retracting it must not
    /// take the verified neighbour's row with it.
    #[cfg(unix)]
    #[test]
    fn a_read_failure_drops_its_fingerprint_row_on_the_manifest_path() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let broken = workspace.join("Broken.bsl");
        fs::write(&broken, "Процедура Ломкая()\nКонецПроцедуры").unwrap();
        let alive = workspace.join("Alive.bsl");
        fs::write(&alive, "Процедура Живая()\nКонецПроцедуры").unwrap();
        let manifest = HashMap::from([
            (key("Broken.bsl"), "manifest-differs".to_owned()),
            (key("Alive.bsl"), "manifest-differs-too".to_owned()),
        ]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.full_refresh_from_manifest(&manifest, &roots, None, 32, &store).unwrap();
        let rows = store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default();
        assert_eq!(rows.len(), 2, "both reads succeeded, both rows persisted");

        fs::write(&broken, "Процедура ЛомкаяЕщёРаз()\nКонецПроцедуры").unwrap();
        if !deny_access(&broken) {
            return;
        }
        let result = cache.full_refresh_from_manifest(&manifest, &roots, None, 32, &store);
        restore_access(&broken);
        result.unwrap();
        let rows = store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default();
        assert!(!rows.contains_key(&key("Broken.bsl")), "the failed read retracts its row");
        assert!(rows.contains_key(&key("Alive.bsl")), "the verified neighbour keeps its row");

        // Single-row leg: with the only file failing, the update map is empty and a guarded
        // save would silently keep the stale row.
        let solo_dir = tempdir().unwrap();
        let solo_ws = solo_dir.path();
        let solo = solo_ws.join("Solo.bsl");
        fs::write(&solo, "Процедура Одна()\nКонецПроцедуры").unwrap();
        let solo_manifest = HashMap::from([(key("Solo.bsl"), "manifest-differs".to_owned())]);
        let solo_roots = single_root(solo_ws);
        let solo_store = Store::open(&solo_ws.join("search.db")).unwrap();
        let mut solo_cache = WorkspaceOverlayCache::default();
        solo_cache
            .full_refresh_from_manifest(&solo_manifest, &solo_roots, None, 32, &solo_store)
            .unwrap();
        assert_eq!(
            solo_store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default().len(),
            1
        );
        fs::write(&solo, "Процедура ОднаЕщёРаз()\nКонецПроцедуры").unwrap();
        if !deny_access(&solo) {
            return;
        }
        let result = solo_cache.full_refresh_from_manifest(
            &solo_manifest,
            &solo_roots,
            None,
            32,
            &solo_store,
        );
        restore_access(&solo);
        result.unwrap();
        let rows =
            solo_store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default();
        assert!(
            !rows.contains_key(&key("Solo.bsl")),
            "an empty update map must not shield the stale row"
        );
    }

    /// The same 2×2 matrix legs through the planned publication — an independent save site.
    #[cfg(unix)]
    #[test]
    fn a_read_failure_drops_its_fingerprint_row_on_the_planned_path() {
        let plan_and_publish = |manifest: &HashMap<FileKey, String>,
                                roots: &WorkspaceRoots,
                                store: &Store,
                                cache: &mut WorkspaceOverlayCache| {
            let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
                manifest,
                roots,
                store,
                &HashMap::new(),
                None,
            )
            .unwrap();
            cache.publish_plan(plan, HashMap::new(), &HashMap::new(), None, store).unwrap();
        };

        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let broken = workspace.join("Broken.bsl");
        fs::write(&broken, "Процедура Ломкая()\nКонецПроцедуры").unwrap();
        let alive = workspace.join("Alive.bsl");
        fs::write(&alive, "Процедура Живая()\nКонецПроцедуры").unwrap();
        let manifest = HashMap::from([
            (key("Broken.bsl"), "manifest-differs".to_owned()),
            (key("Alive.bsl"), "manifest-differs-too".to_owned()),
        ]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        plan_and_publish(&manifest, &roots, &store, &mut cache);
        assert_eq!(
            store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default().len(),
            2
        );

        fs::write(&broken, "Процедура ЛомкаяЕщёРаз()\nКонецПроцедуры").unwrap();
        if !deny_access(&broken) {
            return;
        }
        plan_and_publish(&manifest, &roots, &store, &mut cache);
        restore_access(&broken);
        let rows = store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default();
        assert!(!rows.contains_key(&key("Broken.bsl")), "the failed read retracts its row");
        assert!(rows.contains_key(&key("Alive.bsl")), "the verified neighbour keeps its row");

        let solo_dir = tempdir().unwrap();
        let solo_ws = solo_dir.path();
        let solo = solo_ws.join("Solo.bsl");
        fs::write(&solo, "Процедура Одна()\nКонецПроцедуры").unwrap();
        let solo_manifest = HashMap::from([(key("Solo.bsl"), "manifest-differs".to_owned())]);
        let solo_roots = single_root(solo_ws);
        let solo_store = Store::open(&solo_ws.join("search.db")).unwrap();
        let mut solo_cache = WorkspaceOverlayCache::default();
        plan_and_publish(&solo_manifest, &solo_roots, &solo_store, &mut solo_cache);
        fs::write(&solo, "Процедура ОднаЕщёРаз()\nКонецПроцедуры").unwrap();
        if !deny_access(&solo) {
            return;
        }
        plan_and_publish(&solo_manifest, &solo_roots, &solo_store, &mut solo_cache);
        restore_access(&solo);
        let rows =
            solo_store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default();
        assert!(!rows.contains_key(&key("Solo.bsl")), "an empty update map must not shield it");
    }

    /// A POINT refresh that fails to read a file must retract that file's persisted row too:
    /// the dirty mark knows the file changed but dies with the process, while the row would
    /// survive the restart and vouch "verified" for contents nobody read — the next full plan
    /// would then skip the read forever. The verified neighbour's row survives the retraction.
    #[cfg(unix)]
    #[test]
    fn a_point_read_failure_retracts_its_fingerprint_row_across_restart() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let swapped = workspace.join("Swapped.bsl");
        let original = "Процедура Первая()\nКонецПроцедуры";
        fs::write(&swapped, original).unwrap();
        let neighbour = workspace.join("Neighbour.bsl");
        fs::write(&neighbour, "Процедура Соседняя()\nКонецПроцедуры").unwrap();
        // Both files match the manifest: no overlay entries, but both fingerprint rows persist.
        let manifest = HashMap::from([
            (key("Swapped.bsl"), super::fingerprint_content(original, "Swapped.bsl")),
            (
                key("Neighbour.bsl"),
                super::fingerprint_content("Процедура Соседняя()\nКонецПроцедуры", "Neighbour.bsl"),
            ),
        ]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
        assert_eq!(
            store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default().len(),
            2
        );

        // Swap the contents at the SAME (len, mtime): only the retracted row makes the next
        // full plan re-read the file.
        let mtime = fs::metadata(&swapped).unwrap().modified().unwrap();
        fs::write(&swapped, "Процедура Вторая()\nКонецПроцедуры").unwrap();
        fs::File::options().write(true).open(&swapped).unwrap().set_modified(mtime).unwrap();
        if !deny_access(&swapped) {
            return;
        }
        cache.mark_dirty_path(key("Swapped.bsl"));
        let result = cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false);
        restore_access(&swapped);
        result.unwrap();

        let rows = store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default();
        assert!(
            !rows.contains_key(&key("Swapped.bsl")),
            "the unread file's row must not claim it was verified"
        );
        assert!(rows.contains_key(&key("Neighbour.bsl")), "the neighbour's row survives");

        // "Restart": a fresh plan must re-read the swapped file and see the new contents.
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        assert_eq!(plan.overlay_file_count(), 1, "the swap is visible after the restart");
    }

    /// The stat branch of the point refresh leaves the contents just as unverified as the read
    /// branch, so it must retract the row the same way.
    #[cfg(unix)]
    #[test]
    fn a_point_stat_failure_retracts_its_fingerprint_row_across_restart() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let closed = workspace.join("closed");
        fs::create_dir(&closed).unwrap();
        let swapped = closed.join("Swapped.bsl");
        let original = "Процедура Первая()\nКонецПроцедуры";
        fs::write(&swapped, original).unwrap();
        let manifest = HashMap::from([(
            key("closed/Swapped.bsl"),
            super::fingerprint_content(original, "closed/Swapped.bsl"),
        )]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
        assert_eq!(
            store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default().len(),
            1
        );

        let mtime = fs::metadata(&swapped).unwrap().modified().unwrap();
        fs::write(&swapped, "Процедура Вторая()\nКонецПроцедуры").unwrap();
        fs::File::options().write(true).open(&swapped).unwrap().set_modified(mtime).unwrap();
        if !deny_access(&closed) {
            return;
        }
        cache.mark_dirty_path(key("closed/Swapped.bsl"));
        let result = cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false);
        restore_access(&closed);
        result.unwrap();

        let rows = store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default();
        assert!(
            !rows.contains_key(&key("closed/Swapped.bsl")),
            "a stat failure leaves the contents unverified; the row must go"
        );
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        assert_eq!(plan.overlay_file_count(), 1, "the swap is visible after the restart");
    }

    /// An unclean FULL publication must not leave unseen keys' rows behind either: the row of a
    /// file the scan never reached would survive a restart and vouch for contents that changed
    /// meanwhile. Losing a row costs one extra read on the next clean pass; a false hit costs a
    /// lost edit — the replace-save keeps only what this pass actually verified.
    #[test]
    fn an_unclean_manifest_publication_drops_unseen_fingerprint_rows() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let unseen = workspace.join("Unseen.bsl");
        let original = "Процедура Первая()\nКонецПроцедуры";
        fs::write(&unseen, original).unwrap();
        let seen = workspace.join("Seen.bsl");
        fs::write(&seen, "Процедура Видимая()\nКонецПроцедуры").unwrap();
        let manifest = HashMap::from([
            (key("Unseen.bsl"), super::fingerprint_content(original, "Unseen.bsl")),
            (
                key("Seen.bsl"),
                super::fingerprint_content("Процедура Видимая()\nКонецПроцедуры", "Seen.bsl"),
            ),
        ]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.full_refresh_from_manifest(&manifest, &roots, None, 32, &store).unwrap();
        assert_eq!(
            store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default().len(),
            2
        );

        let mtime = fs::metadata(&unseen).unwrap().modified().unwrap();
        fs::write(&unseen, "Процедура Вторая()\nКонецПроцедуры").unwrap();
        fs::File::options().write(true).open(&unseen).unwrap().set_modified(mtime).unwrap();
        cache
            .full_refresh_from_manifest_scanned(
                &manifest,
                scanned_with(&[(&key("Seen.bsl"), &seen)], 1, 0),
                None,
                32,
                &store,
            )
            .unwrap();
        let rows = store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default();
        assert!(!rows.contains_key(&key("Unseen.bsl")), "the unverified row is dropped");
        assert!(rows.contains_key(&key("Seen.bsl")), "the verified row survives");

        // Single-row leg: an unclean scan that saw NOTHING leaves an empty update map, and a
        // guarded save would keep the whole stale table.
        let solo_dir = tempdir().unwrap();
        let solo_ws = solo_dir.path();
        let solo = solo_ws.join("Solo.bsl");
        let solo_content = "Процедура Одна()\nКонецПроцедуры";
        fs::write(&solo, solo_content).unwrap();
        let solo_manifest = HashMap::from([(
            key("Solo.bsl"),
            super::fingerprint_content(solo_content, "Solo.bsl"),
        )]);
        let solo_roots = single_root(solo_ws);
        let solo_store = Store::open(&solo_ws.join("search.db")).unwrap();
        let mut solo_cache = WorkspaceOverlayCache::default();
        solo_cache
            .full_refresh_from_manifest(&solo_manifest, &solo_roots, None, 32, &solo_store)
            .unwrap();
        assert_eq!(
            solo_store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default().len(),
            1
        );
        solo_cache
            .full_refresh_from_manifest_scanned(
                &solo_manifest,
                scanned_with(&[], 1, 0),
                None,
                32,
                &solo_store,
            )
            .unwrap();
        assert!(
            solo_store
                .load_overlay_fingerprint_cache("")
                .unwrap_or(None)
                .unwrap_or_default()
                .is_empty(),
            "nothing was verified, nothing may stay claimed"
        );
    }

    /// The same unseen-row policy through the planned publication.
    #[test]
    fn an_unclean_published_plan_drops_unseen_fingerprint_rows() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let unseen = workspace.join("Unseen.bsl");
        let original = "Процедура Первая()\nКонецПроцедуры";
        fs::write(&unseen, original).unwrap();
        let seen = workspace.join("Seen.bsl");
        fs::write(&seen, "Процедура Видимая()\nКонецПроцедуры").unwrap();
        let manifest = HashMap::from([
            (key("Unseen.bsl"), super::fingerprint_content(original, "Unseen.bsl")),
            (
                key("Seen.bsl"),
                super::fingerprint_content("Процедура Видимая()\nКонецПроцедуры", "Seen.bsl"),
            ),
        ]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        cache.publish_plan(plan, HashMap::new(), &HashMap::new(), None, &store).unwrap();
        assert_eq!(
            store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default().len(),
            2
        );

        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest_scanned(
            &manifest,
            scanned_with(&[(&key("Seen.bsl"), &seen)], 1, 0),
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        cache.publish_plan(plan, HashMap::new(), &HashMap::new(), None, &store).unwrap();
        let rows = store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default();
        assert!(!rows.contains_key(&key("Unseen.bsl")), "the unverified row is dropped");
        assert!(rows.contains_key(&key("Seen.bsl")), "the verified row survives");

        let solo_dir = tempdir().unwrap();
        let solo_ws = solo_dir.path();
        let solo = solo_ws.join("Solo.bsl");
        let solo_content = "Процедура Одна()\nКонецПроцедуры";
        fs::write(&solo, solo_content).unwrap();
        let solo_manifest = HashMap::from([(
            key("Solo.bsl"),
            super::fingerprint_content(solo_content, "Solo.bsl"),
        )]);
        let solo_roots = single_root(solo_ws);
        let solo_store = Store::open(&solo_ws.join("search.db")).unwrap();
        let mut solo_cache = WorkspaceOverlayCache::default();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &solo_manifest,
            &solo_roots,
            &solo_store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        solo_cache.publish_plan(plan, HashMap::new(), &HashMap::new(), None, &solo_store).unwrap();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest_scanned(
            &solo_manifest,
            scanned_with(&[], 1, 0),
            &solo_store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        solo_cache.publish_plan(plan, HashMap::new(), &HashMap::new(), None, &solo_store).unwrap();
        assert!(
            solo_store
                .load_overlay_fingerprint_cache("")
                .unwrap_or(None)
                .unwrap_or_default()
                .is_empty(),
            "nothing was verified, nothing may stay claimed"
        );
    }

    /// The point refresh distinguishes "the file is provably gone" (remove the entry) from
    /// "the file is alive but unreachable right now" (keep everything, retry with a budget).
    /// Provably gone: NotFound, a directory replaced by a file, a proven symlink cycle — final
    /// or in an ancestor, agreeing with the walk. Alive: a permission error, and a long but
    /// FINITE link chain (`ELOOP` alone is not proof of a cycle). The raw and the manifest
    /// implementations answer independently, so both are exercised.
    #[cfg(unix)]
    #[test]
    fn a_point_refresh_removes_proven_absence_and_keeps_live_files_raw() {
        point_absence_matrix(false);
    }

    #[cfg(unix)]
    #[test]
    fn a_point_refresh_removes_proven_absence_and_keeps_live_files_manifest() {
        point_absence_matrix(true);
    }

    #[cfg(unix)]
    fn point_absence_matrix(use_manifest: bool) {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let workspace = dir.path();
        let body = |name: &str| format!("Процедура {name}()\nКонецПроцедуры");

        fs::write(workspace.join("Gone.bsl"), body("Удалённая")).unwrap();
        fs::create_dir(workspace.join("dir")).unwrap();
        fs::write(workspace.join("dir/Inside.bsl"), body("ВКаталоге")).unwrap();
        fs::write(workspace.join("SelfLoop.bsl"), body("Закольцованная")).unwrap();
        fs::create_dir(workspace.join("anc")).unwrap();
        fs::write(workspace.join("anc/Under.bsl"), body("ПодЦиклом")).unwrap();
        fs::create_dir(workspace.join("perm")).unwrap();
        fs::write(workspace.join("perm/Kept.bsl"), body("Недоступная")).unwrap();
        fs::write(workspace.join("Chain.bsl"), body("Цепочечная")).unwrap();

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let roots = single_root(workspace);
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        let manifest: HashMap<FileKey, String> =
            HashMap::from([(key("Gone.bsl"), "manifest-differs".to_owned())]);
        // The raw arm resolves its baseline through the store, so the baseline row lives there.
        let gone = key("Gone.bsl");
        store.upsert_file(&gone.root_id, &gone.path, b"baseline-differs", "code").unwrap();
        if use_manifest {
            cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
        } else {
            cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        }
        assert_eq!(cache.snapshot().lexical_documents.len(), 6, "all six files indexed");

        // Provably gone, four shapes.
        fs::remove_file(workspace.join("Gone.bsl")).unwrap();
        fs::remove_dir_all(workspace.join("dir")).unwrap();
        fs::write(workspace.join("dir"), "не каталог").unwrap();
        fs::remove_file(workspace.join("SelfLoop.bsl")).unwrap();
        std::os::unix::fs::symlink("SelfLoop.bsl", workspace.join("SelfLoop.bsl")).unwrap();
        fs::remove_dir_all(workspace.join("anc")).unwrap();
        std::os::unix::fs::symlink("anc", workspace.join("anc")).unwrap();
        // Alive but unreachable, two shapes.
        if !deny_access(&workspace.join("perm")) {
            return;
        }
        fs::remove_file(workspace.join("Chain.bsl")).unwrap();
        let mut target = outside.path().join("Target.bsl");
        fs::write(&target, body("Цепочечная")).unwrap();
        for hop in (0..64).rev() {
            let link = outside.path().join(format!("chain{hop}"));
            std::os::unix::fs::symlink(&target, &link).unwrap();
            target = link;
        }
        std::os::unix::fs::symlink(&target, workspace.join("Chain.bsl")).unwrap();

        for path in [
            "Gone.bsl",
            "dir/Inside.bsl",
            "SelfLoop.bsl",
            "anc/Under.bsl",
            "perm/Kept.bsl",
            "Chain.bsl",
        ] {
            cache.mark_dirty_path(key(path));
        }
        let result = if use_manifest {
            cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false)
        } else {
            cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, false)
        };
        restore_access(&workspace.join("perm"));
        result.unwrap();

        let overlay = cache.snapshot();
        let mut names: Vec<&str> =
            overlay.lexical_documents.iter().map(|d| d.symbol_name.as_str()).collect();
        names.sort();
        assert_eq!(
            names,
            vec!["Недоступная", "Цепочечная"],
            "proven absence removes, live-but-unreachable keeps"
        );
        assert!(
            overlay.hidden_paths.contains(&key("Gone.bsl")),
            "a provably deleted baseline file is hidden"
        );
        let dirty = cache.dirty_paths_snapshot();
        assert!(dirty.contains_key(&key("perm/Kept.bsl")), "unreachable keys stay marked");
        assert!(dirty.contains_key(&key("Chain.bsl")));
        assert!(!dirty.contains_key(&key("Gone.bsl")), "removals consume their mark");
    }

    /// Operations that reset or fully re-certify the overlay state must drop the pending-rescan
    /// flag with the rest of it: `mark_initialized_clean` PROVES the store equals the disk, and
    /// a surviving flag would force the full walk the caller just proved unnecessary.
    #[test]
    fn a_clean_initialization_resets_the_pending_rescan() {
        let mut cache = WorkspaceOverlayCache::default();
        cache
            .full_refresh_scanned(
                &HashMap::new(),
                scanned_with(&[], 1, 0),
                None,
                32,
                BaselineHashMode::RawFileBytes,
            )
            .unwrap();
        assert!(cache.needs_full_rescan());
        cache.mark_initialized_clean();
        assert!(!cache.needs_full_rescan(), "a proven-clean initialization owes no rescan");

        cache
            .full_refresh_scanned(
                &HashMap::new(),
                scanned_with(&[], 1, 0),
                None,
                32,
                BaselineHashMode::RawFileBytes,
            )
            .unwrap();
        assert!(cache.needs_full_rescan());
        cache.clear();
        assert!(!cache.needs_full_rescan(), "cleared state carries no debts of the old state");
    }

    /// A PROVEN point removal must retract the file's persisted fingerprint row along with the
    /// entry: the row claims "verified", and if the path is later recreated with the same size
    /// and mtime, the surviving row would make the next full plan skip reading it.
    #[test]
    fn a_point_removal_retracts_its_fingerprint_row() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let solo = workspace.join("Solo.bsl");
        let content = "Процедура Одна()\nКонецПроцедуры";
        fs::write(&solo, content).unwrap();
        let manifest =
            HashMap::from([(key("Solo.bsl"), super::fingerprint_content(content, "Solo.bsl"))]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
        assert_eq!(
            store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default().len(),
            1
        );

        fs::remove_file(&solo).unwrap();
        cache.mark_dirty_path(key("Solo.bsl"));
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false).unwrap();
        let rows = store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default();
        assert!(
            !rows.contains_key(&key("Solo.bsl")),
            "a removed file's row must not survive to vouch for a future namesake"
        );
    }

    /// A published plan must not resurrect fingerprint rows retracted while its lock-free embed
    /// phase ran: the surviving dirty mark says "not verified", and the row written from the
    /// plan's stale snapshot would contradict it across a restart.
    #[cfg(unix)]
    #[test]
    fn a_stale_plan_does_not_resurrect_retracted_rows() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let swapped = workspace.join("Swapped.bsl");
        let original = "Процедура Первая()\nКонецПроцедуры";
        fs::write(&swapped, original).unwrap();
        let manifest = HashMap::from([(
            key("Swapped.bsl"),
            super::fingerprint_content(original, "Swapped.bsl"),
        )]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();

        // Phase A of a slow warmup snapshots the row while the file is still the original.
        let dirty_before = cache.dirty_paths_snapshot();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();

        // Mid-embed, the file is swapped at the same (len, mtime) and a point refresh fails to
        // read it — retracting the row and keeping the key dirty.
        let mtime = fs::metadata(&swapped).unwrap().modified().unwrap();
        fs::write(&swapped, "Процедура Вторая()\nКонецПроцедуры").unwrap();
        fs::File::options().write(true).open(&swapped).unwrap().set_modified(mtime).unwrap();
        if !deny_access(&swapped) {
            return;
        }
        cache.mark_dirty_path(key("Swapped.bsl"));
        let point = cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false);
        restore_access(&swapped);
        point.unwrap();
        assert!(!store
            .load_overlay_fingerprint_cache("")
            .unwrap_or(None)
            .unwrap_or_default()
            .contains_key(&key("Swapped.bsl")));

        cache.publish_plan(plan, HashMap::new(), &dirty_before, None, &store).unwrap();
        let rows = store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default();
        assert!(
            !rows.contains_key(&key("Swapped.bsl")),
            "the stale plan must not overrule the retraction"
        );
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        assert_eq!(plan.overlay_file_count(), 1, "the swap is re-read and visible");
    }

    /// A stat/read failure's obligations — the retracted row and the budgeted retry mark — must
    /// be settled AT the failure, not after the batch: a later error in the same batch aborts
    /// the tail, and the caller has already drained the dirty set.
    #[cfg(unix)]
    #[test]
    fn a_failure_earlier_in_a_batch_survives_a_later_batch_error() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let broken = workspace.join("Broken.bsl");
        let content = "Процедура Ломкая()\nКонецПроцедуры";
        fs::write(&broken, content).unwrap();
        let manifest =
            HashMap::from([(key("Broken.bsl"), super::fingerprint_content(content, "Broken.bsl"))]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
        assert_eq!(
            store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default().len(),
            1
        );

        // A second, new file whose processing needs the embedder — which is unreachable, so the
        // batch errors AFTER the first key's failure.
        fs::write(workspace.join("New.bsl"), "Процедура Новая()\nКонецПроцедуры").unwrap();
        let embedder = super::Embedder::new(crate::EmbedderConfig {
            base_url: "http://127.0.0.1:1".to_owned(),
            model: "test-model".to_owned(),
            dim: Some(3),
            api_key: None,
            provider: None,
        });
        if !deny_access(&broken) {
            return;
        }
        let result = cache.refresh_dirty_paths_from_manifest(
            vec![key("Broken.bsl"), key("New.bsl")],
            super::ManifestBaseline { fingerprints: &manifest, store: &store },
            &roots,
            Some(&embedder),
            32,
            &HashMap::new(),
        );
        restore_access(&broken);
        assert!(result.is_err(), "the unreachable embedder aborts the batch");

        let rows = store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default();
        assert!(
            !rows.contains_key(&key("Broken.bsl")),
            "the retraction happened before the batch died"
        );
        assert!(
            cache.dirty_paths_snapshot().contains_key(&key("Broken.bsl")),
            "the retry mark happened before the batch died"
        );
    }

    /// `NotFound` under a root that is ITSELF unreachable proves nothing about the file: the
    /// full walk classifies a failed root as incomplete coverage, and the point path must not
    /// read the same situation as the file's deletion.
    #[test]
    fn a_missing_root_keeps_the_point_entry() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let configuration = workspace.join("cf");
        fs::create_dir(&configuration).unwrap();
        fs::write(configuration.join("A.bsl"), "Процедура Живая()\nКонецПроцедуры").unwrap();
        let manifest = HashMap::from([(key("A.bsl"), "manifest-differs".to_owned())]);
        let (roots, _) = WorkspaceRoots::build(workspace, &configuration, &[]);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
        assert_eq!(cache.snapshot().lexical_documents.len(), 1);

        fs::rename(&configuration, workspace.join("cf.saved")).unwrap();
        cache.mark_dirty_path(key("A.bsl"));
        let point = cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false);
        fs::rename(workspace.join("cf.saved"), &configuration).unwrap();
        point.unwrap();

        assert_eq!(
            cache.snapshot().lexical_documents.len(),
            1,
            "an unreachable root is not evidence about the file"
        );
        assert!(
            cache.dirty_paths_snapshot().contains_key(&key("A.bsl")),
            "the key stays marked for a retry"
        );
    }

    /// The failure budget counts CONSECUTIVE failures: a successful full publication of the key
    /// resets it, so unrelated failures spread over time never add up to a drop.
    #[test]
    fn a_successful_full_publication_resets_the_read_failure_budget() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let broken = workspace.join("Broken.bsl");
        let manifest = HashMap::from([(key("Broken.bsl"), "manifest-differs".to_owned())]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();

        for round in 0..MAX_DIRTY_REFRESH_FAILURES {
            // A read failure (invalid UTF-8), then a successful publication of the same key.
            fs::write(&broken, [0xff, 0xfe, round as u8]).unwrap();
            cache.full_refresh_from_manifest(&manifest, &roots, None, 32, &store).unwrap();
            assert!(
                cache.dirty_paths_snapshot().contains_key(&key("Broken.bsl")),
                "round {round}: every non-consecutive failure has the full budget"
            );
            fs::write(&broken, format!("Процедура Раз{round}()\nКонецПроцедуры")).unwrap();
            cache.full_refresh_from_manifest(&manifest, &roots, None, 32, &store).unwrap();
        }
    }

    /// The plan's completeness accessors carry the exact walk counters and the exact number of
    /// failed reads: a consumer reports "incomplete warmup" from these, and a zeroed field
    /// there would be indistinguishable from a complete pass.
    #[cfg(unix)]
    #[test]
    fn a_plan_reports_its_scan_counters_and_read_failures() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let broken = workspace.join("Broken.bsl");
        fs::write(&broken, "Процедура Ломкая()\nКонецПроцедуры").unwrap();
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let scanned = scanned_with(&[(&key("Broken.bsl"), &broken)], 2, 1);
        if !deny_access(&broken) {
            return;
        }
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest_scanned(
            &HashMap::new(),
            scanned,
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        restore_access(&broken);
        assert!(!plan.scan_is_clean());
        assert_eq!(
            (plan.scan_unreadable(), plan.scan_canonical_fallbacks(), plan.read_failure_count()),
            (2, 1, 1),
            "each completeness field crosses the accessor unchanged"
        );
    }

    /// Two link targets of the SAME byte length with the SAME forced mtime — the shape that
    /// makes a `(len, mtime)` fingerprint blind, so only the physical spelling can tell them
    /// apart.
    #[cfg(unix)]
    fn equal_stat_targets(outside: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let first = outside.join("Первый.bsl");
        let second = outside.join("Второй.bsl");
        fs::write(&first, "Процедура ПерваяЦель()\nКонецПроцедуры").unwrap();
        fs::write(&second, "Процедура ВтораяЦель()\nКонецПроцедуры").unwrap();
        assert_eq!(
            fs::metadata(&first).unwrap().len(),
            fs::metadata(&second).unwrap().len(),
            "the stand needs equal lengths"
        );
        let mtime = fs::metadata(&first).unwrap().modified().unwrap();
        fs::File::options().write(true).open(&second).unwrap().set_modified(mtime).unwrap();
        (first, second)
    }

    /// Retargeting a link onto a file with the same `(len, mtime)` must be seen by the next
    /// full refresh: a file's identity includes WHERE it physically is, and a fingerprint blind
    /// to the spelling serves the old target forever.
    #[cfg(unix)]
    #[test]
    fn a_retargeted_link_at_equal_stat_is_reread_by_a_full_refresh() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let workspace = dir.path();
        let (first, second) = equal_stat_targets(outside.path());
        let alias = workspace.join("Alias.bsl");
        std::os::unix::fs::symlink(&first, &alias).unwrap();

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let roots = single_root(workspace);
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        assert_eq!(cache.snapshot().lexical_documents[0].symbol_name, "ПерваяЦель");

        fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&second, &alias).unwrap();
        cache.refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true).unwrap();
        assert_eq!(
            cache.snapshot().lexical_documents[0].symbol_name,
            "ВтораяЦель",
            "the retarget moves the fingerprint even at equal (len, mtime)"
        );
    }

    /// The same blindness through both persisted-fingerprint gates: a cache row taken before
    /// the retarget must not pass for the new target, on the in-place manifest refresh and on
    /// the planned one alike.
    #[cfg(unix)]
    #[test]
    fn a_retargeted_link_at_equal_stat_misses_both_fingerprint_gates() {
        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let workspace = dir.path();
        let (first, second) = equal_stat_targets(outside.path());
        let alias = workspace.join("Alias.bsl");
        std::os::unix::fs::symlink(&first, &alias).unwrap();
        // The file EQUALS the manifest, so the first pass leaves no overlay entry — only the
        // cache row; the second pass must miss that row by spelling, re-read and diverge.
        let manifest = HashMap::from([(
            key("Alias.bsl"),
            super::fingerprint_content(&fs::read_to_string(&first).unwrap(), "Alias.bsl"),
        )]);
        let roots = single_root(workspace);

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.full_refresh_from_manifest(&manifest, &roots, None, 32, &store).unwrap();
        assert_eq!(cache.snapshot().lexical_documents.len(), 0, "baseline-equal: row only");
        assert_eq!(
            store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default().len(),
            1
        );

        fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&second, &alias).unwrap();
        cache.full_refresh_from_manifest(&manifest, &roots, None, 32, &store).unwrap();
        assert_eq!(
            cache.snapshot().lexical_documents.len(),
            1,
            "the stale row must not vouch for the retargeted link"
        );
        assert_eq!(cache.snapshot().lexical_documents[0].symbol_name, "ВтораяЦель");

        // The planned path re-reads through its own, independent gate.
        let plan_store = Store::open(&workspace.join("search-plan.db")).unwrap();
        let mut plan_cache = WorkspaceOverlayCache::default();
        fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&first, &alias).unwrap();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &plan_store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        plan_cache.publish_plan(plan, HashMap::new(), &HashMap::new(), None, &plan_store).unwrap();
        fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&second, &alias).unwrap();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &plan_store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        assert_eq!(
            plan.overlay_file_count(),
            1,
            "the planned gate must miss the stale row by spelling too"
        );
    }

    /// The point paths take their fingerprints through their own constructors; the retarget
    /// must move those too, on both independent implementations.
    #[cfg(unix)]
    #[test]
    fn a_retargeted_link_at_equal_stat_is_reread_by_point_refreshes() {
        for use_manifest in [false, true] {
            let dir = tempdir().unwrap();
            let outside = tempdir().unwrap();
            let workspace = dir.path();
            let (first, second) = equal_stat_targets(outside.path());
            let alias = workspace.join("Alias.bsl");
            std::os::unix::fs::symlink(&first, &alias).unwrap();

            let store = Store::open(&workspace.join("search.db")).unwrap();
            let roots = single_root(workspace);
            let manifest: HashMap<FileKey, String> = HashMap::new();
            let mut cache = WorkspaceOverlayCache::default();
            cache.enable_watcher_mode();
            if use_manifest {
                cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
            } else {
                cache
                    .refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true)
                    .unwrap();
            }
            assert_eq!(cache.snapshot().lexical_documents[0].symbol_name, "ПерваяЦель");

            fs::remove_file(&alias).unwrap();
            std::os::unix::fs::symlink(&second, &alias).unwrap();
            cache.mark_dirty_path(key("Alias.bsl"));
            if use_manifest {
                cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false).unwrap();
            } else {
                cache
                    .refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, false)
                    .unwrap();
            }
            assert_eq!(
                cache.snapshot().lexical_documents[0].symbol_name,
                "ВтораяЦель",
                "manifest={use_manifest}: the point path must see the retarget"
            );
        }
    }

    /// A row from before the spelling column existed carries an empty `canonical` and must
    /// never pass the gate, even at matching `(len, mtime)`: the file is re-read and the
    /// re-save fills the spelling in — old rows heal, they are not trusted.
    #[test]
    fn an_empty_canonical_row_is_never_a_gate_hit() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("Solo.bsl");
        fs::write(&file, "Процедура Настоящая()\nКонецПроцедуры").unwrap();
        let manifest = HashMap::from([(key("Solo.bsl"), "manifest-differs".to_owned())]);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        // A stale pre-column row claiming the file EQUALS the manifest, at the file's real
        // (len, mtime) — everything matches except the spelling, which is empty.
        let metadata = fs::metadata(&file).unwrap();
        let (secs, nanos) = super::mtime_to_secs_nanos(metadata.modified().ok()).unwrap();
        store
            .save_overlay_fingerprint_cache(
                "",
                &HashMap::from([(
                    key("Solo.bsl"),
                    crate::store::PersistedFingerprint {
                        file_size: metadata.len(),
                        file_mtime_secs: secs,
                        file_mtime_nanos: nanos,
                        content_fingerprint: "manifest-differs".to_owned(),
                        canonical: String::new(),
                    },
                )]),
            )
            .unwrap();

        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &single_root(workspace),
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        assert_eq!(plan.overlay_file_count(), 1, "the empty-spelling row must not be trusted");
        let mut cache = WorkspaceOverlayCache::default();
        cache.publish_plan(plan, HashMap::new(), &HashMap::new(), None, &store).unwrap();
        let rows = store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default();
        assert!(
            !rows.get(&key("Solo.bsl")).map(|row| row.canonical.as_str()).unwrap_or("").is_empty(),
            "the published re-read heals the row with the real spelling"
        );
    }

    /// A fresh `.bsl`-spelled link to a NON-source target must not become an overlay entry on
    /// the point path: the walk drops such files (the roles of the two spellings disagree), and
    /// the point path must serve the same universe. Both independent implementations.
    #[cfg(unix)]
    #[test]
    fn a_link_to_a_non_source_target_is_not_indexed_by_point_refreshes() {
        for use_manifest in [false, true] {
            let dir = tempdir().unwrap();
            let outside = tempdir().unwrap();
            let workspace = dir.path();
            let target = outside.path().join("Target.txt");
            fs::write(&target, "Процедура ТолькоЧерезСсылку()\nКонецПроцедуры").unwrap();

            let store = Store::open(&workspace.join("search.db")).unwrap();
            let roots = single_root(workspace);
            let manifest: HashMap<FileKey, String> = HashMap::new();
            let mut cache = WorkspaceOverlayCache::default();
            cache.enable_watcher_mode();
            if use_manifest {
                cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
            } else {
                cache
                    .refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true)
                    .unwrap();
            }

            std::os::unix::fs::symlink(&target, workspace.join("Alias.bsl")).unwrap();
            cache.mark_dirty_path(key("Alias.bsl"));
            if use_manifest {
                cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false).unwrap();
            } else {
                cache
                    .refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, false)
                    .unwrap();
            }
            assert_eq!(
                cache.snapshot().lexical_documents.len(),
                0,
                "manifest={use_manifest}: a .txt target is not a source file"
            );
        }
    }

    /// Retargeting an indexed `.bsl` link onto a NON-source file is positive evidence the
    /// source is gone: the point refresh removes the entry instead of serving the foreign
    /// contents. Both independent implementations.
    #[cfg(unix)]
    #[test]
    fn a_retarget_onto_a_non_source_file_removes_the_point_entry() {
        for use_manifest in [false, true] {
            let dir = tempdir().unwrap();
            let outside = tempdir().unwrap();
            let workspace = dir.path();
            let source = outside.path().join("Настоящий.bsl");
            fs::write(&source, "Процедура Настоящая()\nКонецПроцедуры").unwrap();
            let foreign = outside.path().join("Чужой.txt");
            fs::write(&foreign, "Процедура Чужая()\nКонецПроцедуры").unwrap();
            let alias = workspace.join("Alias.bsl");
            std::os::unix::fs::symlink(&source, &alias).unwrap();

            let store = Store::open(&workspace.join("search.db")).unwrap();
            let roots = single_root(workspace);
            let manifest: HashMap<FileKey, String> = HashMap::new();
            let mut cache = WorkspaceOverlayCache::default();
            cache.enable_watcher_mode();
            if use_manifest {
                cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
            } else {
                cache
                    .refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true)
                    .unwrap();
            }
            assert_eq!(cache.snapshot().lexical_documents[0].symbol_name, "Настоящая");

            fs::remove_file(&alias).unwrap();
            std::os::unix::fs::symlink(&foreign, &alias).unwrap();
            cache.mark_dirty_path(key("Alias.bsl"));
            if use_manifest {
                cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false).unwrap();
            } else {
                cache
                    .refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, false)
                    .unwrap();
            }
            assert_eq!(
                cache.snapshot().lexical_documents.len(),
                0,
                "manifest={use_manifest}: the retargeted link no longer names a source file"
            );
        }
    }

    /// A registered root that is a symlink onto a vanished target: the link's inode exists, but
    /// the tree behind it does not — the full walk calls that incomplete coverage, so the point
    /// path must keep the entry and retry, exactly as for a renamed plain root.
    #[cfg(unix)]
    #[test]
    fn a_dangling_root_link_keeps_the_point_entry() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let target = workspace.join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("A.bsl"), "Процедура Живая()\nКонецПроцедуры").unwrap();
        let configuration = workspace.join("cf");
        std::os::unix::fs::symlink(&target, &configuration).unwrap();
        let manifest = HashMap::from([(key("A.bsl"), "manifest-differs".to_owned())]);
        let (roots, _) = WorkspaceRoots::build(workspace, &configuration, &[]);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
        assert_eq!(cache.snapshot().lexical_documents.len(), 1);

        fs::rename(&target, workspace.join("target.saved")).unwrap();
        cache.mark_dirty_path(key("A.bsl"));
        let point = cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false);
        fs::rename(workspace.join("target.saved"), &target).unwrap();
        point.unwrap();

        assert_eq!(
            cache.snapshot().lexical_documents.len(),
            1,
            "a root whose link target vanished proves nothing about the file"
        );
        assert!(cache.dirty_paths_snapshot().contains_key(&key("A.bsl")));
    }

    /// A source file replaced by a DIRECTORY spelled the same: the walk yields only regular
    /// files, so the point path must read the replacement as the source file's removal — not
    /// spin its retry budget on `read_to_string` (a FIFO there would even block it).
    #[test]
    fn a_file_replaced_by_a_directory_is_removed_by_the_point_refresh() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура Живая()\nКонецПроцедуры").unwrap();
        let manifest: HashMap<FileKey, String> = HashMap::new();
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
        assert_eq!(cache.snapshot().lexical_documents.len(), 1);

        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        cache.mark_dirty_path(key("A.bsl"));
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false).unwrap();

        assert_eq!(
            cache.snapshot().lexical_documents.len(),
            0,
            "a directory is not a source file; the walk would not yield it"
        );
        assert!(
            !cache.dirty_paths_snapshot().contains_key(&key("A.bsl")),
            "a settled removal consumes its mark instead of burning the retry budget"
        );
    }

    /// When the stale-row filter empties the plan's map entirely, the rows it filtered must
    /// still be retracted: a skipped save would keep exactly the row the filter rejected.
    #[test]
    fn a_filtered_out_last_fingerprint_row_is_still_retracted() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        let original = "Процедура Первая()\nКонецПроцедуры";
        fs::write(&file, original).unwrap();
        let manifest =
            HashMap::from([(key("A.bsl"), super::fingerprint_content(original, "A.bsl"))]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();

        let dirty_before = cache.dirty_paths_snapshot();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        // Mid-embed the file changes at the same (len, mtime) and gets marked: the plan's only
        // row is now stale, and the publish must not leave it behind just because the filtered
        // map came out empty.
        let mtime = fs::metadata(&file).unwrap().modified().unwrap();
        fs::write(&file, "Процедура Вторая()\nКонецПроцедуры").unwrap();
        fs::File::options().write(true).open(&file).unwrap().set_modified(mtime).unwrap();
        cache.mark_dirty_path(key("A.bsl"));
        cache.publish_plan(plan, HashMap::new(), &dirty_before, None, &store).unwrap();

        assert!(
            !store
                .load_overlay_fingerprint_cache("")
                .unwrap_or(None)
                .unwrap_or_default()
                .contains_key(&key("A.bsl")),
            "the filtered row must not survive to pass the gate after a restart"
        );
    }

    /// A SUCCESSFUL point re-read makes the old fingerprint row stale just as surely as a
    /// failed one: the row says "verified equal to the manifest", the re-read just proved
    /// otherwise, and at unchanged `(len, mtime, canonical)` the surviving row would suppress
    /// the published edit after a restart.
    #[test]
    fn a_successful_point_reread_retracts_the_old_fingerprint_row() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        let original = "Процедура Первая()\nКонецПроцедуры";
        fs::write(&file, original).unwrap();
        let manifest =
            HashMap::from([(key("A.bsl"), super::fingerprint_content(original, "A.bsl"))]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
        assert_eq!(
            store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default().len(),
            1
        );

        let mtime = fs::metadata(&file).unwrap().modified().unwrap();
        fs::write(&file, "Процедура Вторая()\nКонецПроцедуры").unwrap();
        fs::File::options().write(true).open(&file).unwrap().set_modified(mtime).unwrap();
        cache.mark_dirty_path(key("A.bsl"));
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false).unwrap();
        assert_eq!(
            cache.snapshot().lexical_documents[0].symbol_name,
            "Вторая",
            "the point re-read published the edit in-process"
        );

        // "Restart": a fresh plan must not let the stale row suppress the published edit.
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        assert_eq!(plan.overlay_file_count(), 1, "the edit survives the restart");
    }

    /// A failed row retraction must not pass for a done one: the dirty mark stays (with its
    /// budget) so a later pass retries the retraction, instead of the stale row silently
    /// outliving the process.
    #[test]
    fn a_failed_row_retraction_keeps_the_dirty_mark() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура Живая()\nКонецПроцедуры").unwrap();
        let manifest = HashMap::from([(
            key("A.bsl"),
            super::fingerprint_content("Процедура Живая()\nКонецПроцедуры", "A.bsl"),
        )]);
        let roots = single_root(workspace);
        let db_path = workspace.join("search.db");
        let store = Store::open(&db_path).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
        assert_eq!(
            store.load_overlay_fingerprint_cache("").unwrap_or(None).unwrap_or_default().len(),
            1
        );

        // A second connection injects a trigger that makes every row deletion fail.
        let saboteur = rusqlite::Connection::open(&db_path).unwrap();
        saboteur
            .execute_batch(
                "CREATE TRIGGER deny_fp_delete BEFORE DELETE ON overlay_fingerprint_cache \
                 BEGIN SELECT RAISE(FAIL, 'deny'); END;",
            )
            .unwrap();
        fs::remove_file(&file).unwrap();
        cache.mark_dirty_path(key("A.bsl"));
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false).unwrap();
        assert!(
            cache.dirty_paths_snapshot().contains_key(&key("A.bsl")),
            "an unretracted row keeps its key marked for the retry"
        );

        // With the sabotage removed, the retried pass settles the removal for real.
        saboteur.execute_batch("DROP TRIGGER deny_fp_delete;").unwrap();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false).unwrap();
        assert!(
            !store
                .load_overlay_fingerprint_cache("")
                .unwrap_or(None)
                .unwrap_or_default()
                .contains_key(&key("A.bsl")),
            "the retried retraction lands"
        );
        assert!(!cache.dirty_paths_snapshot().contains_key(&key("A.bsl")));
    }

    /// The retraction obligation left by a FAILED retract must be honoured by the retried
    /// pass: the mark that survived the failure brings the key back, the pass re-reads it
    /// (marked keys have no fast path) and settles the retraction before consuming the mark.
    #[test]
    fn a_failed_retraction_is_retried_on_the_next_pass() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        let original = "Процедура Первая()\nКонецПроцедуры";
        fs::write(&file, original).unwrap();
        let manifest =
            HashMap::from([(key("A.bsl"), super::fingerprint_content(original, "A.bsl"))]);
        let roots = single_root(workspace);
        let db_path = workspace.join("search.db");
        let store = Store::open(&db_path).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();

        // The edit at unchanged (len, mtime): the re-read succeeds and publishes, but the
        // sabotaged retraction fails, leaving the mark as the only trace of the stale row.
        let mtime = fs::metadata(&file).unwrap().modified().unwrap();
        fs::write(&file, "Процедура Вторая()\nКонецПроцедуры").unwrap();
        fs::File::options().write(true).open(&file).unwrap().set_modified(mtime).unwrap();
        let saboteur = rusqlite::Connection::open(&db_path).unwrap();
        saboteur
            .execute_batch(
                "CREATE TRIGGER deny_fp_delete BEFORE DELETE ON overlay_fingerprint_cache \
                 BEGIN SELECT RAISE(FAIL, 'deny'); END;",
            )
            .unwrap();
        cache.mark_dirty_path(key("A.bsl"));
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false).unwrap();
        assert_eq!(cache.snapshot().lexical_documents[0].symbol_name, "Вторая");
        assert!(cache.dirty_paths_snapshot().contains_key(&key("A.bsl")));

        // The retried pass meets the freshly-published entry (equal fingerprint, no read) and
        // must STILL settle the retraction before consuming the mark.
        saboteur.execute_batch("DROP TRIGGER deny_fp_delete;").unwrap();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false).unwrap();
        assert!(
            !store
                .load_overlay_fingerprint_cache("")
                .unwrap_or(None)
                .unwrap_or_default()
                .contains_key(&key("A.bsl")),
            "the obligation survives into the fast path"
        );
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        assert_eq!(plan.overlay_file_count(), 1, "the edit survives the restart");
    }

    /// A dirty mark is positive evidence the fingerprint must not be trusted: an edit at
    /// unchanged `(len, mtime, canonical)` with a watcher mark must be re-read by the POINT
    /// paths instead of being consumed through the equal-fingerprint fast path.
    #[test]
    fn a_marked_key_at_equal_stat_is_reread_by_point_refreshes() {
        for use_manifest in [false, true] {
            let dir = tempdir().unwrap();
            let workspace = dir.path();
            let file = workspace.join("A.bsl");
            let original = "Процедура Первая()\nКонецПроцедуры";
            fs::write(&file, original).unwrap();
            let manifest = HashMap::from([(key("A.bsl"), "manifest-differs".to_owned())]);
            let baseline = HashMap::from([(key("A.bsl"), b"baseline-differs".to_vec())]);
            let roots = single_root(workspace);
            let store = Store::open(&workspace.join("search.db")).unwrap();
            let mut cache = WorkspaceOverlayCache::default();
            cache.enable_watcher_mode();
            if use_manifest {
                cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();
            } else {
                let gone = key("A.bsl");
                store.upsert_file(&gone.root_id, &gone.path, &baseline[&gone], "code").unwrap();
                cache
                    .refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, true)
                    .unwrap();
            }
            assert_eq!(cache.snapshot().lexical_documents[0].symbol_name, "Первая");

            let mtime = fs::metadata(&file).unwrap().modified().unwrap();
            fs::write(&file, "Процедура Вторая()\nКонецПроцедуры").unwrap();
            fs::File::options().write(true).open(&file).unwrap().set_modified(mtime).unwrap();
            cache.mark_dirty_path(key("A.bsl"));
            if use_manifest {
                cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false).unwrap();
            } else {
                cache
                    .refresh(&store, &roots, None, 32, BaselineHashMode::RawFileBytes, false)
                    .unwrap();
            }
            assert_eq!(
                cache.snapshot().lexical_documents[0].symbol_name,
                "Вторая",
                "manifest={use_manifest}: the mark outranks the equal fingerprint"
            );
        }
    }

    /// The same evidence must outrank the equal-fingerprint entry gate of the IN-PLACE full
    /// refreshes — the raw entry check and the manifest persisted-row gate alike.
    #[test]
    fn a_marked_key_at_equal_stat_is_reread_by_full_refreshes() {
        // Raw arm: the equal in-memory entry must not shadow the marked edit.
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        let original = "Процедура Первая()\nКонецПроцедуры";
        fs::write(&file, original).unwrap();
        let baseline = HashMap::from([(key("A.bsl"), b"baseline-differs".to_vec())]);
        let roots = single_root(workspace);
        let mut cache = WorkspaceOverlayCache::default();
        cache.full_refresh(&baseline, &roots, None, 32, BaselineHashMode::RawFileBytes).unwrap();
        assert_eq!(cache.snapshot().lexical_documents[0].symbol_name, "Первая");
        let mtime = fs::metadata(&file).unwrap().modified().unwrap();
        fs::write(&file, "Процедура Вторая()\nКонецПроцедуры").unwrap();
        fs::File::options().write(true).open(&file).unwrap().set_modified(mtime).unwrap();
        cache.mark_dirty_path(key("A.bsl"));
        cache.full_refresh(&baseline, &roots, None, 32, BaselineHashMode::RawFileBytes).unwrap();
        assert_eq!(
            cache.snapshot().lexical_documents[0].symbol_name,
            "Вторая",
            "raw: the mark outranks the equal entry fingerprint"
        );

        // Manifest arm: the persisted-row gate must not shadow the marked edit either.
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, original).unwrap();
        let manifest =
            HashMap::from([(key("A.bsl"), super::fingerprint_content(original, "A.bsl"))]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.full_refresh_from_manifest(&manifest, &roots, None, 32, &store).unwrap();
        assert_eq!(cache.snapshot().lexical_documents.len(), 0, "baseline-equal at first");
        let mtime = fs::metadata(&file).unwrap().modified().unwrap();
        fs::write(&file, "Процедура Вторая()\nКонецПроцедуры").unwrap();
        fs::File::options().write(true).open(&file).unwrap().set_modified(mtime).unwrap();
        cache.mark_dirty_path(key("A.bsl"));
        cache.full_refresh_from_manifest(&manifest, &roots, None, 32, &store).unwrap();
        assert_eq!(
            cache.snapshot().lexical_documents.len(),
            1,
            "manifest: the mark outranks the persisted-row gate"
        );
        assert_eq!(cache.snapshot().lexical_documents[0].symbol_name, "Вторая");
    }

    /// The PLANNED path cannot read the mark set (it plans off-lock), so its publish must not
    /// CONSUME a live mark for a key the plan skipped by the persisted-row gate: the mark and
    /// the retracted row stay, and the next point pass delivers the edit.
    #[test]
    fn a_published_plan_leaves_gate_skipped_marked_keys_dirty() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        let original = "Процедура Первая()\nКонецПроцедуры";
        fs::write(&file, original).unwrap();
        let manifest =
            HashMap::from([(key("A.bsl"), super::fingerprint_content(original, "A.bsl"))]);
        let roots = single_root(workspace);
        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, true).unwrap();

        // The edit and its mark land BEFORE the plan is built: the plan's gate skips the key
        // by the stale row, and only the surviving mark can save the edit.
        let mtime = fs::metadata(&file).unwrap().modified().unwrap();
        fs::write(&file, "Процедура Вторая()\nКонецПроцедуры").unwrap();
        fs::File::options().write(true).open(&file).unwrap().set_modified(mtime).unwrap();
        cache.mark_dirty_path(key("A.bsl"));
        let dirty_before = cache.dirty_paths_snapshot();
        let plan = WorkspaceOverlayCache::plan_full_refresh_from_manifest(
            &manifest,
            &roots,
            &store,
            &HashMap::new(),
            None,
        )
        .unwrap();
        cache.publish_plan(plan, HashMap::new(), &dirty_before, None, &store).unwrap();

        assert!(
            cache.dirty_paths_snapshot().contains_key(&key("A.bsl")),
            "a gate-skipped marked key must not be consumed by the publish"
        );
        cache.refresh_with_manifest(&manifest, &roots, None, 32, &store, false).unwrap();
        assert_eq!(
            cache.snapshot().lexical_documents[0].symbol_name,
            "Вторая",
            "the surviving mark delivers the edit on the next point pass"
        );
    }

    /// The overlay must not walk the tree itself: the walk policy (which links
    /// are followed, how errors are classified) lives in `project-model`, and a
    /// private traversal here would quietly diverge from it again.
    #[test]
    fn the_overlay_does_not_carry_its_own_tree_walk() {
        let source = include_str!("workspace_overlay.rs");
        let needle: String = ["walk", "dir"].concat();
        assert!(
            !source.to_lowercase().contains(&needle),
            "workspace_overlay.rs must scan through project_model::SourceSet::scan"
        );
    }
}
