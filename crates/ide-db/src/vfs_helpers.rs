use std::path::{Path, PathBuf};

use vfs::FileId;

use crate::{RootDatabase, RootDatabaseImpl};

pub fn get_file_path(db: &dyn RootDatabase, file_id: FileId) -> Option<PathBuf> {
    let db_impl = db.as_any().downcast_ref::<RootDatabaseImpl>()?;
    db_impl.get_file_path(file_id)
}

pub(crate) fn find_configuration_root(db: &dyn RootDatabase, file_path: &Path) -> Option<PathBuf> {
    let db_impl = db.as_any().downcast_ref::<RootDatabaseImpl>()?;
    db_impl.find_configuration_root(file_path)
}
