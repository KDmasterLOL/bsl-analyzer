use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::{FxBuildHasher, FxHashMap};
use vfs::{file_set::FileSet, FileId, VfsPath};

#[derive(Debug, Clone)]
pub enum FileReader {
    Disk {
        workspace_root: Arc<Path>,
        file_set: Arc<FileSet>,
    },

    InMemory {
        files: Arc<FxHashMap<FileId, String>>,
    },

    InMemoryWithTracking {
        files: Arc<FxHashMap<FileId, String>>,
        read_counts: Arc<DashMap<FileId, AtomicUsize, FxBuildHasher>>,
        total_reads: Arc<AtomicUsize>,
    },
}

impl FileReader {
    pub fn from_disk(workspace_root: impl AsRef<Path>, file_set: Arc<FileSet>) -> Self {
        Self::Disk { workspace_root: Arc::from(workspace_root.as_ref()), file_set }
    }

    pub fn in_memory(files: FxHashMap<FileId, String>) -> Self {
        Self::InMemory { files: Arc::new(files) }
    }

    pub fn in_memory_with_tracking(files: FxHashMap<FileId, String>) -> Self {
        Self::InMemoryWithTracking {
            files: Arc::new(files),
            read_counts: Arc::new(DashMap::with_hasher(FxBuildHasher)),
            total_reads: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn empty() -> Self {
        Self::InMemory { files: Arc::new(FxHashMap::default()) }
    }

    pub fn read(&self, file_id: FileId) -> Option<String> {
        match self {
            FileReader::Disk { workspace_root, file_set } => {
                let vfs_path = file_set.path_for_file(&file_id)?;
                let path = resolve_vfs_path(workspace_root, vfs_path)?;
                std::fs::read_to_string(&path).ok()
            }
            FileReader::InMemory { files } => files.get(&file_id).cloned(),
            FileReader::InMemoryWithTracking { files, read_counts, total_reads } => {
                total_reads.fetch_add(1, Ordering::SeqCst);

                read_counts
                    .entry(file_id)
                    .or_insert_with(|| AtomicUsize::new(0))
                    .fetch_add(1, Ordering::SeqCst);

                files.get(&file_id).cloned()
            }
        }
    }

    pub fn read_count_for(&self, file_id: FileId) -> usize {
        match self {
            FileReader::InMemoryWithTracking { read_counts, .. } => {
                read_counts.get(&file_id).map(|c| c.load(Ordering::SeqCst)).unwrap_or(0)
            }
            _ => 0,
        }
    }

    pub fn total_read_count(&self) -> usize {
        match self {
            FileReader::InMemoryWithTracking { total_reads, .. } => {
                total_reads.load(Ordering::SeqCst)
            }
            _ => 0,
        }
    }

    pub fn reset_counts(&self) {
        if let FileReader::InMemoryWithTracking { read_counts, total_reads, .. } = self {
            read_counts.clear();
            total_reads.store(0, Ordering::SeqCst);
        }
    }
}

fn resolve_vfs_path(workspace_root: &Path, vfs_path: &VfsPath) -> Option<std::path::PathBuf> {
    let path = vfs_path.as_path();
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        Some(workspace_root.join(path))
    }
}
