//! Graph lifecycle state and publication protocol.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use bsl_search::SearchEngine;

use crate::change_hub::{SinkCursor, WorkspaceChangeHub};

use super::snapshot::{FpMapState, PooledSnapshotEntry, ScanCache};
use super::types::{
    FusedStartup, GraphPublishSignal, GraphStatus, GraphStatusReport, NudgeOutcome,
};

/// Minimum time between on-disk drift scans. A scan stats every `.bsl`/`.xml`
/// file under the config roots, so throttling bounds its cost regardless of how
/// fast an agent fires `graph` calls.
const DRIFT_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// State of an in-flight or last-attempted background reload, surfaced to agents
/// so a failed reload is visible rather than leaving them at `stale=true` forever.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ReloadState {
    /// No reload in flight; the published snapshot is the latest.
    Idle,
    /// A reload triggered by detected drift is running in the background.
    Running,
    /// The last reload failed; the previous snapshot is still served.
    Failed(String),
}

impl ReloadState {
    pub(super) fn label(&self) -> &'static str {
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
pub(super) struct Published {
    /// Whether this snapshot was published KNOWING it does not reflect current disk
    /// (the boot stale-publish). Any successful build/reload publish replaces it with
    /// a fresh entry. Gates the boot leftover-marks consume: even if the pre-claimed
    /// catch-up fails (`reload` drops to `Failed`, so `drift_pending` no longer
    /// holds), marks must not be cleared against this snapshot.
    pub(super) stale: bool,
    pub(super) generation: u64,
    pub(super) fingerprint: crate::graph_db::GraphFp,
    pub(super) reload: ReloadState,
}

/// Everything mutable about the published graph, guarded by a single mutex. Locks
/// are only held for brief reads/swaps — the load and the drift scan run without
/// this lock held.
pub(super) struct Inner {
    pub(super) status: GraphStatus,
    pub(super) published: Option<Published>,
}

/// Handle to the workspace call graph. Cheap to clone (shared `Arc`s).
///
/// Loading is lazy: the SQLite graph is built off the workspace on first use, so a
/// server whose user never touches the graph pays nothing. The build is triggered
/// on the first `graph` tool call.
#[derive(Clone)]
pub(crate) struct GraphState {
    pub(super) inner: Arc<Mutex<Inner>>,
    pub(super) scan: Arc<Mutex<Option<ScanCache>>>,
    pub(super) workspace_root: Option<PathBuf>,
    pub(super) drift_interval: Duration,
    /// The daemon's change hub, when this profile has one. The graph does NOT apply
    /// drift in place (its fast path deliberately full-rebuilds on a metadata touch); the
    /// hub only lets a freshness check invalidate its throttled fingerprint cache the
    /// instant a change is delivered, instead of waiting out the drift throttle.
    pub(super) change_hub: Option<WorkspaceChangeHub>,
    /// This graph's cursor into the hub. Subscribed lazily on first freshness check.
    pub(super) hub_cursor: Arc<Mutex<Option<SinkCursor>>>,
    /// Count of actual fingerprint walks (cache misses), so a test can assert an irrelevant
    /// delivered change did NOT invalidate the throttled cache and re-trigger a scan.
    pub(super) scan_count: Arc<AtomicUsize>,
    /// Event-maintained per-file stat map mirroring what a fingerprint walk observes,
    /// so a query-path freshness check can fold ~100k in-memory entries (<1ms) instead
    /// of stat-walking the tree (seconds). Seeded by a real walk, patched per delivered
    /// hub entry, and re-anchored to a real walk every [`WALK_VERIFY_INTERVAL`] — the
    /// hub cannot see everything (events predating its subscribe, writes through paths
    /// outside the watched roots), so the walk stays the periodic source of truth.
    /// Dropped to `None` (next check walks) on hub overflow or a subtree removal.
    pub(super) fp_map: Arc<Mutex<FpMapState>>,
    /// Idle read handles onto the CURRENT published graph file, tagged with the
    /// freshness token they were opened under. Opening the multi-GB SQLite file costs
    /// ~a second on a large configuration; a pooled handle keeps serving the same
    /// coherent snapshot for free. Entries for superseded generations are discarded
    /// lazily at checkout (the tag no longer matches the published generation).
    pub(super) snapshot_pool: Arc<Mutex<Vec<PooledSnapshotEntry>>>,
    /// Invoked on this graph's background thread immediately after each publish/adopt,
    /// once the inner lock is released — the moment the graph "has caught up" and a
    /// consumer (search context re-render) may read the fresh graph. Never called on a
    /// query path. Receives a [`GraphPublishSignal`]: `build_start_seq` bounds which marks
    /// the consumer may clear (correctness), `drift_pending` is a fast-path hint.
    pub(super) on_published: Option<Arc<dyn Fn(GraphPublishSignal) -> bool + Send + Sync>>,
    /// The store's monotonic context-dirty mark counter, wired once the search engine
    /// exists (the engine is built after this graph). Read at each build's start to capture
    /// its `build_start_seq`. Absent (never wired, e.g. a disabled/reference graph, or a build
    /// racing the one-time boot wiring) reads as `0` — a consume of NOTHING, so an early build's
    /// publish can never clear a mark against a graph that predates it. Marks left pending
    /// through that window are picked up explicitly once wiring completes (see
    /// [`Self::consume_leftover_marks`]).
    pub(super) mark_seq: Arc<OnceLock<Arc<AtomicI64>>>,
    /// A one-shot armed by [`Self::consume_leftover_marks`] at boot to consume context-dirty
    /// marks left by a prior daemon run. Stores the mark-seq bound captured at the instant the
    /// caller observed those leftovers (before the engine was published): `0` = disarmed,
    /// `> 0` = armed with that bound. The boot build's own publish ran with the unwired (`0`)
    /// bound and cleared nothing; this makes the next publish (or an immediate call, for an
    /// already-published graph) re-run the refresh with the STORED bound — never a later live
    /// read, which could clear a mark a new drift stamped after the capture. Cleared via `swap`
    /// to `0`, so it fires exactly once.
    pub(super) leftover_bound: Arc<AtomicI64>,
    /// A drift observed (via [`Self::nudge_rebuild`]) while a build/reload was already in
    /// flight, so no reload slot could be claimed then. Re-checked at the next publish
    /// ([`Self::notify_published`]): if disk moved past the just-published build, a follow-up
    /// reload is claimed. Without this a drift arriving mid-build is silently lost — the
    /// build's publish would consume the search context marks against a graph built BEFORE
    /// the change.
    pub(super) pending_nudge: Arc<AtomicBool>,
    /// A topology-changed publish whose hook could not run the whole-collection
    /// context refresh (engine not yet published, or deferred behind a fresher
    /// reload). Re-raised on the next publish so the refresh is never lost.
    pub(super) pending_topology_refresh: Arc<AtomicBool>,
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
            change_hub: None,
            hub_cursor: Arc::new(Mutex::new(None)),
            scan_count: Arc::new(AtomicUsize::new(0)),
            on_published: None,
            pending_nudge: Arc::new(AtomicBool::new(false)),
            pending_topology_refresh: Arc::new(AtomicBool::new(false)),
            mark_seq: Arc::new(OnceLock::new()),
            leftover_bound: Arc::new(AtomicI64::new(0)),
            fp_map: Arc::new(Mutex::new(FpMapState::default())),
            snapshot_pool: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Number of fingerprint walks performed (cache misses), for asserting that an
    /// irrelevant hub delivery did not invalidate the throttled cache.
    #[cfg(test)]
    pub(super) fn scan_count(&self) -> usize {
        self.scan_count.load(Ordering::SeqCst)
    }

    /// Attach the daemon's change hub so a freshness check invalidates the throttled
    /// fingerprint cache as soon as a change is delivered, without waiting out the throttle.
    pub(crate) fn with_change_hub(mut self, hub: WorkspaceChangeHub) -> Self {
        self.change_hub = Some(hub);
        self
    }

    /// Attach a hook invoked on this graph's background thread after each publish/adopt
    /// (see [`Self::notify_published`]). Used to drive the search context re-render once
    /// the graph has caught up with an `.xml` drift. The hook receives
    /// a [`GraphPublishSignal`]: `build_start_seq` bounds which marks it may clear
    /// (correctness), `drift_pending` is a skip-this-round hint (optimization).
    pub(crate) fn with_publish_hook(
        mut self,
        hook: Arc<dyn Fn(GraphPublishSignal) -> bool + Send + Sync>,
    ) -> Self {
        self.on_published = Some(hook);
        self
    }

    /// Wire the store's monotonic mark-seq counter, shared with the search engine so a
    /// build reads the same value the store increments. Called once, after the engine is
    /// built (the engine outlives the graph's construction). Setting it again is a no-op —
    /// there is one counter per workspace store.
    pub(crate) fn set_mark_seq_source(&self, mark_seq: Arc<AtomicI64>) {
        let _ = self.mark_seq.set(mark_seq);
    }

    /// The current mark-seq high-water value, captured at a build's start as its
    /// `build_start_seq`. Unwired (disabled/reference graph, or a build racing the one-time
    /// wiring at boot) reads as `0`: a consume of NOTHING. This deliberately makes an early
    /// build's publish clear no marks, rather than an unbounded consume that could clear a
    /// mark stamped after the publish snapshotted disk. Marks stranded across the wiring
    /// window are recovered by [`Self::consume_leftover_marks`].
    pub(super) fn current_mark_seq(&self) -> i64 {
        self.mark_seq.get().map(|counter| counter.load(Ordering::SeqCst)).unwrap_or(0)
    }

    /// Fire the publish hook, if any. Called after a publish/adopt with no graph lock
    /// held, so the hook may take other locks (e.g. the search engine) without risking a
    /// lock-order inversion against the graph's inner mutex.
    ///
    /// FIRST re-claims a reload for any drift recorded while this build was in flight, so
    /// the hook observes the resulting reload state through [`Self::drift_pending`]: if a
    /// follow-up reload is now catching up, the hook can skip this round and let that
    /// reload's publish consume the marks (against a fresher graph).
    ///
    /// `build_start_seq` is captured by the CALLING build at its start and passed straight
    /// through — never re-read here — so the reclaim's own new build (which captures its own
    /// later seq on another thread) cannot move the bound this publish hands the hook. The
    /// bound is what keeps the consumption correct: only marks at or below it — drifts this
    /// build already reflects — may be cleared.
    pub(super) fn notify_published(&self, build_start_seq: i64, topology_changed: bool) {
        // Idle pooled read handles belong to the superseded file; drop them now so
        // they release it promptly instead of waiting out lazy checkout discards.
        lock_recover(&self.snapshot_pool).clear();
        if self.pending_nudge.swap(false, Ordering::SeqCst) {
            self.reclaim_pending_reload();
        }
        // A topology refresh survives publishes that cannot run it: the flag is
        // re-raised whenever the hook reports the requested whole-collection
        // re-render did not happen (deferred behind a fresher reload, or the
        // search engine is not published yet).
        let topology =
            topology_changed || self.pending_topology_refresh.swap(false, Ordering::SeqCst);
        if !self.fire_hook(build_start_seq, topology) && topology {
            self.pending_topology_refresh.store(true, Ordering::SeqCst);
        }
        // A leftover-marks consume was armed at boot (see `consume_leftover_marks`). This build's
        // own publish (above) captured its own `build_start_seq` — for the pre-wiring boot build
        // that is `0`, which clears nothing — so re-run the hook once with the bound captured when
        // the leftovers were observed, picking up marks a prior run left pending. Single-shot via
        // the `swap`. The STORED bound (never a live read) is what keeps the consume from clearing
        // a mark stamped after the capture: that mark is a new drift with its own nudge→publish.
        let leftover_bound = self.leftover_bound.swap(0, Ordering::SeqCst);
        if leftover_bound != 0 {
            self.fire_hook(leftover_bound, false);
        }
    }

    /// Invoke the publish hook, if any, with the given bound. The consumer sees the current
    /// [`Self::drift_pending`] so it can defer when a fresher reload is imminent. Returns
    /// whether a requested topology refresh was handled (vacuously true when none was
    /// requested or no hook is attached).
    fn fire_hook(&self, build_start_seq: i64, topology_changed: bool) -> bool {
        match &self.on_published {
            Some(hook) => hook(GraphPublishSignal {
                drift_pending: self.drift_pending(),
                build_start_seq,
                topology_changed,
            }),
            None => true,
        }
    }

    /// Recover context-dirty marks a PRIOR daemon run left in the persisted `context_dirty`
    /// table. The boot graph build published BEFORE the mark-seq source was wired, so it ran
    /// with the unwired (`0`) bound and cleared nothing; the persisted marks are still pending.
    /// The caller captures `leftover_bound` — the mark-seq high-water at the instant it observed
    /// these leftovers, before the engine was published — and passes it in. That boot build read
    /// post-restart disk, so a consume against it bounded by `leftover_bound` clears exactly the
    /// leftovers and no more. Correctness: leftover marks predate this daemon run, so any
    /// boot-published graph REFLECTING CURRENT DISK (fresh build, fused, or fingerprint-valid
    /// cached) already reflects their cause — the one exception, a stale boot publish, pre-claims
    /// the reload slot atomically so the `drift_pending` guard below defers this consume to the
    /// catch-up publish; a mark stamped after the capture (seq above the bound) is a new drift
    /// and is guaranteed its own nudge→publish cycle, so it must not be cleared here against the
    /// boot graph that predates it. Handles both post-boot states: an already-published (`Ready`)
    /// graph consumes immediately; an in-flight build (`Loading`, whose own publish captured the
    /// pre-wiring `0` bound and would clear nothing) arms a one-shot so ITS publish runs the
    /// consume with `leftover_bound`. A `Ready` graph with a fresher reload already in flight
    /// (`drift_pending`) leaves the one-shot armed so that reload's publish handles it against
    /// the fresher graph.
    pub(crate) fn consume_leftover_marks(&self, leftover_bound: i64) {
        // Arm first (store the captured bound), then observe state, so a build publishing
        // concurrently either runs the follow-up itself or leaves it for the immediate consume
        // below — the `swap` to `0` in both paths keeps it single-shot.
        self.leftover_bound.store(leftover_bound, Ordering::SeqCst);
        // `published_stale` outlives `drift_pending`: if the stale boot's pre-claimed
        // catch-up FAILS, the slot drops to `Failed` and `drift_pending` no longer
        // holds — but the snapshot still predates the marks' causes, so the one-shot
        // must stay armed for the next successful publish.
        if matches!(self.status(), GraphStatus::Ready { .. })
            && !self.drift_pending()
            && !self.published_stale()
        {
            let bound = self.leftover_bound.swap(0, Ordering::SeqCst);
            if bound != 0 {
                self.fire_hook(bound, false);
            }
        }
    }

    /// Re-run the reload claim for a drift recorded (as a pending nudge) while a build was in
    /// flight. Runs on the publish thread once the graph is `Ready`: claims a reload if disk
    /// drifted past the just-published build; re-arms the pending flag if a reload is somehow
    /// already running so its own publish re-checks; otherwise the published build already
    /// matches disk and nothing is scheduled.
    fn reclaim_pending_reload(&self) {
        if self.claim_reload_slot() {
            self.spawn_reload();
        } else if self.reload_running() {
            self.pending_nudge.store(true, Ordering::SeqCst);
        }
    }

    /// Whether the published snapshot is the boot stale-publish (known not to reflect
    /// current disk). See [`Published::stale`].
    fn published_stale(&self) -> bool {
        lock_recover(&self.inner).published.as_ref().is_some_and(|p| p.stale)
    }

    /// Whether a published reload is currently `Running`.
    fn reload_running(&self) -> bool {
        matches!(
            lock_recover(&self.inner).published.as_ref().map(|p| &p.reload),
            Some(ReloadState::Running)
        )
    }

    /// Whether a fresher build is already catching up: a nudge was recorded while a build
    /// was in flight, or a reload is currently running. The publish hook uses this only as a
    /// fast-path hint — when a follow-up reload will publish shortly it can skip this round
    /// and let that reload's publish re-render against the fresher graph. It is NOT what
    /// makes consumption correct: the `build_start_seq` bound already prevents clearing a
    /// mark against a graph that predates its drift, whatever this returns.
    pub(crate) fn drift_pending(&self) -> bool {
        self.pending_nudge.load(Ordering::SeqCst) || self.reload_running()
    }

    pub(crate) fn status(&self) -> GraphStatus {
        lock_recover(&self.inner).status.clone()
    }

    /// Lifecycle snapshot for the `status` action. For a ready graph it also reports the
    /// served revision and on-disk freshness (and, like every freshness check, kicks an async
    /// reload on drift) — so this walks the filesystem and must be called from a blocking
    /// context. A `Ready` status whose snapshot momentarily cannot be opened is reported as
    /// `loading` (a reload is renaming the file into place), never as a torn read.
    pub(crate) fn status_report(&self) -> GraphStatusReport {
        let report = |state: &'static str| GraphStatusReport {
            state,
            files: None,
            revision: None,
            stale: None,
            reload: None,
            error: None,
        };
        match self.status() {
            GraphStatus::Disabled => report("disabled"),
            GraphStatus::Idle | GraphStatus::Loading => report("loading"),
            GraphStatus::Failed(msg) => GraphStatusReport { error: Some(msg), ..report("failed") },
            GraphStatus::Ready { files } => match self.snapshot() {
                Some(snapshot) => {
                    let freshness = self.freshness(&snapshot);
                    GraphStatusReport {
                        files: Some(files),
                        revision: Some(freshness.revision),
                        stale: Some(freshness.stale),
                        reload: Some(freshness.reload),
                        ..report("ready")
                    }
                }
                None => report("loading"),
            },
        }
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

    /// Claim the initial build for an external builder (the fused cold-build path).
    /// Transitions `Idle → Loading` like [`Self::ensure_loading`] but spawns no loader
    /// thread — the caller builds the graph itself and publishes it via
    /// [`Self::adopt_prebuilt`]. Returns `false` for a disabled graph or one already
    /// loading/ready/failed, in which case the caller must not build (the normal
    /// lifecycle owns it).
    pub(crate) fn try_begin_external_build(&self) -> bool {
        if self.workspace_root.is_none() {
            return false;
        }
        let mut inner = lock_recover(&self.inner);
        if inner.status != GraphStatus::Idle {
            return false;
        }
        inner.status = GraphStatus::Loading;
        true
    }

    /// Publish a graph database an external builder wrote and atomically renamed into
    /// [`graph_db_path`]. Mirrors the tail of [`Self::run_load`]: clears the scan
    /// cache and sets `published` + `Ready`. `force_stale` is already stamped into the
    /// file's meta (read back by a snapshot's freshness token), so it is not tracked
    /// here. Call only after a successful [`Self::try_begin_external_build`] + rename.
    pub(crate) fn adopt_prebuilt(
        &self,
        generation: u64,
        fingerprint: crate::graph_db::GraphFp,
        files: usize,
    ) {
        *lock_recover(&self.scan) = None;
        let mut inner = lock_recover(&self.inner);
        inner.published =
            Some(Published { generation, fingerprint, stale: false, reload: ReloadState::Idle });
        inner.status = GraphStatus::Ready { files };
    }

    /// Abandon a claimed external build that did not produce a usable database, so the
    /// normal lazy/eager path can rebuild. Reverts `Loading → Idle`.
    pub(crate) fn abort_external_build(&self) {
        let mut inner = lock_recover(&self.inner);
        if inner.status == GraphStatus::Loading {
            inner.status = GraphStatus::Idle;
        }
    }

    /// Schedule the graph to catch up with a drift another consumer observed (the search
    /// sink), WITHOUT waiting for a `graph` tool freshness check. A user who only calls
    /// `search_code` never triggers a graph rebuild otherwise, so an `.xml` edit would
    /// leave the search chunks' graph context stale forever — the context re-render only
    /// runs on a graph publish. This closes that chain end-to-end: xml drift →
    /// context-dirty marks + this nudge → background rebuild → publish → hook → refresh.
    ///
    /// Single-flight by construction and never blocks (the rebuild runs on a spawned
    /// thread): an unbuilt graph (`Idle`) starts the one initial loader; a published graph
    /// claims the ONE reload slot only when disk drifted since the build and no reload is
    /// already running (so a storm of xml events during a running build queues no extra
    /// rebuilds); `Disabled`/`Loading`/`Failed` schedule nothing. Walks the filesystem for
    /// the drift check, so call from a blocking context (the sink thread), never a query.
    pub(crate) fn nudge_rebuild(&self) -> NudgeOutcome {
        match self.status() {
            GraphStatus::Idle => {
                self.ensure_loading();
                NudgeOutcome::LoadStarted
            }
            // A build is in flight and captured disk at some earlier instant. Record the
            // drift so the build's publish re-checks and reloads if disk moved past what it
            // captured — otherwise the publish would consume the search context marks against
            // a graph built before this change (the drift would be lost).
            GraphStatus::Loading => {
                self.pending_nudge.store(true, Ordering::SeqCst);
                NudgeOutcome::NoOp
            }
            GraphStatus::Ready { .. } => {
                if self.claim_reload_slot() {
                    self.spawn_reload();
                    NudgeOutcome::ReloadClaimed
                } else {
                    // Couldn't claim: either the graph already matches disk (nothing to do)
                    // or a reload is already `Running`. In the latter case the running build
                    // may have started before this drift, so record a pending nudge — its
                    // publish re-checks and reloads again if disk still differs.
                    if self.reload_running() {
                        self.pending_nudge.store(true, Ordering::SeqCst);
                    }
                    NudgeOutcome::NoOp
                }
            }
            // Nothing to schedule (`Disabled`, or `Failed` — a failed graph does not
            // auto-retry from a nudge).
            GraphStatus::Disabled | GraphStatus::Failed(_) => NudgeOutcome::NoOp,
        }
    }

    /// Claim the single background-reload slot iff the workspace drifted on disk since the
    /// published build and no reload is already `Running`. Returns whether THIS call won
    /// the claim; a caller arriving while a reload runs (or when nothing drifted) gets
    /// `false`. Shares the exact single-flight discipline [`Self::freshness`] uses, so a
    /// nudge and a freshness check cannot both start a reload.
    fn claim_reload_slot(&self) -> bool {
        let disk_fp = self.current_disk_fp();
        let mut inner = lock_recover(&self.inner);
        let Some(published) = inner.published.as_mut() else {
            return false;
        };
        let drifted = disk_fp.map(|fp| fp != published.fingerprint).unwrap_or(false);
        if drifted && published.reload != ReloadState::Running {
            published.reload = ReloadState::Running;
            true
        } else {
            false
        }
    }

    /// Spawn the background reload thread after a successful [`Self::claim_reload_slot`].
    /// On spawn failure the reload slot is marked `Failed` so it is never left stuck
    /// `Running` (which would block every later reload claim).
    pub(super) fn spawn_reload(&self) {
        let state = self.clone();
        let spawned = std::thread::Builder::new()
            .name("bsl-graph-reload".to_owned())
            .spawn(move || state.run_load(true));
        if let Err(e) = spawned {
            let mut inner = lock_recover(&self.inner);
            if let Some(p) = inner.published.as_mut() {
                p.reload = ReloadState::Failed(format!("could not spawn reload: {e}"));
            }
        }
    }

    /// Drive the SqliteLocal startup graph decision in one place: claim the build,
    /// then either reuse a fresh cached graph, build the graph + search chunks in one
    /// fused pass (when an embedder is available), or fall back to a normal lazy graph
    /// build. Returns whether the fused pass already populated the search index, so
    /// the caller knows whether it still needs the standalone indexer.
    pub(crate) fn start_workspace_graph(
        &self,
        engine: &mut SearchEngine,
        source_path: &Path,
    ) -> FusedStartup {
        let Some(workspace_root) = self.workspace_root.clone() else {
            return FusedStartup::Standalone;
        };
        if !self.try_begin_external_build() {
            // A concurrent path (e.g. a graph tool call) already owns the build; index
            // the search engine the normal way against whatever graph it produces.
            return FusedStartup::Standalone;
        }
        // Capture the mark-seq at build start so the publish consumes only marks this build
        // reflects. At boot this is `0` (unwired): the mark-seq source is wired after the engine
        // is published, so a fused/cached boot build clears nothing here. Marks a prior run left
        // pending are recovered explicitly after wiring (see `consume_leftover_marks`).
        let build_start_seq = self.current_mark_seq();
        if self.try_publish_cached(&workspace_root, build_start_seq) {
            // Warm start: the graph is reused from disk and the persisted search index
            // is reused by the standalone indexer's hash-skip (a near no-op).
            return FusedStartup::Standalone;
        }
        // Cached but drifted: stale answers now beat a fused multi-minute rebuild. The
        // stale publish (Ready) supersedes this path's external claim (Loading), the
        // pre-claimed reload catches the graph up, and the search index still reuses
        // its persisted store through the standalone hash-skip.
        if self.try_publish_stale_and_catch_up(&workspace_root) {
            return FusedStartup::Standalone;
        }
        if !engine.has_semantic() {
            // No embedder → no fused semantic pass; build the graph normally and let
            // the caller build the FTS-only index.
            self.abort_external_build();
            self.ensure_loading();
            return FusedStartup::Standalone;
        }
        match self.run_fused_cold_build(engine, source_path, build_start_seq) {
            Ok(()) => FusedStartup::Fused,
            Err(e) => {
                tracing::warn!("fused cold-build failed; falling back to standalone index: {e}");
                self.abort_external_build();
                self.ensure_loading();
                FusedStartup::Standalone
            }
        }
    }
}

/// Lock a mutex, recovering the inner value if a prior holder panicked. The graph
/// mutexes guard brief stores/reads (and one throttled scan), so a poisoned guard
/// still carries valid data.
pub(super) fn lock_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{sample_workspace, wait_ready};
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// A publish hook attached via `with_publish_hook` fires on the graph's background
    /// thread once the build completes and publishes — the seam the search context
    /// re-render hangs on. Without the `notify_published()` call at the publish site the
    /// counter stays zero and this fails.
    #[test]
    fn publish_hook_fires_after_a_build_publishes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let fired = Arc::new(AtomicUsize::new(0));
        let hook = {
            let fired = Arc::clone(&fired);
            Arc::new(move |_signal: GraphPublishSignal| {
                fired.fetch_add(1, Ordering::SeqCst);
                true
            }) as Arc<dyn Fn(GraphPublishSignal) -> bool + Send + Sync>
        };
        let graph = GraphState::for_workspace(root.to_path_buf()).with_publish_hook(hook);
        graph.ensure_loading();
        wait_ready(&graph);

        assert!(
            fired.load(Ordering::SeqCst) >= 1,
            "the publish hook must fire once the graph publishes its build",
        );
    }

    /// The SqliteLocal boot builds the graph and the search chunks in ONE parse pass, and
    /// claims the graph for it through `try_begin_external_build` — which needs the
    /// `Idle → Loading` transition for itself. An eager start that lands first takes that
    /// transition and the claim fails, degrading the fused pass into two. This is why the
    /// boot's eager start is mode-gated and otherwise runs only after the claim.
    #[test]
    fn an_already_started_graph_refuses_the_fused_build_claim() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let graph = GraphState::for_workspace(root.to_path_buf());
        graph.ensure_loading();

        assert!(
            !graph.try_begin_external_build(),
            "a graph already building must refuse the fused claim, not build twice",
        );
        // Let the spawned build finish before the temp workspace goes away.
        wait_ready(&graph);
    }

    /// The mirror image: once the fused build owns the claim, the boot's catch-all start is
    /// inert — it must not spawn a second builder over the one already writing the database.
    /// A spawned loader would publish and fire the hook, so a hook that never fires (while the
    /// claim still reads `Loading`) is what rules a second build out; the status alone would
    /// not, since a second loader leaves it `Loading` too until it publishes.
    #[test]
    fn starting_a_claimed_graph_spawns_no_second_build() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let published = Arc::new(AtomicUsize::new(0));
        let hook = {
            let published = Arc::clone(&published);
            Arc::new(move |_signal: GraphPublishSignal| {
                published.fetch_add(1, Ordering::SeqCst);
                true
            }) as Arc<dyn Fn(GraphPublishSignal) -> bool + Send + Sync>
        };
        let graph = GraphState::for_workspace(root.to_path_buf()).with_publish_hook(hook);
        assert!(graph.try_begin_external_build(), "an idle graph yields the claim");

        graph.ensure_loading();

        // Long enough for a loader spawned by that call to build this two-module workspace and
        // publish: `publish_hook_fires_after_a_build_publishes` waits for the same build.
        std::thread::sleep(Duration::from_secs(2));
        assert_eq!(published.load(Ordering::SeqCst), 0, "no second builder may publish");
        assert_eq!(
            graph.status(),
            GraphStatus::Loading,
            "the external build keeps the claim; nothing else may drive it",
        );
    }

    #[test]
    fn set_mark_seq_source_is_first_writer_wins() {
        let graph = GraphState::disabled();
        let first = Arc::new(AtomicI64::new(7));
        let second = Arc::new(AtomicI64::new(11));

        graph.set_mark_seq_source(Arc::clone(&first));
        graph.set_mark_seq_source(Arc::clone(&second));

        assert_eq!(graph.current_mark_seq(), 7, "the first mark sequence source is retained");
        first.store(13, Ordering::SeqCst);
        assert_eq!(graph.current_mark_seq(), 13, "reads continue using the first source");
    }

    /// A topology refresh the hook cannot run (deferred, engine absent) must be
    /// re-raised on the NEXT publish — otherwise a dependsOn edit landing while
    /// the search engine boots would leave every persisted context stale forever.
    #[test]
    fn an_unhandled_topology_refresh_is_re_raised_on_the_next_publish() {
        use std::sync::atomic::AtomicUsize;

        let seen = Arc::new(AtomicUsize::new(0));
        let handled = Arc::new(AtomicI64::new(0));
        let hook = {
            let seen = Arc::clone(&seen);
            let handled = Arc::clone(&handled);
            Arc::new(move |signal: GraphPublishSignal| {
                if signal.topology_changed {
                    seen.fetch_add(1, Ordering::SeqCst);
                    // First sighting: report unhandled; second: handled.
                    return handled.fetch_add(1, Ordering::SeqCst) > 0;
                }
                true
            }) as Arc<dyn Fn(GraphPublishSignal) -> bool + Send + Sync>
        };
        let graph = GraphState::disabled().with_publish_hook(hook);

        graph.notify_published(0, true);
        assert_eq!(seen.load(Ordering::SeqCst), 1, "the request reaches the hook");
        graph.notify_published(0, false);
        assert_eq!(
            seen.load(Ordering::SeqCst),
            2,
            "an unhandled topology refresh is re-raised even though this publish did not change it",
        );
        graph.notify_published(0, false);
        assert_eq!(seen.load(Ordering::SeqCst), 2, "a handled request is not re-raised");
    }

    /// A drift delivered while a build is in flight (`nudge_rebuild` during `Loading`, or while
    /// a reload runs) is recorded, not dropped: the build's publish re-checks and — seeing disk
    /// moved past what the build captured — claims a follow-up reload whose own publish fires
    /// the hook again. Reverting the `pending_nudge` re-claim in `notify_published` leaves the
    /// hook firing only once and this fails.
    #[test]
    fn a_nudge_recorded_during_a_build_reloads_on_publish() {
        use std::time::Instant;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let fired = Arc::new(AtomicUsize::new(0));
        let hook = {
            let fired = Arc::clone(&fired);
            Arc::new(move |_signal: GraphPublishSignal| {
                fired.fetch_add(1, Ordering::SeqCst);
                true
            }) as Arc<dyn Fn(GraphPublishSignal) -> bool + Send + Sync>
        };
        let graph = GraphState::for_workspace(root.to_path_buf()).with_publish_hook(hook);
        // Simulate an initial build that already published (generation 1) with a fingerprint
        // that does NOT match disk, plus a nudge that arrived while that build was in flight.
        {
            let mut inner = lock_recover(&graph.inner);
            inner.status = GraphStatus::Ready { files: 0 };
            inner.published = Some(Published {
                generation: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                stale: false,
                reload: ReloadState::Idle,
            });
        }
        graph.pending_nudge.store(true, Ordering::SeqCst);

        // The publish chain fires the hook once and, seeing the recorded nudge with disk
        // drifted past the faked build, claims a follow-up reload. Pass an explicit unbounded
        // bound (i64::MAX) so the seq bound never gates this test — only the reclaim behavior
        // under test decides how many times the hook fires.
        graph.notify_published(i64::MAX, false);

        let deadline = Instant::now() + Duration::from_secs(30);
        while fired.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            fired.load(Ordering::SeqCst) >= 2,
            "the recorded nudge triggered a follow-up reload whose publish fired the hook again",
        );
    }

    /// `drift_pending` reports a drift the context re-render must wait for: a recorded nudge or
    /// a running reload. A clean published graph reports none.
    #[test]
    fn drift_pending_reflects_recorded_nudge_and_running_reload() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("A.bsl"), "Процедура П() КонецПроцедуры").unwrap();
        let graph = GraphState::for_workspace(dir.path().to_path_buf());
        {
            let mut inner = lock_recover(&graph.inner);
            inner.status = GraphStatus::Ready { files: 0 };
            inner.published = Some(Published {
                generation: 1,
                fingerprint: crate::graph_db::GraphFp { files: 1, topology: 1 },
                stale: false,
                reload: ReloadState::Idle,
            });
        }
        assert!(!graph.drift_pending(), "a clean published graph has no pending drift");

        graph.pending_nudge.store(true, Ordering::SeqCst);
        assert!(graph.drift_pending(), "a recorded nudge is a pending drift");
        graph.pending_nudge.store(false, Ordering::SeqCst);

        lock_recover(&graph.inner).published.as_mut().unwrap().reload = ReloadState::Running;
        assert!(graph.drift_pending(), "a running reload is a pending drift");
    }

    /// Even after the stale boot's pre-claimed catch-up FAILS (`reload=Failed`, so
    /// `drift_pending` no longer holds), the leftover-marks one-shot must stay armed:
    /// the stale snapshot predates the marks' causes, and firing the hook against it
    /// would clear them for good.
    #[test]
    fn leftover_consume_stays_armed_while_published_snapshot_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let fired = Arc::new(AtomicUsize::new(0));
        let hook_fired = Arc::clone(&fired);
        let graph = GraphState::for_workspace(root.to_path_buf()).with_publish_hook(Arc::new(
            move |_signal| {
                hook_fired.fetch_add(1, Ordering::SeqCst);
                true
            },
        ));
        {
            let mut inner = lock_recover(&graph.inner);
            inner.published = Some(Published {
                generation: 7,
                fingerprint: crate::graph_db::GraphFp { files: 1, topology: 1 },
                stale: true,
                reload: ReloadState::Failed("catch-up failed".to_owned()),
            });
            inner.status = GraphStatus::Ready { files: 1 };
        }

        graph.consume_leftover_marks(5);
        assert_eq!(fired.load(Ordering::SeqCst), 0, "no consume against the stale snapshot");
        assert_eq!(
            graph.leftover_bound.load(Ordering::SeqCst),
            5,
            "the one-shot stays armed for the next successful publish"
        );
    }

    /// The single-flight core of the drift nudge: the FIRST claim on a drifted published
    /// graph wins and marks the reload `Running`; a SECOND claim (a storm of xml events
    /// while the build runs) loses, so no extra rebuild is ever queued. Deterministic — no
    /// build thread is spawned, only the claim discipline is exercised.
    #[test]
    fn claim_reload_slot_is_single_flight() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("A.bsl"), "Процедура П() КонецПроцедуры").unwrap();
        let graph = GraphState::for_workspace(dir.path().to_path_buf());
        {
            let mut inner = lock_recover(&graph.inner);
            inner.status = GraphStatus::Ready { files: 0 };
            // fingerprint 0 can never match the real disk scan → a drift is always seen.
            inner.published = Some(Published {
                generation: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                stale: false,
                reload: ReloadState::Idle,
            });
        }
        assert!(graph.claim_reload_slot(), "the first claim wins on drift");
        assert!(!graph.claim_reload_slot(), "a second claim loses while a reload is Running");
    }

    /// A nudge on an unbuilt (`Idle`) graph starts the one initial load without any `graph`
    /// tool call — the search-only user's path. Asserting the outcome and that the status
    /// left `Idle` (a load never returns to `Idle`; it goes `Loading → Ready`/`Failed`).
    #[test]
    fn nudge_rebuild_from_idle_starts_the_initial_load() {
        let dir = tempfile::tempdir().unwrap();
        sample_workspace(dir.path());
        let graph = GraphState::for_workspace(dir.path().to_path_buf());
        assert_eq!(graph.status(), GraphStatus::Idle);

        assert_eq!(graph.nudge_rebuild(), NudgeOutcome::LoadStarted);
        assert_ne!(graph.status(), GraphStatus::Idle, "the nudge scheduled the initial load");
    }

    /// A nudge arriving while a reload is already `Running` schedules nothing (single-flight),
    /// so a storm of xml drift during a build cannot pile up rebuilds. No thread is spawned.
    #[test]
    fn nudge_rebuild_absorbs_a_storm_while_a_reload_runs() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("A.bsl"), "Процедура П() КонецПроцедуры").unwrap();
        let graph = GraphState::for_workspace(dir.path().to_path_buf());
        {
            let mut inner = lock_recover(&graph.inner);
            inner.status = GraphStatus::Ready { files: 0 };
            inner.published = Some(Published {
                generation: 1,
                fingerprint: crate::graph_db::GraphFp::default(),
                stale: false,
                reload: ReloadState::Running,
            });
        }
        assert_eq!(graph.nudge_rebuild(), NudgeOutcome::NoOp);
        assert_eq!(graph.nudge_rebuild(), NudgeOutcome::NoOp);
    }
}
