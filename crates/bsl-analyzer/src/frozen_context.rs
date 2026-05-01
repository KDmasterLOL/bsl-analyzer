//! Immutable request context for background LSP handlers.
//!
//! Async read-handlers run on a task-pool thread, while the main loop
//! continues to apply `didChange` edits and mutate VFS. This module provides
//! frozen views of `MemDocs` and VFS paths so a worker's source view stays
//! consistent with the Salsa snapshot captured at dispatch time.

use std::path::PathBuf;
use std::sync::Arc;

use base_db::DiagnosticsConfigInput;
use crossbeam_channel::Sender;
use ide::Analysis;
use lsp_types::Url;
use project_model::Project;
use rustc_hash::FxHashMap;
use vfs::{FileId, Vfs};

use crate::global_state::Task;
use crate::mem_docs::FrozenMemDocs;

/// Frozen bidirectional mapping of FileId <-> PathBuf captured at dispatch time.
///
/// Built via `FrozenFilePaths::freeze(&Vfs)` on the main thread. Cheap to
/// clone (two `Arc`s). Matches the lookups handlers need via the synchronous
/// `GlobalStateSnapshot::file_id_for_url` / `url_for_file_id` pair.
#[derive(Debug, Clone, Default)]
pub struct FrozenFilePaths {
    forward: Arc<FxHashMap<PathBuf, FileId>>,
    reverse: Arc<FxHashMap<FileId, PathBuf>>,
}

impl FrozenFilePaths {
    /// Build a frozen snapshot of all live VFS paths.
    ///
    /// Iterates every allocated FileId and filters to ones still present.
    /// Cost is O(num_file_ids); typical workspace is tens of thousands.
    pub fn freeze(vfs: &Vfs) -> Self {
        let n = vfs.num_file_ids();
        let mut forward: FxHashMap<PathBuf, FileId> = FxHashMap::default();
        let mut reverse: FxHashMap<FileId, PathBuf> = FxHashMap::default();
        for i in 0..n {
            let file_id = FileId(i);
            if !vfs.exists(file_id) {
                continue;
            }
            let path = vfs.file_path(file_id).as_path().to_path_buf();
            forward.insert(path.clone(), file_id);
            reverse.insert(file_id, path);
        }
        Self { forward: Arc::new(forward), reverse: Arc::new(reverse) }
    }

    pub fn file_id_for_url(&self, url: &Url) -> anyhow::Result<FileId> {
        let path = url.to_file_path().map_err(|_| anyhow::anyhow!("Invalid file URL: {}", url))?;
        // Reject non-BSL files at the URL boundary. `process_changes` skips
        // `set_file_text` for them, so any downstream `db.parse(file_id)`
        // would panic. Returning `Err` here turns the LSP request into a
        // clean error response instead of a server crash.
        if !project_model::is_bsl_source_path(&path) {
            return Err(anyhow::anyhow!("File is not BSL, request unsupported: {}", url));
        }
        self.forward
            .get(&path)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("File not in frozen VFS snapshot: {}", url))
    }

    pub fn url_for_file_id(&self, file_id: FileId) -> anyhow::Result<Url> {
        let path = self
            .reverse
            .get(&file_id)
            .ok_or_else(|| anyhow::anyhow!("FileId {} not in frozen VFS snapshot", file_id.0))?;
        Url::from_file_path(path)
            .map_err(|_| anyhow::anyhow!("Failed to convert path to URL: {:?}", path))
    }

    pub fn len(&self) -> usize {
        self.reverse.len()
    }

    pub fn is_empty(&self) -> bool {
        self.reverse.is_empty()
    }
}

/// Immutable input to a background read-request handler.
///
/// Created on the main thread via `RequestDispatcher::on_latency`, shipped
/// by value into a task-pool worker. Any state reachable through this
/// context is either owned or `Arc`-shared read-only — no worker ever
/// touches `&mut GlobalState` or live VFS/MemDocs locks.
pub struct LatencyRequestContext {
    /// Owned Salsa snapshot. Send because `Analysis` owns `RootDatabaseImpl`.
    pub analysis: Analysis,
    pub workspace_root: Option<PathBuf>,
    pub project: Option<Project>,
    pub diagnostics_config: DiagnosticsConfigInput,
    pub vfs_done: bool,
    /// Lets handlers queue follow-up work (e.g., external file preload).
    pub task_sender: Sender<Task>,
    /// Frozen in-memory documents at dispatch time.
    pub mem_docs: FrozenMemDocs,
    /// Frozen VFS path mapping at dispatch time.
    pub file_paths: FrozenFilePaths,
}

impl LatencyRequestContext {
    pub fn file_id_for_url(&self, url: &Url) -> anyhow::Result<FileId> {
        self.file_paths.file_id_for_url(url)
    }

    pub fn url_for_file_id(&self, file_id: FileId) -> anyhow::Result<Url> {
        self.file_paths.url_for_file_id(file_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vfs::VfsPath;

    #[test]
    fn frozen_file_paths_roundtrip() {
        let mut vfs = Vfs::new();
        let path = PathBuf::from("/tmp/test.bsl");
        vfs.set_file_contents(VfsPath::new(path.clone()), Some(Arc::from("content")));
        let file_id = vfs.file_id(&VfsPath::new(path.clone())).unwrap();

        let frozen = FrozenFilePaths::freeze(&vfs);

        let url = Url::from_file_path(&path).unwrap();
        assert_eq!(frozen.file_id_for_url(&url).unwrap(), file_id);
        assert_eq!(frozen.url_for_file_id(file_id).unwrap(), url);
        assert_eq!(frozen.len(), 1);
    }

    #[test]
    fn frozen_file_paths_survive_later_mutation() {
        let mut vfs = Vfs::new();
        let path_a = PathBuf::from("/tmp/a.bsl");
        vfs.set_file_contents(VfsPath::new(path_a.clone()), Some(Arc::from("a")));
        let frozen = FrozenFilePaths::freeze(&vfs);

        let path_b = PathBuf::from("/tmp/b.bsl");
        vfs.set_file_contents(VfsPath::new(path_b.clone()), Some(Arc::from("b")));

        let url_b = Url::from_file_path(&path_b).unwrap();
        assert!(
            frozen.file_id_for_url(&url_b).is_err(),
            "frozen snapshot must not see later-added files"
        );
        assert_eq!(frozen.len(), 1);
    }

    #[test]
    fn frozen_file_paths_excludes_deleted() {
        let mut vfs = Vfs::new();
        let path = PathBuf::from("/tmp/gone.bsl");
        vfs.set_file_contents(VfsPath::new(path.clone()), Some(Arc::from("x")));
        vfs.set_file_contents(VfsPath::new(path.clone()), None);

        let frozen = FrozenFilePaths::freeze(&vfs);
        assert!(frozen.is_empty());
    }
}
