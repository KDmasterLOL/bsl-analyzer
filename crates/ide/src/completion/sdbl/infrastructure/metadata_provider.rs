//! Database-backed metadata provider.

use crate::completion::sdbl::domain::MetadataProvider;
use bsl_metadata::Configuration;
use ide_db::RootDatabase;
use std::sync::Arc;
use vfs::FileId;

/// Metadata provider that fetches Configuration from RootDatabase.
///
/// Uses Salsa-cached configuration loading for efficient reuse.
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
        // Use RootDatabase::get_configuration which is Salsa-cached
        self.db.get_configuration(self.file_id)
    }
}
