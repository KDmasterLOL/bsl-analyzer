use std::path::{Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexMap;
use rustc_hash::{FxBuildHasher, FxHasher};

pub mod file_set;
pub mod loader;
mod path_interner;

pub use file_set::FileSet;
use path_interner::PathInterner;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub u32);

impl FileId {
    pub const fn from_raw(raw: u32) -> Self {
        FileId(raw)
    }

    pub const fn index(self) -> u32 {
        self.0
    }
}

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

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileState {
    Exists(u64),
    Deleted,
    WatchOnly,
}

#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub file_id: FileId,
    pub change: Change,
}

impl ChangedFile {
    pub fn kind(&self) -> ChangeKind {
        self.change.kind()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Create(Arc<str>, u64),
    Modify(Arc<str>, u64),
    Delete,
}

impl Change {
    pub fn kind(&self) -> ChangeKind {
        match self {
            Change::Create(_, _) => ChangeKind::Create,
            Change::Modify(_, _) => ChangeKind::Modify,
            Change::Delete => ChangeKind::Delete,
        }
    }

    pub fn content(&self) -> Option<&Arc<str>> {
        match self {
            Change::Create(text, _) | Change::Modify(text, _) => Some(text),
            Change::Delete => None,
        }
    }

    pub fn hash(&self) -> Option<u64> {
        match self {
            Change::Create(_, hash) | Change::Modify(_, hash) => Some(*hash),
            Change::Delete => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Create,
    Modify,
    Delete,
}

#[derive(Debug, Default)]
pub struct Vfs {
    interner: PathInterner,
    data: Vec<FileState>,
    changes: IndexMap<FileId, ChangedFile, FxBuildHasher>,
}

impl Vfs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_file_contents(&mut self, path: VfsPath, contents: Option<Arc<str>>) -> bool {
        let _span = tracing::debug_span!("Vfs::set_file_contents", ?path).entered();

        let file_id = self.alloc_file_id(path);
        let state = self.get_state(file_id);

        let change = match (state, contents) {
            (FileState::Exists(old_hash), Some(text)) => {
                let new_hash = stdx::hash_once::<FxHasher>(&*text);
                if new_hash == old_hash {
                    return false;
                }
                self.data[file_id.0 as usize] = FileState::Exists(new_hash);
                Change::Modify(text, new_hash)
            }

            (FileState::Exists(_), None) => {
                self.data[file_id.0 as usize] = FileState::Deleted;
                Change::Delete
            }

            (FileState::Deleted, Some(text)) | (FileState::WatchOnly, Some(text)) => {
                let new_hash = stdx::hash_once::<FxHasher>(&*text);
                self.data[file_id.0 as usize] = FileState::Exists(new_hash);
                Change::Create(text, new_hash)
            }

            (FileState::Deleted, None) => {
                return false;
            }

            (FileState::WatchOnly, None) => {
                self.data[file_id.0 as usize] = FileState::Deleted;
                return false;
            }
        };

        self.record_change(file_id, change);

        true
    }

    pub fn alloc_file_id(&mut self, path: VfsPath) -> FileId {
        let file_id = self.interner.intern(path);

        while self.data.len() <= file_id.0 as usize {
            self.data.push(FileState::Deleted);
        }

        file_id
    }

    pub fn register_watch_only(&mut self, path: VfsPath) -> FileId {
        let _span = tracing::debug_span!("Vfs::register_watch_only", ?path).entered();
        let file_id = self.alloc_file_id(path);
        if matches!(self.get_state(file_id), FileState::Deleted) {
            self.data[file_id.0 as usize] = FileState::WatchOnly;
        }
        file_id
    }

    fn get_state(&self, file_id: FileId) -> FileState {
        self.data.get(file_id.0 as usize).copied().unwrap_or(FileState::Deleted)
    }

    fn record_change(&mut self, file_id: FileId, change: Change) {
        use indexmap::map::Entry;

        match self.changes.entry(file_id) {
            Entry::Occupied(mut entry) => {
                let existing = &entry.get().change;
                let merged = Self::merge_changes(existing, &change);
                entry.get_mut().change = merged;
            }
            Entry::Vacant(entry) => {
                entry.insert(ChangedFile { file_id, change });
            }
        }
    }

    fn merge_changes(old: &Change, new: &Change) -> Change {
        match (old, new) {
            (Change::Create(_, _), Change::Create(text, hash))
            | (Change::Create(_, _), Change::Modify(text, hash)) => {
                Change::Create(text.clone(), *hash)
            }

            (Change::Create(_, _), Change::Delete) => Change::Delete,
            (Change::Modify(_, _), Change::Modify(text, hash)) => {
                Change::Modify(text.clone(), *hash)
            }

            (Change::Modify(_, _), Change::Delete) => Change::Delete,
            (Change::Delete, Change::Create(text, hash)) => Change::Modify(text.clone(), *hash),
            _ => new.clone(),
        }
    }

    pub fn take_changes(&mut self) -> Vec<ChangedFile> {
        let _span = tracing::debug_span!("Vfs::take_changes", count = self.changes.len()).entered();
        std::mem::take(&mut self.changes).into_iter().map(|(_, change)| change).collect()
    }

    /// Number of pending changes and the total bytes of source text they hold
    /// (the live `Arc<str>` payloads of `Create`/`Modify`; deletions count zero
    /// bytes). Read-only instrumentation: sampled per loaded batch during the
    /// initial load to track the buffered-text high-water before each batch is
    /// drained into Salsa.
    pub fn pending_change_bytes(&self) -> (usize, usize) {
        let bytes = self
            .changes
            .values()
            .map(|c| match &c.change {
                Change::Create(text, _) | Change::Modify(text, _) => text.len(),
                Change::Delete => 0,
            })
            .sum();
        (self.changes.len(), bytes)
    }

    pub fn file_path(&self, file_id: FileId) -> &VfsPath {
        self.interner.lookup(file_id)
    }

    pub fn file_id(&self, path: &VfsPath) -> Option<FileId> {
        self.interner.get(path)
    }

    pub fn exists(&self, file_id: FileId) -> bool {
        matches!(self.get_state(file_id), FileState::Exists(_) | FileState::WatchOnly)
    }

    pub fn num_file_ids(&self) -> u32 {
        self.data.len() as u32
    }

    pub fn file_content(&self, _file_id: FileId) -> Option<Arc<str>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vfs_create_file() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/test.bsl");
        let content: Arc<str> = Arc::from("Процедура Тест() КонецПроцедуры");

        let changed = vfs.set_file_contents(path.clone(), Some(content.clone()));
        assert!(changed);

        let changes = vfs.take_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind(), ChangeKind::Create);
        assert_eq!(changes[0].change.content(), Some(&content));
    }

    #[test]
    fn test_vfs_modify_file() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/test.bsl");

        vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("v1")));
        vfs.take_changes();

        let new_content: Arc<str> = Arc::from("v2");
        let changed = vfs.set_file_contents(path.clone(), Some(new_content.clone()));
        assert!(changed);

        let changes = vfs.take_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind(), ChangeKind::Modify);
        assert_eq!(changes[0].change.content(), Some(&new_content));
    }

    #[test]
    fn test_vfs_no_change_same_content() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/test.bsl");
        let content: Arc<str> = Arc::from("Процедура Тест() КонецПроцедуры");

        vfs.set_file_contents(path.clone(), Some(content.clone()));
        vfs.take_changes();

        let changed = vfs.set_file_contents(path.clone(), Some(content.clone()));
        assert!(!changed);

        let changes = vfs.take_changes();
        assert_eq!(changes.len(), 0);
    }

    #[test]
    fn test_vfs_delete_file() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/test.bsl");

        vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("content")));
        vfs.take_changes();

        let changed = vfs.set_file_contents(path.clone(), None);
        assert!(changed);

        let changes = vfs.take_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind(), ChangeKind::Delete);
    }

    #[test]
    fn test_vfs_change_merging() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/test.bsl");

        vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("v1")));
        vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("v2")));

        let changes = vfs.take_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind(), ChangeKind::Create);
        assert_eq!(&**changes[0].change.content().unwrap(), "v2");
    }

    #[test]
    fn test_vfs_file_id_lookup() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/test.bsl");

        vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("content")));

        let file_id = vfs.file_id(&path).expect("File should exist");
        assert_eq!(vfs.file_path(file_id), &path);
    }

    #[test]
    fn test_vfs_exists() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/test.bsl");

        vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("content")));
        let file_id = vfs.file_id(&path).unwrap();

        assert!(vfs.exists(file_id));

        vfs.set_file_contents(path.clone(), None);
        assert!(!vfs.exists(file_id));
    }

    #[test]
    fn test_register_watch_only_new_path() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/Form.xml");

        let file_id = vfs.register_watch_only(path.clone());
        assert!(vfs.exists(file_id), "watch-only path should report as existing");
        assert_eq!(vfs.file_id(&path), Some(file_id));

        assert!(vfs.take_changes().is_empty(), "register_watch_only must not record a Change");
    }

    #[test]
    fn test_register_watch_only_idempotent() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/Form.xml");

        let id1 = vfs.register_watch_only(path.clone());
        let id2 = vfs.register_watch_only(path.clone());
        assert_eq!(id1, id2);
        assert!(vfs.take_changes().is_empty());
    }

    #[test]
    fn test_register_watch_only_then_set_contents_emits_create() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/Form.xml");

        vfs.register_watch_only(path.clone());
        let changed = vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("<form/>")));
        assert!(changed);

        let changes = vfs.take_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind(), ChangeKind::Create);
    }

    #[test]
    fn test_register_watch_only_preserves_existing_contents() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/Form.xml");

        vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("<form/>")));
        let file_id = vfs.file_id(&path).unwrap();
        vfs.take_changes();

        let same_id = vfs.register_watch_only(path.clone());
        assert_eq!(file_id, same_id);
        assert!(vfs.take_changes().is_empty());
        let changed =
            vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("<form><x/></form>")));
        assert!(changed);
        let changes = vfs.take_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind(), ChangeKind::Modify);
    }

    #[test]
    fn test_watch_only_drop_emits_no_change() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/Form.xml");

        vfs.register_watch_only(path.clone());
        let changed = vfs.set_file_contents(path.clone(), None);
        assert!(!changed, "dropping a watch-only registration must not be reported as a change");
        assert!(vfs.take_changes().is_empty());

        let file_id = vfs.file_id(&path).unwrap();
        assert!(!vfs.exists(file_id), "after drop, the file should no longer be tracked");
    }

    #[test]
    fn test_multiple_files() {
        let mut vfs = Vfs::new();
        let path1 = VfsPath::new("/test1.bsl");
        let path2 = VfsPath::new("/test2.bsl");

        vfs.set_file_contents(path1.clone(), Some(Arc::<str>::from("content1")));
        vfs.set_file_contents(path2.clone(), Some(Arc::<str>::from("content2")));

        let changes = vfs.take_changes();
        assert_eq!(changes.len(), 2);
    }
}
