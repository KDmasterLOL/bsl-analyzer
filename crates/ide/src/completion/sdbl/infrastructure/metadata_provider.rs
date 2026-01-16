//! Database-backed metadata provider.

use crate::completion::sdbl::domain::MetadataProvider;
use bsl_metadata::Configuration;
use ide_db::RootDatabase;
use std::path::Path;
use std::sync::Arc;
use vfs::FileId;

/// Metadata provider that fetches Configuration from RootDatabase.
pub struct DbMetadataProvider<'a> {
    // These fields are kept for future Salsa-based caching implementation
    #[allow(dead_code)]
    db: &'a dyn RootDatabase,
    #[allow(dead_code)]
    file_id: FileId,
    workspace_root: Option<&'a Path>,
}

impl<'a> DbMetadataProvider<'a> {
    /// Create with explicit workspace root.
    pub fn with_workspace(
        db: &'a dyn RootDatabase,
        file_id: FileId,
        workspace_root: Option<&'a Path>,
    ) -> Self {
        Self { db, file_id, workspace_root }
    }
}

impl MetadataProvider for DbMetadataProvider<'_> {
    fn get_configuration(&self) -> Option<Arc<Configuration>> {
        // If workspace_root provided, try to find configuration
        let root = self.workspace_root?;

        tracing::debug!(
            workspace_root = ?root,
            "DbMetadataProvider: searching for configuration"
        );

        let config_path = crate::config_finder::find_configuration_path(root)?;

        match bsl_metadata::load_from_directory(&config_path) {
            Ok(config) => {
                tracing::debug!(
                    config_path = ?config_path,
                    "DbMetadataProvider: loaded metadata"
                );
                Some(Arc::new(config))
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    config_path = ?config_path,
                    "DbMetadataProvider: failed to load metadata"
                );
                None
            }
        }
    }
}
