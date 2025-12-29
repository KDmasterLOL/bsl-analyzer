//! Input types for the database - SourceRoot and related structures.
//!
//! A SourceRoot represents a logical grouping of files, typically corresponding
//! to a directory on the filesystem (like a project or a library).

use vfs::file_set::FileSet;
use vfs::FileId;

/// Unique identifier for a source root.
///
/// Source roots partition the set of all files into logical groups.
/// Each file belongs to exactly one source root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceRootId(pub u32);

/// A source root is a directory on the filesystem that contains related files.
///
/// Source roots are typically used to distinguish between:
/// - Local code (your project)
/// - Library code (external dependencies)
///
/// This distinction is important for setting appropriate Salsa durability levels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRoot {
    /// Whether this is a library (external dependencies vs local code).
    ///
    /// Library code is assumed to change rarely (HIGH durability in Salsa),
    /// while local code changes frequently (LOW durability).
    pub is_library: bool,

    /// The set of files in this source root.
    file_set: FileSet,
}

impl SourceRoot {
    /// Create a new source root for local (project) code.
    pub fn new_local(file_set: FileSet) -> Self {
        SourceRoot { is_library: false, file_set }
    }

    /// Create a new source root for library (external) code.
    pub fn new_library(file_set: FileSet) -> Self {
        SourceRoot { is_library: true, file_set }
    }

    /// Get the file set for this source root.
    pub fn file_set(&self) -> &FileSet {
        &self.file_set
    }

    /// Iterate over all file IDs in this source root.
    pub fn iter(&self) -> impl Iterator<Item = FileId> + '_ {
        self.file_set.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use vfs::VfsPath;

    #[test]
    fn test_source_root_local() {
        let mut file_set = FileSet::new();
        file_set.insert(FileId(0), VfsPath::from(PathBuf::from("/test.bsl")));

        let root = SourceRoot::new_local(file_set);
        assert!(!root.is_library);
        assert_eq!(root.file_set().len(), 1);
    }

    #[test]
    fn test_source_root_library() {
        let mut file_set = FileSet::new();
        file_set.insert(FileId(0), VfsPath::from(PathBuf::from("/lib.bsl")));

        let root = SourceRoot::new_library(file_set);
        assert!(root.is_library);
        assert_eq!(root.file_set().len(), 1);
    }

    #[test]
    fn test_source_root_iter() {
        let mut file_set = FileSet::new();
        let id1 = FileId(0);
        let id2 = FileId(1);
        file_set.insert(id1, VfsPath::from(PathBuf::from("/test1.bsl")));
        file_set.insert(id2, VfsPath::from(PathBuf::from("/test2.bsl")));

        let root = SourceRoot::new_local(file_set);
        let ids: Vec<_> = root.iter().collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_source_root_id_eq() {
        let id1 = SourceRootId(0);
        let id2 = SourceRootId(0);
        let id3 = SourceRootId(1);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }
}
