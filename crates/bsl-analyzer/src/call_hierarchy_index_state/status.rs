use super::*;

impl CallHierarchyIndexState {
    pub fn is_building(&self, source_root: SourceRootId, generation: u64) -> bool {
        let state = self.inner.read();
        matches!(state.roots.get(&source_root), Some(root) if matches!(&root.lifecycle, Lifecycle::Building(building) if building.generation == generation && !building.structural_superseded))
    }

    pub fn has_active_build(&self, source_root: SourceRootId) -> bool {
        self.inner
            .read()
            .roots
            .get(&source_root)
            .is_some_and(|root| matches!(&root.lifecycle, Lifecycle::Building(_)))
    }
}
