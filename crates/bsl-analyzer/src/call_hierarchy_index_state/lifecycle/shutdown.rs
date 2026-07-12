use super::super::*;

impl CallHierarchyIndexState {
    /// Cancels active builds, wakes waiters, and permanently rejects new starts.
    pub fn shutdown(&self) {
        let (waiters, retired_lifecycles) = {
            let mut state = self.inner.write();
            if std::mem::replace(&mut state.shutdown, true) {
                return;
            }
            let mut waiters = Vec::new();
            let mut retired_lifecycles = Vec::with_capacity(state.roots.len());
            for root in state.roots.values_mut() {
                let (previous, waiter) = match &mut root.lifecycle {
                    Lifecycle::Idle => (None, None),
                    Lifecycle::Building(building) => {
                        building.cancellation.cancel();
                        (building.previous.clone(), building.completion.take())
                    }
                    Lifecycle::Ready(ready) => (Some(Arc::clone(&ready.index)), None),
                    Lifecycle::Failed(failed) => (failed.previous.clone(), None),
                };
                let retired = std::mem::replace(
                    &mut root.lifecycle,
                    Lifecycle::Failed(Failed {
                        generation: root.generation,
                        reason: "call hierarchy index service shut down".to_owned(),
                        previous,
                    }),
                );
                retired_lifecycles.push(retired);
                if let Some(waiter) = waiter {
                    waiters.push(waiter);
                }
            }
            (waiters, retired_lifecycles)
        };
        drop(retired_lifecycles);
        for waiter in waiters {
            super::notify(Some(waiter), CallHierarchyIndexCompletion::Shutdown);
        }
    }
}
