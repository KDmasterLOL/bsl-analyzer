use crate::call_hierarchy_index_overlay::CallHierarchyIndexFrozenSnapshot;
use crate::call_hierarchy_index_state::CallHierarchyIndexState;
use crate::global_state::Task;
use crate::mem_docs::MemDocs;

use super::reconcile::reconcile;
use super::BATCH_SIZE;

pub(super) struct BuildContext {
    pub(super) lifecycle: CallHierarchyIndexState,
    pub(super) mem_docs: MemDocs,
    pub(super) frozen: CallHierarchyIndexFrozenSnapshot,
}

pub(super) fn run_build(
    lifecycle: CallHierarchyIndexState,
    mem_docs: MemDocs,
    frozen: CallHierarchyIndexFrozenSnapshot,
) -> Task {
    let context = BuildContext { lifecycle, mem_docs, frozen };
    let source_root = context.frozen.source_root_id;
    let generation = context.frozen.creation_generation;
    let _worker_span = tracing::info_span!(
        "call_hierarchy_index_worker",
        ?source_root,
        generation,
        batch_size = BATCH_SIZE,
        implementation = "compact_reverse_index",
        workspace_call_graph = false,
    )
    .entered();
    tracing::debug!(phase = "build", "call hierarchy compact index worker started");
    let recovery_lifecycle = context.lifecycle.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(context)));

    match result {
        Ok(task) => task,
        Err(_) => {
            tracing::error!(
                ?source_root,
                generation,
                phase = "failure",
                failure_reason = "panic",
                "call hierarchy compact index worker panicked"
            );
            superseded(&recovery_lifecycle, source_root, generation)
        }
    }
}

fn build(context: BuildContext) -> Task {
    let source_root = context.frozen.source_root_id;
    let generation = context.frozen.creation_generation;
    if !context.lifecycle.is_building(source_root, generation) {
        tracing::debug!(
            phase = "supersession",
            supersession_reason = "generation_not_building",
            "call hierarchy compact index build superseded"
        );
        return superseded(&context.lifecycle, source_root, generation);
    }
    let BuildContext { lifecycle, mem_docs, frozen } = context;
    let frozen = frozen.materialize();
    let _freeze_span = tracing::info_span!(
        "call_hierarchy_index_build_phase",
        phase = "freeze",
        ?source_root,
        generation,
    )
    .entered();
    frozen.freeze_config_inputs();
    let modules = frozen.modules();
    tracing::debug!(
        phase = "freeze",
        module_count = modules.len(),
        "call hierarchy compact index configuration freeze completed"
    );
    let mut open_batch = |batch: &[ide::ModuleId]| frozen.open_batch(batch);
    let built = ide::build_call_hierarchy_index(
        ide::CallHierarchyIndexBuildRequest::new(&modules, BATCH_SIZE),
        &mut open_batch,
    );
    let built = match built {
        Ok(built) => built,
        Err(error) => {
            tracing::warn!(
                phase = "failure",
                failure_reason = %error,
                "call hierarchy compact index build failed"
            );
            return Task::CallHierarchyIndexFailed {
                source_root,
                generation,
                reason: error.to_string(),
            };
        }
    };
    tracing::debug!(
        phase = "build",
        method_count = built.method_count,
        pair_count = built.pair_count,
        estimated_heap_bytes = built.estimated_heap_bytes,
        "call hierarchy compact index base build completed"
    );
    reconcile(BuildContext { lifecycle, mem_docs, frozen }, built.index, built.target_index)
}

pub(super) fn superseded(
    lifecycle: &CallHierarchyIndexState,
    source_root: base_db::SourceRootId,
    generation: u64,
) -> Task {
    tracing::debug!(
        ?source_root,
        generation,
        phase = "supersession",
        "call hierarchy compact index lifecycle marked superseded"
    );
    lifecycle.supersede(source_root);
    Task::CallHierarchyIndexSuperseded { source_root, generation }
}
