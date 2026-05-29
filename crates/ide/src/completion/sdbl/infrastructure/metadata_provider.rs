use crate::completion::sdbl::domain::MetadataProvider;
use bsl_metadata::Configuration;
use ide_db::RootDatabase;
use std::sync::Arc;
use vfs::FileId;

pub struct DbMetadataProvider<'a> {
    db: &'a dyn RootDatabase,
    file_id: FileId,
}

impl<'a> DbMetadataProvider<'a> {
    pub fn new(db: &'a dyn RootDatabase, file_id: FileId) -> Self {
        Self { db, file_id }
    }
}

impl MetadataProvider for DbMetadataProvider<'_> {
    fn get_configuration(&self) -> Option<Arc<Configuration>> {
        self.db.get_configuration(self.file_id)
    }
}
