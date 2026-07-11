use std::time::{Duration, Instant};

use hir::{
    graph_index::{project_batch_method_call_pairs, GraphIndex},
    CallHierarchyReverseIndex, MethodCallPair, ModuleId,
};
use rustc_hash::FxHashMap;

use crate::graph::BatchDbOpener;

/// Borrowed inputs for a bounded call-hierarchy build.
#[derive(Clone, Copy)]
pub struct CallHierarchyIndexBuildRequest<'a> {
    pub modules: &'a [ModuleId],
    pub batch_size: usize,
}

impl<'a> CallHierarchyIndexBuildRequest<'a> {
    pub const fn new(modules: &'a [ModuleId], batch_size: usize) -> Self {
        Self { modules, batch_size }
    }
}

/// The two passes that access a fresh batch database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallHierarchyBatchPhase {
    Index,
    MethodPairs,
}

/// A lifecycle boundary for one fresh batch database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallHierarchyBatchEventKind {
    Started,
    DatabaseDropped,
    Completed,
}

/// A timestamped batch lifecycle event retained for build diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallHierarchyBatchEvent {
    pub phase: CallHierarchyBatchPhase,
    pub kind: CallHierarchyBatchEventKind,
    pub batch_index: usize,
    pub module_count: usize,
    pub pair_count: usize,
    pub elapsed: Duration,
}

/// Resident-set sample captured after a batch database and parser caches are released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallHierarchyRssSample {
    pub phase: CallHierarchyBatchPhase,
    pub batch_index: usize,
    pub bytes: Option<usize>,
}

/// Compact call-hierarchy data and measurements produced by a bounded build.
#[derive(Debug)]
pub struct CallHierarchyIndexBuildResult {
    pub index: CallHierarchyReverseIndex,
    pub method_count: usize,
    pub pair_count: usize,
    pub batch_events: Vec<CallHierarchyBatchEvent>,
    pub rss_samples: Vec<CallHierarchyRssSample>,
    pub elapsed: Duration,
    pub estimated_heap_bytes: usize,
}

/// Failures possible before a complete compact index is produced.
#[derive(Debug, thiserror::Error)]
pub enum CallHierarchyIndexBuildError {
    #[error("failed to build the call-hierarchy rayon pool: {0}")]
    ThreadPool(#[from] rayon::ThreadPoolBuildError),
    #[error("missing layout hash for indexed module {0:?}")]
    MissingLayoutHash(ModuleId),
}

trait BatchObserver {
    fn on_event(&mut self, event: &CallHierarchyBatchEvent);
    fn on_node_caches_cleared(&mut self, phase: CallHierarchyBatchPhase, batch_index: usize);
}

struct NoopBatchObserver;

impl BatchObserver for NoopBatchObserver {
    fn on_event(&mut self, _: &CallHierarchyBatchEvent) {}

    fn on_node_caches_cleared(&mut self, _: CallHierarchyBatchPhase, _: usize) {}
}

/// Builds a compact reverse call index while keeping Salsa and parser residency to one batch.
pub fn build_call_hierarchy_index(
    request: CallHierarchyIndexBuildRequest<'_>,
    open_batch: &mut BatchDbOpener<'_>,
) -> Result<CallHierarchyIndexBuildResult, CallHierarchyIndexBuildError> {
    let mut observer = NoopBatchObserver;
    build_call_hierarchy_index_with_observer(request, open_batch, &mut observer)
}

fn build_call_hierarchy_index_with_observer(
    request: CallHierarchyIndexBuildRequest<'_>,
    open_batch: &mut BatchDbOpener<'_>,
    observer: &mut dyn BatchObserver,
) -> Result<CallHierarchyIndexBuildResult, CallHierarchyIndexBuildError> {
    let started = Instant::now();
    let batch_size = request.batch_size.max(1);
    let pool = rayon::ThreadPoolBuilder::new().build()?;
    let mut events = Vec::new();
    let mut rss_samples = Vec::new();
    let mut graph_index = GraphIndex::new();

    for (batch_index, batch) in request.modules.chunks(batch_size).enumerate() {
        record_event(
            &mut events,
            observer,
            started,
            CallHierarchyBatchPhase::Index,
            CallHierarchyBatchEventKind::Started,
            batch_index,
            batch.len(),
            0,
        );
        let db = open_batch(batch);
        graph_index.add_batch(&pool, &db, batch);
        drop(db);
        record_event(
            &mut events,
            observer,
            started,
            CallHierarchyBatchPhase::Index,
            CallHierarchyBatchEventKind::DatabaseDropped,
            batch_index,
            batch.len(),
            0,
        );
        clear_node_caches(&pool);
        observer.on_node_caches_cleared(CallHierarchyBatchPhase::Index, batch_index);
        rss_samples.push(rss_sample(CallHierarchyBatchPhase::Index, batch_index));
        record_event(
            &mut events,
            observer,
            started,
            CallHierarchyBatchPhase::Index,
            CallHierarchyBatchEventKind::Completed,
            batch_index,
            batch.len(),
            0,
        );
    }

    let method_count = graph_index.method_nodes().count();
    let mut layout_hashes = FxHashMap::default();
    let mut index = CallHierarchyReverseIndex::new();
    for &module in request.modules {
        let layout_hash = graph_index
            .module_layout_hash(module)
            .ok_or(CallHierarchyIndexBuildError::MissingLayoutHash(module))?;
        layout_hashes.insert(module, layout_hash);
        index.replace_module(module, [], layout_hash);
    }

    for (batch_index, batch) in request.modules.chunks(batch_size).enumerate() {
        record_event(
            &mut events,
            observer,
            started,
            CallHierarchyBatchPhase::MethodPairs,
            CallHierarchyBatchEventKind::Started,
            batch_index,
            batch.len(),
            0,
        );
        let db = open_batch(batch);
        let pairs = project_batch_method_call_pairs(&db, &graph_index, batch);
        let pair_count = pairs.len();
        drop(db);
        record_event(
            &mut events,
            observer,
            started,
            CallHierarchyBatchPhase::MethodPairs,
            CallHierarchyBatchEventKind::DatabaseDropped,
            batch_index,
            batch.len(),
            pair_count,
        );
        clear_node_caches(&pool);
        observer.on_node_caches_cleared(CallHierarchyBatchPhase::MethodPairs, batch_index);
        rss_samples.push(rss_sample(CallHierarchyBatchPhase::MethodPairs, batch_index));
        for &module in batch {
            let layout_hash = layout_hashes
                .get(&module)
                .copied()
                .ok_or(CallHierarchyIndexBuildError::MissingLayoutHash(module))?;
            index.replace_module(
                module,
                pairs
                    .iter()
                    .filter(|(caller, _)| caller.module == module)
                    .map(|&(caller, target)| MethodCallPair::new(caller, target)),
                layout_hash,
            );
        }
        record_event(
            &mut events,
            observer,
            started,
            CallHierarchyBatchPhase::MethodPairs,
            CallHierarchyBatchEventKind::Completed,
            batch_index,
            batch.len(),
            pair_count,
        );
    }

    drop(graph_index);
    let pair_count = index.len();
    let estimated_heap_bytes = index.estimated_heap_bytes();
    Ok(CallHierarchyIndexBuildResult {
        index,
        method_count,
        pair_count,
        batch_events: events,
        rss_samples,
        elapsed: started.elapsed(),
        estimated_heap_bytes,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "batch events retain every requested dimension without an ephemeral allocation"
)]
fn record_event(
    events: &mut Vec<CallHierarchyBatchEvent>,
    observer: &mut dyn BatchObserver,
    started: Instant,
    phase: CallHierarchyBatchPhase,
    kind: CallHierarchyBatchEventKind,
    batch_index: usize,
    module_count: usize,
    pair_count: usize,
) {
    let event = CallHierarchyBatchEvent {
        phase,
        kind,
        batch_index,
        module_count,
        pair_count,
        elapsed: started.elapsed(),
    };
    observer.on_event(&event);
    events.push(event);
}

fn clear_node_caches(pool: &rayon::ThreadPool) {
    syntax::clear_shared_node_cache();
    pool.broadcast(|_| syntax::clear_shared_node_cache());
}

fn rss_sample(phase: CallHierarchyBatchPhase, batch_index: usize) -> CallHierarchyRssSample {
    CallHierarchyRssSample { phase, batch_index, bytes: current_rss_bytes() }
}

fn current_rss_bytes() -> Option<usize> {
    std::fs::read_to_string("/proc/self/status").ok()?.lines().find_map(|line| {
        let kibibytes =
            line.strip_prefix("VmRSS:")?.split_whitespace().next()?.parse::<usize>().ok()?;
        kibibytes.checked_mul(1024)
    })
}

#[cfg(test)]
mod tests;
