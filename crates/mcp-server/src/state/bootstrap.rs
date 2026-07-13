use super::types::{
    OverlayInit, OverlayWarmupState, PendingEmbed, SemanticRuntimeStatus, SharedSearchEngine,
    WorkspaceSearchInit, WorkspaceSearchMode,
};
use super::MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY;
use crate::baseline::{
    BaselineBootstrap, BaselineRuntime, DeferredBaselineRuntime, ExternalBaselineService,
};
use crate::change_hub::WorkspaceChangeHub;
use crate::diagnostics_state::DiagnosticsState;
use crate::graph::GraphState;
use bsl_platform::PlatformDataInner;
use bsl_search::{BaselineHashMode, CorpusId, Document, IndexProgress, SearchEngine};
use onec_client::Client as OnecClient;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{
    env,
    path::{Path, PathBuf},
};

/// How long a search-init thread waits for the deferred baseline connect before
/// proceeding degraded. Generous by design: the wait sits on a background thread and
/// only unusually slow networks ever reach it; the connect itself typically lands in
/// seconds and wakes the waiter through the slot's condvar immediately.
const BASELINE_CONNECT_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Clone)]
pub struct SharedState {
    workspace_root: Option<PathBuf>,
    /// The configuration root (the `Configuration.xml`-bearing directory, e.g. `src/cf`),
    /// which may be nested under `workspace_root`. File-tree lookups such as
    /// `metadata(form)` resolve object directories relative to THIS root, not the repo root.
    source_root: Option<PathBuf>,
    onec_client: Option<OnecClient>,
    debug_session: Arc<Mutex<Option<bsl_debug::session::DebugSession>>>,
    search_engine: SharedSearchEngine,
    index_progress: Arc<IndexProgress>,
    semantic_runtime: Arc<Mutex<SemanticRuntimeStatus>>,
    /// Outcome of the startup overlay warmup, so `search status` can distinguish "no local
    /// diffs" from "warmup failed" instead of leaving a bare `Ready` ambiguous.
    overlay_warmup: Arc<Mutex<OverlayWarmupState>>,
    workspace_search_mode: WorkspaceSearchMode,
    /// Baseline runtime behind its connect lifecycle: the PG source is built on a
    /// background thread, so construction (and thus the MCP `initialize` handshake)
    /// never waits on the network. Readers see an explicit pending state meanwhile.
    baseline: DeferredBaselineRuntime,
    graph: GraphState,
    diagnostics: DiagnosticsState,
    /// Daemon-owned filesystem change hub. Created before any consumer subscribes
    /// so its lifecycle is independent of the search engine's (which starts later,
    /// in a background init thread). `None` for the reference/shared profiles,
    /// which have no workspace tree to watch. Held so additional sinks (diagnostics
    /// drain-on-read, graph invalidation) can subscribe once they land; the search
    /// sink already runs off a clone taken at construction.
    #[allow(dead_code)]
    change_hub: Option<WorkspaceChangeHub>,
    /// Number of background index/embedding tasks currently in flight. The broker
    /// backend keeps itself alive while this is non-zero so it never idle-exits (and
    /// kills) a long embedding pass. Incremented at the start of each such task and
    /// decremented by [`BackgroundWorkGuard`] on every exit path — including early `?`
    /// returns and panics — so it can never get stuck above zero.
    background_indexers: Arc<AtomicUsize>,
    /// The ONE embed single-flight shared by the boot pass and the post-refresh re-embed kick,
    /// held here so `init_search` reuses the same flight the publish hook does — otherwise the
    /// two could race an index swap and last-writer-wins would install a stale index.
    embed_flight: Arc<EmbedFlight>,
}

/// RAII counter for in-flight background indexing. Increments on construction and
/// decrements on drop (including unwind), so a panicking or early-returning indexing
/// task always releases its hold and the broker's liveness signal returns to idle.
struct BackgroundWorkGuard(Arc<AtomicUsize>);

impl BackgroundWorkGuard {
    fn new(counter: &Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(counter))
    }
}

impl Drop for BackgroundWorkGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The ONE embed single-flight for the whole workspace. Both the boot pass (fills the initial
/// NULL embeddings after a fused cold build) and the post-context-refresh re-embed kick funnel
/// through it, so an older pass can never install a vector index over a newer one
/// (last-writer-wins). A pass that loses the claim records `rerun_pending`; the winning owner
/// loops while that flag is set, and both the "record a rerun" and the "release the claim"
/// decisions happen under the same mutex — so a rerun request can never be lost between the
/// owner deciding to stop and a late caller signalling more work.
struct EmbedFlight {
    state: Mutex<EmbedFlightState>,
}

#[derive(Default)]
struct EmbedFlightState {
    in_flight: bool,
    rerun_pending: bool,
}

impl EmbedFlight {
    fn new() -> Arc<Self> {
        Arc::new(Self { state: Mutex::new(EmbedFlightState::default()) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, EmbedFlightState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Try to claim the flight. `true` = THIS caller won and must run the pass; `false` = a
    /// pass is already running and a rerun was recorded so it loops again for this caller's
    /// (later-NULLed) chunks.
    fn claim(&self) -> bool {
        let mut st = self.lock();
        if st.in_flight {
            st.rerun_pending = true;
            false
        } else {
            st.in_flight = true;
            true
        }
    }

    /// Start of a pass iteration: clear the rerun flag so a request arriving DURING this
    /// iteration triggers another loop rather than being swallowed.
    fn begin_pass(&self) {
        self.lock().rerun_pending = false;
    }

    /// End of a pass iteration. `true` = a rerun was requested (keep the claim, loop again);
    /// `false` = none, so the claim is released under the same lock (no wakeup can be lost).
    fn finish_pass(&self) -> bool {
        let mut st = self.lock();
        if st.rerun_pending {
            true
        } else {
            st.in_flight = false;
            false
        }
    }

    /// Force-release the claim on an abnormal exit (panic / embed error). A leftover rerun
    /// request is harmless — the next owner clears it in `begin_pass` and runs anyway.
    fn release(&self) {
        self.lock().in_flight = false;
    }

    #[cfg(test)]
    fn is_in_flight(&self) -> bool {
        self.lock().in_flight
    }

    #[cfg(test)]
    fn in_flight_for_test() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(EmbedFlightState { in_flight: true, rerun_pending: false }),
        })
    }
}

/// RAII release of the shared embed claim on an abnormal exit (panic / early return) while the
/// owner still holds it, so a crashed pass never strands the flight `in_flight`. A clean exit
/// calls [`Self::disarm`] first (the owner released the claim itself under the flight lock), so
/// this does not stomp a later owner that already re-claimed.
struct EmbedClaimGuard {
    flight: Arc<EmbedFlight>,
    armed: bool,
}

impl EmbedClaimGuard {
    fn new(flight: Arc<EmbedFlight>) -> Self {
        Self { flight, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for EmbedClaimGuard {
    fn drop(&mut self) {
        if self.armed {
            self.flight.release();
        }
    }
}

/// RAII restoration of the semantic runtime status for a background embed pass. The pass sets
/// `Indexing` before it starts; this guarantees the status leaves `Indexing` even if the pass
/// panics or returns early without an explicit terminal transition — otherwise a crashed pass
/// would strand the runtime at `Indexing` forever. An explicit [`Self::finish`] on a clean
/// success/failure suppresses the fallback.
struct EmbedStatusGuard {
    runtime: Arc<Mutex<SemanticRuntimeStatus>>,
    finished: bool,
}

impl EmbedStatusGuard {
    fn new(runtime: Arc<Mutex<SemanticRuntimeStatus>>) -> Self {
        Self { runtime, finished: false }
    }

    fn finish(&mut self) {
        self.finished = true;
    }
}

impl Drop for EmbedStatusGuard {
    fn drop(&mut self) {
        if !self.finished {
            SharedState::set_semantic_runtime_status(
                &self.runtime,
                SemanticRuntimeStatus::Failed("embedding pass ended without completing".to_owned()),
            );
        }
    }
}

/// Test seam: force the embed pass body to panic after its guards are in place, to verify the
/// guards restore the flight claim and the runtime status (never leaving it stuck `Indexing`).
#[cfg(test)]
static FORCE_EMBED_PASS_PANIC: AtomicBool = AtomicBool::new(false);

/// Test seam: a callback invoked once after the first embed iteration installs its index (and
/// before `finish_pass`), so a test can create a NULL chunk mid-flight and signal a rerun,
/// proving the owner loops and embeds it. Receives the store DB path.
#[cfg(test)]
type EmbedPostPassHook = Box<dyn FnMut(&Path) + Send>;
#[cfg(test)]
static EMBED_POST_PASS_HOOK: Mutex<Option<EmbedPostPassHook>> = Mutex::new(None);

/// Test seam: force a reconcile walk (the overflow rescan and the boot store reconcile) to count as
/// errored, so a test can assert the reconcile is skipped (a partial walk must never be treated as
/// authoritative and delete healthy files) — and, at boot, that a Clean init downgrades to a prime.
#[cfg(test)]
static FORCE_REWALK_WALK_ERROR: AtomicBool = AtomicBool::new(false);

impl SharedState {
    pub fn workspace(source_dir: PathBuf) -> Self {
        let project = project_model::Project::new(&source_dir);
        let config_path = project.source_path();
        let source_root = config_path.to_path_buf();

        let search_engine: SharedSearchEngine = Arc::new(Mutex::new(None));
        let index_progress = IndexProgress::new();
        let semantic_runtime = Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled));
        let overlay_warmup = Arc::new(Mutex::new(OverlayWarmupState::Pending));
        let background_indexers = Arc::new(AtomicUsize::new(0));
        let watcher_ready = Arc::new(AtomicBool::new(false));
        // Only the cheap, local part of baseline resolution runs here (config, env,
        // credential helper); the PG connect itself is deferred to a background thread
        // so a slow or unreachable server never delays the daemon's socket. The search
        // mode is therefore decided by configured INTENT (credentials resolved for a
        // postgres workspace baseline), not by connect success: a PG outage keeps the
        // workspace in Postgres mode with a visible issue instead of silently falling
        // back to (re)building a local index of the whole configuration.
        let bootstrap = BaselineRuntime::workspace_bootstrap(Some(&project.root), &project.config);
        let workspace_search_mode = Self::workspace_mode_for(&bootstrap);
        let baseline = match bootstrap {
            BaselineBootstrap::Immediate(runtime) => DeferredBaselineRuntime::ready(runtime),
            BaselineBootstrap::Connect(plan) => DeferredBaselineRuntime::spawn(*plan),
        };

        // The change hub owns the recursive workspace watcher and starts before any
        // consumer subscribes: the search engine is built on a background thread and must
        // not gate the watcher's lifecycle. It watches the whole drift-scan universe — the
        // config source root plus every extension root — so diagnostics/graph drift in
        // extensions is event-delivered, not left to the reconciler. Search subscribes as a
        // sink and preserves its prior behavior (mark only source-root `.bsl` paths dirty).
        let mut watch_roots = vec![config_path.to_path_buf()];
        watch_roots.extend(project.extension_paths().iter().map(|(_, path)| path.clone()));
        let change_hub = WorkspaceChangeHub::start(watch_roots);

        // Created before the search-init thread so it can own the workspace graph: for
        // a local SQLite workspace the search-init drives a single fused parse pass
        // that builds the graph AND the search index, then publishes the graph through
        // this handle. A clone (cheap, shared `Arc`s) goes to the search thread; this
        // copy stays in `SharedState` for graph-tool serving and drift/reload. It carries
        // the hub so a graph freshness check invalidates its fingerprint cache on delivery.
        // On each graph publish/adopt (on the graph's own background thread) re-render the
        // search chunks marked context-dirty by an `.xml` drift, now that the graph has
        // caught up. Captures only shared handles; the closure never runs on a query path.
        // ONE embed single-flight shared by the boot pass and the post-refresh re-embed kick,
        // so overlapping passes collapse into one and the installed index is always built from
        // the latest store state.
        let embed_flight = EmbedFlight::new();
        let publish_hook = Self::build_publish_hook(
            Arc::clone(&search_engine),
            source_dir.clone(),
            Arc::clone(&semantic_runtime),
            Arc::clone(&background_indexers),
            Arc::clone(&index_progress),
            Arc::clone(&embed_flight),
        );
        let graph = GraphState::for_workspace(source_dir.clone())
            .with_change_hub(change_hub.clone())
            .with_publish_hook(publish_hook);

        // The `metadata` tool reads the resident diagnostics host (per-MDO substrate for
        // `object`, Channel-2 `load_configuration` for `tree`/`info`); it is seeded and
        // kept fresh by the resident's own drift poll, so no separate configuration
        // snapshot is loaded here. The same resident serves the search overlay's incremental
        // reindex through the snapshot-source adapter.
        let diagnostics =
            DiagnosticsState::for_workspace(source_dir.clone()).with_change_hub(change_hub.clone());
        let snapshot_source: Arc<dyn bsl_search::ModuleSnapshotSource> = Arc::new(
            crate::diagnostics_state::ResidentModuleSnapshotSource::new(diagnostics.clone()),
        );

        Self::spawn_workspace_search_init(
            Arc::clone(&search_engine),
            Arc::clone(&index_progress),
            Arc::clone(&semantic_runtime),
            Arc::clone(&overlay_warmup),
            Arc::clone(&background_indexers),
            source_dir.clone(),
            Arc::clone(&watcher_ready),
            baseline.clone(),
            workspace_search_mode.clone(),
            graph.clone(),
            Arc::clone(&embed_flight),
            Arc::clone(&snapshot_source),
        );

        Self::spawn_search_sink(
            change_hub.clone(),
            Arc::clone(&search_engine),
            Arc::clone(&watcher_ready),
            config_path.to_path_buf(),
            graph.clone(),
        );

        Self {
            workspace_root: Some(source_dir),
            source_root: Some(source_root),
            onec_client: None,
            debug_session: Arc::new(Mutex::new(None)),
            search_engine,
            index_progress,
            semantic_runtime,
            overlay_warmup,
            workspace_search_mode,
            baseline,
            graph,
            diagnostics,
            change_hub: Some(change_hub),
            background_indexers,
            embed_flight,
        }
    }

    /// The production publish hook: after a graph publish it re-renders the search chunks
    /// marked context-dirty by an `.xml` drift, then re-embeds them. Extracted so a test can
    /// wire the SAME closure the daemon does rather than calling the refresh by hand. The
    /// hook receives `(drift_pending, build_start_seq)`: `build_start_seq` bounds which marks
    /// the refresh may clear (only drifts this build already reflects), while `drift_pending`
    /// is a fast-path hint to skip a round when a fresher reload is imminent.
    fn build_publish_hook(
        search_engine: SharedSearchEngine,
        workspace_root: PathBuf,
        semantic_runtime: Arc<Mutex<SemanticRuntimeStatus>>,
        background_indexers: Arc<AtomicUsize>,
        index_progress: Arc<IndexProgress>,
        embed_flight: Arc<EmbedFlight>,
    ) -> Arc<dyn Fn(crate::graph::GraphPublishSignal) + Send + Sync> {
        Arc::new(move |signal| {
            Self::refresh_search_contexts_after_graph(
                &search_engine,
                &workspace_root,
                &semantic_runtime,
                &background_indexers,
                &index_progress,
                &embed_flight,
                signal,
            );
        })
    }

    // Each argument is a distinct shared handle the spawned init thread must own (engine,
    // progress, runtime status, indexer counter, roots, baseline, graph, embed flight).
    // Bundling them into a context struct would only move the same fields behind one name
    // without clarifying anything, so the small over-arity is accepted here.
    #[allow(clippy::too_many_arguments)]
    fn spawn_workspace_search_init(
        search_engine: SharedSearchEngine,
        index_progress: Arc<IndexProgress>,
        semantic_runtime: Arc<Mutex<SemanticRuntimeStatus>>,
        overlay_warmup: Arc<Mutex<OverlayWarmupState>>,
        background_indexers: Arc<AtomicUsize>,
        workspace_root: PathBuf,
        watcher_ready: Arc<AtomicBool>,
        baseline: DeferredBaselineRuntime,
        mode: WorkspaceSearchMode,
        graph: GraphState,
        embed_flight: Arc<EmbedFlight>,
        snapshot_source: Arc<dyn bsl_search::ModuleSnapshotSource>,
    ) {
        std::thread::Builder::new()
            .name("bsl-search-init".to_owned())
            .spawn(move || {
                // Held for the whole init (incl. a multi-minute fused cold build) so the
                // broker stays alive even if the launching client disconnects mid-build.
                let _init_guard = BackgroundWorkGuard::new(&background_indexers);
                tracing::info!("search engine initialization started in background");
                // Postgres mode needs the baseline connect's outcome before it can load
                // the manifest; waiting HERE keeps the wait on this background thread
                // (never a request path) and off the slot's lock. On timeout the init
                // proceeds without a service and fails exactly like today's PG-error
                // path — offline with a visible issue, never a local reindex.
                let external_baseline = match mode {
                    WorkspaceSearchMode::PostgresRemoteOverlay => {
                        if !baseline.wait_ready(BASELINE_CONNECT_WAIT) {
                            tracing::warn!(
                                timeout_secs = BASELINE_CONNECT_WAIT.as_secs(),
                                "baseline connect still pending; workspace search init proceeds degraded"
                            );
                        }
                        baseline.external()
                    }
                    WorkspaceSearchMode::SqliteLocal => None,
                };
                let init = Self::init_workspace_search_engine(
                    &workspace_root,
                    &watcher_ready,
                    mode,
                    external_baseline,
                    &graph,
                );

                let Some(mut init) = init else {
                    Self::set_semantic_runtime_status(
                        &semantic_runtime,
                        SemanticRuntimeStatus::Failed(
                            "workspace search engine initialization failed".to_owned(),
                        ),
                    );
                    tracing::warn!("workspace search engine initialization failed");
                    return;
                };

                let pending_embed = init.pending_embed.take();
                let needs_overlay_warmup =
                    matches!(init.mode, WorkspaceSearchMode::PostgresRemoteOverlay);

                // When the fused build deferred embeddings, mark the runtime `Indexing`
                // BEFORE the engine becomes visible. The published engine still has an
                // empty vector index; without this ordering a concurrent semantic query
                // could reach `engine.search` on that empty index and return a silent
                // zero instead of degrading to lexical.
                let status_after_publish = match &pending_embed {
                    Some(_) => {
                        Self::set_semantic_runtime_status(
                            &semantic_runtime,
                            SemanticRuntimeStatus::Indexing,
                        );
                        None
                    }
                    None => Some(Self::semantic_runtime_status_for_mode(&init.engine, &init.mode)),
                };

                // Wire the graph's mark-seq source to the store's counter now that the engine
                // exists (the graph was built before it). From here a build captures a bounded
                // `build_start_seq`; publishes before this point (the fused/cached boot build)
                // ran with the unwired `0` bound and cleared nothing.
                graph.set_mark_seq_source(init.engine.mark_seq_handle());

                // `context_dirty` persists across restarts, so a prior run may have left marks
                // the boot build's (unwired) publish did not clear. Capture whether any survive
                // AND the mark-seq high-water bound at THIS instant — both read off the engine
                // BEFORE it moves into the shared handle. The captured bound is what the leftover
                // consume clears against: it predates any drift the now-running search sink may
                // stamp after this point, so those newer marks (higher seq) are excluded and left
                // to their own nudge→publish cycle rather than cleared against the boot graph. The
                // consume itself runs AFTER the move so the publish hook can read the published
                // engine.
                let has_leftover_marks =
                    init.engine.context_dirty_paths("code").map(|m| !m.is_empty()).unwrap_or(false);
                let leftover_bound = init.engine.mark_seq_handle().load(Ordering::SeqCst);

                // Wire the resident snapshot source so the overlay reindex can read text+parse
                // from the shared resident host. Set before publish so the first query already
                // sees it; the resident read itself is prefetched off the engine lock by
                // `prefetch_resident_overlay`, so the two locks never nest.
                init.engine.set_module_snapshot_source(snapshot_source);

                // Bring the workspace overlay online BEFORE publishing. The overlay is inert until
                // initialized (`reindex_dirty_from_snapshots` no-ops on `!initialized`), so the
                // resident-fed incremental reindex — and overlay edit-freshness generally — is
                // unreachable in local SQLite mode without this. Done here, on the still-owned
                // engine, so it holds NO engine lock: `Prime`'s disk scan must not serialize behind
                // the shared lock (I3), and the cold FTS branch already indexes disk before
                // publishing, so a warm prime delays publish no differently.
                match init.overlay_init {
                    OverlayInit::Clean => {
                        if let Err(e) = init.engine.initialize_workspace_overlay_clean() {
                            tracing::warn!("workspace overlay clean-init failed: {e}");
                        }
                    }
                    OverlayInit::Prime => {
                        if let Err(e) = init.engine.prime_workspace_overlay() {
                            tracing::warn!("workspace overlay prime failed: {e}");
                        }
                    }
                    OverlayInit::RemoteWarmup => {}
                }

                if let Ok(mut guard) = search_engine.lock() {
                    *guard = Some(init.engine);
                }

                if has_leftover_marks {
                    graph.consume_leftover_marks(leftover_bound);
                }

                if let Some(status) = status_after_publish {
                    Self::set_semantic_runtime_status(&semantic_runtime, status);
                }

                tracing::info!("search engine initialization complete");

                if let Some(pending) = pending_embed {
                    // The boot pass shares the ONE embed single-flight with the post-refresh
                    // kick, so a kick that lands while boot runs is absorbed (its NULL chunks
                    // picked up by boot's rerun loop) instead of racing a second index swap.
                    Self::spawn_embed_pass(
                        Arc::clone(&search_engine),
                        Arc::clone(&semantic_runtime),
                        Arc::clone(&background_indexers),
                        Arc::clone(&index_progress),
                        Arc::clone(&embed_flight),
                        pending.db_path,
                        pending.config,
                    );
                }

                if needs_overlay_warmup {
                    Self::set_semantic_runtime_status(
                        &semantic_runtime,
                        SemanticRuntimeStatus::OverlaySyncing,
                    );
                    let search_engine = Arc::clone(&search_engine);
                    let semantic_runtime = Arc::clone(&semantic_runtime);
                    let overlay_warmup = Arc::clone(&overlay_warmup);
                    let warmup_guard = BackgroundWorkGuard::new(&background_indexers);
                    std::thread::Builder::new()
                        .name("bsl-search-overlay-warmup".to_owned())
                        .spawn(move || {
                            let _warmup_guard = warmup_guard;
                            tracing::info!("workspace overlay semantic warmup started");
                            Self::run_overlay_warmup(&search_engine, &overlay_warmup);
                            // Semantic stays available via the baseline even when the overlay
                            // warmup failed; the detailed warmup outcome lives in `overlay_warmup`.
                            Self::set_semantic_runtime_status(
                                &semantic_runtime,
                                SemanticRuntimeStatus::Ready,
                            );
                        })
                        .ok();
                }
            })
            .ok();
    }

    pub fn reference(project_root: Option<PathBuf>) -> Self {
        let search_engine: SharedSearchEngine = Arc::new(Mutex::new(None));
        let index_progress = IndexProgress::new();
        let semantic_runtime = Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled));
        let background_indexers = Arc::new(AtomicUsize::new(0));
        let project_config = project_root.as_deref().and_then(project_model::ProjectConfig::load);
        // Same deferral as the workspace profile: the PG/Vault connect runs off-thread
        // so a reference daemon's socket comes up immediately.
        let baseline = match BaselineRuntime::reference_bootstrap(project_config.as_ref()) {
            BaselineBootstrap::Immediate(runtime) => DeferredBaselineRuntime::ready(runtime),
            BaselineBootstrap::Connect(plan) => DeferredBaselineRuntime::spawn(*plan),
        };

        {
            let engine_arc = Arc::clone(&search_engine);
            let progress_arc = Arc::clone(&index_progress);
            let semantic_runtime_arc = Arc::clone(&semantic_runtime);
            let baseline = baseline.clone();
            let init_guard = BackgroundWorkGuard::new(&background_indexers);
            std::thread::Builder::new()
                .name("bsl-search-reference-init".to_owned())
                .spawn(move || {
                    let _init_guard = init_guard;
                    tracing::info!("reference search engine initialization started in background");
                    // Wait for the deferred connect before deciding shared-vs-local:
                    // a still-pending baseline read as `None` would rebuild the local
                    // platform docs cache instead of serving the shared snapshot.
                    if !baseline.wait_ready(BASELINE_CONNECT_WAIT) {
                        tracing::warn!(
                            timeout_secs = BASELINE_CONNECT_WAIT.as_secs(),
                            "baseline connect still pending; reference search init proceeds degraded"
                        );
                    }
                    let engine =
                        Self::init_reference_search_engine(&progress_arc, baseline.external());
                    let semantic_status = engine
                        .as_ref()
                        .map(|engine| {
                            Self::semantic_runtime_status_for_mode(
                                engine,
                                &WorkspaceSearchMode::SqliteLocal,
                            )
                        })
                        .unwrap_or_else(|| {
                            SemanticRuntimeStatus::Failed(
                                "reference search engine initialization failed".to_owned(),
                            )
                        });
                    if let Ok(mut guard) = engine_arc.lock() {
                        *guard = engine;
                    }
                    Self::set_semantic_runtime_status(&semantic_runtime_arc, semantic_status);
                    tracing::info!("reference search engine initialization complete");
                })
                .ok();
        }

        Self {
            workspace_root: None,
            source_root: None,
            onec_client: None,
            debug_session: Arc::new(Mutex::new(None)),
            search_engine,
            index_progress,
            semantic_runtime,
            overlay_warmup: Arc::new(Mutex::new(OverlayWarmupState::Pending)),
            workspace_search_mode: WorkspaceSearchMode::SqliteLocal,
            baseline,
            graph: GraphState::disabled(),
            diagnostics: DiagnosticsState::disabled(),
            change_hub: None,
            background_indexers,
            embed_flight: EmbedFlight::new(),
        }
    }

    pub fn shared() -> Self {
        Self {
            workspace_root: None,
            source_root: None,
            onec_client: None,
            debug_session: Arc::new(Mutex::new(None)),
            search_engine: Arc::new(Mutex::new(None)),
            index_progress: IndexProgress::new(),
            semantic_runtime: Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            overlay_warmup: Arc::new(Mutex::new(OverlayWarmupState::Pending)),
            workspace_search_mode: WorkspaceSearchMode::SqliteLocal,
            baseline: DeferredBaselineRuntime::absent(),
            graph: GraphState::disabled(),
            diagnostics: DiagnosticsState::disabled(),
            change_hub: None,
            background_indexers: Arc::new(AtomicUsize::new(0)),
            embed_flight: EmbedFlight::new(),
        }
    }

    pub(crate) fn graph(&self) -> &GraphState {
        &self.graph
    }

    /// Start building the diagnostics resident now instead of on the first tool call.
    ///
    /// A serve path calls this right after construction so the resident (seconds of
    /// enumerate + metadata substrate on a large configuration) is ready before the
    /// agent's first `diagnostics` request rather than billed to it. Deliberately not
    /// part of [`Self::workspace`]: state is also constructed by tests and short-lived
    /// commands that never serve diagnostics, and those must not pay for (or race) a
    /// background resident build. No-op without a workspace root.
    pub fn warm_start(&self) {
        self.diagnostics.ensure_loading();
    }

    pub(crate) fn diagnostics(&self) -> &DiagnosticsState {
        &self.diagnostics
    }

    // Consumed by the diagnostics/graph sinks once they subscribe; exposed now so
    // the hub the daemon owns is reachable from the tool layer.
    #[allow(dead_code)]
    pub(crate) fn change_hub(&self) -> Option<&WorkspaceChangeHub> {
        self.change_hub.as_ref()
    }

    pub fn set_onec_client(&mut self, client: OnecClient) {
        self.onec_client = Some(client);
    }

    pub fn onec_client(&self) -> Option<&OnecClient> {
        self.onec_client.as_ref()
    }

    pub fn set_workspace_root(&mut self, root: PathBuf) {
        self.workspace_root = Some(root);
    }

    pub fn workspace_root(&self) -> Option<&PathBuf> {
        self.workspace_root.as_ref()
    }

    /// The configuration root (`Configuration.xml`-bearing directory, e.g. `src/cf`), under
    /// which metadata object directories live. Falls back to `workspace_root` when no nested
    /// configuration root was discovered (a flat layout where the two coincide).
    pub fn source_root(&self) -> Option<&PathBuf> {
        self.source_root.as_ref().or(self.workspace_root.as_ref())
    }

    pub fn debug_session(&self) -> &Arc<Mutex<Option<bsl_debug::session::DebugSession>>> {
        &self.debug_session
    }

    pub fn search_engine(&self) -> &SharedSearchEngine {
        &self.search_engine
    }

    pub fn index_progress(&self) -> &Arc<IndexProgress> {
        &self.index_progress
    }

    /// Whether a long-running background index/embedding task is in flight. The broker
    /// backend uses this so it does not idle-exit (and kill the task) just because no
    /// client is currently connected — the expensive embedding run, which can take far
    /// longer than the idle window, must be allowed to finish so its already-spent work
    /// is not wasted on the next cold start. Backed by a guarded counter that is released
    /// on every task exit path (including panics), so the signal cannot get stuck.
    pub fn background_indexing_active(&self) -> bool {
        self.background_indexers.load(Ordering::SeqCst) > 0
    }

    pub(crate) fn semantic_runtime(&self) -> Arc<Mutex<SemanticRuntimeStatus>> {
        Arc::clone(&self.semantic_runtime)
    }

    pub(crate) fn overlay_warmup(&self) -> Arc<Mutex<OverlayWarmupState>> {
        Arc::clone(&self.overlay_warmup)
    }

    pub(crate) fn workspace_search_mode(&self) -> WorkspaceSearchMode {
        self.workspace_search_mode.clone()
    }

    /// A single-lock snapshot of the baseline lifecycle — the only read surface for
    /// tool handlers. While the deferred connect is `pending`, gates answer "warming —
    /// retry shortly" instead of a config error; one snapshot per request keeps the
    /// pending flag and the runtime pieces describing the same instant.
    pub(crate) fn baseline_view(&self) -> crate::baseline::BaselineView {
        self.baseline.view()
    }

    /// The workspace search mode implied by the baseline bootstrap: configured INTENT,
    /// never connect success. EVERY postgres-configured outcome — a deferred connect
    /// AND the immediate failures (unconfigured section, credential rejection) — stays
    /// in Postgres mode. Mapping a failure to `SqliteLocal` would route `search_code`
    /// into a silent full local reindex of the configuration, hiding exactly the
    /// failure the issue text reports; in Postgres mode the gates surface that issue.
    fn workspace_mode_for(bootstrap: &BaselineBootstrap) -> WorkspaceSearchMode {
        match bootstrap {
            BaselineBootstrap::Connect(plan)
                if matches!(plan.corpus(), CorpusId::WorkspaceCode) =>
            {
                WorkspaceSearchMode::PostgresRemoteOverlay
            }
            BaselineBootstrap::Immediate(runtime)
                if runtime.configured_baseline.backend == "postgres" =>
            {
                WorkspaceSearchMode::PostgresRemoteOverlay
            }
            _ => WorkspaceSearchMode::SqliteLocal,
        }
    }

    pub fn shutdown(&self) {
        self.baseline.shutdown();
        self.diagnostics.shutdown();
    }

    fn embedding_config() -> Option<bsl_search::SearchConfig> {
        let base_url = std::env::var("EMBEDDING_URL").ok()?;
        // The model must be declared explicitly: a wrong default would silently mix
        // vectors from different models into one index. Unset means FTS-only.
        let model = std::env::var("EMBEDDING_MODEL").ok()?;
        let dim: usize =
            std::env::var("EMBEDDING_DIM").ok().and_then(|s| s.parse().ok()).unwrap_or(1024);
        // Background index/embedding workers otherwise saturate every core and starve interactive
        // `search_code` for tens of seconds during the one-time build. Default to leaving two cores
        // free for queries; an explicit EMBEDDING_CONCURRENCY still wins (operators who want max
        // build throughput set it).
        let concurrency: usize = std::env::var("EMBEDDING_CONCURRENCY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|n| n.get().saturating_sub(2).max(2))
                    .unwrap_or(4)
            });

        Some(bsl_search::SearchConfig {
            embedder: bsl_search::EmbedderConfig {
                base_url,
                model,
                dim: Some(dim),
                api_key: std::env::var("EMBEDDING_API_KEY").ok(),
                provider: std::env::var("EMBEDDING_PROVIDER").ok(),
            },
            execution: bsl_search::EmbeddingExecutionPolicy {
                batch_size: std::env::var("EMBEDDING_BATCH_SIZE")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(32),
                concurrency,
                progress_interval: std::env::var("EMBEDDING_PROGRESS_INTERVAL")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(20),
            },
        })
    }

    fn open_semantic_search_engine(
        db_path: &Path,
        config: bsl_search::SearchConfig,
    ) -> Option<SearchEngine> {
        let model = config.embedder.model.clone();
        match SearchEngine::new(db_path, config) {
            Ok(engine) => {
                tracing::info!(
                    files = engine.file_count().unwrap_or(0),
                    chunks = engine.chunk_count().unwrap_or(0),
                    vectors = engine.vector_count(),
                    model,
                    "search engine loaded (FTS + semantic)"
                );
                Some(engine)
            }
            Err(e) => {
                tracing::warn!("failed to init search engine with embedder: {e}");
                None
            }
        }
    }

    fn open_fts_only_search_engine(db_path: &Path) -> Option<SearchEngine> {
        match SearchEngine::fts_only(db_path) {
            Ok(engine) => {
                tracing::info!(
                    files = engine.file_count().unwrap_or(0),
                    chunks = engine.chunk_count().unwrap_or(0),
                    "search engine loaded (FTS-only)"
                );
                Some(engine)
            }
            Err(e) => {
                tracing::warn!("failed to init FTS-only search engine: {e}");
                None
            }
        }
    }

    fn open_search_engine(db_path: &Path) -> Option<SearchEngine> {
        if let Some(config) = Self::embedding_config() {
            return Self::open_semantic_search_engine(db_path, config);
        }
        Self::open_fts_only_search_engine(db_path)
    }

    fn open_workspace_overlay_search_engine(db_path: &Path) -> Option<SearchEngine> {
        if let Some(config) = Self::embedding_config() {
            let model = config.embedder.model.clone();
            return match SearchEngine::semantic_overlay_only(db_path, config) {
                Ok(engine) => {
                    tracing::info!(
                        files = engine.file_count().unwrap_or(0),
                        chunks = engine.chunk_count().unwrap_or(0),
                        vectors = engine.vector_count(),
                        model,
                        "workspace overlay engine loaded (remote baseline + local overlay semantic)"
                    );
                    Some(engine)
                }
                Err(e) => {
                    tracing::warn!("failed to init overlay-only semantic search engine: {e}");
                    None
                }
            };
        }
        Self::open_fts_only_search_engine(db_path)
    }

    fn configure_workspace_engine(
        engine: &mut SearchEngine,
        workspace_source_root: &Path,
        watcher_ready: &AtomicBool,
        hash_mode: BaselineHashMode,
    ) {
        engine.set_workspace_root(workspace_source_root.to_path_buf());
        engine.set_workspace_baseline_hash_mode(hash_mode);
        if watcher_ready.load(Ordering::SeqCst) {
            engine.enable_workspace_watcher_mode();
        }
    }

    /// Drive the PostgresRemoteOverlay warmup without ever holding the engine lock across the
    /// multi-minute remote embed. Three phases: a brief lock to clone what the standalone prime
    /// needs (db path, embedder config, workspace root, warm embedding cache, graph provider); a
    /// lock-free plan+embed against a reopened standalone store; a brief lock to publish the
    /// merged result atomically. While the embed runs, concurrent `search_code` / `search_status`
    /// keep the lock free.
    fn run_overlay_warmup(
        search_engine: &SharedSearchEngine,
        overlay_warmup: &Arc<Mutex<OverlayWarmupState>>,
    ) {
        let cloned = match search_engine.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(engine) => {
                    let Some(embedder_config) = engine.embedder_config() else {
                        tracing::debug!("overlay warmup: no embedder configured; skipping");
                        Self::set_overlay_warmup_state(
                            overlay_warmup,
                            OverlayWarmupState::Skipped("no embedder configured".to_owned()),
                        );
                        return;
                    };
                    let Some(workspace_root) = engine.workspace_root().map(Path::to_path_buf)
                    else {
                        tracing::debug!("overlay warmup: no workspace root; skipping");
                        Self::set_overlay_warmup_state(
                            overlay_warmup,
                            OverlayWarmupState::Skipped("no workspace root".to_owned()),
                        );
                        return;
                    };
                    let warm_cache = match engine.workspace_overlay_embedding_cache_snapshot() {
                        Ok(cache) => cache,
                        Err(error) => {
                            tracing::warn!(
                                "overlay warmup: failed to snapshot warm cache: {error}"
                            );
                            Self::set_overlay_warmup_state(
                                overlay_warmup,
                                OverlayWarmupState::Failed(error.to_string()),
                            );
                            return;
                        }
                    };
                    // Captured here, under the same lock as the warm cache and before the lock-free
                    // embed: the publish below clears only these, so a watcher edit landing mid-embed
                    // stays dirty and is re-embedded by a later refresh instead of being lost.
                    let dirty_before = match engine.workspace_overlay_dirty_paths_snapshot() {
                        Ok(dirty) => dirty,
                        Err(error) => {
                            tracing::warn!(
                                "overlay warmup: failed to snapshot dirty paths: {error}"
                            );
                            Self::set_overlay_warmup_state(
                                overlay_warmup,
                                OverlayWarmupState::Failed(error.to_string()),
                            );
                            return;
                        }
                    };
                    Some((
                        engine.db_path().to_path_buf(),
                        embedder_config,
                        workspace_root,
                        warm_cache,
                        engine.graph_context_provider(),
                        dirty_before,
                    ))
                }
                None => None,
            },
            Err(e) => {
                tracing::warn!("overlay warmup: engine lock error: {e}");
                Self::set_overlay_warmup_state(
                    overlay_warmup,
                    OverlayWarmupState::Failed(format!("engine lock error: {e}")),
                );
                return;
            }
        };
        let Some((
            db_path,
            embedder_config,
            workspace_root,
            warm_cache,
            graph_provider,
            dirty_before,
        )) = cloned
        else {
            // Engine was published earlier but is gone now (e.g. shutdown raced the warmup).
            Self::set_overlay_warmup_state(
                overlay_warmup,
                OverlayWarmupState::Skipped("engine unavailable".to_owned()),
            );
            return;
        };

        // Lock-free: plan against a reopened standalone store and embed the missing chunks. The
        // engine mutex is NOT held here, so search/status stay responsive during the remote embed.
        let primed = SearchEngine::prime_workspace_overlay_standalone(
            &db_path,
            embedder_config,
            &workspace_root,
            warm_cache,
            graph_provider,
        );
        let (plan, new_embeddings) = match primed {
            Ok(result) => result,
            Err(error) => {
                tracing::warn!("workspace overlay semantic warmup failed: {error}");
                Self::set_overlay_warmup_state(
                    overlay_warmup,
                    OverlayWarmupState::Failed(error.to_string()),
                );
                return;
            }
        };

        // Capture plan stats BEFORE `plan`/`new_embeddings` are consumed by the publish below, so
        // the warmup outcome can report how many local files were embedded (and how many chunks).
        let plan_empty = plan.is_empty();
        let overlay_files = plan.overlay_file_count();
        let embedded = new_embeddings.len();

        // Brief lock to publish the merged result atomically.
        match search_engine.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(engine) => {
                    match engine.publish_workspace_overlay(plan, new_embeddings, &dirty_before) {
                        Ok(()) => {
                            tracing::info!("workspace overlay semantic warmup complete");
                            let outcome = if plan_empty {
                                OverlayWarmupState::NoLocalDiffs
                            } else {
                                OverlayWarmupState::Synced { overlay_files, embedded }
                            };
                            Self::set_overlay_warmup_state(overlay_warmup, outcome);
                        }
                        Err(error) => {
                            tracing::warn!("overlay warmup: publish failed: {error}");
                            Self::set_overlay_warmup_state(
                                overlay_warmup,
                                OverlayWarmupState::Failed(error.to_string()),
                            );
                        }
                    }
                }
                None => {
                    tracing::warn!("overlay warmup: engine gone before publish");
                    Self::set_overlay_warmup_state(
                        overlay_warmup,
                        OverlayWarmupState::Skipped("engine unavailable".to_owned()),
                    );
                }
            },
            Err(e) => {
                tracing::warn!("overlay warmup: engine lock error at publish: {e}");
                Self::set_overlay_warmup_state(
                    overlay_warmup,
                    OverlayWarmupState::Failed(format!("engine lock error: {e}")),
                );
            }
        }
    }

    fn set_overlay_warmup_state(
        overlay_warmup: &Arc<Mutex<OverlayWarmupState>>,
        state: OverlayWarmupState,
    ) {
        if let Ok(mut guard) = overlay_warmup.lock() {
            *guard = state;
        }
    }

    fn set_semantic_runtime_status(
        semantic_runtime: &Arc<Mutex<SemanticRuntimeStatus>>,
        status: SemanticRuntimeStatus,
    ) {
        if let Ok(mut guard) = semantic_runtime.lock() {
            *guard = status;
        }
    }

    fn semantic_runtime_status_for_mode(
        engine: &SearchEngine,
        mode: &WorkspaceSearchMode,
    ) -> SemanticRuntimeStatus {
        match mode {
            WorkspaceSearchMode::SqliteLocal | WorkspaceSearchMode::PostgresRemoteOverlay => {
                if engine.has_semantic() {
                    SemanticRuntimeStatus::Ready
                } else {
                    SemanticRuntimeStatus::Disabled
                }
            }
        }
    }

    /// Whether the persisted manifest was fetched for exactly this snapshot. The
    /// snapshot id is the strong per-publish key; the fingerprint must ALSO agree
    /// (including a both-`None` pair from publishers that never stamp one) so a
    /// re-published snapshot that reused an id can never serve stale fingerprints.
    fn baseline_manifest_matches_snapshot(
        record: &bsl_search::BaselineManifestRecord,
        snapshot: &bsl_search::Snapshot,
    ) -> bool {
        record.snapshot_id == snapshot.id.0 && record.fingerprint == snapshot.fingerprint
    }

    /// A failed Postgres-mode init must not leave a manifest behind that a later boot
    /// could mistake for a valid warm cache. The clear itself failing only costs that
    /// boot a manifest re-download, so it is not worth failing over.
    fn clear_baseline_manifest_best_effort(store: &bsl_search::Store) {
        if let Err(error) = store.clear_baseline_manifest() {
            tracing::warn!("failed to clear stale workspace baseline manifest: {error}");
        }
    }

    fn init_workspace_search_engine(
        workspace_root: &std::path::Path,
        watcher_ready: &Arc<AtomicBool>,
        mode: WorkspaceSearchMode,
        external_baseline: Option<Arc<ExternalBaselineService>>,
        graph: &GraphState,
    ) -> Option<WorkspaceSearchInit> {
        crate::cache::ensure_workspace_cache_dir(workspace_root).ok();
        let db_path = crate::cache::search_db_path(workspace_root);

        let project = project_model::Project::new(workspace_root);
        let source_path = project.source_path().to_path_buf();

        // Branch by the configured MODE, never by baseline presence: in Postgres mode a
        // missing service (connect failed / still pending past the wait) must leave the
        // search offline with a visible issue. Falling through to the local branch here
        // would silently start a full local reindex of the whole configuration — the
        // exact cost Postgres mode exists to avoid.
        if matches!(mode, WorkspaceSearchMode::PostgresRemoteOverlay) {
            let Some(external_baseline) = external_baseline
                .as_ref()
                .filter(|baseline| matches!(baseline.corpus(), CorpusId::WorkspaceCode))
            else {
                tracing::warn!(
                    "Postgres workspace mode is configured but the shared baseline is \
                     unavailable; workspace search stays offline (no local fallback)"
                );
                return None;
            };
            let mut engine = Self::open_workspace_overlay_search_engine(&db_path)?;
            Self::configure_workspace_engine(
                &mut engine,
                &source_path,
                watcher_ready,
                BaselineHashMode::NormalizedChunks,
            );

            let store = engine.store();
            if let Err(error) = store.clear_collection("code") {
                tracing::warn!("failed to clear stale local workspace baseline rows: {error}");
                return None;
            }
            if let Err(error) = store.clear_overlay_state("code") {
                tracing::warn!("failed to clear stale local overlay state: {error}");
                return None;
            }

            // The persisted manifest is deliberately NOT cleared up front: it is
            // immutable for a given snapshot, so it doubles as a warm-boot disk cache.
            // Once the cheap snapshot resolution below confirms the baseline still
            // points at the snapshot the manifest was fetched for, the expensive
            // per-file manifest download from Postgres is skipped entirely. Every
            // failure path clears it instead, so a failed init stays fail-closed.
            let manifest_files = match external_baseline.resolve_snapshot() {
                Ok(Some((_baseline_ref, snapshot))) => {
                    let cached = store.load_coherent_baseline_manifest().unwrap_or_else(|error| {
                        tracing::debug!(
                            "failed to read the persisted workspace baseline manifest: {error}"
                        );
                        None
                    });
                    let cached = cached.filter(|record| {
                        Self::baseline_manifest_matches_snapshot(record, &snapshot)
                    });
                    match cached {
                        Some(record) => {
                            tracing::info!(
                                snapshot_id = %snapshot.id.0,
                                manifest_files = record.manifest_files,
                                "workspace baseline manifest served from disk cache"
                            );
                            record.manifest_files
                        }
                        None => match external_baseline.load_baseline_manifest(&snapshot.id.0) {
                            Ok(manifest) => {
                                if let Err(error) = store.save_baseline_manifest(&manifest) {
                                    tracing::warn!(
                                        "failed to persist workspace baseline manifest: {error}"
                                    );
                                    Self::clear_baseline_manifest_best_effort(store);
                                    return None;
                                }
                                tracing::info!(
                                    snapshot_id = %snapshot.id.0,
                                    manifest_files = manifest.files.len(),
                                    "workspace baseline manifest loaded and persisted"
                                );
                                manifest.files.len()
                            }
                            Err(error) => {
                                tracing::warn!(
                                    "failed to load workspace baseline manifest: {error}"
                                );
                                Self::clear_baseline_manifest_best_effort(store);
                                return None;
                            }
                        },
                    }
                }
                Ok(None) => {
                    tracing::warn!(
                        "workspace baseline manifest unavailable for configured Postgres mode"
                    );
                    Self::clear_baseline_manifest_best_effort(store);
                    return None;
                }
                Err(error) => {
                    tracing::warn!("failed to resolve workspace baseline snapshot: {error}");
                    Self::clear_baseline_manifest_best_effort(store);
                    return None;
                }
            };

            tracing::info!(
                manifest_files,
                "workspace overlay-only baseline initialized; baseline search served from Postgres"
            );

            return Some(WorkspaceSearchInit {
                engine,
                mode: WorkspaceSearchMode::PostgresRemoteOverlay,
                pending_embed: None,
                overlay_init: OverlayInit::RemoteWarmup,
            });
        }

        let mut engine = Self::open_search_engine(&db_path)?;

        // A restart with partially embedded code must resume, not re-embed. The deferred
        // embedding pass already selects exactly the NULL-embedding chunks
        // (`load_pending_embedding_documents`), so an interrupted run picks up where it
        // left off regardless of file hashes. Clearing the hashes here would instead force
        // `index_directory_deferred` to DELETE+reinsert those files' chunks with NULL
        // embeddings, throwing away vectors already paid for — the opposite of resume.
        // Changed files are still detected and re-embedded via their content-hash mismatch.

        Self::configure_workspace_engine(
            &mut engine,
            &source_path,
            watcher_ready,
            BaselineHashMode::RawFileBytes,
        );

        // Fused cold-build: the graph owns the startup build decision. When it builds
        // the graph fresh it streams the search chunks (with graph context) from the
        // same parse pass, so this run only has to fill embeddings — no second parse,
        // no graph round-trip. On a warm cache, a missing embedder, or any failure it
        // returns `Standalone` and we fall through to the standalone indexer below.
        if let crate::graph::FusedStartup::Fused =
            graph.start_workspace_graph(&mut engine, &source_path)
        {
            // FTS chunks and graph context are written; embeddings are still NULL. Hand
            // the engine back immediately so lexical search and the graph go live in
            // minutes, and defer the ~hours-long embedding pass to a background thread
            // on its own connection (see `spawn_workspace_search_init`).
            let pending_embed = Self::embedding_config()
                .map(|config| PendingEmbed { db_path: db_path.clone(), config });
            // The fused parse pass ingested files present on disk but never removed rows for a `.bsl`
            // deleted while the daemon was down. Reconcile the store to disk so the overlay baseline
            // truly == working tree before asserting Clean; a walk that could not prove this
            // downgrades to a prime (which never asserts a false clean).
            let overlay_init = if Self::reconcile_boot_store_with_disk(&mut engine, &source_path) {
                OverlayInit::Clean
            } else {
                OverlayInit::Prime
            };
            return Some(WorkspaceSearchInit {
                engine,
                mode: WorkspaceSearchMode::SqliteLocal,
                pending_embed,
                overlay_init,
            });
        }

        // Standalone path (warm cache, no embedder, or fused fallback). Enrich semantic
        // embeddings with each method's call-graph context when the graph database is
        // already built; if absent (still building) the embeddings are graph-free this
        // run and pick up context on a later reindex.
        if engine.has_semantic() {
            let graph_path = crate::cache::graph_db_path(workspace_root);
            match crate::graph_query::GraphDb::open(&graph_path) {
                Ok(graph_db) => {
                    engine.set_graph_context_provider(Arc::new(
                        crate::graph_query::GraphDbContextProvider::new(graph_db),
                    ));
                    tracing::info!("graph-enriched embeddings enabled");
                }
                Err(e) => {
                    tracing::debug!(
                        "graph database unavailable; embeddings without graph context: {e}"
                    );
                }
            }
        }

        if engine.has_semantic() {
            // Same publish-early contract as the fused path, for the rare standalone
            // semantic cold start (fused build failed but an embedder is configured):
            // write FTS + chunks + graph context synchronously but WITHOUT embeddings (no
            // HTTP) so the engine publishes within minutes, then defer the hours-long
            // embedding to the background pass instead of blocking publication on a
            // synchronous `index_directory`. The graph context set above is persisted with
            // the chunks, so the deferred vectors are graph-enriched just as
            // `index_directory` would have produced.
            match engine.index_directory_deferred(&source_path) {
                Ok(indexed) => {
                    if indexed > 0 {
                        tracing::info!(indexed, "FTS + graph context written; embedding deferred");
                    }
                }
                Err(e) => tracing::warn!("failed to write deferred index: {e}"),
            }

            // Schedule the background pass only when chunks actually lack vectors. A warm
            // restart has none pending, so it stays `Ready` with no transient downgrade.
            let code_chunks = engine.chunk_count().unwrap_or(0);
            let code_embeddings = engine.embedding_count_by_collection("code").unwrap_or(0);
            let pending_embed = (code_chunks > code_embeddings)
                .then(Self::embedding_config)
                .flatten()
                .map(|config| PendingEmbed { db_path: db_path.clone(), config });

            // `index_directory_deferred` above re-ingested every file whose content hash changed
            // (incl. edits made while the daemon was down) but did not remove rows for a `.bsl`
            // deleted while down. Reconcile the store to disk so the overlay baseline == working tree
            // before asserting Clean; a walk that could not prove this downgrades to a prime.
            let overlay_init = if Self::reconcile_boot_store_with_disk(&mut engine, &source_path) {
                OverlayInit::Clean
            } else {
                OverlayInit::Prime
            };
            return Some(WorkspaceSearchInit {
                engine,
                mode: WorkspaceSearchMode::SqliteLocal,
                pending_embed,
                overlay_init,
            });
        }

        // FTS-only branch (no embedder configured — the common local dev setup). A cold store (no
        // chunks yet) gets a full walk+hash ingest; a warm store with existing chunks skips
        // re-indexing entirely, so it is NOT reconciled against files EDITED while the daemon was
        // down and must prime for those. Either way the index step never removes rows for files
        // DELETED while down, so both sub-branches reconcile the store to disk here (removing gone
        // rows); only the cold, freshly-ingested-and-reconciled sub-branch may then assert Clean.
        let overlay_init = if engine.chunk_count().unwrap_or(0) == 0 {
            tracing::info!(?source_path, "building FTS index from source files");
            match engine.index_directory_fts(&source_path) {
                Ok(indexed) => tracing::info!(indexed, "FTS index built"),
                Err(e) => tracing::warn!("failed to build FTS index: {e}"),
            }
            if Self::reconcile_boot_store_with_disk(&mut engine, &source_path) {
                OverlayInit::Clean
            } else {
                OverlayInit::Prime
            }
        } else {
            // Warm store: prime handles the while-down EDITS; the reconcile still removes rows for
            // files DELETED while down (a prime only hides them lazily and never from the store).
            Self::reconcile_boot_store_with_disk(&mut engine, &source_path);
            OverlayInit::Prime
        };

        Some(WorkspaceSearchInit {
            engine,
            mode: WorkspaceSearchMode::SqliteLocal,
            pending_embed: None,
            overlay_init,
        })
    }

    fn init_reference_search_engine(
        progress: &Arc<IndexProgress>,
        external_baseline: Option<Arc<ExternalBaselineService>>,
    ) -> Option<SearchEngine> {
        let db_path = Self::reference_search_db_path()?;
        Self::init_reference_search_engine_at(&db_path, progress, external_baseline)
    }

    fn init_reference_search_engine_at(
        db_path: &Path,
        progress: &Arc<IndexProgress>,
        external_baseline: Option<Arc<ExternalBaselineService>>,
    ) -> Option<SearchEngine> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let mut engine = Self::open_search_engine(db_path)?;
        if external_baseline
            .as_ref()
            .is_some_and(|baseline| matches!(baseline.corpus(), CorpusId::Reference))
        {
            if let Some(external_baseline) = external_baseline.as_ref() {
                let model_id = engine.embedding_model().map(ToOwned::to_owned);
                let dimension = engine.embedding_dimension();
                match external_baseline
                    .load_reference_snapshot_documents(model_id.as_deref(), dimension)
                {
                    Ok(Some(snapshot)) => {
                        if engine.has_semantic() {
                            let cleared = engine
                                .clear_file_hashes_without_embeddings("platform")
                                .unwrap_or(0);
                            if cleared > 0 {
                                tracing::info!(
                                    cleared,
                                    "cleared hashes for reference cache files without embeddings"
                                );
                            }
                        }
                        if let Err(error) = engine.remove_file("platform://docs", "platform") {
                            tracing::warn!("failed to clear local reference docs cache before external baseline mode: {error}");
                        }
                        Self::index_external_reference_docs(&mut engine, progress, snapshot);
                    }
                    Ok(None) => {
                        tracing::warn!(
                            "external reference baseline is configured but no snapshot was resolved; rebuilding local reference docs cache"
                        );
                        Self::rebuild_local_reference_docs_cache(&mut engine, progress);
                    }
                    Err(error) => {
                        tracing::warn!(
                            "failed to load external reference baseline snapshot for local semantic cache: {error}; rebuilding local reference docs cache"
                        );
                        Self::rebuild_local_reference_docs_cache(&mut engine, progress);
                    }
                }
            }
            tracing::info!(
                "external reference baseline is configured; lexical search uses the shared snapshot and semantic cache is synchronized locally"
            );
        } else {
            Self::index_platform_docs(&mut engine, progress);
        }
        Some(engine)
    }

    fn index_external_reference_docs(
        engine: &mut SearchEngine,
        progress: &Arc<IndexProgress>,
        snapshot: crate::baseline::BaselineSnapshotDocuments,
    ) {
        let version = snapshot.fingerprint.unwrap_or(snapshot.snapshot_id);

        tracing::info!(
            snapshot = %version,
            documents = snapshot.documents.len(),
            shared_embeddings = snapshot.shared_embeddings.len(),
            "synchronizing external reference snapshot into local semantic cache"
        );

        match engine.sync_indexed_documents_in_collection_with_embeddings(
            "platform",
            &snapshot.documents,
            Some(&snapshot.shared_embeddings),
            Some(progress),
        ) {
            Ok(indexed_files) => {
                if indexed_files > 0 {
                    tracing::info!(indexed_files, "external reference docs cached locally");
                } else {
                    tracing::info!("external reference docs cache is up to date");
                }
            }
            Err(error) => {
                tracing::warn!("failed to cache external reference docs locally: {error}");
            }
        }
    }

    fn rebuild_local_reference_docs_cache(
        engine: &mut SearchEngine,
        progress: &Arc<IndexProgress>,
    ) {
        Self::clear_reference_docs_cache(engine);
        Self::index_platform_docs(engine, progress);
    }

    fn clear_reference_docs_cache(engine: &mut SearchEngine) {
        match engine.sync_indexed_documents_in_collection(
            "platform",
            &[] as &[bsl_search::IndexedDocument],
            None,
        ) {
            Ok(removed_files) => {
                if removed_files > 0 {
                    tracing::info!(removed_files, "cleared stale reference docs cache files");
                }
            }
            Err(error) => {
                tracing::warn!("failed to clear stale reference docs cache: {error}");
            }
        }
    }

    fn index_platform_docs(engine: &mut SearchEngine, progress: &Arc<IndexProgress>) {
        let platform = PlatformDataInner::instance();
        if platform.all_types().is_empty() {
            tracing::debug!("no platform data available, skipping docs indexing");
            return;
        }

        let mut documents = Vec::new();

        for ty in platform.all_types() {
            let methods = platform.get_type_methods(&ty.name);
            let method_list: String = methods
                .iter()
                .map(|m| format!("{} / {}", m.name, m.english_name))
                .collect::<Vec<_>>()
                .join(", ");

            let body = format!("Тип: {} / {}\nМетоды: {method_list}", ty.name, ty.english_name,);
            documents.push(Document {
                title: format!("{} / {}", ty.name, ty.english_name),
                body,
                kind: "type".to_owned(),
            });
        }

        for method in platform.all_methods() {
            let mut body = format!(
                "Тип: {}\nМетод: {} / {}\n",
                method.type_name, method.name, method.english_name,
            );
            if let Some(ref ret) = method.return_type {
                body.push_str(&format!("Возвращает: {ret}\n"));
            }
            if let Some(docs) = platform.get_method_docs(method.id) {
                if !docs.syntax.is_empty() {
                    body.push_str(&format!("Синтаксис: {}\n", docs.syntax));
                }
                if !docs.description.is_empty() {
                    body.push_str(&format!("Описание: {}\n", docs.description));
                }
                for p in &docs.params {
                    body.push_str(&format!("Параметр {}: {}\n", p.name, p.description));
                }
                for ex in &docs.examples {
                    body.push_str(&format!("Пример: {}\n", ex.code));
                }
            }
            documents.push(Document {
                title: format!(
                    "{}.{} / {}.{}",
                    method.type_name, method.name, method.type_name, method.english_name
                ),
                body,
                kind: "method".to_owned(),
            });
        }

        for func in platform.all_global_functions() {
            let mut body = format!("Глобальная функция: {} / {}\n", func.name, func.english_name,);
            if let Some(ref ret) = func.return_type {
                body.push_str(&format!("Возвращает: {ret}\n"));
            }
            if let Some(docs) = platform.get_global_function_docs(func.id) {
                if !docs.syntax.is_empty() {
                    body.push_str(&format!("Синтаксис: {}\n", docs.syntax));
                }
                if !docs.description.is_empty() {
                    body.push_str(&format!("Описание: {}\n", docs.description));
                }
                for p in &docs.params {
                    body.push_str(&format!("Параметр {}: {}\n", p.name, p.description));
                }
            }
            documents.push(Document {
                title: format!("{} / {}", func.name, func.english_name),
                body,
                kind: "global_function".to_owned(),
            });
        }

        let version_bytes = env!("CARGO_PKG_VERSION").as_bytes();

        tracing::info!(
            types = platform.all_types().len(),
            methods = platform.all_methods().len(),
            global_functions = platform.all_global_functions().len(),
            total_documents = documents.len(),
            "indexing platform reference documentation"
        );

        match engine.index_documents(
            "platform",
            "platform://docs",
            version_bytes,
            &documents,
            Some(progress),
        ) {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(count, "platform docs indexed");
                } else {
                    tracing::info!("platform docs unchanged, skipped");
                }
            }
            Err(e) => {
                tracing::warn!("failed to index platform docs: {e}");
            }
        }
    }

    pub fn init_search(&self) {
        if let Some(root) = self.workspace_root.clone() {
            let watcher_ready = Arc::new(AtomicBool::new(false));
            Self::spawn_workspace_search_init(
                Arc::clone(&self.search_engine),
                Arc::clone(&self.index_progress),
                Arc::clone(&self.semantic_runtime),
                Arc::clone(&self.overlay_warmup),
                Arc::clone(&self.background_indexers),
                root,
                watcher_ready,
                self.baseline.clone(),
                self.workspace_search_mode.clone(),
                self.graph.clone(),
                Arc::clone(&self.embed_flight),
                Arc::new(crate::diagnostics_state::ResidentModuleSnapshotSource::new(
                    self.diagnostics.clone(),
                )),
            );
        }
    }

    fn reference_search_db_path() -> Option<PathBuf> {
        if let Some(base) = dirs::cache_dir() {
            return Some(base.join("bsl-analyzer").join("reference-search.db"));
        }

        if let Some(home) = env::var_os("HOME") {
            return Some(PathBuf::from(home).join(".cache/bsl-analyzer/reference-search.db"));
        }

        if let Some(profile) = env::var_os("USERPROFILE") {
            return Some(PathBuf::from(profile).join(".bsl-analyzer/reference-search.db"));
        }

        None
    }

    /// Drive the search overlay from the change hub. Search is one sink among
    /// several: it drains its own cursor and applies the shared drift classification
    /// (stateless policy) — `.bsl` bodies marked dirty, deleted `.bsl` removed from the
    /// store, `.xml` metadata resolved to the affected documents' context. The raw
    /// (non-canonical) path is used so the strip against the configured source root
    /// still matches when that root has symlinks.
    fn spawn_search_sink(
        hub: WorkspaceChangeHub,
        engine: SharedSearchEngine,
        watcher_ready: Arc<AtomicBool>,
        watch_root: PathBuf,
        graph: GraphState,
    ) {
        std::thread::Builder::new()
            .name("bsl-search-overlay-watch".to_owned())
            .spawn(move || {
                // Setup is asynchronous, so wait for it to settle rather than racing
                // a bare `is_watching` check that would bail before the watch arms.
                if !hub.wait_until_watching(Duration::from_secs(60)) {
                    tracing::warn!(
                        "workspace change hub is not watching; search overlay stays in scan mode"
                    );
                    return;
                }

                // Publish readiness before the engine may exist: the engine's own
                // configuration step checks this flag and enables watcher mode when
                // it finishes initializing. Enabling here too covers a warm engine
                // that is already published.
                watcher_ready.store(true, Ordering::SeqCst);
                if let Ok(mut guard) = engine.lock() {
                    if let Some(engine) = guard.as_mut() {
                        engine.enable_workspace_watcher_mode();
                    }
                }
                tracing::info!("search overlay sink subscribed to workspace change hub");

                let mut cursor = hub.subscribe();
                let mut generation = 0u64;
                loop {
                    // Wake on new drift; the timeout only bounds how long a shutdown
                    // takes to be noticed (the daemon detaches this thread).
                    generation = hub.wait_for_change(generation, Duration::from_secs(30));
                    let batch = hub.drain(cursor);
                    cursor = batch.cursor;
                    Self::apply_search_drift(
                        &engine,
                        &watch_root,
                        &batch.entries,
                        batch.rescan_required,
                        &graph,
                    );
                }
            })
            .ok();
    }

    /// Apply one drained batch to the search overlay. Extracted from the sink loop so it
    /// is unit-testable without driving the thread. On overflow (exact paths lost) it
    /// re-walks the whole tree; otherwise it classifies (stateless policy) and applies
    /// each bucket: `.bsl` bodies dirty, deleted `.bsl` removed, `.xml` → affected context.
    fn apply_search_drift(
        engine: &SharedSearchEngine,
        watch_root: &Path,
        entries: &[crate::change_hub::ChangeEntry],
        rescan_required: bool,
        graph: &GraphState,
    ) {
        // Overflow means the hub dropped detail: the exact changed paths are lost.
        // Restore parity with the old unbounded watcher (which never lost a `.bsl`) by
        // re-marking every workspace `.bsl` dirty, so the overlay's incremental refresh
        // reconsiders them all.
        if rescan_required {
            tracing::warn!(
                "workspace change hub overflowed; re-marking all workspace .bsl paths dirty for the search overlay"
            );
            Self::rewalk_workspace_bsl_dirty(engine, watch_root);
            return;
        }

        // Search keeps no per-path baseline, so the stateless policy (no baseline, empty
        // config set) buckets straight from on-disk truth.
        let class =
            crate::drift_classify::classify_drift(entries, &std::collections::HashSet::new(), None);

        // Modified `.bsl` bodies: mark dirty for the overlay's incremental refresh.
        for dp in &class.bsl_modified {
            Self::mark_search_path_dirty(engine, &dp.raw);
        }

        // Deleted `.bsl`: drop from the store so it stops appearing in results.
        if !class.bsl_removed.is_empty() {
            Self::remove_search_paths(engine, class.bsl_removed.iter().map(|d| d.raw.as_path()));
        }

        // Changed `.xml` metadata: mark the affected documents' stored context stale, then
        // nudge the graph to catch up. The context re-render only runs on a graph publish;
        // without this nudge a user who only calls `search_code` never triggers a rebuild,
        // so the marks would sit unresolved forever. The nudge is single-flight and never
        // blocks — it schedules a background rebuild whose publish fires the refresh hook.
        if !class.xml_paths.is_empty()
            && Self::mark_xml_affected_context_dirty(engine, &class.xml_paths, graph)
        {
            graph.nudge_rebuild();
        }

        // A subtree removal lost the descendant list → reconsider the whole tree.
        if class.structural_rescan {
            Self::rewalk_workspace_bsl_dirty(engine, watch_root);
        }
    }

    /// Remove a batch of deleted `.bsl` files from the workspace store. Each removal
    /// evicts exactly that file's vectors from the live index incrementally (no full
    /// rebuild, no sidecar rewrite — the row deletion already invalidates the persisted
    /// sidecar), so a large deletion no longer stalls under the engine lock. A path that
    /// is not a workspace `.bsl` is skipped.
    fn remove_search_paths<'a>(engine: &SharedSearchEngine, paths: impl Iterator<Item = &'a Path>) {
        if let Ok(mut guard) = engine.lock() {
            if let Some(engine) = guard.as_mut() {
                for path in paths {
                    match engine.remove_workspace_path(path) {
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!(path = ?path, "failed to remove workspace file: {e}")
                        }
                    }
                }
            }
        }
    }

    /// Resolve each changed `.xml` descriptor to the workspace documents it affects and
    /// mark their stored graph context stale, so a later reindex/embed pass re-renders it
    /// (marking only — the render is deferred, so it never races the graph's own drift
    /// catch-up). OWNED modules resolve by path convention: an MDO / common-module /
    /// service / form descriptor at `<Dir>/<Name>.xml` owns every `.bsl` under the sibling
    /// `<Dir>/<Name>/` subtree. Any `.xml` directly at the workspace root (a
    /// configuration-root descriptor, whose change can shift any module's context)
    /// conservatively marks the whole collection. REFERENCING modules (a module that
    /// merely READS the changed MDO — its rendered `graph_context` embeds the object's
    /// metadata reads) are additionally resolved through the persisted graph's inbound read
    /// edges (see [`Self::resolve_referencing_module_rels`]).
    ///
    /// The filesystem walk (owned-subtree resolution) and the graph db read (referencing
    /// resolution) both run OUTSIDE the engine lock; the lock is taken only briefly for the
    /// store writes. Returns whether it marked at least one path context-dirty (owned,
    /// referencing, or a whole-collection mark), so the caller can gate the graph catch-up
    /// nudge on real work having been queued.
    fn mark_xml_affected_context_dirty(
        engine: &SharedSearchEngine,
        xml_paths: &[crate::drift_classify::DriftPath],
        graph: &GraphState,
    ) -> bool {
        // Read the workspace root once (brief lock), then resolve owned subtrees off-lock.
        let workspace_root = {
            let Ok(guard) = engine.lock() else { return false };
            let Some(engine) = guard.as_ref() else { return false };
            engine.workspace_root().map(Path::to_path_buf)
        };

        let mut owned_modules: Vec<PathBuf> = Vec::new();
        let mut mark_whole_collection = false;
        for dp in xml_paths {
            match owned_module_subtree(&dp.raw) {
                Some(subtree) => owned_modules.extend(walk_bsl_files(&subtree)),
                None if is_workspace_root_xml(&dp.raw, workspace_root.as_deref()) => {
                    mark_whole_collection = true;
                }
                None => {}
            }
        }

        // Referencing modules: resolved off any lock via the persisted graph, BEFORE the
        // store-write lock below (the graph db read must never nest under the engine lock).
        let referencing_rels =
            Self::resolve_referencing_module_rels(graph, xml_paths, workspace_root.as_deref());

        if owned_modules.is_empty() && referencing_rels.is_empty() && !mark_whole_collection {
            return false;
        }

        // Brief lock for the store writes only.
        let Ok(guard) = engine.lock() else { return false };
        let Some(engine) = guard.as_ref() else { return false };
        let mut marked = false;
        if mark_whole_collection {
            match engine.mark_workspace_context_dirty() {
                Ok(count) => marked |= count > 0,
                Err(e) => tracing::warn!("failed to mark collection context dirty: {e}"),
            }
        }
        for bsl in owned_modules {
            match engine.mark_workspace_path_context_dirty(&bsl) {
                Ok(did) => marked |= did,
                Err(e) => tracing::warn!(path = ?bsl, "failed to mark context dirty: {e}"),
            }
        }
        for rel in referencing_rels {
            match engine.mark_workspace_path_context_dirty(&rel) {
                Ok(did) => marked |= did,
                Err(e) => {
                    tracing::warn!(path = %rel, "failed to mark referencing context dirty: {e}")
                }
            }
        }
        marked
    }

    /// Reverse-look-up the workspace modules that READ any changed MDO, returning their
    /// workspace-relative `.bsl` keys (the spelling the `code` collection stores). A metadata
    /// change alters the `graph_context` of every module that reads the object — not just its
    /// owned modules — and the persisted graph is the only record of who reads what.
    ///
    /// Queries the CURRENTLY PUBLISHED graph via [`GraphState::snapshot`], which gates on a
    /// published build and opens the read-only db off the graph's inner lock. Pre-drift edges
    /// are exactly right here: the set of referencing modules is defined by OTHER modules'
    /// bodies, which this `.xml` edit did not touch — the follow-up rebuild only re-renders the
    /// contexts marked here, it never changes who references the object. No published graph yet
    /// (or an `.xml` that maps to no MDO node — a form/command/config-root descriptor) → an
    /// empty set, so referencing marks are simply skipped and the owned marks + nudge still fire;
    /// a later publish consumes whatever marks then exist. Degrades, never blocks or errors.
    ///
    /// Off-lock throughout: opens the graph db once and runs one index-backed inbound-edge
    /// query per resolved MDO node id, so a batch of N `.xml` edits does at most N indexed
    /// queries, never a table scan.
    fn resolve_referencing_module_rels(
        graph: &GraphState,
        xml_paths: &[crate::drift_classify::DriftPath],
        workspace_root: Option<&Path>,
    ) -> std::collections::HashSet<String> {
        let mut rels = std::collections::HashSet::new();
        let mdo_ids: Vec<String> =
            xml_paths.iter().filter_map(|dp| xml_to_mdo_id(&dp.raw)).collect();
        if mdo_ids.is_empty() {
            return rels;
        }
        let Some(workspace_root) = workspace_root else { return rels };
        let Some(snapshot) = graph.snapshot() else { return rels };
        let source_prefix = canonical_source_prefix(workspace_root);
        for mdo_id in mdo_ids {
            match snapshot.graph.referencing_files(&mdo_id) {
                Ok(files) => {
                    for file in files {
                        if let Some(rel) = graph_file_to_rel(&file, &source_prefix) {
                            rels.insert(rel);
                        }
                    }
                }
                Err(e) => tracing::warn!(mdo = %mdo_id, "referencing-files lookup failed: {e}"),
            }
        }
        rels
    }

    /// After the graph publishes a fresh build, re-render the stored graph context of any
    /// search chunk whose owning file was marked context-dirty by an `.xml` drift, so a
    /// metadata edit becomes visible without waiting for the owning `.bsl` to change. This
    /// runs on the graph's background publish thread — never on a query path — because the
    /// freshly published graph is the "caught up" state a re-render must read. `build_start_seq`
    /// (captured when this build STARTED) bounds the marks it may clear: only drifts this
    /// build already reflects, never one stamped after it began, so a mark is never cleared
    /// against a graph that predates its `.xml` change. Opens the just-published graph
    /// database for the render; when the graph is unavailable nothing is cleared and the
    /// marks persist for the next publish. Never touches the resident mutex.
    fn refresh_search_contexts_after_graph(
        engine: &SharedSearchEngine,
        workspace_root: &Path,
        semantic_runtime: &Arc<Mutex<SemanticRuntimeStatus>>,
        background_indexers: &Arc<AtomicUsize>,
        index_progress: &Arc<IndexProgress>,
        embed_flight: &Arc<EmbedFlight>,
        signal: crate::graph::GraphPublishSignal,
    ) {
        let crate::graph::GraphPublishSignal { drift_pending, build_start_seq } = signal;
        // Fast-path skip (an optimization, not correctness): a follow-up reload is already
        // catching up, so let ITS publish re-render against the fresher graph. Correctness
        // does not depend on this — the `build_start_seq` bound below already prevents
        // clearing a mark against a graph that predates its drift.
        if drift_pending {
            tracing::debug!(
                "graph drift still pending; deferring search context refresh to the next publish"
            );
            return;
        }
        let graph_path = crate::cache::graph_db_path(workspace_root);
        let graph_db = match crate::graph_query::GraphDb::open(&graph_path) {
            Ok(db) => db,
            Err(e) => {
                tracing::debug!("graph unavailable for search context refresh: {e}");
                return;
            }
        };
        let provider = crate::graph_query::GraphDbContextProvider::new(graph_db);
        let cleared_embeddings = {
            let Ok(guard) = engine.lock() else { return };
            let Some(engine) = guard.as_ref() else { return };
            match engine.refresh_dirty_contexts(&provider, build_start_seq) {
                Ok(stats) if stats.paths_cleared > 0 => {
                    tracing::info!(
                        paths = stats.paths_cleared,
                        chunks = stats.chunks_updated,
                        cleared_embeddings = stats.cleared_embeddings,
                        "search graph context refreshed after graph publish"
                    );
                    stats.cleared_embeddings
                }
                Ok(_) => 0,
                Err(e) => {
                    tracing::warn!("search context refresh failed: {e}");
                    0
                }
            }
        };
        // Re-rendered chunks had their live embedding NULLed; without a re-embed they serve
        // the OLD vector in-process and vanish from semantic results after a restart until
        // the boot pass. Kick the same background embed machinery workspace init uses.
        if cleared_embeddings > 0 {
            Self::kick_context_reembed(
                engine,
                semantic_runtime,
                background_indexers,
                index_progress,
                embed_flight,
            );
        }
    }

    /// After a context refresh NULLed live embeddings, re-embed the pending chunks through the
    /// shared embed single-flight — the same pass workspace boot uses, so the two never race an
    /// index swap. When no embedder is configured the kick returns without claiming (lexical
    /// results, already fresh from the refresh, are the whole story).
    fn kick_context_reembed(
        engine: &SharedSearchEngine,
        semantic_runtime: &Arc<Mutex<SemanticRuntimeStatus>>,
        background_indexers: &Arc<AtomicUsize>,
        index_progress: &Arc<IndexProgress>,
        embed_flight: &Arc<EmbedFlight>,
    ) {
        // A no-embedder engine has nothing to re-embed; resolve the DB path only if semantic
        // is live so we never claim the flight for a pass that would do nothing.
        let db_path = engine.lock().ok().and_then(|guard| {
            guard
                .as_ref()
                .and_then(|engine| engine.has_semantic().then(|| engine.db_path().to_path_buf()))
        });
        let Some(db_path) = db_path else { return };
        let Some(config) = Self::embedding_config() else { return };

        Self::spawn_embed_pass(
            Arc::clone(engine),
            Arc::clone(semantic_runtime),
            Arc::clone(background_indexers),
            Arc::clone(index_progress),
            Arc::clone(embed_flight),
            db_path,
            config,
        );
    }

    /// The ONE background embed entry for the workspace: both boot (initial NULL embeddings)
    /// and the post-refresh kick funnel through here so they share a single claim. The caller
    /// that wins the claim runs the pass in a loop, re-running while a rerun was requested — so
    /// a caller that lost the claim (its later-NULLed chunks absorbed) is guaranteed a later
    /// iteration sees them.
    ///
    /// INVARIANT: because `embed_pending_chunks_standalone` re-selects NULL chunks from the
    /// store on every iteration and the `set_vector_index` swap happens per iteration, the LAST
    /// iteration installs an index reflecting the latest store state — an older caller can never
    /// install a stale index over a newer one.
    fn spawn_embed_pass(
        engine: SharedSearchEngine,
        semantic_runtime: Arc<Mutex<SemanticRuntimeStatus>>,
        background_indexers: Arc<AtomicUsize>,
        index_progress: Arc<IndexProgress>,
        embed_flight: Arc<EmbedFlight>,
        db_path: PathBuf,
        config: bsl_search::SearchConfig,
    ) {
        if !embed_flight.claim() {
            // A pass is already running; it will loop again and absorb these NULL chunks.
            return;
        }

        // Take the daemon-alive hold BEFORE spawning so the count never dips to zero.
        let work_guard = BackgroundWorkGuard::new(&background_indexers);
        Self::set_semantic_runtime_status(&semantic_runtime, SemanticRuntimeStatus::Indexing);
        // Clone the handles the thread owns; the originals stay behind for the spawn-error path.
        let engine = Arc::clone(&engine);
        let runtime = Arc::clone(&semantic_runtime);
        let flight = Arc::clone(&embed_flight);
        let spawned =
            std::thread::Builder::new().name("bsl-search-embed".to_owned()).spawn(move || {
                let _work_guard = work_guard;
                // Restore the flight claim on any abnormal exit; a clean release calls
                // `disarm()` first so this never stomps a later owner that already re-claimed.
                let mut claim_guard = EmbedClaimGuard::new(Arc::clone(&flight));
                // Restore the runtime status on any abnormal exit so it never sticks `Indexing`.
                let mut status_guard = EmbedStatusGuard::new(Arc::clone(&runtime));
                tracing::info!("background embedding pass started");
                loop {
                    flight.begin_pass();
                    #[cfg(test)]
                    if FORCE_EMBED_PASS_PANIC.load(Ordering::SeqCst) {
                        panic!("forced embedding pass panic");
                    }
                    match SearchEngine::embed_pending_chunks_standalone(
                        &db_path,
                        &config,
                        Some(&index_progress),
                    ) {
                        Ok(index) => {
                            let swapped = match engine.lock() {
                                Ok(mut guard) => match guard.as_mut() {
                                    Some(engine) => {
                                        engine.set_vector_index(index);
                                        true
                                    }
                                    None => false,
                                },
                                Err(e) => {
                                    tracing::warn!("embedding pass: engine lock error: {e}");
                                    false
                                }
                            };
                            if !swapped {
                                // The engine is gone (daemon shutting down); stop and let the
                                // status guard record the pass did not complete.
                                break;
                            }
                            #[cfg(test)]
                            {
                                let mut hook =
                                    EMBED_POST_PASS_HOOK.lock().unwrap_or_else(|p| p.into_inner());
                                if let Some(h) = hook.as_mut() {
                                    h(&db_path);
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("background embedding pass failed: {e}");
                            Self::set_semantic_runtime_status(
                                &runtime,
                                SemanticRuntimeStatus::Failed(format!(
                                    "background embedding failed: {e}"
                                )),
                            );
                            status_guard.finish();
                            return;
                        }
                    }
                    if !flight.finish_pass() {
                        // No rerun requested → the claim was released under the flight lock.
                        claim_guard.disarm();
                        Self::set_semantic_runtime_status(&runtime, SemanticRuntimeStatus::Ready);
                        status_guard.finish();
                        tracing::info!("background embedding pass complete; semantic index live");
                        return;
                    }
                    // A rerun was requested during the pass; loop again for its NULL chunks.
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("failed to spawn embedding thread: {e}");
            embed_flight.release();
            Self::set_semantic_runtime_status(
                &semantic_runtime,
                SemanticRuntimeStatus::Failed(format!("could not spawn embedding thread: {e}")),
            );
        }
    }

    /// Re-mark every workspace `.bsl` dirty for the search overlay, then reconcile the
    /// store against what is actually on disk. Used when the change hub overflowed or a
    /// subtree was removed and the exact changed paths are no longer known, so the overlay
    /// must reconsider the whole tree. Marking alone only covers files that STILL exist; a
    /// file deleted during the lost window would keep its FTS rows and vectors forever, so
    /// the reconcile diffs the walked (present) set against the stored set and removes the
    /// gone paths. The walk runs OUTSIDE the engine lock; the reconcile takes the lock only
    /// for its bounded O(stored) store writes.
    fn rewalk_workspace_bsl_dirty(engine: &SharedSearchEngine, watch_root: &Path) {
        let mut present: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        let mut walk_errors = 0usize;
        for file in walkdir::WalkDir::new(watch_root).follow_links(true) {
            let file = match file {
                Ok(file) => file,
                Err(e) => {
                    walk_errors += 1;
                    tracing::warn!("search rescan walk error: {e}");
                    continue;
                }
            };
            if file.file_type().is_file() {
                let path = file.path();
                if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")) {
                    present.insert(path.to_path_buf());
                }
                Self::mark_search_path_dirty(engine, path);
            }
        }
        #[cfg(test)]
        if FORCE_REWALK_WALK_ERROR.load(Ordering::SeqCst) {
            walk_errors += 1;
        }
        // A partial walk (permission error, symlink loop, transient IO) is NOT authoritative:
        // `present` is missing healthy files, so reconciling against it would delete them from
        // the store. Only reconcile when the walk completed with zero errors; marking the found
        // files dirty already happened above regardless.
        if walk_errors > 0 {
            tracing::warn!(
                walk_errors,
                "search rescan walk incomplete; skipping reconcile to avoid deleting healthy files"
            );
            return;
        }
        if let Ok(mut guard) = engine.lock() {
            if let Some(engine) = guard.as_mut() {
                match engine.reconcile_workspace_files(&present) {
                    Ok(removed) if removed > 0 => {
                        tracing::info!(
                            removed,
                            "search rescan reconciled deleted files out of the index"
                        )
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("search rescan reconcile failed: {e}"),
                }
            }
        }
    }

    /// Reconcile the just-indexed workspace store against on-disk truth at BOOT, on the still-owned
    /// engine (no shared lock held), BEFORE the overlay-init decision is applied. A boot index step
    /// (`index_directory_deferred` / `index_directory_fts`, or a fused parse ingest) only re-ingests
    /// files that EXIST now — it never removes rows for a `.bsl` DELETED while the daemon was down.
    /// So a store row for a vanished file survives, and an [`OverlayInit::Clean`] — which asserts the
    /// store already equals the working tree — would serve that ghost forever. This walks the source
    /// tree (error-aware) and, on a CLEAN walk, calls [`SearchEngine::reconcile_workspace_files`] to
    /// remove every stored-but-gone path (tombstone + overlay dirty + incremental vector eviction —
    /// the same removal path the overflow rescan ships).
    ///
    /// Returns whether the store was PROVEN reconciled: `false` on any walk error OR a reconcile
    /// failure. A partial walk's `present` set is short, so trusting it would delete healthy rows —
    /// hence the S1 gate (skip reconcile on any walk error) is kept verbatim. And because a failed
    /// walk could not prove reconciliation, the caller must NOT stay Clean: it downgrades to a prime,
    /// whose own scan lazily hides files it finds missing. A prime's scan may itself be incomplete
    /// after a walk error, but a prime never ASSERTS a clean store the way `Clean` does — it only
    /// serves what it can see and hides the rest — so it is the strictly safer degraded default,
    /// matching the pre-existing behavior for a store that could not be reconciled.
    fn reconcile_boot_store_with_disk(engine: &mut SearchEngine, source_root: &Path) -> bool {
        let mut present: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        let mut walk_errors = 0usize;
        for entry in walkdir::WalkDir::new(source_root).follow_links(true) {
            match entry {
                Ok(entry) => {
                    if entry.file_type().is_file()
                        && entry
                            .path()
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("bsl"))
                    {
                        present.insert(entry.path().to_path_buf());
                    }
                }
                Err(e) => {
                    walk_errors += 1;
                    tracing::warn!("search boot reconcile walk error: {e}");
                }
            }
        }
        #[cfg(test)]
        if FORCE_REWALK_WALK_ERROR.load(Ordering::SeqCst) {
            walk_errors += 1;
        }
        if walk_errors > 0 {
            tracing::warn!(
                walk_errors,
                "search boot reconcile walk incomplete; priming the overlay instead of clean-init"
            );
            return false;
        }
        match engine.reconcile_workspace_files(&present) {
            Ok(removed) => {
                if removed > 0 {
                    tracing::info!(
                        removed,
                        "search boot reconciled deleted files out of the store"
                    );
                }
                true
            }
            Err(e) => {
                tracing::warn!("search boot reconcile failed; priming the overlay instead: {e}");
                false
            }
        }
    }

    /// Prefetch resident snapshots for the overlay's dirty paths and feed them into the
    /// incremental reindex, so a following query serves chunks cut from the SHARED resident
    /// parse instead of a second disk read+parse. Called at the top of a code-search request,
    /// before the query acquires the engine lock.
    ///
    /// Bounded to [`MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY`] paths per call.
    ///
    /// Lock discipline: the resident read must never overlap the engine lock. So this
    /// reads the dirty-path list and the source handle under a brief engine lock, RELEASES it,
    /// fetches the snapshots with NO lock held, then applies them under a second brief engine
    /// lock that only touches the overlay cache (never the resident). A resident that is
    /// absent/loading, or a path it cannot serve, is simply missing from the map and the
    /// reindex disk-reads it — so search never regresses when the resident is unavailable.
    pub(super) fn prefetch_resident_overlay_impl(engine: &SharedSearchEngine) {
        let (source, workspace_root, dirty) = {
            let Ok(guard) = engine.lock() else { return };
            let Some(engine) = guard.as_ref() else { return };
            let Some(source) = engine.module_snapshot_source() else { return };
            // The overlay keys dirty paths relative to THIS engine root (the project's — possibly
            // nested — source root); resolving them for the resident needs the same root.
            let Some(workspace_root) = engine.workspace_root().map(Path::to_path_buf) else {
                return;
            };
            match engine.workspace_overlay_dirty_paths() {
                Ok(dirty) => (source, workspace_root, dirty),
                Err(e) => {
                    tracing::debug!("overlay dirty-path read failed: {e}");
                    return;
                }
            }
        };
        if dirty.is_empty() {
            return;
        }

        // Search and diagnostics drain independent hub cursors and a query never polls drift on
        // its own, so the resident is usually BEHIND disk on the just-edited files. Reconcile
        // pending drift FIRST — off the engine lock, resident lock only (I3 holds) — so the
        // snapshot text below matches disk and the byte-compare hits instead of falling back to a
        // disk read. A resident rebuild in flight is skipped inside the drain, never blocking here.
        source.catch_up();

        // Resident reads run OFF the engine lock. The `!Send` parses stay in this local map on
        // the calling thread and never cross a thread or an await boundary.
        let mut snapshots: std::collections::HashMap<String, bsl_search::ModuleSnapshot> =
            std::collections::HashMap::new();
        // Cap the per-query resident prefetch: a branch switch can dirty thousands of paths, and
        // fetching+reindexing them all on the query thread would be unbounded work. Serve at most
        // this many from the shared parse per query; the remainder STAY dirty and are picked up by
        // the query's own lazy disk refresh and by later queries' prefetches. The cap is the whole
        // budget — no separate time budget needed.
        for rel_path in dirty.iter().take(MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY) {
            // Resolve the engine-relative dirty path to an ABSOLUTE path against the engine root
            // before handing it to the resident: the resident is indexed under the OUTER workspace
            // root, so a bare relative path would be re-joined against that root and silently miss
            // on every nested config. The snapshot map stays keyed by the engine rel, which is
            // what `reindex_dirty_from_snapshots` looks up.
            let abs_path = workspace_root.join(rel_path);
            if let bsl_search::SnapshotFetch::Fetched(snapshot) =
                source.text_and_parse(&abs_path.to_string_lossy())
            {
                snapshots.insert(rel_path.clone(), snapshot);
            }
        }
        if snapshots.is_empty() {
            return;
        }

        let Ok(guard) = engine.lock() else { return };
        let Some(engine) = guard.as_ref() else { return };
        if let Err(e) = engine.reindex_dirty_from_snapshots(&snapshots) {
            tracing::debug!("resident-fed overlay reindex failed: {e}");
        }
    }

    /// Mark one path dirty in the search overlay if it is a `.bsl` file. Filtering
    /// on the consumer side keeps the hub itself extension-agnostic.
    fn mark_search_path_dirty(engine: &SharedSearchEngine, path: &Path) {
        if !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")) {
            return;
        }
        if let Ok(guard) = engine.lock() {
            if let Some(engine) = guard.as_ref() {
                if let Err(e) = engine.mark_workspace_path_dirty(path) {
                    tracing::warn!(path = ?path, "failed to mark workspace file dirty: {e}");
                }
            }
        }
    }
}

/// The owned-module subtree of a metadata descriptor `.xml`: `<Dir>/<Name>/` beside a
/// `<Dir>/<Name>.xml`, when that directory exists. Every `.bsl` under it (object /
/// manager / recordset / form / command modules, or a common-module / service body) is
/// owned by the object the descriptor defines — so the path convention covers ordinary
/// MDOs (which carry no substrate back-link) and common-modules/services alike, with no
/// resident lookup and no resident/engine lock coupling.
fn owned_module_subtree(xml: &Path) -> Option<PathBuf> {
    let stem = xml.file_stem()?;
    let subtree = xml.parent()?.join(stem);
    subtree.is_dir().then_some(subtree)
}

/// Every `.bsl` file under `dir`.
fn walk_bsl_files(dir: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .filter(|p| p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")))
        .collect()
}

/// Map a metadata descriptor `.xml` at `<KindPlural>/<Name>.xml` to its graph MDO node id
/// `mdo/<EnglishType>/<Name>` (the id the fused build encodes, verified against
/// `ide::GraphRowEncoder`). `None` when the parent directory is not a known metadata-kind
/// plural — a form/command descriptor, an `Ext/…` file, or a configuration-root descriptor —
/// since those carry no `mdo/` node and thus no inbound read edges to reverse-look-up. The
/// `<KindPlural>` → [`bsl_metadata::MdoType`] mapping reuses the canonical
/// [`bsl_metadata::MdoType::from_plural`] table rather than duplicating a directory map.
fn xml_to_mdo_id(xml: &Path) -> Option<String> {
    let name = xml.file_stem()?.to_str()?;
    let kind_dir = xml.parent()?.file_name()?.to_str()?;
    let mdo_type = bsl_metadata::MdoType::from_plural(kind_dir)?;
    Some(format!("mdo/{}/{name}", mdo_type.english_name()))
}

/// The canonical, `/`-normalised source prefix used to relativise a graph `nodes.file`
/// (stored absolute + canonical by `enumerate_bsl_files`) into the `code` collection key,
/// derived exactly as `FusedChunkWriter` derives its stored rel paths so the two agree.
fn canonical_source_prefix(workspace_root: &Path) -> String {
    workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

/// Relativise an absolute, `/`-normalised graph `nodes.file` to the `code` collection key,
/// mirroring `FusedChunkWriter`: strip the source prefix, require a path-separator boundary
/// so a sibling root whose name merely starts with the prefix string is not mistaken for a
/// child, then drop the leading `/`. `None` for a file outside the source root (an extension
/// module the local index omits) or an empty remainder.
fn graph_file_to_rel(file: &str, source_prefix: &str) -> Option<String> {
    let prefix = source_prefix.trim_end_matches('/');
    let rel =
        file.strip_prefix(prefix).filter(|rest| rest.starts_with('/'))?.trim_start_matches('/');
    (!rel.is_empty()).then(|| rel.to_owned())
}

/// Whether `xml` sits directly at the workspace root — any such descriptor
/// (`Configuration.xml`, `ConfigDumpInfo.xml`, a plugin's root descriptor, …) can shift
/// any module's context, so it is handled conservatively by marking the whole collection
/// rather than a resolvable owned subtree. When the workspace root is unknown, fall back
/// to the `Configuration.xml` name so the conservative branch still fires for the one
/// descriptor guaranteed to live at the root.
fn is_workspace_root_xml(xml: &Path, workspace_root: Option<&Path>) -> bool {
    match workspace_root {
        Some(root) => xml.parent() == Some(root),
        None => xml.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Configuration.xml")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{EnvVarGuard, ENV_LOCK};
    use super::{BackgroundWorkGuard, OverlayInit, SharedState, WorkspaceSearchMode};
    use crate::baseline::{
        BaselineBootstrap, BaselineRuntime, ConfiguredBaselineStatus, ExternalBaselineService,
        RefreshableExternalBaselineSource,
    };
    use bsl_search::{
        BaselineRef, CorpusId, Document, ExternalBaselineConfig, IndexedDocument, SearchEngine,
    };
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;
    fn immediate_bootstrap(backend: &'static str, issue: Option<&str>) -> BaselineBootstrap {
        BaselineBootstrap::Immediate(BaselineRuntime {
            configured_baseline: ConfiguredBaselineStatus {
                backend,
                selection: "test".to_owned(),
                issue: issue.map(str::to_owned),
                support: None,
            },
            external_baseline: None,
        })
    }

    /// A postgres config failure (unconfigured section, credential rejection) must NOT
    /// downgrade the mode to SqliteLocal: that would silently reindex the whole
    /// configuration locally instead of surfacing the configured backend's issue.
    #[test]
    fn workspace_mode_stays_postgres_for_immediate_config_failures() {
        assert!(matches!(
            SharedState::workspace_mode_for(&immediate_bootstrap(
                "postgres",
                Some("credentials rejected"),
            )),
            WorkspaceSearchMode::PostgresRemoteOverlay
        ));
        assert!(matches!(
            SharedState::workspace_mode_for(&immediate_bootstrap("sqlite", None)),
            WorkspaceSearchMode::SqliteLocal
        ));
    }

    #[test]
    fn background_work_guard_releases_on_every_path_including_panic() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let counter = Arc::new(AtomicUsize::new(0));
        {
            let _g1 = BackgroundWorkGuard::new(&counter);
            assert_eq!(counter.load(Ordering::SeqCst), 1);
            let _g2 = BackgroundWorkGuard::new(&counter);
            assert_eq!(counter.load(Ordering::SeqCst), 2, "nested tasks stack");
        }
        assert_eq!(counter.load(Ordering::SeqCst), 0, "both holds released on scope exit");

        // The embedding pass can bail via `?` or panic mid-run; the broker's liveness
        // signal must still fall back to idle, or the daemon would never shut down.
        let counter2 = Arc::clone(&counter);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = BackgroundWorkGuard::new(&counter2);
            assert_eq!(counter2.load(Ordering::SeqCst), 1);
            panic!("simulated indexing-task panic");
        }));
        assert!(unwound.is_err(), "the task panicked");
        assert_eq!(counter.load(Ordering::SeqCst), 0, "the hold was released on unwind");
    }

    #[test]
    fn workspace_external_failure_clears_local_baseline_rows_before_failing_closed() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _embedding_url = EnvVarGuard::unset("EMBEDDING_URL");
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        fs::write(
            workspace.join("CommonModule.bsl"),
            "Процедура ЛокальнаяПроцедура()\nКонецПроцедуры",
        )
        .unwrap();
        crate::cache::ensure_workspace_cache_dir(workspace).unwrap();

        let db_path = crate::cache::search_db_path(workspace);
        let mut stale_engine = SearchEngine::fts_only(&db_path).unwrap();
        stale_engine
            .sync_indexed_documents_in_collection(
                "code",
                &[IndexedDocument {
                    collection: "code".to_owned(),
                    path: "GhostModule.bsl".to_owned(),
                    symbol_name: "ПризрачнаяПроцедура".to_owned(),
                    kind: "procedure".to_owned(),
                    line_start: 0,
                    line_end: 1,
                    text: "Процедура ПризрачнаяПроцедура()\nКонецПроцедуры".to_owned(),
                    content_hash: "ghost".to_owned(),
                    graph_context: None,
                }],
                None,
            )
            .unwrap();
        assert_eq!(stale_engine.file_count().unwrap(), 1);
        // Seed a persisted manifest too: with the warm-boot cache the init path no
        // longer wipes it up front, so this test must prove the failure branch does.
        stale_engine
            .store()
            .save_baseline_manifest(&bsl_search::WorkspaceBaselineManifest {
                snapshot_id: "stale-snap".to_owned(),
                snapshot_fingerprint: Some("stale-fp".to_owned()),
                files: vec![bsl_search::BaselineManifestFile {
                    collection: "code".to_owned(),
                    path: "GhostModule.bsl".to_owned(),
                    file_fingerprint: "ghost".to_owned(),
                    document_count: 1,
                    file_object_id: "obj-ghost".to_owned(),
                }],
            })
            .unwrap();
        assert!(stale_engine.store().load_baseline_manifest().unwrap().is_some());
        drop(stale_engine);

        let watcher_ready = Arc::new(AtomicBool::new(false));
        let external = ExternalBaselineService::for_test(
            RefreshableExternalBaselineSource::for_test(
                ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
                BaselineRef {
                    corpus: CorpusId::WorkspaceCode,
                    snapshot_id: None,
                    branch: Some("main".to_owned()),
                    commit: None,
                },
            )
            .unwrap(),
        );

        let init = SharedState::init_workspace_search_engine(
            workspace,
            &watcher_ready,
            crate::state::WorkspaceSearchMode::PostgresRemoteOverlay,
            Some(external),
            &crate::graph::GraphState::disabled(),
        );

        assert!(init.is_none());
        let reopened = SearchEngine::fts_only(&db_path).unwrap();
        assert_eq!(reopened.file_count().unwrap(), 0);
        assert!(reopened.text_search("ПризрачнаяПроцедура", 10, Some("code")).unwrap().is_empty());
        assert!(reopened.store().load_baseline_manifest().unwrap().is_none());
    }

    #[test]
    fn baseline_manifest_matches_snapshot_requires_id_and_fingerprint_agreement() {
        let record =
            |snapshot_id: &str, fingerprint: Option<&str>| bsl_search::BaselineManifestRecord {
                snapshot_id: snapshot_id.to_owned(),
                fingerprint: fingerprint.map(str::to_owned),
                manifest_files: 1,
                fetched_at: "0".to_owned(),
            };
        let snapshot = |id: &str, fingerprint: Option<&str>| {
            let snapshot = bsl_search::Snapshot::new(id, CorpusId::WorkspaceCode);
            match fingerprint {
                Some(fingerprint) => snapshot.with_fingerprint(fingerprint),
                None => snapshot,
            }
        };
        let matches = SharedState::baseline_manifest_matches_snapshot;

        assert!(matches(&record("snap-1", Some("fp-1")), &snapshot("snap-1", Some("fp-1"))));
        assert!(matches(&record("snap-1", None), &snapshot("snap-1", None)));
        assert!(!matches(&record("snap-1", Some("fp-1")), &snapshot("snap-2", Some("fp-1"))));
        assert!(!matches(&record("snap-1", Some("fp-1")), &snapshot("snap-1", Some("fp-2"))));
        assert!(!matches(&record("snap-1", None), &snapshot("snap-1", Some("fp-1"))));
        assert!(!matches(&record("snap-1", Some("fp-1")), &snapshot("snap-1", None)));
    }

    #[test]
    fn workspace_external_failure_with_embeddings_fails_closed_without_hybrid_warmup() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _embedding_url = EnvVarGuard::set("EMBEDDING_URL", "http://127.0.0.1:9/v1");
        // A configured embedder now requires an explicit model (no silent default), so set
        // one here; otherwise the ambient env decides whether the engine is semantic, which
        // is what made this test pass locally but fail in CI.
        let _embedding_model = EnvVarGuard::set("EMBEDDING_MODEL", "test-model");

        let dir = tempdir().unwrap();
        let workspace = dir.path();
        crate::cache::ensure_workspace_cache_dir(workspace).unwrap();
        let db_path = crate::cache::search_db_path(workspace);
        let mut stale_engine = SearchEngine::fts_only(&db_path).unwrap();
        stale_engine
            .sync_indexed_documents_in_collection(
                "code",
                &[IndexedDocument {
                    collection: "code".to_owned(),
                    path: "GhostModule.bsl".to_owned(),
                    symbol_name: "ПризрачнаяПроцедура".to_owned(),
                    kind: "procedure".to_owned(),
                    line_start: 0,
                    line_end: 1,
                    text: "Процедура ПризрачнаяПроцедура()\nКонецПроцедуры".to_owned(),
                    content_hash: "ghost".to_owned(),
                    graph_context: None,
                }],
                None,
            )
            .unwrap();
        drop(stale_engine);

        let watcher_ready = Arc::new(AtomicBool::new(false));
        let external = ExternalBaselineService::for_test(
            RefreshableExternalBaselineSource::for_test(
                ExternalBaselineConfig::postgres("postgres://127.0.0.1:1"),
                BaselineRef {
                    corpus: CorpusId::WorkspaceCode,
                    snapshot_id: None,
                    branch: Some("main".to_owned()),
                    commit: None,
                },
            )
            .unwrap(),
        );

        let init = SharedState::init_workspace_search_engine(
            workspace,
            &watcher_ready,
            crate::state::WorkspaceSearchMode::PostgresRemoteOverlay,
            Some(external),
            &crate::graph::GraphState::disabled(),
        );

        assert!(init.is_none());
        let reopened = SearchEngine::fts_only(&db_path).unwrap();
        assert_eq!(reopened.file_count().unwrap(), 0);
        assert!(reopened.store().load_baseline_manifest().unwrap().is_none());
    }

    #[test]
    fn workspace_standalone_semantic_fallback_publishes_before_embedding() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        // A configured embedder makes the engine semantic, but the URL is unreachable:
        // the point is that init must NOT run the synchronous embed here. It writes the
        // FTS chunks and defers embedding, so init returns promptly with work pending.
        let _embedding_url = EnvVarGuard::set("EMBEDDING_URL", "http://127.0.0.1:9/v1");
        // A configured embedder now requires an explicit model (no silent default), so set
        // one here; otherwise the ambient env decides whether the engine is semantic, which
        // is what made this test pass locally but fail in CI.
        let _embedding_model = EnvVarGuard::set("EMBEDDING_MODEL", "test-model");

        let dir = tempdir().unwrap();
        let workspace = dir.path();
        crate::cache::ensure_workspace_cache_dir(workspace).unwrap();
        fs::write(workspace.join("CommonModule.bsl"), "Процедура СделатьЧтоТо()\nКонецПроцедуры")
            .unwrap();

        let watcher_ready = Arc::new(AtomicBool::new(false));
        // A disabled graph has no workspace root, so the fused path is skipped and the
        // standalone semantic branch runs — the path that previously embedded inline.
        let init = SharedState::init_workspace_search_engine(
            workspace,
            &watcher_ready,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("standalone init should produce an engine");

        // FTS chunks are written (lexical search goes live)...
        assert!(init.engine.chunk_count().unwrap() > 0);
        // ...the unreachable embedder was never called, so no vectors exist yet...
        assert_eq!(init.engine.vector_count(), 0);
        // ...and the embedding work is handed to the background pass.
        assert!(init.pending_embed.is_some());
    }

    #[test]
    fn clear_reference_docs_cache_removes_stale_local_and_external_docs() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("reference-search.db");
        let mut stale_engine = SearchEngine::fts_only(&db_path).unwrap();
        stale_engine
            .index_documents(
                "platform",
                "platform://docs",
                b"stale-docs",
                &[Document {
                    title: "СтарыйДокумент".to_owned(),
                    body: "Описание СтарыйДокумент".to_owned(),
                    kind: "type".to_owned(),
                }],
                None,
            )
            .unwrap();
        stale_engine
            .index_documents(
                "platform",
                "platform://legacy/external",
                b"stale-external-docs",
                &[Document {
                    title: "СтарыйВнешнийДокумент".to_owned(),
                    body: "Описание СтарыйВнешнийДокумент".to_owned(),
                    kind: "type".to_owned(),
                }],
                None,
            )
            .unwrap();
        assert_eq!(
            stale_engine.text_search("СтарыйДокумент", 10, Some("platform")).unwrap().len(),
            1
        );
        assert_eq!(
            stale_engine.text_search("СтарыйВнешнийДокумент", 10, Some("platform")).unwrap().len(),
            1
        );
        drop(stale_engine);

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        SharedState::clear_reference_docs_cache(&mut engine);

        assert!(engine.text_search("СтарыйДокумент", 10, Some("platform")).unwrap().is_empty());
        assert!(engine
            .text_search("СтарыйВнешнийДокумент", 10, Some("platform"))
            .unwrap()
            .is_empty());
    }

    /// `metadata form` in a nested layout — config root `<ws>/src/cf`, workspace root one
    /// level up — resolves object form directories relative to the CONFIG root. That root
    /// is `SharedState::source_root()`, which must survive the `MetadataCache` retirement:
    /// `form` reads it directly and is the one metadata action with no substrate backing.
    #[test]
    fn metadata_form_resolves_under_nested_source_root_after_cache_removal() {
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let cf = ws.join("src").join("cf");
        fs::create_dir_all(cf.join("Catalogs").join("Товары").join("Forms").join("ФормаСписка"))
            .unwrap();
        fs::write(cf.join("Configuration.xml"), "<Configuration/>").unwrap();

        let state = SharedState::workspace(ws.to_path_buf());
        let source_root =
            state.source_root().cloned().expect("source_root is set for a workspace profile");
        assert!(
            source_root.ends_with("src/cf") || source_root.ends_with("src\\cf"),
            "source_root points at the nested config root, not the workspace root: {source_root:?}",
        );

        // `metadata form` lists the object's forms relative to that config root.
        let result = crate::tools::metadata::get_form_structure(
            Some(&source_root),
            "Catalog",
            Some("Товары"),
            None,
        );
        state.shutdown();
        let result = result.expect("metadata form must resolve under the nested config root");
        let text = result.content[0].raw.as_text().expect("text content").text.clone();
        assert!(text.contains("ФормаСписка"), "form listing resolves under src/cf: {text}");
    }

    /// The search overlay sink preserves the pre-hub watcher behavior: a `.bsl`
    /// change lands in the engine's dirty set, while a non-`.bsl` change reaches
    /// the hub accumulator (any consumer can see it) but is not marked dirty for
    /// search.
    #[test]
    fn search_sink_marks_only_bsl_paths_dirty() {
        use crate::change_hub::WorkspaceChangeHub;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::{Duration, Instant};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let hub = WorkspaceChangeHub::start(vec![workspace.clone()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)), "the watch must arm");
        // A second cursor observes the raw accumulator independently of the sink.
        let observer = hub.subscribe();

        let watcher_ready = Arc::new(AtomicBool::new(false));
        SharedState::spawn_search_sink(
            hub.clone(),
            Arc::clone(&engine_arc),
            Arc::clone(&watcher_ready),
            workspace.clone(),
            crate::graph::GraphState::disabled(),
        );

        // Wait deterministically for the sink to subscribe (observer + sink = 2
        // cursors) before mutating the tree, so its cursor covers the changes below.
        let deadline = Instant::now() + Duration::from_secs(5);
        while hub.active_cursor_count() < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(hub.active_cursor_count(), 2, "the sink subscribed its cursor");

        let bsl = workspace.join("Module.bsl");
        std::fs::write(&bsl, "Процедура П()\nКонецПроцедуры").unwrap();
        let xml = workspace.join("Configuration.xml");
        std::fs::write(&xml, "<Configuration/>").unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut dirty_has_bsl = false;
        while Instant::now() < deadline {
            let snapshot = {
                let guard = engine_arc.lock().unwrap();
                guard.as_ref().unwrap().workspace_overlay_dirty_paths_snapshot().unwrap()
            };
            if snapshot.keys().any(|p| p.ends_with("Module.bsl")) {
                dirty_has_bsl = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(dirty_has_bsl, "the .bsl change is marked dirty for the search overlay");

        let snapshot = {
            let guard = engine_arc.lock().unwrap();
            guard.as_ref().unwrap().workspace_overlay_dirty_paths_snapshot().unwrap()
        };
        assert!(
            !snapshot.keys().any(|p| p.ends_with("Configuration.xml")),
            "search ignores non-.bsl paths",
        );
        assert!(watcher_ready.load(Ordering::SeqCst), "the sink publishes watcher readiness");

        // The hub itself accepted the .xml change; only the consumer filtered it.
        // The event is asynchronous, so poll the observer cursor until it lands.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut observer = observer;
        let mut saw_xml = false;
        while Instant::now() < deadline {
            let batch = hub.drain(observer);
            observer = batch.cursor;
            if batch.entries.iter().any(|e| e.raw.ends_with("Configuration.xml")) {
                saw_xml = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(saw_xml, "the accumulator carries the .xml change for other consumers");
    }

    /// On a hub overflow the exact changed paths are lost, so the sink re-walks the
    /// workspace and marks every `.bsl` dirty (and nothing else), restoring the
    /// old unbounded watcher's guarantee that no `.bsl` change is dropped.
    #[test]
    fn search_sink_rewalks_all_bsl_on_overflow() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        // Watcher mode makes `mark_workspace_path_dirty` record into the dirty set.
        engine.enable_workspace_watcher_mode();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        // A nested tree of `.bsl` plus a non-`.bsl` file that must NOT be marked.
        let nested = workspace.join("CommonModules").join("Модуль");
        fs::create_dir_all(&nested).unwrap();
        let a = workspace.join("A.bsl");
        let b = nested.join("B.bsl");
        fs::write(&a, "Процедура П()\nКонецПроцедуры").unwrap();
        fs::write(&b, "Процедура П()\nКонецПроцедуры").unwrap();
        fs::write(workspace.join("Configuration.xml"), "<Configuration/>").unwrap();

        SharedState::rewalk_workspace_bsl_dirty(&engine_arc, &workspace);

        let snapshot = {
            let guard = engine_arc.lock().unwrap();
            guard.as_ref().unwrap().workspace_overlay_dirty_paths_snapshot().unwrap()
        };
        assert!(snapshot.keys().any(|p| p.ends_with("A.bsl")), "top-level .bsl re-marked");
        assert!(snapshot.keys().any(|p| p.ends_with("B.bsl")), "nested .bsl re-marked");
        assert!(
            !snapshot.keys().any(|p| p.ends_with("Configuration.xml")),
            "non-.bsl paths are left alone",
        );
    }

    /// A deleted `.bsl` is removed from the workspace store so it stops appearing in
    /// results — closing the pre-existing gap where a deleted file lingered in FTS.
    #[test]
    fn search_sink_removes_deleted_bsl_from_results() {
        use crate::change_hub::{ChangeEntry, ChangeKind};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine
            .sync_indexed_documents_in_collection(
                "code",
                &[IndexedDocument {
                    collection: "code".to_owned(),
                    path: "Removed.bsl".to_owned(),
                    symbol_name: "УдаляемаяПроцедура".to_owned(),
                    kind: "procedure".to_owned(),
                    line_start: 0,
                    line_end: 1,
                    text: "Процедура УдаляемаяПроцедура()\nКонецПроцедуры".to_owned(),
                    content_hash: "h".to_owned(),
                    graph_context: None,
                }],
                None,
            )
            .unwrap();
        assert_eq!(engine.file_count().unwrap(), 1);
        assert!(
            !engine.text_search("УдаляемаяПроцедура", 10, Some("code")).unwrap().is_empty(),
            "the indexed file is initially found",
        );
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        // The file is gone from disk: classification re-stats it (stats are truth) → removed.
        let removed = workspace.join("Removed.bsl");
        let entry = ChangeEntry {
            canonical: removed.clone(),
            raw: removed,
            kind: ChangeKind::MaybeRemoved,
            seq: 1,
        };
        SharedState::apply_search_drift(
            &engine_arc,
            &workspace,
            &[entry],
            false,
            &crate::graph::GraphState::disabled(),
        );

        let guard = engine_arc.lock().unwrap();
        let engine = guard.as_ref().unwrap();
        assert_eq!(engine.file_count().unwrap(), 0, "the deleted file is dropped from the store");
        assert!(
            engine.text_search("УдаляемаяПроцедура", 10, Some("code")).unwrap().is_empty(),
            "the deleted file no longer appears in FTS results",
        );
    }

    /// An `.xml` metadata edit marks only the owned modules (the sibling `<Dir>/<Name>/`
    /// subtree) context-dirty via the store side table; unrelated modules are untouched
    /// and nothing is marked dirty — proving the resolver walks the owned subtree only,
    /// never the whole workspace.
    #[test]
    fn search_sink_xml_marks_only_owned_modules_context_dirty() {
        use crate::change_hub::{ChangeEntry, ChangeKind};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        // An MDO descriptor with an owned module, plus an unrelated object elsewhere.
        let owned = workspace.join("Catalogs/Товары/Ext/ObjectModule.bsl");
        fs::create_dir_all(owned.parent().unwrap()).unwrap();
        fs::write(&owned, "Процедура П()\nКонецПроцедуры").unwrap();
        let unrelated = workspace.join("Catalogs/Другой/Ext/ObjectModule.bsl");
        fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
        fs::write(&unrelated, "Процедура П()\nКонецПроцедуры").unwrap();
        let xml = workspace.join("Catalogs/Товары.xml");
        fs::write(&xml, "<MetaDataObject/>").unwrap();

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.enable_workspace_watcher_mode();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let entry = ChangeEntry {
            canonical: xml.clone(),
            raw: xml,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };
        SharedState::apply_search_drift(
            &engine_arc,
            &workspace,
            &[entry],
            false,
            &crate::graph::GraphState::disabled(),
        );

        let guard = engine_arc.lock().unwrap();
        let engine = guard.as_ref().unwrap();
        let dirty = engine.context_dirty_paths("code").unwrap();
        assert!(
            dirty.contains("Catalogs/Товары/Ext/ObjectModule.bsl"),
            "the owned module is marked context-dirty: {dirty:?}",
        );
        assert!(
            !dirty.contains("Catalogs/Другой/Ext/ObjectModule.bsl"),
            "an unrelated object's module is left untouched: {dirty:?}",
        );
        assert_eq!(dirty.len(), 1, "only the owned subtree is marked, not the whole tree");
        // The xml path is metadata context, not a body edit: nothing is marked dirty and
        // no whole-workspace walk ran.
        let snapshot = engine.workspace_overlay_dirty_paths_snapshot().unwrap();
        assert!(snapshot.is_empty(), "an xml edit marks no body dirty and triggers no walk");
    }

    /// A metadata `.xml` edit marks BOTH the object's owned modules (path convention) AND the
    /// REFERENCING modules — those whose `graph_context` embeds a read of the object — resolved
    /// through the persisted graph's inbound read edges. A module that references nothing about
    /// the object is left untouched.
    ///
    /// Revert-proof: drop the `resolve_referencing_module_rels` call in
    /// `mark_xml_affected_context_dirty` and the referencing module `Б` is no longer marked —
    /// the referencing assertion fails.
    #[test]
    fn search_sink_xml_marks_owned_and_referencing_modules_context_dirty() {
        use crate::change_hub::{ChangeEntry, ChangeKind};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        fs::write(workspace.join("Configuration.xml"), "<Configuration/>").unwrap();

        // Catalog Х with an OWNED object module (A), resolved by path convention.
        let xml = workspace.join("Catalogs/Х.xml");
        fs::create_dir_all(xml.parent().unwrap()).unwrap();
        fs::write(
            &xml,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-000000000001">
        <Properties><Name>Х</Name><CodeLength>9</CodeLength></Properties>
    </Catalog>
</MetaDataObject>"#,
        )
        .unwrap();
        let owned_a = workspace.join("Catalogs/Х/Ext/ObjectModule.bsl");
        fs::create_dir_all(owned_a.parent().unwrap()).unwrap();
        fs::write(&owned_a, "Процедура П() Экспорт\nКонецПроцедуры").unwrap();

        // Referencing common module Б reads the catalog (manager access + query) → inbound
        // read edges into `mdo/Catalog/Х`. Non-referencing module В reads nothing about it.
        write_common_module(
            &workspace,
            "Б",
            "&НаСервере\nПроцедура ЧитаетХ() Экспорт\nСправочники.Х.СоздатьЭлемент();\nЗапрос = \"ВЫБРАТЬ Код ИЗ Справочник.Х\";\nКонецПроцедуры",
        );
        write_common_module(
            &workspace,
            "В",
            "&НаСервере\nПроцедура НичегоНеЧитает() Экспорт\nВозврат;\nКонецПроцедуры",
        );

        // Build + publish the graph so the reverse lookup has real inbound edges to read.
        let out = crate::cache::graph_db_path(&workspace);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        let summary = crate::graph_db::build_graph_database(
            &workspace,
            &out,
            100,
            &crate::graph_db::GraphMeta {
                revision: 1,
                fingerprint: 0,
                files: 0,
                built_at: "t".to_owned(),
            },
        )
        .expect("graph builds");
        let graph = crate::graph::GraphState::for_workspace(workspace.clone());
        graph.adopt_prebuilt(1, 0, summary.modules);

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.enable_workspace_watcher_mode();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let entry = ChangeEntry {
            canonical: xml.clone(),
            raw: xml,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };
        SharedState::apply_search_drift(&engine_arc, &workspace, &[entry], false, &graph);

        let guard = engine_arc.lock().unwrap();
        let engine = guard.as_ref().unwrap();
        let dirty = engine.context_dirty_paths("code").unwrap();
        assert!(
            dirty.contains("Catalogs/Х/Ext/ObjectModule.bsl"),
            "the owned module is marked context-dirty: {dirty:?}",
        );
        assert!(
            dirty.contains("CommonModules/Б/Ext/Module.bsl"),
            "the referencing module (reads the catalog) is marked context-dirty: {dirty:?}",
        );
        assert!(
            !dirty.contains("CommonModules/В/Ext/Module.bsl"),
            "a module that references nothing about the catalog is left untouched: {dirty:?}",
        );
    }

    /// An `.xml` edit BEFORE any graph is published degrades: owned modules are still marked
    /// (path convention needs no graph) and referencing resolution is silently skipped — no
    /// error, no panic. The reverse lookup only rides a published graph.
    #[test]
    fn search_sink_xml_referencing_degrades_without_published_graph() {
        use crate::change_hub::{ChangeEntry, ChangeKind};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        fs::write(workspace.join("Configuration.xml"), "<Configuration/>").unwrap();
        let xml = workspace.join("Catalogs/Х.xml");
        fs::create_dir_all(xml.parent().unwrap()).unwrap();
        fs::write(&xml, "<MetaDataObject/>").unwrap();
        let owned_a = workspace.join("Catalogs/Х/Ext/ObjectModule.bsl");
        fs::create_dir_all(owned_a.parent().unwrap()).unwrap();
        fs::write(&owned_a, "Процедура П() Экспорт\nКонецПроцедуры").unwrap();
        // A would-be referencing module exists on disk but there is NO published graph, so it
        // is not discoverable and must not be marked.
        write_common_module(
            &workspace,
            "Б",
            "&НаСервере\nПроцедура ЧитаетХ() Экспорт\nСправочники.Х.СоздатьЭлемент();\nКонецПроцедуры",
        );

        // A workspace graph that has never been built → `snapshot()` returns None.
        let graph = crate::graph::GraphState::for_workspace(workspace.clone());

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.enable_workspace_watcher_mode();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let entry = ChangeEntry {
            canonical: xml.clone(),
            raw: xml,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };
        SharedState::apply_search_drift(&engine_arc, &workspace, &[entry], false, &graph);

        let guard = engine_arc.lock().unwrap();
        let engine = guard.as_ref().unwrap();
        let dirty = engine.context_dirty_paths("code").unwrap();
        assert!(
            dirty.contains("Catalogs/Х/Ext/ObjectModule.bsl"),
            "the owned module is still marked without a published graph: {dirty:?}",
        );
        assert!(
            !dirty.contains("CommonModules/Б/Ext/Module.bsl"),
            "referencing resolution is skipped with no published graph: {dirty:?}",
        );
    }

    /// ANY `.xml` directly at the workspace root (not only `Configuration.xml`), with no
    /// owned-module subtree, conservatively marks the whole collection context-dirty — a
    /// root descriptor change can shift any module's context.
    #[test]
    fn search_sink_root_xml_marks_whole_collection_context_dirty() {
        use crate::change_hub::{ChangeEntry, ChangeKind};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        let doc = |path: &str, sym: &str| IndexedDocument {
            collection: "code".to_owned(),
            path: path.to_owned(),
            symbol_name: sym.to_owned(),
            kind: "procedure".to_owned(),
            line_start: 0,
            line_end: 1,
            text: format!("Процедура {sym}()\nКонецПроцедуры"),
            content_hash: "h".to_owned(),
            graph_context: None,
        };
        engine
            .sync_indexed_documents_in_collection(
                "code",
                &[doc("A.bsl", "Ааа"), doc("B.bsl", "Ббб")],
                None,
            )
            .unwrap();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        // A root `.xml` NOT named Configuration.xml, with no sibling `<stem>/` subtree.
        let xml = workspace.join("SomePlugin.xml");
        fs::write(&xml, "<Root/>").unwrap();
        let entry = ChangeEntry {
            canonical: xml.clone(),
            raw: xml,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };
        SharedState::apply_search_drift(
            &engine_arc,
            &workspace,
            &[entry],
            false,
            &crate::graph::GraphState::disabled(),
        );

        let guard = engine_arc.lock().unwrap();
        let engine = guard.as_ref().unwrap();
        let dirty = engine.context_dirty_paths("code").unwrap();
        assert_eq!(dirty.len(), 2, "a root .xml marks every indexed file: {dirty:?}");
        assert!(dirty.contains("A.bsl") && dirty.contains("B.bsl"));
    }

    /// First byte offset of `needle` in `haystack`, or `None`.
    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    /// A minimal in-process HTTP embedding endpoint: answers `POST /v1/embeddings` with one
    /// fixed vector per input, so the real `Embedder` produces deterministic vectors without
    /// a live service. Returns the base URL; the detached server thread stops on process exit.
    fn spawn_mock_embedding_server(vector: Vec<f32>) -> String {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = Vec::new();
                let mut tmp = [0u8; 2048];
                let mut header_end: Option<usize> = None;
                let mut content_len = 0usize;
                loop {
                    let n = match stream.read(&mut tmp) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    buf.extend_from_slice(&tmp[..n]);
                    if header_end.is_none() {
                        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                            header_end = Some(pos + 4);
                            let headers = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
                            for line in headers.lines() {
                                if let Some(v) = line.strip_prefix("content-length:") {
                                    content_len = v.trim().parse().unwrap_or(0);
                                }
                            }
                        }
                    }
                    if header_end.is_some_and(|he| buf.len() >= he + content_len) {
                        break;
                    }
                }
                let body = header_end.map(|he| &buf[he..]).unwrap_or(&[]);
                let n_inputs = serde_json::from_slice::<serde_json::Value>(body)
                    .ok()
                    .and_then(|v| v.get("input").and_then(|i| i.as_array().map(|a| a.len())))
                    .unwrap_or(1);
                let data: Vec<serde_json::Value> = (0..n_inputs)
                    .map(|i| serde_json::json!({ "index": i, "embedding": vector }))
                    .collect();
                let resp_body = serde_json::json!({ "data": data }).to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    resp_body.len(),
                    resp_body,
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://{addr}")
    }

    /// A semantic `SearchConfig` pointed at `base_url`, dim 3.
    fn mock_semantic_config(base_url: &str) -> bsl_search::SearchConfig {
        bsl_search::SearchConfig {
            embedder: bsl_search::EmbedderConfig {
                base_url: base_url.to_owned(),
                model: "test-model".to_owned(),
                dim: Some(3),
                api_key: None,
                provider: None,
            },
            execution: bsl_search::EmbeddingExecutionPolicy::default(),
        }
    }

    /// Point `Self::embedding_config()` (env-driven) at the mock server for the duration of a
    /// test. Returns the guards (kept alive by the caller) plus the shared env lock guard.
    fn mock_embedding_env(base_url: &str) -> Vec<EnvVarGuard> {
        vec![
            EnvVarGuard::set("EMBEDDING_URL", base_url),
            EnvVarGuard::set("EMBEDDING_MODEL", "test-model"),
            EnvVarGuard::set("EMBEDDING_DIM", "3"),
        ]
    }

    /// A minimal CommonModule descriptor + body under `root`, so the module is declared and
    /// its method resolves to a durable graph id (`method/common/<name>/<method>`).
    fn write_common_module(root: &std::path::Path, name: &str, body: &str) {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core">
	<CommonModule uuid="00000000-0000-0000-0000-0000000000{id:02}">
		<Properties>
			<Name>{name}</Name>
			<Global>false</Global>
			<ClientManagedApplication>false</ClientManagedApplication>
			<Server>true</Server>
			<ExternalConnection>false</ExternalConnection>
			<ClientOrdinaryApplication>false</ClientOrdinaryApplication>
			<ServerCall>false</ServerCall>
			<Privileged>false</Privileged>
			<ReturnValuesReuse>DontUse</ReturnValuesReuse>
		</Properties>
	</CommonModule>
</MetaDataObject>"#,
            id = name.len(),
        );
        let xml_path = root.join(format!("CommonModules/{name}.xml"));
        fs::create_dir_all(xml_path.parent().unwrap()).unwrap();
        fs::write(&xml_path, xml).unwrap();
        let module_path = root.join(format!("CommonModules/{name}/Ext/Module.bsl"));
        fs::create_dir_all(module_path.parent().unwrap()).unwrap();
        fs::write(&module_path, body).unwrap();
    }

    /// An `.xml` drift whose owned module is marked context-dirty must NUDGE the graph to
    /// catch up — otherwise a search-only user (who never triggers a `graph` tool freshness
    /// check) leaves the marks unresolved forever. Asserting the graph left `Idle` with NO
    /// graph tool call. Disable the `graph.nudge_rebuild()` call → the graph stays `Idle` and
    /// this fails.
    #[test]
    fn search_sink_xml_drift_nudges_graph_to_catch_up() {
        use crate::change_hub::{ChangeEntry, ChangeKind};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");

        // An MDO descriptor with an owned module so the xml resolves to a real dirty mark.
        let owned = workspace.join("Catalogs/Товары/Ext/ObjectModule.bsl");
        fs::create_dir_all(owned.parent().unwrap()).unwrap();
        fs::write(&owned, "Процедура П()\nКонецПроцедуры").unwrap();
        let xml = workspace.join("Catalogs/Товары.xml");
        fs::write(&xml, "<MetaDataObject/>").unwrap();

        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.enable_workspace_watcher_mode();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let graph = crate::graph::GraphState::for_workspace(workspace.clone());
        assert_eq!(graph.status(), crate::graph::GraphStatus::Idle, "graph starts idle");

        let entry = ChangeEntry {
            canonical: xml.clone(),
            raw: xml,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };
        SharedState::apply_search_drift(&engine_arc, &workspace, &[entry], false, &graph);

        assert_ne!(
            graph.status(),
            crate::graph::GraphStatus::Idle,
            "the xml drift nudged the graph to catch up without any graph tool call",
        );
    }

    /// The re-embed kick: after a context refresh NULLs a chunk's embedding, the kick's
    /// background pass re-embeds it and swaps the fresh vector into the LIVE engine, so the
    /// re-contexted chunk answers semantic queries in-process (not only after a restart).
    /// Disable the spawn in `kick_context_reembed` → the live index stays empty and this fails.
    #[test]
    fn context_reembed_kick_fills_nulled_chunks_into_the_live_index() {
        use bsl_search::{Chunk, ChunkKind, Store};
        use std::time::{Duration, Instant};

        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mock = spawn_mock_embedding_server(vec![1.0, 0.0, 0.0]);
        let _env = mock_embedding_env(&mock);

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search.db");
        // A chunk with NO embedding (pending): the kick must fill it.
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file_with_context(
                    "Owned.bsl",
                    b"h1",
                    &[Chunk {
                        kind: ChunkKind::Procedure,
                        name: "Считать".to_owned(),
                        is_export: true,
                        annotations: vec![],
                        line_start: 0,
                        line_end: 1,
                        text: "Процедура Считать()\nКонецПроцедуры".to_owned(),
                    }],
                    None,
                    Some(&[Some("контекст".to_owned())]),
                )
                .unwrap();
        }
        let mut engine = SearchEngine::new(&db_path, mock_semantic_config(&mock)).unwrap();
        engine.set_workspace_root(dir.path());
        assert!(engine.has_semantic());
        // No vector is live yet: the query for the mock vector finds nothing.
        assert!(
            engine.search_with_embedding(&[1.0, 0.0, 0.0], 5, Some("code")).unwrap().is_empty(),
            "no vector is live before the kick",
        );
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let semantic_runtime = Arc::new(Mutex::new(super::SemanticRuntimeStatus::Indexing));
        let background_indexers = Arc::new(super::AtomicUsize::new(0));
        let index_progress = bsl_search::IndexProgress::new();
        let embed_flight = super::EmbedFlight::new();

        SharedState::kick_context_reembed(
            &engine_arc,
            &semantic_runtime,
            &background_indexers,
            &index_progress,
            &embed_flight,
        );

        // Poll until the background pass swaps the fresh vector into the live engine.
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut live = false;
        while Instant::now() < deadline {
            let hits = {
                let guard = engine_arc.lock().unwrap();
                guard
                    .as_ref()
                    .unwrap()
                    .search_with_embedding(&[1.0, 0.0, 0.0], 5, Some("code"))
                    .unwrap()
            };
            if hits.iter().any(|h| h.symbol_name == "Считать") {
                live = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(live, "the re-embed kick made the NULLed chunk answer with its new live vector");
    }

    /// Single-flight: a kick arriving while a pass is already claimed is absorbed — it spawns
    /// no second pass (the in-flight background count does not rise). Disable the
    /// `compare_exchange` claim guard → the second kick proceeds and the count rises.
    #[test]
    fn context_reembed_kick_is_single_flight() {
        use bsl_search::{Chunk, ChunkKind, Store};

        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mock = spawn_mock_embedding_server(vec![1.0, 0.0, 0.0]);
        let _env = mock_embedding_env(&mock);

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search.db");
        // A chunk that is already embedded (no pending), so a proceeding pass returns fast.
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file_with_context(
                    "Owned.bsl",
                    b"h1",
                    &[Chunk {
                        kind: ChunkKind::Procedure,
                        name: "Считать".to_owned(),
                        is_export: true,
                        annotations: vec![],
                        line_start: 0,
                        line_end: 1,
                        text: "Процедура Считать()\nКонецПроцедуры".to_owned(),
                    }],
                    Some(&[vec![1.0, 0.0, 0.0]]),
                    Some(&[Some("контекст".to_owned())]),
                )
                .unwrap();
        }
        let mut engine = SearchEngine::new(&db_path, mock_semantic_config(&mock)).unwrap();
        engine.set_workspace_root(dir.path());
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let semantic_runtime = Arc::new(Mutex::new(super::SemanticRuntimeStatus::Ready));
        let background_indexers = Arc::new(super::AtomicUsize::new(0));
        let index_progress = bsl_search::IndexProgress::new();
        // A pass is already in flight: the kick must be absorbed, spawning nothing.
        let embed_flight = super::EmbedFlight::in_flight_for_test();

        let before = background_indexers.load(super::Ordering::SeqCst);
        SharedState::kick_context_reembed(
            &engine_arc,
            &semantic_runtime,
            &background_indexers,
            &index_progress,
            &embed_flight,
        );
        assert_eq!(
            background_indexers.load(super::Ordering::SeqCst),
            before,
            "a kick while a pass is claimed spawns no second pass",
        );
        assert!(embed_flight.is_in_flight(), "the existing claim is untouched");
    }

    /// End-to-end lifecycle net through PRODUCTION wiring, using real components (real store,
    /// real graph build, real hub types, the real publish hook built by `build_publish_hook`)
    /// and faking only the embedder: an `.xml` drift → `apply_search_drift` marks the owned
    /// module + nudges the graph → the graph builds and its REAL publish fires the hook → the
    /// hook re-renders the stale context from the just-published graph, NULLs the embedding, and
    /// the shared embed pass re-embeds it into the live index. The refresh runs off the graph's
    /// own publish, not a hand-call, so the whole chain is exercised.
    #[test]
    fn xml_drift_lifecycle_refreshes_context_and_reembeds_into_live_index() {
        use crate::change_hub::{ChangeEntry, ChangeKind};
        use bsl_search::{Chunk, ChunkKind, Store};
        use std::time::{Duration, Instant};

        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mock = spawn_mock_embedding_server(vec![1.0, 0.0, 0.0]);
        let _env = mock_embedding_env(&mock);

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        // A real CommonModule so its method resolves to a graph id and the graph renders context.
        write_common_module(&workspace, "Сервер", "Функция Считать() Экспорт КонецФункции");
        let module_rel = "CommonModules/Сервер/Ext/Module.bsl";

        // The search chunk starts with a STALE stored context and a live embedding, so the
        // refresh detects a change, rewrites it, and NULLs the embedding.
        let db_path = workspace.join("search.db");
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file_with_context(
                    module_rel,
                    b"h1",
                    &[Chunk {
                        kind: ChunkKind::Function,
                        name: "Считать".to_owned(),
                        is_export: true,
                        annotations: vec![],
                        line_start: 0,
                        line_end: 1,
                        text: "Функция Считать() Экспорт КонецФункции".to_owned(),
                    }],
                    Some(&[vec![0.0, 1.0, 0.0]]),
                    Some(&[Some("СТАРЫЙ контекст".to_owned())]),
                )
                .unwrap();
        }
        let mut engine = SearchEngine::new(&db_path, mock_semantic_config(&mock)).unwrap();
        engine.set_workspace_root(&workspace);
        engine.enable_workspace_watcher_mode();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        // Wire the SAME publish hook the daemon builds, so the graph's real publish — not a
        // hand-call — drives the context refresh and re-embed.
        let semantic_runtime = Arc::new(Mutex::new(super::SemanticRuntimeStatus::Ready));
        let background_indexers = Arc::new(super::AtomicUsize::new(0));
        let index_progress = bsl_search::IndexProgress::new();
        let embed_flight = super::EmbedFlight::new();
        let hook = SharedState::build_publish_hook(
            Arc::clone(&engine_arc),
            workspace.clone(),
            Arc::clone(&semantic_runtime),
            Arc::clone(&background_indexers),
            Arc::clone(&index_progress),
            Arc::clone(&embed_flight),
        );
        let graph =
            crate::graph::GraphState::for_workspace(workspace.clone()).with_publish_hook(hook);
        // Wire the mark-seq source as the daemon does at boot, so the nudged build captures a
        // bound that covers the mark this drift stamps. An unwired build captures bound 0 and
        // clears nothing.
        graph.set_mark_seq_source(engine_arc.lock().unwrap().as_ref().unwrap().mark_seq_handle());

        // The xml drift marks the owned module context-dirty and nudges the graph; the nudged
        // build publishes and fires the hook automatically.
        let xml = workspace.join("CommonModules/Сервер.xml");
        let entry = ChangeEntry {
            canonical: xml.clone(),
            raw: xml,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };
        SharedState::apply_search_drift(&engine_arc, &workspace, &[entry], false, &graph);
        {
            let guard = engine_arc.lock().unwrap();
            let dirty = guard.as_ref().unwrap().context_dirty_paths("code").unwrap();
            assert!(
                dirty.contains(module_rel),
                "the owned module is marked context-dirty: {dirty:?}"
            );
        }
        assert_ne!(graph.status(), crate::graph::GraphStatus::Idle, "the graph nudge fired");

        // The stored context is re-rendered from the real graph (no longer the stale string),
        // and the re-embed kick swaps the fresh vector into the live index.
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut refreshed = false;
        while Instant::now() < deadline {
            let (ctx, hits) = {
                let guard = engine_arc.lock().unwrap();
                let engine = guard.as_ref().unwrap();
                let docs = engine.load_indexed_documents(Some("code")).unwrap();
                let ctx = docs
                    .iter()
                    .find(|d| d.symbol_name == "Считать")
                    .and_then(|d| d.graph_context.clone());
                let hits = engine.search_with_embedding(&[1.0, 0.0, 0.0], 5, Some("code")).unwrap();
                (ctx, hits)
            };
            let ctx_fresh =
                ctx.as_deref().is_some_and(|c| c != "СТАРЫЙ контекст" && c.contains("Signature"));
            if ctx_fresh && hits.iter().any(|h| h.symbol_name == "Считать") {
                refreshed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            refreshed,
            "the xml drift re-rendered the module's graph context and re-embedded it into the live index",
        );
    }

    /// While a graph drift is still being caught up (a follow-up reload is pending), the context
    /// refresh must DEFER: consuming the marks against the pre-drift publish would clear them
    /// against stale facts. Reverting the `drift_pending` guard makes the deferred call consume
    /// the mark and the survival assertion fails.
    #[test]
    fn context_refresh_defers_marks_while_graph_drift_is_pending() {
        use crate::change_hub::{ChangeEntry, ChangeKind};
        use bsl_search::{Chunk, ChunkKind, Store};
        use std::time::{Duration, Instant};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        write_common_module(&workspace, "Сервер", "Функция Считать() Экспорт КонецФункции");
        let module_rel = "CommonModules/Сервер/Ext/Module.bsl";

        // A chunk with a stale stored context and NO live embedding (so consumption needs no
        // embedder — the mark, not the vector, is under test).
        let db_path = workspace.join("search.db");
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file_with_context(
                    module_rel,
                    b"h1",
                    &[Chunk {
                        kind: ChunkKind::Function,
                        name: "Считать".to_owned(),
                        is_export: true,
                        annotations: vec![],
                        line_start: 0,
                        line_end: 1,
                        text: "Функция Считать() Экспорт КонецФункции".to_owned(),
                    }],
                    None,
                    Some(&[Some("СТАРЫЙ контекст".to_owned())]),
                )
                .unwrap();
        }
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(&workspace);
        engine.enable_workspace_watcher_mode();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        // Mark the owned module context-dirty (disabled graph → the nudge is a no-op here).
        let xml = workspace.join("CommonModules/Сервер.xml");
        let entry = ChangeEntry {
            canonical: xml.clone(),
            raw: xml,
            kind: ChangeKind::MaybeChanged,
            seq: 1,
        };
        SharedState::apply_search_drift(
            &engine_arc,
            &workspace,
            &[entry],
            false,
            &crate::graph::GraphState::disabled(),
        );
        {
            let g = engine_arc.lock().unwrap();
            assert!(
                g.as_ref().unwrap().context_dirty_paths("code").unwrap().contains(module_rel),
                "the owned module is marked context-dirty",
            );
        }

        // Build a real graph the refresh can read.
        let graph = crate::graph::GraphState::for_workspace(workspace.clone());
        graph.ensure_loading();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !matches!(graph.status(), crate::graph::GraphStatus::Ready { .. }) {
            if Instant::now() > deadline {
                panic!("graph did not build: {:?}", graph.status());
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let semantic_runtime = Arc::new(Mutex::new(super::SemanticRuntimeStatus::Ready));
        let background_indexers = Arc::new(super::AtomicUsize::new(0));
        let index_progress = bsl_search::IndexProgress::new();
        let embed_flight = super::EmbedFlight::new();

        // drift_pending = true → defer: the mark SURVIVES for the follow-up reload's publish.
        // An unbounded seq (i64::MAX) isolates the drift_pending skip from the seq bound.
        SharedState::refresh_search_contexts_after_graph(
            &engine_arc,
            &workspace,
            &semantic_runtime,
            &background_indexers,
            &index_progress,
            &embed_flight,
            crate::graph::GraphPublishSignal { drift_pending: true, build_start_seq: i64::MAX },
        );
        {
            let g = engine_arc.lock().unwrap();
            assert!(
                g.as_ref().unwrap().context_dirty_paths("code").unwrap().contains(module_rel),
                "a pending drift defers the refresh; the mark survives",
            );
        }

        // drift_pending = false → consume: the mark is cleared against the fresh graph.
        SharedState::refresh_search_contexts_after_graph(
            &engine_arc,
            &workspace,
            &semantic_runtime,
            &background_indexers,
            &index_progress,
            &embed_flight,
            crate::graph::GraphPublishSignal { drift_pending: false, build_start_seq: i64::MAX },
        );
        {
            let g = engine_arc.lock().unwrap();
            assert!(
                !g.as_ref().unwrap().context_dirty_paths("code").unwrap().contains(module_rel),
                "with no pending drift the mark is consumed",
            );
        }
    }

    /// A graph whose mark-seq source is NOT yet wired (the boot window before
    /// `set_mark_seq_source`) captures the unwired default bound (`0`), so its publish's
    /// consume clears NOTHING — never a mark stamped before the source existed. Reverting the
    /// unwired default from `0` back to `i64::MAX` makes the publish consume the mark and the
    /// survival assertion fails.
    #[test]
    fn an_unwired_graph_publish_cannot_clear_context_marks() {
        use bsl_search::{Chunk, ChunkKind, SearchEngine, Store};
        use std::sync::atomic::Ordering;
        use std::time::{Duration, Instant};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        write_common_module(&workspace, "Сервер", "Функция Считать() Экспорт КонецФункции");
        let module_rel = "CommonModules/Сервер/Ext/Module.bsl";

        let db_path = workspace.join("search.db");
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file_with_context(
                    module_rel,
                    b"h1",
                    &[Chunk {
                        kind: ChunkKind::Function,
                        name: "Считать".to_owned(),
                        is_export: true,
                        annotations: vec![],
                        line_start: 0,
                        line_end: 1,
                        text: "Функция Считать() Экспорт КонецФункции".to_owned(),
                    }],
                    None,
                    Some(&[Some("СТАРЫЙ контекст".to_owned())]),
                )
                .unwrap();
            // A mark left pending before any wired bound exists (seq 1).
            store.mark_context_dirty("code", module_rel).unwrap();
        }
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(&workspace);
        engine.enable_workspace_watcher_mode();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let semantic_runtime = Arc::new(Mutex::new(super::SemanticRuntimeStatus::Ready));
        let background_indexers = Arc::new(super::AtomicUsize::new(0));
        let index_progress = bsl_search::IndexProgress::new();
        let embed_flight = super::EmbedFlight::new();

        // The real refresh, wrapped so the test can wait until the publish actually fired the
        // hook (with bound 0 the consume has no observable side effect to poll on otherwise).
        let fired = Arc::new(super::AtomicUsize::new(0));
        let hook = {
            let engine_arc = Arc::clone(&engine_arc);
            let workspace = workspace.clone();
            let semantic_runtime = Arc::clone(&semantic_runtime);
            let background_indexers = Arc::clone(&background_indexers);
            let index_progress = Arc::clone(&index_progress);
            let embed_flight = Arc::clone(&embed_flight);
            let fired = Arc::clone(&fired);
            Arc::new(move |signal: crate::graph::GraphPublishSignal| {
                SharedState::refresh_search_contexts_after_graph(
                    &engine_arc,
                    &workspace,
                    &semantic_runtime,
                    &background_indexers,
                    &index_progress,
                    &embed_flight,
                    signal,
                );
                fired.fetch_add(1, Ordering::SeqCst);
            }) as Arc<dyn Fn(crate::graph::GraphPublishSignal) + Send + Sync>
        };

        // The graph is never wired to a mark-seq source: its build captures the unwired default.
        let graph =
            crate::graph::GraphState::for_workspace(workspace.clone()).with_publish_hook(hook);
        graph.ensure_loading();
        let deadline = Instant::now() + Duration::from_secs(30);
        while fired.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(fired.load(Ordering::SeqCst) >= 1, "the build published and fired the hook");

        let guard = engine_arc.lock().unwrap();
        assert!(
            guard.as_ref().unwrap().context_dirty_paths("code").unwrap().contains(module_rel),
            "an unwired build's publish (bound 0) clears no marks; the mark survives",
        );
    }

    /// Marks a PRIOR daemon run left in `context_dirty` survive the boot build's unwired publish
    /// (as the test above shows), then are consumed by the explicit leftover pickup once the
    /// mark-seq source is wired: a wired-bound consume against the already-fresh boot graph.
    /// Removing the `consume_leftover_marks` call leaves the mark stranded and the final
    /// assertion fails.
    #[test]
    fn leftover_marks_are_consumed_after_boot_wiring() {
        use bsl_search::{Chunk, ChunkKind, SearchEngine, Store};
        use std::sync::atomic::Ordering;
        use std::time::{Duration, Instant};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        write_common_module(&workspace, "Сервер", "Функция Считать() Экспорт КонецФункции");
        let module_rel = "CommonModules/Сервер/Ext/Module.bsl";

        let db_path = workspace.join("search.db");
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file_with_context(
                    module_rel,
                    b"h1",
                    &[Chunk {
                        kind: ChunkKind::Function,
                        name: "Считать".to_owned(),
                        is_export: true,
                        annotations: vec![],
                        line_start: 0,
                        line_end: 1,
                        text: "Функция Считать() Экспорт КонецФункции".to_owned(),
                    }],
                    None,
                    Some(&[Some("СТАРЫЙ контекст".to_owned())]),
                )
                .unwrap();
            store.mark_context_dirty("code", module_rel).unwrap();
        }
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(&workspace);
        engine.enable_workspace_watcher_mode();
        let mark_seq = engine.mark_seq_handle();
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let semantic_runtime = Arc::new(Mutex::new(super::SemanticRuntimeStatus::Ready));
        let background_indexers = Arc::new(super::AtomicUsize::new(0));
        let index_progress = bsl_search::IndexProgress::new();
        let embed_flight = super::EmbedFlight::new();
        let fired = Arc::new(super::AtomicUsize::new(0));
        let hook = {
            let engine_arc = Arc::clone(&engine_arc);
            let workspace = workspace.clone();
            let semantic_runtime = Arc::clone(&semantic_runtime);
            let background_indexers = Arc::clone(&background_indexers);
            let index_progress = Arc::clone(&index_progress);
            let embed_flight = Arc::clone(&embed_flight);
            let fired = Arc::clone(&fired);
            Arc::new(move |signal: crate::graph::GraphPublishSignal| {
                SharedState::refresh_search_contexts_after_graph(
                    &engine_arc,
                    &workspace,
                    &semantic_runtime,
                    &background_indexers,
                    &index_progress,
                    &embed_flight,
                    signal,
                );
                fired.fetch_add(1, Ordering::SeqCst);
            }) as Arc<dyn Fn(crate::graph::GraphPublishSignal) + Send + Sync>
        };

        // Boot: the graph builds and publishes while UNWIRED, so the leftover mark survives.
        let graph =
            crate::graph::GraphState::for_workspace(workspace.clone()).with_publish_hook(hook);
        graph.ensure_loading();
        let deadline = Instant::now() + Duration::from_secs(30);
        while fired.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(fired.load(Ordering::SeqCst) >= 1, "the boot build published and fired the hook");
        assert!(
            engine_arc
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .context_dirty_paths("code")
                .unwrap()
                .contains(module_rel),
            "the leftover mark survives the unwired boot publish",
        );

        // Boot wiring, then the explicit pickup: a consume bounded by the seq captured at
        // observation time clears the leftover mark synchronously (the graph is already `Ready`).
        let leftover_bound = mark_seq.load(Ordering::SeqCst);
        graph.set_mark_seq_source(mark_seq);
        graph.consume_leftover_marks(leftover_bound);

        assert!(
            !engine_arc
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .context_dirty_paths("code")
                .unwrap()
                .contains(module_rel),
            "the leftover pickup consumed the mark with the wired bound",
        );
    }

    /// The leftover pickup must clear ONLY marks that existed when its bound was captured. A
    /// drift the running search sink stamps AFTER the capture (a higher mark seq) must survive
    /// the pickup — its own nudge→publish will resolve it against a graph that reflects it.
    /// Reverting the direct (`Ready`) fire path to a LIVE `current_mark_seq()` read makes the
    /// pickup clear the newer mark too, and the survival assertion fails.
    #[test]
    fn a_newer_mark_survives_the_leftover_pickups_captured_bound() {
        use bsl_search::{Chunk, ChunkKind, SearchEngine, Store};
        use std::sync::atomic::Ordering;
        use std::time::{Duration, Instant};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        write_common_module(&workspace, "Сервер", "Функция Считать() Экспорт КонецФункции");
        let leftover_rel = "CommonModules/Сервер/Ext/Module.bsl";
        // A path the search sink will freshly mark AFTER the bound is captured; never indexed,
        // it only needs to resolve to a workspace `.bsl` to receive a higher-seq mark.
        let newer_rel = "CommonModules/Клиент/Ext/Module.bsl";

        let db_path = workspace.join("search.db");
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file_with_context(
                    leftover_rel,
                    b"h1",
                    &[Chunk {
                        kind: ChunkKind::Function,
                        name: "Считать".to_owned(),
                        is_export: true,
                        annotations: vec![],
                        line_start: 0,
                        line_end: 1,
                        text: "Функция Считать() Экспорт КонецФункции".to_owned(),
                    }],
                    None,
                    Some(&[Some("СТАРЫЙ контекст".to_owned())]),
                )
                .unwrap();
            // The leftover mark a prior run left pending (seq 1).
            store.mark_context_dirty("code", leftover_rel).unwrap();
        }
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(&workspace);
        engine.enable_workspace_watcher_mode();
        let mark_seq = engine.mark_seq_handle();
        // The bound captured at observation time: the high-water at seq 1 (the leftover only).
        let leftover_bound = mark_seq.load(Ordering::SeqCst);
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let semantic_runtime = Arc::new(Mutex::new(super::SemanticRuntimeStatus::Ready));
        let background_indexers = Arc::new(super::AtomicUsize::new(0));
        let index_progress = bsl_search::IndexProgress::new();
        let embed_flight = super::EmbedFlight::new();
        let fired = Arc::new(super::AtomicUsize::new(0));
        let hook = {
            let engine_arc = Arc::clone(&engine_arc);
            let workspace = workspace.clone();
            let semantic_runtime = Arc::clone(&semantic_runtime);
            let background_indexers = Arc::clone(&background_indexers);
            let index_progress = Arc::clone(&index_progress);
            let embed_flight = Arc::clone(&embed_flight);
            let fired = Arc::clone(&fired);
            Arc::new(move |signal: crate::graph::GraphPublishSignal| {
                SharedState::refresh_search_contexts_after_graph(
                    &engine_arc,
                    &workspace,
                    &semantic_runtime,
                    &background_indexers,
                    &index_progress,
                    &embed_flight,
                    signal,
                );
                fired.fetch_add(1, Ordering::SeqCst);
            }) as Arc<dyn Fn(crate::graph::GraphPublishSignal) + Send + Sync>
        };

        // Boot: build+publish while UNWIRED, so the leftover mark survives, then wire the source.
        let graph =
            crate::graph::GraphState::for_workspace(workspace.clone()).with_publish_hook(hook);
        graph.ensure_loading();
        let deadline = Instant::now() + Duration::from_secs(30);
        while fired.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(fired.load(Ordering::SeqCst) >= 1, "the boot build published and fired the hook");
        graph.set_mark_seq_source(mark_seq);

        // The search sink stamps a NEW drift (seq 2) after the bound was captured — as it would
        // between publishing the engine and reaching its own nudge→publish.
        {
            let guard = engine_arc.lock().unwrap();
            let engine = guard.as_ref().unwrap();
            assert!(
                engine.mark_workspace_path_context_dirty(workspace.join(newer_rel)).unwrap(),
                "the newer path resolves to a workspace .bsl and receives a higher-seq mark",
            );
        }

        // The explicit pickup fires on the already-`Ready` graph with the CAPTURED bound.
        graph.consume_leftover_marks(leftover_bound);

        let guard = engine_arc.lock().unwrap();
        let dirty = guard.as_ref().unwrap().context_dirty_paths("code").unwrap();
        assert!(!dirty.contains(leftover_rel), "the leftover mark is consumed by the pickup");
        assert!(
            dirty.contains(newer_rel),
            "the newer mark (stamped after the captured bound) survives the pickup",
        );
    }

    /// The deferred (`Loading`) pickup path: arming while the graph is not yet `Ready` stores the
    /// captured bound, and the build's own publish re-fires the consume with THAT stored bound. A
    /// newer mark stamped after the capture must still survive. Reverting the deferred fire in
    /// `notify_published` to a live `current_mark_seq()` read (which on this unwired graph is `0`)
    /// makes the deferred consume clear nothing, so the leftover-consumed assertion fails.
    #[test]
    fn a_newer_mark_survives_the_deferred_leftover_pickup() {
        use bsl_search::{Chunk, ChunkKind, SearchEngine, Store};
        use std::sync::atomic::Ordering;
        use std::time::{Duration, Instant};

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        write_common_module(&workspace, "Сервер", "Функция Считать() Экспорт КонецФункции");
        let leftover_rel = "CommonModules/Сервер/Ext/Module.bsl";
        let newer_rel = "CommonModules/Клиент/Ext/Module.bsl";

        let db_path = workspace.join("search.db");
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file_with_context(
                    leftover_rel,
                    b"h1",
                    &[Chunk {
                        kind: ChunkKind::Function,
                        name: "Считать".to_owned(),
                        is_export: true,
                        annotations: vec![],
                        line_start: 0,
                        line_end: 1,
                        text: "Функция Считать() Экспорт КонецФункции".to_owned(),
                    }],
                    None,
                    Some(&[Some("СТАРЫЙ контекст".to_owned())]),
                )
                .unwrap();
            store.mark_context_dirty("code", leftover_rel).unwrap();
        }
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(&workspace);
        engine.enable_workspace_watcher_mode();
        // Capture the bound (seq 1) before stamping the newer mark.
        let leftover_bound = engine.mark_seq_handle().load(Ordering::SeqCst);
        // The newer drift (seq 2), stamped before the engine is shared.
        assert!(
            engine.mark_workspace_path_context_dirty(workspace.join(newer_rel)).unwrap(),
            "the newer path resolves to a workspace .bsl and receives a higher-seq mark",
        );
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let semantic_runtime = Arc::new(Mutex::new(super::SemanticRuntimeStatus::Ready));
        let background_indexers = Arc::new(super::AtomicUsize::new(0));
        let index_progress = bsl_search::IndexProgress::new();
        let embed_flight = super::EmbedFlight::new();
        let fired = Arc::new(super::AtomicUsize::new(0));
        let hook = {
            let engine_arc = Arc::clone(&engine_arc);
            let workspace = workspace.clone();
            let semantic_runtime = Arc::clone(&semantic_runtime);
            let background_indexers = Arc::clone(&background_indexers);
            let index_progress = Arc::clone(&index_progress);
            let embed_flight = Arc::clone(&embed_flight);
            let fired = Arc::clone(&fired);
            Arc::new(move |signal: crate::graph::GraphPublishSignal| {
                SharedState::refresh_search_contexts_after_graph(
                    &engine_arc,
                    &workspace,
                    &semantic_runtime,
                    &background_indexers,
                    &index_progress,
                    &embed_flight,
                    signal,
                );
                fired.fetch_add(1, Ordering::SeqCst);
            }) as Arc<dyn Fn(crate::graph::GraphPublishSignal) + Send + Sync>
        };

        // The graph is `Idle` (never wired): arming the pickup here stores the bound but cannot
        // fire, so the build's own publish runs the deferred consume with the stored bound.
        let graph =
            crate::graph::GraphState::for_workspace(workspace.clone()).with_publish_hook(hook);
        graph.consume_leftover_marks(leftover_bound);
        graph.ensure_loading();
        let deadline = Instant::now() + Duration::from_secs(30);
        while fired.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(fired.load(Ordering::SeqCst) >= 1, "the build published and fired the hook");

        let guard = engine_arc.lock().unwrap();
        let dirty = guard.as_ref().unwrap().context_dirty_paths("code").unwrap();
        assert!(
            !dirty.contains(leftover_rel),
            "the deferred pickup consumed the leftover mark with the stored bound",
        );
        assert!(
            dirty.contains(newer_rel),
            "the newer mark (stamped after the captured bound) survives the deferred pickup",
        );
    }

    /// The shared embed single-flight: exactly one owner runs; a caller that loses the claim
    /// records a rerun that makes the owner loop again. Reverting the loop (ignoring the rerun in
    /// `finish_pass`) makes the first `finish_pass` return false and the assertion fails.
    #[test]
    fn embed_flight_is_single_flight_with_a_rerun_loop() {
        let flight = super::EmbedFlight::new();
        assert!(flight.claim(), "the first caller wins the claim");
        flight.begin_pass();
        assert!(!flight.claim(), "a concurrent caller loses and records a rerun");
        assert!(flight.finish_pass(), "a rerun requested during the pass loops the owner again");
        flight.begin_pass();
        assert!(!flight.finish_pass(), "no rerun requested → the claim is released");
        assert!(flight.claim(), "the released flight can be claimed again");
    }

    /// A NULL chunk created AFTER the pass has read the store still gets embedded, because the
    /// owner loops on the recorded rerun and the final `set_vector_index` reflects the latest
    /// store state. Reverting the rerun loop leaves the mid-flight chunk unembedded and it never
    /// answers the query.
    #[test]
    fn embed_pass_rerun_loop_embeds_a_chunk_nulled_mid_flight() {
        use bsl_search::{Chunk, ChunkKind, Store};
        use std::time::{Duration, Instant};

        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mock = spawn_mock_embedding_server(vec![1.0, 0.0, 0.0]);
        let _env = mock_embedding_env(&mock);

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search.db");
        let chunk = |name: &str| Chunk {
            kind: ChunkKind::Procedure,
            name: name.to_owned(),
            is_export: true,
            annotations: vec![],
            line_start: 0,
            line_end: 1,
            text: format!("Процедура {name}()\nКонецПроцедуры"),
        };
        // Chunk A is NULL at the start; chunk B is added mid-flight by the post-pass hook.
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file_with_context(
                    "A.bsl",
                    b"ha",
                    &[chunk("Альфа")],
                    None,
                    Some(&[Some("ctx".to_owned())]),
                )
                .unwrap();
        }
        let mut engine = SearchEngine::new(&db_path, mock_semantic_config(&mock)).unwrap();
        engine.set_workspace_root(dir.path());
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let embed_flight = super::EmbedFlight::new();

        // A one-shot hook fired after the first iteration installs its index: it creates a NULL
        // chunk B and contends for the claim, recording a rerun so the owner loops for B.
        struct ResetHook;
        impl Drop for ResetHook {
            fn drop(&mut self) {
                *super::EMBED_POST_PASS_HOOK.lock().unwrap_or_else(|p| p.into_inner()) = None;
            }
        }
        let _reset = ResetHook;
        {
            let flight_for_hook = Arc::clone(&embed_flight);
            let mut fired = false;
            *super::EMBED_POST_PASS_HOOK.lock().unwrap() =
                Some(Box::new(move |db: &std::path::Path| {
                    if fired {
                        return;
                    }
                    fired = true;
                    let mut store = Store::open(db).unwrap();
                    store
                        .reindex_file_with_context(
                            "B.bsl",
                            b"hb",
                            &[Chunk {
                                kind: ChunkKind::Procedure,
                                name: "Бета".to_owned(),
                                is_export: true,
                                annotations: vec![],
                                line_start: 0,
                                line_end: 1,
                                text: "Процедура Бета()\nКонецПроцедуры".to_owned(),
                            }],
                            None,
                            Some(&[Some("ctx".to_owned())]),
                        )
                        .unwrap();
                    flight_for_hook.claim();
                }));
        }

        let semantic_runtime = Arc::new(Mutex::new(super::SemanticRuntimeStatus::Indexing));
        let background_indexers = Arc::new(super::AtomicUsize::new(0));
        let index_progress = bsl_search::IndexProgress::new();
        SharedState::spawn_embed_pass(
            Arc::clone(&engine_arc),
            semantic_runtime,
            background_indexers,
            index_progress,
            Arc::clone(&embed_flight),
            db_path.clone(),
            mock_semantic_config(&mock),
        );

        // Both A and B must answer the query: A from iteration 1, B from the rerun iteration.
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut both = false;
        while Instant::now() < deadline {
            let hits = {
                let guard = engine_arc.lock().unwrap();
                guard
                    .as_ref()
                    .unwrap()
                    .search_with_embedding(&[1.0, 0.0, 0.0], 5, Some("code"))
                    .unwrap()
            };
            let has_a = hits.iter().any(|h| h.symbol_name == "Альфа");
            let has_b = hits.iter().any(|h| h.symbol_name == "Бета");
            if has_a && has_b {
                both = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(both, "the rerun loop embedded the chunk created after the pass started");
    }

    /// A panicking embed pass leaves the runtime `Failed`, never stuck `Indexing`, and releases
    /// the shared flight claim (RAII guards fire on unwind). Reverting the status guard leaves the
    /// runtime stuck `Indexing`.
    #[test]
    fn embed_pass_panic_leaves_status_failed_and_releases_flight() {
        use bsl_search::{Chunk, ChunkKind, Store};
        use std::time::{Duration, Instant};

        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mock = spawn_mock_embedding_server(vec![1.0, 0.0, 0.0]);
        let _env = mock_embedding_env(&mock);

        struct ResetPanic;
        impl Drop for ResetPanic {
            fn drop(&mut self) {
                super::FORCE_EMBED_PASS_PANIC.store(false, super::Ordering::SeqCst);
            }
        }
        super::FORCE_EMBED_PASS_PANIC.store(true, super::Ordering::SeqCst);
        let _reset = ResetPanic;

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("search.db");
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file_with_context(
                    "Owned.bsl",
                    b"h1",
                    &[Chunk {
                        kind: ChunkKind::Procedure,
                        name: "Считать".to_owned(),
                        is_export: true,
                        annotations: vec![],
                        line_start: 0,
                        line_end: 1,
                        text: "Процедура Считать()\nКонецПроцедуры".to_owned(),
                    }],
                    None,
                    Some(&[Some("ctx".to_owned())]),
                )
                .unwrap();
        }
        let mut engine = SearchEngine::new(&db_path, mock_semantic_config(&mock)).unwrap();
        engine.set_workspace_root(dir.path());
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let semantic_runtime = Arc::new(Mutex::new(super::SemanticRuntimeStatus::Indexing));
        let background_indexers = Arc::new(super::AtomicUsize::new(0));
        let index_progress = bsl_search::IndexProgress::new();
        let embed_flight = super::EmbedFlight::new();
        SharedState::kick_context_reembed(
            &engine_arc,
            &semantic_runtime,
            &background_indexers,
            &index_progress,
            &embed_flight,
        );

        let deadline = Instant::now() + Duration::from_secs(20);
        let mut failed = false;
        while Instant::now() < deadline {
            let status = semantic_runtime.lock().unwrap_or_else(|p| p.into_inner()).clone();
            if matches!(status, super::SemanticRuntimeStatus::Failed(_)) {
                failed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(failed, "a panicking embed pass ends Failed, not stuck Indexing");
        // Give the guards a beat to run on unwind, then assert the claim was released.
        let deadline = Instant::now() + Duration::from_secs(5);
        while embed_flight.is_in_flight() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!embed_flight.is_in_flight(), "the flight claim is released after the panic");
    }

    /// A partial rescan walk (an error mid-walk) must NOT reconcile: `present` is missing healthy
    /// files, so deleting stored files against it would evict live data. Only a clean walk
    /// reconciles. Reverting the walk-error guard deletes the stored file on the errored walk.
    #[test]
    fn rescan_walk_error_skips_reconcile_and_keeps_stored_files() {
        use bsl_search::{Chunk, ChunkKind, Store};

        // This test toggles the process-global `FORCE_REWALK_WALK_ERROR` seam; serialize against the
        // boot-reconcile tests (which read it) so its forced error can't leak into their walk.
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let db_path = dir.path().join("search.db");
        {
            let mut store = Store::open(&db_path).unwrap();
            store
                .reindex_file(
                    "Gone.bsl",
                    b"ha",
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
                )
                .unwrap();
        }
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.enable_workspace_watcher_mode();
        assert_eq!(engine.file_count().unwrap(), 1, "the stored file is present");
        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        struct ResetWalkErr;
        impl Drop for ResetWalkErr {
            fn drop(&mut self) {
                super::FORCE_REWALK_WALK_ERROR.store(false, super::Ordering::SeqCst);
            }
        }

        // Errored walk: reconcile is skipped, so the stored (disk-absent) file SURVIVES.
        {
            super::FORCE_REWALK_WALK_ERROR.store(true, super::Ordering::SeqCst);
            let _reset = ResetWalkErr;
            SharedState::rewalk_workspace_bsl_dirty(&engine_arc, &workspace);
            assert_eq!(
                engine_arc.lock().unwrap().as_ref().unwrap().file_count().unwrap(),
                1,
                "a partial walk must not reconcile healthy files out of the store",
            );
        }

        // Clean walk: the stored-but-absent file is reconciled out.
        SharedState::rewalk_workspace_bsl_dirty(&engine_arc, &workspace);
        assert_eq!(
            engine_arc.lock().unwrap().as_ref().unwrap().file_count().unwrap(),
            0,
            "a clean walk reconciles the deleted file out",
        );
    }

    /// Write a common module (descriptor XML + `Ext/Module.bsl`) under `base`.
    fn write_common_module_tree(base: &std::path::Path, name: &str, body: &str) {
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\">\n\
             \t<CommonModule uuid=\"00000000-0000-0000-0000-000000000001\">\n\
             \t\t<Properties><Name>{name}</Name><Server>true</Server></Properties>\n\
             \t</CommonModule>\n\
             </MetaDataObject>\n"
        );
        fs::create_dir_all(base.join("CommonModules").join(name).join("Ext")).unwrap();
        fs::write(base.join("CommonModules").join(format!("{name}.xml")), xml).unwrap();
        fs::write(base.join("CommonModules").join(name).join("Ext").join("Module.bsl"), body)
            .unwrap();
    }

    /// The overlay keys dirty paths relative to the ENGINE root (the nested config source root),
    /// while the resident is indexed under the OUTER workspace root. `prefetch_resident_overlay`
    /// must resolve each dirty rel to an absolute path against the engine root before asking the
    /// resident, so a nested config (every real workspace) actually gets a resident-fed reindex.
    /// Reverting the absolute-join (passing the rel verbatim) leaves the resident-fed count at 0.
    #[test]
    fn prefetch_resident_overlay_feeds_nested_config_from_resident() {
        use crate::diagnostics_state::{
            DiagnosticsState, DiagnosticsStatus, ResidentModuleSnapshotSource,
        };
        use std::time::{Duration, Instant};

        let dir = tempdir().unwrap();
        let outer = dir.path().to_path_buf();
        let cf = outer.join("src").join("cf");
        fs::create_dir_all(&cf).unwrap();
        fs::write(
            cf.join("Configuration.xml"),
            "<Configuration><Name>Конфа</Name></Configuration>",
        )
        .unwrap();
        write_common_module_tree(
            &cf,
            "Сервер",
            "&НаСервере\nФункция Ч() Экспорт Возврат 1; КонецФункции\n",
        );
        let module = cf.join("CommonModules").join("Сервер").join("Ext").join("Module.bsl");

        // Overlay engine rooted at the NESTED config root, so `source_path != outer`.
        let mut engine = SearchEngine::fts_only(&outer.join("search.db")).unwrap();
        engine.set_workspace_root(cf.clone());
        engine.enable_workspace_watcher_mode();
        engine.prime_workspace_overlay().unwrap();

        // The file grows on disk so the reindex genuinely rebuilds it (fingerprint differs).
        fs::write(
            &module,
            "&НаСервере\nФункция Ч() Экспорт Возврат 1; КонецФункции\n\
             Процедура Ещё() Экспорт КонецПроцедуры\n",
        )
        .unwrap();

        // The resident is built against the OUTER root AFTER the edit, so it holds the new bytes.
        let diagnostics = DiagnosticsState::for_workspace(outer.clone());
        diagnostics.ensure_loading();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !matches!(diagnostics.status(), DiagnosticsStatus::Ready { .. }) {
            assert!(Instant::now() < deadline, "the resident did not become ready");
            std::thread::sleep(Duration::from_millis(20));
        }

        let source: Arc<dyn bsl_search::ModuleSnapshotSource> =
            Arc::new(ResidentModuleSnapshotSource::new(diagnostics.clone()));
        engine.set_module_snapshot_source(source);
        assert!(
            engine.mark_workspace_path_dirty(&module).unwrap(),
            "the nested module marks dirty"
        );

        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));
        SharedState::prefetch_resident_overlay(&engine_arc);

        let fed = engine_arc
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .workspace_overlay_resident_fed_count()
            .unwrap();
        assert_eq!(
            fed, 1,
            "a nested-config dirty path must be served from the resident's shared parse",
        );
    }

    /// Search and diagnostics drain independent hub cursors, so a just-edited file leaves the
    /// resident BEHIND disk. `prefetch_resident_overlay` must catch the resident up on pending
    /// drift FIRST, so the snapshot text matches disk and the reindex is resident-fed rather than
    /// falling back to a disk read. Reverting the `catch_up` call leaves the resident stale, the
    /// byte-compare misses, and the resident-fed count stays 0.
    #[test]
    fn prefetch_resident_overlay_catches_up_stale_resident_before_reading() {
        use crate::change_hub::WorkspaceChangeHub;
        use crate::diagnostics_state::{
            DiagnosticsState, DiagnosticsStatus, ResidentModuleSnapshotSource,
        };
        use std::time::{Duration, Instant};

        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::write(
            root.join("Configuration.xml"),
            "<Configuration><Name>Конфа</Name></Configuration>",
        )
        .unwrap();
        write_common_module_tree(
            &root,
            "Сервер",
            "&НаСервере\nФункция Ч() Экспорт Возврат 1; КонецФункции\n",
        );
        let module = root.join("CommonModules").join("Сервер").join("Ext").join("Module.bsl");

        let hub = WorkspaceChangeHub::start(vec![root.clone()]);
        assert!(hub.wait_until_watching(Duration::from_secs(5)), "the hub must arm");
        let mut observer = hub.subscribe();

        let mut engine = SearchEngine::fts_only(&root.join("search.db")).unwrap();
        engine.set_workspace_root(root.clone());
        engine.enable_workspace_watcher_mode();
        engine.prime_workspace_overlay().unwrap();

        // Resident built at v1, wired to the SAME hub, but it never polls drift on its own.
        let diagnostics =
            DiagnosticsState::for_workspace(root.clone()).with_change_hub(hub.clone());
        diagnostics.ensure_loading();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !matches!(diagnostics.status(), DiagnosticsStatus::Ready { .. }) {
            assert!(Instant::now() < deadline, "the resident did not become ready");
            std::thread::sleep(Duration::from_millis(20));
        }

        let source: Arc<dyn bsl_search::ModuleSnapshotSource> =
            Arc::new(ResidentModuleSnapshotSource::new(diagnostics.clone()));
        engine.set_module_snapshot_source(source);

        // Edit on disk (v2, longer): the resident's recorded revision is now stale.
        std::thread::sleep(Duration::from_millis(10));
        fs::write(
            &module,
            "&НаСервере\nФункция Ч() Экспорт Возврат 2; КонецФункции\n\
             Процедура Ещё() Экспорт КонецПроцедуры\n",
        )
        .unwrap();
        assert!(engine.mark_workspace_path_dirty(&module).unwrap());

        // Wait until the hub delivered the edit, so the diagnostics cursor drains it in `catch_up`.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut delivered = false;
        while Instant::now() < deadline {
            let batch = hub.drain(observer);
            observer = batch.cursor;
            if batch.entries.iter().any(|e| e.raw.to_string_lossy().ends_with("Module.bsl")) {
                delivered = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(delivered, "the hub delivered the edit");

        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));
        SharedState::prefetch_resident_overlay(&engine_arc);

        let fed = engine_arc
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .workspace_overlay_resident_fed_count()
            .unwrap();
        assert_eq!(
            fed, 1,
            "catch_up must reconcile the stale resident so the snapshot matches disk (fed reindex)",
        );
    }

    /// The per-query prefetch is capped: marking N + k paths dirty serves exactly N from the
    /// shared parse in one prefetch, and the remaining k stay dirty for the lazy disk path / a
    /// later prefetch. This bounds the query-path work S2 adds.
    #[test]
    fn prefetch_resident_overlay_caps_paths_per_query() {
        use super::MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY;
        use bsl_search::{ModuleSnapshot, ModuleSnapshotSource, SnapshotFetch};

        struct DiskFakeSource;
        impl ModuleSnapshotSource for DiskFakeSource {
            fn text_and_parse(&self, path: &str) -> SnapshotFetch {
                match std::fs::read_to_string(path) {
                    Ok(text) => {
                        let root = parser::parse(&text).syntax_node();
                        SnapshotFetch::Fetched(ModuleSnapshot { text: text.into(), root })
                    }
                    Err(_) => SnapshotFetch::Unavailable,
                }
            }
        }

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let mut engine = SearchEngine::fts_only(&workspace.join("search.db")).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.enable_workspace_watcher_mode();
        engine.prime_workspace_overlay().unwrap();
        engine.set_module_snapshot_source(Arc::new(DiskFakeSource));

        let extra = 3usize;
        let total = MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY + extra;
        for i in 0..total {
            let rel = format!("Module{i}.bsl");
            fs::write(workspace.join(&rel), format!("Процедура П{i}()\nКонецПроцедуры\n")).unwrap();
            assert!(engine.mark_workspace_path_dirty(workspace.join(&rel)).unwrap());
        }

        let engine_arc: super::SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));
        SharedState::prefetch_resident_overlay(&engine_arc);

        let guard = engine_arc.lock().unwrap();
        let engine = guard.as_ref().unwrap();
        assert_eq!(
            engine.workspace_overlay_resident_fed_count().unwrap(),
            MAX_RESIDENT_PREFETCH_PATHS_PER_QUERY,
            "exactly the per-query cap is served from the shared parse",
        );
        assert_eq!(
            engine.workspace_overlay_dirty_paths().unwrap().len(),
            extra,
            "paths beyond the cap stay dirty for the lazy disk path / a later prefetch",
        );
    }

    /// End-to-end through the REAL `SharedState::workspace` boot on a local SQLite workspace (no
    /// Postgres baseline, no embedder — the common local setup): the boot must bring the workspace
    /// overlay online so a post-boot edit is served fresh through the ordinary query path, fed from
    /// the resident's shared parse. NO hand-priming — the boot itself has to initialize the overlay,
    /// or the resident-fed incremental reindex (`reindex_dirty_from_snapshots` no-ops on
    /// `!initialized`) is unreachable and the edit is never served. Reverting the boot's overlay-init
    /// wiring leaves the overlay uninitialized, the reindex a no-op, the resident-fed count 0, and
    /// the fresh symbol unfound — this test then fails.
    #[test]
    fn workspace_boot_initializes_overlay_so_local_edits_serve_fresh_from_resident() {
        use crate::diagnostics_state::DiagnosticsStatus;
        use std::time::{Duration, Instant};

        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        // No embedder configured -> FTS-only local mode, the branch whose overlay was never
        // initialized before this fix.
        let _embedding_url = EnvVarGuard::unset("EMBEDDING_URL");
        let _embedding_model = EnvVarGuard::unset("EMBEDDING_MODEL");

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        fs::write(
            workspace.join("Configuration.xml"),
            "<Configuration><Name>Конфа</Name></Configuration>",
        )
        .unwrap();
        // v1 baseline body; the boot ingests it into the store.
        write_common_module_tree(
            &workspace,
            "Сервер",
            "&НаСервере\nФункция Ч() Экспорт Возврат 1; КонецФункции\n",
        );
        let module = workspace.join("CommonModules").join("Сервер").join("Ext").join("Module.bsl");

        let state = SharedState::workspace(workspace.clone());

        // Wait for the background init to publish the engine (the overlay is initialized just
        // before publish, so a visible engine already has it online).
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if state.search_engine().lock().unwrap().is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "the search engine never published");
            std::thread::sleep(Duration::from_millis(20));
        }

        // The resident feeds the overlay's shared parse; drive it to Ready.
        state.diagnostics().ensure_loading();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !matches!(state.diagnostics().status(), DiagnosticsStatus::Ready { .. }) {
            assert!(Instant::now() < deadline, "the resident never became ready");
            std::thread::sleep(Duration::from_millis(20));
        }

        // Observe the workspace watcher independently so the edit's delivery is confirmed before we
        // rely on the resident's own cursor (which `prefetch_resident_overlay` drains via catch_up).
        let hub = state.change_hub().expect("workspace boot owns a change hub").clone();
        assert!(hub.wait_until_watching(Duration::from_secs(10)), "the watcher must arm");
        let mut observer = hub.subscribe();

        // Edit on disk: v2 adds a symbol absent from the v1 baseline, so a hit for it can only come
        // from the overlay serving the working-tree bytes.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(
            &module,
            "&НаСервере\nФункция Ч() Экспорт Возврат 1; КонецФункции\n\
             Процедура СвежаяПроцедура() Экспорт КонецПроцедуры\n",
        )
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut delivered = false;
        while Instant::now() < deadline {
            let batch = hub.drain(observer);
            observer = batch.cursor;
            if batch.entries.iter().any(|e| e.raw.to_string_lossy().ends_with("Module.bsl")) {
                delivered = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(delivered, "the watcher delivered the edit");

        // Prove the SINK marked the edit — do NOT hand-mark. The search sink (spawned by
        // `SharedState::workspace`) drains the hub and marks the edited `.bsl` dirty in the overlay.
        // That is only reachable once the boot brought the overlay online; revert that wiring and the
        // overlay stays uninitialized, the sink's mark lands nowhere, and this poll never trips. So
        // waiting for the sink's OWN mark exercises hub -> sink -> mark end-to-end.
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let marked = {
                let guard = state.search_engine().lock().unwrap();
                let engine = guard.as_ref().unwrap();
                engine
                    .workspace_overlay_dirty_paths_snapshot()
                    .unwrap()
                    .keys()
                    .any(|p| p.ends_with("Module.bsl"))
            };
            if marked {
                break;
            }
            assert!(Instant::now() < deadline, "the sink never marked the edited path dirty");
            std::thread::sleep(Duration::from_millis(20));
        }

        // With the sink's mark in place, drive the resident-fed prefetch exactly as the search tool
        // does — NO dirty-marking of our own. `prefetch_resident_overlay` catch_ups the resident to
        // the new bytes (off the engine lock) BEFORE it reads the snapshot, so the very call that
        // consumes the dirty path already sees fresh bytes and feeds the reindex from the SHARED
        // parse. This proves mark -> prefetch -> resident-fed.
        let deadline = Instant::now() + Duration::from_secs(20);
        let fed = loop {
            SharedState::prefetch_resident_overlay(state.search_engine());
            let fed = state
                .search_engine()
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .workspace_overlay_resident_fed_count()
                .unwrap();
            if fed >= 1 {
                break fed;
            }
            assert!(Instant::now() < deadline, "the resident-fed reindex never ran");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(fed, 1, "the edited file was reindexed from the resident's shared parse");

        // Fresh bytes are served: the new symbol is found through the overlay (the lexical path the
        // search tool drives), though it is absent from the v1 store baseline.
        let (hits, _hidden) = {
            let guard = state.search_engine().lock().unwrap();
            let engine = guard.as_ref().unwrap();
            engine.workspace_overlay_lexical_hits("СвежаяПроцедура", 10).unwrap()
        };
        state.shutdown();
        assert!(
            hits.iter().any(|hit| hit.file_path.ends_with("Module.bsl")),
            "the overlay must serve the fresh working-tree bytes for the edited file",
        );
    }

    /// Warm boot of a local FTS-only workspace: the store is reused from a prior run and its
    /// re-index is skipped (chunks already exist), so a file changed WHILE THE DAEMON WAS DOWN is
    /// not in the store and no watcher event ever fires for it. That branch must NOT empty-init the
    /// overlay (which would be false-clean and serve the stale baseline forever) — it must prime,
    /// scanning disk against the store baseline so the while-down edit is served fresh. This asserts
    /// the branch selects [`OverlayInit::Prime`] and that the prime serves the fresh bytes with no
    /// dirty-marking at all.
    #[test]
    fn warm_boot_ftsonly_primes_overlay_for_edits_made_while_down() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _embedding_url = EnvVarGuard::unset("EMBEDDING_URL");
        let _embedding_model = EnvVarGuard::unset("EMBEDDING_MODEL");

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        fs::write(
            workspace.join("Configuration.xml"),
            "<Configuration><Name>Конфа</Name></Configuration>",
        )
        .unwrap();
        write_common_module_tree(
            &workspace,
            "Сервер",
            "&НаСервере\nФункция Ч() Экспорт Возврат 1; КонецФункции\n",
        );
        let module = workspace.join("CommonModules").join("Сервер").join("Ext").join("Module.bsl");
        let watcher_ready = Arc::new(AtomicBool::new(false));

        // First (cold) boot: an empty store, so the FTS index is built from v1 disk and the branch
        // reconciles -> Clean. Dropping the init persists the store under the workspace cache dir.
        let cold = SharedState::init_workspace_search_engine(
            &workspace,
            &watcher_ready,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("cold FTS-only init produces an engine");
        assert!(matches!(cold.overlay_init, OverlayInit::Clean), "cold boot reconciles the store");
        assert!(cold.engine.chunk_count().unwrap() > 0, "the store now holds the v1 baseline");
        drop(cold);

        // The daemon is "down"; the file gains a symbol absent from the persisted v1 store.
        fs::write(
            &module,
            "&НаСервере\nФункция Ч() Экспорт Возврат 1; КонецФункции\n\
             Процедура СвежаяПроцедура() Экспорт КонецПроцедуры\n",
        )
        .unwrap();

        // Second (warm) boot: the persisted store already has chunks, so FTS re-indexing is skipped
        // and the store is NOT reconciled with the while-down edit -> this branch must prime.
        let warm = SharedState::init_workspace_search_engine(
            &workspace,
            &watcher_ready,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("warm FTS-only init produces an engine");
        assert!(
            matches!(warm.overlay_init, OverlayInit::Prime),
            "a warm store that skipped re-indexing must prime, not empty-init",
        );

        // The store baseline is stale (v1), proving the boot did not reconcile it:
        assert!(
            warm.engine
                .text_search("СвежаяПроцедура", 10, Some("code"))
                .unwrap_or_default()
                .is_empty(),
            "sanity: the stale store baseline does not hold the while-down symbol",
        );

        // Apply the boot's chosen initialization exactly as `spawn_workspace_search_init` does. The
        // prime scans disk against the store baseline; NO dirty-marking, NO watcher event.
        warm.engine.prime_workspace_overlay().unwrap();

        let (hits, _hidden) =
            warm.engine.workspace_overlay_lexical_hits("СвежаяПроцедура", 10).unwrap();
        assert!(
            hits.iter().any(|hit| hit.file_path.ends_with("Module.bsl")),
            "the prime must serve the while-down edit that no watcher event covered",
        );
    }

    /// Deleted-while-down through the REAL init path on the STANDALONE (deferred-embedding) branch,
    /// a Clean branch: a semantic engine indexes two modules, the daemon stops, one module is
    /// deleted on disk, and a re-boot re-runs the deferred index — which only re-ingests files that
    /// still EXIST. The boot reconcile is what removes the vanished module's rows so the store ==
    /// working tree, and the branch must still assert Clean (its baseline is now truly clean).
    /// Store-level `file_count` is asserted (not `text_search(code)`, which routes through the
    /// overlay and would hide the deleted file regardless), so reverting the boot reconcile leaves
    /// the ghost row and fails this.
    #[test]
    fn deferred_boot_reconciles_deleted_file_and_stays_clean() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        // A configured embedder selects the semantic deferred branch; the URL is never dialed
        // (deferred indexing writes NULL embeddings), it only flips `has_semantic` true.
        let _embedding_url = EnvVarGuard::set("EMBEDDING_URL", "http://127.0.0.1:9/v1");
        let _embedding_model = EnvVarGuard::set("EMBEDDING_MODEL", "test-model");

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        fs::write(
            workspace.join("Configuration.xml"),
            "<Configuration><Name>Конфа</Name></Configuration>",
        )
        .unwrap();
        write_common_module_tree(
            &workspace,
            "Постоянный",
            "&НаСервере\nФункция ЖивойСимвол() Экспорт Возврат 1; КонецФункции\n",
        );
        write_common_module_tree(
            &workspace,
            "Улетевший",
            "&НаСервере\nФункция ИсчезнувшийСимвол() Экспорт Возврат 1; КонецФункции\n",
        );
        let watcher_ready = Arc::new(AtomicBool::new(false));

        // Cold boot: the deferred branch indexes both modules -> Clean; drop persists the store.
        let cold = SharedState::init_workspace_search_engine(
            &workspace,
            &watcher_ready,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("cold deferred init produces an engine");
        assert!(cold.engine.has_semantic(), "a configured embedder selects the semantic branch");
        assert!(matches!(cold.overlay_init, OverlayInit::Clean), "the deferred branch is Clean");
        assert_eq!(cold.engine.file_count().unwrap(), 2, "both modules are indexed");
        drop(cold);

        // The daemon is down; the Улетевший module is deleted.
        fs::remove_dir_all(workspace.join("CommonModules").join("Улетевший")).unwrap();
        fs::remove_file(workspace.join("CommonModules").join("Улетевший.xml")).unwrap();

        // Warm re-boot through the same real init path: the deferred re-index only sees present
        // files, so ONLY the boot reconcile can remove the deleted module's rows.
        let warm = SharedState::init_workspace_search_engine(
            &workspace,
            &watcher_ready,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("warm deferred init produces an engine");
        assert!(
            matches!(warm.overlay_init, OverlayInit::Clean),
            "a reconciled deferred boot stays Clean",
        );
        assert_eq!(
            warm.engine.file_count().unwrap(),
            1,
            "the boot reconcile removed the deleted-while-down module's rows",
        );
        let files: Vec<String> = warm
            .engine
            .store()
            .all_files_in_collection("code")
            .unwrap()
            .into_iter()
            .map(|(path, _hash)| path)
            .collect();
        assert!(
            files.iter().any(|p| p.contains("Постоянный")),
            "the surviving module is untouched: {files:?}",
        );
        assert!(
            !files.iter().any(|p| p.contains("Улетевший")),
            "the deleted module is gone from the store: {files:?}",
        );
    }

    /// Unit proof of the shared boot reconcile that every Clean branch funnels through
    /// ([`SharedState::reconcile_boot_store_with_disk`]): a store row for a file DELETED while the
    /// daemon was down is reconciled out, while a present file is kept, and the helper reports the
    /// store PROVEN reconciled. The fused / standalone-deferred / FTS-cold Clean branches all call
    /// this exact helper after their index step, so proving it here proves the deletion is removed on
    /// each — without standing up a full graph build for the fused path. Store-level `file_count` is
    /// asserted so the removal is real, not overlay-hidden.
    #[test]
    fn boot_reconcile_removes_deleted_file_keeps_present() {
        // The boot reconcile reads the process-global `FORCE_REWALK_WALK_ERROR` seam; serialize
        // against the walk-error tests that toggle it so a concurrent set can't force a false error.
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        write_common_module_tree(
            &workspace,
            "Улетевший",
            "&НаСервере\nФункция ИсчезнувшийСимвол() Экспорт Возврат 1; КонецФункции\n",
        );
        write_common_module_tree(
            &workspace,
            "Постоянный",
            "&НаСервере\nФункция ЖивойСимвол() Экспорт Возврат 1; КонецФункции\n",
        );

        let db_path = dir.path().join("search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.index_directory_fts(&workspace).unwrap();
        assert_eq!(engine.file_count().unwrap(), 2, "both modules are indexed");

        // The Улетевший module vanishes while the daemon is down.
        fs::remove_dir_all(workspace.join("CommonModules").join("Улетевший")).unwrap();
        fs::remove_file(workspace.join("CommonModules").join("Улетевший.xml")).unwrap();

        let reconciled = SharedState::reconcile_boot_store_with_disk(&mut engine, &workspace);
        assert!(reconciled, "a clean walk proves the store reconciled");
        assert_eq!(
            engine.file_count().unwrap(),
            1,
            "the deleted file's rows are reconciled out of the store",
        );
        let files: Vec<String> = engine
            .store()
            .all_files_in_collection("code")
            .unwrap()
            .into_iter()
            .map(|(path, _hash)| path)
            .collect();
        assert!(
            files.iter().any(|p| p.contains("Постоянный")) && files.len() == 1,
            "only the present module survives: {files:?}",
        );
    }

    /// A walk error at boot cannot prove the store was reconciled, so a Clean branch must DOWNGRADE
    /// to a prime rather than assert a false clean. Force the reconcile walk to error and drive a
    /// cold FTS-only boot (otherwise Clean) through the real init path: it must select Prime.
    /// Reverting the downgrade (staying Clean on a failed walk) fails this.
    #[test]
    fn boot_walk_error_downgrades_clean_to_prime() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _embedding_url = EnvVarGuard::unset("EMBEDDING_URL");
        let _embedding_model = EnvVarGuard::unset("EMBEDDING_MODEL");

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        fs::write(
            workspace.join("Configuration.xml"),
            "<Configuration><Name>Конфа</Name></Configuration>",
        )
        .unwrap();
        write_common_module_tree(
            &workspace,
            "Сервер",
            "&НаСервере\nФункция Ч() Экспорт Возврат 1; КонецФункции\n",
        );
        let watcher_ready = Arc::new(AtomicBool::new(false));

        struct ResetWalkErr;
        impl Drop for ResetWalkErr {
            fn drop(&mut self) {
                super::FORCE_REWALK_WALK_ERROR.store(false, super::Ordering::SeqCst);
            }
        }
        super::FORCE_REWALK_WALK_ERROR.store(true, super::Ordering::SeqCst);
        let _reset = ResetWalkErr;

        let init = SharedState::init_workspace_search_engine(
            &workspace,
            &watcher_ready,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("cold FTS-only init produces an engine");
        assert!(
            matches!(init.overlay_init, OverlayInit::Prime),
            "a boot whose reconcile walk errored must prime, not assert a false clean",
        );
    }

    /// A file that still EXISTS but was gutted to comments-only while the daemon was down yields zero
    /// chunks; the boot indexer must REMOVE its now-stale prior chunks rather than skip it (the
    /// deletion reconcile can't help — the file is not gone). Index a module with a symbol, gut it,
    /// re-index: the prior chunk must leave the store. Reverting the chunkless-removal (bare
    /// `continue`) leaves the stale chunk and fails this.
    #[test]
    fn boot_indexer_removes_stale_chunks_when_file_gutted_to_comments() {
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        write_common_module_tree(
            &workspace,
            "Сервер",
            "&НаСервере\nФункция УникальныйСимвол() Экспорт Возврат 1; КонецФункции\n",
        );
        let module = workspace.join("CommonModules").join("Сервер").join("Ext").join("Module.bsl");

        let db_path = dir.path().join("search.db");
        let mut engine = SearchEngine::fts_only(&db_path).unwrap();
        engine.set_workspace_root(workspace.clone());
        engine.index_directory_fts(&workspace).unwrap();
        assert_eq!(engine.chunk_count().unwrap(), 1, "the original body is one chunk");

        // Gutted to a comment-only file while down: hash changes, chunking yields nothing.
        fs::write(&module, "// только комментарий, без исполняемого кода\n").unwrap();
        engine.index_directory_fts(&workspace).unwrap();

        assert_eq!(
            engine.chunk_count().unwrap(),
            0,
            "the stale chunk of the now-chunkless file is removed from the store",
        );
    }
}
