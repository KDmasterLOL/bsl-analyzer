//! VFS helper functions for accessing file paths and configuration roots.
//!
//! These functions downcast the database to `RootDatabaseImpl` to access
//! VFS and file system operations that are not part of the trait interface.

use std::path::{Path, PathBuf};

use vfs::FileId;

use crate::{RootDatabase, RootDatabaseImpl};

/// Get file path for SDBL HIR loading.
pub(crate) fn get_file_path_for_sdbl(db: &dyn RootDatabase, file_id: FileId) -> Option<PathBuf> {
    let db_impl = db.as_any().downcast_ref::<RootDatabaseImpl>()?;
    db_impl.get_file_path(file_id)
}

/// Find configuration root for SDBL HIR loading.
pub(crate) fn find_configuration_root_for_sdbl(
    db: &dyn RootDatabase,
    file_path: &Path,
) -> Option<PathBuf> {
    let db_impl = db.as_any().downcast_ref::<RootDatabaseImpl>()?;
    db_impl.find_configuration_root(file_path)
}

/// Get file path for metadata loading.
///
/// This function provides VFS access for the Salsa query.
/// It downcasts the database to RootDatabaseImpl to access file path resolution.
pub(crate) fn get_file_path_for_metadata(
    db: &dyn RootDatabase,
    file_id: FileId,
) -> Option<PathBuf> {
    // Downcast to concrete type to access get_file_path method
    let db_impl = db.as_any().downcast_ref::<RootDatabaseImpl>()?;
    db_impl.get_file_path(file_id)
}

/// Find configuration root for metadata loading.
///
/// This function provides file system access for the Salsa query.
/// It downcasts the database to RootDatabaseImpl to access configuration search.
pub(crate) fn find_configuration_root_for_metadata(
    db: &dyn RootDatabase,
    file_path: &Path,
) -> Option<PathBuf> {
    // Downcast to concrete type to access find_configuration_root method
    let db_impl = db.as_any().downcast_ref::<RootDatabaseImpl>()?;
    db_impl.find_configuration_root(file_path)
}
