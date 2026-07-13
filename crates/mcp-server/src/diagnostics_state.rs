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
//! workspace on disk (throttled). A changed `.bsl` body is re-keyed with `set_file_text`,
//! a created/deleted `.bsl` is (un)registered into the live source root, and any `.xml`
//! add/remove/edit point-refreshes the metadata substrate — all in place under the
//! resident mutex, preserving every unrelated memo. Only an analyzer config-file change
//! or a removed directory subtree falls back to a full off-thread rebuild. An idle
//! sweeper drops the resident db
//! after a quiet period so a standalone `mcp serve` reclaims the memory after a burst.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ide::{Analysis, RootDatabaseImpl};
use vfs::{FileId, Vfs, VfsPath};

use crate::change_hub::{ChangeEntry, Health, SinkCursor, WorkspaceChangeHub};
use crate::graph::input::{
    build_source_root, db_for_files_lazy, enumerate_bsl_files, project_config_paths,
    GRAPH_SOURCE_ROOT,
};
use crate::graph::scan::{classify_changes, scan_file_stats, FileStat, WorkspaceDiff};

/// Minimum time between on-disk drift scans, mirroring the graph's throttle. A scan
/// stats every `.bsl`/`.xml` under the config roots, so this bounds its cost
/// regardless of how fast an agent fires `diagnostics file` calls.
const DRIFT_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// Drop the resident database after this long with no `diagnostics file` call, so a
/// standalone server reclaims the ~2.8 GB after a burst. The next call rebuilds.
const IDLE_EVICTION: Duration = Duration::from_secs(600);

/// The shortest interval a forced re-scan (a `metadata object` miss retry) may bypass the
/// drift throttle. Bounds how fast a loop of genuinely-absent lookups can stat-walk the
/// workspace, mirroring the retired `MetadataCache`'s force floor.
const FORCE_RESCAN_FLOOR: Duration = Duration::from_millis(250);

/// How often the idle sweeper wakes to check the last-access time.
const SWEEP_INTERVAL: Duration = Duration::from_secs(30);

/// How often the reconciler runs a full scan to catch any drift the change hub's
/// event stream failed to deliver (a lossy backend). This is the worst-case
/// staleness bound when the watcher silently misses an event; the healthy path is
/// sub-second. Overridable via `BSL_MCP_RECONCILE_SECS` for tests and tuning.
const RECONCILE_INTERVAL: Duration = Duration::from_secs(90);

/// Floor for the reconcile interval: a smaller value would busy-loop the sweeper thread
/// running full scans. `0` and unparseable inputs fall back to the default rather than
/// clamping, so a mistyped env var does not silently pin the scan rate at the floor.
const MIN_RECONCILE_INTERVAL: Duration = Duration::from_secs(5);

fn reconcile_interval() -> Duration {
    clamp_reconcile_interval(
        std::env::var("BSL_MCP_RECONCILE_SECS").ok().and_then(|s| s.parse::<u64>().ok()),
    )
}

/// Turn a parsed `BSL_MCP_RECONCILE_SECS` value into an interval: `0` or absent/garbage
/// (`None`) → the default; anything else is clamped up to [`MIN_RECONCILE_INTERVAL`].
fn clamp_reconcile_interval(secs: Option<u64>) -> Duration {
    match secs {
        Some(0) | None => RECONCILE_INTERVAL,
        Some(secs) => Duration::from_secs(secs).max(MIN_RECONCILE_INTERVAL),
    }
}

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

/// Adapts the resident's owned [`Vfs`] to the lock-neutral [`ide_host_core::VfsWrite`]
/// the shared metadata policy expects. The resident is only ever touched while the
/// caller holds the state mutex (the db is `!Sync`), so a single-threaded `RefCell`
/// gives the interning critical section its interior mutability without a second lock —
/// the same discipline the LSP's `parking_lot`-locked adapter has, minus the lock.
struct ResidentVfs(RefCell<Vfs>);

impl ide_host_core::VfsWrite for ResidentVfs {
    fn with_write<R>(&self, f: impl FnOnce(&mut Vfs) -> R) -> R {
        f(&mut self.0.borrow_mut())
    }
}

/// The outcome of one [`DiagnosticsState::try_snapshot_once`] pass.
enum SnapshotAttempt {
    /// The resident served text + shared parse.
    Fetched((Arc<str>, syntax::Parse<syntax::SyntaxNode>)),
    /// Definitively unserveable this call — no retry. Covers no resident / a non-resident path,
    /// and an UNEXPECTED read panic (already logged at error): a genuine invariant bug will not
    /// clear on a retry, so it degrades straight to the caller's disk read.
    Unavailable,
    /// A read unwound on an EXPECTED drift race (cancellation or `file_text` revision/disk-read
    /// panic). The caller retries once on a fresh snapshot.
    Unwound,
}

/// Classify a caught unwind from an unlocked resident snapshot read. Two payloads are EXPECTED on
/// the drift hot path and logged at debug: a `salsa::Cancelled` (a concurrent `set_file_text`
/// cancelled the cloned handle's in-flight query) and `file_text_query`'s own revision/disk-read
/// panic (the file's bytes changed between the recorded revision and this disk re-read). ANY OTHER
/// payload is a real invariant bug, not a drift race, so it is logged at error with its message —
/// never masked as a race — while the read still degrades to `Unavailable` rather than taking down
/// the query thread.
///
/// The revision panic also fires the process-global panic hook, printing a backtrace to stderr on
/// every genuine drift race. Swapping the hook (`set_hook`/`update_hook`) is process-global and
/// would race other threads' panics, so it is deliberately NOT done here: the stderr backtrace is
/// accepted as expected noise on a drift race.
fn classify_snapshot_unwind(
    path: &Path,
    payload: Box<dyn std::any::Any + Send>,
) -> SnapshotAttempt {
    if payload.is::<salsa::Cancelled>() {
        tracing::debug!(?path, "resident snapshot read cancelled (drift race); retrying once");
        return SnapshotAttempt::Unwound;
    }
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&'static str>().copied());
    match message {
        Some(msg) if msg.starts_with("file_text") => {
            tracing::debug!(
                ?path,
                %msg,
                "resident snapshot read unwound (drift race); retrying once"
            );
            SnapshotAttempt::Unwound
        }
        Some(msg) => {
            tracing::error!(
                ?path,
                %msg,
                "resident snapshot read panicked unexpectedly; treating as unavailable"
            );
            SnapshotAttempt::Unavailable
        }
        None => {
            tracing::error!(
                ?path,
                "resident snapshot read panicked with a non-string payload; treating as unavailable"
            );
            SnapshotAttempt::Unavailable
        }
    }
}

/// Adapts the resident [`DiagnosticsState`] to the search index's
/// [`bsl_search::ModuleSnapshotSource`] port, so the overlay's incremental reindex chunks the
/// shared resident parse instead of reading and re-parsing the file itself. Cheap to clone (the
/// state is `Arc`-backed). Its `text_and_parse` takes the resident lock only briefly and never
/// while the search engine lock is held (the caller prefetches off-lock).
#[derive(Clone)]
pub(crate) struct ResidentModuleSnapshotSource {
    diagnostics: DiagnosticsState,
}

impl ResidentModuleSnapshotSource {
    pub(crate) fn new(diagnostics: DiagnosticsState) -> Self {
        Self { diagnostics }
    }
}

impl bsl_search::ModuleSnapshotSource for ResidentModuleSnapshotSource {
    fn text_and_parse(&self, path: &str) -> bsl_search::SnapshotFetch {
        match self.diagnostics.snapshot_text_and_parse(Path::new(path)) {
            Some((text, parse)) => bsl_search::SnapshotFetch::Fetched(bsl_search::ModuleSnapshot {
                text,
                root: parse.syntax_node(),
            }),
            None => bsl_search::SnapshotFetch::Unavailable,
        }
    }

    fn catch_up(&self) {
        self.diagnostics.poll_pending_drift();
    }
}

/// The built resident database plus the path→FileId index needed to resolve a request
/// path to the Salsa input it set. Held behind the [`Mutex`]; reads borrow it, a
/// reload mutates `db` in place.
pub(crate) struct DiagnosticsResident {
    db: RootDatabaseImpl,
    /// The VFS pre-seeded with the resident's `.bsl` FileIds and grown by the metadata
    /// bootstrap with the metadata-XML ids. Kept alongside the db so a drift-driven
    /// substrate refresh can intern new composing files onto the same id space without
    /// rebuilding it.
    vfs: ResidentVfs,
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

    /// The resident Salsa database, for the `metadata` tool's root-scoped metadata
    /// reads (`resolve_*_across_roots` point-lookups and the Channel-2
    /// `configuration_for_root` header/enumeration). Borrowed under the state lock, so
    /// the borrow cannot outlive the read and a reload can never alias it.
    pub(crate) fn db(&self) -> &RootDatabaseImpl {
        &self.db
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
    /// The workspace change-hub view, when this profile has one. Lets an agent tell
    /// event-driven freshness from a scan fallback.
    pub watch: Option<WatchReport>,
}

/// The change hub's contribution to the diagnostics status: whether drift is
/// served event-driven or via the scan fallback, its health, and how many raw
/// filesystem events it has observed.
pub(crate) struct WatchReport {
    pub health: &'static str,
    pub events_seen: u64,
    /// `event-driven` while healthy, `scan-fallback` while degraded.
    pub mode: &'static str,
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
    /// The daemon's filesystem change hub, when this profile has one. Drift is served
    /// event-driven (drain-on-read) while the hub is healthy, falling back to the
    /// throttled scan otherwise. `None` for reference/shared profiles and tests, which
    /// keep the pure scan path.
    change_hub: Option<WorkspaceChangeHub>,
    /// This state's cursor into the hub. Subscribed when a resident is (re)built and
    /// dropped on eviction, so an idle diagnostics profile never pins the accumulator.
    hub_cursor: Arc<Mutex<Option<SinkCursor>>>,
    /// Set by [`Self::force_rescan`]: forces the next poll onto the scan path even when
    /// the hub is healthy (the `metadata object` miss escape hatch).
    force_scan: Arc<AtomicBool>,
    /// When the reconciler last ran a watchdog scan.
    last_reconcile: Arc<Mutex<Instant>>,
    reconcile_interval: Duration,
    /// Count of actual workspace walks (not cache hits), so a test can assert the
    /// event-driven hot path performs no scan.
    scan_count: Arc<AtomicUsize>,
    /// One-shot test seam fired between the reconciler's first drain and its scan.
    #[cfg(test)]
    reconcile_probe: ReconcileProbe,
}

/// A one-shot callback the reconciler fires between its first drain and its scan.
#[cfg(test)]
type ReconcileProbe = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;

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
            change_hub: None,
            hub_cursor: Arc::new(Mutex::new(None)),
            force_scan: Arc::new(AtomicBool::new(false)),
            last_reconcile: Arc::new(Mutex::new(Instant::now())),
            reconcile_interval: reconcile_interval(),
            scan_count: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            reconcile_probe: Arc::new(Mutex::new(None)),
        }
    }

    /// Attach the daemon's change hub so drift is served event-driven (drain-on-read)
    /// while the watcher is healthy, with the scan as the reconcile/fallback oracle.
    pub(crate) fn with_change_hub(mut self, hub: WorkspaceChangeHub) -> Self {
        self.change_hub = Some(hub);
        self
    }

    pub(crate) fn status(&self) -> DiagnosticsStatus {
        lock_recover(&self.inner).status.clone()
    }

    /// Whether a resident build or reload is in flight. The broker backend ORs this
    /// into its background-work signal so it does not idle-exit (and kill) a cold
    /// diagnostics build during a client-disconnect window — the build runs on its own
    /// thread and would otherwise be invisible to the idle timer, wasting its work.
    pub(crate) fn is_busy(&self) -> bool {
        let inner = lock_recover(&self.inner);
        inner.status == DiagnosticsStatus::Loading || inner.reload == ReloadState::Running
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
        let watch = self.change_hub.as_ref().map(|hub| {
            let health = hub.health();
            WatchReport {
                mode: if matches!(health, Health::Healthy) {
                    "event-driven"
                } else {
                    "scan-fallback"
                },
                health: health.label(),
                events_seen: hub.events_seen(),
            }
        });
        StatusReport {
            state,
            generation: inner.generation,
            files,
            reload: inner.reload.label(),
            error,
            elapsed_ms: inner.loading_since.map(|t| t.elapsed().as_millis() as u64),
            watch,
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
        // When the hub is healthy, `poll_drift` above has already reconciled the resident
        // to disk through the drain path, so freshness needs no scan — staleness reduces
        // to an in-flight reload. Only fall back to a freshness scan with no hub or a
        // degraded one (the scan path), keeping the healthy hot path free of a walk.
        let hub_healthy =
            matches!(&self.change_hub, Some(hub) if matches!(hub.health(), Health::Healthy));
        let scan = if matches!(self.status(), DiagnosticsStatus::Ready { .. }) && !hub_healthy {
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

    /// A lock-free snapshot of a resident file's text and shared parse, for the search index's
    /// incremental reindex.
    ///
    /// Resolves the path and clones the db handle under a BRIEF lock, then reads `file_text`
    /// and `parse` OUTSIDE the lock. Those reads run on a cloned Salsa handle that shares the
    /// resident storage, so a concurrent drift `set_file_text` on another thread can cancel
    /// them (`salsa::Cancelled`) or — for a disk-backed file whose bytes changed between the
    /// recorded revision and this read — panic inside `file_text_query`'s revision assert. Both
    /// unwind. Catching them here is sound: the cloned handle is discarded on unwind, a read
    /// never mutates the resident master db, and no half-built state escapes — which is why the
    /// `AssertUnwindSafe` wrapper is justified. A caught unwind retries ONCE on a fresh
    /// snapshot, then returns `None`. `None` also covers a resident that is absent / loading /
    /// evicted or a path that is not resident (the caller then disk-reads instead). Never
    /// forces a resident build and never touches drift state.
    pub(crate) fn snapshot_text_and_parse(
        &self,
        path: &Path,
    ) -> Option<(Arc<str>, syntax::Parse<syntax::SyntaxNode>)> {
        match self.try_snapshot_once(path) {
            SnapshotAttempt::Fetched(pair) => Some(pair),
            SnapshotAttempt::Unavailable => None,
            SnapshotAttempt::Unwound => match self.try_snapshot_once(path) {
                SnapshotAttempt::Fetched(pair) => Some(pair),
                SnapshotAttempt::Unavailable | SnapshotAttempt::Unwound => None,
            },
        }
    }

    /// One resolve+read attempt for [`Self::snapshot_text_and_parse`]. `Unavailable` is
    /// definitive (no resident / not a resident file) and must not retry; `Unwound` is a
    /// transient cancellation/revision race the caller retries once.
    fn try_snapshot_once(&self, path: &Path) -> SnapshotAttempt {
        let (analysis, file_id) = {
            let inner = lock_recover(&self.inner);
            if !matches!(inner.status, DiagnosticsStatus::Ready { .. }) {
                return SnapshotAttempt::Unavailable;
            }
            let Some(resident) = inner.resident.as_ref() else {
                return SnapshotAttempt::Unavailable;
            };
            let Some(file_id) = resident.file_id_for(path) else {
                return SnapshotAttempt::Unavailable;
            };
            (resident.analysis(), file_id)
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let text = analysis.file_text_arc(file_id);
            let parse = analysis.parse(file_id);
            (text, parse)
        })) {
            Ok(pair) => SnapshotAttempt::Fetched(pair),
            Err(payload) => classify_snapshot_unwind(path, payload),
        }
    }

    /// Reconcile pending workspace drift for the search prefetch: the cheap event-driven drain
    /// (or, with no/degraded hub, the throttled scan) that a diagnostics read runs before it
    /// serves, so a snapshot read taken right after a file edit sees fresh resident text instead
    /// of the stale pre-edit content. Takes only the resident lock (respecting the invariant that
    /// it is never nested under the search engine lock); a full rebuild in flight is skipped by
    /// the drain path, so this never blocks a query on a rebuild.
    pub(crate) fn poll_pending_drift(&self) {
        self.poll_drift();
    }

    /// The resident's current generation, bumped on every build / reload / incremental
    /// apply. Test-only observation point; production reads it under the lock via
    /// [`Self::read`], which returns it folded into the freshness verdict.
    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.poll_drift();
        lock_recover(&self.inner).generation
    }

    /// Number of actual workspace walks performed (cache misses), for asserting the
    /// event-driven hot path does no scanning.
    #[cfg(test)]
    fn scan_count(&self) -> usize {
        self.scan_count.load(Ordering::SeqCst)
    }

    /// Whether a hub cursor is currently held (dropped on eviction).
    #[cfg(test)]
    fn has_hub_cursor(&self) -> bool {
        lock_recover(&self.hub_cursor).is_some()
    }

    /// Drain this state's cursor and throw the entries away, advancing past them without
    /// applying — simulating a lossy sink so the reconciler has an undelivered change to
    /// catch.
    #[cfg(test)]
    fn drain_and_discard_cursor(&self) {
        let cursor = *lock_recover(&self.hub_cursor);
        if let (Some(hub), Some(cursor)) = (&self.change_hub, cursor) {
            let batch = hub.drain(cursor);
            *lock_recover(&self.hub_cursor) = Some(batch.cursor);
        }
    }

    /// Arm the one-shot reconciler probe (fired between its first drain and its scan).
    #[cfg(test)]
    fn set_reconcile_probe(&self, f: impl FnOnce() + Send + 'static) {
        *lock_recover(&self.reconcile_probe) = Some(Box::new(f));
    }

    /// Detect and handle on-disk drift since the last build/apply. Reconciled in place
    /// (under the resident mutex) for `.bsl` body edits and any `.xml` add/remove/edit —
    /// the latter through a metadata-substrate point-refresh. Only a non-`.xml` add/remove
    /// or an analyzer-config change forces a full off-thread rebuild. Throttled and a
    /// no-op unless Ready.
    fn poll_drift(&self) {
        let Some(root) = self.workspace_root.clone() else {
            return;
        };
        if !matches!(self.status(), DiagnosticsStatus::Ready { .. }) {
            return;
        }

        // The `metadata object` miss escape hatch forces a scan regardless of hub health.
        if self.force_scan.swap(false, Ordering::SeqCst) {
            self.poll_drift_via_scan(&root);
            return;
        }

        // Healthy hub → event-driven drain (O(change), no scan on the hot path).
        // No hub, or a degraded one, → today's throttled scan-on-read (parity).
        match &self.change_hub {
            Some(hub) if matches!(hub.health(), Health::Healthy) => {
                self.poll_drift_via_drain(hub, &root);
            }
            _ => {
                self.poll_drift_via_scan(&root);
            }
        }
    }

    /// The throttled full-scan drift path: at most one workspace walk per drift window,
    /// diffed against the last-applied stats and reconciled through the same rules the
    /// hub-driven path feeds. Returns whether any drift was found (so the reconciler can
    /// tell a lossy-backend miss from an in-sync workspace). This is the parity path used
    /// when there is no hub or the hub is degraded.
    fn poll_drift_via_scan(&self, root: &Path) -> bool {
        let Some(scan) = self.throttled_scan(root) else {
            return false;
        };

        // Diff under a short read lock against the last-applied stats.
        let (changes, config_changed) = {
            let inner = lock_recover(&self.inner);
            let stored: HashMap<String, u64> = inner.stats.clone();
            (classify_changes(&stored, &scan.stats), inner.config_fp != scan.config_fp)
        };
        self.apply_scan_drift(&changes, config_changed, &scan)
    }

    /// Apply already-classified full-scan drift: a full rebuild for structural/config
    /// drift, else an in-place metadata + body apply. Returns whether any drift was
    /// handled. Shared by the scan path and the reconciler (which classifies once, then
    /// re-checks late-delivered events before deciding to degrade).
    fn apply_scan_drift(
        &self,
        changes: &WorkspaceDiff,
        config_changed: bool,
        scan: &OwnedScan,
    ) -> bool {
        if changes.is_empty() && !config_changed {
            return false;
        }

        // Only an analyzer-config edit forces a full rebuild (it can change the
        // extension set and the effective diagnostics config). Everything else — any
        // `.xml` add/remove/edit, `.bsl` body edits, AND `.bsl` files appearing or
        // vanishing — is reconciled into the live resident in place. A removed subtree
        // needs no special case here: the scan diff enumerates every descendant.
        if config_changed {
            self.kick_full_reload();
            return true;
        }

        // XML drift spans all three buckets: an added/removed object is a structural
        // listing change, an edited one a content change — the substrate refresh handles
        // all of them by re-discovery + re-read of changed/new composing files.
        let xml_paths: Vec<PathBuf> = changes
            .added
            .iter()
            .chain(&changes.removed)
            .chain(&changes.modified)
            .filter(|p| p.ends_with(".xml"))
            .map(PathBuf::from)
            .collect();
        let added_bsl: Vec<String> =
            changes.added.iter().filter(|p| !p.ends_with(".xml")).cloned().collect();
        let modified_bsl: Vec<String> =
            changes.modified.iter().filter(|p| !p.ends_with(".xml")).cloned().collect();
        let removed_bsl: Vec<String> =
            changes.removed.iter().filter(|p| !p.ends_with(".xml")).cloned().collect();
        self.apply_metadata_and_body_drift(
            &xml_paths,
            &added_bsl,
            &modified_bsl,
            &removed_bsl,
            scan,
        );
        true
    }

    /// The event-driven drift path: drain this state's cursor and reconcile only the
    /// changed paths. Empty drain → nothing to do (crucially, NO scan on the hot path).
    /// A hub overflow (`rescan_required`) → fall back to the full scan, today's path.
    fn poll_drift_via_drain(&self, hub: &WorkspaceChangeHub, root: &Path) {
        // A full rebuild in flight will publish a fresh resident whose baseline scan already
        // reflects disk, and `apply_drained_resident` defers to it (bails on `Running`).
        // Draining now would advance the cursor past events the apply then drops — the
        // resident would miss the whole rebuild window. Leave them pending: the reload
        // re-subscribes a fresh cursor at its start, and the next poll after it finishes
        // drains that window onto the new resident. Mirrors the scan path, which bails
        // without rebasing its baseline so the drift is re-detected.
        if lock_recover(&self.inner).reload == ReloadState::Running {
            return;
        }
        let Some(cursor) = *lock_recover(&self.hub_cursor) else {
            // Ready but no cursor yet (a read racing the build's subscribe): reconcile via
            // scan this once; the next poll uses the cursor.
            self.poll_drift_via_scan(root);
            return;
        };
        let batch = hub.drain(cursor);
        *lock_recover(&self.hub_cursor) = Some(batch.cursor);
        if batch.rescan_required {
            self.poll_drift_via_scan(root);
            return;
        }
        if batch.entries.is_empty() {
            return;
        }
        self.apply_drained_entries(&batch.entries);
    }

    /// Reconcile the paths a drain reported. Events are hints, stats are truth: each path
    /// is re-stat'd and classified through the SAME fingerprint diff the scan uses, then
    /// fed the identical downstream — a full rebuild for structural/config drift, else an
    /// in-place metadata + body apply. Only the affected paths are stat'd, never the tree.
    fn apply_drained_entries(&self, entries: &[ChangeEntry]) {
        let baseline: HashMap<String, u64> = lock_recover(&self.inner).stats.clone();
        // The analyzer config files fingerprinted by `config_fingerprint` — canonicalised
        // to match the drained key spelling. Only a file at THIS exact location is config
        // drift; an identically-named file elsewhere in the tree is not (parity with the
        // scan path, which fingerprints only `root.join(name)`).
        let config_paths = self.config_file_paths();

        let class = crate::drift_classify::classify_drift(entries, &config_paths, Some(&baseline));

        // A config edit changes the effective diagnostics/extension setup and a removed
        // subtree hides descendants the drain could not enumerate — neither is
        // expressible in place. Everything else (xml, and `.bsl` added / modified /
        // removed) reconciles into the live resident.
        if class.config_changed || class.structural_rescan {
            self.kick_full_reload();
            return;
        }
        if class.xml_paths.is_empty()
            && class.bsl_modified.is_empty()
            && class.bsl_added.is_empty()
            && class.bsl_removed.is_empty()
        {
            return;
        }
        let xml_paths: Vec<PathBuf> =
            class.xml_paths.iter().map(|d| PathBuf::from(&d.key)).collect();
        let added_bsl: Vec<String> = class.bsl_added.iter().map(|d| d.key.clone()).collect();
        let modified_bsl: Vec<String> = class.bsl_modified.iter().map(|d| d.key.clone()).collect();
        let removed_bsl: Vec<String> = class.bsl_removed.iter().map(|d| d.key.clone()).collect();
        self.apply_drained_resident(
            &xml_paths,
            &added_bsl,
            &modified_bsl,
            &removed_bsl,
            &class.removed_keys,
            &class.new_fp,
        );
    }

    /// Apply already-classified event-driven drift to the resident and advance the drift
    /// baseline incrementally (only the drained paths change). Shares the exact resident
    /// mutation — substrate point-refresh + body re-key + `Running`-guard — with the scan
    /// path via [`apply_resident_changes`]; only the stats update differs (delta vs full
    /// rebase), because the drain has no whole-workspace scan to rebase onto.
    #[allow(
        clippy::too_many_arguments,
        reason = "one bucket per drift class, same as the classifier"
    )]
    fn apply_drained_resident(
        &self,
        xml_paths: &[PathBuf],
        added_bsl: &[String],
        modified_bsl: &[String],
        removed_bsl: &[String],
        removed_keys: &[String],
        new_fp: &HashMap<String, u64>,
    ) {
        let mut needs_rebuild = false;
        {
            let mut inner = lock_recover(&self.inner);
            if inner.reload == ReloadState::Running {
                return;
            }
            let Inner { resident: Some(resident), stats, generation, status, .. } = &mut *inner
            else {
                return;
            };
            let (rebuild, moved) = apply_resident_changes(
                resident,
                xml_paths,
                added_bsl,
                modified_bsl,
                removed_bsl,
                |p| new_fp.get(p).copied(),
                stats,
            );
            if rebuild {
                needs_rebuild = true;
            } else {
                for (key, fp) in new_fp {
                    stats.insert(key.clone(), *fp);
                }
                for key in removed_keys {
                    stats.remove(key);
                }
                if moved {
                    *generation += 1;
                    // An add/remove changed the served file universe; keep the
                    // observable `Ready { files }` count truthful.
                    *status = DiagnosticsStatus::Ready { files: resident.by_path.len() };
                    tracing::info!(
                        xml = xml_paths.len(),
                        added = added_bsl.len(),
                        bodies = modified_bsl.len(),
                        removed = removed_bsl.len(),
                        generation = *generation,
                        "diagnostics event-driven drift refresh",
                    );
                }
            }
        }
        if needs_rebuild {
            self.kick_full_reload();
        }
    }

    /// Reconcile metadata-only structural drift in place, without a whole-db rebuild:
    /// point-refresh the metadata substrate for the drifted `.xml` (re-discovering the
    /// affected roots and re-reading only changed/new composing files) and re-key the
    /// drifted `.bsl` bodies to their on-disk revision. Everything runs under ONE hold of
    /// the resident mutex — the same discipline as a body-only apply — so a concurrent
    /// full rebuild (which also takes the lock) can never swap the resident mid-apply. A
    /// modified `.bsl` with no resident FileId means the file universe moved, so we bail
    /// to a full rebuild. The drift baseline is rebased to `scan` once reconciled, and the
    /// generation is bumped only when a Salsa input actually moved.
    fn apply_metadata_and_body_drift(
        &self,
        xml_paths: &[PathBuf],
        added_bsl: &[String],
        modified_bsl: &[String],
        removed_bsl: &[String],
        scan: &OwnedScan,
    ) {
        let new_fp: HashMap<&str, u64> =
            scan.stats.iter().map(|s| (s.path.as_str(), s.fingerprint())).collect();

        let mut needs_rebuild = false;
        {
            let mut inner = lock_recover(&self.inner);
            // A full rebuild already in flight will publish a fresh resident; defer to it
            // rather than mutating a resident that is about to be replaced.
            if inner.reload == ReloadState::Running {
                return;
            }
            // Another caller may have reconciled this exact scan already (both passed the
            // throttle, then serialised here); bail so we neither re-walk the roots nor
            // double-bump the generation.
            if classify_changes(&inner.stats, &scan.stats).is_empty()
                && inner.config_fp == scan.config_fp
            {
                return;
            }
            let Inner { resident: Some(resident), stats, generation, status, .. } = &mut *inner
            else {
                return;
            };
            let (rebuild, moved) = apply_resident_changes(
                resident,
                xml_paths,
                added_bsl,
                modified_bsl,
                removed_bsl,
                |p| new_fp.get(p).copied(),
                stats,
            );
            if rebuild {
                needs_rebuild = true;
            } else {
                // Advance the drift baseline to the scan we reconciled against: every
                // applied body and every XML add/remove/edit is now reflected in the
                // resident, so its state equals `scan`. Rebasing even when nothing moved
                // (a pure mtime touch with unchanged content) stops us re-scanning it
                // every window.
                *stats = scan.stats.iter().map(|s| (s.path.clone(), s.fingerprint())).collect();
                if moved {
                    *generation += 1;
                    // An add/remove changed the served file universe; keep the
                    // observable `Ready { files }` count truthful.
                    *status = DiagnosticsStatus::Ready { files: resident.by_path.len() };
                    tracing::info!(
                        xml = xml_paths.len(),
                        added = added_bsl.len(),
                        bodies = modified_bsl.len(),
                        removed = removed_bsl.len(),
                        generation = *generation,
                        "diagnostics metadata drift refresh",
                    );
                }
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
        // Snapshot the hub cursor at build start: the build's baseline scan captures the
        // disk as of now, so only events landing after this point need replaying onto the
        // published resident.
        self.resubscribe_cursor();
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
        // Fresh cursor snapshot at rebuild start; events during the rebuild replay onto
        // the new resident, events before it are covered by the rebuild's baseline scan.
        self.resubscribe_cursor();
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
        // Canonicalise the config roots so a module back-link the metadata substrate
        // resolves (`root.join("CommonModules/X/Ext/Module.bsl")`) matches the
        // canonical `.bsl` path `enumerate_bsl_files` produced — otherwise the reverse
        // lookup would miss and silently drop the back-link on a symlinked workspace.
        let config_paths: Vec<(Option<String>, PathBuf)> = project_config_paths(&project)
            .into_iter()
            .map(|(label, path)| (label, path.canonicalize().unwrap_or(path)))
            .collect();
        let config = ide::DiagnosticsConfig::from_project_json(
            &project.config.diagnostics,
            project.config.output.resolve_locale().unwrap_or_default(),
        );
        let source_root = build_source_root(&files);
        // Disk-backed: register each file's content revision and drop its text, so the
        // whole-workspace resident is not pinned as salsa inputs (which OOMs on a large
        // config). `file_text_query` re-reads on demand under its LRU cap — the same
        // model the LSP server and CLI `analyze` use.
        let mut db = db_for_files_lazy(&source_root, &files, &config_paths, None);

        // Pre-seed the VFS with the SAME FileIds the source root uses for each `.bsl`,
        // in enumerate order, so the interner assigns id `i` to `files[i]`. The metadata
        // bootstrap resolves common-module / service module back-links through
        // `vfs.file_id(<Module.bsl>)`; without these ids present it drops them silently.
        // Bootstrap then allocates only the metadata-XML ids on top of this id space.
        let vfs = ResidentVfs(RefCell::new(Vfs::default()));
        {
            let mut guard = vfs.0.borrow_mut();
            for (file_id, path) in &files {
                let allocated = guard.alloc_file_id(VfsPath::new(path.clone()));
                // A hard check, not `debug_assert`: this is a one-time O(n) pass at
                // resident build (not a hot path), and a release-mode misalignment
                // would silently scatter every metadata back-link with no signal.
                assert_eq!(
                    allocated, *file_id,
                    "resident VFS must mirror enumerate_bsl_files ids for back-link resolution",
                );
            }
        }
        ide_host_core::bootstrap_metadata_substrate(&mut db, &vfs);

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
            DiagnosticsResident { db, vfs, by_path, config, workspace_root: root.to_path_buf() },
            stats,
            config_fp,
        ))
    }

    /// Drop the throttled scan cache so the next [`Self::read`] re-scans immediately, even
    /// inside the drift window. Used when a `metadata object` lookup misses, in case the
    /// object was added since the last scan. Storm-guarded: it only drops the cache when
    /// the last scan is older than [`FORCE_RESCAN_FLOOR`], so a loop of genuinely-absent
    /// lookups cannot stat-walk the workspace faster than that floor (the D2 discipline).
    pub(crate) fn force_rescan(&self) {
        let mut cache = lock_recover(&self.scan);
        let stale = cache.as_ref().is_none_or(|c| c.at.elapsed() >= FORCE_RESCAN_FLOOR);
        if stale {
            *cache = None;
            // Route the next poll through the scan even when the hub is healthy: the
            // event path would not re-observe an object the caller thinks it just added.
            self.force_scan.store(true, Ordering::SeqCst);
        }
    }

    /// (Re)subscribe this state's hub cursor, dropping any prior one. Called at the start
    /// of a (re)build so the fresh cursor is snapshotted at that moment: events BEFORE the
    /// build are captured by the build's own baseline scan, events DURING/AFTER it stay
    /// pending and apply to the freshly-published resident.
    fn resubscribe_cursor(&self) {
        let Some(hub) = &self.change_hub else {
            return;
        };
        let mut slot = lock_recover(&self.hub_cursor);
        if let Some(old) = slot.take() {
            hub.unsubscribe(old);
        }
        *slot = Some(hub.subscribe());
    }

    /// The canonical paths of the analyzer config files at the workspace root — the exact
    /// set [`config_fingerprint`] folds. A drained path counts as config drift only if it
    /// is one of these, matching the scan path (which fingerprints only `root.join(name)`)
    /// so an identically-named file elsewhere in the tree is not a spurious rebuild trigger.
    fn config_file_paths(&self) -> std::collections::HashSet<PathBuf> {
        let Some(root) = self.workspace_root.as_deref() else {
            return std::collections::HashSet::new();
        };
        CONFIG_FILES
            .iter()
            .map(|name| {
                let path = root.join(name);
                path.canonicalize().unwrap_or(path)
            })
            .collect()
    }

    /// Drop this state's hub cursor so an evicted (or never-built) resident does not pin
    /// the accumulator against reclamation.
    fn drop_cursor(&self) {
        let Some(hub) = &self.change_hub else {
            return;
        };
        let mut slot = lock_recover(&self.hub_cursor);
        if let Some(old) = slot.take() {
            hub.unsubscribe(old);
        }
    }

    /// Drain this state's cursor once, apply the delivered changes, and return the set of
    /// canonical paths the drain reported (empty when there is no cursor). A hub overflow
    /// (`rescan_required`) reconciles via a full scan and reports no paths. The path set
    /// lets the reconciler tell a late-but-delivered edit from a genuinely-missed one.
    fn drain_delivered_paths(
        &self,
        hub: &WorkspaceChangeHub,
        root: &Path,
    ) -> std::collections::HashSet<String> {
        let cursor = *lock_recover(&self.hub_cursor);
        let Some(cursor) = cursor else {
            return std::collections::HashSet::new();
        };
        let batch = hub.drain(cursor);
        *lock_recover(&self.hub_cursor) = Some(batch.cursor);
        if batch.rescan_required {
            self.poll_drift_via_scan(root);
            return std::collections::HashSet::new();
        }
        let delivered: std::collections::HashSet<String> =
            batch.entries.iter().map(|e| e.canonical.to_string_lossy().into_owned()).collect();
        if !batch.entries.is_empty() {
            self.apply_drained_entries(&batch.entries);
        }
        delivered
    }

    /// The reconciler/watchdog: a periodic full scan that catches any drift the hub's
    /// event stream failed to deliver (a lossy backend). Runs on the idle sweeper thread.
    /// Applies everything the hub delivered, then scans for the residue; a change that a
    /// second drain shows was merely late (delivered DURING the scan, not missed) does not
    /// degrade — only genuinely-undelivered file drift does.
    fn reconcile_tick(&self) {
        let Some(root) = self.workspace_root.clone() else {
            return;
        };
        let Some(hub) = self.change_hub.clone() else {
            return;
        };
        if !matches!(self.status(), DiagnosticsStatus::Ready { .. }) {
            return;
        }

        // 1. Apply everything the hub delivered so far. Draining also clears this cursor's
        //    reconcile flag, so a degraded hub recovers once the scan below is clean. The
        //    delivered paths are remembered so they are not later mistaken for a miss.
        let mut delivered = self.drain_delivered_paths(&hub, &root);

        // A delivered structural change kicked a full rebuild that will re-baseline the
        // whole workspace and re-subscribe a fresh cursor; nothing more to reconcile here.
        if lock_recover(&self.inner).reload == ReloadState::Running {
            return;
        }

        #[cfg(test)]
        self.fire_reconcile_probe();

        // 2. Fresh scan: classify the drift the events did not cover (do not apply yet).
        *lock_recover(&self.scan) = None;
        let Some(scan) = self.throttled_scan(&root) else {
            return;
        };
        let (changes, config_changed) = {
            let inner = lock_recover(&self.inner);
            (classify_changes(&inner.stats, &scan.stats), inner.config_fp != scan.config_fp)
        };
        if changes.is_empty() && !config_changed {
            return;
        }

        // 3. A legitimate edit may have landed AFTER step 1's drain but DURING the scan
        //    above. Drain once more: the paths it now delivers were merely late, not missed.
        delivered.extend(self.drain_delivered_paths(&hub, &root));

        // 4. Apply the residual drift (the just-delivered paths now match and are skipped).
        self.apply_scan_drift(&changes, config_changed, &scan);

        // 5. Degrade only if a FILE change (bsl/xml) was genuinely undelivered. Config drift
        //    is already fully rebuilt above and is expected to reach the reconciler in nested
        //    layouts (the config file sits above the watched root), so it is not a miss.
        let missed = changes
            .added
            .iter()
            .chain(&changes.removed)
            .chain(&changes.modified)
            .any(|p| !delivered.contains(p));
        if missed {
            tracing::warn!(
                "diagnostics reconciler found drift the change hub did not deliver; \
                 degrading to scan-on-read until the watcher recovers"
            );
            hub.degrade_external();
        }
    }

    /// A one-shot test seam fired between the reconciler's first drain and its scan, so a
    /// test can inject an edit that lands DURING the scan window (delivered, not missed).
    #[cfg(test)]
    fn fire_reconcile_probe(&self) {
        let probe = lock_recover(&self.reconcile_probe).take();
        if let Some(probe) = probe {
            probe();
        }
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
        self.scan_count.fetch_add(1, Ordering::SeqCst);
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
            std::thread::sleep(
                SWEEP_INTERVAL.min(state.eviction_after).min(state.reconcile_interval),
            );
            if state.shutdown.load(Ordering::SeqCst) {
                return;
            }

            // Watchdog: catch drift the hub's event stream may have missed. Runs on this
            // existing thread (no new one), independent of eviction, at its own cadence.
            if state.change_hub.is_some()
                && lock_recover(&state.last_reconcile).elapsed() >= state.reconcile_interval
                && matches!(state.status(), DiagnosticsStatus::Ready { .. })
            {
                *lock_recover(&state.last_reconcile) = Instant::now();
                state.reconcile_tick();
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
            // Release the hub cursor: an evicted resident must not pin the accumulator.
            state.drop_cursor();
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

/// Apply drifted XML metadata + modified BSL bodies to the resident under an
/// already-held lock, shared by the scan and event-driven drift paths so both mutate
/// the resident identically. Returns `(needs_rebuild, moved)`: a full rebuild is
/// needed when an XML path resolves outside every config root (a symlink the
/// point-refresh cannot express) or a modified `.bsl` has no resident FileId (the file
/// universe moved); `moved` is whether any Salsa input actually changed. `fp_of` yields
/// the on-disk fingerprint of a `modified_bsl` path so an already-current body is
/// skipped. The caller owns the drift-baseline update (full rebase vs incremental).
fn apply_resident_changes(
    resident: &mut DiagnosticsResident,
    xml_paths: &[PathBuf],
    added_bsl: &[String],
    modified_bsl: &[String],
    removed_bsl: &[String],
    fp_of: impl Fn(&str) -> Option<u64>,
    stats: &HashMap<String, u64>,
) -> (bool, bool) {
    use base_db::{SourceDatabase, SourceRoot};
    use ide_host_core::{set_file_text_source, FileTextSource, VfsWrite};

    // Pre-classification: an XML path resolving outside every registered config root is
    // drift the point-refresh cannot express — `refresh_metadata_substrate` gates its
    // re-discovery on `changed.starts_with(root)`, so it would silently no-op. Bail to a
    // full rebuild, which re-reads through the discovery joins, symlinks and all.
    let config_roots = resident.db.all_config_paths();
    let xml_outside_roots =
        xml_paths.iter().any(|p| !config_roots.iter().any(|(_, root)| p.starts_with(root)));
    if xml_outside_roots {
        return (true, false);
    }

    let mut moved = false;

    // (1) Reconcile the file universe FIRST — mirrors the LSP's `process_changes`
    // discipline (one FileSet clone, per-file inputs, one `set_source_root`), so the
    // substrate refresh below resolves module back-links through an up-to-date VFS and
    // root. Per-file ordering matters: the source-root + content-revision inputs are
    // registered BEFORE the file becomes visible through the FileSet
    // (`file_text_query` panics on a visible file with no revision).
    let mut file_set_modified = false;
    let mut file_set = {
        let db = &resident.db;
        db.source_root_input(GRAPH_SOURCE_ROOT).root(db).file_set().clone()
    };
    for path in added_bsl {
        // Vanished again before we got here (create+delete coalesced apart): the
        // removal pass — or the next drift window — settles it.
        if fp_of(path).is_none() && !Path::new(path).is_file() {
            continue;
        }
        let vfs_path = vfs::VfsPath::new(path.clone());
        let file_id = resident.vfs.with_write(|vfs| vfs.alloc_file_id(vfs_path.clone()));
        if let Some(&known) = resident.by_path.get(path.as_str()) {
            if known != file_id {
                // The path is already registered under a different id — an aliasing
                // (symlink/canonicalisation) case registration cannot express safely.
                return (true, moved);
            }
        }
        resident.db.set_file_source_root(file_id, GRAPH_SOURCE_ROOT);
        match base_db::read_disk_text(Path::new(path)) {
            Ok(text) => {
                set_file_text_source(&mut resident.db, file_id, FileTextSource::Disk(&text))
            }
            Err(_) => set_file_text_source(&mut resident.db, file_id, FileTextSource::Tombstone),
        }
        if file_set.path_for_file(&file_id).is_none() {
            file_set.insert(file_id, vfs_path);
            file_set_modified = true;
        }
        // The classifier's `key` IS the canonical by_path spelling (both come from the
        // scan-universe canonicalisation), so insert it verbatim — re-canonicalising
        // here could diverge on a path that vanished between classify and apply.
        resident.by_path.insert(path.clone(), file_id);
        moved = true;
    }
    for path in removed_bsl {
        // Never indexed → nothing to unregister (an untracked removal is not drift).
        let Some(&file_id) = resident.by_path.get(path.as_str()) else { continue };
        set_file_text_source(&mut resident.db, file_id, FileTextSource::Tombstone);
        if file_set.path_for_file(&file_id).is_some() {
            file_set.remove(file_id);
            file_set_modified = true;
        }
        resident.by_path.remove(path.as_str());
        moved = true;
    }
    if file_set_modified {
        resident.db.set_source_root(GRAPH_SOURCE_ROOT, SourceRoot::new_local(file_set));
    }

    // (2) Refresh the per-MDO substrate. Beside the drifted `.xml`, a created or
    // deleted common-module/service body changes its listing's `module_file`
    // reverse-index entry (the body is ordinary source, so it never flows through the
    // metadata-XML path) — include those bodies in the same re-discovery, exactly as
    // the LSP does. The config-revision bump stays `.xml`-only: a body add/remove does
    // not change the whole-config metadata content.
    let structural_listing_bodies: Vec<PathBuf> = added_bsl
        .iter()
        .chain(removed_bsl)
        .map(PathBuf::from)
        .filter(|p| project_model::is_substrate_listed_body_path(p))
        .filter(|p| config_roots.iter().any(|(_, root)| p.starts_with(root)))
        .collect();
    if !xml_paths.is_empty() || !structural_listing_bodies.is_empty() {
        let mut refresh: Vec<PathBuf> = xml_paths.to_vec();
        refresh.extend(structural_listing_bodies);
        ide_host_core::refresh_metadata_substrate(&mut resident.db, &resident.vfs, &refresh);
        if !xml_paths.is_empty() {
            resident.db.bump_config_for_paths(xml_paths.iter().map(|p| p.as_path()));
        }
        moved = true;
    }

    // `.bsl` bodies: disk-backed re-key. A body already at its on-disk fingerprint (a
    // racing caller beat us) is skipped.
    for path in modified_bsl {
        let Some(fp) = fp_of(path) else { continue };
        if stats.get(path).copied() == Some(fp) {
            continue;
        }
        let Some(&file_id) = resident.by_path.get(path) else {
            return (true, moved); // a modified `.bsl` we never indexed → structural
        };
        match base_db::read_disk_text(Path::new(path)) {
            Ok(text) => {
                set_file_text_source(&mut resident.db, file_id, FileTextSource::Disk(&text))
            }
            // Unreadable now: an empty overlay so a later query yields `""` instead of
            // panicking on the disk re-read, matching the load path.
            Err(_) => set_file_text_source(&mut resident.db, file_id, FileTextSource::Tombstone),
        }
        moved = true;
    }

    (false, moved)
}

fn lock_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change_hub::ChangeKind;
    use ide::DiagnosticsConfig;
    use std::fs;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// Write only a common module's descriptor XML (not its `Ext/Module.bsl`), so a test
    /// can flip a metadata property as pure `.xml` drift without touching the body.
    fn write_common_module_xml(root: &Path, name: &str, server: bool) {
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
    }

    fn write_common_module(root: &Path, name: &str, server: bool, body: &str) {
        write_common_module_xml(root, name, server);
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

    /// A resident write racing an unlocked snapshot read: the file changes on disk while the
    /// resident still records the OLD content revision (no drift poll re-keyed it), so the
    /// cloned `file_text` read trips `assert_revision` and unwinds. `snapshot_text_and_parse`
    /// must catch it, retry once on a fresh snapshot, and return `None` — never propagate the
    /// panic. Reverting the `catch_unwind` in `try_snapshot_once` makes this test panic.
    #[test]
    fn snapshot_returns_none_on_revision_drift() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        // Change the file on disk WITHOUT a drift poll (never call `read`/`generation`): the
        // recorded revision now disagrees with disk, so the unlocked read must unwind.
        let path = module_path(root, "Сервер");
        std::fs::write(&path, "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции")
            .unwrap();

        assert!(
            state.snapshot_text_and_parse(&path).is_none(),
            "a revision-drift read must degrade to None, not panic"
        );
    }

    /// The panic classifier separates an EXPECTED drift race (a `file_text` revision/disk panic,
    /// which the caller retries once) from a genuine invariant bug (any other payload, which
    /// degrades straight to unavailable and is logged at error, never masked as a race).
    #[test]
    fn snapshot_unwind_classification_separates_drift_from_bugs() {
        let path = Path::new("/ws/M.bsl");

        let drift: Box<dyn std::any::Any + Send> =
            Box::new("file_text revision mismatch for FileId(1): content changed".to_owned());
        assert!(
            matches!(classify_snapshot_unwind(path, drift), SnapshotAttempt::Unwound),
            "a file_text revision panic is an expected drift race → retry"
        );

        let bug: Box<dyn std::any::Any + Send> = Box::new("index out of bounds: len 0".to_owned());
        assert!(
            matches!(classify_snapshot_unwind(path, bug), SnapshotAttempt::Unavailable),
            "an unrelated string panic is a real bug → unavailable, not a retried race"
        );

        let opaque: Box<dyn std::any::Any + Send> = Box::new(7u8);
        assert!(
            matches!(classify_snapshot_unwind(path, opaque), SnapshotAttempt::Unavailable),
            "a non-string payload is treated as unavailable, never as a drift race"
        );
    }

    /// An unbuilt (`Idle`) or `Disabled` resident yields `None` immediately and never forces a
    /// build, so a single-file reindex degrades to the caller's own disk read.
    #[test]
    fn snapshot_none_when_resident_unbuilt_or_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        let path = module_path(root, "Сервер");

        let idle = DiagnosticsState::for_workspace(root.to_path_buf());
        assert!(idle.snapshot_text_and_parse(&path).is_none());
        assert!(
            matches!(idle.status(), DiagnosticsStatus::Idle),
            "a snapshot read must not kick a resident build"
        );

        let disabled = DiagnosticsState::disabled();
        assert!(disabled.snapshot_text_and_parse(&path).is_none());
    }

    /// The happy path: a Ready resident serves verbatim text and a parse whose chunk output
    /// matches a plain disk read+parse, so the overlay can safely chunk the shared tree.
    #[test]
    fn snapshot_serves_text_and_parse_matching_disk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let path = module_path(root, "Сервер");
        let (text, parse) =
            state.snapshot_text_and_parse(&path).expect("Ready resident must serve the file");

        let disk = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.as_ref(), disk, "resident text must be byte-verbatim");

        let via_shared = bsl_search::Chunker::chunk_parsed(&parse.syntax_node(), &text);
        let via_disk = bsl_search::Chunker::chunk(&disk);
        assert_eq!(via_shared.len(), via_disk.len());
        for (a, b) in via_shared.iter().zip(&via_disk) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.kind, b.kind);
            assert_eq!(a.is_export, b.is_export);
            assert_eq!(a.annotations, b.annotations);
            assert_eq!(a.line_start, b.line_start);
            assert_eq!(a.line_end, b.line_end);
            assert_eq!(a.text, b.text);
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

    /// `is_busy` is true while a build is `Loading` or a reload is `Running`, and false
    /// otherwise — the signal the broker ORs in so it keeps the backend alive through a
    /// cold diagnostics build but lets it idle-exit once the resident is settled.
    #[test]
    fn is_busy_reflects_loading_and_reload() {
        let state = DiagnosticsState::for_workspace(std::env::temp_dir());
        assert!(!state.is_busy(), "idle is not busy");

        lock_recover(&state.inner).status = DiagnosticsStatus::Loading;
        assert!(state.is_busy(), "loading is busy");

        {
            let mut inner = lock_recover(&state.inner);
            inner.status = DiagnosticsStatus::Ready { files: 0 };
            inner.reload = ReloadState::Running;
        }
        assert!(state.is_busy(), "a running reload is busy even when ready");

        lock_recover(&state.inner).reload = ReloadState::Idle;
        assert!(!state.is_busy(), "ready with no reload is not busy");
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

    /// A brand-new common module (descriptor + body) registers into the live resident:
    /// no rebuild (pre-existing FileIds stay stable — a re-enumeration would shift them,
    /// the new name sorts first), and the substrate lists the module (its module-level
    /// diagnostic fires, which requires the `module_file` back-link).
    #[test]
    fn incremental_add_of_new_common_module() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();
        let existing = module_path(root, "Сервер");
        let existing_id_before = match state.read(|r, _| r.file_id_for(&existing)) {
            ResidentOutcome::Ready(Some(id), _) => id,
            _ => panic!("existing module resolves before the add"),
        };

        // The name sorts before "Сервер", so a full re-enumeration would renumber it.
        std::thread::sleep(Duration::from_millis(10));
        write_common_module(root, "ААльфа", true, "Процедура Внутренняя()\nКонецПроцедуры");

        assert!(wait_for_apply(&state, gen0), "the add applies in place");
        assert!(
            matches!(state.status(), DiagnosticsStatus::Ready { .. }),
            "incremental add stays Ready"
        );

        let added = module_path(root, "ААльфа");
        let out = state.read(|resident, _| {
            let existing_id_after =
                resident.file_id_for(&existing).expect("existing module still resolves");
            let new_id = resident.file_id_for(&added).expect("new module resolves");
            let findings = resident.analysis().diagnostics(new_id, &DiagnosticsConfig::default());
            (existing_id_after, findings)
        });
        let ResidentOutcome::Ready((existing_id_after, findings), _) = out else {
            panic!("expected Ready")
        };
        assert_eq!(
            existing_id_after, existing_id_before,
            "pre-existing FileIds survive an incremental add (a rebuild would renumber)"
        );
        assert!(
            findings.iter().any(|d| d.code.as_str() == "CommonModuleMissingAPI"),
            "the module-level diagnostic fires — the substrate listed the new module: {:?}",
            findings.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
        );
    }

    /// A new body with no metadata descriptor still registers (readable, findings served);
    /// it just carries no substrate listing until its `.xml` lands.
    #[test]
    fn incremental_add_of_bare_body_without_descriptor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();

        std::thread::sleep(Duration::from_millis(10));
        let body = module_path(root, "БезОписания");
        fs::create_dir_all(body.parent().unwrap()).unwrap();
        fs::write(&body, "Процедура Тест()\n    Перем Неиспользуемая;\nКонецПроцедуры").unwrap();

        assert!(wait_for_apply(&state, gen0), "the bare add applies in place");
        let out = state.read(|resident, _| {
            let id = resident.file_id_for(&body).expect("bare body resolves");
            resident.analysis().diagnostics(id, &DiagnosticsConfig::default()).len()
        });
        assert!(matches!(out, ResidentOutcome::Ready(n, _) if n > 0), "findings served");
    }

    /// A deleted body unregisters in place: the path stops resolving, the survivor keeps
    /// its FileId, and the state stays Ready without a rebuild.
    #[test]
    fn incremental_remove_of_module_body() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write_common_module(root, "Удаляемый", true, "Процедура У()\nКонецПроцедуры");

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();
        let survivor = module_path(root, "Сервер");
        let survivor_id_before = match state.read(|r, _| r.file_id_for(&survivor)) {
            ResidentOutcome::Ready(Some(id), _) => id,
            _ => panic!("survivor resolves before the removal"),
        };

        std::thread::sleep(Duration::from_millis(10));
        let doomed = module_path(root, "Удаляемый");
        fs::remove_file(&doomed).unwrap();

        assert!(wait_for_apply(&state, gen0), "the removal applies in place");
        assert!(
            matches!(state.status(), DiagnosticsStatus::Ready { .. }),
            "incremental removal stays Ready"
        );
        let out = state
            .read(|resident, _| (resident.file_id_for(&doomed), resident.file_id_for(&survivor)));
        let ResidentOutcome::Ready((gone, kept), _) = out else { panic!("expected Ready") };
        assert!(gone.is_none(), "the removed body no longer resolves");
        assert_eq!(kept, Some(survivor_id_before), "the survivor keeps its FileId");
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

    /// The resident's metadata substrate resolves a common module's `Ext/Module.bsl`
    /// back to the SAME FileId the resident indexed for it. This guards the seeding
    /// invariant: the VFS is pre-seeded with the resident's `.bsl` ids before the
    /// bootstrap interns the metadata XML on top, so the reverse index carries the
    /// resident's own id. Were the ids unseeded, the bootstrap would drop the back-link
    /// and `common_module_for_file_id` would return `None`.
    #[test]
    fn resident_substrate_backlinks_common_module_to_its_own_file_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let module = module_path(root, "Сервер");
        // Derive the config-root key the same way `build_resident` does, so the listing
        // lookup uses the exact (canonicalised) root string the bootstrap keyed by.
        let project = project_model::Project::new(root);
        let config_root = project
            .source_path()
            .canonicalize()
            .unwrap_or_else(|_| project.source_path().to_path_buf());
        let root_key = config_root.to_string_lossy().into_owned();

        let out = state.read(|resident, _gen| {
            let file_id = resident.file_id_for(&module).expect("module .bsl resolves to a FileId");
            // The listing present ⇒ the substrate is bootstrapped for the root, so
            // `common_module_for_file_id` takes the substrate branch (no URI fallback).
            let listing_present = resident.db.metadata_listing(&root_key).is_some();
            let resolved = resident.db.common_module_for_file_id(file_id).is_some();
            (listing_present, resolved)
        });
        match out {
            ResidentOutcome::Ready((listing_present, resolved), _) => {
                assert!(
                    listing_present,
                    "the metadata substrate must be bootstrapped for the config root"
                );
                assert!(
                    resolved,
                    "the substrate must resolve the common module through the resident's own id"
                );
            }
            _ => panic!("expected Ready outcome from a loaded db"),
        }
    }

    /// A symlink *inside* the config tree (here a symlinked `CommonModules` directory)
    /// must not drop the common module's back-link. `enumerate_bsl_files` follows the
    /// symlink and canonicalises the `.bsl` to its real path (what the VFS is seeded
    /// with), while the metadata discovery composes `root.join("CommonModules/…")`,
    /// which keeps the symlink unresolved. Without the canonicalising fallback in the
    /// back-link lookup the two paths diverge, the reverse index misses, and — since the
    /// substrate is bootstrapped, so no URI fallback runs — `common_module_for_file_id`
    /// returns `None`, a regression versus the pre-substrate behaviour.
    #[cfg(unix)]
    #[test]
    fn resident_substrate_backlinks_common_module_through_symlinked_dir() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let root = base.join("ws");
        std::fs::create_dir_all(&root).unwrap();
        // Real common-module content lives OUTSIDE the workspace root; the workspace
        // reaches it only through a symlinked `CommonModules` directory.
        let real = base.join("real");
        write_common_module(&real, "Сервер", true, "&НаСервере\nФункция Ч() Экспорт КонецФункции");
        std::os::unix::fs::symlink(real.join("CommonModules"), root.join("CommonModules")).unwrap();

        let state = DiagnosticsState::for_workspace(root.clone());
        state.ensure_loading();
        wait_ready(&state);

        // The request path uses the symlinked location; `file_id_for` canonicalises it
        // to the same real id the resident indexed.
        let module = root.join("CommonModules/Сервер/Ext/Module.bsl");
        let out = state.read(|resident, _gen| {
            let file_id = resident.file_id_for(&module).expect("module .bsl resolves to a FileId");
            resident.db.common_module_for_file_id(file_id).is_some()
        });
        match out {
            ResidentOutcome::Ready(resolved, _) => {
                assert!(
                    resolved,
                    "back-link must resolve through a symlinked config subtree via the \
                     canonicalising fallback"
                );
            }
            _ => panic!("expected Ready outcome from a loaded db"),
        }
    }

    fn write_catalog(root: &Path, name: &str, code_length: u32) {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-0000000000{id:02}">
        <Properties><Name>{name}</Name><CodeLength>{code_length}</CodeLength></Properties>
    </Catalog>
</MetaDataObject>"#,
            id = name.len(),
        );
        write(root, &format!("Catalogs/{name}.xml"), &xml);
    }

    /// Whether the catalog `Товары` resolves through the substrate, anchored at the
    /// common module's `.bsl` (a file with a valid config root).
    fn catalog_resolves(state: &DiagnosticsState, module: &Path) -> bool {
        let out = state.read(|resident, _| {
            let fid = resident.file_id_for(module).expect("module resolves to a FileId");
            resident
                .db
                .resolve_metadata_object_for_file(fid, bsl_metadata::MdoType::Catalog, "Товары")
                .is_some()
        });
        match out {
            ResidentOutcome::Ready(v, _) => v,
            _ => panic!("expected Ready outcome"),
        }
    }

    /// The module file's diagnostics as sorted debug strings, for comparing an in-place
    /// point-refresh against a cold build over the same tree.
    fn module_diag_fingerprint(state: &DiagnosticsState, module: &Path) -> Vec<String> {
        let out = state.read(|resident, _| {
            let fid = resident.file_id_for(module).expect("module resolves to a FileId");
            let analysis = resident.analysis();
            let mut lines: Vec<String> = analysis
                .diagnostics(fid, resident.config())
                .iter()
                .map(|d| format!("{d:?}"))
                .collect();
            lines.sort();
            lines
        });
        match out {
            ResidentOutcome::Ready(v, _) => v,
            _ => panic!("expected Ready outcome"),
        }
    }

    /// Adding a metadata `.xml` point-refreshes the substrate in place: the new object
    /// resolves without a full db rebuild (no reload kicked), and the generation bumps.
    #[test]
    fn xml_add_point_refreshes_substrate_without_full_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();

        let module = module_path(root, "Сервер");
        assert!(!catalog_resolves(&state, &module), "catalog absent before the add");

        std::thread::sleep(Duration::from_millis(10));
        write_catalog(root, "Товары", 9);

        assert!(catalog_resolves(&state, &module), "added catalog resolves after point-refresh");
        assert_eq!(
            state.status_report().reload,
            "none",
            "no full rebuild was kicked for an XML add"
        );
        assert!(state.generation() > gen0, "the point-refresh bumps the generation");
        assert!(matches!(state.status(), DiagnosticsStatus::Ready { .. }), "stays Ready, no churn");
    }

    /// Removing a metadata `.xml` tombstones the object through a point-refresh — it no
    /// longer resolves — with no full rebuild.
    #[test]
    fn xml_remove_point_refreshes_substrate_without_full_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write_catalog(root, "Товары", 9);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();

        let module = module_path(root, "Сервер");
        assert!(catalog_resolves(&state, &module), "catalog present before the remove");

        std::thread::sleep(Duration::from_millis(10));
        std::fs::remove_file(root.join("Catalogs/Товары.xml")).unwrap();

        assert!(
            !catalog_resolves(&state, &module),
            "removed catalog tombstoned after point-refresh"
        );
        assert_eq!(
            state.status_report().reload,
            "none",
            "no full rebuild was kicked for an XML remove"
        );
        assert!(state.generation() > gen0, "the point-refresh bumps the generation");
    }

    /// Editing a metadata `.xml` re-reads only that object; the resident stays in place
    /// (no full rebuild) and its diagnostics equal a cold build over the mutated tree.
    #[test]
    fn xml_edit_point_refreshes_and_matches_fresh_build() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write_catalog(root, "Товары", 9);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();

        let module = module_path(root, "Сервер");
        std::thread::sleep(Duration::from_millis(10));
        write_catalog(root, "Товары", 12); // content edit → the object's revision moves

        // A read triggers the synchronous point-refresh.
        let _ = state.read(|_, _| ());
        assert_eq!(
            state.status_report().reload,
            "none",
            "no full rebuild was kicked for an XML edit"
        );
        assert!(state.generation() > gen0, "the edit is detected and applied in place");

        // A cold resident over the same on-disk tree must agree diagnostic-for-diagnostic.
        let fresh = DiagnosticsState::for_workspace(root.to_path_buf());
        fresh.ensure_loading();
        wait_ready(&fresh);
        assert_eq!(
            module_diag_fingerprint(&state, &module),
            module_diag_fingerprint(&fresh, &module),
            "point-refreshed diagnostics must equal a cold build over the mutated tree"
        );
    }

    /// An analyzer-config edit is NOT a metadata point-refresh: it still forces a full
    /// rebuild, re-deriving the effective config (something only a rebuild does). Proven
    /// by the resident picking up the flipped `Typo` setting after the edit.
    #[test]
    fn analyzer_config_edit_still_full_rebuilds() {
        use ide::DiagnosticCode;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = false\n");

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let disabled0 = state.read(|r, _| r.config().is_disabled(DiagnosticCode::Typo));
        assert!(matches!(disabled0, ResidentOutcome::Ready(true, _)), "initial toml disables Typo");

        std::thread::sleep(Duration::from_millis(10));
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = true\n");

        // The reload runs off-thread; poll until the re-derived config lands.
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
        assert!(reloaded, "a config edit full-rebuilds and re-derives the effective config");
    }

    /// A metadata `.xml` that `discover_*` does NOT enroll as a composing file
    /// (`Configuration.xml` here — a whole-config `load_from_directory` would re-read it)
    /// must still invalidate the coarse Channel-2 `load_configuration` memo via an
    /// unconditional config-revision bump, without a full rebuild. Observed directly
    /// through the config-root revision the `load_configuration` query keys on.
    #[test]
    fn non_enrolled_xml_edit_bumps_channel2_without_full_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write(root, "Configuration.xml", "<Configuration><Name>Конфа</Name></Configuration>");

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);

        let module = module_path(root, "Сервер");
        let rev0 = state.read(|r, _| r.db.config_root_revision_for_path(&module));
        let ResidentOutcome::Ready(rev0, _) = rev0 else { panic!("expected Ready") };

        std::thread::sleep(Duration::from_millis(10));
        write(root, "Configuration.xml", "<Configuration><Name>Другая</Name></Configuration>");

        // A read triggers the synchronous point-refresh; the non-enrolled edit still bumps
        // the config revision even though no per-MDO composing file moved.
        let rev1 = state.read(|r, _| r.db.config_root_revision_for_path(&module));
        let ResidentOutcome::Ready(rev1, _) = rev1 else { panic!("expected Ready") };

        assert_eq!(
            state.status_report().reload,
            "none",
            "a non-enrolled XML edit is a point-refresh, not a full rebuild"
        );
        assert!(rev1 > rev0, "the config-root revision the Channel-2 memo keys on must bump");
    }

    /// A common module's server flag, resolved through the substrate back-link.
    #[cfg(unix)]
    fn module_is_server(state: &DiagnosticsState, module: &Path) -> Option<bool> {
        let out = state.read(|r, _| {
            let fid = r.file_id_for(module)?;
            Some(r.db.common_module_for_file_id(fid)?.is_server())
        });
        match out {
            ResidentOutcome::Ready(v, _) => v,
            _ => panic!("expected Ready outcome"),
        }
    }

    /// A metadata subtree that is a symlink to a directory OUTSIDE the config root: the
    /// canonical (scan) path of its XML resolves outside the root, so the point-refresh
    /// cannot express the drift. Editing such an XML must still reach the resident — the
    /// pre-classification routes it to a full rebuild instead of silently forgetting it.
    #[cfg(unix)]
    #[test]
    fn symlinked_subtree_outside_root_xml_edit_is_not_lost() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let root = base.join("ws");
        std::fs::create_dir_all(&root).unwrap();
        // Real common-module content lives OUTSIDE the workspace root, reached only via a
        // symlinked `CommonModules` directory.
        let real = base.join("real");
        write_common_module(&real, "Сервер", true, "&НаСервере\nФункция Ч() Экспорт КонецФункции");
        std::os::unix::fs::symlink(real.join("CommonModules"), root.join("CommonModules")).unwrap();

        let mut state = DiagnosticsState::for_workspace(root.clone());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);

        let module = root.join("CommonModules/Сервер/Ext/Module.bsl");
        assert_eq!(module_is_server(&state, &module), Some(true), "starts server-side");

        // Flip Server→false via the descriptor XML only (no body edit). Its canonical path
        // is outside the root, so the point-refresh cannot own it.
        std::thread::sleep(Duration::from_millis(10));
        write_common_module_xml(&real, "Сервер", false);

        // The full rebuild is async; poll until the edit lands.
        let mut flipped = false;
        for _ in 0..300 {
            let _ = state.read(|_, _| ());
            if module_is_server(&state, &module) == Some(false) {
                flipped = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(flipped, "an XML edit under a symlinked-outside subtree must not be lost");
    }

    /// A metadata subtree that is a symlink to another directory INSIDE the config root:
    /// the canonical path stays under the root (so the point-refresh owns it), but the
    /// discovery join keeps the symlink unresolved. Editing the XML must re-read the file
    /// in place (via `enroll_refresh`'s canonicalise-on-miss) — a point-refresh, not a
    /// full rebuild.
    #[cfg(unix)]
    #[test]
    fn symlinked_subtree_inside_root_xml_edit_point_refreshes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Real modules live under `root/RealCM`; `root/CommonModules` is a symlink to it —
        // both inside the root, so canonical paths stay under the root.
        let realcm = root.join("RealCM");
        std::fs::create_dir_all(&realcm).unwrap();
        write_common_module(
            &realcm,
            "Сервер",
            true,
            "&НаСервере\nФункция Ч() Экспорт КонецФункции",
        );
        std::os::unix::fs::symlink(realcm.join("CommonModules"), root.join("CommonModules"))
            .unwrap();

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);

        let module = root.join("CommonModules/Сервер/Ext/Module.bsl");
        assert_eq!(module_is_server(&state, &module), Some(true), "starts server-side");

        std::thread::sleep(Duration::from_millis(10));
        write_common_module_xml(&realcm, "Сервер", false);

        // A read triggers the synchronous point-refresh; no full rebuild.
        let _ = state.read(|_, _| ());
        assert_eq!(
            state.status_report().reload,
            "none",
            "an in-root symlinked XML edit is a point-refresh, not a full rebuild"
        );
        assert_eq!(
            module_is_server(&state, &module),
            Some(false),
            "enroll_refresh must re-read the edited XML through the canonicalise-on-miss path"
        );
    }

    /// The `metadata object` tool path (`object_from_db` over the resident substrate) sees
    /// a newly-added catalog through the point-refresh — no full db rebuild, generation
    /// bumped — and never loads the whole configuration (the resolver is substrate-only).
    #[test]
    fn metadata_object_finds_added_catalog_without_full_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);
        let gen0 = state.generation();

        let found = |state: &DiagnosticsState| {
            let out = state.read(|r, _| {
                crate::tools::metadata::object_from_db(r.db(), "Catalog", "Товары").is_ok()
            });
            match out {
                ResidentOutcome::Ready(v, _) => v,
                _ => panic!("expected Ready"),
            }
        };

        assert!(!found(&state), "catalog absent before the add");

        std::thread::sleep(Duration::from_millis(10));
        write_catalog(root, "Товары", 9);

        assert!(found(&state), "the metadata object tool finds the added catalog");
        assert_eq!(state.status_report().reload, "none", "no full db rebuild for an object add");
        assert!(state.generation() > gen0, "the point-refresh bumped the generation");
    }

    /// The idle-eviction contract for metadata reads: after the resident is evicted, a read
    /// re-triggers the build and degrades to a "loading" outcome (or Ready once rebuilt) —
    /// NEVER a hard `Disabled`/`Failed` error. The tool maps `Loading` to a retry envelope.
    #[test]
    fn metadata_read_after_eviction_is_loading_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.eviction_after = Duration::from_millis(50);
        state.ensure_loading();
        wait_ready(&state);

        for _ in 0..300 {
            if state.status() == DiagnosticsStatus::Idle {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(state.status(), DiagnosticsStatus::Idle, "resident evicted after idle");

        // A metadata read after eviction: re-trigger the build and read. It is Loading (still
        // rebuilding) or Ready (rebuilt fast) — the tool turns Loading into a retry envelope,
        // never surfacing a hard "not loaded" error.
        state.ensure_loading();
        let out = state
            .read(|r, _| crate::tools::metadata::object_from_db(r.db(), "Catalog", "X").is_ok());
        assert!(
            matches!(out, ResidentOutcome::Loading | ResidentOutcome::Ready(_, _)),
            "an evicted metadata read must be loading or ready, never a hard error",
        );
    }

    /// The `metadata object` miss retry drops the throttle cache to force a re-scan, but
    /// only when the last scan is older than [`FORCE_RESCAN_FLOOR`] — so a loop of
    /// genuinely-absent lookups cannot stat-walk the workspace faster than that floor
    /// (the retired MetadataCache's storm guard). Exercised with a synthetic past
    /// `Instant`, so it is deterministic and needs no real sleep.
    #[test]
    fn force_rescan_is_storm_guarded_by_the_floor() {
        let state = DiagnosticsState::for_workspace(std::env::temp_dir());

        // A fresh scan (just now) must NOT be force-cleared — the storm guard.
        *lock_recover(&state.scan) =
            Some(ScanCache { at: Instant::now(), stats: Vec::new(), config_fp: 0 });
        state.force_rescan();
        assert!(
            lock_recover(&state.scan).is_some(),
            "a scan within the floor is kept, so repeated misses cannot hammer the FS",
        );

        // A scan older than the floor IS cleared, so the next read re-scans and can pick up
        // a just-added object.
        let stale_at = Instant::now()
            .checked_sub(FORCE_RESCAN_FLOOR + Duration::from_millis(50))
            .expect("a valid past instant");
        *lock_recover(&state.scan) =
            Some(ScanCache { at: stale_at, stats: Vec::new(), config_fp: 0 });
        state.force_rescan();
        assert!(
            lock_recover(&state.scan).is_none(),
            "a scan older than the floor is force-cleared so the retry re-scans",
        );
    }

    // --- Event-driven drift (W2): the change hub feeds a drain-on-read path. ---

    /// A workspace state wired to a real change hub over `root`, with the scan throttle
    /// disabled so the fallback path is exercised without waiting.
    fn state_with_hub(root: &Path) -> (DiagnosticsState, WorkspaceChangeHub) {
        let hub = WorkspaceChangeHub::start(vec![root.to_path_buf()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)), "the hub must arm");
        let mut state =
            DiagnosticsState::for_workspace(root.to_path_buf()).with_change_hub(hub.clone());
        state.drift_interval = Duration::from_millis(0);
        (state, hub)
    }

    /// Read the generation without triggering drift handling (a plain `generation()` would
    /// `poll_drift` and apply the change we are trying to observe out-of-band).
    fn raw_generation(state: &DiagnosticsState) -> u64 {
        lock_recover(&state.inner).generation
    }

    /// Poll `read` until the generation advances past `gen0`, returning whether it did.
    fn wait_for_apply(state: &DiagnosticsState, gen0: u64) -> bool {
        for _ in 0..300 {
            let _ = state.read(|_, _| ());
            if raw_generation(state) > gen0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// Poll the hub through `cursor` until an entry whose path contains `needle` is drained.
    fn wait_for_delivery(hub: &WorkspaceChangeHub, cursor: &mut SinkCursor, needle: &str) -> bool {
        for _ in 0..300 {
            let batch = hub.drain(*cursor);
            *cursor = batch.cursor;
            if batch.entries.iter().any(|e| e.raw.to_string_lossy().contains(needle)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    /// A `.bsl` body edit reaches the resident through the hub drain, and the healthy hot
    /// path performs NO workspace scan — the whole point of the event-driven path.
    #[test]
    fn event_driven_body_edit_lands_via_drain_without_scan() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);
        assert_eq!(state.scan_count(), 0, "the cold build does not go through the throttled scan");

        let gen0 = raw_generation(&state);
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            module_path(root, "Сервер"),
            "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции\n",
        )
        .unwrap();

        assert!(wait_for_apply(&state, gen0), "the body edit must be applied via drain");
        assert_eq!(state.scan_count(), 0, "the event-driven hot path performs no scan");

        let text = state.read(|resident, _| {
            let fid = resident.file_id_for(&module_path(root, "Сервер")).unwrap();
            resident.analysis().file_text(fid)
        });
        match text {
            ResidentOutcome::Ready(t, _) => {
                assert!(t.contains("Возврат 1"), "edited text resident")
            }
            _ => panic!("expected Ready"),
        }
    }

    /// A metadata `.xml` edit is delivered through the drain and point-refreshes the
    /// substrate in place (no full rebuild), again with no scan on the hot path.
    #[test]
    fn event_driven_xml_edit_lands_via_drain() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let gen0 = raw_generation(&state);
        std::thread::sleep(Duration::from_millis(10));
        // Flip the common module's server flag: a pure `.xml` edit (no body change).
        write_common_module_xml(root, "Сервер", false);

        assert!(wait_for_apply(&state, gen0), "the xml edit must be applied via drain");
        assert_eq!(
            state.status_report().reload,
            "none",
            "an xml edit is a point-refresh, not a full rebuild"
        );
        assert_eq!(state.scan_count(), 0, "the event-driven hot path performs no scan");
    }

    /// A degraded hub falls back to exactly today's throttled scan path: the edit is still
    /// applied, but through a scan (parity with the pre-hub behaviour).
    #[test]
    fn degraded_hub_reconciles_via_scan_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);
        // Force the scan fallback.
        hub.degrade_external();
        assert!(matches!(hub.health(), Health::Degraded(_)));

        let gen0 = raw_generation(&state);
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            module_path(root, "Сервер"),
            "&НаСервере\nФункция Считать() Экспорт Возврат 2; КонецФункции\n",
        )
        .unwrap();

        assert!(wait_for_apply(&state, gen0), "a degraded hub still applies the edit via scan");
        assert!(state.scan_count() > 0, "the degraded path uses the scan, matching today");
    }

    /// The reconciler/watchdog: a change the event stream failed to deliver (simulated by
    /// draining the cursor without applying) is caught by the periodic scan, which applies
    /// the drift AND degrades the hub so reads revert to scanning until it recovers.
    #[test]
    fn reconciler_catches_undelivered_drift_and_degrades() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let mut observer = hub.subscribe();
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            module_path(root, "Сервер"),
            "&НаСервере\nФункция Считать() Экспорт Возврат 3; КонецФункции\n",
        )
        .unwrap();
        // Confirm the hub delivered the change (so the diagnostics cursor has it too)...
        assert!(wait_for_delivery(&hub, &mut observer, "Module.bsl"), "hub delivered the edit");
        // ...then simulate a lossy sink dropping it: consume the cursor without applying.
        state.drain_and_discard_cursor();

        let gen0 = raw_generation(&state);
        assert_eq!(hub.health(), Health::Healthy, "still healthy before the reconcile");
        state.reconcile_tick();

        assert!(raw_generation(&state) > gen0, "the reconciler applied the missed drift");
        assert_eq!(
            hub.health().label(),
            "degraded:reconcile-miss",
            "a delivered-but-undrained miss degrades the hub to the scan fallback",
        );
    }

    /// An analyzer-config edit delivered through the drain is structural: it forces a full
    /// rebuild that re-derives the effective config, exactly like the scan path.
    #[test]
    fn event_driven_config_edit_full_rebuilds() {
        use ide::DiagnosticCode;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = false\n");

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);
        let disabled0 = state.read(|r, _| r.config().is_disabled(DiagnosticCode::Typo));
        assert!(matches!(disabled0, ResidentOutcome::Ready(true, _)), "initial toml disables Typo");

        std::thread::sleep(Duration::from_millis(10));
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = true\n");

        let mut reloaded = false;
        for _ in 0..300 {
            if let ResidentOutcome::Ready(false, _) =
                state.read(|r, _| r.config().is_disabled(DiagnosticCode::Typo))
            {
                reloaded = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(reloaded, "a config edit via drain full-rebuilds and re-derives the config");
    }

    /// After a full rebuild the cursor is re-subscribed, so a change landing AFTER the
    /// rebuild is applied to the fresh resident (the drain path survives a rebuild).
    #[test]
    fn events_after_rebuild_apply_to_the_new_resident() {
        use ide::DiagnosticCode;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = false\n");

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        // Force a full rebuild via a config edit and wait for the fresh resident.
        std::thread::sleep(Duration::from_millis(10));
        write(root, "bsl-analyzer.toml", "[diagnostics.parameters]\nTypo = true\n");
        let mut reloaded = false;
        for _ in 0..300 {
            if let ResidentOutcome::Ready(false, _) =
                state.read(|r, _| r.config().is_disabled(DiagnosticCode::Typo))
            {
                reloaded = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(reloaded, "the config rebuild completed");

        // A body edit AFTER the rebuild must reach the freshly-built resident via drain.
        let gen0 = raw_generation(&state);
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            module_path(root, "Сервер"),
            "&НаСервере\nФункция Считать() Экспорт Возврат 42; КонецФункции\n",
        )
        .unwrap();
        assert!(wait_for_apply(&state, gen0), "post-rebuild edits apply to the new resident");
        let text = state.read(|r, _| {
            let fid = r.file_id_for(&module_path(root, "Сервер")).unwrap();
            r.analysis().file_text(fid)
        });
        match text {
            ResidentOutcome::Ready(t, _) => {
                assert!(t.contains("Возврат 42"), "new resident edited")
            }
            _ => panic!("expected Ready"),
        }
    }

    /// Idle eviction releases the hub cursor so an evicted resident does not pin the
    /// accumulator against reclamation.
    #[test]
    fn eviction_releases_hub_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (mut state, _hub) = state_with_hub(root);
        state.eviction_after = Duration::from_millis(50);
        state.ensure_loading();
        wait_ready(&state);
        assert!(state.has_hub_cursor(), "a built resident holds a cursor");

        for _ in 0..300 {
            if state.status() == DiagnosticsStatus::Idle {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(state.status(), DiagnosticsStatus::Idle, "resident evicted after idle");
        assert!(!state.has_hub_cursor(), "eviction drops the cursor");
    }

    /// `status_report` surfaces the hub view so an agent can tell an event-driven serve
    /// from a scan fallback.
    #[test]
    fn status_report_exposes_watch_mode() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let watch = state.status_report().watch.expect("a hub-backed profile reports watch");
        assert_eq!(watch.mode, "event-driven");
        assert_eq!(watch.health, "healthy");

        hub.degrade_external();
        let watch = state.status_report().watch.expect("watch report present");
        assert_eq!(watch.mode, "scan-fallback", "a degraded hub reports the scan fallback");
    }

    /// An edit that lands WHILE a full rebuild is in flight must not be dropped: the drain
    /// leaves it pending (rather than draining-then-bailing on the reload) and applies it to
    /// the fresh resident once the rebuild finishes — without waiting for the reconciler.
    /// With the old drain-before-reload-check order this test fails (the edit is lost).
    #[test]
    fn edit_during_rebuild_applies_after_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        // Simulate a full rebuild in flight.
        lock_recover(&state.inner).reload = ReloadState::Running;

        let mut observer = hub.subscribe();
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            module_path(root, "Сервер"),
            "&НаСервере\nФункция Считать() Экспорт Возврат 11; КонецФункции\n",
        )
        .unwrap();
        assert!(wait_for_delivery(&hub, &mut observer, "Module.bsl"), "hub delivered the edit");

        // A read during the rebuild must NOT drain/apply (else the edit is lost).
        let gen0 = raw_generation(&state);
        let _ = state.read(|_, _| ());
        assert_eq!(raw_generation(&state), gen0, "no apply while a rebuild is in flight");

        // The rebuild finishes.
        lock_recover(&state.inner).reload = ReloadState::Idle;

        // The still-pending edit now applies to the (current) resident on the next read.
        assert!(wait_for_apply(&state, gen0), "the pending edit applies once the rebuild ends");
        let text = state.read(|r, _| {
            let fid = r.file_id_for(&module_path(root, "Сервер")).unwrap();
            r.analysis().file_text(fid)
        });
        match text {
            ResidentOutcome::Ready(t, _) => assert!(t.contains("Возврат 11"), "edit applied"),
            _ => panic!("expected Ready"),
        }
    }

    /// A legitimate edit that lands DURING the reconciler's scan (delivered to the cursor,
    /// just after its first drain) must NOT be counted as a lossy-backend miss: the second
    /// drain covers it, so the hub stays Healthy. With the old single-drain reconciler this
    /// fails (the scan sees drift and degrades).
    #[test]
    fn reconciler_does_not_degrade_a_late_delivered_edit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        // The probe fires between the reconciler's first drain and its scan: it writes the
        // edit and waits for the hub to deliver it into the accumulator (so the diagnostics
        // cursor holds it for the reconciler's second drain).
        let probe_root = root.to_path_buf();
        let probe_hub = hub.clone();
        state.set_reconcile_probe(move || {
            fs::write(
                probe_root.join("CommonModules/Сервер/Ext/Module.bsl"),
                "&НаСервере\nФункция Считать() Экспорт Возврат 13; КонецФункции\n",
            )
            .unwrap();
            let mut obs = probe_hub.subscribe();
            for _ in 0..300 {
                let batch = probe_hub.drain(obs);
                obs = batch.cursor;
                if batch.entries.iter().any(|e| e.raw.to_string_lossy().contains("Module.bsl")) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });

        let gen0 = raw_generation(&state);
        state.reconcile_tick();

        assert!(raw_generation(&state) > gen0, "the late edit is still applied");
        assert_eq!(
            hub.health(),
            Health::Healthy,
            "an edit delivered during the scan is not a miss and must not degrade",
        );
    }

    /// A `bsl-analyzer.toml` in a SUBDIRECTORY is not the analyzer config (which lives at the
    /// workspace root): the drain must ignore it, matching the scan path's `config_fingerprint`
    /// which only fingerprints `root.join(name)`. A subtree toml edit is not a rebuild trigger.
    #[test]
    fn subdir_config_file_is_not_config_drift() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        // A toml deep in the tree, NOT the root analyzer config.
        write(root, "CommonModules/Сервер/bsl-analyzer.toml", "[diagnostics]\n");

        let (state, _hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let subdir_toml = root.join("CommonModules/Сервер/bsl-analyzer.toml");
        let canonical = subdir_toml.canonicalize().unwrap_or(subdir_toml);
        let entry = ChangeEntry {
            canonical: canonical.clone(),
            raw: canonical,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };

        let gen0 = raw_generation(&state);
        // Feeding the subdir toml as drift must NOT kick a full rebuild.
        state.apply_drained_entries(&[entry]);
        assert_eq!(
            state.status_report().reload,
            "none",
            "a toml outside the workspace root is not analyzer-config drift",
        );
        assert_eq!(raw_generation(&state), gen0, "no structural rebuild for a subtree toml");
    }

    /// A `.bsl` edit in an EXTENSION root (disjoint from the config source root) is delivered
    /// through the drain, because the hub watches every scan root — not just the source one.
    /// Without extension coverage this drift would be invisible until the 90s reconciler.
    #[test]
    fn extension_root_edit_lands_via_drain() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Nested layout: config source under `src/cf`, an extension auto-discovered under
        // `src/cfe/*` (both need a `Configuration.xml` to be recognised).
        let cf = root.join("src/cf");
        fs::create_dir_all(&cf).unwrap();
        fs::write(cf.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_common_module(&cf, "Сервер", true, "&НаСервере\nФункция Ч() Экспорт КонецФункции");
        let ext = root.join("src/cfe/Расш");
        fs::create_dir_all(&ext).unwrap();
        fs::write(ext.join("Configuration.xml"), "<Configuration/>").unwrap();
        write_common_module(
            &ext,
            "РасшМодуль",
            true,
            "&НаСервере\nФункция Р() Экспорт КонецФункции",
        );

        // Build the hub over the SAME roots the scan sees (source + extensions), as production does.
        let project = project_model::Project::new(root);
        let mut roots = vec![project.source_path().to_path_buf()];
        roots.extend(project.extension_paths().iter().map(|(_, p)| p.clone()));
        assert!(roots.len() >= 2, "the extension root must be discovered: {roots:?}");
        let hub = WorkspaceChangeHub::start(roots);
        assert!(hub.wait_until_watching(Duration::from_secs(5)), "the hub must arm");

        let mut state =
            DiagnosticsState::for_workspace(root.to_path_buf()).with_change_hub(hub.clone());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        wait_ready(&state);

        let ext_module = ext.join("CommonModules/РасшМодуль/Ext/Module.bsl");
        let resident = state.read(|r, _| r.file_id_for(&ext_module).is_some());
        assert!(
            matches!(resident, ResidentOutcome::Ready(true, _)),
            "the extension module must be resident",
        );

        let gen0 = raw_generation(&state);
        std::thread::sleep(Duration::from_millis(10));
        fs::write(&ext_module, "&НаСервере\nФункция Р() Экспорт Возврат 9; КонецФункции\n")
            .unwrap();

        assert!(wait_for_apply(&state, gen0), "an extension-root edit is delivered via drain");
        assert_eq!(state.scan_count(), 0, "the event-driven path performs no scan");
    }

    /// `BSL_MCP_RECONCILE_SECS` clamping: `0` and garbage fall back to the default; small
    /// positive values are floored so the sweeper cannot busy-loop; valid values pass through.
    #[test]
    fn reconcile_interval_clamps_bad_env() {
        assert_eq!(clamp_reconcile_interval(None), RECONCILE_INTERVAL, "unset/garbage → default");
        assert_eq!(clamp_reconcile_interval(Some(0)), RECONCILE_INTERVAL, "zero → default");
        assert_eq!(clamp_reconcile_interval(Some(1)), MIN_RECONCILE_INTERVAL, "floored");
        assert_eq!(clamp_reconcile_interval(Some(4)), MIN_RECONCILE_INTERVAL, "floored");
        assert_eq!(clamp_reconcile_interval(Some(5)), Duration::from_secs(5), "at the floor");
        assert_eq!(clamp_reconcile_interval(Some(120)), Duration::from_secs(120), "passthrough");
        // Unparseable env text becomes `None` before clamping.
        assert_eq!("nonsense".parse::<u64>().ok(), None);
    }

    /// A delivered file outside the scan universe (an editor temp file) is a no-op for the
    /// diagnostics drain: it touches no resident input and triggers no scan on the healthy
    /// hot path. `apply_drained_entries` already ignores non-`.bsl`/`.xml`/config paths.
    #[test]
    fn non_scan_file_delivery_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let (state, hub) = state_with_hub(root);
        state.ensure_loading();
        wait_ready(&state);

        let mut observer = hub.subscribe();
        std::thread::sleep(Duration::from_millis(10));
        fs::write(root.join("CommonModules/Сервер/Ext/Module.bsl.tmp"), "editor swap").unwrap();
        assert!(wait_for_delivery(&hub, &mut observer, ".tmp"), "hub delivered the temp file");

        let gen0 = raw_generation(&state);
        // A read drains the diagnostics cursor (which holds the .tmp), and must apply nothing.
        let _ = state.read(|_, _| ());
        assert_eq!(raw_generation(&state), gen0, "a non-scan file does not move the resident");
        assert_eq!(state.scan_count(), 0, "and triggers no scan on the healthy path");
    }
}
