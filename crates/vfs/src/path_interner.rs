//! Path interning for efficient FileId ↔ VfsPath bidirectional mapping.
//!
//! This module provides the `PathInterner` which uses an `IndexSet` to maintain
//! a bidirectional mapping between paths and file IDs. This allows for O(1) lookups
//! in both directions and ensures each path is stored only once.

use indexmap::IndexSet;
use rustc_hash::FxBuildHasher;

use crate::{FileId, VfsPath};

/// Bidirectional interner for VfsPath ↔ FileId mapping.
///
/// Internally uses an `IndexSet` which provides:
/// - Stable indices that can be used as FileIds
/// - Fast lookup by path (hash-based)
/// - Fast lookup by FileId (index-based)
/// - Each path stored only once (interned)
#[derive(Default, Debug)]
pub(crate) struct PathInterner {
    map: IndexSet<VfsPath, FxBuildHasher>,
}

impl PathInterner {
    /// Create a new empty path interner.
    #[allow(dead_code)]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Intern a path and return its FileId.
    ///
    /// If the path already exists, returns the existing FileId.
    /// Otherwise, allocates a new FileId and stores the path.
    pub(crate) fn intern(&mut self, path: VfsPath) -> FileId {
        let (idx, _inserted) = self.map.insert_full(path);
        FileId(idx as u32)
    }

    /// Get the FileId for a path if it exists.
    ///
    /// Returns `None` if the path has not been interned.
    pub(crate) fn get(&self, path: &VfsPath) -> Option<FileId> {
        self.map.get_index_of(path).map(|idx| FileId(idx as u32))
    }

    /// Look up a path by its FileId.
    ///
    /// # Panics
    ///
    /// Panics if the FileId is invalid (out of bounds).
    pub(crate) fn lookup(&self, id: FileId) -> &VfsPath {
        &self.map[id.0 as usize]
    }

    /// Returns the number of interned paths.
    #[allow(dead_code)]
    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns true if no paths have been interned.
    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_intern_new_path() {
        let mut interner = PathInterner::new();
        let path = VfsPath::from(PathBuf::from("/test.bsl"));

        let id = interner.intern(path.clone());
        assert_eq!(id, FileId(0));
        assert_eq!(interner.len(), 1);
    }

    #[test]
    fn test_intern_existing_path() {
        let mut interner = PathInterner::new();
        let path = VfsPath::from(PathBuf::from("/test.bsl"));

        let id1 = interner.intern(path.clone());
        let id2 = interner.intern(path.clone());

        assert_eq!(id1, id2);
        assert_eq!(interner.len(), 1); // Only one entry
    }

    #[test]
    fn test_get_existing_path() {
        let mut interner = PathInterner::new();
        let path = VfsPath::from(PathBuf::from("/test.bsl"));

        let id = interner.intern(path.clone());
        assert_eq!(interner.get(&path), Some(id));
    }

    #[test]
    fn test_get_nonexistent_path() {
        let interner = PathInterner::new();
        let path = VfsPath::from(PathBuf::from("/nonexistent.bsl"));

        assert_eq!(interner.get(&path), None);
    }

    #[test]
    fn test_lookup() {
        let mut interner = PathInterner::new();
        let path = VfsPath::from(PathBuf::from("/test.bsl"));

        let id = interner.intern(path.clone());
        let looked_up = interner.lookup(id);

        assert_eq!(looked_up, &path);
    }

    #[test]
    fn test_multiple_paths() {
        let mut interner = PathInterner::new();
        let path1 = VfsPath::from(PathBuf::from("/test1.bsl"));
        let path2 = VfsPath::from(PathBuf::from("/test2.bsl"));
        let path3 = VfsPath::from(PathBuf::from("/test3.bsl"));

        let id1 = interner.intern(path1.clone());
        let id2 = interner.intern(path2.clone());
        let id3 = interner.intern(path3.clone());

        assert_eq!(id1, FileId(0));
        assert_eq!(id2, FileId(1));
        assert_eq!(id3, FileId(2));

        assert_eq!(interner.lookup(id1), &path1);
        assert_eq!(interner.lookup(id2), &path2);
        assert_eq!(interner.lookup(id3), &path3);
    }

    #[test]
    #[should_panic]
    fn test_lookup_invalid_id() {
        let interner = PathInterner::new();
        let invalid_id = FileId(999);
        let _ = interner.lookup(invalid_id);
    }
}
