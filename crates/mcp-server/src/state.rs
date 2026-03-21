//! Shared state for MCP server tools.

use bsl_metadata::Configuration;
use bsl_search::SearchEngine;
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

    /// Initialize search engine from workspace root if DB exists.
    fn init_search_engine(workspace_root: &std::path::Path) -> Option<SearchEngine> {
        let db_path = workspace_root.join(".build/bsl-search.db");
        if !db_path.exists() {
            tracing::info!("search index not found at {}", db_path.display());
            return None;
        }

        let base_url =
            std::env::var("EMBEDDING_URL").unwrap_or_else(|_| "http://localhost:8090".to_owned());
        let model = std::env::var("EMBEDDING_MODEL")
            .unwrap_or_else(|_| "Qwen/Qwen3-Embedding-0.6B".to_owned());
        let dim: usize =
            std::env::var("EMBEDDING_DIM").ok().and_then(|s| s.parse().ok()).unwrap_or(1024);

        let config = bsl_search::SearchConfig {
            embedder: bsl_search::EmbedderConfig { base_url, model: model.clone(), dim: Some(dim) },
            batch_size: 32,
        };

        match SearchEngine::new(&db_path, config) {
            Ok(engine) => {
                tracing::info!(
                    files = engine.file_count().unwrap_or(0),
                    chunks = engine.chunk_count().unwrap_or(0),
                    vectors = engine.vector_count(),
                    model,
                    "search engine loaded"
                );
                Some(engine)
            }
            Err(e) => {
                tracing::warn!("failed to initialize search engine: {e}");
                None
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
