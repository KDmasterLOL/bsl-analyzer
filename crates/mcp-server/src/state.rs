//! Shared state for MCP server tools.

use bsl_metadata::Configuration;
use bsl_platform::PlatformDataInner;
use bsl_search::{Document, IndexProgress, SearchEngine};
use onec_client::Client as OnecClient;
use std::sync::{Arc, Mutex};
use std::{
    env,
    path::{Path, PathBuf},
};
use tokio::sync::RwLock;

/// State shared between all MCP tool handlers.
///
/// Contains the 1C configuration metadata, workspace info,
/// and optional HTTP client to a live 1C database.
/// Thread-safe: can be cloned and shared across async tasks.
///
/// In LSP+MCP mode, Configuration is updated when LSP reloads it.
/// In standalone mode, Configuration is loaded once at startup.
#[derive(Clone)]
pub struct SharedState {
    configuration: Arc<RwLock<Option<Configuration>>>,
    /// Extension configurations: (name, Configuration).
    extensions: Arc<RwLock<Vec<(String, Configuration)>>>,
    workspace_root: Option<PathBuf>,
    onec_client: Option<OnecClient>,
    debug_session: Arc<Mutex<Option<bsl_debug::session::DebugSession>>>,
    search_engine: Arc<Mutex<Option<SearchEngine>>>,
    index_progress: Arc<IndexProgress>,
}

impl SharedState {
    /// Create state for workspace MCP mode (loads configuration from directory).
    ///
    /// Returns immediately. Metadata loading is synchronous (~1-2s).
    /// Search engine initialization (FTS indexing) runs in a background thread.
    pub fn workspace(source_dir: PathBuf) -> Self {
        // Use Project to discover configuration path (configurationRoot,
        // recursive Configuration.xml search, common patterns).
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

        // Spawn background thread so standalone() returns immediately.
        // MCP tools check engine readiness and return a friendly message while init is in progress.
        {
            let engine_arc = Arc::clone(&search_engine);
            let progress_arc = Arc::clone(&index_progress);
            let root = source_dir.clone();
            std::thread::Builder::new()
                .name("bsl-search-init".to_owned())
                .spawn(move || {
                    tracing::info!("search engine initialization started in background");
                    let engine = Self::init_workspace_search_engine(&root, &progress_arc);
                    if let Ok(mut guard) = engine_arc.lock() {
                        *guard = engine;
                    }
                    tracing::info!("search engine initialization complete");
                })
                .ok();
        }

        // Load extension configurations
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

        Self {
            configuration: Arc::new(RwLock::new(configuration)),
            extensions: Arc::new(RwLock::new(extensions)),
            workspace_root: Some(source_dir),
            onec_client: None,
            debug_session: Arc::new(Mutex::new(None)),
            search_engine,
            index_progress,
        }
    }

    /// Create state for global reference MCP mode.
    ///
    /// Loads no project metadata and builds a user-level docs-only search index in the background.
    pub fn reference() -> Self {
        let search_engine: Arc<Mutex<Option<SearchEngine>>> = Arc::new(Mutex::new(None));
        let index_progress = IndexProgress::new();

        {
            let engine_arc = Arc::clone(&search_engine);
            let progress_arc = Arc::clone(&index_progress);
            std::thread::Builder::new()
                .name("bsl-search-reference-init".to_owned())
                .spawn(move || {
                    tracing::info!("reference search engine initialization started in background");
                    let engine = Self::init_reference_search_engine(&progress_arc);
                    if let Ok(mut guard) = engine_arc.lock() {
                        *guard = engine;
                    }
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
        }
    }

    /// Create state for LSP+MCP shared mode.
    ///
    /// Configuration will be set later via `update_configuration`.
    pub fn shared() -> Self {
        Self {
            configuration: Arc::new(RwLock::new(None)),
            extensions: Arc::new(RwLock::new(Vec::new())),
            workspace_root: None,
            onec_client: None,
            debug_session: Arc::new(Mutex::new(None)),
            search_engine: Arc::new(Mutex::new(None)),
            index_progress: IndexProgress::new(),
        }
    }

    /// Set the 1C HTTP client for live database access.
    pub fn set_onec_client(&mut self, client: OnecClient) {
        self.onec_client = Some(client);
    }

    /// Get the 1C HTTP client.
    pub fn onec_client(&self) -> Option<&OnecClient> {
        self.onec_client.as_ref()
    }

    /// Update configuration (called from LSP main thread when config reloads).
    pub async fn update_configuration(&self, config: Configuration) {
        *self.configuration.write().await = Some(config);
    }

    /// Update configuration from a sync context (LSP main thread).
    /// Panics if called from an async context.
    pub fn update_configuration_blocking(&self, config: Configuration) {
        *self.configuration.blocking_write() = Some(config);
    }

    /// Set workspace root.
    pub fn set_workspace_root(&mut self, root: PathBuf) {
        self.workspace_root = Some(root);
    }

    /// Get a clone of the current configuration.
    pub async fn configuration(&self) -> Option<Configuration> {
        self.configuration.read().await.clone()
    }

    /// Access configuration with a closure (avoids clone for read-only access).
    pub async fn with_configuration<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&Configuration) -> R,
    {
        let guard = self.configuration.read().await;
        guard.as_ref().map(f)
    }

    /// Get configuration as Arc for SDBL HIR lowering (sync, blocking).
    pub fn configuration_arc(&self) -> Option<std::sync::Arc<Configuration>> {
        let guard = self.configuration.blocking_read();
        guard.as_ref().map(|c| std::sync::Arc::new(c.clone()))
    }

    /// Get extension configurations.
    pub async fn extensions(&self) -> Vec<(String, Configuration)> {
        self.extensions.read().await.clone()
    }

    /// Access extensions with a closure (avoids clone).
    pub async fn with_extensions<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&[(String, Configuration)]) -> R,
    {
        let guard = self.extensions.read().await;
        f(&guard)
    }

    /// Workspace root directory.
    pub fn workspace_root(&self) -> Option<&PathBuf> {
        self.workspace_root.as_ref()
    }

    /// Access the debug session mutex.
    pub fn debug_session(&self) -> &Arc<Mutex<Option<bsl_debug::session::DebugSession>>> {
        &self.debug_session
    }

    /// Access the search engine mutex.
    pub fn search_engine(&self) -> &Arc<Mutex<Option<SearchEngine>>> {
        &self.search_engine
    }

    /// Access the indexing progress tracker.
    pub fn index_progress(&self) -> &Arc<IndexProgress> {
        &self.index_progress
    }

    /// Read embedding configuration from environment variables.
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
            },
            batch_size: 32,
            concurrency,
        })
    }

    /// Open search engine from DB, creating it if needed.
    fn open_search_engine(db_path: &Path) -> Option<SearchEngine> {
        if let Some(config) = Self::embedding_config() {
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
        } else {
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
    }

    /// Initialize search engine from workspace root.
    ///
    /// If a DB exists, opens it. Otherwise builds index from source files.
    /// If EMBEDDING_URL is set, enables semantic search too.
    /// If DB has FTS data but no embeddings, rebuilds for semantic upgrade.
    fn init_workspace_search_engine(
        workspace_root: &std::path::Path,
        progress: &Arc<IndexProgress>,
    ) -> Option<SearchEngine> {
        let build_dir = workspace_root.join(".build");
        std::fs::create_dir_all(&build_dir).ok();
        let db_path = build_dir.join("bsl-search.db");

        let mut engine = Self::open_search_engine(&db_path)?;

        // If DB has code chunks without embeddings and embedder is available,
        // clear their file hashes so index_directory will re-process them.
        if engine.has_semantic() {
            let code_embeddings = engine.embedding_count_by_collection("code").unwrap_or(0);
            let code_chunks = engine.chunk_count().unwrap_or(0);
            if code_chunks > 0 && code_embeddings < code_chunks {
                // Some or all code files lack embeddings — clear hashes for unembedded files.
                // index_directory will skip files whose hash matches (already indexed with
                // embeddings) and re-process the rest.
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

        {
            let project = project_model::Project::new(workspace_root);
            let source_path = project.source_path();
            engine.set_workspace_root(source_path.to_path_buf());

            if engine.has_semantic() {
                // Always call index_directory — it skips files by hash.
                // Files without embeddings had their hashes cleared above.
                match engine.index_directory(source_path, Some(progress)) {
                    Ok(indexed) => {
                        if indexed > 0 {
                            tracing::info!(indexed, "FTS + semantic index updated");
                        }
                    }
                    Err(e) => {
                        tracing::warn!("failed to build semantic index, falling back to FTS: {e}");
                        if engine.chunk_count().unwrap_or(0) == 0 {
                            match engine.index_directory_fts(source_path) {
                                Ok(indexed) => {
                                    tracing::info!(indexed, "FTS index built (fallback)")
                                }
                                Err(e2) => tracing::warn!("failed to build FTS index: {e2}"),
                            }
                        }
                    }
                }
            } else if engine.chunk_count().unwrap_or(0) == 0 {
                tracing::info!(?source_path, "building FTS index from source files");
                match engine.index_directory_fts(source_path) {
                    Ok(indexed) => {
                        tracing::info!(indexed, "FTS index built");
                    }
                    Err(e) => {
                        tracing::warn!("failed to build FTS index: {e}");
                    }
                }
            }
        }

        Some(engine)
    }

    fn init_reference_search_engine(progress: &Arc<IndexProgress>) -> Option<SearchEngine> {
        let db_path = Self::reference_search_db_path()?;
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let mut engine = Self::open_search_engine(&db_path)?;
        Self::index_platform_docs(&mut engine, progress);
        Some(engine)
    }

    /// Index platform reference documentation into the search engine.
    ///
    /// Converts all platform types, methods, and global functions into
    /// searchable documents. Uses a version hash to skip re-indexing
    /// if data hasn't changed.
    fn index_platform_docs(engine: &mut SearchEngine, progress: &Arc<IndexProgress>) {
        let platform = PlatformDataInner::instance();
        if platform.all_types().is_empty() {
            tracing::debug!("no platform data available, skipping docs indexing");
            return;
        }

        let mut documents = Vec::new();

        // Index platform types.
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

        // Index platform methods with documentation.
        for method in platform.all_methods() {
            let mut body = format!(
                "Тип: {}\nМетод: {} / {}\n",
                method.type_name, method.name, method.english_name,
            );
            if let Some(ref ret) = method.return_type {
                body.push_str(&format!("Возвращает: {ret}\n"));
            }
            // Add documentation if available.
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

        // Index global functions.
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

        // Use package version as hash — platform data changes only with new releases.
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

    /// Initialize search engine for LSP+MCP mode (called when workspace root is set).
    pub fn init_search(&self) {
        if let Some(ref root) = self.workspace_root {
            let engine = Self::init_workspace_search_engine(root, &self.index_progress);
            if let Ok(mut guard) = self.search_engine.lock() {
                *guard = engine;
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
