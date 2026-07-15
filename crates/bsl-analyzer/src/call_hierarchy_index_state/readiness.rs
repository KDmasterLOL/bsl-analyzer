use super::*;

impl CallHierarchyIndexState {
    pub fn prepare_authorization(
        &self,
        source_root: SourceRootId,
    ) -> Option<(u64, CallHierarchyIndexPrepareAction)> {
        let mut state = self.inner.write();
        if state.shutdown {
            return None;
        }
        let root = state.roots.entry(source_root).or_default();
        let (generation, action) = match &root.lifecycle {
            Lifecycle::Ready(_) => (root.generation, CallHierarchyIndexPrepareAction::UseReady),
            Lifecycle::Building(building)
                if building.generation == root.generation && !building.structural_superseded =>
            {
                (root.generation, CallHierarchyIndexPrepareAction::UseExisting)
            }
            Lifecycle::Idle | Lifecycle::Building(_) | Lifecycle::Failed(_) => {
                (root.generation.checked_add(1)?, CallHierarchyIndexPrepareAction::StartBuild)
            }
        };
        root.prepared_generation =
            Some(root.prepared_generation.map_or(generation, |prepared| prepared.max(generation)));
        Some((generation, action))
    }

    pub fn record_prepare(&self, source_root: SourceRootId, generation: u64) -> bool {
        let mut state = self.inner.write();
        let root = state.roots.entry(source_root).or_default();
        match root.prepared_generation {
            Some(prepared) if prepared >= generation => false,
            Some(_) | None => {
                root.prepared_generation = Some(generation);
                true
            }
        }
    }

    pub fn is_prepared(&self, source_root: SourceRootId, generation: u64) -> bool {
        self.inner
            .read()
            .roots
            .get(&source_root)
            .and_then(|root| root.prepared_generation)
            .is_some_and(|prepared| prepared >= generation)
    }

    pub fn is_ready_generation(&self, source_root: SourceRootId, generation: u64) -> bool {
        let state = self.inner.read();
        matches!(state.roots.get(&source_root), Some(root) if matches!(&root.lifecycle, Lifecycle::Ready(_)) && root.generation == generation)
    }

    pub fn next_generation(&self, source_root: SourceRootId) -> Option<u64> {
        match self.inner.read().roots.get(&source_root) {
            Some(root) => root.generation.checked_add(1),
            None => Some(1),
        }
    }

    pub fn current(&self, source_root: SourceRootId) -> Option<Arc<CallHierarchyReverseIndex>> {
        self.inner.read().roots.get(&source_root).and_then(RootState::current)
    }

    pub fn generation(&self, source_root: SourceRootId) -> Option<u64> {
        self.inner.read().roots.get(&source_root).map(|root| root.generation)
    }
}
