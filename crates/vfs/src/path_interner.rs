use indexmap::IndexSet;
use rustc_hash::FxBuildHasher;

use crate::{FileId, VfsPath};

#[derive(Default, Debug)]
pub(crate) struct PathInterner {
    map: IndexSet<VfsPath, FxBuildHasher>,
}

impl PathInterner {
    pub(crate) fn intern(&mut self, path: VfsPath) -> FileId {
        let (idx, _inserted) = self.map.insert_full(path);
        FileId(idx as u32)
    }

    pub(crate) fn get(&self, path: &VfsPath) -> Option<FileId> {
        self.map.get_index_of(path).map(|idx| FileId(idx as u32))
    }

    pub(crate) fn lookup(&self, id: FileId) -> &VfsPath {
        &self.map[id.0 as usize]
    }

    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.map.len()
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
        assert_eq!(interner.len(), 1);
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
