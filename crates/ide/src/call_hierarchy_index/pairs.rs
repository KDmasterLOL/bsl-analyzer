use hir::{
    graph_index::{project_batch_method_call_pairs, GraphIndex},
    CallHierarchyReverseIndex, MethodCallPair,
};
use rustc_hash::FxHashMap;

use super::{
    BatchLifecycle, CallHierarchyBatchPhase, CallHierarchyIndexBuildError,
    CallHierarchyIndexBuildRequest,
};
use crate::graph::{run_batch_db, BatchDbOpener, BatchDbRelease};

pub(super) fn project_and_fold_method_pairs(
    request: CallHierarchyIndexBuildRequest<'_>,
    open_batch: &mut BatchDbOpener<'_>,
    pool: &rayon::ThreadPool,
    graph_index: &GraphIndex,
    lifecycle: &mut BatchLifecycle<'_>,
) -> Result<CallHierarchyReverseIndex, CallHierarchyIndexBuildError> {
    let batch_size = request.batch_size.max(1);
    let mut layout_hashes = FxHashMap::default();
    let mut module_pairs = FxHashMap::default();
    for &module in request.modules {
        let layout_hash = graph_index
            .module_layout_hash(module)
            .ok_or(CallHierarchyIndexBuildError::MissingLayoutHash(module))?;
        layout_hashes.insert(module, layout_hash);
        module_pairs.insert(module, Vec::new());
    }

    for (batch_index, batch) in request.modules.chunks(batch_size).enumerate() {
        let _batch_span = tracing::info_span!(
            "call_hierarchy_index_build_batch",
            phase = "pass2",
            batch_index,
            batch_size = batch.len(),
            implementation = "compact_reverse_index",
            workspace_call_graph = false,
        )
        .entered();
        tracing::debug!(
            phase = "pass2",
            batch_index,
            batch_size = batch.len(),
            "call hierarchy compact index batch started"
        );
        lifecycle.started(CallHierarchyBatchPhase::MethodPairs, batch_index, batch.len());
        run_batch_db(
            batch,
            open_batch,
            pool,
            |db| {
                let pairs = project_batch_method_call_pairs(db, graph_index, batch);
                let pair_count = pairs.len();
                for (module, pairs) in MethodCallPair::group_by_caller_module(&pairs) {
                    module_pairs.entry(module).or_default().extend(pairs);
                }
                pair_count
            },
            |release| {
                let &pair_count = match release {
                    BatchDbRelease::DatabaseDropped(count)
                    | BatchDbRelease::NodeCachesCleared(count) => count,
                };
                match release {
                    BatchDbRelease::DatabaseDropped(_) => {
                        tracing::debug!(
                            phase = "pass2",
                            batch_index,
                            batch_size = batch.len(),
                            pair_count,
                            "call hierarchy compact index batch database dropped"
                        );
                        lifecycle.database_dropped(
                            CallHierarchyBatchPhase::MethodPairs,
                            batch_index,
                            batch.len(),
                            pair_count,
                        );
                    }
                    BatchDbRelease::NodeCachesCleared(_) => {
                        lifecycle.node_caches_cleared(
                            pool,
                            CallHierarchyBatchPhase::MethodPairs,
                            batch_index,
                        );
                        lifecycle.completed(
                            CallHierarchyBatchPhase::MethodPairs,
                            batch_index,
                            batch.len(),
                            pair_count,
                        );
                        tracing::debug!(
                            phase = "pass2",
                            batch_index,
                            batch_size = batch.len(),
                            pair_count,
                            "call hierarchy compact index batch completed"
                        );
                    }
                }
            },
        );
    }

    let modules = request
        .modules
        .iter()
        .map(|&module| {
            let layout_hash = layout_hashes
                .get(&module)
                .copied()
                .ok_or(CallHierarchyIndexBuildError::MissingLayoutHash(module))?;
            let pairs = module_pairs.remove(&module).unwrap_or_default();
            Ok((module, pairs, layout_hash))
        })
        .collect::<Result<Vec<_>, CallHierarchyIndexBuildError>>()?;
    Ok(CallHierarchyReverseIndex::from_modules(modules))
}
