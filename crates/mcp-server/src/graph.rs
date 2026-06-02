//! Background-built semantic call graph for the workspace MCP profile.
//!
//! The whole-config call graph is built into an on-disk SQLite store (the
//! in-memory graph does not fit in RAM on large configs) and served read-only from
//! there. The build runs off-thread in RAM-bounded batches: tools observe
//! [`GraphStatus`] and degrade gracefully while it indexes.
//!
//! Freshness is **pull-on-request**: each `graph` call cheaply checks whether the
//! workspace drifted on disk since the snapshot it served and, on drift, kicks an
//! async reload while still serving the current (stale) snapshot. The agent-facing
//! freshness token is a monotonic *generation*, recorded in the built file's `meta`
//! so a served response's revision always describes the exact build it serves.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide::RootDatabaseImpl;
use vfs::{file_set::FileSet, FileId, VfsPath};
use walkdir::WalkDir;

use crate::graph_query::{graph_db_path, GraphDb};

/// The whole workspace is loaded into a single source root.
pub(crate) const GRAPH_SOURCE_ROOT: SourceRootId = SourceRootId(0);

/// Minimum time between on-disk drift scans. A scan stats every `.bsl`/`.xml`
/// file under the config roots, so throttling bounds its cost regardless of how
/// fast an agent fires `graph` calls.
const DRIFT_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// Modules whose edges are projected per batch when building the on-disk graph.
/// 500 keeps peak RSS comfortably bounded on a 25k-module config (measured ~2.9 GB)
/// while the resident method index resolves cross-batch calls.
const GRAPH_BUILD_BATCH: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GraphStatus {
    /// Not a workspace profile — the graph is unavailable.
    Disabled,
    /// A workspace is configured but the graph has not been loaded yet; the
    /// first `graph` tool call triggers the load.
    Idle,
    /// Background load in progress.
    Loading,
    /// Ready to serve, with the indexed `.bsl` file count.
    Ready { files: usize },
    /// Load failed.
    Failed(String),
}

/// State of an in-flight or last-attempted background reload, surfaced to agents
/// so a failed reload is visible rather than leaving them at `stale=true` forever.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReloadState {
    /// No reload in flight; the published snapshot is the latest.
    Idle,
    /// A reload triggered by detected drift is running in the background.
    Running,
    /// The last reload failed; the previous snapshot is still served.
    Failed(String),
}

impl ReloadState {
    fn label(&self) -> &'static str {
        match self {
            ReloadState::Idle => "none",
            ReloadState::Running => "running",
            ReloadState::Failed(_) => "failed",
        }
    }
}

/// The published build's freshness metadata. The graph itself lives in the SQLite
/// file at `graph_db_path(workspace_root)` (atomically renamed into place by the
/// loader), so only the generation/fingerprint/reload move under the lock; a query
/// opens the file separately. Keeping these together still gives a reader a torn-free
/// freshness token.
struct Published {
    generation: u64,
    fingerprint: u64,
    reload: ReloadState,
}

/// Everything mutable about the published graph, guarded by a single mutex. Locks
/// are only held for brief reads/swaps — the load and the drift scan run without
/// this lock held.
struct Inner {
    status: GraphStatus,
    published: Option<Published>,
}

/// Throttled cache of the last on-disk fingerprint scan. Guarded by its own mutex
/// held *across* the walk, so concurrent callers serialize onto one scan per
/// window rather than all walking the tree (no thundering herd).
struct ScanCache {
    at: Instant,
    disk_fp: u64,
}

/// A served graph handle plus the freshness token it was built at. Capturing the
/// generation/fingerprint at snapshot time (not at response time) keeps the
/// envelope's `revision`/`stale` consistent with the data actually returned, even
/// if a reload publishes a newer generation while the query runs. The handle is an
/// own read-only connection opened against the on-disk SQLite graph.
pub(crate) struct GraphSnapshot {
    pub graph: GraphDb,
    generation: u64,
    fingerprint: u64,
    force_stale: bool,
}

/// Freshness verdict for one `graph` response.
pub(crate) struct Freshness {
    /// The generation of the snapshot that served this response.
    pub revision: u64,
    /// The workspace drifted on disk since this snapshot was built.
    pub stale: bool,
    /// Reload state: `"none"`, `"running"`, or `"failed"`.
    pub reload: &'static str,
}

/// Handle to the workspace call graph. Cheap to clone (shared `Arc`s).
///
/// Loading is lazy: the SQLite graph is built off the workspace on first use, so a
/// server whose user never touches the graph pays nothing. The build is triggered
/// on the first `graph` tool call.
#[derive(Clone)]
pub(crate) struct GraphState {
    inner: Arc<Mutex<Inner>>,
    scan: Arc<Mutex<Option<ScanCache>>>,
    workspace_root: Option<PathBuf>,
    drift_interval: Duration,
}

impl GraphState {
    /// A disabled graph (reference / shared profiles).
    pub(crate) fn disabled() -> Self {
        Self::with_status(GraphStatus::Disabled, None)
    }

    /// A workspace graph that loads lazily on first use.
    pub(crate) fn for_workspace(workspace_root: PathBuf) -> Self {
        Self::with_status(GraphStatus::Idle, Some(workspace_root))
    }

    fn with_status(status: GraphStatus, workspace_root: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner { status, published: None })),
            scan: Arc::new(Mutex::new(None)),
            workspace_root,
            drift_interval: DRIFT_CHECK_INTERVAL,
        }
    }

    pub(crate) fn status(&self) -> GraphStatus {
        lock_recover(&self.inner).status.clone()
    }

    /// Trigger the background load if this is the first call. Transitions
    /// `Idle → Loading` and spawns exactly one loader thread; later calls return
    /// immediately. No-op for disabled / already-loading / ready / failed graphs.
    pub(crate) fn ensure_loading(&self) {
        if self.workspace_root.is_none() {
            return;
        }
        {
            let mut inner = lock_recover(&self.inner);
            if inner.status != GraphStatus::Idle {
                return;
            }
            inner.status = GraphStatus::Loading;
        }

        let state = self.clone();
        let spawned = std::thread::Builder::new()
            .name("bsl-graph-init".to_owned())
            .spawn(move || state.run_load(false));
        if let Err(e) = spawned {
            let mut inner = lock_recover(&self.inner);
            inner.status = GraphStatus::Failed(format!("could not spawn loader: {e}"));
        }
    }

    /// Snapshot the graph for a blocking query, if built. The returned
    /// [`GraphSnapshot`] owns a read-only SQLite handle and its freshness token,
    /// and can be moved onto a blocking task without holding the lock during the
    /// query.
    pub(crate) fn snapshot(&self) -> Option<GraphSnapshot> {
        // Gate on a published build, but take the served revision/fingerprint from
        // the FILE's own meta (below), not from the lock — so even if a reload
        // renames a newer file in between this check and the open, the snapshot's
        // freshness token describes exactly the build it serves, never a torn mix.
        lock_recover(&self.inner).published.as_ref()?;
        // A complete file is always present once `Ready` (the loader renames it into
        // place atomically and publishes only after); a failed open (incomplete or
        // missing) degrades to the caller's "still loading" path.
        let path = graph_db_path(self.workspace_root.as_deref()?);
        let graph = GraphDb::open(&path).ok()?;
        let (generation, fingerprint, force_stale) = graph.freshness_token().ok()?;
        Some(GraphSnapshot { graph, generation, fingerprint, force_stale })
    }

    /// Report the freshness of `snapshot` relative to disk, and on drift kick an
    /// async reload (at most one in flight). `stale`/`revision` are relative to the
    /// snapshot that served the response; the reload decision is relative to the
    /// latest published snapshot. Walks the filesystem, so call from a blocking
    /// context.
    pub(crate) fn freshness(&self, snapshot: &GraphSnapshot) -> Freshness {
        let disk_fp = self.current_disk_fp();
        // A snapshot from a straddled load is unconditionally stale until a clean
        // reload replaces it — equality alone could be fooled by an ABA rollback to
        // the pre-load fingerprint.
        let stale =
            snapshot.force_stale || disk_fp.map(|fp| fp != snapshot.fingerprint).unwrap_or(false);

        let mut inner = lock_recover(&self.inner);
        let Some(published) = inner.published.as_mut() else {
            return Freshness { revision: snapshot.generation, stale, reload: "none" };
        };
        let mut reload = published.reload.label();
        // Claim the single reload slot under the lock — comparing the fresh disk
        // fingerprint against the *current* published one means a reload that
        // landed during our walk is not re-triggered.
        let claim_reload = disk_fp.map(|fp| fp != published.fingerprint).unwrap_or(false)
            && published.reload != ReloadState::Running;
        if claim_reload {
            published.reload = ReloadState::Running;
            reload = "running";
        }
        drop(inner);

        if claim_reload {
            let state = self.clone();
            let spawned = std::thread::Builder::new()
                .name("bsl-graph-reload".to_owned())
                .spawn(move || state.run_load(true));
            if let Err(e) = spawned {
                let mut inner = lock_recover(&self.inner);
                if let Some(p) = inner.published.as_mut() {
                    p.reload = ReloadState::Failed(format!("could not spawn reload: {e}"));
                }
                reload = "failed";
            }
        }

        Freshness { revision: snapshot.generation, stale, reload }
    }

    /// The current on-disk fingerprint, throttled to one scan per drift interval.
    /// The scan mutex is held across the walk, so concurrent callers within the
    /// window block briefly then reuse the cached value instead of re-walking.
    /// `None` when no workspace is configured.
    fn current_disk_fp(&self) -> Option<u64> {
        let root = self.workspace_root.as_deref()?;
        let mut cache = lock_recover(&self.scan);
        if let Some(c) = cache.as_ref() {
            if c.at.elapsed() < self.drift_interval {
                return Some(c.disk_fp);
            }
        }
        let fp = workspace_fingerprint(root);
        *cache = Some(ScanCache { at: Instant::now(), disk_fp: fp });
        Some(fp)
    }

    /// Build (or rebuild) the database off-thread and publish it coherently.
    /// `is_reload` distinguishes the initial load (sets `Ready`, generation 1)
    /// from a drift-triggered reload (bumps the generation, keeps the old snapshot
    /// served on failure).
    fn run_load(&self, is_reload: bool) {
        let Some(workspace_root) = self.workspace_root.clone() else {
            return;
        };
        // The generation this build will carry. Only one load runs at a time (the
        // initial load, then at most one reload via the claim guard), so peeking the
        // current generation without reserving it is race-free; a failed build leaves
        // it unpublished and the next attempt reuses the same number.
        let generation =
            lock_recover(&self.inner).published.as_ref().map(|p| p.generation).unwrap_or(0) + 1;

        // On the initial load, reuse a cached build from a previous process run if it
        // still matches the workspace — turning a multi-minute rebuild into a stat
        // walk plus an open. A reload is skipped here: it only fires once drift has
        // been detected, so the on-disk file is known stale and must be rebuilt.
        if !is_reload && self.try_publish_cached(&workspace_root) {
            return;
        }

        // On reload the previous build is still on disk; classify which files drifted
        // against its stored per-file fingerprints. Observational for now (the full
        // rebuild below still runs) — the body-only fast path will branch on this.
        if is_reload {
            let stored = read_stored_fingerprints(&graph_db_path(&workspace_root));
            let diff = classify_changes(&stored, &scan_file_stats(&workspace_root));
            tracing::info!(
                added = diff.added.len(),
                removed = diff.removed.len(),
                modified = diff.modified.len(),
                xml = diff.touches_metadata(),
                empty = diff.is_empty(),
                "graph reload: classified workspace drift"
            );
        }

        tracing::info!(?workspace_root, is_reload, generation, "graph database build started");
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Bracket the build with two fingerprint scans. The build reads files
            // between them, so when `fp_pre == fp_post` the disk did not move and the
            // graph provably reflects exactly that state — publish it fresh. When they
            // differ the build straddled a write and is an indeterminate mix; we still
            // publish it but mark it `force_stale` so freshness reports it stale until
            // a clean reload replaces it, even under an ABA rollback to `fp_pre`.
            let fp_pre = workspace_fingerprint(&workspace_root);
            let out_path = graph_db_path(&workspace_root);
            // Build into a sibling temp file and rename atomically, so a reader always
            // sees a complete database — the previous one until the swap, never a
            // half-written file.
            let tmp_path = out_path.with_extension("db.building");
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let built_at = chrono::Utc::now().to_rfc3339();
            let summary = crate::graph_db::build_graph_database(
                &workspace_root,
                &tmp_path,
                GRAPH_BUILD_BATCH,
                &crate::graph_db::GraphMeta {
                    revision: generation,
                    fingerprint: fp_pre,
                    files: 0,
                    built_at,
                },
            )?;
            let fp_post = workspace_fingerprint(&workspace_root);
            let force_stale = fp_pre != fp_post;
            // Stamp the build-determined freshness into the file's own meta before
            // it is swapped in, so a served snapshot reads `force_stale` (and the
            // true file count) from the exact build it serves rather than from a
            // separately-locked field that a concurrent reload could desync.
            {
                let conn = rusqlite::Connection::open(&tmp_path)?;
                conn.execute(
                    "INSERT OR REPLACE INTO meta (key, value) VALUES ('force_stale', ?1)",
                    rusqlite::params![if force_stale { "1" } else { "0" }],
                )?;
                conn.execute(
                    "INSERT OR REPLACE INTO meta (key, value) VALUES ('files', ?1)",
                    rusqlite::params![summary.modules.to_string()],
                )?;
            }
            std::fs::rename(&tmp_path, &out_path)?;
            anyhow::Ok((summary.modules, fp_pre, force_stale))
        }));

        match outcome {
            Ok(Ok((files, fp_pre, force_stale))) => {
                if force_stale {
                    tracing::warn!(
                        is_reload,
                        "graph build straddled a disk write; marking snapshot stale to force reload"
                    );
                }
                // Drop the stale scan cache *before* publishing so a concurrent
                // freshness check re-scans against the new snapshot rather than a
                // pre-reload cached fingerprint.
                *lock_recover(&self.scan) = None;
                {
                    let mut inner = lock_recover(&self.inner);
                    inner.published = Some(Published {
                        generation,
                        fingerprint: fp_pre,
                        reload: ReloadState::Idle,
                    });
                    inner.status = GraphStatus::Ready { files };
                }
                tracing::info!(files, generation, is_reload, "graph database build complete");
            }
            Ok(Err(e)) => {
                let msg = e.to_string();
                tracing::warn!("graph database build failed: {msg}");
                self.record_load_failure(is_reload, msg);
            }
            Err(_) => {
                tracing::error!("graph database build panicked");
                self.record_load_failure(is_reload, "builder panicked".to_owned());
            }
        }
    }

    /// Publish an existing on-disk build instead of rebuilding, when it is still a
    /// valid, current, non-straddled match for the workspace. Returns `true` (and
    /// transitions to `Ready`) when the cache was reused; `false` to fall through to
    /// a full build. The fingerprint scan it runs is the same one the build would do.
    fn try_publish_cached(&self, workspace_root: &Path) -> bool {
        let path = graph_db_path(workspace_root);
        let Ok(graph) = GraphDb::open(&path) else {
            return false; // missing, truncated, or stale-schema → rebuild
        };
        let Ok((revision, fingerprint, force_stale)) = graph.freshness_token() else {
            return false;
        };
        let fp_now = workspace_fingerprint(workspace_root);
        // Reuse only an exact, clean match: a fingerprint mismatch means the
        // workspace moved since the build, and `force_stale` means the build
        // straddled a write and was never a coherent snapshot.
        if force_stale || fingerprint != fp_now {
            return false;
        }
        let files = graph.files().unwrap_or(0);

        *lock_recover(&self.scan) = None;
        let mut inner = lock_recover(&self.inner);
        inner.published =
            Some(Published { generation: revision, fingerprint, reload: ReloadState::Idle });
        inner.status = GraphStatus::Ready { files };
        tracing::info!(files, revision, "reused cached graph database (workspace unchanged)");
        true
    }

    /// A failed initial load surfaces as `Failed`; a failed reload keeps the
    /// previous snapshot but flags `reload="failed"` so the agent sees it. A
    /// later drift check retries the reload (the throttle bounds the retry rate).
    fn record_load_failure(&self, is_reload: bool, msg: String) {
        let mut inner = lock_recover(&self.inner);
        if is_reload {
            if let Some(p) = inner.published.as_mut() {
                p.reload = ReloadState::Failed(msg);
            }
        } else {
            inner.status = GraphStatus::Failed(msg);
        }
    }
}

/// Lock a mutex, recovering the inner value if a prior holder panicked. The graph
/// mutexes guard brief stores/reads (and one throttled scan), so a poisoned guard
/// still carries valid data.
fn lock_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The configuration source directory plus every extension directory — the file
/// universe both the loader and the drift scan must agree on.
fn scan_roots(workspace_root: &Path) -> Vec<PathBuf> {
    let project = project_model::Project::new(workspace_root);
    let mut roots = vec![project.source_path().to_path_buf()];
    roots.extend(project.extension_paths().iter().map(|(_, p)| p.clone()));
    roots
}

/// One graph-relevant file's stat-only identity: canonical `/`-normalised path,
/// mtime in nanos, and length. Produced once per scan and shared by the
/// whole-workspace fingerprint (which folds them) and the per-file `files` table
/// (which persists them for granular drift classification).
pub(crate) struct FileStat {
    pub(crate) path: String,
    mtime: u128,
    len: u64,
}

impl FileStat {
    /// The per-file fingerprint stored in (and compared against) the `files` table.
    /// Must stay deterministic across runs so a reload's recomputed value matches the
    /// stored one for an unchanged file.
    pub(crate) fn fingerprint(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        (self.mtime, self.len).hash(&mut hasher);
        hasher.finish()
    }
}

/// Stat every graph-relevant file (`.bsl` sources + `.xml` metadata descriptors)
/// under the scan roots, once. Covers both extensions because graph resolution
/// depends on configuration visibility registered from the metadata, not only on
/// module text. Uses `(canonical path, mtime, len)` — stat only, no file reads —
/// and mirrors the loader's scan roots and symlink/canonicalization policy so it
/// compares the same file universe (otherwise it would report phantom drift).
pub(crate) fn scan_file_stats(workspace_root: &Path) -> Vec<FileStat> {
    let mut stats: Vec<FileStat> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for root in scan_roots(workspace_root) {
        for entry in WalkDir::new(&root).follow_links(true) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            match entry.path().extension().and_then(|e| e.to_str()) {
                Some("bsl") | Some("xml") => {}
                _ => continue,
            }
            let path = entry.path().canonicalize().unwrap_or_else(|_| entry.path().to_path_buf());
            if !seen.insert(path.clone()) {
                continue;
            }
            let (mtime, len) = entry
                .metadata()
                .ok()
                .map(|m| {
                    let mtime = m
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_nanos())
                        .unwrap_or(0);
                    (mtime, m.len())
                })
                .unwrap_or((0, 0));
            stats.push(FileStat { path: path.to_string_lossy().into_owned(), mtime, len });
        }
    }

    stats
}

/// A cheap, order-independent fingerprint of the graph-relevant files on disk.
/// Folds every file's `(path, mtime, len)` into one `u64`; B4 cache reuse compares
/// it for an exact whole-workspace match.
fn workspace_fingerprint(workspace_root: &Path) -> u64 {
    let mut entries: Vec<(String, u128, u64)> =
        scan_file_stats(workspace_root).into_iter().map(|s| (s.path, s.mtime, s.len)).collect();
    entries.sort();
    let mut hasher = DefaultHasher::new();
    entries.hash(&mut hasher);
    hasher.finish()
}

/// Granular drift between a built graph's stored per-file fingerprints and the
/// current on-disk state. The body-only fast path acts on this; today it is computed
/// for observability while the full rebuild still runs.
pub(crate) struct WorkspaceDiff {
    pub(crate) added: Vec<String>,
    pub(crate) removed: Vec<String>,
    pub(crate) modified: Vec<String>,
}

impl WorkspaceDiff {
    pub(crate) fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }

    /// Whether any changed file is `.xml` metadata. Metadata drift can change
    /// configuration visibility for *any* module, so it forces a full rebuild — no
    /// fast path is sound for it.
    pub(crate) fn touches_metadata(&self) -> bool {
        self.added.iter().chain(&self.removed).chain(&self.modified).any(|p| p.ends_with(".xml"))
    }
}

/// Classify per-file drift between the stored fingerprint map (read from a built
/// graph's `files` table) and the current on-disk stats. A path present only on disk
/// is `added`, present only in the store is `removed`, present in both with a
/// different fingerprint is `modified`.
pub(crate) fn classify_changes(
    stored: &std::collections::HashMap<String, u64>,
    current: &[FileStat],
) -> WorkspaceDiff {
    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut seen: HashSet<&str> = HashSet::with_capacity(current.len());

    for stat in current {
        seen.insert(stat.path.as_str());
        match stored.get(&stat.path) {
            None => added.push(stat.path.clone()),
            Some(&fp) if fp != stat.fingerprint() => modified.push(stat.path.clone()),
            Some(_) => {}
        }
    }
    let mut removed: Vec<String> =
        stored.keys().filter(|p| !seen.contains(p.as_str())).cloned().collect();

    added.sort();
    modified.sort();
    removed.sort();
    WorkspaceDiff { added, removed, modified }
}

/// Read the stored per-file fingerprints from a built graph's `files` table. Any
/// open/query failure (missing file, older schema without the table) yields an empty
/// map, which classifies every current file as `added` → conservative full rebuild.
pub(crate) fn read_stored_fingerprints(db_path: &Path) -> std::collections::HashMap<String, u64> {
    let mut map = std::collections::HashMap::new();
    // Read-only open: never create the file as a side effect. A missing/older DB
    // errors here and yields an empty map → every current file classified `added`.
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return map;
    };
    let Ok(mut stmt) = conn.prepare("SELECT path, fingerprint FROM files") else {
        return map;
    };
    let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))
    else {
        return map;
    };
    for row in rows.flatten() {
        map.insert(row.0, row.1);
    }
    map
}

/// The configuration source + extension metadata paths the resolver needs for
/// visibility checks, registered on every database (full or per-batch) just like
/// the LSP workspace loader does.
pub(crate) fn config_metadata_paths(workspace_root: &Path) -> Vec<(Option<String>, PathBuf)> {
    let project = project_model::Project::new(workspace_root);
    let mut config_paths: Vec<(Option<String>, PathBuf)> =
        vec![(None, project.source_path().to_path_buf())];
    for (name, ext_path) in project.extension_paths() {
        config_paths.push((Some(name.clone()), ext_path.clone()));
    }
    config_paths
}

/// Enumerate every `.bsl` file under the config + extension roots, assigning a
/// stable [`FileId`] in walk order. No file text is read — this is the cheap
/// file-id↔path map that lets the graph build load one batch of texts at a time
/// while keeping ids consistent across batches.
pub(crate) fn enumerate_bsl_files(workspace_root: &Path) -> Vec<(FileId, PathBuf)> {
    let mut entries: Vec<(FileId, PathBuf)> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut next_id = 0u32;
    for root in scan_roots(workspace_root) {
        for entry in WalkDir::new(&root).follow_links(true) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("graph scan: walk error: {e}");
                    continue;
                }
            };
            if !entry.file_type().is_file()
                || entry.path().extension().and_then(|e| e.to_str()) != Some("bsl")
            {
                continue;
            }
            let path = entry.path().canonicalize().unwrap_or_else(|_| entry.path().to_path_buf());
            if !seen.insert(path.clone()) {
                continue;
            }
            entries.push((FileId(next_id), path));
            next_id += 1;
        }
    }
    entries
}

/// The whole-workspace source root: a file-id ↔ path map covering EVERY file, so
/// cross-module resolution through the module index can find any target's
/// [`FileId`]. Built once per build and shared (cheaply cloned — the map is
/// `Arc`-backed) into every per-batch database.
pub(crate) fn build_source_root(all_files: &[(FileId, PathBuf)]) -> SourceRoot {
    let mut file_set = FileSet::new();
    for (file_id, path) in all_files {
        file_set.insert(*file_id, VfsPath::new(path.clone()));
    }
    SourceRoot::new_local(file_set)
}

/// Build a batch database that shares the whole-workspace `source_root` (so any
/// target is addressable by path through the module index) but loads text only for
/// `batch_files` — the only modules this database lowers.
///
/// `file_source_root` is set ONLY for `batch_files`: the per-file source-root input
/// is read solely for the file being lowered (resolver / infer / `get_file_path`),
/// and the build never lowers a non-batch file. Cross-batch call targets resolve
/// through the path-keyed module index built from the shared source root, which
/// never consults `file_source_root`. Setting it for all files would re-pay a
/// whole-config-sized loop on every batch database for no resolution benefit.
pub(crate) fn db_for_files(
    source_root: &SourceRoot,
    batch_files: &[(FileId, PathBuf)],
    config_paths: &[(Option<String>, PathBuf)],
    config_cache: Option<&Arc<ide::GraphConfigCache>>,
) -> RootDatabaseImpl {
    let mut db = RootDatabaseImpl::default();
    if let Some(cache) = config_cache {
        db.set_graph_config_cache(Arc::clone(cache));
    }
    db.set_source_root(GRAPH_SOURCE_ROOT, source_root.clone());
    for (file_id, path) in batch_files {
        db.set_file_source_root(*file_id, GRAPH_SOURCE_ROOT);
        match std::fs::read_to_string(path) {
            Ok(text) => db.set_file_text(*file_id, &text),
            Err(e) => {
                tracing::warn!(path = %path.display(), "graph scan: read failed: {e}");
                db.set_file_text(*file_id, "");
            }
        }
    }
    db.set_all_config_paths(config_paths.to_vec());
    db
}

/// Walk the configuration source and extension directories, load every `.bsl`
/// file into a fresh database, and register the config metadata paths. Test-only:
/// the production graph is built straight into SQLite per batch, never as one
/// whole-config in-memory database.
#[cfg(test)]
fn load_workspace_db(workspace_root: &Path) -> anyhow::Result<(RootDatabaseImpl, usize)> {
    let files = enumerate_bsl_files(workspace_root);
    let config_paths = config_metadata_paths(workspace_root);
    let source_root = build_source_root(&files);
    let db = db_for_files(&source_root, &files, &config_paths, None);
    Ok((db, files.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_db::build_graph_database;
    use ide::Analysis;
    use rusqlite::Connection;
    use std::fs;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// Minimal common-module metadata descriptor so the module is declared in the
    /// configuration (the resolver refuses qualified calls to undeclared modules)
    /// and its client/server execution context is known.
    fn write_common_module(root: &Path, name: &str, server: bool, body: &str) {
        let client = !server;
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core">
	<CommonModule uuid="00000000-0000-0000-0000-0000000000{id:02}">
		<Properties>
			<Name>{name}</Name>
			<Global>false</Global>
			<ClientManagedApplication>{client}</ClientManagedApplication>
			<Server>{server}</Server>
			<ExternalConnection>false</ExternalConnection>
			<ClientOrdinaryApplication>{client}</ClientOrdinaryApplication>
			<ServerCall>false</ServerCall>
			<Privileged>false</Privileged>
			<ReturnValuesReuse>DontUse</ReturnValuesReuse>
		</Properties>
	</CommonModule>
</MetaDataObject>"#,
            id = name.len(),
        );
        write(root, &format!("CommonModules/{name}.xml"), &xml);
        write(root, &format!("CommonModules/{name}/Ext/Module.bsl"), body);
    }

    fn sample_workspace(root: &Path) {
        write_common_module(
            root,
            "Клиент",
            false,
            "&НаКлиенте\nПроцедура Главная() Экспорт\nСервер.Считать();\nКонецПроцедуры",
        );
        write_common_module(
            root,
            "Сервер",
            true,
            "&НаСервере\nФункция Считать() Экспорт КонецФункции",
        );
    }

    fn wait_ready(graph: &GraphState) {
        for _ in 0..200 {
            match graph.status() {
                GraphStatus::Ready { .. } => return,
                GraphStatus::Failed(msg) => panic!("graph load failed: {msg}"),
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        panic!("graph did not become ready");
    }

    /// End-to-end through `GraphState`: a first use builds the SQLite graph off
    /// the workspace and serves overview/node/neighbors from the opened handle.
    #[test]
    fn loads_workspace_and_serves_graph() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let graph = GraphState::for_workspace(root.to_path_buf());
        graph.ensure_loading();
        wait_ready(&graph);
        let snap = graph.snapshot().expect("ready graph snapshots an opened handle");
        let gdb = &snap.graph;

        let overview = gdb.overview(10).expect("overview");
        assert_eq!(overview.edges, 1, "Клиент.Главная → Сервер.Считать is one resolved edge");
        assert_eq!(overview.client_to_server_edges, 1);

        let node = gdb
            .node("method/common/Сервер/Считать", ide::GraphDetail::Names)
            .expect("query")
            .expect("durable id resolves from the on-disk graph");
        assert_eq!(node.node.name, "Считать");
        assert_eq!(node.node.dispatch, vec!["server"]);

        // Callers traversal reaches the client method via the resolved edge.
        let callers = gdb
            .neighbors(&ide::NeighborsParams {
                id: "method/common/Сервер/Считать",
                dir: ide::Direction::In,
                depth: 1,
                max_nodes: 50,
                detail: ide::GraphDetail::Names,
                provenance_filter: Vec::new(),
            })
            .expect("query")
            .expect("neighbors resolve");
        assert!(callers.nodes.iter().any(|n| n.id == "method/common/Клиент/Главная"));
    }

    /// Seed a graph database at the workspace's cache path as a prior process run
    /// would, with a distinctive `revision`/`built_at` so a test can tell a reused
    /// cache from a fresh rebuild.
    fn seed_cache(root: &Path, fingerprint: u64) {
        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            root,
            &out,
            GRAPH_BUILD_BATCH,
            &crate::graph_db::GraphMeta {
                revision: 7,
                fingerprint,
                files: 0,
                built_at: "cached-build-sentinel".to_string(),
            },
        )
        .expect("seed cache builds");
    }

    fn meta_string(path: &Path, key: &str) -> String {
        Connection::open(path)
            .unwrap()
            .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
            .unwrap()
    }

    /// A cached build that still matches the workspace is republished as-is — no
    /// rebuild — so its `revision` and `built_at` survive the load.
    #[test]
    fn reuses_a_matching_cached_build() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        seed_cache(root, workspace_fingerprint(root));

        let graph = GraphState::for_workspace(root.to_path_buf());
        graph.ensure_loading();
        wait_ready(&graph);

        // Reused: the served revision is the cache's (7); a rebuild would reset it to 1.
        let snap = graph.snapshot().expect("ready graph snapshots");
        assert_eq!(snap.generation, 7, "served the cached revision, not a fresh build");
        // The file was not rewritten — its build timestamp is untouched.
        assert_eq!(meta_string(&graph_db_path(root), "built_at"), "cached-build-sentinel");
    }

    /// A cached build whose fingerprint no longer matches the workspace (it moved
    /// since the build) is discarded and rebuilt from scratch.
    #[test]
    fn rebuilds_when_cached_fingerprint_differs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        seed_cache(root, workspace_fingerprint(root).wrapping_add(1));

        let graph = GraphState::for_workspace(root.to_path_buf());
        graph.ensure_loading();
        wait_ready(&graph);

        let snap = graph.snapshot().expect("ready graph snapshots");
        assert_eq!(snap.generation, 1, "stale cache discarded and rebuilt at generation 1");
        assert_ne!(meta_string(&graph_db_path(root), "built_at"), "cached-build-sentinel");
    }

    /// A cached build flagged `force_stale` (it straddled a disk write and was never
    /// a coherent snapshot) is never reused even if its fingerprint matches.
    #[test]
    fn rebuilds_when_cached_build_is_force_stale() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        let fp = workspace_fingerprint(root);
        seed_cache(root, fp);
        Connection::open(graph_db_path(root))
            .unwrap()
            .execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('force_stale', '1')", [])
            .unwrap();

        let graph = GraphState::for_workspace(root.to_path_buf());
        graph.ensure_loading();
        wait_ready(&graph);

        let snap = graph.snapshot().expect("ready graph snapshots");
        assert_eq!(snap.generation, 1, "force_stale cache rebuilt at generation 1");
        assert_ne!(meta_string(&graph_db_path(root), "built_at"), "cached-build-sentinel");
    }

    /// The streaming SQLite build must reproduce the in-memory graph: identical
    /// node-kind tallies, edge counts, durable ids, dispatch and in-degree.
    #[test]
    fn sqlite_build_matches_in_memory_graph() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (db, files) = load_workspace_db(root).expect("workspace loads");
        let analysis = Analysis::from_database(db.clone());
        let overview = analysis.graph_overview(GRAPH_SOURCE_ROOT, Some(root), 10);

        let out = root.join(".build/bsl-graph.db");
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        let summary = build_graph_database(
            root,
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: 0,
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        assert_eq!(summary.edges, overview.edges);

        let conn = Connection::open(&out).unwrap();
        let count = |sql: &str| -> usize {
            conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap() as usize
        };

        assert_eq!(count("SELECT COUNT(*) FROM nodes"), overview.nodes);
        assert_eq!(count("SELECT COUNT(*) FROM nodes WHERE kind='method'"), overview.methods);
        assert_eq!(count("SELECT COUNT(*) FROM nodes WHERE kind='module'"), overview.modules);
        assert_eq!(count("SELECT COUNT(*) FROM nodes WHERE kind='mdo'"), overview.mdos);
        assert_eq!(count("SELECT COUNT(*) FROM nodes WHERE kind='attribute'"), overview.attributes);
        assert_eq!(count("SELECT COUNT(*) FROM edges"), overview.edges);
        assert_eq!(
            count("SELECT COUNT(*) FROM edges WHERE crosses=1"),
            overview.client_to_server_edges
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM edges WHERE provenance='resolved'"),
            *overview.edge_provenance.get("resolved").unwrap_or(&0)
        );

        let (name, dispatch): (String, String) = conn
            .query_row(
                "SELECT name, dispatch FROM nodes WHERE id = ?1",
                rusqlite::params!["method/common/Сервер/Считать"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((name.as_str(), dispatch.as_str()), ("Считать", "server"));

        let in_degree: i64 = conn
            .query_row(
                "SELECT degree FROM in_degree WHERE id = 'method/common/Сервер/Считать'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(in_degree, 1, "Сервер.Считать is called once");
    }

    /// A metadata object reached by a manager call in one module and by an SDBL
    /// query in another, across separate batches (`batch_size = 1`), must get the
    /// SAME durable `Mdo` node id from the streaming build as the in-memory fold.
    /// The build runs call edges across all batches before query edges, mirroring
    /// the fold's Pass-2-then-Pass-3 order, so the first-seen (canonical) spelling —
    /// and thus the id — cannot diverge even when the call and query sites differ in
    /// case.
    #[test]
    fn cross_batch_mdo_node_id_matches_fold() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write(
            root,
            "Catalogs/Номенклатура.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-000000000001">
        <Properties><Name>Номенклатура</Name><CodeLength>9</CodeLength></Properties>
    </Catalog>
</MetaDataObject>"#,
        );
        // One module creates via the manager (canonical case), another reads it in a
        // query (upper case). Their batch order is fixed by walk order; the build's
        // global call-before-query order decides the canonical spelling regardless.
        write(
            root,
            "CommonModules/Менеджер/Ext/Module.bsl",
            "Процедура Создать() Экспорт\nСправочники.Номенклатура.СоздатьЭлемент();\nКонецПроцедуры",
        );
        write(
            root,
            "CommonModules/Отчет/Ext/Module.bsl",
            "Процедура Читать() Экспорт\n\
             Запрос = \"ВЫБРАТЬ Код ИЗ Справочник.НОМЕНКЛАТУРА\";\nКонецПроцедуры",
        );

        let (db, files) = load_workspace_db(root).expect("workspace loads");
        let analysis = Analysis::from_database(db);
        let fold = analysis.graph_overview(GRAPH_SOURCE_ROOT, Some(root), 50);
        let fold_mdo: Vec<&str> = fold
            .top_by_centrality
            .iter()
            .filter(|n| n.kind == "mdo")
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(fold_mdo.len(), 1, "exactly one catalog Mdo node in the fold: {fold_mdo:?}");
        let fold_id = fold_mdo[0];

        let out = root.join(".build/bsl-graph.db");
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            root,
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: 0,
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");

        let conn = Connection::open(&out).unwrap();
        let sqlite_mdo: Vec<String> = {
            let mut stmt = conn.prepare("SELECT id FROM nodes WHERE kind='mdo'").unwrap();
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(sqlite_mdo.len(), 1, "exactly one catalog Mdo node in SQLite: {sqlite_mdo:?}");
        assert_eq!(
            sqlite_mdo[0], fold_id,
            "cross-batch Mdo node id must be byte-identical to the in-memory fold's"
        );
    }

    /// Serving overview/node/neighbors/source from the SQLite store must produce
    /// JSON byte-identical to the in-memory `ide::Analysis::graph_*` path it
    /// replaces — same fields, signatures, bodies, edges and budget behaviour.
    #[test]
    fn sqlite_serving_matches_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (db, files) = load_workspace_db(root).expect("workspace loads");
        let analysis = Analysis::from_database(db);

        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            root,
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: 0,
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let gdb = GraphDb::open(&out).expect("graph database opens and validates");

        let id = "method/common/Сервер/Считать";

        let mem_overview =
            serde_json::to_value(analysis.graph_overview(GRAPH_SOURCE_ROOT, Some(root), 10))
                .unwrap();
        let sql_overview = serde_json::to_value(gdb.overview(10).unwrap()).unwrap();
        assert_eq!(mem_overview, sql_overview, "overview JSON");

        let mem_node = serde_json::to_value(
            analysis
                .graph_node(GRAPH_SOURCE_ROOT, Some(root), id, ide::GraphDetail::Bodies)
                .unwrap(),
        )
        .unwrap();
        let sql_node =
            serde_json::to_value(gdb.node(id, ide::GraphDetail::Bodies).unwrap().unwrap()).unwrap();
        assert_eq!(mem_node, sql_node, "node JSON (bodies detail)");

        let params = ide::NeighborsParams {
            id,
            dir: ide::Direction::In,
            depth: 1,
            max_nodes: 50,
            detail: ide::GraphDetail::Signatures,
            provenance_filter: Vec::new(),
        };
        let mem_nb = serde_json::to_value(
            analysis.graph_neighbors(GRAPH_SOURCE_ROOT, Some(root), &params).unwrap(),
        )
        .unwrap();
        let sql_nb = serde_json::to_value(gdb.neighbors(&params).unwrap().unwrap()).unwrap();
        assert_eq!(mem_nb, sql_nb, "neighbors JSON");

        let ids = [id.to_string()];
        let mem_src =
            serde_json::to_value(analysis.graph_source(GRAPH_SOURCE_ROOT, Some(root), &ids, 4000))
                .unwrap();
        let sql_src = serde_json::to_value(gdb.source(&ids, 4000).unwrap()).unwrap();
        assert_eq!(mem_src, sql_src, "source JSON");

        // A malformed/unknown id reports NotFound, not an infra error.
        let missing = gdb.node("method/common/Нет/Метод", ide::GraphDetail::Names).unwrap();
        assert!(missing.is_err(), "unknown id resolves to a GraphError");
    }

    /// The build parallelises per-module resolution within a batch. A batch holding
    /// several modules that call each other and touch the same metadata object must
    /// still produce the fold's graph exactly — same edges, and the shared `Mdo`
    /// node spelled by whichever module the deterministic (file-order) projection
    /// sees first. Built with a batch large enough to hold every module at once, so
    /// the concurrent `map_with` path is exercised, not the one-module-per-batch case.
    #[test]
    fn parallel_multi_module_batch_matches_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write(
            root,
            "Catalogs/Номенклатура.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-000000000001">
        <Properties><Name>Номенклатура</Name><CodeLength>9</CodeLength></Properties>
    </Catalog>
</MetaDataObject>"#,
        );
        // Both modules touch the catalog through both edge passes — a manager call
        // (Pass 2) and a query (Pass 3) — so the parallel collection of call summaries
        // AND of SDBL query refs is exercised across multiple modules in one batch.
        write_common_module(
            root,
            "Альфа",
            true,
            "&НаСервере\nПроцедура ШагА() Экспорт\nБета.ШагБ();\nСправочники.Номенклатура.СоздатьЭлемент();\nЗапрос = \"ВЫБРАТЬ Код ИЗ Справочник.Номенклатура\";\nКонецПроцедуры",
        );
        write_common_module(
            root,
            "Бета",
            true,
            "&НаСервере\nПроцедура ШагБ() Экспорт\nЗапрос = \"ВЫБРАТЬ Наименование ИЗ Справочник.Номенклатура\";\nКонецПроцедуры",
        );

        let (db, files) = load_workspace_db(root).expect("workspace loads");
        let analysis = Analysis::from_database(db);

        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        // A batch_size far above the module count puts every module in one batch.
        build_graph_database(
            root,
            &out,
            100,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: 0,
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let gdb = GraphDb::open(&out).expect("graph database opens");

        // Overview parity covers node/edge tallies, provenance, and the
        // centrality ranking (whose nodes carry the canonical Mdo spelling).
        let mem_overview =
            serde_json::to_value(analysis.graph_overview(GRAPH_SOURCE_ROOT, Some(root), 10))
                .unwrap();
        let sql_overview = serde_json::to_value(gdb.overview(10).unwrap()).unwrap();
        assert_eq!(mem_overview, sql_overview, "overview JSON from a multi-module batch");
        // Guard the coverage: the query pass really produced edges across the batch,
        // so the parallel SDBL collection path is genuinely exercised, not vacuous.
        assert!(
            sql_overview["edge_provenance"]["inferred"].as_u64().unwrap_or(0) >= 2,
            "both modules' queries yield inferred query_ref edges: {sql_overview}"
        );

        // The single catalog Mdo node is reached identically from both modules.
        let mdo_id = "mdo/Catalog/Номенклатура";
        let params = ide::NeighborsParams {
            id: mdo_id,
            dir: ide::Direction::In,
            depth: 1,
            max_nodes: 50,
            detail: ide::GraphDetail::Names,
            provenance_filter: Vec::new(),
        };
        let mem_nb = serde_json::to_value(
            analysis.graph_neighbors(GRAPH_SOURCE_ROOT, Some(root), &params).unwrap(),
        )
        .unwrap();
        let sql_nb = serde_json::to_value(gdb.neighbors(&params).unwrap().unwrap()).unwrap();
        assert_eq!(mem_nb, sql_nb, "Mdo neighbours from a multi-module batch");
    }

    /// When `max_nodes` cuts through a set of equal-centrality neighbours, the
    /// in-memory and SQLite paths must keep/drop the *same* nodes — both rank by
    /// `(in_degree desc, durable id asc)`. Guards the tie-break parity.
    #[test]
    fn neighbors_tie_break_matches_across_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_common_module(root, "Ядро", true, "&НаСервере\nФункция Цель() Экспорт КонецФункции");
        // Three callers, each with in-degree 0 — a three-way centrality tie.
        write_common_module(
            root,
            "Вызовы",
            true,
            "&НаСервере\n\
             Процедура А() Экспорт Ядро.Цель(); КонецПроцедуры\n\
             Процедура Б() Экспорт Ядро.Цель(); КонецПроцедуры\n\
             Процедура В() Экспорт Ядро.Цель(); КонецПроцедуры",
        );

        let (db, files) = load_workspace_db(root).expect("workspace loads");
        let analysis = Analysis::from_database(db);

        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            root,
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: 0,
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let gdb = GraphDb::open(&out).expect("graph database opens");

        let params = ide::NeighborsParams {
            id: "method/common/Ядро/Цель",
            dir: ide::Direction::In,
            depth: 1,
            max_nodes: 1,
            detail: ide::GraphDetail::Names,
            provenance_filter: Vec::new(),
        };
        let mem = analysis.graph_neighbors(GRAPH_SOURCE_ROOT, Some(root), &params).unwrap();
        let sql = gdb.neighbors(&params).unwrap().unwrap();

        assert_eq!(mem.total, 3, "all three tied callers counted");
        assert_eq!(mem.nodes.len(), 1);
        assert_eq!(mem.dropped.len(), 2);
        // The cut resolves identically on both paths, not just by count.
        assert_eq!(
            serde_json::to_value(&mem).unwrap(),
            serde_json::to_value(&sql).unwrap(),
            "tie-break keeps/drops the same nodes on both paths"
        );
    }

    /// The SQLite reader must keep the in-memory resolver's id semantics: a
    /// malformed id is `BadId` (not `NotFound`), and a metadata id resolves
    /// case-insensitively on its type and object name.
    #[test]
    fn sqlite_serving_bad_id_and_case_insensitive_mdo() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("Configuration.xml"), "<Configuration/>").unwrap();
        write(
            root,
            "Catalogs/Номенклатура.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-000000000001">
        <Properties><Name>Номенклатура</Name><CodeLength>9</CodeLength></Properties>
    </Catalog>
</MetaDataObject>"#,
        );
        write(
            root,
            "CommonModules/Менеджер/Ext/Module.bsl",
            "Процедура Создать() Экспорт\nСправочники.Номенклатура.СоздатьЭлемент();\nКонецПроцедуры",
        );

        let files = enumerate_bsl_files(root).len();
        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            root,
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: 0,
                files,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let gdb = GraphDb::open(&out).expect("opens");

        let canonical = gdb
            .overview(50)
            .unwrap()
            .top_by_centrality
            .iter()
            .find(|n| n.kind == "mdo")
            .map(|n| n.id.clone())
            .expect("a catalog Mdo node");
        assert_eq!(canonical, "mdo/Catalog/Номенклатура");

        // Case-insensitive on the object name and ASCII type segment, and accepting
        // a localized type spelling (Справочник → Catalog).
        for variant in
            ["mdo/Catalog/НОМЕНКЛАТУРА", "mdo/catalog/номенклатура", "mdo/Справочник/Номенклатура"]
        {
            let r = gdb
                .node(variant, ide::GraphDetail::Names)
                .unwrap()
                .unwrap_or_else(|e| panic!("{variant} should resolve, got {e:?}"));
            assert_eq!(r.node.id, canonical, "{variant} resolves to the canonical node");
        }

        // Malformed ids are BadId, not NotFound.
        for garbage in ["garbage", "mdo/NoSuchType/X", "method/file/x"] {
            assert!(
                matches!(
                    gdb.node(garbage, ide::GraphDetail::Names).unwrap(),
                    Err(ide::GraphError::BadId { .. })
                ),
                "{garbage} must be BadId"
            );
        }
        // Well-formed but absent → NotFound.
        assert!(matches!(
            gdb.node("method/common/Нет/М", ide::GraphDetail::Names).unwrap(),
            Err(ide::GraphError::NotFound { .. })
        ));
    }

    #[test]
    fn fingerprint_changes_on_bsl_edit_and_xml_edit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let base = workspace_fingerprint(root);

        // A `.bsl` body edit (different length) shifts the fingerprint.
        write(
            root,
            "CommonModules/Сервер/Ext/Module.bsl",
            "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции",
        );
        let after_bsl = workspace_fingerprint(root);
        assert_ne!(base, after_bsl, "a .bsl edit must change the fingerprint");

        // A `.xml` metadata edit must also shift it — graph resolution depends on
        // configuration metadata, not only module text.
        write(root, "CommonModules/Сервер.xml", "<MetaDataObject/>");
        let after_xml = workspace_fingerprint(root);
        assert_ne!(after_bsl, after_xml, "a .xml metadata edit must change the fingerprint");
    }

    /// A build persists a per-file fingerprint for every `.bsl` AND `.xml` file, so
    /// a later reload can classify drift granularly. `sig_hash` is NULL for now.
    #[test]
    fn build_persists_per_file_fingerprints_for_bsl_and_xml() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            root,
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: 0,
                files: 0,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");

        let conn = Connection::open(&out).unwrap();
        let bsl: i64 = conn
            .query_row("SELECT COUNT(*) FROM files WHERE path LIKE '%.bsl'", [], |r| r.get(0))
            .unwrap();
        let xml: i64 = conn
            .query_row("SELECT COUNT(*) FROM files WHERE path LIKE '%.xml'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(bsl, 2, "both common-module bodies are fingerprinted");
        assert_eq!(xml, 2, "both common-module descriptors are fingerprinted");

        // The stored fingerprints match a fresh stat-scan: an unchanged workspace
        // classifies as an empty diff.
        let stored = read_stored_fingerprints(&out);
        assert_eq!(stored.len(), 4);
        let diff = classify_changes(&stored, &scan_file_stats(root));
        assert!(
            diff.is_empty(),
            "unchanged workspace ⇒ empty diff: {:?}",
            (&diff.added, &diff.removed, &diff.modified)
        );

        let null_sigs: i64 = conn
            .query_row("SELECT COUNT(*) FROM files WHERE sig_hash IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(null_sigs, 0, "sig_hash stays NULL in this phase");
    }

    /// `classify_changes` sorts each modified/added/removed file into the right
    /// bucket, and `.xml` drift is flagged for the (forced) full-rebuild path.
    #[test]
    fn classify_changes_buckets_add_remove_modify_and_flags_xml() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let out = graph_db_path(root);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        build_graph_database(
            root,
            &out,
            1,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: 0,
                files: 0,
                built_at: "t".to_string(),
            },
        )
        .expect("graph database builds");
        let stored = read_stored_fingerprints(&out);

        // Modify one body, add a new module, remove an existing one.
        write(
            root,
            "CommonModules/Сервер/Ext/Module.bsl",
            "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции",
        );
        write_common_module(
            root,
            "Новый",
            true,
            "&НаСервере\nПроцедура П() Экспорт КонецПроцедуры",
        );
        fs::remove_file(root.join("CommonModules/Клиент/Ext/Module.bsl")).unwrap();

        let diff = classify_changes(&stored, &scan_file_stats(root));
        assert!(!diff.is_empty());

        let ends = |v: &[String], suffix: &str| v.iter().filter(|p| p.ends_with(suffix)).count();
        assert_eq!(ends(&diff.modified, "Сервер/Ext/Module.bsl"), 1, "edited body is modified");
        assert_eq!(ends(&diff.added, "Новый/Ext/Module.bsl"), 1, "new body is added");
        // The new module also drops a new `.xml` descriptor → metadata drift.
        assert_eq!(ends(&diff.added, "Новый.xml"), 1, "new descriptor is added");
        assert_eq!(ends(&diff.removed, "Клиент/Ext/Module.bsl"), 1, "deleted body is removed");
        assert!(diff.touches_metadata(), "an added .xml descriptor forces the full-rebuild path");

        // A modified-only `.bsl` (no add/remove, no `.xml`) does NOT flag metadata.
        let body_only = WorkspaceDiff {
            added: vec![],
            removed: vec![],
            modified: vec!["/cfg/SomeModule/Ext/Module.bsl".to_string()],
        };
        assert!(!body_only.touches_metadata(), "a body-only change does not touch metadata");
    }

    #[test]
    fn drift_marks_stale_and_async_reload_bumps_generation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut graph = GraphState::for_workspace(root.to_path_buf());
        // Disable throttling so each check actually scans.
        graph.drift_interval = Duration::ZERO;
        graph.ensure_loading();
        wait_ready(&graph);

        let snap1 = graph.snapshot().expect("ready graph snapshots");
        let fresh = graph.freshness(&snap1);
        assert_eq!(fresh.revision, 1);
        assert!(!fresh.stale);
        assert_eq!(fresh.reload, "none");

        // Edit a module on disk: a freshness check against the old snapshot must
        // read as stale and kick a reload that publishes a bumped generation.
        write(
            root,
            "CommonModules/Сервер/Ext/Module.bsl",
            "&НаСервере\nФункция Считать() Экспорт Возврат 42; КонецФункции",
        );
        let drifted = graph.freshness(&snap1);
        assert!(drifted.stale, "an on-disk edit must read as stale");
        assert_eq!(drifted.revision, 1, "the stale response still serves the old generation");
        assert!(matches!(drifted.reload, "running" | "failed"));

        // The async reload publishes generation 2; a fresh snapshot is then clean.
        let mut settled = None;
        for _ in 0..200 {
            let snap = graph.snapshot().expect("snapshot");
            if snap.generation == 2 {
                settled = Some(graph.freshness(&snap));
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let settled = settled.expect("reload did not publish a new generation");
        assert!(!settled.stale);
        assert_eq!(settled.revision, 2);
        assert_eq!(settled.reload, "none");
    }
}
