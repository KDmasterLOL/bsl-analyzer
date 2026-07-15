use std::collections::VecDeque;

use hir::{
    call_graph::ModuleCallSummary,
    graph_index::{extract_batch_index_and_pair_intents, GraphIndex, ModuleIndexExtraction},
    ModuleId,
};

use super::{
    BatchLifecycle, CallHierarchyBatchPhase, CallHierarchyIndexBuildError,
    CallHierarchyIndexBuildRequest,
};
use crate::graph::{clear_node_caches, run_batch_db, BatchDbOpener, BatchDbRelease};

/// Per-module retained pair intents, in module order across all batches.
type RetainedIntents = Vec<(ModuleId, ModuleCallSummary)>;

/// Builds the resident method index over every module AND, in the same parallel
/// region, extracts each module's pair-relevant call-summary subset — so every
/// file is parsed and lowered exactly once for the whole build, instead of once
/// for the index and again for the pair projection. The retained subsets are
/// resolved against the completed index later (see `pairs`), which is what makes
/// the single pass possible: resolution needs the whole-config index, extraction
/// does not.
///
/// With `request.concurrency >= 2` the batches run pipelined: up to that many
/// batch databases are in flight, each extracting on its own rayon pool (Salsa
/// permits at most one database per thread, so lanes must not share workers),
/// while the driver thread opens the next database and folds finished lanes in
/// batch order. This overlaps one batch's serial windows — open, drop, and the
/// tail where its largest module finishes alone — with another batch's parallel
/// work, at the cost of the extra in-flight batches' transient residency.
pub(super) fn build_graph_index_extracting_intents(
    request: CallHierarchyIndexBuildRequest<'_>,
    open_batch: &mut BatchDbOpener<'_>,
    pool: &rayon::ThreadPool,
    lifecycle: &mut BatchLifecycle<'_>,
) -> Result<(GraphIndex, usize, RetainedIntents), CallHierarchyIndexBuildError> {
    let batch_size = request.batch_size.max(1);
    let batch_count = request.modules.len().div_ceil(batch_size);
    let lanes = request.concurrency.clamp(1, batch_count.max(1));
    let mut graph_index = GraphIndex::new();
    let mut intents = Vec::with_capacity(request.modules.len());

    if lanes <= 1 {
        build_sequential(
            request,
            batch_size,
            open_batch,
            pool,
            lifecycle,
            &mut graph_index,
            &mut intents,
        );
    } else {
        build_pipelined(
            request,
            batch_size,
            lanes,
            open_batch,
            lifecycle,
            &mut graph_index,
            &mut intents,
        )?;
    }

    let method_count = graph_index.method_nodes().count();
    tracing::debug!(
        phase = "pass1",
        method_count,
        "call hierarchy compact index method discovery completed"
    );
    Ok((graph_index, method_count, intents))
}

fn build_sequential(
    request: CallHierarchyIndexBuildRequest<'_>,
    batch_size: usize,
    open_batch: &mut BatchDbOpener<'_>,
    pool: &rayon::ThreadPool,
    lifecycle: &mut BatchLifecycle<'_>,
    graph_index: &mut GraphIndex,
    intents: &mut RetainedIntents,
) {
    for (batch_index, batch) in request.modules.chunks(batch_size).enumerate() {
        let _batch_span = batch_span(batch_index, batch.len()).entered();
        batch_started(lifecycle, batch_index, batch.len());
        run_batch_db(
            batch,
            open_batch,
            pool,
            |db| intents.extend(graph_index.add_batch_extracting_pair_intents(pool, db, batch)),
            |release| match release {
                BatchDbRelease::DatabaseDropped(_) => {
                    batch_database_dropped(lifecycle, batch_index, batch.len());
                }
                BatchDbRelease::NodeCachesCleared(_) => {
                    lifecycle.node_caches_cleared(CallHierarchyBatchPhase::Index, batch_index);
                    batch_completed(lifecycle, batch_index, batch.len());
                }
            },
        );
    }
}

fn build_pipelined(
    request: CallHierarchyIndexBuildRequest<'_>,
    batch_size: usize,
    lanes: usize,
    open_batch: &mut BatchDbOpener<'_>,
    lifecycle: &mut BatchLifecycle<'_>,
    graph_index: &mut GraphIndex,
    intents: &mut RetainedIntents,
) -> Result<(), CallHierarchyIndexBuildError> {
    let lane_pools = (0..lanes)
        .map(|_| rayon::ThreadPoolBuilder::new().build())
        .collect::<Result<Vec<_>, _>>()?;

    std::thread::scope(|scope| {
        struct Inflight<'scope> {
            batch_index: usize,
            module_count: usize,
            handle: std::thread::ScopedJoinHandle<'scope, Vec<ModuleIndexExtraction>>,
        }

        let mut inflight: VecDeque<Inflight<'_>> = VecDeque::with_capacity(lanes);
        let fold_oldest = |inflight: &mut VecDeque<Inflight<'_>>,
                           graph_index: &mut GraphIndex,
                           intents: &mut RetainedIntents,
                           lifecycle: &mut BatchLifecycle<'_>| {
            let Some(job) = inflight.pop_front() else { return };
            let extractions =
                job.handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            batch_database_dropped(lifecycle, job.batch_index, job.module_count);
            lifecycle.node_caches_cleared(CallHierarchyBatchPhase::Index, job.batch_index);
            for extraction in extractions {
                intents.push(graph_index.insert_extraction(extraction));
            }
            batch_completed(lifecycle, job.batch_index, job.module_count);
        };

        for (batch_index, batch) in request.modules.chunks(batch_size).enumerate() {
            if inflight.len() == lanes {
                fold_oldest(&mut inflight, graph_index, intents, lifecycle);
            }
            let _batch_span = batch_span(batch_index, batch.len()).entered();
            batch_started(lifecycle, batch_index, batch.len());
            let db = open_batch(batch);
            let lane_pool = &lane_pools[batch_index % lanes];
            let handle = scope.spawn(move || {
                let extractions = extract_batch_index_and_pair_intents(lane_pool, &db, batch);
                drop(db);
                clear_node_caches(lane_pool);
                extractions
            });
            inflight.push_back(Inflight { batch_index, module_count: batch.len(), handle });
        }
        while !inflight.is_empty() {
            fold_oldest(&mut inflight, graph_index, intents, lifecycle);
        }
    });
    Ok(())
}

fn batch_span(batch_index: usize, batch_size: usize) -> tracing::Span {
    tracing::info_span!(
        "call_hierarchy_index_build_batch",
        phase = "pass1",
        batch_index,
        batch_size,
        implementation = "compact_reverse_index",
        workspace_call_graph = false,
    )
}

fn batch_started(lifecycle: &mut BatchLifecycle<'_>, batch_index: usize, module_count: usize) {
    tracing::debug!(
        phase = "pass1",
        batch_index,
        batch_size = module_count,
        "call hierarchy compact index batch started"
    );
    lifecycle.started(CallHierarchyBatchPhase::Index, batch_index, module_count);
}

fn batch_database_dropped(
    lifecycle: &mut BatchLifecycle<'_>,
    batch_index: usize,
    module_count: usize,
) {
    tracing::debug!(
        phase = "pass1",
        batch_index,
        batch_size = module_count,
        "call hierarchy compact index batch database dropped"
    );
    lifecycle.database_dropped(CallHierarchyBatchPhase::Index, batch_index, module_count, 0);
}

fn batch_completed(lifecycle: &mut BatchLifecycle<'_>, batch_index: usize, module_count: usize) {
    lifecycle.completed(CallHierarchyBatchPhase::Index, batch_index, module_count, 0);
    tracing::debug!(
        phase = "pass1",
        batch_index,
        batch_size = module_count,
        "call hierarchy compact index batch completed"
    );
}
