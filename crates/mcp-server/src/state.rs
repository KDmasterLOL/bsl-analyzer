use crate::baseline::{BaselineRuntime, ConfiguredBaselineStatus, ExternalBaselineService};
use crate::graph::GraphState;
use bsl_metadata::Configuration;
use bsl_platform::PlatformDataInner;
use bsl_search::{BaselineHashMode, CorpusId, Document, IndexProgress, SearchEngine};
use notify::{
    Config as NotifyConfig, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use onec_client::Client as OnecClient;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::{
    env,
    path::{Path, PathBuf},
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct SharedState {
    configuration: Arc<RwLock<Option<Configuration>>>,
    extensions: Arc<RwLock<Vec<(String, Configuration)>>>,
    workspace_root: Option<PathBuf>,
    onec_client: Option<OnecClient>,
    debug_session: Arc<Mutex<Option<bsl_debug::session::DebugSession>>>,
    search_engine: Arc<Mutex<Option<SearchEngine>>>,
    index_progress: Arc<IndexProgress>,
    semantic_runtime: Arc<Mutex<SemanticRuntimeStatus>>,
    workspace_search_mode: WorkspaceSearchMode,
    external_baseline: Option<Arc<ExternalBaselineService>>,
    configured_baseline: Option<ConfiguredBaselineStatus>,
    graph: GraphState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkspaceSearchMode {
    SqliteLocal,
    PostgresRemoteOverlay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SemanticRuntimeStatus {
    Disabled,
    #[allow(dead_code, reason = "status display uses this once overlay sync is implemented")]
    OverlaySyncing,
    Ready,
    Failed(String),
}

struct WorkspaceSearchInit {
    engine: SearchEngine,
    mode: WorkspaceSearchMode,
}

impl SharedState {
    pub fn workspace(source_dir: PathBuf) -> Self {
        let project = project_model::Project::new(&source_dir);
        let config_path = project.source_path();
        let configuration = bsl_metadata::load_from_directory(config_path)
            .map_err(|e| {
                tracing::warn!(?config_path, "failed to load configuration: {e}");
                e
            })
            .ok();

        let search_engine: Arc<Mutex<Option<SearchEngine>>> = Arc::new(Mutex::new(None));
        let index_progress = IndexProgress::new();
        let semantic_runtime = Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled));
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

        Self::spawn_workspace_search_init(
            Arc::clone(&search_engine),
            Arc::clone(&index_progress),
            Arc::clone(&semantic_runtime),
            source_dir.clone(),
            Arc::clone(&watcher_ready),
            baseline_runtime.external_baseline.clone(),
        );

        {
            let engine_arc = Arc::clone(&search_engine);
            let watch_root = config_path.to_path_buf();
            let watcher_ready = Arc::clone(&watcher_ready);
            std::thread::Builder::new()
                .name("bsl-search-overlay-watch".to_owned())
                .spawn(move || {
                    Self::run_workspace_overlay_watcher(engine_arc, watch_root, watcher_ready);
                })
                .ok();
        }

        let mut extensions = Vec::new();
        for (name, ext_path) in project.extension_paths() {
            match bsl_metadata::load_from_directory(ext_path) {
                Ok(ext_config) => {
                    tracing::info!(
                        name = %name,
                        common_modules = ext_config.common_modules().len(),
                        "loaded extension metadata"
                    );
                    extensions.push((name.clone(), ext_config));
                }
                Err(e) => {
                    tracing::warn!(name = %name, ?ext_path, "failed to load extension: {e}");
                }
            }
        }

        let graph = GraphState::for_workspace(source_dir.clone());

        Self {
            configuration: Arc::new(RwLock::new(configuration)),
            extensions: Arc::new(RwLock::new(extensions)),
            workspace_root: Some(source_dir),
            onec_client: None,
            debug_session: Arc::new(Mutex::new(None)),
            search_engine,
            index_progress,
            semantic_runtime,
            workspace_search_mode,
            external_baseline: baseline_runtime.external_baseline,
            configured_baseline: Some(baseline_runtime.configured_baseline),
            graph,
        }
    }

    fn spawn_workspace_search_init(
        search_engine: Arc<Mutex<Option<SearchEngine>>>,
        index_progress: Arc<IndexProgress>,
        semantic_runtime: Arc<Mutex<SemanticRuntimeStatus>>,
        workspace_root: PathBuf,
        watcher_ready: Arc<AtomicBool>,
        external_baseline: Option<Arc<ExternalBaselineService>>,
    ) {
        std::thread::Builder::new()
            .name("bsl-search-init".to_owned())
            .spawn(move || {
                tracing::info!("search engine initialization started in background");
                let init = Self::init_workspace_search_engine(
                    &workspace_root,
                    &index_progress,
                    &watcher_ready,
                    external_baseline,
                );

                let Some(init) = init else {
                    Self::set_semantic_runtime_status(
                        &semantic_runtime,
                        SemanticRuntimeStatus::Failed(
                            "workspace search engine initialization failed".to_owned(),
                        ),
                    );
                    tracing::warn!("workspace search engine initialization failed");
                    return;
                };

                let semantic_status =
                    Self::semantic_runtime_status_for_mode(&init.engine, &init.mode);
                let needs_overlay_warmup =
                    matches!(init.mode, WorkspaceSearchMode::PostgresRemoteOverlay);

                if let Ok(mut guard) = search_engine.lock() {
                    *guard = Some(init.engine);
                }

                Self::set_semantic_runtime_status(&semantic_runtime, semantic_status);

                tracing::info!("search engine initialization complete");

                if needs_overlay_warmup {
                    Self::set_semantic_runtime_status(
                        &semantic_runtime,
                        SemanticRuntimeStatus::OverlaySyncing,
                    );
                    let search_engine = Arc::clone(&search_engine);
                    let semantic_runtime = Arc::clone(&semantic_runtime);
                    std::thread::Builder::new()
                        .name("bsl-search-overlay-warmup".to_owned())
                        .spawn(move || {
                            tracing::info!("workspace overlay semantic warmup started");
                            let result = match search_engine.lock() {
                                Ok(guard) => match guard.as_ref() {
                                    Some(engine) => engine.prime_workspace_overlay(),
                                    None => return,
                                },
                                Err(e) => {
                                    tracing::warn!("overlay warmup: engine lock error: {e}");
                                    return;
                                }
                            };
                            match result {
                                Ok(()) => {
                                    tracing::info!("workspace overlay semantic warmup complete");
                                }
                                Err(error) => {
                                    tracing::warn!(
                                        "workspace overlay semantic warmup failed: {error}"
                                    );
                                }
                            }
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
        let search_engine: Arc<Mutex<Option<SearchEngine>>> = Arc::new(Mutex::new(None));
        let index_progress = IndexProgress::new();
        let semantic_runtime = Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled));
        let project_config = project_root.as_deref().and_then(project_model::ProjectConfig::load);
        let baseline_runtime = BaselineRuntime::reference(project_config.as_ref());

        {
            let engine_arc = Arc::clone(&search_engine);
            let progress_arc = Arc::clone(&index_progress);
            let semantic_runtime_arc = Arc::clone(&semantic_runtime);
            let external_baseline = baseline_runtime.external_baseline.clone();
            std::thread::Builder::new()
                .name("bsl-search-reference-init".to_owned())
                .spawn(move || {
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
            configuration: Arc::new(RwLock::new(None)),
            extensions: Arc::new(RwLock::new(Vec::new())),
            workspace_root: None,
            onec_client: None,
            debug_session: Arc::new(Mutex::new(None)),
            search_engine,
            index_progress,
            semantic_runtime,
            workspace_search_mode: WorkspaceSearchMode::SqliteLocal,
            external_baseline: baseline_runtime.external_baseline,
            configured_baseline: Some(baseline_runtime.configured_baseline),
            graph: GraphState::disabled(),
        }
    }

    pub fn shared() -> Self {
        Self {
            configuration: Arc::new(RwLock::new(None)),
            extensions: Arc::new(RwLock::new(Vec::new())),
            workspace_root: None,
            onec_client: None,
            debug_session: Arc::new(Mutex::new(None)),
            search_engine: Arc::new(Mutex::new(None)),
            index_progress: IndexProgress::new(),
            semantic_runtime: Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            workspace_search_mode: WorkspaceSearchMode::SqliteLocal,
            external_baseline: None,
            configured_baseline: None,
            graph: GraphState::disabled(),
        }
    }

    pub(crate) fn graph(&self) -> &GraphState {
        &self.graph
    }

    pub fn set_onec_client(&mut self, client: OnecClient) {
        self.onec_client = Some(client);
    }

    pub fn onec_client(&self) -> Option<&OnecClient> {
        self.onec_client.as_ref()
    }

    pub async fn update_configuration(&self, config: Configuration) {
        *self.configuration.write().await = Some(config);
    }

    pub fn update_configuration_blocking(&self, config: Configuration) {
        *self.configuration.blocking_write() = Some(config);
    }

    pub fn set_workspace_root(&mut self, root: PathBuf) {
        self.workspace_root = Some(root);
    }

    pub async fn configuration(&self) -> Option<Configuration> {
        self.configuration.read().await.clone()
    }

    pub async fn with_configuration<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&Configuration) -> R,
    {
        let guard = self.configuration.read().await;
        guard.as_ref().map(f)
    }

    pub fn configuration_arc(&self) -> Option<std::sync::Arc<Configuration>> {
        let guard = self.configuration.blocking_read();
        guard.as_ref().map(|c| std::sync::Arc::new(c.clone()))
    }

    pub async fn extensions(&self) -> Vec<(String, Configuration)> {
        self.extensions.read().await.clone()
    }

    pub async fn with_extensions<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&[(String, Configuration)]) -> R,
    {
        let guard = self.extensions.read().await;
        f(&guard)
    }

    pub fn workspace_root(&self) -> Option<&PathBuf> {
        self.workspace_root.as_ref()
    }

    pub fn debug_session(&self) -> &Arc<Mutex<Option<bsl_debug::session::DebugSession>>> {
        &self.debug_session
    }

    pub fn search_engine(&self) -> &Arc<Mutex<Option<SearchEngine>>> {
        &self.search_engine
    }

    pub fn index_progress(&self) -> &Arc<IndexProgress> {
        &self.index_progress
    }

    pub(crate) fn semantic_runtime(&self) -> Arc<Mutex<SemanticRuntimeStatus>> {
        Arc::clone(&self.semantic_runtime)
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
    }

    fn embedding_config() -> Option<bsl_search::SearchConfig> {
        let base_url = std::env::var("EMBEDDING_URL").ok()?;
        let model = std::env::var("EMBEDDING_MODEL")
            .unwrap_or_else(|_| "Qwen/Qwen3-Embedding-0.6B".to_owned());
        let dim: usize =
            std::env::var("EMBEDDING_DIM").ok().and_then(|s| s.parse().ok()).unwrap_or(1024);
        let concurrency: usize =
            std::env::var("EMBEDDING_CONCURRENCY").ok().and_then(|s| s.parse().ok()).unwrap_or(10);

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
        progress: &Arc<IndexProgress>,
        watcher_ready: &Arc<AtomicBool>,
        external_baseline: Option<Arc<ExternalBaselineService>>,
    ) -> Option<WorkspaceSearchInit> {
        let build_dir = workspace_root.join(".build");
        std::fs::create_dir_all(&build_dir).ok();
        let db_path = build_dir.join("bsl-search.db");

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
            });
        }

        let mut engine = Self::open_search_engine(&db_path)?;

        if engine.has_semantic() {
            let code_embeddings = engine.embedding_count_by_collection("code").unwrap_or(0);
            let code_chunks = engine.chunk_count().unwrap_or(0);
            if code_chunks > 0 && code_embeddings < code_chunks {
                let cleared = engine.clear_file_hashes_without_embeddings("code").unwrap_or(0);
                if cleared > 0 {
                    tracing::info!(
                        code_embeddings,
                        code_chunks,
                        cleared,
                        "cleared hashes for code files without embeddings"
                    );
                }
            }
        }

        Self::configure_workspace_engine(
            &mut engine,
            &source_path,
            watcher_ready,
            BaselineHashMode::RawFileBytes,
        );

        if engine.has_semantic() {
            match engine.index_directory(&source_path, Some(progress)) {
                Ok(indexed) => {
                    if indexed > 0 {
                        tracing::info!(indexed, "FTS + semantic index updated");
                    }
                }
                Err(e) => {
                    tracing::warn!("failed to build semantic index, falling back to FTS: {e}");
                    if engine.chunk_count().unwrap_or(0) == 0 {
                        match engine.index_directory_fts(&source_path) {
                            Ok(indexed) => tracing::info!(indexed, "FTS index built (fallback)"),
                            Err(e2) => tracing::warn!("failed to build FTS index: {e2}"),
                        }
                    }
                }
            }
        } else if engine.chunk_count().unwrap_or(0) == 0 {
            tracing::info!(?source_path, "building FTS index from source files");
            match engine.index_directory_fts(&source_path) {
                Ok(indexed) => tracing::info!(indexed, "FTS index built"),
                Err(e) => tracing::warn!("failed to build FTS index: {e}"),
            }
        }

        Some(WorkspaceSearchInit { engine, mode: WorkspaceSearchMode::SqliteLocal })
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
                root,
                watcher_ready,
                self.external_baseline.clone(),
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

    fn run_workspace_overlay_watcher(
        engine: Arc<Mutex<Option<SearchEngine>>>,
        watch_root: PathBuf,
        watcher_ready: Arc<AtomicBool>,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match RecommendedWatcher::new(
            move |event| {
                let _ = tx.send(event);
            },
            NotifyConfig::default(),
        ) {
            Ok(watcher) => watcher,
            Err(e) => {
                tracing::warn!(?watch_root, "failed to create workspace watcher: {e}");
                return;
            }
        };

        if let Err(e) = watcher.watch(&watch_root, RecursiveMode::Recursive) {
            tracing::warn!(?watch_root, "failed to watch workspace for overlay updates: {e}");
            return;
        }

        watcher_ready.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = engine.lock() {
            if let Some(engine) = guard.as_mut() {
                engine.enable_workspace_watcher_mode();
            }
        }
        tracing::info!(?watch_root, "workspace overlay watcher started");

        while let Ok(event) = rx.recv() {
            match event {
                Ok(event) => Self::handle_workspace_watch_event(&engine, &event),
                Err(e) => tracing::warn!(?watch_root, "workspace watch event error: {e}"),
            }
        }
    }

    fn handle_workspace_watch_event(engine: &Arc<Mutex<Option<SearchEngine>>>, event: &Event) {
        if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_))
        {
            return;
        }

        for path in &event.paths {
            if !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")) {
                continue;
            }

            if let Ok(guard) = engine.lock() {
                if let Some(engine) = guard.as_ref() {
                    if let Err(e) = engine.mark_workspace_path_dirty(path) {
                        tracing::warn!(?path, "failed to mark workspace file dirty: {e}");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SharedState;
    use crate::baseline::{ExternalBaselineService, RefreshableExternalBaselineSource};
    use bsl_search::{
        BaselineRef, CorpusId, Document, ExternalBaselineConfig, IndexProgress, IndexedDocument,
        SearchEngine,
    };
    use std::env;
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
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
        fs::create_dir_all(workspace.join(".build")).unwrap();

        let db_path = workspace.join(".build").join("bsl-search.db");
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
                }],
                None,
            )
            .unwrap();
        assert_eq!(stale_engine.file_count().unwrap(), 1);
        drop(stale_engine);

        let progress = Arc::new(IndexProgress::default());
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
            &progress,
            &watcher_ready,
            Some(external),
        );

        assert!(init.is_none());
        let reopened = SearchEngine::fts_only(&db_path).unwrap();
        assert_eq!(reopened.file_count().unwrap(), 0);
        assert!(reopened.text_search("ПризрачнаяПроцедура", 10, Some("code")).unwrap().is_empty());
        assert!(reopened.store().load_baseline_manifest().unwrap().is_none());
        assert_eq!(progress.done_batches.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn workspace_external_failure_with_embeddings_fails_closed_without_hybrid_warmup() {
        let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let _embedding_url = EnvVarGuard::set("EMBEDDING_URL", "http://127.0.0.1:9/v1");

        let dir = tempdir().unwrap();
        let workspace = dir.path();
        fs::create_dir_all(workspace.join(".build")).unwrap();
        let db_path = workspace.join(".build").join("bsl-search.db");
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
                }],
                None,
            )
            .unwrap();
        drop(stale_engine);

        let progress = Arc::new(IndexProgress::default());
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
            &progress,
            &watcher_ready,
            Some(external),
        );

        assert!(init.is_none());
        let reopened = SearchEngine::fts_only(&db_path).unwrap();
        assert_eq!(reopened.file_count().unwrap(), 0);
        assert!(reopened.store().load_baseline_manifest().unwrap().is_none());
        assert_eq!(progress.done_batches.load(Ordering::Relaxed), 0);
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
}
