use super::*;

impl CallHierarchyIndexState {
    pub fn journal_files(&self, source_root: SourceRootId, generation: u64) -> Option<Vec<FileId>> {
        let journal = {
            let state = self.inner.read();
            let root = state.roots.get(&source_root)?;
            match &root.lifecycle {
                Lifecycle::Building(building) if building.generation == generation => {
                    Some(building.journal.clone())
                }
                Lifecycle::Idle
                | Lifecycle::Building(_)
                | Lifecycle::Ready(_)
                | Lifecycle::Failed(_) => None,
            }
        };
        journal.map(|journal| journal.files())
    }

    pub fn drain_journal(&self, source_root: SourceRootId, generation: u64) -> Option<Vec<FileId>> {
        let journal = {
            let mut state = self.inner.write();
            let root = state.roots.get_mut(&source_root)?;
            match &mut root.lifecycle {
                Lifecycle::Building(building)
                    if building.generation == generation && !building.structural_superseded =>
                {
                    Some(std::mem::take(&mut building.journal))
                }
                Lifecycle::Idle
                | Lifecycle::Building(_)
                | Lifecycle::Ready(_)
                | Lifecycle::Failed(_) => None,
            }
        };
        journal.map(|journal| journal.files())
    }
}
