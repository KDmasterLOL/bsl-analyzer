//! Database-backed metadata provider.

use crate::completion::sdbl::domain::MetadataProvider;
use bsl_metadata::Configuration;
use ide_db::RootDatabase;
use std::sync::Arc;
use vfs::FileId;

/// Metadata provider that fetches Configuration from RootDatabase.
pub struct DbMetadataProvider<'a> {
    db: &'a dyn RootDatabase,
    file_id: FileId,
}

impl<'a> DbMetadataProvider<'a> {
    /// Create a new provider for given database and file.
    pub fn new(db: &'a dyn RootDatabase, file_id: FileId) -> Self {
        Self { db, file_id }
    }
}

impl MetadataProvider for DbMetadataProvider<'_> {
    fn get_configuration(&self) -> Option<Arc<Configuration>> {
        // TODO: Implement proper metadata loading from RootDatabase
        // For now, return None - this will be implemented when migrating use cases
        let _ = (self.db, self.file_id);
        None
    }
}
