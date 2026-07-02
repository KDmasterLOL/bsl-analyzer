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
//! workspace on disk (throttled). A changed `.bsl` body is re-keyed with `set_file_text`
//! and any `.xml` add/remove/edit point-refreshes the metadata substrate — both in place
//! under the resident mutex. Only a non-`.xml` file add/remove or an analyzer config-file
//! change falls back to a full off-thread rebuild. An idle sweeper drops the resident db
//! after a quiet period so a standalone `mcp serve` reclaims the memory after a burst.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ide::{Analysis, RootDatabaseImpl};
use vfs::{FileId, Vfs, VfsPath};

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

/// The shortest interval a forced re-scan (a `metadata object` miss retry) may bypass the
/// drift throttle. Bounds how fast a loop of genuinely-absent lookups can stat-walk the
/// workspace, mirroring the retired `MetadataCache`'s force floor.
const FORCE_RESCAN_FLOOR: Duration = Duration::from_millis(250);

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

        // A non-XML add/remove moves the file universe (a new or vanished `.bsl`), and an
        // analyzer-config edit can change the extension set — neither is expressible as a
        // substrate point-refresh, so both force a full rebuild. Everything else (any
        // `.xml` add/remove/edit, plus `.bsl` body edits) is reconciled in place.
        let full_rebuild = config_changed
            || changes.added.iter().any(|p| !p.ends_with(".xml"))
            || changes.removed.iter().any(|p| !p.ends_with(".xml"));

        if full_rebuild {
            self.kick_full_reload();
            return;
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
        let modified_bsl: Vec<String> =
            changes.modified.iter().filter(|p| !p.ends_with(".xml")).cloned().collect();
        self.apply_metadata_and_body_drift(&xml_paths, &modified_bsl, &scan);
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
        modified_bsl: &[String],
        scan: &OwnedScan,
    ) {
        use ide_host_core::{set_file_text_source, FileTextSource};

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
            let Inner { resident: Some(resident), stats, generation, .. } = &mut *inner else {
                return;
            };

            // Pre-classification: an XML path resolving outside every registered config
            // root is drift the point-refresh cannot express. `refresh_metadata_substrate`
            // gates its re-discovery on `changed.starts_with(root)`, so such a path
            // silently no-ops there — and the baseline rebase below would then forget the
            // change forever. It arises when a metadata subtree is a symlink whose
            // canonical (scan) path resolves outside the root. Bail to a full rebuild
            // WITHOUT rebasing the baseline; a full rebuild re-reads through the discovery
            // joins, symlinks and all. (`all_config_paths` roots are canonicalised at build.)
            let config_roots = resident.db.all_config_paths();
            let xml_outside_roots =
                xml_paths.iter().any(|p| !config_roots.iter().any(|(_, root)| p.starts_with(root)));

            if xml_outside_roots {
                needs_rebuild = true;
            } else {
                // Metadata XML drift is two independent invalidations, both under this lock.
                // (1) Point-refresh the per-MDO substrate: re-discover the owning roots and
                // re-read only the changed/new composing files `discover_*` enrolls.
                // (2) Bump each owning root's config revision UNCONDITIONALLY. `refresh`
                // reports movement only for enrolled composing files, but a non-enrolled
                // `.xml` a whole-config `load_from_directory` would re-read (`Configuration.xml`,
                // form/template/command descriptors) must still invalidate the coarse
                // Channel-2 `load_configuration` memo — a cheap revision counter, recomputed
                // lazily and only if consumed. Any `.xml` drift is observable movement.
                let mut moved = false;
                if !xml_paths.is_empty() {
                    ide_host_core::refresh_metadata_substrate(
                        &mut resident.db,
                        &resident.vfs,
                        xml_paths,
                    );
                    resident.db.bump_config_for_paths(xml_paths.iter().map(|p| p.as_path()));
                    moved = true;
                }

                // `.bsl` bodies: disk-backed re-key, mirroring the body-only apply. A body
                // already at its on-disk fingerprint (a racing caller beat us) is skipped.
                for path in modified_bsl {
                    let Some(&fp) = new_fp.get(path.as_str()) else { continue };
                    if stats.get(path).copied() == Some(fp) {
                        continue;
                    }
                    let Some(&file_id) = resident.by_path.get(path) else {
                        needs_rebuild = true; // a modified `.bsl` we never indexed → structural
                        break;
                    };
                    match base_db::read_disk_text(Path::new(path)) {
                        Ok(text) => set_file_text_source(
                            &mut resident.db,
                            file_id,
                            FileTextSource::Disk(&text),
                        ),
                        // Unreadable now: an empty overlay so a later query yields `""`
                        // instead of panicking on the disk re-read, matching the load path.
                        Err(_) => set_file_text_source(
                            &mut resident.db,
                            file_id,
                            FileTextSource::Tombstone,
                        ),
                    }
                    moved = true;
                }

                if !needs_rebuild {
                    // Advance the drift baseline to the scan we reconciled against: every
                    // applied body and every XML add/remove/edit is now reflected in the
                    // resident, so its state equals `scan`. Rebasing even when nothing moved
                    // (a pure mtime touch with unchanged content) stops us re-scanning it
                    // every window.
                    *stats = scan.stats.iter().map(|s| (s.path.clone(), s.fingerprint())).collect();
                    if moved {
                        *generation += 1;
                        tracing::info!(
                            xml = xml_paths.len(),
                            bodies = modified_bsl.len(),
                            generation = *generation,
                            "diagnostics metadata drift refresh",
                        );
                    }
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
        // Canonicalise the config roots so a module back-link the metadata substrate
        // resolves (`root.join("CommonModules/X/Ext/Module.bsl")`) matches the
        // canonical `.bsl` path `enumerate_bsl_files` produced — otherwise the reverse
        // lookup would miss and silently drop the back-link on a symlinked workspace.
        let config_paths: Vec<(Option<String>, PathBuf)> =
            crate::graph::project_config_paths(&project)
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
}
