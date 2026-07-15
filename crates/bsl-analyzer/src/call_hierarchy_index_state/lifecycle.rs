use super::*;

mod shutdown;

impl CallHierarchyIndexState {
    /// Records a new frozen build unless this root already has that generation in flight.
    pub fn start_build(
        &self,
        source_root: SourceRootId,
        generation: u64,
        frozen_snapshot: CallHierarchyIndexSnapshotId,
    ) -> bool {
        self.start_build_inner(source_root, generation, frozen_snapshot)
    }

    fn start_build_inner(
        &self,
        source_root: SourceRootId,
        generation: u64,
        frozen_snapshot: CallHierarchyIndexSnapshotId,
    ) -> bool {
        let (started, superseded, retired) = {
            let mut state = self.inner.write();
            if state.shutdown {
                return false;
            }
            let root = state.roots.entry(source_root).or_default();
            let should_start = match &root.lifecycle {
                Lifecycle::Idle => generation >= root.generation,
                Lifecycle::Building(building) => generation > building.generation,
                Lifecycle::Ready(_) | Lifecycle::Failed(_) => generation > root.generation,
            };
            if !should_start {
                return false;
            }

            let previous = root.current();
            let superseded = match &mut root.lifecycle {
                Lifecycle::Building(building) => {
                    building.cancellation.cancel();
                    building.completion.take()
                }
                Lifecycle::Idle | Lifecycle::Ready(_) | Lifecycle::Failed(_) => None,
            };
            root.generation = generation;
            let retired = std::mem::replace(
                &mut root.lifecycle,
                Lifecycle::Building(Building {
                    generation,
                    frozen_snapshot,
                    journal: CallHierarchyIndexJournal::default(),
                    structural_superseded: false,
                    completion: None,
                    cancellation: CallHierarchyIndexCancellation::default(),
                    previous,
                }),
            );
            (true, superseded, retired)
        };
        // A final Arc drop can deallocate the resident index, so never do it while readers wait.
        drop(retired);
        notify(superseded, CallHierarchyIndexCompletion::Superseded);
        started
    }

    /// Atomically publishes a completed build only when its generation is still current.
    pub fn publish(
        &self,
        source_root: SourceRootId,
        generation: u64,
        index: Arc<CallHierarchyReverseIndex>,
    ) -> bool {
        let (waiter, retired) = {
            let mut state = self.inner.write();
            if state.shutdown {
                return false;
            }
            let Some(root) = state.roots.get_mut(&source_root) else {
                return false;
            };
            let Lifecycle::Building(building) = &mut root.lifecycle else {
                return false;
            };
            if root.generation != generation
                || building.generation != generation
                || building.structural_superseded
                || building.cancellation.is_cancelled()
                || !building.journal.is_empty()
            {
                return false;
            }
            let waiter = building.completion.take();
            let retired = std::mem::replace(
                &mut root.lifecycle,
                Lifecycle::Ready(Ready { index: Arc::clone(&index) }),
            );
            (waiter, retired)
        };
        drop(retired);
        notify(waiter, CallHierarchyIndexCompletion::Ready(index));
        true
    }

    pub fn record_body_edit_or_supersede_ready(
        &self,
        source_root: SourceRootId,
        generation: u64,
        file_id: FileId,
    ) -> bool {
        let (recorded, retired) = {
            let mut state = self.inner.write();
            let Some(root) = state.roots.get_mut(&source_root) else {
                return false;
            };
            if root.generation != generation {
                return false;
            }
            let terminal_generation = match &root.lifecycle {
                Lifecycle::Ready(_) => true,
                Lifecycle::Failed(failed) => failed.generation == generation,
                Lifecycle::Idle | Lifecycle::Building(_) => false,
            };
            if terminal_generation {
                let retired = std::mem::replace(&mut root.lifecycle, Lifecycle::Idle);
                (true, Some(retired))
            } else {
                let recorded = match &mut root.lifecycle {
                    Lifecycle::Building(building) if building.generation == generation => {
                        building.journal.record(file_id);
                        true
                    }
                    Lifecycle::Idle
                    | Lifecycle::Building(_)
                    | Lifecycle::Ready(_)
                    | Lifecycle::Failed(_) => false,
                };
                (recorded, None)
            }
        };
        drop(retired);
        recorded
    }

    /// Moves a matching lifecycle to Failed and wakes its single registered waiter.
    pub fn fail(&self, source_root: SourceRootId, generation: u64, reason: String) -> bool {
        let (waiter, retired) = {
            let mut state = self.inner.write();
            let Some(root) = state.roots.get_mut(&source_root) else {
                return false;
            };
            if root.generation != generation {
                return false;
            }
            let previous = root.current();
            let waiter = match &mut root.lifecycle {
                Lifecycle::Building(building) => building.completion.take(),
                Lifecycle::Idle | Lifecycle::Ready(_) | Lifecycle::Failed(_) => None,
            };
            let retired = std::mem::replace(
                &mut root.lifecycle,
                Lifecycle::Failed(Failed { generation, reason: reason.clone(), previous }),
            );
            (waiter, retired)
        };
        drop(retired);
        notify(waiter, CallHierarchyIndexCompletion::Failed(reason));
        true
    }

    /// Marks an in-flight build structurally stale; the caller must schedule its replacement.
    pub fn supersede(&self, source_root: SourceRootId) -> bool {
        let (waiter, retired) = {
            let mut state = self.inner.write();
            let Some(root) = state.roots.get_mut(&source_root) else {
                return false;
            };
            match &mut root.lifecycle {
                Lifecycle::Idle => return false,
                Lifecycle::Building(building) => {
                    if building.structural_superseded {
                        return false;
                    }
                    building.structural_superseded = true;
                    building.cancellation.cancel();
                    (building.completion.take(), None)
                }
                Lifecycle::Ready(_) | Lifecycle::Failed(_) => {
                    let retired = std::mem::replace(&mut root.lifecycle, Lifecycle::Idle);
                    (None, Some(retired))
                }
            }
        };
        drop(retired);
        notify(waiter, CallHierarchyIndexCompletion::Superseded);
        true
    }

    pub fn finish_superseded(&self, source_root: SourceRootId, generation: u64) -> bool {
        let retired = {
            let mut state = self.inner.write();
            let Some(root) = state.roots.get_mut(&source_root) else {
                return false;
            };
            let Lifecycle::Building(building) = &root.lifecycle else {
                return false;
            };
            if root.generation != generation
                || building.generation != generation
                || !building.structural_superseded
            {
                return false;
            }
            let previous = building.previous.clone();
            std::mem::replace(
                &mut root.lifecycle,
                Lifecycle::Failed(Failed {
                    generation,
                    reason: "call hierarchy index build superseded".to_owned(),
                    previous,
                }),
            )
        };
        drop(retired);
        true
    }
}

fn notify(
    waiter: Option<Sender<CallHierarchyIndexCompletion>>,
    completion: CallHierarchyIndexCompletion,
) {
    if let Some(waiter) = waiter {
        let _ = waiter.send(completion);
    }
}
