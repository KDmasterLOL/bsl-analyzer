use super::*;

impl CallHierarchyIndexState {
    pub fn frozen_snapshot(
        &self,
        source_root: SourceRootId,
        generation: u64,
    ) -> Option<CallHierarchyIndexSnapshotId> {
        let state = self.inner.read();
        let root = state.roots.get(&source_root)?;
        match &root.lifecycle {
            Lifecycle::Building(building) if building.generation == generation => {
                Some(building.frozen_snapshot)
            }
            Lifecycle::Idle
            | Lifecycle::Building(_)
            | Lifecycle::Ready(_)
            | Lifecycle::Failed(_) => None,
        }
    }

    pub fn failure_reason(&self, source_root: SourceRootId, generation: u64) -> Option<String> {
        let state = self.inner.read();
        let root = state.roots.get(&source_root)?;
        match &root.lifecycle {
            Lifecycle::Failed(failed) if failed.generation == generation => {
                Some(failed.reason.clone())
            }
            Lifecycle::Idle
            | Lifecycle::Building(_)
            | Lifecycle::Ready(_)
            | Lifecycle::Failed(_) => None,
        }
    }

    pub fn cancellation(
        &self,
        source_root: SourceRootId,
        generation: u64,
    ) -> Option<CallHierarchyIndexCancellation> {
        let state = self.inner.read();
        let root = state.roots.get(&source_root)?;
        match &root.lifecycle {
            Lifecycle::Building(building) if building.generation == generation => {
                Some(building.cancellation.clone())
            }
            Lifecycle::Idle
            | Lifecycle::Building(_)
            | Lifecycle::Ready(_)
            | Lifecycle::Failed(_) => None,
        }
    }
}
