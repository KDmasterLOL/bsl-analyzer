//! Virtual file system for bsl-analyzer.
//!
//! This crate provides an abstraction over the file system, allowing
//! the analyzer to work with files without direct filesystem access.
//!
//! The VFS tracks file changes using content hashing to detect when files
//! have actually changed, enabling efficient incremental computation with Salsa.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexMap;
use rustc_hash::{FxBuildHasher, FxHasher};

pub mod file_set;
pub mod loader;
mod path_interner;

pub use file_set::FileSet;
use path_interner::PathInterner;

/// Unique identifier for a file in the VFS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub u32);

impl FileId {
    /// Create a FileId from a raw u32 value.
    pub const fn from_raw(raw: u32) -> Self {
        FileId(raw)
    }

    /// Get the raw u32 value of this FileId.
    pub const fn index(self) -> u32 {
        self.0
    }
}

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

/// State of a file in the VFS.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileState {
    /// File exists with the given content hash
    Exists(u64),
    /// File was deleted
    Deleted,
    /// Path is registered for change-tracking, but contents are not stored.
    ///
    /// Used for metadata files (XML) where the consumer wants notifications
    /// when the path changes without paying the resident-memory cost of an
    /// `Arc<str>` copy. Transitioning to `Exists` (via `set_file_contents`
    /// with `Some`) is allowed and emits a `Create` change. Transitioning
    /// back to `Deleted` (via `set_file_contents` with `None`) is allowed
    /// and emits no change — no `Create` was ever observed by consumers.
    WatchOnly,
}

/// A file change detected by the VFS.
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

/// The type of change that occurred to a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// File was created with the given content and hash
    Create(Arc<str>, u64),
    /// File was modified with new content and hash
    Modify(Arc<str>, u64),
    /// File was deleted
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

/// Simplified change kind without associated data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Create,
    Modify,
    Delete,
}

/// The virtual file system.
///
/// Tracks files and their changes using content hashing to detect
/// when files have actually changed (not just been touched).
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

    /// Set the contents of a file.
    ///
    /// Returns `true` if the file was actually changed (content differs).
    /// Returns `false` if the content is the same (detected via hash).
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the file
    /// * `contents` - The new contents, or `None` to delete the file
    pub fn set_file_contents(&mut self, path: VfsPath, contents: Option<Arc<str>>) -> bool {
        let _span = tracing::debug_span!("Vfs::set_file_contents", ?path).entered();

        // Allocate or get existing FileId
        let file_id = self.alloc_file_id(path);

        // Get current state
        let state = self.get_state(file_id);

        // Determine what change occurred
        let change = match (state, contents) {
            // File exists, new content provided
            (FileState::Exists(old_hash), Some(text)) => {
                let new_hash = stdx::hash_once::<FxHasher>(&*text);
                if new_hash == old_hash {
                    // Content unchanged
                    return false;
                }
                self.data[file_id.0 as usize] = FileState::Exists(new_hash);
                Change::Modify(text, new_hash)
            }

            // File exists, being deleted
            (FileState::Exists(_), None) => {
                self.data[file_id.0 as usize] = FileState::Deleted;
                Change::Delete
            }

            // File was deleted (or registered as watch-only), now being created.
            // For `WatchOnly` -> `Exists`, no prior `Create` was emitted, so
            // the consumer sees this as the first observation of the file.
            (FileState::Deleted, Some(text)) | (FileState::WatchOnly, Some(text)) => {
                let new_hash = stdx::hash_once::<FxHasher>(&*text);
                self.data[file_id.0 as usize] = FileState::Exists(new_hash);
                Change::Create(text, new_hash)
            }

            // File was deleted, still deleted (no-op)
            (FileState::Deleted, None) => {
                return false;
            }

            // Watch-only registration dropped. Transition to `Deleted` but
            // emit no change: consumers never observed a `Create`, so
            // pairing it with a `Delete` would be a phantom event.
            (FileState::WatchOnly, None) => {
                self.data[file_id.0 as usize] = FileState::Deleted;
                return false;
            }
        };

        // Record the change (with merging logic)
        self.record_change(file_id, change);

        true
    }

    /// Allocate or get existing FileId for a path.
    pub fn alloc_file_id(&mut self, path: VfsPath) -> FileId {
        let file_id = self.interner.intern(path);

        // Extend data vector if needed
        while self.data.len() <= file_id.0 as usize {
            self.data.push(FileState::Deleted);
        }

        file_id
    }

    /// Register `path` for change-tracking without storing its contents.
    ///
    /// Allocates a [`FileId`] for the path and marks it [`FileState::WatchOnly`].
    /// This is the entry point used by the loader's
    /// [`Message::WatchOnly`](loader::Message::WatchOnly) dispatch (PR-3): the
    /// consumer receives the watch-only path, calls this to obtain a
    /// [`FileId`], and then drives a downstream loader (e.g. metadata XML
    /// reader) using the file id as the cache key.
    ///
    /// If the path is already in [`FileState::Exists`], the existing state
    /// is preserved (an active full-content registration trumps a
    /// watch-only re-registration). Calling this on an already-watch-only
    /// path is a no-op.
    pub fn register_watch_only(&mut self, path: VfsPath) -> FileId {
        let _span = tracing::debug_span!("Vfs::register_watch_only", ?path).entered();
        let file_id = self.alloc_file_id(path);
        if matches!(self.get_state(file_id), FileState::Deleted) {
            self.data[file_id.0 as usize] = FileState::WatchOnly;
        }
        file_id
    }

    /// Get the current state of a file.
    fn get_state(&self, file_id: FileId) -> FileState {
        self.data.get(file_id.0 as usize).copied().unwrap_or(FileState::Deleted)
    }

    /// Record a change, merging with existing changes if needed.
    fn record_change(&mut self, file_id: FileId, change: Change) {
        use indexmap::map::Entry;

        match self.changes.entry(file_id) {
            Entry::Occupied(mut entry) => {
                // Merge with existing change
                let existing = &entry.get().change;
                let merged = Self::merge_changes(existing, &change);
                entry.get_mut().change = merged;
            }
            Entry::Vacant(entry) => {
                entry.insert(ChangedFile { file_id, change });
            }
        }
    }

    /// Merge two changes to the same file.
    ///
    /// Implements smart merging rules:
    /// - Create + Modify = Create (with new content)
    /// - Modify + Modify = Modify (with latest content)
    /// - Delete + Create = Modify
    fn merge_changes(old: &Change, new: &Change) -> Change {
        match (old, new) {
            // Create followed by any change uses the new content
            (Change::Create(_, _), Change::Create(text, hash))
            | (Change::Create(_, _), Change::Modify(text, hash)) => {
                Change::Create(text.clone(), *hash)
            }

            // Create followed by delete cancels out to modify (or nothing)
            (Change::Create(_, _), Change::Delete) => Change::Delete,

            // Modify followed by modify keeps latest
            (Change::Modify(_, _), Change::Modify(text, hash)) => {
                Change::Modify(text.clone(), *hash)
            }

            // Modify followed by delete
            (Change::Modify(_, _), Change::Delete) => Change::Delete,

            // Delete followed by create is a modification
            (Change::Delete, Change::Create(text, hash)) => Change::Modify(text.clone(), *hash),

            // Other combinations shouldn't happen in normal operation
            _ => new.clone(),
        }
    }

    /// Take all accumulated changes since the last call.
    ///
    /// This drains the changes buffer and returns them as a Vec.
    /// Typically called by the main loop to feed changes to Salsa.
    pub fn take_changes(&mut self) -> Vec<ChangedFile> {
        let _span = tracing::debug_span!("Vfs::take_changes", count = self.changes.len()).entered();
        std::mem::take(&mut self.changes).into_iter().map(|(_, change)| change).collect()
    }

    /// Get the path for a file ID.
    pub fn file_path(&self, file_id: FileId) -> &VfsPath {
        self.interner.lookup(file_id)
    }

    /// Get the file ID for a path, if it exists.
    pub fn file_id(&self, path: &VfsPath) -> Option<FileId> {
        self.interner.get(path)
    }

    /// Check if a file exists in the VFS.
    ///
    /// Returns `true` for both [`FileState::Exists`] (contents stored) and
    /// [`FileState::WatchOnly`] (path registered for change-tracking, no
    /// contents stored): from a consumer's standpoint, both correspond to
    /// "the file is present and tracked". Use [`Self::file_content`] /
    /// downstream Salsa queries to distinguish the two when content is
    /// actually needed.
    pub fn exists(&self, file_id: FileId) -> bool {
        matches!(self.get_state(file_id), FileState::Exists(_) | FileState::WatchOnly)
    }

    /// Returns the number of allocated file IDs.
    pub fn num_file_ids(&self) -> u32 {
        self.data.len() as u32
    }

    /// Get the content of a file (for compatibility).
    ///
    /// Note: This doesn't return the actual stored content, as we don't
    /// store it in the VFS (only track changes). Use the database queries
    /// to get file contents.
    pub fn file_content(&self, _file_id: FileId) -> Option<Arc<str>> {
        // In the refactored VFS, we don't store file contents directly.
        // Content is managed by Salsa database queries.
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

        // Create file
        vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("v1")));
        vfs.take_changes(); // Clear changes

        // Modify file
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

        // Create file
        vfs.set_file_contents(path.clone(), Some(content.clone()));
        vfs.take_changes();

        // Set same content (should detect no change via hash)
        let changed = vfs.set_file_contents(path.clone(), Some(content.clone()));
        assert!(!changed);

        let changes = vfs.take_changes();
        assert_eq!(changes.len(), 0);
    }

    #[test]
    fn test_vfs_delete_file() {
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/test.bsl");

        // Create file
        vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("content")));
        vfs.take_changes();

        // Delete file
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

        // Create then modify in same cycle
        vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("v1")));
        vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("v2")));

        let changes = vfs.take_changes();
        assert_eq!(changes.len(), 1);
        // Should be Create with final content
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

        // Delete file
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

        // Watch-only registration emits no Change — consumers track these
        // via the loader's `Message::WatchOnly` stream, not the VFS change
        // pipeline.
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
        // A watch-only path being upgraded to full-content (e.g. user opens
        // an XML file in the editor and the consumer decides to load it
        // for diagnostics) should look like a fresh `Create` to consumers,
        // since they never saw a prior change for this path.
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
        // If a path already has content (`Exists`), a subsequent
        // `register_watch_only` must not downgrade it — full-content state
        // wins.
        let mut vfs = Vfs::new();
        let path = VfsPath::new("/Form.xml");

        vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("<form/>")));
        let file_id = vfs.file_id(&path).unwrap();
        vfs.take_changes();

        let same_id = vfs.register_watch_only(path.clone());
        assert_eq!(file_id, same_id);
        // No new change recorded; the file is still in `Exists`.
        assert!(vfs.take_changes().is_empty());
        // Modifying it still works as `Modify`, proving state is `Exists`,
        // not `WatchOnly`.
        let changed =
            vfs.set_file_contents(path.clone(), Some(Arc::<str>::from("<form><x/></form>")));
        assert!(changed);
        let changes = vfs.take_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind(), ChangeKind::Modify);
    }

    #[test]
    fn test_watch_only_drop_emits_no_change() {
        // Calling `set_file_contents(path, None)` on a watch-only path
        // (consumer dropping the registration) transitions to `Deleted`
        // but emits no change — consumers never observed a `Create` for
        // it, so a `Delete` would be a phantom event.
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
