use hir::{
    call_graph::ModuleCallSummary,
    graph_index::{project_method_pairs_from_intents, GraphIndex},
    CallHierarchyReverseIndex, MethodCallPair, ModuleId,
};

use super::{
    BatchLifecycle, CallHierarchyBatchPhase, CallHierarchyIndexBuildError,
    CallHierarchyIndexBuildRequest,
};
use crate::graph::{run_batch_db, BatchDbOpener, BatchDbRelease};

/// How many extraction batches one resolve chunk spans. Resolution reads only
/// path and configuration state — never module texts — so its databases parse
/// nothing and a chunk can be much wider than an extraction batch. It must not
/// be the WHOLE workspace, though: an extension caller's visibility check
/// materialises a merged configuration (a deep clone of the base) per extension
/// inside the live database, so one workspace-wide database would hold every
/// extension's merge simultaneously. Chunking restores a residency bound while
/// keeping the per-chunk locate memos shared across thousands of modules.
const RESOLVE_CHUNK_BATCHES: usize = 16;

/// Resolves the retained pair intents against the completed `graph_index` and
/// folds the result into the compact reverse index.
///
/// Runs as one lifecycle batch per resolve chunk. A chunk's database registers
/// only that chunk's caller modules (registration matters: reading an
/// unregistered file's source root panics, and the caller-scoped resolvers read
/// their callers' source roots); target modules are resolved through the
/// resident `graph_index`, never through the database.
pub(super) fn resolve_and_fold_method_pairs(
    request: CallHierarchyIndexBuildRequest<'_>,
    open_batch: &mut BatchDbOpener<'_>,
    pool: &rayon::ThreadPool,
    graph_index: &GraphIndex,
    intents: &[(ModuleId, ModuleCallSummary)],
    lifecycle: &mut BatchLifecycle<'_>,
) -> Result<CallHierarchyReverseIndex, CallHierarchyIndexBuildError> {
    let chunk_size = request.batch_size.max(1).saturating_mul(RESOLVE_CHUNK_BATCHES);
    let mut per_module: Vec<(ModuleId, Vec<MethodCallPair>)> =
        Vec::with_capacity(request.modules.len());

    for (chunk_index, (chunk_modules, chunk_intents)) in
        request.modules.chunks(chunk_size).zip(intents.chunks(chunk_size)).enumerate()
    {
        debug_assert!(
            chunk_modules.len() == chunk_intents.len()
                && chunk_modules
                    .iter()
                    .zip(chunk_intents)
                    .all(|(&module, &(intent_module, _))| module == intent_module),
            "retained intents must align with request.modules — a chunk registers \
             exactly its callers' files",
        );
        let _batch_span = tracing::info_span!(
            "call_hierarchy_index_build_batch",
            phase = "pass2",
            batch_index = chunk_index,
            batch_size = chunk_modules.len(),
            implementation = "compact_reverse_index",
            workspace_call_graph = false,
        )
        .entered();
        tracing::debug!(
            phase = "pass2",
            batch_index = chunk_index,
            module_count = chunk_modules.len(),
            "call hierarchy compact index pair resolution started"
        );
        lifecycle.started(CallHierarchyBatchPhase::MethodPairs, chunk_index, chunk_modules.len());
        let resolved = run_batch_db(
            chunk_modules,
            open_batch,
            pool,
            |db| project_method_pairs_from_intents(pool, db, graph_index, chunk_intents),
            |release| {
                let resolved = match release {
                    BatchDbRelease::DatabaseDropped(resolved)
                    | BatchDbRelease::NodeCachesCleared(resolved) => resolved,
                };
                let pair_count = resolved.iter().map(|(_, pairs)| pairs.len()).sum();
                match release {
                    BatchDbRelease::DatabaseDropped(_) => {
                        tracing::debug!(
                            phase = "pass2",
                            batch_index = chunk_index,
                            module_count = chunk_modules.len(),
                            pair_count,
                            "call hierarchy compact index pair resolution database dropped"
                        );
                        lifecycle.database_dropped(
                            CallHierarchyBatchPhase::MethodPairs,
                            chunk_index,
                            chunk_modules.len(),
                            pair_count,
                        );
                    }
                    BatchDbRelease::NodeCachesCleared(_) => {
                        lifecycle
                            .node_caches_cleared(CallHierarchyBatchPhase::MethodPairs, chunk_index);
                        lifecycle.completed(
                            CallHierarchyBatchPhase::MethodPairs,
                            chunk_index,
                            chunk_modules.len(),
                            pair_count,
                        );
                        tracing::debug!(
                            phase = "pass2",
                            batch_index = chunk_index,
                            module_count = chunk_modules.len(),
                            pair_count,
                            "call hierarchy compact index pair resolution completed"
                        );
                    }
                }
            },
        );
        per_module.extend(resolved);
    }

    let modules = per_module
        .into_iter()
        .map(|(module, pairs)| {
            let layout_hash = graph_index
                .module_layout_hash(module)
                .ok_or(CallHierarchyIndexBuildError::MissingLayoutHash(module))?;
            Ok((module, pairs, layout_hash))
        })
        .collect::<Result<Vec<_>, CallHierarchyIndexBuildError>>()?;
    Ok(CallHierarchyReverseIndex::from_modules(modules))
}
