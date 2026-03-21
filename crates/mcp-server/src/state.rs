//! Shared state for MCP server tools.

use bsl_metadata::Configuration;
use bsl_platform::PlatformDataInner;
use bsl_search::{Document, IndexProgress, SearchEngine};
use onec_client::Client as OnecClient;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
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
    workspace_root: Option<PathBuf>,
    onec_client: Option<OnecClient>,
    debug_session: Arc<Mutex<Option<bsl_debug::session::DebugSession>>>,
    search_engine: Arc<Mutex<Option<SearchEngine>>>,
    index_progress: Arc<IndexProgress>,
}

impl SharedState {
    /// Create state for standalone MCP mode (loads configuration from directory).
    pub fn standalone(source_dir: PathBuf) -> Self {
        let configuration = bsl_metadata::load_from_directory(&source_dir)
            .map_err(|e| {
                tracing::warn!(?source_dir, "failed to load configuration: {e}");
                e
            })
            .ok();

        let search_engine = Self::init_search_engine(&source_dir);

        Self {
            configuration: Arc::new(RwLock::new(configuration)),
            workspace_root: Some(source_dir),
            onec_client: None,
            debug_session: Arc::new(Mutex::new(None)),
            search_engine: Arc::new(Mutex::new(search_engine)),
            index_progress: IndexProgress::new(),
        }
    }

    /// Create state for LSP+MCP shared mode.
    ///
    /// Configuration will be set later via `update_configuration`.
    pub fn shared() -> Self {
        Self {
            configuration: Arc::new(RwLock::new(None)),
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

    /// Initialize search engine from workspace root.
    ///
    /// If a DB exists, opens it. Otherwise builds a FTS-only index from source files.
    /// If EMBEDDING_URL is set, enables semantic search too.
    fn init_search_engine(workspace_root: &std::path::Path) -> Option<SearchEngine> {
        let build_dir = workspace_root.join(".build");
        std::fs::create_dir_all(&build_dir).ok();
        let db_path = build_dir.join("bsl-search.db");

        let has_embedder = std::env::var("EMBEDDING_URL").is_ok();

        let mut engine = if has_embedder {
            let base_url = std::env::var("EMBEDDING_URL").unwrap();
            let model = std::env::var("EMBEDDING_MODEL")
                .unwrap_or_else(|_| "Qwen/Qwen3-Embedding-0.6B".to_owned());
            let dim: usize =
                std::env::var("EMBEDDING_DIM").ok().and_then(|s| s.parse().ok()).unwrap_or(1024);

            let concurrency: usize = std::env::var("EMBEDDING_CONCURRENCY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10);

            let config = bsl_search::SearchConfig {
                embedder: bsl_search::EmbedderConfig {
                    base_url,
                    model: model.clone(),
                    dim: Some(dim),
                    api_key: std::env::var("EMBEDDING_API_KEY").ok(),
                },
                batch_size: 32,
                concurrency,
            };

            match SearchEngine::new(&db_path, config) {
                Ok(engine) => {
                    tracing::info!(
                        files = engine.file_count().unwrap_or(0),
                        chunks = engine.chunk_count().unwrap_or(0),
                        vectors = engine.vector_count(),
                        model,
                        "search engine loaded (FTS + semantic)"
                    );
                    engine
                }
                Err(e) => {
                    tracing::warn!("failed to init search engine with embedder: {e}");
                    return None;
                }
            }
        } else {
            match SearchEngine::fts_only(&db_path) {
                Ok(engine) => {
                    tracing::info!(
                        files = engine.file_count().unwrap_or(0),
                        chunks = engine.chunk_count().unwrap_or(0),
                        "search engine loaded (FTS-only)"
                    );
                    engine
                }
                Err(e) => {
                    tracing::warn!("failed to init FTS-only search engine: {e}");
                    return None;
                }
            }
        };

        // Auto-build FTS index if DB is empty.
        if engine.chunk_count().unwrap_or(0) == 0 {
            let project = project_model::Project::new(workspace_root);
            let source_path = project.source_path();
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

        // Index platform reference documentation.
        Self::index_platform_docs(&mut engine);

        Some(engine)
    }

    /// Index platform reference documentation into the search engine.
    ///
    /// Converts all platform types, methods, and global functions into
    /// searchable documents. Uses a version hash to skip re-indexing
    /// if data hasn't changed.
    fn index_platform_docs(engine: &mut SearchEngine) {
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

        match engine.index_documents("platform", "platform://docs", version_bytes, &documents, None)
        {
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
            let engine = Self::init_search_engine(root);
            if let Ok(mut guard) = self.search_engine.lock() {
                *guard = engine;
            }
        }
    }
}
