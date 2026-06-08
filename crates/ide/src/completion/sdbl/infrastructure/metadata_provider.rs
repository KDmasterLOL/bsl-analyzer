use crate::completion::sdbl::domain::MetadataProvider;
use bsl_metadata::{Configuration, MdoType, MetadataObject, Register};
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

    fn resolve_metadata_object(
        &self,
        mdo_type: MdoType,
        name: &str,
    ) -> Option<Arc<MetadataObject>> {
        self.db.resolve_metadata_object(self.file_id, mdo_type, name)
    }

    fn resolve_register(&self, mdo_type: MdoType, name: &str) -> Option<Arc<Register>> {
        self.db.resolve_register(self.file_id, mdo_type, name)
    }
}
