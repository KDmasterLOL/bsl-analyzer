//! FileSet - bidirectional mapping of FileId ↔ VfsPath for a collection of files.
//!
//! A `FileSet` represents a logical grouping of files, typically corresponding to
//! a source root (like a project directory or library). It maintains bidirectional
//! mappings for efficient lookup in both directions.

use indexmap::IndexMap;
use rustc_hash::{FxBuildHasher, FxHashMap};

use crate::{FileId, VfsPath};

/// A set of files with bidirectional FileId ↔ VfsPath mapping.
///
/// FileSet is used to group files logically (e.g., all files in a project)
/// and provides O(1) lookups in both directions.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct FileSet {
    /// Path → FileId mapping for fast lookup by path
    files: FxHashMap<VfsPath, FileId>,
    /// FileId → Path mapping for fast lookup by ID, maintains insertion order
    paths: IndexMap<FileId, VfsPath, FxBuildHasher>,
}

impl FileSet {
    /// Create a new empty FileSet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of files in this set.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Returns true if this set contains no files.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Insert a file into this set.
    ///
    /// If the path or FileId already exists, it will be updated.
    pub fn insert(&mut self, file_id: FileId, path: VfsPath) {
        self.files.insert(path.clone(), file_id);
        self.paths.insert(file_id, path);
    }

    /// Look up the FileId for a given path.
    ///
    /// Returns `None` if the path is not in this set.
    pub fn file_for_path(&self, path: &VfsPath) -> Option<&FileId> {
        self.files.get(path)
    }

    /// Look up the VfsPath for a given FileId.
    ///
    /// Returns `None` if the FileId is not in this set.
    pub fn path_for_file(&self, file: &FileId) -> Option<&VfsPath> {
        self.paths.get(file)
    }

    /// Iterate over all FileIds in this set.
    ///
    /// The iteration order is the insertion order.
    pub fn iter(&self) -> impl Iterator<Item = FileId> + '_ {
        self.paths.keys().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_empty_file_set() {
        let set = FileSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn test_insert_and_lookup() {
        let mut set = FileSet::new();
        let file_id = FileId(0);
        let path = VfsPath::from(PathBuf::from("/test.bsl"));

        set.insert(file_id, path.clone());

        assert_eq!(set.len(), 1);
        assert_eq!(set.file_for_path(&path), Some(&file_id));
        assert_eq!(set.path_for_file(&file_id), Some(&path));
    }

    #[test]
    fn test_multiple_files() {
        let mut set = FileSet::new();
        let id1 = FileId(0);
        let id2 = FileId(1);
        let id3 = FileId(2);
        let path1 = VfsPath::from(PathBuf::from("/test1.bsl"));
        let path2 = VfsPath::from(PathBuf::from("/test2.bsl"));
        let path3 = VfsPath::from(PathBuf::from("/test3.bsl"));

        set.insert(id1, path1.clone());
        set.insert(id2, path2.clone());
        set.insert(id3, path3.clone());

        assert_eq!(set.len(), 3);
        assert_eq!(set.file_for_path(&path1), Some(&id1));
        assert_eq!(set.file_for_path(&path2), Some(&id2));
        assert_eq!(set.file_for_path(&path3), Some(&id3));
    }

    #[test]
    fn test_lookup_nonexistent() {
        let set = FileSet::new();
        let path = VfsPath::from(PathBuf::from("/nonexistent.bsl"));
        let id = FileId(999);

        assert_eq!(set.file_for_path(&path), None);
        assert_eq!(set.path_for_file(&id), None);
    }

    #[test]
    fn test_iter() {
        let mut set = FileSet::new();
        let id1 = FileId(0);
        let id2 = FileId(1);
        let id3 = FileId(2);
        let path1 = VfsPath::from(PathBuf::from("/test1.bsl"));
        let path2 = VfsPath::from(PathBuf::from("/test2.bsl"));
        let path3 = VfsPath::from(PathBuf::from("/test3.bsl"));

        set.insert(id1, path1);
        set.insert(id2, path2);
        set.insert(id3, path3);

        let ids: Vec<_> = set.iter().collect();
        assert_eq!(ids, vec![id1, id2, id3]);
    }

    #[test]
    fn test_update_existing() {
        let mut set = FileSet::new();
        let id1 = FileId(0);
        let id2 = FileId(1);
        let path = VfsPath::from(PathBuf::from("/test.bsl"));

        // Insert with id1
        set.insert(id1, path.clone());
        assert_eq!(set.file_for_path(&path), Some(&id1));

        // Update with id2 for same path
        set.insert(id2, path.clone());
        assert_eq!(set.file_for_path(&path), Some(&id2));
        // len() returns number of unique paths, which is 1
        // But we have 2 IDs in the paths map
        assert_eq!(set.len(), 1); // One unique path
        assert_eq!(set.iter().count(), 2); // Two IDs
    }

    #[test]
    fn test_clone() {
        let mut set1 = FileSet::new();
        let id = FileId(0);
        let path = VfsPath::from(PathBuf::from("/test.bsl"));

        set1.insert(id, path.clone());

        let set2 = set1.clone();
        assert_eq!(set2.len(), 1);
        assert_eq!(set2.file_for_path(&path), Some(&id));
    }
}
