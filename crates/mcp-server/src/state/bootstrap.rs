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
use std::sync::atomic::{AtomicBool, Ordering};
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

        let search_engine: SharedSearchEngine = Arc::new(Mutex::new(None));
        let index_progress = IndexProgress::new();
        let semantic_runtime = Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled));
        let overlay_warmup = Arc::new(Mutex::new(OverlayWarmupState::Pending));
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
        let mut scan_roots = vec![config_path.to_path_buf()];
        scan_roots.extend(project.extension_paths().iter().map(|(_, path)| path.clone()));
        let change_hub = WorkspaceChangeHub::start_targets(crate::change_hub::watch_targets_for(
            &project.root,
            &scan_roots,
        ));

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

        Self::spawn_workspace_search_init(
            Arc::clone(&search_engine),
            Arc::clone(&index_progress),
            Arc::clone(&semantic_runtime),
            source_dir.clone(),
            Arc::clone(&watcher_ready),
            baseline.clone(),
            workspace_search_mode.clone(),
            graph.clone(),
            Arc::clone(&embed_flight),
            Arc::clone(&snapshot_source),
            workspace_lease.clone(),
            overlay_retry.clone(),
        );

        Self::spawn_search_sink(
            change_hub.clone(),
            Arc::clone(&search_engine),
            Arc::clone(&watcher_ready),
            config_path.to_path_buf(),
            graph.clone(),
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
            embed_flight,
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
        watcher_ready: Arc<AtomicBool>,
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
                let init = Self::init_workspace_search_engine(
                    &workspace_root,
                    &watcher_ready,
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

                if has_leftover_marks {
                    graph.consume_leftover_marks(leftover_bound);
                }

                // A boot that found the cached graph built under a different extension topology
                // asked for a whole-collection re-render before this engine existed to run it.
                // The publish that followed could not hand the request anywhere, so it is
                // honoured here — otherwise files the build skipped as byte-identical keep the
                // contexts they were given under the old topology.
                graph.flush_pending_topology_refresh();

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
                        retry.kick();
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
            embed_flight: EmbedFlight::new(),
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
            embed_flight: EmbedFlight::new(),
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

    pub(super) fn init_workspace_search_engine(
        workspace_root: &std::path::Path,
        watcher_ready: &Arc<AtomicBool>,
        mode: WorkspaceSearchMode,
        external_baseline: Option<Arc<ExternalBaselineService>>,
        graph: &GraphState,
    ) -> Option<WorkspaceSearchInit> {
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
                &source_path,
                watcher_ready,
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
                root,
                watcher_ready,
                self.baseline.clone(),
                self.workspace_search_mode.clone(),
                self.graph.clone(),
                Arc::clone(&self.embed_flight),
                Arc::new(crate::diagnostics_state::ResidentModuleSnapshotSource::new(
                    self.diagnostics.clone(),
                )),
                self.workspace_lease.clone(),
                self.overlay_retry.clone(),
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
    use bsl_search::{
        BaselineRef, CorpusId, Document, ExternalBaselineConfig, IndexProgress, IndexedDocument,
        SearchEngine,
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

        SharedState::spawn_workspace_search_init(
            Arc::new(Mutex::new(None)),
            IndexProgress::new(),
            Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            workspace.clone(),
            Arc::new(AtomicBool::new(false)),
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
}
