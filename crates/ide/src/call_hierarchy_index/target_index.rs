use hir::graph_index::GraphIndex;

use super::{BatchLifecycle, CallHierarchyBatchPhase, CallHierarchyIndexBuildRequest};
use crate::graph::{run_batch_db, BatchDbOpener, BatchDbRelease};

pub(super) fn build_graph_index(
    request: CallHierarchyIndexBuildRequest<'_>,
    open_batch: &mut BatchDbOpener<'_>,
    pool: &rayon::ThreadPool,
    lifecycle: &mut BatchLifecycle<'_>,
) -> (GraphIndex, usize) {
    let batch_size = request.batch_size.max(1);
    let mut graph_index = GraphIndex::new();

    for (batch_index, batch) in request.modules.chunks(batch_size).enumerate() {
        let _batch_span = tracing::info_span!(
            "call_hierarchy_index_build_batch",
            phase = "pass1",
            batch_index,
            batch_size = batch.len(),
            implementation = "compact_reverse_index",
            workspace_call_graph = false,
        )
        .entered();
        tracing::debug!(
            phase = "pass1",
            batch_index,
            batch_size = batch.len(),
            "call hierarchy compact index batch started"
        );
        lifecycle.started(CallHierarchyBatchPhase::Index, batch_index, batch.len());
        run_batch_db(
            batch,
            open_batch,
            pool,
            |db| graph_index.add_batch(pool, db, batch),
            |release| match release {
                BatchDbRelease::DatabaseDropped(_) => {
                    tracing::debug!(
                        phase = "pass1",
                        batch_index,
                        batch_size = batch.len(),
                        "call hierarchy compact index batch database dropped"
                    );
                    lifecycle.database_dropped(
                        CallHierarchyBatchPhase::Index,
                        batch_index,
                        batch.len(),
                        0,
                    );
                }
                BatchDbRelease::NodeCachesCleared(_) => {
                    lifecycle.node_caches_cleared(
                        pool,
                        CallHierarchyBatchPhase::Index,
                        batch_index,
                    );
                    lifecycle.completed(
                        CallHierarchyBatchPhase::Index,
                        batch_index,
                        batch.len(),
                        0,
                    );
                    tracing::debug!(
                        phase = "pass1",
                        batch_index,
                        batch_size = batch.len(),
                        "call hierarchy compact index batch completed"
                    );
                }
            },
        );
    }

    let method_count = graph_index.method_nodes().count();
    tracing::debug!(
        phase = "pass1",
        method_count,
        "call hierarchy compact index method discovery completed"
    );
    (graph_index, method_count)
}
