//! File content provider for streaming analysis.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::{FxBuildHasher, FxHashMap};
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

    /// In-memory with per-file read tracking (for testing caching behavior).
    InMemoryWithTracking {
        /// Map from FileId to file content.
        files: Arc<FxHashMap<FileId, String>>,
        /// Per-file read counts.
        read_counts: Arc<DashMap<FileId, AtomicUsize, FxBuildHasher>>,
        /// Total read count.
        total_reads: Arc<AtomicUsize>,
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

    /// Create an in-memory file reader with read tracking (for testing caching).
    pub fn in_memory_with_tracking(files: FxHashMap<FileId, String>) -> Self {
        Self::InMemoryWithTracking {
            files: Arc::new(files),
            read_counts: Arc::new(DashMap::with_hasher(FxBuildHasher)),
            total_reads: Arc::new(AtomicUsize::new(0)),
        }
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
            FileReader::InMemoryWithTracking { files, read_counts, total_reads } => {
                // Increment total reads
                total_reads.fetch_add(1, Ordering::SeqCst);

                // Increment per-file read count
                read_counts
                    .entry(file_id)
                    .or_insert_with(|| AtomicUsize::new(0))
                    .fetch_add(1, Ordering::SeqCst);

                files.get(&file_id).cloned()
            }
        }
    }

    /// Get read count for a specific file (tracking mode only).
    ///
    /// Returns 0 for non-tracking modes.
    pub fn read_count_for(&self, file_id: FileId) -> usize {
        match self {
            FileReader::InMemoryWithTracking { read_counts, .. } => {
                read_counts.get(&file_id).map(|c| c.load(Ordering::SeqCst)).unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// Get total read count (tracking mode only).
    ///
    /// Returns 0 for non-tracking modes.
    pub fn total_read_count(&self) -> usize {
        match self {
            FileReader::InMemoryWithTracking { total_reads, .. } => {
                total_reads.load(Ordering::SeqCst)
            }
            _ => 0,
        }
    }

    /// Reset all read counts (tracking mode only).
    pub fn reset_counts(&self) {
        if let FileReader::InMemoryWithTracking { read_counts, total_reads, .. } = self {
            read_counts.clear();
            total_reads.store(0, Ordering::SeqCst);
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
