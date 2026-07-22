use std::sync::Arc;

use indexmap::IndexMap;
use rustc_hash::{FxBuildHasher, FxHashMap};

use crate::{FileId, VfsPath};

/// A bidirectional file-id ↔ path map. The two indices are held behind `Arc` so a
/// clone is O(1): the same map can be shared across many Salsa databases (e.g. one
/// per graph-build batch) without re-cloning every path. Mutation copies on write
/// via [`Arc::make_mut`], so building a fresh set stays cheap while clones stay shared.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct FileSet {
    files: Arc<FxHashMap<VfsPath, FileId>>,
    paths: Arc<IndexMap<FileId, VfsPath, FxBuildHasher>>,
}

impl FileSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn insert(&mut self, file_id: FileId, path: VfsPath) {
        Arc::make_mut(&mut self.files).insert(path.clone(), file_id);
        Arc::make_mut(&mut self.paths).insert(file_id, path);
    }

    pub fn file_for_path(&self, path: &VfsPath) -> Option<&FileId> {
        self.files.get(path)
    }

    pub fn path_for_file(&self, file: &FileId) -> Option<&VfsPath> {
        self.paths.get(file)
    }

    pub fn iter(&self) -> impl Iterator<Item = FileId> + '_ {
        self.paths.keys().copied()
    }

    pub fn remove(&mut self, file_id: FileId) -> Option<VfsPath> {
        let path = Arc::make_mut(&mut self.paths).shift_remove(&file_id)?;
        Arc::make_mut(&mut self.files).remove(&path);
        Some(path)
    }

    /// Approximate live heap bytes owned by this file set: both lookup tables
    /// (`files`, `paths`) plus each entry's own [`VfsPath`] buffer. Each path is
    /// stored — and counted — once per table, since `files` and `paths` hold
    /// independent `VfsPath` clones rather than sharing one allocation.
    ///
    /// The two `Arc`s may be shared across `SourceRoot` clones (see the struct
    /// doc comment above), so summing this per Salsa memo over-counts a `FileSet`
    /// shared by many inputs — an accepted over-count, not a bug, per the
    /// workspace `heap_size` convention (count owned payloads, ignore sharing).
    pub fn estimated_heap_size(&self) -> usize {
        let files_bytes = stdx::heap::map_table_bytes::<VfsPath, FileId>(self.files.len())
            + self.files.keys().map(VfsPath::estimated_heap_size).sum::<usize>();
        let paths_bytes = stdx::heap::map_table_bytes::<FileId, VfsPath>(self.paths.len())
            + self.paths.values().map(VfsPath::estimated_heap_size).sum::<usize>();
        files_bytes + paths_bytes
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

        set.insert(id1, path.clone());
        assert_eq!(set.file_for_path(&path), Some(&id1));

        set.insert(id2, path.clone());
        assert_eq!(set.file_for_path(&path), Some(&id2));
        assert_eq!(set.len(), 1);
        assert_eq!(set.iter().count(), 2);
    }

    #[test]
    fn estimated_heap_size_counts_tables_and_paths() {
        let mut set = FileSet::new();
        set.insert(
            FileId(0),
            VfsPath::from(PathBuf::from("/Catalogs/Товары/Ext/ObjectModule.bsl")),
        );
        set.insert(FileId(1), VfsPath::from(PathBuf::from("/CommonModules/Общий/Module.bsl")));
        set.insert(FileId(2), VfsPath::from(PathBuf::from("/test.bsl")));

        let path_bytes: usize =
            set.iter().map(|id| set.path_for_file(&id).unwrap().estimated_heap_size()).sum();
        let bytes = set.estimated_heap_size();
        // At least the three paths' own bytes counted in both tables; well under
        // a few kilobytes for three short paths.
        assert!(bytes > path_bytes * 2);
        assert!(bytes < 4096);
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
