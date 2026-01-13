//! File content provider for streaming analysis.

use std::path::Path;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use vfs::{file_set::FileSet, FileId, VfsPath};

/// File content provider for streaming mode.
///
/// Can read files from disk or from a pre-loaded map.
#[derive(Debug, Clone)]
pub enum FileReader {
    /// Read files from disk using workspace root.
    Disk {
        /// Workspace root directory.
        workspace_root: Arc<Path>,
        /// FileSet for FileId → path resolution.
        file_set: Arc<FileSet>,
    },

    /// Use pre-loaded file contents (for testing).
    InMemory {
        /// Map from FileId to file content.
        files: Arc<FxHashMap<FileId, String>>,
    },
}

impl FileReader {
    /// Create a disk-based file reader.
    pub fn from_disk(workspace_root: impl AsRef<Path>, file_set: Arc<FileSet>) -> Self {
        Self::Disk { workspace_root: Arc::from(workspace_root.as_ref()), file_set }
    }

    /// Create an in-memory file reader (for testing).
    pub fn in_memory(files: FxHashMap<FileId, String>) -> Self {
        Self::InMemory { files: Arc::new(files) }
    }

    /// Create an empty file reader.
    pub fn empty() -> Self {
        Self::InMemory { files: Arc::new(FxHashMap::default()) }
    }

    /// Read file content.
    pub fn read(&self, file_id: FileId) -> Option<String> {
        match self {
            FileReader::Disk { workspace_root, file_set } => {
                let vfs_path = file_set.path_for_file(&file_id)?;
                let path = resolve_vfs_path(workspace_root, vfs_path)?;
                std::fs::read_to_string(&path).ok()
            }
            FileReader::InMemory { files } => files.get(&file_id).cloned(),
        }
    }
}

/// Resolve VfsPath to absolute filesystem path.
fn resolve_vfs_path(workspace_root: &Path, vfs_path: &VfsPath) -> Option<std::path::PathBuf> {
    let path = vfs_path.as_path();
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        // Relative to workspace root
        Some(workspace_root.join(path))
    }
}
