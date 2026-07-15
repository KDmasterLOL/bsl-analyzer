use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use base_db::SourceRootId;
use crossbeam_channel::Sender;
use hir::CallHierarchyReverseIndex;
use parking_lot::RwLock;
use rustc_hash::FxHashSet;
use vfs::FileId;

/// Identity of the frozen overlay-aware source snapshot used by one build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallHierarchyIndexSnapshotId(pub u64);

/// Body edits collected while a build keeps working on its frozen snapshot.
#[derive(Debug, Clone, Default)]
pub struct CallHierarchyIndexJournal {
    edited_files: Arc<FxHashSet<FileId>>,
}

impl CallHierarchyIndexJournal {
    fn record(&mut self, file_id: FileId) {
        Arc::make_mut(&mut self.edited_files).insert(file_id);
    }

    fn files(&self) -> Vec<FileId> {
        let mut files: Vec<_> = self.edited_files.iter().copied().collect();
        files.sort_unstable();
        files
    }

    fn is_empty(&self) -> bool {
        self.edited_files.is_empty()
    }
}

/// Cancellation shared with a build worker, reserved for shutdown and supersession.
#[derive(Debug, Clone, Default)]
pub struct CallHierarchyIndexCancellation(Arc<AtomicBool>);

impl CallHierarchyIndexCancellation {
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Terminal notification for the sole latency worker permitted to wait for a build.
#[derive(Debug)]
pub enum CallHierarchyIndexCompletion {
    Ready(Arc<CallHierarchyReverseIndex>),
    Failed(String),
    Superseded,
    Shutdown,
}

#[derive(Debug)]
pub enum CallHierarchyIndexWaitOrReady {
    Waiting(CallHierarchyIndexWaiter),
    Ready(Arc<CallHierarchyReverseIndex>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallHierarchyIndexPrepareAction {
    UseReady,
    UseExisting,
    StartBuild,
}

/// Sole claim on a build-completion notification for one request.
#[derive(Debug)]
pub struct CallHierarchyIndexWaiter {
    state: CallHierarchyIndexState,
    source_root: SourceRootId,
    generation: u64,
    receiver: crossbeam_channel::Receiver<CallHierarchyIndexCompletion>,
}

impl CallHierarchyIndexWaiter {
    pub fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<CallHierarchyIndexCompletion, crossbeam_channel::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}

impl Drop for CallHierarchyIndexWaiter {
    fn drop(&mut self) {
        self.state.release_waiter(self.source_root, self.generation);
    }
}

/// Cloneable, non-Salsa resident lifecycle for compact reverse call indexes.
#[derive(Debug, Clone, Default)]
pub struct CallHierarchyIndexState {
    inner: Arc<RwLock<IndexState>>,
}

#[derive(Debug, Default)]
struct IndexState {
    shutdown: bool,
    roots: HashMap<SourceRootId, RootState>,
}

#[derive(Debug, Default)]
struct RootState {
    generation: u64,
    prepared_generation: Option<u64>,
    lifecycle: Lifecycle,
}

#[derive(Debug, Default)]
enum Lifecycle {
    #[default]
    Idle,
    Building(Building),
    Ready(Ready),
    Failed(Failed),
}

#[derive(Debug)]
struct Building {
    generation: u64,
    frozen_snapshot: CallHierarchyIndexSnapshotId,
    journal: CallHierarchyIndexJournal,
    structural_superseded: bool,
    completion: Option<Sender<CallHierarchyIndexCompletion>>,
    cancellation: CallHierarchyIndexCancellation,
    previous: Option<Arc<CallHierarchyReverseIndex>>,
}

#[derive(Debug)]
struct Ready {
    index: Arc<CallHierarchyReverseIndex>,
}

#[derive(Debug)]
struct Failed {
    generation: u64,
    reason: String,
    previous: Option<Arc<CallHierarchyReverseIndex>>,
}

impl RootState {
    fn current(&self) -> Option<Arc<CallHierarchyReverseIndex>> {
        match &self.lifecycle {
            Lifecycle::Idle => None,
            Lifecycle::Building(building) if building.structural_superseded => None,
            Lifecycle::Building(building) => building.previous.clone(),
            Lifecycle::Ready(ready) => Some(Arc::clone(&ready.index)),
            Lifecycle::Failed(failed) if failed.generation == self.generation => {
                failed.previous.clone()
            }
            Lifecycle::Failed(_) => None,
        }
    }
}

mod journal;
mod lifecycle;
mod readiness;
mod snapshot;
mod status;
mod waiter;

#[cfg(test)]
#[path = "call_hierarchy_index_state_tests.rs"]
mod tests;
