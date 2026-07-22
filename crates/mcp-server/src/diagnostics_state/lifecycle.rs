use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vfs::{Vfs, VfsPath};

use crate::change_hub::{Health, SinkCursor, WorkspaceChangeHub};
use crate::graph::input::{
    build_source_root, db_for_files_lazy, enumerate_bsl_files, project_config_paths,
};
use crate::graph::scan::scan_file_stats;

use super::drift::{
    compute_freshness, config_fingerprint, reconcile_interval, ScanCache, DRIFT_CHECK_INTERVAL,
};
use super::resident::{canonical_key, DiagnosticsResident, ResidentVfs};
use super::types::{DiagnosticsStatus, ReloadState, ResidentOutcome, StatusReport, WatchReport};

/// Drop the resident database after this long with no `diagnostics file` call, so a
/// standalone server reclaims the ~2.8 GB after a burst. The next call rebuilds.
const IDLE_EVICTION: Duration = Duration::from_secs(600);

/// How often the idle sweeper wakes to check the last-access time.
const SWEEP_INTERVAL: Duration = Duration::from_secs(30);

/// Everything mutable about the resident db, guarded by one `Mutex`. The lock is held
/// for the duration of a per-file query or an incremental/full reload, so the two are
/// mutually exclusive (the db is `!Sync`, so a query cannot run off-thread anyway).
pub(super) struct Inner {
    pub(super) status: DiagnosticsStatus,
    pub(super) resident: Option<DiagnosticsResident>,
    /// Per-file `(path → stat fingerprint)` from the last build/apply, for drift diff.
    pub(super) stats: HashMap<String, u64>,
    /// Folded fingerprint of the analyzer config files, for config drift.
    pub(super) config_fp: u64,
    pub(super) generation: u64,
    pub(super) reload: ReloadState,
    /// When the current `Loading` build started, for the `status`/`loading` envelope's
    /// `elapsed_ms`. Set on `Idle → Loading`, cleared when the resident becomes `Ready`.
    pub(super) loading_since: Option<Instant>,
}

/// Handle to the workspace diagnostics database. Cheap to clone (shared `Arc`s).
#[derive(Clone)]
pub(crate) struct DiagnosticsState {
    pub(super) inner: Arc<Mutex<Inner>>,
    pub(super) scan: Arc<Mutex<Option<ScanCache>>>,
    pub(super) last_access: Arc<Mutex<Instant>>,
    pub(super) shutdown: Arc<AtomicBool>,
    /// Guards spawning exactly one idle sweeper for the lifetime of the handle.
    pub(super) sweeper_started: Arc<AtomicBool>,
    pub(super) workspace_root: Option<PathBuf>,
    pub(super) drift_interval: Duration,
    pub(super) eviction_after: Duration,
    /// The daemon's filesystem change hub, when this profile has one. Drift is served
    /// event-driven (drain-on-read) while the hub is healthy, falling back to the
    /// throttled scan otherwise. `None` for reference/shared profiles and tests, which
    /// keep the pure scan path.
    pub(super) change_hub: Option<WorkspaceChangeHub>,
    /// This state's cursor into the hub. Subscribed when a resident is (re)built and
    /// dropped on eviction, so an idle diagnostics profile never pins the accumulator.
    pub(super) hub_cursor: Arc<Mutex<Option<SinkCursor>>>,
    /// Set by [`Self::force_rescan`]: forces the next poll onto the scan path even when
    /// the hub is healthy (the `metadata object` miss escape hatch).
    pub(super) force_scan: Arc<AtomicBool>,
    /// When the reconciler last ran a watchdog scan.
    pub(super) last_reconcile: Arc<Mutex<Instant>>,
    pub(super) reconcile_interval: Duration,
    /// Count of actual workspace walks (not cache hits), so a test can assert the
    /// event-driven hot path performs no scan.
    pub(super) scan_count: Arc<AtomicUsize>,
    /// One-shot test seam fired between the reconciler's first drain and its scan.
    #[cfg(test)]
    pub(super) reconcile_probe: ReconcileProbe,
}

/// A one-shot callback the reconciler fires between its first drain and its scan.
#[cfg(test)]
pub(super) type ReconcileProbe = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;

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
    /// Whether a hub cursor is currently held (dropped on eviction).
    /// Drain this state's cursor and throw the entries away, advancing past them without
    /// applying — simulating a lossy sink so the reconciler has an undelivered change to
    /// catch.
    /// Arm the one-shot reconciler probe (fired between its first drain and its scan).
    /// Detect and handle on-disk drift since the last build/apply. Reconciled in place
    /// (under the resident mutex) for `.bsl` body edits and any `.xml` add/remove/edit —
    /// the latter through a metadata-substrate point-refresh. Only a non-`.xml` add/remove
    /// or an analyzer-config change forces a full off-thread rebuild. Throttled and a
    /// no-op unless Ready.
    /// The throttled full-scan drift path: at most one workspace walk per drift window,
    /// diffed against the last-applied stats and reconciled through the same rules the
    /// hub-driven path feeds. Returns whether any drift was found (so the reconciler can
    /// tell a lossy-backend miss from an in-sync workspace). This is the parity path used
    /// when there is no hub or the hub is degraded.
    /// Apply already-classified full-scan drift: a full rebuild for structural/config
    /// drift, else an in-place metadata + body apply. Returns whether any drift was
    /// handled. Shared by the scan path and the reconciler (which classifies once, then
    /// re-checks late-delivered events before deciding to degrade).
    /// The event-driven drift path: drain this state's cursor and reconcile only the
    /// changed paths. Empty drain → nothing to do (crucially, NO scan on the hot path).
    /// A hub overflow (`rescan_required`) → fall back to the full scan, today's path.
    /// Reconcile the paths a drain reported. Events are hints, stats are truth: each path
    /// is re-stat'd and classified through the SAME fingerprint diff the scan uses, then
    /// fed the identical downstream — a full rebuild for structural/config drift, else an
    /// in-place metadata + body apply. Only the affected paths are stat'd, never the tree.
    /// Apply already-classified event-driven drift to the resident and advance the drift
    /// baseline incrementally (only the drained paths change). Shares the exact resident
    /// mutation — substrate point-refresh + body re-key + `Running`-guard — with the scan
    /// path via [`apply_resident_changes`]; only the stats update differs (delta vs full
    /// rebase), because the drain has no whole-workspace scan to rebase onto.
    #[allow(
        clippy::too_many_arguments,
        reason = "one bucket per drift class, same as the classifier"
    )]
    /// Reconcile metadata-only structural drift in place, without a whole-db rebuild:
    /// point-refresh the metadata substrate for the drifted `.xml` (re-discovering the
    /// affected roots and re-reading only changed/new composing files) and re-key the
    /// drifted `.bsl` bodies to their on-disk revision. Everything runs under ONE hold of
    /// the resident mutex — the same discipline as a body-only apply — so a concurrent
    /// full rebuild (which also takes the lock) can never swap the resident mid-apply. A
    /// modified `.bsl` with no resident FileId means the file universe moved, so we bail
    /// to a full rebuild. The drift baseline is rebased to `scan` once reconciled, and the
    /// generation is bumped only when a Salsa input actually moved.
    /// Spawn a full rebuild (at most one in flight) that replaces the resident db.
    /// Peak RAM is bounded: the new db is built, then swapped under the write lock and
    /// the old one dropped — the brief overlap is the price of a structural change.
    pub(super) fn kick_full_reload(&self) {
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
    pub(super) fn catch_build<T>(build: impl FnOnce() -> anyhow::Result<T>) -> Result<T, String> {
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
        let project = project_model::Project::new(root)
            .map_err(|e| anyhow::anyhow!("invalid project at {}: {e}", root.display()))?;
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
    /// (Re)subscribe this state's hub cursor, dropping any prior one. Called at the start
    /// of a (re)build so the fresh cursor is snapshotted at that moment: events BEFORE the
    /// build are captured by the build's own baseline scan, events DURING/AFTER it stay
    /// pending and apply to the freshly-published resident.
    /// The canonical paths of the analyzer config files at the workspace root — the exact
    /// set [`config_fingerprint`] folds. A drained path counts as config drift only if it
    /// is one of these, matching the scan path (which fingerprints only `root.join(name)`)
    /// so an identically-named file elsewhere in the tree is not a spurious rebuild trigger.
    /// Drop this state's hub cursor so an evicted (or never-built) resident does not pin
    /// the accumulator against reclamation.
    /// Drain this state's cursor once, apply the delivered changes, and return the set of
    /// canonical paths the drain reported (empty when there is no cursor). A hub overflow
    /// (`rescan_required`) reconciles via a full scan and reports no paths. The path set
    /// lets the reconciler tell a late-but-delivered edit from a genuinely-missed one.
    /// The reconciler/watchdog: a periodic full scan that catches any drift the hub's
    /// event stream failed to deliver (a lossy backend). Runs on the idle sweeper thread.
    /// Applies everything the hub delivered, then scans for the residue; a change that a
    /// second drain shows was merely late (delivered DURING the scan, not missed) does not
    /// degrade — only genuinely-undelivered file drift does.
    /// A one-shot test seam fired between the reconciler's first drain and its scan, so a
    /// test can inject an edit that lands DURING the scan window (delivered, not missed).
    /// The throttled disk scan: at most one walk per drift interval, its result shared
    /// by concurrent callers. Returns a borrowed view valid until the next scan.
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
/// The freshness verdict for `inner` against an optional drift `scan`. Computed by the
/// caller while holding the inner lock, so the verdict (`revision`/`stale`/`reload`) is
/// atomic with the generation a read serves. `scan` is `None` when the db is not Ready
/// (nothing to compare), making `stale` depend only on an in-flight reload.
/// A fold of the analyzer config files' `(presence, len, mtime)`. Any change forces a
/// full rebuild because it can alter the project's extension set and thus the db inputs.
pub(super) fn lock_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
