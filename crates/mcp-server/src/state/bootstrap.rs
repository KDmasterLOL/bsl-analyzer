use super::embed::EmbedFlight;
use super::types::{
    OverlayInit, OverlayWarmupState, PendingEmbed, SemanticRuntimeStatus, SharedSearchEngine,
    WorkspaceSearchInit, WorkspaceSearchMode,
};
use super::SharedState;
use crate::baseline::{
    BaselineBootstrap, BaselineRuntime, DeferredBaselineRuntime, ExternalBaselineService,
};
use crate::change_hub::WorkspaceChangeHub;
use crate::diagnostics_state::DiagnosticsState;
use crate::graph::GraphState;
use bsl_platform::PlatformDataInner;
use bsl_search::{BaselineHashMode, CorpusId, Document, IndexProgress, SearchEngine};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::{
    env,
    path::{Path, PathBuf},
};

/// How long a search-init thread waits for the deferred baseline connect before
/// proceeding degraded. Generous by design: the wait sits on a background thread and
/// only unusually slow networks ever reach it; the connect itself typically lands in
/// seconds and wakes the waiter through the slot's condvar immediately.
const BASELINE_CONNECT_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

impl SharedState {
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

    /// Errors when the project config or its extension topology is invalid: a
    /// daemon must not come up analyzing a differently-shaped project than the
    /// one configured.
    pub fn workspace(source_dir: PathBuf) -> Result<Self, project_model::ProjectError> {
        let project = crate::project::at(&source_dir)?;
        let config_path = project.source_path();
        let source_root = config_path.to_path_buf();

        // Claimed before any background pass starts, so the graph's very first build already
        // knows whether this daemon owns the workspace's derived caches or is the superseded
        // generation of a pair that overlaps over them.
        let workspace_lease = crate::workspace_lease::WorkspaceLease::claim(&source_dir);

        // Said once, here, because this is the only point EVERY boot passes. The engine's own
        // initialization has early exits before it builds a table — an unreachable shared
        // baseline, a store that will not open — and on those a dropped root would never be
        // named at all, which is the silence the warning exists to break. The resident builds
        // the same table on every config drift and deliberately says nothing: a line repeated
        // per rebuild buries the one that is new.
        crate::project::warn_about_rejected_roots(&crate::project::workspace_roots(&project).1);

        let search_engine: SharedSearchEngine = Arc::new(Mutex::new(None));
        let index_progress = IndexProgress::new();
        let semantic_runtime = Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled));
        let overlay_warmup = Arc::new(Mutex::new(OverlayWarmupState::Pending));
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
        //
        // Witnessed by `a_second_boot_over_a_matching_cache_declares_every_source_root_to_the_hub`,
        // and only a second boot can witness it: every graph path that BUILDS re-declares the
        // hub onto its own snapshot's roots, so a narrower set here would be repaired on a cold
        // boot before a reader could reach it. The publish that reuses a matching cache is the
        // one path that declares nothing. The window belongs to that test rather than to a
        // session: a serving daemon calls `warm_start` right after this constructor, and the
        // resident's publish re-declares the roots too.
        let scan_roots = project.source_roots();
        let change_hub = WorkspaceChangeHub::start_targets(crate::change_hub::watch_targets_for(
            &project.root,
            &scan_roots,
        ));

        // Subscribed here, synchronously, before the thread that reads disk even exists —
        // the same order the graph and the resident already keep, where the cursor is taken
        // first and the baseline scan second, so only what lands after the baseline needs
        // replaying. Search used to take it last, on a third thread, which left the window
        // between the two paid for by a full rescan of every root on essentially every boot.
        let sink_lease = crate::change_hub::CursorLease::new(change_hub.clone());

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
            Arc::clone(&index_progress),
            Arc::clone(&embed_flight),
            workspace_lease.clone(),
        );
        let graph = GraphState::for_workspace(source_dir.clone())
            .with_change_hub(change_hub.clone())
            .with_publish_hook(publish_hook)
            .with_lease(workspace_lease.clone());

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

        // The overlay retry driver exists only where an Embed pass exists: Postgres mode
        // with an embedder. PG without one is the legitimate FTS-only shape — a driver
        // there would run a `Skipped` pass per tick without ever consuming a signal; the
        // warmup state is settled up front instead, or `search status` would show
        // "building..." forever with the direct startup warmup gone.
        let overlay_retry =
            if matches!(workspace_search_mode, WorkspaceSearchMode::PostgresRemoteOverlay) {
                if Self::embedding_config().is_some() {
                    Some(super::overlay_retry::OverlayRetry::spawn(
                        Arc::clone(&search_engine),
                        Arc::clone(&overlay_warmup),
                        Arc::clone(&semantic_runtime),
                        workspace_lease.clone(),
                    ))
                } else {
                    Self::set_overlay_warmup_state(
                        &overlay_warmup,
                        OverlayWarmupState::Skipped("no embedder configured".to_owned()),
                    );
                    None
                }
            } else {
                None
            };

        // The sink is started by this thread, once there is an engine to feed and a watch
        // to feed it from; the lease rides along so the cursor is released on every way out
        // that does not end in a running sink — including this spawn failing, where the
        // closure is dropped unrun.
        Self::spawn_workspace_search_init(
            Arc::clone(&search_engine),
            Arc::clone(&index_progress),
            Arc::clone(&semantic_runtime),
            source_dir.clone(),
            change_hub.clone(),
            sink_lease,
            baseline.clone(),
            workspace_search_mode.clone(),
            graph.clone(),
            Arc::clone(&embed_flight),
            Arc::clone(&snapshot_source),
            workspace_lease.clone(),
            overlay_retry.clone(),
        );

        Ok(Self {
            workspace_root: Some(source_dir),
            source_root: Some(source_root),
            onec_client: None,
            onec_connections: Default::default(),
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
            workspace_lease,
            overlay_retry,
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
        workspace_root: PathBuf,
        change_hub: WorkspaceChangeHub,
        sink_lease: crate::change_hub::CursorLease,
        baseline: DeferredBaselineRuntime,
        mode: WorkspaceSearchMode,
        graph: GraphState,
        embed_flight: Arc<EmbedFlight>,
        snapshot_source: Arc<dyn bsl_search::ModuleSnapshotSource>,
        lease: crate::workspace_lease::WorkspaceLease,
        overlay_retry: Option<Arc<super::overlay_retry::OverlayRetry>>,
    ) {
        std::thread::Builder::new()
            .name("bsl-search-init".to_owned())
            .spawn(move || {
                tracing::info!("search engine initialization started in background");

                // The graph is a boot subsystem like the resident, not a lazy one. In
                // SqliteLocal the fused cold build below claims and builds it; the Postgres
                // branch never reaches that claim at all, which is why a PG workspace paid for
                // a whole-config graph build mid-session, on the first `graph`/`symbol_info`
                // call. Start it here instead — ahead of the baseline connect wait, which the
                // graph does not depend on. (Other early exits are covered by the catch-all
                // start after the init returns.)
                //
                // Mode-gated on purpose: in SqliteLocal an eager kick would win the
                // `Idle → Loading` transition that `try_begin_external_build` needs, the fused
                // claim would fail, and one parse pass producing both graph and search chunks
                // would degrade into two. A warm graph cache makes either start cheap —
                // `run_load` publishes the cached build instead of rebuilding.
                if matches!(mode, WorkspaceSearchMode::PostgresRemoteOverlay) {
                    graph.ensure_loading();
                }

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
                let mut sink_lease = sink_lease;
                let init = Self::init_workspace_search_engine(
                    &workspace_root,
                    Some((&change_hub, super::sync::WatchWaitPolicy::PRODUCTION)),
                    mode,
                    external_baseline,
                    &graph,
                );

                // Whatever the init decided, the graph must not be left idle: it may have
                // bailed (invalid project, unopenable store) before reaching the fused claim
                // at all. A no-op once the build is claimed or published, so the fused and
                // cached paths above are untouched.
                graph.ensure_loading();

                let Some(mut init) = init else {
                    Self::set_semantic_runtime_status(
                        &semantic_runtime,
                        SemanticRuntimeStatus::Failed(
                            "workspace search engine initialization failed".to_owned(),
                        ),
                    );
                    // Terminal for this process: no engine will ever publish, and the
                    // driver retrying "engine unavailable" forever would only mask the
                    // failure with an endless OverlaySyncing/backoff cycle.
                    if let Some(retry) = &overlay_retry {
                        retry.disarm();
                    }
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

                // Only now, and only if the watch is up: the sink drains into the published
                // engine, and a sink started before this point would drop every batch it
                // read into an engine that was not there yet.
                if init.watch_armed {
                    if let Some(cursor) = sink_lease.cursor() {
                        if Self::spawn_search_sink(
                            change_hub.clone(),
                            cursor,
                            Arc::clone(&search_engine),
                            graph.clone(),
                            overlay_retry.clone(),
                        ) {
                            sink_lease.handed_over();
                        }
                    }
                }

                if has_leftover_marks {
                    graph.consume_leftover_marks(leftover_bound);
                }

                // A boot that found the cached graph built under a different extension topology
                // asked for a whole-collection re-render before this engine existed to run it.
                // The publish that followed could not hand the request anywhere, so it is
                // honoured here — otherwise files the build skipped as byte-identical keep the
                // contexts they were given under the old topology.
                graph.flush_pending_topology_refresh();
                graph.flush_pending_search_roots_refresh();

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
                        Arc::clone(&index_progress),
                        Arc::clone(&embed_flight),
                        lease.clone(),
                        pending.db_path,
                        pending.config,
                    );
                }

                // The startup warmup goes through the SAME single-flight the retries do:
                // a direct spawn here would race a driver-triggered publication and let
                // last-writer-wins install the older plan. The driver owns the semantic
                // status transitions around each pass; the `!initialized` signal makes the
                // first pass unconditional.
                if needs_overlay_warmup {
                    if let Some(retry) = &overlay_retry {
                        // The fresh kick, not the bare one: the worker's first tick may have
                        // raced this publication, collected a transient "engine unavailable"
                        // and armed its backoff — the engine appearing is exactly the kind of
                        // new fact that resets it.
                        retry.kick_fresh();
                    }
                }
            })
            .ok();
    }

    pub fn reference(project_root: Option<PathBuf>) -> Self {
        let search_engine: SharedSearchEngine = Arc::new(Mutex::new(None));
        let index_progress = IndexProgress::new();
        let semantic_runtime = Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled));
        let project_config = project_root.as_deref().and_then(|root| {
            match project_model::ProjectConfig::load(root) {
                Ok(config) => config,
                Err(e) => {
                    // The reference profile only mines the config for baseline
                    // settings; a broken file loses those settings but must not
                    // keep the reference daemon from serving.
                    tracing::error!(error = %e, "reference profile ignores unreadable project config");
                    None
                }
            }
        });
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
            std::thread::Builder::new()
                .name("bsl-search-reference-init".to_owned())
                .spawn(move || {
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
            onec_connections: Default::default(),
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
            workspace_lease: crate::workspace_lease::WorkspaceLease::unmanaged(),
            overlay_retry: None,
        }
    }

    pub fn shared() -> Self {
        Self {
            workspace_root: None,
            source_root: None,
            onec_client: None,
            onec_connections: Default::default(),
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
            workspace_lease: crate::workspace_lease::WorkspaceLease::unmanaged(),
            overlay_retry: None,
        }
    }

    pub(super) fn embedding_config() -> Option<bsl_search::SearchConfig> {
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
        workspace_roots: bsl_search::WorkspaceRoots,
        hash_mode: BaselineHashMode,
    ) {
        engine.set_workspace_roots(workspace_roots);
        engine.set_workspace_baseline_hash_mode(hash_mode);
    }

    fn roots_of(project: &project_model::Project) -> bsl_search::WorkspaceRoots {
        crate::project::workspace_roots(project).0
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

    /// `watch` is the change hub this boot's baseline must not outrun, with how long to
    /// wait for it. Every read below is a baseline, and a baseline taken before the watch
    /// armed can be older than the oldest change anyone will ever report — a window that
    /// used to be paid for afterwards by a full rescan of every root. Waiting instead of
    /// paying costs the arming time once; the rescan cost a walk plus a stat and a full
    /// read of every file in the configuration, on every boot. `None` (tests, and the
    /// entry points that have no hub) waits for nothing and reports no watch.
    pub(super) fn init_workspace_search_engine(
        workspace_root: &std::path::Path,
        watch: Option<(&WorkspaceChangeHub, super::sync::WatchWaitPolicy)>,
        mode: WorkspaceSearchMode,
        external_baseline: Option<Arc<ExternalBaselineService>>,
        graph: &GraphState,
    ) -> Option<WorkspaceSearchInit> {
        let watch_armed = match watch {
            Some((hub, policy)) => Self::await_watch(hub, policy),
            None => false,
        };
        crate::cache::ensure_workspace_cache_dir(workspace_root).ok();
        let db_path = crate::cache::search_db_path(workspace_root);

        // The daemon only reaches this after `workspace()` validated the project;
        // a config broken by a mid-session edit keeps search down, loudly.
        let project = match crate::project::at(workspace_root) {
            Ok(project) => project,
            Err(e) => {
                tracing::error!(error = %e, "invalid project; workspace search stays offline");
                return None;
            }
        };
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
                Self::roots_of(&project),
                BaselineHashMode::NormalizedChunks,
            );
            if let Err(error) = engine.set_serves_external_baseline(true) {
                tracing::warn!("failed to declare the external-baseline mode: {error}");
                return None;
            }

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
                watch_armed,
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
            Self::roots_of(&project),
            BaselineHashMode::RawFileBytes,
        );
        // Declaring the local mode also clears inherited fingerprint rows: they claim
        // "verified against the manifest", which this mode can neither honour nor refresh —
        // a row surviving the local period would suppress a same-stat edit after a switch
        // back to the same snapshot. A failed clear leaves that lie standing, so the boot
        // fails closed, exactly like the Postgres branch does on its own failed clears.
        if let Err(error) = engine.set_serves_external_baseline(false) {
            tracing::warn!("failed to clear inherited overlay fingerprint rows: {error}");
            return None;
        }

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
            let overlay_init = if Self::reconcile_boot_store_with_disk(&mut engine) {
                OverlayInit::Clean
            } else {
                OverlayInit::Prime
            };
            return Some(WorkspaceSearchInit {
                watch_armed,
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
                Ok(graph_db)
                    if !crate::graph::scan::graph_file_matches_live_topology(
                        workspace_root,
                        &graph_db,
                    ) =>
                {
                    tracing::warn!(
                        "graph database was built for another extension topology; \
                         embeddings without graph context"
                    );
                }
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
            let overlay_init = if Self::reconcile_boot_store_with_disk(&mut engine) {
                OverlayInit::Clean
            } else {
                OverlayInit::Prime
            };
            return Some(WorkspaceSearchInit {
                watch_armed,
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
            if Self::reconcile_boot_store_with_disk(&mut engine) {
                OverlayInit::Clean
            } else {
                OverlayInit::Prime
            }
        } else {
            // Warm store: prime handles the while-down EDITS; the reconcile still removes rows for
            // files DELETED while down (a prime only hides them lazily and never from the store).
            // A root DECLARED while down is neither: it has no rows to refresh and no rows to
            // remove, so the skip is taken per root and only the unindexed ones are ingested.
            match engine.index_unindexed_roots_fts() {
                Ok(indexed) if indexed > 0 => {
                    tracing::info!(
                        indexed,
                        "indexed a source root declared while the daemon was down"
                    )
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("failed to index a newly declared source root: {e}"),
            }
            Self::reconcile_boot_store_with_disk(&mut engine);
            OverlayInit::Prime
        };

        Some(WorkspaceSearchInit {
            watch_armed,
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
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{write_common_module_tree, EnvVarGuard, ENV_LOCK};
    use super::{
        DiagnosticsState, EmbedFlight, GraphState, OverlayInit, SemanticRuntimeStatus, SharedState,
        WorkspaceSearchMode,
    };
    use crate::baseline::{
        BaselineBootstrap, BaselineRuntime, ConfiguredBaselineStatus, DeferredBaselineRuntime,
        ExternalBaselineService, RefreshableExternalBaselineSource,
    };
    use crate::change_hub::WorkspaceChangeHub;
    use bsl_search::{
        BaselineRef, CorpusId, Document, ExternalBaselineConfig, IndexProgress, IndexedDocument,
        SearchEngine,
    };
    use std::fs;
    use std::path::PathBuf;
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

    /// The Postgres branch of the search init returns before it ever reaches the fused cold
    /// build's graph claim, which used to leave the graph idle until the first
    /// `graph`/`symbol_info` call — billing a whole-config build to a mid-session request.
    /// The boot must start it regardless, so this drives the harshest case: an unavailable
    /// baseline, where the search init bails immediately and touches no graph at all.
    #[test]
    fn postgres_boot_starts_the_graph_even_when_the_search_init_bails() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _embedding_url = EnvVarGuard::unset("EMBEDDING_URL");

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
            "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции\n",
        );

        let graph = GraphState::for_workspace(workspace.clone());
        // Postgres mode with no external baseline: `init_workspace_search_engine` warns and
        // returns `None` without opening a store.
        let baseline = DeferredBaselineRuntime::ready(BaselineRuntime {
            configured_baseline: ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "test".to_owned(),
                issue: Some("baseline unavailable".to_owned()),
                support: None,
            },
            external_baseline: None,
        });

        // Armed before the boot starts, so the wait for it costs nothing and the branch under
        // test is the one that bails on the baseline, not the one that waits on the watch.
        let hub = WorkspaceChangeHub::start(vec![workspace.clone()]);
        assert!(hub.wait_until_watching(std::time::Duration::from_secs(5)), "the watch must arm");

        SharedState::spawn_workspace_search_init(
            Arc::new(Mutex::new(None)),
            IndexProgress::new(),
            Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            workspace.clone(),
            hub.clone(),
            crate::change_hub::CursorLease::new(hub),
            baseline,
            WorkspaceSearchMode::PostgresRemoteOverlay,
            graph.clone(),
            EmbedFlight::new(),
            Arc::new(crate::diagnostics_state::ResidentModuleSnapshotSource::new(
                DiagnosticsState::disabled(),
            )),
            crate::workspace_lease::WorkspaceLease::unmanaged(),
            None,
        );

        for _ in 0..600 {
            match graph.status() {
                crate::graph::GraphStatus::Ready { .. } => return,
                crate::graph::GraphStatus::Failed(msg) => panic!("graph load failed: {msg}"),
                _ => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        }
        panic!("the boot left the graph at {:?}; it must not stay lazy", graph.status());
    }

    fn local_workspace_for_boot(dir: &std::path::Path) -> (PathBuf, DeferredBaselineRuntime) {
        let workspace = dir.to_path_buf();
        fs::write(
            workspace.join("Configuration.xml"),
            "<Configuration><Name>Конфа</Name></Configuration>",
        )
        .unwrap();
        write_common_module_tree(
            &workspace,
            "Сервер",
            "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции\n",
        );
        let baseline = DeferredBaselineRuntime::ready(BaselineRuntime {
            configured_baseline: ConfiguredBaselineStatus {
                backend: "sqlite",
                selection: "test".to_owned(),
                issue: None,
                support: None,
            },
            external_baseline: None,
        });
        (workspace, baseline)
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_local_boot(
        workspace: PathBuf,
        hub: WorkspaceChangeHub,
        lease: crate::change_hub::CursorLease,
        baseline: DeferredBaselineRuntime,
        engine: super::SharedSearchEngine,
    ) {
        SharedState::spawn_workspace_search_init(
            engine,
            IndexProgress::new(),
            Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            workspace,
            hub,
            lease,
            baseline,
            WorkspaceSearchMode::SqliteLocal,
            GraphState::disabled(),
            EmbedFlight::new(),
            Arc::new(crate::diagnostics_state::ResidentModuleSnapshotSource::new(
                DiagnosticsState::disabled(),
            )),
            crate::workspace_lease::WorkspaceLease::unmanaged(),
            None,
        );
    }

    /// Every read this init does is a baseline, and a baseline taken before the watch armed
    /// can be older than the oldest change anyone will ever report — the window this node
    /// exists to close. Closing it is nothing but statement order, which is exactly the kind
    /// of guarantee a later edit erases without noticing, so both reads the init performs
    /// are pinned, each by its own effect.
    ///
    /// The store is pinned by its file: none may exist while the hub is held. The project
    /// load is pinned by its input: the extension is declared only in the instant before
    /// release, so an init that read the project early would register one root and an init
    /// that waited registers two. Checking only the store would leave the project read free
    /// to drift back above the wait — reopening the window on exactly the input whose drift
    /// this node is about.
    #[test]
    fn the_boot_read_waits_for_the_watch_to_arm() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _embedding_url = EnvVarGuard::unset("EMBEDDING_URL");

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        // The configuration sits in its own subdirectory and the extension beside it: an
        // extension nested inside the configuration root is rejected as an overlap, and the
        // test would then measure the rejection instead of the read order.
        let configuration = workspace.join("src").join("cf");
        let extension = workspace.join("ext");
        for (dir, name) in [(&configuration, "Конфа"), (&extension, "Расширение")] {
            fs::create_dir_all(dir).unwrap();
            fs::write(
                dir.join("Configuration.xml"),
                format!("<Configuration><Name>{name}</Name></Configuration>"),
            )
            .unwrap();
            write_common_module_tree(
                dir,
                "Сервер",
                "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции\n",
            );
        }
        fs::write(workspace.join("bsl-analyzer.toml"), "[source]\nroot = \"src/cf\"\n").unwrap();

        let db_path = crate::cache::search_db_path(&workspace);
        let (hub, hold) = WorkspaceChangeHub::start_targets_held(vec![
            crate::change_hub::WatchTarget::recursive(workspace.clone()),
        ]);

        let init = {
            let workspace = workspace.clone();
            let hub = hub.clone();
            std::thread::spawn(move || {
                SharedState::init_workspace_search_engine(
                    &workspace,
                    Some((
                        &hub,
                        crate::state::sync::WatchWaitPolicy::new(
                            std::time::Duration::from_millis(5),
                            std::time::Duration::from_secs(30),
                        ),
                    )),
                    WorkspaceSearchMode::SqliteLocal,
                    None,
                    &GraphState::disabled(),
                )
                .map(|init| {
                    init.engine.workspace_roots().map_or(0, |roots| roots.entries().count())
                })
            })
        };

        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(
            !db_path.exists(),
            "the boot opened its store before the watch was up: {}",
            db_path.display(),
        );

        // Only now is the extension part of the project.
        fs::write(
            workspace.join("bsl-analyzer.toml"),
            "[source]\nroot = \"src/cf\"\nextensions = [{ name = \"e\", path = \"ext\" }]\n",
        )
        .unwrap();
        hold.release();

        let roots = init.join().unwrap().expect("the init runs to completion once the watch is up");
        assert_eq!(
            roots, 2,
            "the project was read after the watch, so the extension declared meanwhile is registered",
        );
        assert!(db_path.exists(), "and the store the init opened is on disk");
    }

    /// A cursor is subscribed before the thread that will read it exists, so every way out
    /// that does not end in a running sink has to release it — and there are more of those
    /// than a list would hold: a watch that never arms, an init that publishes nothing, a
    /// spawn the operating system refuses. A cursor nobody drains holds entries back for
    /// the life of the process.
    #[test]
    fn a_boot_without_a_watch_leaves_no_cursor_behind() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _embedding_url = EnvVarGuard::unset("EMBEDDING_URL");

        let dir = tempdir().unwrap();
        let (workspace, baseline) = local_workspace_for_boot(dir.path());
        let hub = WorkspaceChangeHub::start_with_unstartable_thread(vec![
            crate::change_hub::WatchTarget::recursive(workspace.clone()),
        ]);
        let lease = crate::change_hub::CursorLease::new(hub.clone());
        assert_eq!(hub.active_cursor_count(), 1, "the boot holds a cursor from the start");

        spawn_local_boot(workspace, hub.clone(), lease, baseline, Arc::new(Mutex::new(None)));

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while hub.active_cursor_count() > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(
            hub.active_cursor_count(),
            0,
            "a boot that started no sink must not leave its cursor subscribed",
        );
    }

    /// Watcher mode is one-way in the store and doubles as "skip the full rescan", so it may
    /// only be asked for by something that is actually feeding the overlay. Asserting the
    /// mode rather than one delivered change is the point: a polling overlay delivers changes
    /// too, through the full scan, so a delivery test would pass either way.
    #[test]
    fn a_boot_with_a_watch_hands_the_cursor_to_a_running_sink() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _embedding_url = EnvVarGuard::unset("EMBEDDING_URL");

        let dir = tempdir().unwrap();
        let (workspace, baseline) = local_workspace_for_boot(dir.path());
        let hub = WorkspaceChangeHub::start(vec![workspace.clone()]);
        assert!(hub.wait_until_watching(std::time::Duration::from_secs(5)), "the watch must arm");
        let lease = crate::change_hub::CursorLease::new(hub.clone());
        let engine: super::SharedSearchEngine = Arc::new(Mutex::new(None));

        spawn_local_boot(workspace, hub.clone(), lease, baseline, Arc::clone(&engine));

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut watching = false;
        while std::time::Instant::now() < deadline {
            let mode = engine
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|engine| engine.workspace_overlay_stats().ok().flatten())
                .map(|stats| stats.watcher_mode);
            if mode == Some(true) {
                watching = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(watching, "an armed boot must end with a sink feeding the overlay");
        assert_eq!(hub.active_cursor_count(), 1, "and the sink owns the cursor it was handed");
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
                    root_id: bsl_search::CONFIGURATION_ROOT_ID.to_owned(),
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
                    root_id: bsl_search::CONFIGURATION_ROOT_ID.to_owned(),
                    path: "GhostModule.bsl".to_owned(),
                    file_fingerprint: "ghost".to_owned(),
                    document_count: 1,
                    file_object_id: "obj-ghost".to_owned(),
                }],
            })
            .unwrap();
        assert!(stale_engine.store().load_baseline_manifest().unwrap().is_some());
        drop(stale_engine);

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
            None,
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
                    root_id: bsl_search::CONFIGURATION_ROOT_ID.to_owned(),
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
            None,
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

        // A disabled graph has no workspace root, so the fused path is skipped and the
        // standalone semantic branch runs — the path that previously embedded inline.
        let init = SharedState::init_workspace_search_engine(
            workspace,
            None,
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

        let state = SharedState::workspace(ws.to_path_buf()).expect("valid workspace project");
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

        let state = SharedState::workspace(workspace.clone()).expect("valid workspace project");

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
                    .any(|key| key.path.ends_with("Module.bsl"))
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

        // First (cold) boot: an empty store, so the FTS index is built from v1 disk and the branch
        // reconciles -> Clean. Dropping the init persists the store under the workspace cache dir.
        let cold = SharedState::init_workspace_search_engine(
            &workspace,
            None,
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
            None,
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

        // Cold boot: the deferred branch indexes both modules -> Clean; drop persists the store.
        let cold = SharedState::init_workspace_search_engine(
            &workspace,
            None,
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
            None,
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
            .map(|(key, _hash)| key.path)
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

    /// Lay out a workspace whose configuration sits in a SUBDIRECTORY and whose extensions sit
    /// beside it, then declare both extensions. The nesting matters: it is the only shape where
    /// a table built on the configuration root and one built on the project root disagree about
    /// every extension's identifier.
    fn workspace_with_two_extensions() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let configuration = workspace.join("src").join("cf");
        fs::create_dir_all(&configuration).unwrap();
        fs::write(
            configuration.join("Configuration.xml"),
            "<Configuration><Name>Конфа</Name></Configuration>",
        )
        .unwrap();
        write_common_module_tree(
            &configuration,
            "Основной",
            "&НаСервере\nФункция СимволКонфигурации() Экспорт Возврат 1; КонецФункции\n",
        );
        for (directory, symbol) in [("ext-a", "СимволПервого"), ("ext-b", "СимволВторого")]
        {
            let extension = workspace.join(directory);
            fs::create_dir_all(&extension).unwrap();
            fs::write(
                extension.join("Configuration.xml"),
                format!("<Configuration><Name>{directory}</Name></Configuration>"),
            )
            .unwrap();
            write_common_module_tree(
                &extension,
                "Расширенный",
                &format!("&НаСервере\nФункция {symbol}() Экспорт Возврат 1; КонецФункции\n"),
            );
        }
        fs::write(
            workspace.join("bsl-analyzer.toml"),
            "[source]\nroot = \"src/cf\"\n\
             extensions = [{ name = \"a\", path = \"ext-a\" }, { name = \"b\", path = \"ext-b\" }]\n",
        )
        .unwrap();
        (dir, workspace)
    }

    /// The production boot must register EVERY declared extension root, and under the identifier
    /// `WorkspaceRoots` promises: relative to the project directory, not to the configuration root
    /// ("root ids are relative to it… not the configuration root, which may sit in a
    /// subdirectory"). The identifier is the stored rows' identity across restarts, so an
    /// absolute one is a silent, persistent mis-keying rather than a cosmetic difference.
    /// Two extensions, because a table built from only the first element of the declared list
    /// satisfies every single-extension check.
    #[test]
    fn boot_registers_every_declared_extension_under_a_workspace_relative_id() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let (_dir, workspace) = workspace_with_two_extensions();

        let init = SharedState::init_workspace_search_engine(
            &workspace,
            None,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("the local init produces an engine");

        let roots = init.engine.workspace_roots().expect("the boot configures a root table");
        let ids: Vec<&str> = roots.ids().collect();
        assert!(
            ids.contains(&"ext-a") && ids.contains(&"ext-b"),
            "both declared extensions are registered by their workspace-relative ids: {ids:?}",
        );

        let extension_module = workspace
            .join("ext-a")
            .join("CommonModules")
            .join("Расширенный")
            .join("Ext")
            .join("Module.bsl");
        assert!(
            init.engine.mark_workspace_path_dirty(&extension_module).unwrap(),
            "a file of a declared extension resolves to a store key",
        );

        // The table's workspace is the project directory, but the base callers resolve a hit's
        // relative path against is the CONFIGURATION root. Conflating the two sends every
        // relative path one directory level too high, which shows up as unresolvable graph ids
        // and unrecognised root descriptors rather than as an error.
        assert_eq!(
            init.engine.configuration_root(),
            Some(workspace.join("src").join("cf").as_path()),
            "the configuration root stays the base of stored relative paths",
        );
    }

    /// Drive a graph to `Ready`, or say which state it got stuck in.
    fn wait_until_graph_ready(graph: &GraphState) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            match graph.status() {
                crate::graph::GraphStatus::Ready { .. } => return,
                crate::graph::GraphStatus::Failed(msg) => panic!("graph load failed: {msg}"),
                other => assert!(
                    std::time::Instant::now() < deadline,
                    "the graph never became ready; it stayed {other:?}",
                ),
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn canonical(path: &std::path::Path) -> PathBuf {
        path.canonicalize()
            .unwrap_or_else(|e| panic!("{path:?} must exist to be canonicalized: {e}"))
    }

    /// The boot declares to the change hub EVERY root the project has — the configuration
    /// and each extension — plus the workspace directory itself, non-recursively, so a
    /// config-file edit is delivered even in a nested layout.
    ///
    /// The set is observable on a SECOND boot and only there. Every graph path that builds
    /// re-declares the hub onto its own snapshot's roots, so on a first boot a narrower set
    /// here is repaired while the test is still waiting for the graph; the publish that
    /// reuses a matching cache is the one path that declares nothing, which is why this test
    /// pays for two boots. The same holds for the diagnostics resident, whose publish also
    /// re-declares — hence the assertion that it never started.
    ///
    /// What the hub was ASKED to watch is the subject, not what the watcher took: an
    /// exhausted inotify limit leaves the declaration intact, so this gate cannot turn red
    /// on a machine's resource state.
    #[test]
    fn a_second_boot_over_a_matching_cache_declares_every_source_root_to_the_hub() {
        use crate::diagnostics_state::DiagnosticsStatus;

        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let (_dir, workspace) = workspace_with_two_extensions();
        let graph_db = crate::cache::graph_db_path(&workspace);

        let first = SharedState::workspace(workspace.clone()).expect("valid workspace project");
        wait_until_graph_ready(first.graph());
        first.shutdown();
        let built_at_before = crate::graph::test_support::meta_string(&graph_db, "built_at");

        let second = SharedState::workspace(workspace.clone()).expect("valid workspace project");
        wait_until_graph_ready(second.graph());

        let built_at_now = crate::graph::test_support::meta_string(&graph_db, "built_at");
        let snapshot = second.graph().snapshot().expect("a ready graph snapshots");
        let freshness = second.graph().freshness(&snapshot);
        let resident = second.diagnostics().status();
        let declared =
            second.change_hub().expect("a workspace boot owns a change hub").declared_targets();
        second.shutdown();

        // Warmth, in three parts because the two non-warm branches announce themselves
        // differently. A finished full build writes its meta before publishing `Ready`, so
        // the timestamp catches it. The stale-cache publish writes no meta at all — it goes
        // `Ready` and pre-claims the reload in one lock hold — so at the instant it is
        // observed only the published state tells it apart from a warm republish. Where its
        // catch-up lands then decides whether the declaration is repaired at all: a full
        // rebuild re-declares the roots, a body-only incremental one never does. Neither
        // outcome is safe to observe over, which is why this refuses both.
        assert_eq!(
            built_at_now, built_at_before,
            "the second boot must republish the cached build, not rebuild it",
        );
        assert!(!freshness.stale, "a republished cache is not stale: {:?}", freshness.reload);
        assert_eq!(freshness.reload, "none", "no catch-up reload may be in flight");
        // The resident's publish re-declares the hub too, and moves neither the graph's meta
        // nor its published state — the assertions above cannot see it. `SharedState::workspace`
        // leaves the resident idle on purpose (serve paths call `warm_start` themselves); this
        // says so out loud, so moving that call inside the constructor fails here instead of
        // silently disarming the check below.
        assert!(
            matches!(resident, DiagnosticsStatus::Idle),
            "the resident must not have started; it was {resident:?}",
        );

        let mut declared: Vec<(PathBuf, bool)> =
            declared.iter().map(|t| (canonical(&t.path), t.recursive)).collect();
        declared.sort();
        // Spelled out rather than recomputed from `project.source_roots()`: the derivation is
        // what is on trial, and checking a call against itself would pass whatever it returned.
        let mut expected = vec![
            (canonical(&workspace.join("src").join("cf")), true),
            (canonical(&workspace.join("ext-a")), true),
            (canonical(&workspace.join("ext-b")), true),
            (canonical(&workspace), false),
        ];
        expected.sort();
        assert_eq!(
            declared, expected,
            "the boot declares the configuration, every extension, and the workspace root",
        );
    }

    /// Overlapping roots behave in two DIFFERENT ways, and both must survive the boot: an
    /// extension INSIDE the configuration takes no root of its own (its files stay the
    /// configuration's, and must remain findable), while an extension CONTAINING the
    /// configuration registers normally and is told apart from it by the longest matching
    /// prefix. An implementation that rejects the containing root loses its files entirely —
    /// the configuration's walk never reaches them.
    #[test]
    fn overlapping_roots_keep_every_file_under_exactly_one_owner() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let configuration = workspace.join("src").join("cf");
        fs::create_dir_all(&configuration).unwrap();
        fs::write(
            configuration.join("Configuration.xml"),
            "<Configuration><Name>Конфа</Name></Configuration>",
        )
        .unwrap();
        write_common_module_tree(
            &configuration,
            "Основной",
            "&НаСервере\nФункция СимволКонфигурации() Экспорт Возврат 1; КонецФункции\n",
        );
        // Inside the configuration root.
        let inner = configuration.join("inner-ext");
        fs::create_dir_all(&inner).unwrap();
        fs::write(
            inner.join("Configuration.xml"),
            "<Configuration><Name>Внутреннее</Name></Configuration>",
        )
        .unwrap();
        write_common_module_tree(
            &inner,
            "Внутренний",
            "&НаСервере\nФункция СимволВнутреннего() Экспорт Возврат 1; КонецФункции\n",
        );
        // Containing the configuration root: the workspace itself.
        fs::write(
            workspace.join("Configuration.xml"),
            "<Configuration><Name>Внешнее</Name></Configuration>",
        )
        .unwrap();
        write_common_module_tree(
            &workspace,
            "Внешний",
            "&НаСервере\nФункция СимволВнешнего() Экспорт Возврат 1; КонецФункции\n",
        );
        fs::write(
            workspace.join("bsl-analyzer.toml"),
            "[source]\nroot = \"src/cf\"\n\
             extensions = [{ name = \"inner\", path = \"src/cf/inner-ext\" }, \
             { name = \"outer\", path = \".\" }]\n",
        )
        .unwrap();
        let project = crate::project::at(&workspace);
        assert!(project.is_ok(), "the fixture project parses: {:?}", project.err());

        let init = SharedState::init_workspace_search_engine(
            &workspace,
            None,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("the local init produces an engine");
        let roots = init.engine.workspace_roots().expect("the boot configures a root table");

        let owner_of = |path: &std::path::Path| {
            let canonical = path.canonicalize().expect("the fixture file exists");
            roots.root_of(path, &canonical).expect("every fixture file has an owner")
        };

        let inner_module =
            inner.join("CommonModules").join("Внутренний").join("Ext").join("Module.bsl");
        let owner = owner_of(&inner_module);
        assert_eq!(
            owner.root_id,
            bsl_search::CONFIGURATION_ROOT_ID,
            "an extension inside the configuration takes no root of its own",
        );

        let configuration_module =
            configuration.join("CommonModules").join("Основной").join("Ext").join("Module.bsl");
        assert_eq!(
            owner_of(&configuration_module).root_id,
            bsl_search::CONFIGURATION_ROOT_ID,
            "the containing extension does not steal the configuration's own files",
        );

        let outer_module =
            workspace.join("CommonModules").join("Внешний").join("Ext").join("Module.bsl");
        assert_eq!(
            owner_of(&outer_module).root_id,
            ".",
            "a file outside the configuration belongs to the extension that contains it",
        );
    }

    /// A cold boot must index the declared extensions, not just the configuration — otherwise the
    /// extension reaches the index only if someone happens to edit it. And it must key each row by
    /// its own root: a `cfe` extension repeats the configuration's relative paths wholesale, so a
    /// writer that keys everything as the configuration silently overwrites one file with the
    /// other and serves one symbol where two exist.
    #[test]
    fn a_cold_boot_indexes_every_root_and_keeps_same_named_paths_apart() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let configuration = workspace.join("src").join("cf");
        let extension = workspace.join("ext-a");
        for (root, name, symbol) in [
            (&configuration, "Конфа", "СимволКонфигурации"),
            (&extension, "Расш", "СимволРасширения"),
        ] {
            fs::create_dir_all(root).unwrap();
            fs::write(
                root.join("Configuration.xml"),
                format!("<Configuration><Name>{name}</Name></Configuration>"),
            )
            .unwrap();
            // The SAME relative path under both roots: this is the shape a `cfe` extension has.
            write_common_module_tree(
                root,
                "Общий",
                &format!("&НаСервере\nФункция {symbol}() Экспорт Возврат 1; КонецФункции\n"),
            );
        }
        fs::write(
            workspace.join("bsl-analyzer.toml"),
            "[source]\nroot = \"src/cf\"\nextensions = [{ name = \"a\", path = \"ext-a\" }]\n",
        )
        .unwrap();

        let init = SharedState::init_workspace_search_engine(
            &workspace,
            None,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("the local init produces an engine");

        let rows: Vec<(String, String)> = init
            .engine
            .store()
            .all_files_in_collection("code")
            .unwrap()
            .into_iter()
            .map(|(key, _hash)| (key.root_id, key.path))
            .collect();
        let module_rows: Vec<&(String, String)> =
            rows.iter().filter(|(_, path)| path.ends_with("Module.bsl")).collect();
        assert_eq!(
            module_rows.len(),
            2,
            "the same relative path under two roots is two rows: {rows:?}",
        );
        assert!(
            module_rows.iter().any(|(root_id, _)| root_id == "ext-a"),
            "the extension's row is keyed by its own root: {rows:?}",
        );

        for symbol in ["СимволКонфигурации", "СимволРасширения"] {
            let hits = init.engine.text_search(symbol, 10, Some("code")).unwrap();
            assert!(!hits.is_empty(), "a cold boot serves {symbol}: {rows:?}");
        }
    }

    /// A warm FTS store skips re-indexing entirely — that is what makes a restart cheap. But an
    /// extension declared while the daemon was down has NO rows at all, and "skip everything"
    /// would leave it out of the store until someone edits it. The cheap skip must therefore be
    /// per-root, not global: roots that already have rows stay untouched (the whole point), roots
    /// with none get indexed.
    #[test]
    fn a_warm_store_indexes_a_root_declared_while_it_was_down() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        // No embedder: this is the FTS-only branch, the one that skips a warm store.
        let _embedding_url = EnvVarGuard::unset("EMBEDDING_URL");
        let _embedding_model = EnvVarGuard::unset("EMBEDDING_MODEL");

        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let configuration = workspace.join("src").join("cf");
        let extension = workspace.join("ext-a");
        for (root, name, symbol) in [
            (&configuration, "Конфа", "СимволКонфигурации"),
            (&extension, "Расш", "СимволРасширения"),
        ] {
            fs::create_dir_all(root).unwrap();
            fs::write(
                root.join("Configuration.xml"),
                format!("<Configuration><Name>{name}</Name></Configuration>"),
            )
            .unwrap();
            write_common_module_tree(
                root,
                "Общий",
                &format!("&НаСервере\nФункция {symbol}() Экспорт Возврат 1; КонецФункции\n"),
            );
        }
        // First boot: the extension is not declared yet, so the store warms up on the
        // configuration alone.
        fs::write(workspace.join("bsl-analyzer.toml"), "[source]\nroot = \"src/cf\"\n").unwrap();
        let cold = SharedState::init_workspace_search_engine(
            &workspace,
            None,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("the first init produces an engine");
        assert!(cold.engine.chunk_count().unwrap() > 0, "the first boot warms the store");
        let configuration_rows = cold.engine.store().all_files_in_collection("code").unwrap();
        drop(cold);

        // The extension is declared while the daemon is down.
        fs::write(
            workspace.join("bsl-analyzer.toml"),
            "[source]\nroot = \"src/cf\"\nextensions = [{ name = \"a\", path = \"ext-a\" }]\n",
        )
        .unwrap();
        let warm = SharedState::init_workspace_search_engine(
            &workspace,
            None,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("the warm init produces an engine");

        let rows: Vec<(String, String)> = warm
            .engine
            .store()
            .all_files_in_collection("code")
            .unwrap()
            .into_iter()
            .map(|(key, _hash)| (key.root_id, key.path))
            .collect();
        assert!(
            rows.iter().any(|(root_id, _)| root_id == "ext-a"),
            "the newly declared root is indexed on the warm boot: {rows:?}",
        );
        // The point of the warm branch is that the already-indexed root is NOT rewritten.
        let warm_configuration: Vec<_> = warm
            .engine
            .store()
            .all_files_in_collection("code")
            .unwrap()
            .into_iter()
            .filter(|(key, _)| key.root_id.is_empty())
            .collect();
        assert_eq!(
            warm_configuration, configuration_rows,
            "the configuration's rows survive the warm boot untouched",
        );
    }

    /// Reaching the store is not the same as reaching the SEMANTIC index: `search_code` serves a
    /// query from its lexical half whenever the semantic half is empty, so "the symbol is found"
    /// stays true with zero vectors for the extension. What must hold is that the extension's
    /// chunks enter the embedding queue — that queue is exactly what the deferred pass drains.
    /// The vectors themselves are written by the background pass over HTTP and are not built here.
    #[test]
    fn a_deferred_boot_queues_the_extension_for_embedding() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        // A configured embedder selects the semantic deferred branch; the URL is never dialed.
        let _embedding_url = EnvVarGuard::set("EMBEDDING_URL", "http://127.0.0.1:9/v1");
        let _embedding_model = EnvVarGuard::set("EMBEDDING_MODEL", "test-model");

        let (_dir, workspace) = workspace_with_two_extensions();
        let init = SharedState::init_workspace_search_engine(
            &workspace,
            None,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("the local init produces an engine");
        assert!(init.engine.has_semantic(), "a configured embedder selects the semantic branch");

        let pending = init.engine.store().load_pending_embedding_documents("code").unwrap();
        let queued_roots: Vec<&str> =
            pending.iter().map(|(_, document)| document.root_id.as_str()).collect();
        for root_id in ["", "ext-a", "ext-b"] {
            assert!(
                queued_roots.contains(&root_id),
                "root {root_id:?} has chunks waiting for a vector: {queued_roots:?}",
            );
        }
        // Positive control on the count: an empty queue would satisfy a "no root is missing"
        // check written as a subset test, so the queue must actually hold chunks.
        assert!(pending.len() >= 3, "one chunk per module at least: {}", pending.len());
    }

    /// The negative control for the test above: with nothing declared, the very same file is NOT
    /// a workspace key. Without it, an implementation that keys every path under the sun would
    /// pass the positive check.
    #[test]
    fn a_file_outside_every_declared_root_has_no_key() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let (_dir, workspace) = workspace_with_two_extensions();
        fs::write(workspace.join("bsl-analyzer.toml"), "[source]\nroot = \"src/cf\"\n").unwrap();

        let init = SharedState::init_workspace_search_engine(
            &workspace,
            None,
            crate::state::WorkspaceSearchMode::SqliteLocal,
            None,
            &crate::graph::GraphState::disabled(),
        )
        .expect("the local init produces an engine");

        let extension_module = workspace
            .join("ext-a")
            .join("CommonModules")
            .join("Расширенный")
            .join("Ext")
            .join("Module.bsl");
        assert!(
            !init.engine.mark_workspace_path_dirty(&extension_module).unwrap(),
            "an undeclared tree stays outside the index",
        );
    }
}
