//! Shared state for MCP server tools.

use bsl_metadata::Configuration;
use onec_client::Client as OnecClient;
use std::path::PathBuf;
use std::sync::Arc;
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

        Self {
            configuration: Arc::new(RwLock::new(configuration)),
            workspace_root: Some(source_dir),
            onec_client: None,
        }
    }

    /// Create state for LSP+MCP shared mode.
    ///
    /// Configuration will be set later via `update_configuration`.
    pub fn shared() -> Self {
        Self { configuration: Arc::new(RwLock::new(None)), workspace_root: None, onec_client: None }
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
}
