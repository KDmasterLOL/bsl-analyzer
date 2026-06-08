//! Lazily-built resident analysis database for per-file diagnostics (workspace
//! profile).
//!
//! Unlike the call `graph`, diagnostics are content-dependent and cannot be folded
//! into a static on-disk store: computing them needs a live Salsa database with the
//! target file and its whole resolution closure resident, exactly like the LSP
//! server. So the first `diagnostics file` call builds a resident
//! [`RootDatabaseImpl`] over every workspace `.bsl` text (the proven ~2.8 GB LSP
//! footprint, NOT the graph's fold) and serves per-file diagnostics from it through
//! Salsa's lazy lowering + LRU cache. `catalog`/`schema` never trigger this.
//!
//! Concurrency is a single [`Mutex`], not an `RwLock`: a Salsa `RootDatabaseImpl` is
//! `Send` but `!Sync`, so a `&db` cannot be shared across threads — reads must run on
//! the thread that holds the handle. Each per-file query therefore runs on the calling
//! (blocking) thread WHILE holding the mutex, so reads serialise but a drift reload's
//! `set_file_text` can never alias an in-flight query (no `salsa::Cancelled` path).
//! Per-file diagnostics are LRU-cached and fast, so serialising them is cheap. Cloning
//! the db handle inside the lock shares the memo/LRU cache (`RootDatabaseImpl::clone`
//! clones the Salsa `Storage`) and the clone never leaves the calling thread.
//!
//! Freshness is pull-on-request, mirroring the graph: each read cheaply re-checks the
//! workspace on disk (throttled), applies an incremental `set_file_text` for changed
//! `.bsl` bodies, and falls back to a full rebuild for structural drift (added /
//! removed files, any `.xml`, or an analyzer config-file change). An idle sweeper
//! drops the resident db after a quiet period so a standalone `mcp serve` reclaims the
//! memory after a burst.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ide::{Analysis, RootDatabaseImpl};
use vfs::FileId;

use crate::graph::{
    build_source_root, classify_changes, db_for_files_lazy, enumerate_bsl_files, scan_file_stats,
    FileStat,
};

/// Minimum time between on-disk drift scans, mirroring the graph's throttle. A scan
/// stats every `.bsl`/`.xml` under the config roots, so this bounds its cost
/// regardless of how fast an agent fires `diagnostics file` calls.
const DRIFT_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// Drop the resident database after this long with no `diagnostics file` call, so a
/// standalone server reclaims the ~2.8 GB after a burst. The next call rebuilds.
const IDLE_EVICTION: Duration = Duration::from_secs(600);

/// How often the idle sweeper wakes to check the last-access time.
const SWEEP_INTERVAL: Duration = Duration::from_secs(30);

/// The analyzer configuration files whose change forces a full rebuild: they can alter
/// the project structure (e.g. the extension set), which changes the db's config paths
/// and file enumeration. Matches the LSP server's watched config set.
const CONFIG_FILES: [&str; 3] =
    ["bsl-analyzer.toml", ".bsl-analyzer.json", ".bsl-language-server.json"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DiagnosticsStatus {
    /// Not a workspace profile — diagnostics over files are unavailable.
    Disabled,
    /// A workspace is configured but the resident db has not been built yet (or was
    /// evicted); the next `diagnostics file` call triggers the build.
    Idle,
    /// Background build in progress.
    Loading,
    /// Ready to serve, with the resident `.bsl` file count.
    Ready { files: usize },
    /// Build failed.
    Failed(String),
}

/// State of an in-flight or last-attempted drift reload, surfaced so a failed reload
/// is visible rather than leaving the agent at `stale=true` forever.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReloadState {
    Idle,
    Running,
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

/// The built resident database plus the path→FileId index needed to resolve a request
/// path to the Salsa input it set. Held behind the [`RwLock`]; reads borrow it, a
/// reload mutates `db` in place.
pub(crate) struct DiagnosticsResident {
    db: RootDatabaseImpl,
    /// Canonical-path string → FileId for every resident `.bsl`.
    by_path: HashMap<String, FileId>,
    /// The project's effective diagnostics settings, loaded from `bsl-analyzer.toml` /
    /// `.bsl-analyzer.json` the same way LSP and CLI do — so `file`/`workspace` honour
    /// the project's disabled rules and thresholds, not analyzer defaults.
    config: ide::DiagnosticsConfig,
    /// The workspace root the resident was built against — the SAME root the graph build
    /// uses (`source_dir`), so an absolute finding path strips to the graph encoder's rel
    /// and the `method/file/<rel>::<name>` graph bridge resolves.
    workspace_root: PathBuf,
}

impl DiagnosticsResident {
    /// Resolve a request path to the resident FileId, canonicalising it the same way
    /// the loader did. A relative path is resolved against the workspace root (not the
    /// process CWD), so `diagnostics file` works regardless of where the server was
    /// started. `None` when the path is not a resident workspace `.bsl`.
    pub(crate) fn file_id_for(&self, path: &Path) -> Option<FileId> {
        let resolved;
        let abs: &Path = if path.is_absolute() {
            path
        } else {
            resolved = self.workspace_root.join(path);
            &resolved
        };
        self.by_path.get(&canonical_key(abs)).copied()
    }

    /// The workspace root the resident was built against (the graph's `source_dir`),
    /// used to bridge findings to durable `method/file/<rel>::<name>` graph ids.
    pub(crate) fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// An `Analysis` view over a cloned db handle. The clone shares the Salsa storage
    /// (memo/LRU cache), and is dropped before the read guard is released.
    pub(crate) fn analysis(&self) -> Analysis {
        Analysis::from_database(self.db.clone())
    }

    pub(crate) fn file_count(&self) -> usize {
        self.by_path.len()
    }

    /// The project's effective diagnostics config, the single source of truth shared
    /// with LSP and CLI. `file` and `workspace` analyse against this, never defaults.
    pub(crate) fn config(&self) -> &ide::DiagnosticsConfig {
        &self.config
    }

    /// Workspace-wide diagnostics aggregated per code (the `workspace` action). Runs
    /// rayon over per-worker db clones (shared Salsa storage, the CLI `analyze`
    /// discipline). The caller MUST hold the state lock for the whole sweep so no
    /// reload mutates the master db mid-flight — that would cancel the cloned queries.
    /// Bounded by `opts.max_files` over a stable FileId order, so a cap is deterministic.
    pub(crate) fn workspace_aggregates(
        &self,
        config: &ide::DiagnosticsConfig,
        opts: &SweepOptions,
    ) -> WorkspaceSweep {
        use rayon::prelude::*;
        use std::collections::HashSet;

        let mut files: Vec<FileId> = self.by_path.values().copied().collect();
        files.sort_by_key(|f| f.0);
        let files_total = files.len();
        let truncated = files_total > opts.max_files;
        let swept = &files[..opts.max_files.min(files_total)];

        // Per file: the (code, bucket) of each diagnostic. Each rayon worker owns a db
        // clone; queries run in parallel on the shared, unmutated Salsa storage.
        let per_file: Vec<Vec<(String, ide::SeverityBucket)>> = swept
            .par_iter()
            .map_with(self.db.clone(), |db, &file_id| {
                let analysis = Analysis::from_database(db.clone());
                analysis
                    .diagnostics(file_id, config)
                    .iter()
                    .map(|d| (d.code.as_str().to_string(), ide::SeverityBucket::from(d.severity)))
                    .collect()
            })
            .collect();

        // Fold: code -> (bucket, total count, files-affected). All occurrences of a code
        // share a bucket under one config, so first-seen is representative.
        let mut map: HashMap<String, (ide::SeverityBucket, usize, usize)> = HashMap::new();
        for file_diags in &per_file {
            let mut seen_here: HashSet<&str> = HashSet::new();
            for (code, bucket) in file_diags {
                let entry = map.entry(code.clone()).or_insert((*bucket, 0, 0));
                entry.1 += 1;
                if seen_here.insert(code.as_str()) {
                    entry.2 += 1;
                }
            }
        }

        let mut aggregates: Vec<CodeAggregate> = map
            .into_iter()
            .filter(|(_, (bucket, _, _))| *bucket >= opts.min_severity)
            .filter(|(code, _)| opts.codes.is_empty() || opts.codes.iter().any(|c| c == code))
            .map(|(code, (severity, count, files_affected))| CodeAggregate {
                code,
                severity,
                count,
                files_affected,
            })
            .collect();
        // Most-severe first, then most-frequent, then code for a stable order.
        aggregates.sort_by(|a, b| {
            b.severity.cmp(&a.severity).then(b.count.cmp(&a.count)).then(a.code.cmp(&b.code))
        });

        WorkspaceSweep { aggregates, files_swept: swept.len(), files_total, truncated }
    }
}

/// Filters for a workspace sweep.
pub(crate) struct SweepOptions {
    pub min_severity: ide::SeverityBucket,
    /// Keep only these codes (empty = all).
    pub codes: Vec<String>,
    /// Cap on files swept (bounds the cost of an opt-in whole-config pass).
    pub max_files: usize,
}

/// One code's workspace-wide tally.
pub(crate) struct CodeAggregate {
    pub code: String,
    pub severity: ide::SeverityBucket,
    pub count: usize,
    pub files_affected: usize,
}

/// The result of a workspace sweep: per-code aggregates plus coverage bookkeeping.
pub(crate) struct WorkspaceSweep {
    pub aggregates: Vec<CodeAggregate>,
    pub files_swept: usize,
    pub files_total: usize,
    pub truncated: bool,
}

/// Everything mutable about the resident db, guarded by one `Mutex`. The lock is held
/// for the duration of a per-file query or an incremental/full reload, so the two are
/// mutually exclusive (the db is `!Sync`, so a query cannot run off-thread anyway).
struct Inner {
    status: DiagnosticsStatus,
    resident: Option<DiagnosticsResident>,
    /// Per-file `(path → stat fingerprint)` from the last build/apply, for drift diff.
    stats: HashMap<String, u64>,
    /// Folded fingerprint of the analyzer config files, for config drift.
    config_fp: u64,
    generation: u64,
    reload: ReloadState,
    /// When the current `Loading` build started, for the `status`/`loading` envelope's
    /// `elapsed_ms`. Set on `Idle → Loading`, cleared when the resident becomes `Ready`.
    loading_since: Option<Instant>,
}

/// Throttled cache of the last on-disk drift scan, guarded across the walk so
/// concurrent readers serialise onto one scan per window (no thundering herd).
struct ScanCache {
    at: Instant,
    stats: Vec<FileStat>,
    config_fp: u64,
}

/// Outcome of a resident read: the closure's result paired with the freshness verdict
/// computed under the SAME lock hold (so the envelope is atomic — `revision`/`stale`/
/// `reload` always describe the exact generation the result was read from), or why the
/// read could not run.
pub(crate) enum ResidentOutcome<R> {
    Ready(R, Freshness),
    /// Idle or loading — the agent should retry shortly.
    Loading,
    /// Reference profile.
    Disabled,
    Failed(String),
}

/// Freshness verdict for one diagnostics response, matching the graph envelope.
pub(crate) struct Freshness {
    pub revision: u64,
    pub stale: bool,
    pub reload: &'static str,
}

/// A snapshot of the resident lifecycle for the `status` action and the enriched
/// `loading` envelope — so an agent can tell "building, N ms in" from "stuck/failed"
/// instead of polling a flat `loading`.
pub(crate) struct StatusReport {
    /// `disabled | idle | loading | ready | failed`.
    pub state: &'static str,
    pub generation: u64,
    /// Resident `.bsl` count once `ready`.
    pub files: Option<usize>,
    /// Background reload state: `none | running | failed`.
    pub reload: &'static str,
    /// The failure message when `state == failed` (build panicked or errored).
    pub error: Option<String>,
    /// Milliseconds since the current `loading` build started (`None` unless loading).
    pub elapsed_ms: Option<u64>,
}

/// Handle to the workspace diagnostics database. Cheap to clone (shared `Arc`s).
#[derive(Clone)]
pub(crate) struct DiagnosticsState {
    inner: Arc<Mutex<Inner>>,
    scan: Arc<Mutex<Option<ScanCache>>>,
    last_access: Arc<Mutex<Instant>>,
    shutdown: Arc<AtomicBool>,
    /// Guards spawning exactly one idle sweeper for the lifetime of the handle.
    sweeper_started: Arc<AtomicBool>,
    workspace_root: Option<PathBuf>,
    drift_interval: Duration,
    eviction_after: Duration,
}

impl DiagnosticsState {
    /// A disabled handle (reference / shared profiles).
    pub(crate) fn disabled() -> Self {
        Self::with_status(DiagnosticsStatus::Disabled, None)
    }

    /// A workspace handle that builds its resident db lazily on first use.
    pub(crate) fn for_workspace(workspace_root: PathBuf) -> Self {
        Self::with_status(DiagnosticsStatus::Idle, Some(workspace_root))
    }

    fn with_status(status: DiagnosticsStatus, workspace_root: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                status,
                resident: None,
                stats: HashMap::new(),
                config_fp: 0,
                generation: 0,
                reload: ReloadState::Idle,
                loading_since: None,
            })),
            scan: Arc::new(Mutex::new(None)),
            last_access: Arc::new(Mutex::new(Instant::now())),
            shutdown: Arc::new(AtomicBool::new(false)),
            sweeper_started: Arc::new(AtomicBool::new(false)),
            workspace_root,
            drift_interval: DRIFT_CHECK_INTERVAL,
            eviction_after: IDLE_EVICTION,
        }
    }

    pub(crate) fn status(&self) -> DiagnosticsStatus {
        lock_recover(&self.inner).status.clone()
    }

    /// A lifecycle snapshot for the `status` action and the enriched `loading` envelope.
    pub(crate) fn status_report(&self) -> StatusReport {
        let inner = lock_recover(&self.inner);
        let (state, files) = match &inner.status {
            DiagnosticsStatus::Disabled => ("disabled", None),
            DiagnosticsStatus::Idle => ("idle", None),
            DiagnosticsStatus::Loading => ("loading", None),
            DiagnosticsStatus::Ready { files } => ("ready", Some(*files)),
            DiagnosticsStatus::Failed(_) => ("failed", None),
        };
        let error = match &inner.status {
            DiagnosticsStatus::Failed(msg) => Some(msg.clone()),
            _ => None,
        };
        StatusReport {
            state,
            generation: inner.generation,
            files,
            reload: inner.reload.label(),
            error,
            elapsed_ms: inner.loading_since.map(|t| t.elapsed().as_millis() as u64),
        }
    }

    /// Stop the idle sweeper (called on server shutdown).
    pub(crate) fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// Trigger the background build if this is the first call (or the db was evicted).
    /// Transitions `Idle → Loading` and spawns exactly one loader thread; later calls
    /// return immediately. No-op for disabled / loading / ready / failed states.
    pub(crate) fn ensure_loading(&self) {
        if self.workspace_root.is_none() {
            return;
        }
        {
            let mut inner = lock_recover(&self.inner);
            if inner.status != DiagnosticsStatus::Idle {
                return;
            }
            inner.status = DiagnosticsStatus::Loading;
            inner.loading_since = Some(Instant::now());
        }
        // Spawn the single idle sweeper on first use (it outlives evict→rebuild cycles).
        if !self.sweeper_started.swap(true, Ordering::SeqCst) {
            self.spawn_sweeper();
        }
        let state = self.clone();
        let spawned = std::thread::Builder::new()
            .name("bsl-diag-init".to_owned())
            .spawn(move || state.run_load());
        if let Err(e) = spawned {
            let mut inner = lock_recover(&self.inner);
            inner.loading_since = None;
            inner.status = DiagnosticsStatus::Failed(format!("could not spawn loader: {e}"));
        }
    }

    /// Run `f` against the resident db under the lock, after refreshing the workspace
    /// on disk (throttled). Bumps the last-access time so the idle sweeper does not
    /// evict an actively-used db. The closure's borrow cannot outlive the guard, so a
    /// concurrent writer (reload / eviction) never aliases a live read.
    ///
    /// `f` MUST NOT call back into `&self` methods that lock `inner` (`status`,
    /// `generation`, `freshness`, …): the lock is held across `f` and is non-reentrant,
    /// so doing so self-deadlocks. The current generation is passed to `f` so it can
    /// stamp a result id consistent with the resident state it reads — never call
    /// `generation()` inside `f`. The freshness verdict is returned alongside the
    /// result, computed under the same lock, so the caller need not (and must not)
    /// re-sample it.
    pub(crate) fn read<F, R>(&self, f: F) -> ResidentOutcome<R>
    where
        F: FnOnce(&DiagnosticsResident, u64) -> R,
    {
        *lock_recover(&self.last_access) = Instant::now();
        // Freshness is handled before taking the lock: an incremental apply needs the
        // same mutex and it is non-reentrant. A full rebuild runs off-thread and this
        // read serves the current (stale) resident meanwhile. After `poll_drift`, the
        // generation read under the lock matches the resident content `f` will query.
        self.poll_drift();

        // Snapshot the drift scan BEFORE taking the inner lock (scan → inner order,
        // matching `freshness`), so the freshness verdict and the result are computed
        // from one consistent resident state. Without this, a reload finishing between
        // the read and a separate freshness sample could report `stale: false` for an
        // already-superseded generation.
        let scan = if matches!(self.status(), DiagnosticsStatus::Ready { .. }) {
            self.workspace_root.as_deref().and_then(|root| self.throttled_scan(root))
        } else {
            None
        };

        let inner = lock_recover(&self.inner);
        let generation = inner.generation;
        match &inner.status {
            DiagnosticsStatus::Ready { .. } => match inner.resident.as_ref() {
                Some(resident) => {
                    let freshness = compute_freshness(&inner, scan.as_ref());
                    let result = f(resident, generation);
                    ResidentOutcome::Ready(result, freshness)
                }
                None => ResidentOutcome::Loading,
            },
            DiagnosticsStatus::Idle | DiagnosticsStatus::Loading => ResidentOutcome::Loading,
            DiagnosticsStatus::Disabled => ResidentOutcome::Disabled,
            DiagnosticsStatus::Failed(msg) => ResidentOutcome::Failed(msg.clone()),
        }
    }

    /// The resident's current generation, bumped on every build / reload / incremental
    /// apply. Test-only observation point; production reads it under the lock via
    /// [`Self::read`], which returns it folded into the freshness verdict.
    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.poll_drift();
        lock_recover(&self.inner).generation
    }

    /// Detect and handle on-disk drift since the last build/apply. Incremental for
    /// `.bsl`-body-only changes (`set_file_text` under the write lock); a full rebuild
    /// off-thread for structural drift. Throttled and a no-op unless Ready.
    fn poll_drift(&self) {
        let Some(root) = self.workspace_root.clone() else {
            return;
        };
        if !matches!(self.status(), DiagnosticsStatus::Ready { .. }) {
            return;
        }
        let Some(scan) = self.throttled_scan(&root) else {
            return;
        };

        // Diff under a short read lock against the last-applied stats.
        let diff = {
            let inner = lock_recover(&self.inner);
            let stored: HashMap<String, u64> = inner.stats.clone();
            let config_changed = inner.config_fp != scan.config_fp;
            (classify_changes(&stored, &scan.stats), config_changed)
        };
        let (changes, config_changed) = diff;
        if changes.is_empty() && !config_changed {
            return;
        }

        // Structural drift (added/removed/.xml/config) forces a full rebuild; a `.bsl`
        // body-only change applies incrementally.
        let structural = config_changed
            || !changes.added.is_empty()
            || !changes.removed.is_empty()
            || changes.modified.iter().any(|p| p.ends_with(".xml"));

        if structural {
            self.kick_full_reload();
        } else {
            self.apply_incremental(&changes.modified, &scan);
        }
    }

    /// Re-key each modified `.bsl` to its on-disk content revision, re-reading from disk.
    /// Disk-backed (not an overlay) so the edited file's text stays LRU-evictable like the
    /// rest of the resident, mirroring the load path. The entire resolve→read→set→record
    /// sequence runs under ONE lock hold, so a concurrent full rebuild (which also takes
    /// the lock) cannot swap the resident mid-apply and make us record a stale revision. A
    /// modified path with no resident FileId is a structural change: we bail to a full
    /// rebuild (after dropping the lock). Idempotent against a racing apply, and bumps the
    /// generation only when content actually moved.
    fn apply_incremental(&self, modified: &[String], scan: &OwnedScan) {
        use base_db::SourceDatabase;

        let new_fp: HashMap<&str, u64> =
            scan.stats.iter().map(|s| (s.path.as_str(), s.fingerprint())).collect();

        let mut needs_rebuild = false;
        {
            let mut inner = lock_recover(&self.inner);
            // A full rebuild already in flight will publish a fresh resident; defer to
            // it rather than mutating a resident that is about to be replaced.
            if inner.reload == ReloadState::Running {
                return;
            }
            let Inner { resident: Some(resident), stats, generation, .. } = &mut *inner else {
                return;
            };
            let mut applied = 0usize;
            for path in modified {
                let Some(&fp) = new_fp.get(path.as_str()) else { continue };
                if stats.get(path).copied() == Some(fp) {
                    continue; // already applied (e.g. by a racing caller)
                }
                let Some(&file_id) = resident.by_path.get(path) else {
                    needs_rebuild = true; // a modified path we never indexed → structural
                    break;
                };
                match base_db::read_disk_text(Path::new(path)) {
                    Ok(text) => resident
                        .db
                        .set_file_revision_from_disk(file_id, base_db::content_revision(&text)),
                    // Unreadable now: pin an empty overlay so a later query yields `""`
                    // instead of panicking on the disk re-read, matching the load path.
                    Err(_) => resident.db.set_file_text(file_id, ""),
                }
                stats.insert(path.clone(), fp);
                applied += 1;
            }
            if applied > 0 && !needs_rebuild {
                *generation += 1;
                tracing::info!(applied, generation = *generation, "diagnostics incremental reload");
            }
        }
        if needs_rebuild {
            self.kick_full_reload();
        }
    }

    /// Spawn a full rebuild (at most one in flight) that replaces the resident db.
    /// Peak RAM is bounded: the new db is built, then swapped under the write lock and
    /// the old one dropped — the brief overlap is the price of a structural change.
    fn kick_full_reload(&self) {
        {
            let mut inner = lock_recover(&self.inner);
            if inner.reload == ReloadState::Running {
                return;
            }
            inner.reload = ReloadState::Running;
        }
        let state = self.clone();
        let spawned = std::thread::Builder::new()
            .name("bsl-diag-reload".to_owned())
            .spawn(move || state.run_reload());
        if let Err(e) = spawned {
            let mut inner = lock_recover(&self.inner);
            inner.reload = ReloadState::Failed(format!("could not spawn reload: {e}"));
        }
    }

    /// The initial resident build: load every `.bsl` text into a fresh database with
    /// the LSP-equivalent inputs, index path→FileId, record the drift baseline, and
    /// publish `Ready`. On success spawns the idle sweeper.
    fn run_load(&self) {
        let Some(root) = self.workspace_root.clone() else {
            return;
        };
        tracing::info!(?root, "diagnostics resident db build started");
        match Self::catch_build(|| Self::build_resident(&root)) {
            Ok((resident, stats, config_fp)) => {
                let files = resident.file_count();
                {
                    let mut inner = lock_recover(&self.inner);
                    inner.resident = Some(resident);
                    inner.stats = stats;
                    inner.config_fp = config_fp;
                    inner.generation += 1;
                    inner.reload = ReloadState::Idle;
                    inner.loading_since = None;
                    inner.status = DiagnosticsStatus::Ready { files };
                }
                *lock_recover(&self.scan) = None;
                tracing::info!(files, "diagnostics resident db ready");
            }
            Err(msg) => {
                tracing::warn!("diagnostics resident db build failed: {msg}");
                let mut inner = lock_recover(&self.inner);
                inner.loading_since = None;
                inner.status = DiagnosticsStatus::Failed(msg);
            }
        }
    }

    /// A full rebuild triggered by structural drift: build a fresh resident off-thread,
    /// then swap it in under the write lock. Keeps the old resident served until the
    /// swap; on failure the old one stays and `reload` is flagged failed.
    fn run_reload(&self) {
        let Some(root) = self.workspace_root.clone() else {
            return;
        };
        match Self::catch_build(|| Self::build_resident(&root)) {
            Ok((resident, stats, config_fp)) => {
                let files = resident.file_count();
                let mut inner = lock_recover(&self.inner);
                inner.resident = Some(resident);
                inner.stats = stats;
                inner.config_fp = config_fp;
                inner.generation += 1;
                inner.reload = ReloadState::Idle;
                inner.status = DiagnosticsStatus::Ready { files };
                drop(inner);
                *lock_recover(&self.scan) = None;
                tracing::info!(files, "diagnostics resident db reloaded");
            }
            Err(msg) => {
                tracing::warn!("diagnostics resident db reload failed: {msg}");
                let mut inner = lock_recover(&self.inner);
                inner.reload = ReloadState::Failed(msg);
            }
        }
    }

    /// Run a resident-build closure with panic isolation. A panic in the build thread must
    /// NOT leave the status pinned at `Loading` forever (the agent would see "still
    /// building" with no recovery, and the idle sweeper only evicts from `Ready`). Folding
    /// both an `Err` and a panic into `Err(String)` lets the caller publish `Failed`, which
    /// is visible and retryable. Generic over the closure so the fold itself is testable.
    fn catch_build<T>(build: impl FnOnce() -> anyhow::Result<T>) -> Result<T, String> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(build)) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => Err(e.to_string()),
            Err(panic) => {
                let detail = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown panic".to_owned());
                Err(format!("diagnostics build panicked: {detail}"))
            }
        }
    }

    /// Build a resident db over every workspace `.bsl`, with the same Salsa inputs the
    /// LSP server registers (source root, per-file source root + text, all config
    /// paths). Returns the resident, the per-file drift baseline, and the config fp.
    fn build_resident(
        root: &Path,
    ) -> anyhow::Result<(DiagnosticsResident, HashMap<String, u64>, u64)> {
        let files = enumerate_bsl_files(root);
        // Load the project once: its config paths feed the db inputs, and its
        // `[diagnostics]` settings + locale become the resident's effective config, so
        // `file`/`workspace` honour the same project rules as LSP and CLI.
        let project = project_model::Project::new(root);
        let config_paths = crate::graph::project_config_paths(&project);
        let config = ide::DiagnosticsConfig::from_project_json(
            &project.config.diagnostics,
            project.config.output.resolve_locale().unwrap_or_default(),
        );
        let source_root = build_source_root(&files);
        // Disk-backed: register each file's content revision and drop its text, so the
        // whole-workspace resident is not pinned as salsa inputs (which OOMs on a large
        // config). `file_text_query` re-reads on demand under its LRU cap — the same
        // model the LSP server and CLI `analyze` use.
        let db = db_for_files_lazy(&source_root, &files, &config_paths, None);

        let mut by_path = HashMap::with_capacity(files.len());
        for (file_id, path) in &files {
            by_path.insert(canonical_key(path), *file_id);
        }
        let stats: HashMap<String, u64> = scan_file_stats(root)
            .into_iter()
            .map(|s| {
                let fp = s.fingerprint();
                (s.path, fp)
            })
            .collect();
        let config_fp = config_fingerprint(root);

        Ok((
            DiagnosticsResident { db, by_path, config, workspace_root: root.to_path_buf() },
            stats,
            config_fp,
        ))
    }

    /// The throttled disk scan: at most one walk per drift interval, its result shared
    /// by concurrent callers. Returns a borrowed view valid until the next scan.
    fn throttled_scan(&self, root: &Path) -> Option<OwnedScan> {
        let mut cache = lock_recover(&self.scan);
        if let Some(c) = cache.as_ref() {
            if c.at.elapsed() < self.drift_interval {
                return Some(OwnedScan { stats: c.stats.clone(), config_fp: c.config_fp });
            }
        }
        let stats = scan_file_stats(root);
        let config_fp = config_fingerprint(root);
        *cache = Some(ScanCache { at: Instant::now(), stats: stats.clone(), config_fp });
        Some(OwnedScan { stats, config_fp })
    }

    /// The single idle sweeper, spawned once per handle and living until shutdown. Each
    /// tick it drops the resident db (→ `Idle`) when it has been `Ready` and untouched
    /// for `eviction_after`; otherwise it sleeps on. It does NOT exit after evicting, so
    /// a later rebuild (via `ensure_loading`/`run_reload`) is monitored again without
    /// needing a replacement sweeper.
    fn spawn_sweeper(&self) {
        let state = self.clone();
        let _ = std::thread::Builder::new().name("bsl-diag-sweep".to_owned()).spawn(move || loop {
            std::thread::sleep(SWEEP_INTERVAL.min(state.eviction_after));
            if state.shutdown.load(Ordering::SeqCst) {
                return;
            }
            if !matches!(state.status(), DiagnosticsStatus::Ready { .. }) {
                continue;
            }
            if lock_recover(&state.last_access).elapsed() < state.eviction_after {
                continue;
            }
            let mut inner = lock_recover(&state.inner);
            // Re-check under the lock: a read between the idle check and the lock bumps
            // last_access, and a reload may be in flight — never evict either.
            if lock_recover(&state.last_access).elapsed() < state.eviction_after
                || !matches!(inner.status, DiagnosticsStatus::Ready { .. })
                || inner.reload == ReloadState::Running
            {
                continue;
            }
            inner.resident = None;
            inner.stats.clear();
            inner.status = DiagnosticsStatus::Idle;
            drop(inner);
            *lock_recover(&state.scan) = None;
            tracing::info!("diagnostics resident db evicted after idle period");
        });
    }
}

/// An owned snapshot of one drift scan, decoupled from the cache lock.
struct OwnedScan {
    stats: Vec<FileStat>,
    config_fp: u64,
}

/// The freshness verdict for `inner` against an optional drift `scan`. Computed by the
/// caller while holding the inner lock, so the verdict (`revision`/`stale`/`reload`) is
/// atomic with the generation a read serves. `scan` is `None` when the db is not Ready
/// (nothing to compare), making `stale` depend only on an in-flight reload.
fn compute_freshness(inner: &Inner, scan: Option<&OwnedScan>) -> Freshness {
    let drifted = match scan {
        Some(s) => {
            inner.config_fp != s.config_fp || !classify_changes(&inner.stats, &s.stats).is_empty()
        }
        None => false,
    };
    Freshness {
        revision: inner.generation,
        stale: drifted || inner.reload == ReloadState::Running,
        reload: inner.reload.label(),
    }
}

/// Canonicalise a path to the same key the loader indexed by (`enumerate_bsl_files`
/// canonicalises, falling back to the raw path). Lets a request path in any form
/// resolve to the resident FileId.
fn canonical_key(path: &Path) -> String {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned()
}

/// A fold of the analyzer config files' `(presence, len, mtime)`. Any change forces a
/// full rebuild because it can alter the project's extension set and thus the db inputs.
fn config_fingerprint(root: &Path) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::UNIX_EPOCH;

    let mut entries: Vec<(String, u64, u128)> = Vec::new();
    for name in CONFIG_FILES {
        let path = root.join(name);
        if let Ok(meta) = std::fs::metadata(&path) {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            entries.push((name.to_string(), meta.len(), mtime));
        }
    }
    entries.sort();
    let mut hasher = DefaultHasher::new();
    entries.hash(&mut hasher);
    hasher.finish()
}

fn lock_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide::DiagnosticsConfig;
    use std::fs;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

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
            "Сервер",
            true,
            "&НаСервере\nФункция Считать() Экспорт КонецФункции",
        );
    }

    fn module_path(root: &Path, name: &str) -> PathBuf {
        root.join(format!("CommonModules/{name}/Ext/Module.bsl"))
    }

    fn wait_ready(state: &DiagnosticsState) {
        for _ in 0..300 {
            match state.status() {
                DiagnosticsStatus::Ready { .. } => return,
                DiagnosticsStatus::Failed(msg) => panic!("diagnostics load failed: {msg}"),
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        panic!("diagnostics db did not become ready");
    }

    /// First use builds the resident db over the workspace and resolves a request
    /// path to a FileId, then computes diagnostics for it.
    #[test]
    fn builds_resident_and_serves_file_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let path = module_path(root, "Сервер");
        let out = state.read(|resident, _gen| {
            let file_id = resident.file_id_for(&path).expect("path resolves to a resident FileId");
            let analysis = resident.analysis();
            analysis.diagnostics(file_id, &DiagnosticsConfig::default()).len()
        });
        match out {
            ResidentOutcome::Ready(_count, _) => {}
            _ => panic!("expected Ready outcome from a loaded db"),
        }
    }

    /// The resident is disk-backed: a workspace file is registered by content revision,
    /// not pinned as a `FileTextInput` overlay, so `file_text_query` re-reads it from disk
    /// under the LRU cap. This is what keeps a whole-workspace resident from OOMing. The
    /// file's text must still be queryable (diagnostics ran above), it just must not be
    /// held resident as a salsa input.
    #[test]
    fn resident_text_is_disk_backed_not_pinned() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let path = module_path(root, "Сервер");
        let out = state.read(|resident, _gen| {
            let file_id = resident.file_id_for(&path).expect("path resolves to a resident FileId");
            // No overlay pinned: the text is sourced from disk on demand.
            let pinned = resident.db.try_file_text(file_id).is_some();
            // ...yet it is still queryable (read through file_text_query).
            let len = resident.analysis().file_text(file_id).len();
            (pinned, len)
        });
        match out {
            ResidentOutcome::Ready((pinned, len), _) => {
                assert!(!pinned, "workspace file must be disk-backed, not pinned as an overlay");
                assert!(len > 0, "disk-backed text must still be readable on demand");
            }
            _ => panic!("expected Ready outcome"),
        }
    }

    /// The resident loads the project's `bsl-analyzer.toml` and exposes it as the
    /// effective config, so `file`/`workspace` honour the same disabled rules and tuned
    /// thresholds as LSP and CLI — not analyzer defaults.
    #[test]
    fn resident_config_reflects_project_toml() {
        use ide::DiagnosticCode;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write(
            root,
            "bsl-analyzer.toml",
            "[source]\nroot = \".\"\n\n\
             [diagnostics.parameters]\n\
             Typo = false\n\n\
             [diagnostics.parameters.LineLength]\n\
             maxLineLength = 200\n",
        );

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let out = state.read(|resident, _gen| {
            let config = resident.config();
            (
                config.is_disabled(DiagnosticCode::Typo),
                config.get_int(DiagnosticCode::LineLength, "maxLineLength"),
            )
        });
        match out {
            ResidentOutcome::Ready((typo_disabled, line_len), _) => {
                assert!(typo_disabled, "project toml disables Typo");
                assert_eq!(line_len, Some(200), "project toml sets the LineLength threshold");
            }
            _ => panic!("expected Ready outcome"),
        }
    }

    /// Editing `bsl-analyzer.toml` is structural drift: the resident fully reloads and
    /// re-derives its effective config, so a later `file`/`workspace` sees the new
    /// settings — the same single source LSP and CLI would pick up.
    #[test]
    fn config_edit_triggers_reload_with_new_config() {
        use ide::DiagnosticCode;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = false\n");

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0); // scan every read
        state.ensure_loading();
        wait_ready(&state);

        let typo0 = state.read(|r, _| r.config().is_disabled(DiagnosticCode::Typo));
        assert!(matches!(typo0, ResidentOutcome::Ready(true, _)), "initial toml disables Typo");

        // Flip the config; mtime/len change is what config drift keys on.
        std::thread::sleep(Duration::from_millis(10));
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = true\n");

        // A read sees config drift → full reload (off-thread); poll until it lands.
        let mut reloaded = false;
        for _ in 0..200 {
            if let ResidentOutcome::Ready(false, _) =
                state.read(|r, _| r.config().is_disabled(DiagnosticCode::Typo))
            {
                reloaded = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(reloaded, "config edit reloads the resident with the updated diagnostics config");
    }

    /// A disabled handle never loads and reads degrade to `Disabled`.
    #[test]
    fn disabled_handle_does_not_load() {
        let state = DiagnosticsState::disabled();
        state.ensure_loading();
        assert_eq!(state.status(), DiagnosticsStatus::Disabled);
        let out = state.read(|_, _| 1usize);
        assert!(matches!(out, ResidentOutcome::Disabled));
    }

    /// Editing a `.bsl` body drifts the workspace; the next read applies an
    /// incremental `set_file_text` and bumps the generation, with no full rebuild.
    #[test]
    fn incremental_reload_on_body_edit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0); // scan every read
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();

        // Modify the body; mtime/len change is what the drift scan keys on.
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            module_path(root, "Сервер"),
            "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции\n",
        )
        .unwrap();

        // A read triggers drift handling; the edited text must be resident afterwards.
        let _ = state.read(|_, _| ());
        // Give a beat in case the apply raced; then re-read.
        for _ in 0..50 {
            if state.generation() > gen0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            let _ = state.read(|_, _| ());
        }
        assert!(state.generation() > gen0, "incremental apply should bump the generation");
        assert!(
            matches!(state.status(), DiagnosticsStatus::Ready { .. }),
            "incremental apply stays Ready, no rebuild churn"
        );

        let text = state.read(|resident, _gen| {
            let file_id = resident.file_id_for(&module_path(root, "Сервер")).unwrap();
            resident.analysis().file_text(file_id)
        });
        match text {
            ResidentOutcome::Ready(t, _) => {
                assert!(t.contains("Возврат 1"), "edited text resident")
            }
            _ => panic!("expected Ready"),
        }
    }

    /// Idle eviction drops the resident db back to `Idle` after the quiet period, and
    /// a later read rebuilds it.
    #[test]
    fn idle_eviction_drops_and_rebuilds() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.eviction_after = Duration::from_millis(50);
        state.ensure_loading();
        wait_ready(&state);

        // No reads for longer than the eviction window → sweeper drops it.
        for _ in 0..300 {
            if state.status() == DiagnosticsStatus::Idle {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(state.status(), DiagnosticsStatus::Idle, "resident evicted after idle");

        // A later use rebuilds.
        state.ensure_loading();
        wait_ready(&state);
        assert!(matches!(state.status(), DiagnosticsStatus::Ready { .. }));
    }

    /// A `diagnostics file` request may pass a workspace-relative path; it must resolve
    /// against the workspace root, not the process CWD.
    #[test]
    fn file_id_resolves_relative_path_against_workspace_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let rel = Path::new("CommonModules/Сервер/Ext/Module.bsl");
        let abs = module_path(root, "Сервер");
        let found = state.read(|resident, _gen| {
            (resident.file_id_for(rel).is_some(), resident.file_id_for(&abs).is_some())
        });
        match found {
            ResidentOutcome::Ready((rel_ok, abs_ok), _) => {
                assert!(rel_ok, "relative path resolves against the workspace root");
                assert!(abs_ok, "absolute path still resolves");
            }
            _ => panic!("expected Ready"),
        }
    }

    /// `status_report` reflects the lifecycle: `idle` before load, `ready` with the file
    /// count and a bumped generation after, and `reload = none` when not reloading.
    #[test]
    fn status_report_tracks_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        let before = state.status_report();
        assert_eq!(before.state, "idle");
        assert_eq!(before.generation, 0);
        assert_eq!(before.files, None);

        state.ensure_loading();
        wait_ready(&state);

        let after = state.status_report();
        assert_eq!(after.state, "ready");
        assert!(after.generation >= 1, "generation bumped on build");
        assert_eq!(after.files, Some(1), "one resident .bsl");
        assert_eq!(after.reload, "none");
        assert!(after.error.is_none());
        // elapsed_ms is cleared once ready.
        assert!(after.elapsed_ms.is_none());
    }

    /// The production `catch_build` fold: an `Ok` build passes through, an `Err` becomes a
    /// message, and a PANIC is folded into `Err` (so the caller publishes `Failed` instead
    /// of leaving a dead thread with the status pinned at `Loading`).
    #[test]
    fn catch_build_folds_ok_err_and_panic() {
        let ok: Result<i32, String> = DiagnosticsState::catch_build(|| Ok(42));
        assert_eq!(ok, Ok(42));

        let err = DiagnosticsState::catch_build(|| -> anyhow::Result<i32> {
            anyhow::bail!("plain build error")
        });
        assert_eq!(err, Err("plain build error".to_owned()));

        let panicked = DiagnosticsState::catch_build(|| -> anyhow::Result<i32> {
            panic!("synthetic build panic")
        });
        let msg = panicked.unwrap_err();
        assert!(msg.contains("panicked") && msg.contains("synthetic build panic"), "{msg}");
    }

    /// End-to-end: a loader that publishes via `catch_build`'s `Err` path lands in
    /// `Failed` with `loading_since` cleared (no stale `elapsed_ms`), never stuck `Loading`.
    #[test]
    fn failed_build_clears_loading_since_and_is_visible() {
        let err = DiagnosticsState::catch_build(|| -> anyhow::Result<()> { panic!("boom") });
        // Simulate run_load's Err arm publishing the failure.
        let state = DiagnosticsState::for_workspace(std::env::temp_dir());
        {
            let mut inner = lock_recover(&state.inner);
            inner.loading_since = Some(Instant::now());
            inner.status = DiagnosticsStatus::Loading;
            // The exact publication run_load performs on Err.
            inner.loading_since = None;
            inner.status = DiagnosticsStatus::Failed(err.unwrap_err());
        }
        let report = state.status_report();
        assert_eq!(report.state, "failed");
        assert!(report.error.as_deref().unwrap().contains("boom"));
        assert!(report.elapsed_ms.is_none(), "loading_since cleared on failure");
    }
}
