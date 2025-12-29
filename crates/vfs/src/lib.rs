//! Virtual file system for bsl-analyzer.
//!
//! This crate provides an abstraction over the file system, allowing
//! the analyzer to work with files without direct filesystem access.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexMap;
use rustc_hash::{FxHashMap, FxBuildHasher};

/// Unique identifier for a file in the VFS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

/// Represents a path in the VFS.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VfsPath(PathBuf);

impl VfsPath {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, path: impl AsRef<Path>) -> Self {
        Self(self.0.join(path))
    }
}

impl From<PathBuf> for VfsPath {
    fn from(path: PathBuf) -> Self {
        Self(path)
    }
}

impl From<&Path> for VfsPath {
    fn from(path: &Path) -> Self {
        Self(path.to_path_buf())
    }
}

/// The virtual file system.
#[derive(Debug, Default)]
pub struct Vfs {
    files: IndexMap<VfsPath, Option<Arc<str>>, FxBuildHasher>,
    path_to_id: FxHashMap<VfsPath, FileId>,
    id_to_path: FxHashMap<FileId, VfsPath>,
    next_id: u32,
}

impl Vfs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Gets or creates a FileId for the given path.
    pub fn alloc_file_id(&mut self, path: VfsPath) -> FileId {
        if let Some(&id) = self.path_to_id.get(&path) {
            return id;
        }

        let id = FileId(self.next_id);
        self.next_id += 1;
        self.path_to_id.insert(path.clone(), id);
        self.id_to_path.insert(id, path.clone());
        self.files.insert(path, None);
        id
    }

    /// Sets the content of a file.
    pub fn set_file_content(&mut self, file_id: FileId, content: Option<Arc<str>>) {
        if let Some(path) = self.id_to_path.get(&file_id) {
            self.files.insert(path.clone(), content);
        }
    }

    /// Gets the content of a file.
    pub fn file_content(&self, file_id: FileId) -> Option<&Arc<str>> {
        let path = self.id_to_path.get(&file_id)?;
        self.files.get(path)?.as_ref()
    }

    /// Gets the path for a file ID.
    pub fn file_path(&self, file_id: FileId) -> Option<&VfsPath> {
        self.id_to_path.get(&file_id)
    }

    /// Gets the file ID for a path.
    pub fn file_id(&self, path: &VfsPath) -> Option<FileId> {
        self.path_to_id.get(path).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vfs_basic() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/test.bsl");
        let file_id = vfs.alloc_file_id(path.clone());

        assert_eq!(vfs.file_id(&path), Some(file_id));
        assert_eq!(vfs.file_path(file_id), Some(&path));

        vfs.set_file_content(file_id, Some(Arc::from("Процедура Тест() КонецПроцедуры")));
        assert!(vfs.file_content(file_id).is_some());
    }
}
