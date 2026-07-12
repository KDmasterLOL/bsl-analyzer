mod reconcile;
mod scheduler;
mod worker;

const BATCH_SIZE: usize = 250;
const CATCH_UP_PASSES: usize = 3;
const CATCH_UP_LIMIT: std::time::Duration = std::time::Duration::from_secs(1);

#[cfg(test)]
use crate::call_hierarchy_index_overlay::CallHierarchyIndexFrozenSnapshot;
#[cfg(test)]
use crate::call_hierarchy_index_state::CallHierarchyIndexState;
#[cfg(test)]
use crate::global_state::Task;
#[cfg(test)]
use reconcile::catch_up_exhausted;
#[cfg(test)]
use std::time::Instant;
#[cfg(test)]
use worker::run_build;

#[cfg(test)]
#[path = "../call_hierarchy_index_service_tests.rs"]
mod tests;
