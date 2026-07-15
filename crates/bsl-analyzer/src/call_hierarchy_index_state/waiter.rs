use super::*;

impl CallHierarchyIndexState {
    pub fn waiter(
        &self,
        source_root: SourceRootId,
        generation: u64,
    ) -> Option<CallHierarchyIndexWaiter> {
        let mut state = self.inner.write();
        let root = state.roots.get_mut(&source_root)?;
        let Lifecycle::Building(building) = &mut root.lifecycle else {
            return None;
        };
        if building.generation != generation
            || building.structural_superseded
            || building.completion.is_some()
        {
            return None;
        }
        let (sender, receiver) = crossbeam_channel::bounded(1);
        building.completion = Some(sender);
        Some(CallHierarchyIndexWaiter { state: self.clone(), source_root, generation, receiver })
    }

    pub fn wait_or_ready(
        &self,
        source_root: SourceRootId,
        generation: u64,
    ) -> Option<CallHierarchyIndexWaitOrReady> {
        let mut state = self.inner.write();
        let root = state.roots.get_mut(&source_root)?;
        if root.generation != generation {
            return None;
        }
        match &mut root.lifecycle {
            Lifecycle::Ready(ready) => {
                Some(CallHierarchyIndexWaitOrReady::Ready(Arc::clone(&ready.index)))
            }
            Lifecycle::Building(building)
                if building.generation == generation
                    && !building.structural_superseded
                    && building.completion.is_none() =>
            {
                let (sender, receiver) = crossbeam_channel::bounded(1);
                building.completion = Some(sender);
                Some(CallHierarchyIndexWaitOrReady::Waiting(CallHierarchyIndexWaiter {
                    state: self.clone(),
                    source_root,
                    generation,
                    receiver,
                }))
            }
            Lifecycle::Idle | Lifecycle::Building(_) | Lifecycle::Failed(_) => None,
        }
    }

    pub(crate) fn release_waiter(&self, source_root: SourceRootId, generation: u64) {
        let mut state = self.inner.write();
        let Some(root) = state.roots.get_mut(&source_root) else {
            return;
        };
        let Lifecycle::Building(building) = &mut root.lifecycle else {
            return;
        };
        if building.generation == generation {
            building.completion = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn has_waiter(&self, source_root: SourceRootId, generation: u64) -> bool {
        let state = self.inner.read();
        matches!(state.roots.get(&source_root), Some(root) if matches!(&root.lifecycle, Lifecycle::Building(building) if building.generation == generation && building.completion.is_some()))
    }
}
