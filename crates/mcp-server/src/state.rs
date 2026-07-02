use crate::baseline::{BaselineRuntime, ConfiguredBaselineStatus, ExternalBaselineService};
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

/// The search engine behind a mutex. It MUST stay a `Mutex` (not an `RwLock`): the engine
/// owns a `rusqlite::Connection`, which is `Send` but `!Sync` — its internal statement cache
/// mutates through a `RefCell` even on read-only SQL, so two threads may never hold `&engine`
/// at once. Searches therefore serialize here by necessity. The "overlay warming up" failure
/// under a concurrent batch is fixed in [`crate::tools::search`] by *blocking* on this lock
/// (queueing) rather than bailing out on brief contention, not by widening the lock.
pub(crate) type SharedSearchEngine = Arc<Mutex<Option<SearchEngine>>>;

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
    external_baseline: Option<Arc<ExternalBaselineService>>,
    configured_baseline: Option<ConfiguredBaselineStatus>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkspaceSearchMode {
    SqliteLocal,
    PostgresRemoteOverlay,
}

/// Outcome of the one-shot PostgresRemoteOverlay warmup that embeds local working-tree diffs
/// against the published baseline at startup. Tracked separately from [`SemanticRuntimeStatus`]
/// so `search status` can tell "no local diffs" (semantic is fully baseline-served, nothing to
/// build) apart from "warmup failed" (baseline still serves, but local edits are NOT in the
/// semantic index): a bare `Ready` + empty overlay is ambiguous between the two.
#[derive(Debug, Clone)]
pub(crate) enum OverlayWarmupState {
    /// Not started, in progress, or a non-overlay mode where no warmup runs.
    Pending,
    /// Warmup did not run: no embedder configured or no workspace root.
    Skipped(String),
    /// Completed; nothing in the working tree differed from the baseline.
    NoLocalDiffs,
    /// Completed; embedded `embedded` chunks across `overlay_files` locally-changed files.
    Synced { overlay_files: usize, embedded: usize },
    /// Prime or publish failed. The baseline semantic index still serves; local edits are not
    /// reflected semantically until the next MCP restart retries the warmup.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SemanticRuntimeStatus {
    Disabled,
    OverlaySyncing,
    /// The local SQLite semantic index is being built in the background after the
    /// engine was published early: lexical search and the call graph are already live,
    /// while the RAG vectors fill in over the longer embedding pass. Distinct from
    /// [`Self::OverlaySyncing`], which is the remote-baseline overlay warmup.
    Indexing,
    Ready,
    Failed(String),
}

struct WorkspaceSearchInit {
    engine: SearchEngine,
    mode: WorkspaceSearchMode,
    /// Set by the fused cold-build path: the engine is published with FTS + graph
    /// context already written but embeddings still NULL, and this carries what the
    /// background pass needs to fill them on its own connection. `None` means the
    /// engine is fully ready (warm cache, FTS-only, or standalone reindex).
    pending_embed: Option<PendingEmbed>,
}

/// Inputs for the background embedding pass: its own database path and embedder config
/// so [`bsl_search::SearchEngine::embed_pending_chunks_standalone`] opens a separate WAL
/// connection and never holds the live engine's mutex during the long embed.
struct PendingEmbed {
    db_path: PathBuf,
    config: bsl_search::SearchConfig,
}

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
        let baseline_runtime = BaselineRuntime::workspace(Some(&project.root), &project.config);
        let workspace_search_mode = if baseline_runtime
            .external_baseline
            .as_ref()
            .is_some_and(|baseline| matches!(baseline.corpus(), CorpusId::WorkspaceCode))
        {
            WorkspaceSearchMode::PostgresRemoteOverlay
        } else {
            WorkspaceSearchMode::SqliteLocal
        };

        // Created before the search-init thread so it can own the workspace graph: for
        // a local SQLite workspace the search-init drives a single fused parse pass
        // that builds the graph AND the search index, then publishes the graph through
        // this handle. A clone (cheap, shared `Arc`s) goes to the search thread; this
        // copy stays in `SharedState` for graph-tool serving and drift/reload.
        let graph = GraphState::for_workspace(source_dir.clone());

        Self::spawn_workspace_search_init(
            Arc::clone(&search_engine),
            Arc::clone(&index_progress),
            Arc::clone(&semantic_runtime),
            Arc::clone(&overlay_warmup),
            Arc::clone(&background_indexers),
            source_dir.clone(),
            Arc::clone(&watcher_ready),
            baseline_runtime.external_baseline.clone(),
            graph.clone(),
        );

        // The change hub owns the one recursive workspace watcher and starts before
        // any consumer subscribes: the search engine is built on a background thread
        // and must not gate the watcher's lifecycle. Search subscribes as a sink and
        // preserves its prior behavior (mark only `.bsl` paths dirty).
        let change_hub = WorkspaceChangeHub::start(config_path.to_path_buf());
        Self::spawn_search_sink(
            change_hub.clone(),
            Arc::clone(&search_engine),
            Arc::clone(&watcher_ready),
            config_path.to_path_buf(),
        );

        // The `metadata` tool reads the resident diagnostics host (per-MDO substrate for
        // `object`, Channel-2 `load_configuration` for `tree`/`info`); it is seeded and
        // kept fresh by the resident's own drift poll, so no separate configuration
        // snapshot is loaded here.
        let diagnostics = DiagnosticsState::for_workspace(source_dir.clone());

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
            external_baseline: baseline_runtime.external_baseline,
            configured_baseline: Some(baseline_runtime.configured_baseline),
            graph,
            diagnostics,
            change_hub: Some(change_hub),
            background_indexers,
        }
    }

    // Each argument is a distinct shared handle the spawned init thread must own (engine,
    // progress, runtime status, indexer counter, roots, baseline, graph). Bundling them
    // into a context struct would only move the same fields behind one name without
    // clarifying anything, so the small over-arity is accepted here.
    #[allow(clippy::too_many_arguments)]
    fn spawn_workspace_search_init(
        search_engine: SharedSearchEngine,
        index_progress: Arc<IndexProgress>,
        semantic_runtime: Arc<Mutex<SemanticRuntimeStatus>>,
        overlay_warmup: Arc<Mutex<OverlayWarmupState>>,
        background_indexers: Arc<AtomicUsize>,
        workspace_root: PathBuf,
        watcher_ready: Arc<AtomicBool>,
        external_baseline: Option<Arc<ExternalBaselineService>>,
        graph: GraphState,
    ) {
        std::thread::Builder::new()
            .name("bsl-search-init".to_owned())
            .spawn(move || {
                // Held for the whole init (incl. a multi-minute fused cold build) so the
                // broker stays alive even if the launching client disconnects mid-build.
                let _init_guard = BackgroundWorkGuard::new(&background_indexers);
                tracing::info!("search engine initialization started in background");
                let init = Self::init_workspace_search_engine(
                    &workspace_root,
                    &watcher_ready,
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

                if let Ok(mut guard) = search_engine.lock() {
                    *guard = Some(init.engine);
                }

                if let Some(status) = status_after_publish {
                    Self::set_semantic_runtime_status(&semantic_runtime, status);
                }

                tracing::info!("search engine initialization complete");

                if let Some(pending) = pending_embed {
                    let search_engine = Arc::clone(&search_engine);
                    let semantic_runtime = Arc::clone(&semantic_runtime);
                    let index_progress = Arc::clone(&index_progress);
                    // Take the hold BEFORE spawning so the count never dips to zero between
                    // this init thread ending and the embed thread starting.
                    let embed_guard = BackgroundWorkGuard::new(&background_indexers);
                    std::thread::Builder::new()
                        .name("bsl-search-embed".to_owned())
                        .spawn(move || {
                            let _embed_guard = embed_guard;
                            tracing::info!("background embedding pass started");
                            match SearchEngine::embed_pending_chunks_standalone(
                                &pending.db_path,
                                &pending.config,
                                Some(&index_progress),
                            ) {
                                Ok(index) => {
                                    let swapped = match search_engine.lock() {
                                        Ok(mut guard) => match guard.as_mut() {
                                            Some(engine) => {
                                                engine.set_vector_index(index);
                                                true
                                            }
                                            None => false,
                                        },
                                        Err(e) => {
                                            tracing::warn!(
                                                "background embedding: engine lock error: {e}"
                                            );
                                            false
                                        }
                                    };
                                    if swapped {
                                        Self::set_semantic_runtime_status(
                                            &semantic_runtime,
                                            SemanticRuntimeStatus::Ready,
                                        );
                                        tracing::info!(
                                            "background embedding pass complete; semantic index live"
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("background embedding pass failed: {e}");
                                    Self::set_semantic_runtime_status(
                                        &semantic_runtime,
                                        SemanticRuntimeStatus::Failed(format!(
                                            "background embedding failed: {e}"
                                        )),
                                    );
                                }
                            }
                        })
                        .ok();
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
        let baseline_runtime = BaselineRuntime::reference(project_config.as_ref());

        {
            let engine_arc = Arc::clone(&search_engine);
            let progress_arc = Arc::clone(&index_progress);
            let semantic_runtime_arc = Arc::clone(&semantic_runtime);
            let external_baseline = baseline_runtime.external_baseline.clone();
            let init_guard = BackgroundWorkGuard::new(&background_indexers);
            std::thread::Builder::new()
                .name("bsl-search-reference-init".to_owned())
                .spawn(move || {
                    let _init_guard = init_guard;
                    tracing::info!("reference search engine initialization started in background");
                    let engine =
                        Self::init_reference_search_engine(&progress_arc, external_baseline);
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
            external_baseline: baseline_runtime.external_baseline,
            configured_baseline: Some(baseline_runtime.configured_baseline),
            graph: GraphState::disabled(),
            diagnostics: DiagnosticsState::disabled(),
            change_hub: None,
            background_indexers,
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
            external_baseline: None,
            configured_baseline: None,
            graph: GraphState::disabled(),
            diagnostics: DiagnosticsState::disabled(),
            change_hub: None,
            background_indexers: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(crate) fn graph(&self) -> &GraphState {
        &self.graph
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

    pub(crate) fn external_baseline(&self) -> Option<Arc<ExternalBaselineService>> {
        self.external_baseline.clone()
    }

    pub(crate) fn configured_baseline(&self) -> Option<ConfiguredBaselineStatus> {
        self.configured_baseline.clone()
    }

    pub fn shutdown(&self) {
        if let Some(ref service) = self.external_baseline {
            service.shutdown();
        }
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

    fn init_workspace_search_engine(
        workspace_root: &std::path::Path,
        watcher_ready: &Arc<AtomicBool>,
        external_baseline: Option<Arc<ExternalBaselineService>>,
        graph: &GraphState,
    ) -> Option<WorkspaceSearchInit> {
        crate::cache::ensure_workspace_cache_dir(workspace_root).ok();
        let db_path = crate::cache::search_db_path(workspace_root);

        let project = project_model::Project::new(workspace_root);
        let source_path = project.source_path().to_path_buf();

        if let Some(external_baseline) = external_baseline
            .as_ref()
            .filter(|baseline| matches!(baseline.corpus(), CorpusId::WorkspaceCode))
        {
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
            if let Err(error) = store.clear_baseline_manifest() {
                tracing::warn!("failed to clear stale workspace baseline manifest: {error}");
                return None;
            }

            let manifest = match external_baseline.resolve_snapshot() {
                Ok(Some((_baseline_ref, snapshot))) => {
                    match external_baseline.load_baseline_manifest(&snapshot.id.0) {
                        Ok(manifest) => {
                            let store = engine.store();
                            if let Err(error) = store.save_baseline_manifest(&manifest) {
                                tracing::warn!(
                                    "failed to persist workspace baseline manifest: {error}"
                                );
                                return None;
                            }
                            tracing::info!(
                                snapshot_id = %snapshot.id.0,
                                manifest_files = manifest.files.len(),
                                "workspace baseline manifest loaded and persisted"
                            );
                            manifest
                        }
                        Err(error) => {
                            tracing::warn!("failed to load workspace baseline manifest: {error}");
                            return None;
                        }
                    }
                }
                Ok(None) => {
                    tracing::warn!(
                        "workspace baseline manifest unavailable for configured Postgres mode"
                    );
                    return None;
                }
                Err(error) => {
                    tracing::warn!("failed to resolve workspace baseline snapshot: {error}");
                    return None;
                }
            };

            tracing::info!(
                manifest_files = manifest.files.len(),
                "workspace overlay-only baseline initialized; baseline search served from Postgres"
            );

            return Some(WorkspaceSearchInit {
                engine,
                mode: WorkspaceSearchMode::PostgresRemoteOverlay,
                pending_embed: None,
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
            return Some(WorkspaceSearchInit {
                engine,
                mode: WorkspaceSearchMode::SqliteLocal,
                pending_embed,
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

            return Some(WorkspaceSearchInit {
                engine,
                mode: WorkspaceSearchMode::SqliteLocal,
                pending_embed,
            });
        }

        if engine.chunk_count().unwrap_or(0) == 0 {
            tracing::info!(?source_path, "building FTS index from source files");
            match engine.index_directory_fts(&source_path) {
                Ok(indexed) => tracing::info!(indexed, "FTS index built"),
                Err(e) => tracing::warn!("failed to build FTS index: {e}"),
            }
        }

        Some(WorkspaceSearchInit {
            engine,
            mode: WorkspaceSearchMode::SqliteLocal,
            pending_embed: None,
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
                        if let Err(error) = engine.remove_file("platform://docs") {
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
                self.external_baseline.clone(),
                self.graph.clone(),
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
    /// several: it drains its own cursor and marks only `.bsl` paths dirty, exactly
    /// as the standalone overlay watcher did before the hub existed. The raw
    /// (non-canonical) path is used so `mark_workspace_path_dirty`'s strip against
    /// the configured source root still matches when that root has symlinks.
    fn spawn_search_sink(
        hub: WorkspaceChangeHub,
        engine: SharedSearchEngine,
        watcher_ready: Arc<AtomicBool>,
        watch_root: PathBuf,
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

                    for entry in &batch.entries {
                        Self::mark_search_path_dirty(&engine, &entry.raw);
                    }

                    // Overflow means the hub dropped detail: the exact changed paths
                    // are lost. Restore parity with the old unbounded watcher (which
                    // never lost a `.bsl`) by re-marking every workspace `.bsl` dirty,
                    // so the overlay's incremental refresh reconsiders them all.
                    if batch.rescan_required {
                        tracing::warn!(
                            "workspace change hub overflowed; re-marking all workspace .bsl paths dirty for the search overlay"
                        );
                        Self::rewalk_workspace_bsl_dirty(&engine, &watch_root);
                    }
                }
            })
            .ok();
    }

    /// Re-mark every workspace `.bsl` dirty for the search overlay. Used when the
    /// change hub overflowed and the exact changed paths are no longer known, so
    /// the overlay's incremental refresh must reconsider the whole tree.
    fn rewalk_workspace_bsl_dirty(engine: &SharedSearchEngine, watch_root: &Path) {
        for file in walkdir::WalkDir::new(watch_root).follow_links(true) {
            let Ok(file) = file else { continue };
            if file.file_type().is_file() {
                Self::mark_search_path_dirty(engine, file.path());
            }
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

#[cfg(test)]
mod tests {
    use super::{BackgroundWorkGuard, SharedState};
    use crate::baseline::{ExternalBaselineService, RefreshableExternalBaselineSource};
    use bsl_search::{
        BaselineRef, CorpusId, Document, ExternalBaselineConfig, IndexedDocument, SearchEngine,
    };
    use std::env;
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex, OnceLock};
    use tempfile::tempdir;
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = env::var_os(key);
            env::set_var(key, value);
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = env::var_os(key);
            env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = self.previous.take() {
                env::set_var(self.key, value);
            } else {
                env::remove_var(self.key);
            }
        }
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

        let hub = WorkspaceChangeHub::start(workspace.clone());
        assert!(hub.wait_until_watching(Duration::from_secs(5)), "the watch must arm");
        // A second cursor observes the raw accumulator independently of the sink.
        let observer = hub.subscribe();

        let watcher_ready = Arc::new(AtomicBool::new(false));
        SharedState::spawn_search_sink(
            hub.clone(),
            Arc::clone(&engine_arc),
            Arc::clone(&watcher_ready),
            workspace.clone(),
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
}
