//! Bounded task pool for running background tasks with result reporting.
//!
//! Each spawned closure runs on one of a fixed number of worker threads pulled
//! from a crossbeam channel. Previously the pool used `std::thread::spawn` per
//! task, so a scroll-spam scenario (e.g. `documentHighlight` per cursor move
//! on a 25k-file workspace) could fan out to hundreds of concurrent Salsa
//! snapshots, each pinning a workspace-wide HIR cache.

use std::num::NonZeroUsize;
use std::sync::OnceLock;
use std::thread::available_parallelism;

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

/// Maximum default worker count when nothing else is specified.
///
/// Picked empirically: more than 8 concurrent navigation handlers on a
/// 25k-file project provides no perceived latency win but multiplies the
/// memory footprint linearly.
const DEFAULT_MAX_WORKERS: usize = 8;

/// Environment override for the worker count. Useful for diagnosing
/// oversubscription / undersubscription without recompiling.
const WORKER_COUNT_ENV: &str = "BSL_LSP_WORKERS";

/// Job queue capacity multiplier. Total in-flight closures (running + waiting)
/// is bounded by `workers + workers * BACKPRESSURE_FACTOR`; on the LSP-server
/// hot path each closure captures a heavyweight `LatencyRequestContext`
/// (cloned Salsa snapshot + frozen `MemDocs` + `FrozenFilePaths`), so the
/// queue itself — not just the worker set — must apply backpressure.
const BACKPRESSURE_FACTOR: usize = 4;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// A bounded task pool that runs `spawn`ed closures on a fixed worker set.
///
/// Each worker thread loops on a shared task queue; when every `Sender<Job>`
/// gets dropped the workers exit cleanly. The pool's `Sender<T>` is kept
/// public so notification handlers can forward `Task` events to the main
/// event loop without a round-trip through `spawn`.
pub struct TaskPool<T> {
    /// Sender for task results (consumed by the main event loop).
    pub sender: Sender<T>,
    /// Sender for queued jobs (consumed by worker threads).
    job_sender: Sender<Job>,
}

/// Handle bundling the pool, the result receiver, and join handles for workers.
pub struct Handle<T> {
    pub pool: TaskPool<T>,
    pub receiver: Receiver<T>,
    /// Join handles for spawned worker threads. Dropping the `Handle`
    /// implicitly closes the job channel because `pool` owns the only
    /// `Sender<Job>`, and the workers exit on `recv` error.
    _workers: Vec<std::thread::JoinHandle<()>>,
}

impl<T: Send + 'static> TaskPool<T> {
    /// Creates a new task pool sized by [`worker_count`].
    pub fn new_with_handle() -> Handle<T> {
        Self::new_with_workers(worker_count())
    }

    /// Creates a new task pool with an explicit worker count. Intended for tests.
    pub fn new_with_workers(workers: usize) -> Handle<T> {
        let workers = workers.max(1);
        let (sender, receiver) = unbounded();
        let capacity = workers.saturating_mul(BACKPRESSURE_FACTOR).max(workers);
        let (job_sender, job_receiver) = bounded::<Job>(capacity);

        tracing::info!(workers, capacity, "Starting bounded task pool");

        let mut handles = Vec::with_capacity(workers);
        for worker_idx in 0..workers {
            let job_rx = job_receiver.clone();
            let handle = std::thread::Builder::new()
                .name(format!("bsl-task-pool-{worker_idx}"))
                .spawn(move || worker_loop(job_rx))
                .expect("failed to spawn task pool worker");
            handles.push(handle);
        }

        Handle { pool: TaskPool { sender, job_sender }, receiver, _workers: handles }
    }

    /// Enqueues a job whose result is published on the shared result channel.
    ///
    /// Blocks if the job queue is at capacity. Backpressure on the main loop
    /// is intentional: each pending job retains a heavyweight Salsa snapshot
    /// captured by the caller, so admitting more closures than the workers
    /// can drain would defeat the entire reason the pool is bounded.
    pub fn spawn<F>(&self, task: F)
    where
        F: FnOnce() -> T + Send + 'static,
    {
        let sender = self.sender.clone();
        let job: Job = Box::new(move || {
            let result = task();
            let _ = sender.send(result);
        });
        if let Err(err) = self.job_sender.send(job) {
            tracing::warn!(?err, "task pool job channel closed; dropping task");
        }
    }

    /// Returns the job queue's capacity if it is bounded.
    ///
    /// Exposed for tests and telemetry; production callers should treat
    /// capacity as an implementation detail of [`new_with_handle`].
    pub fn capacity(&self) -> Option<usize> {
        self.job_sender.capacity()
    }
}

impl<T: Send + 'static> Default for TaskPool<T> {
    fn default() -> Self {
        Self::new_with_handle().pool
    }
}

fn worker_loop(job_receiver: Receiver<Job>) {
    while let Ok(job) = job_receiver.recv() {
        // A panicking handler must not kill the worker — losing one slot to
        // a stuck thread would silently halve the pool's capacity. Catching
        // here keeps the pool steady while letting the handler's failure
        // surface through its own `Task::RequestResult` (handlers wrap their
        // bodies in `salsa::Cancelled::catch` before publishing).
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
    }
}

/// Resolved number of workers for [`TaskPool::new_with_handle`].
///
/// Resolution priority:
/// 1. `BSL_LSP_WORKERS` env (parsed as `usize`, clamped to ≥ 1).
/// 2. `min(`[`DEFAULT_MAX_WORKERS`]`, available_parallelism)`.
/// 3. Final `1` if `available_parallelism` returns an error.
fn worker_count() -> usize {
    static CACHED: OnceLock<usize> = OnceLock::new();
    *CACHED.get_or_init(|| {
        if let Ok(raw) = std::env::var(WORKER_COUNT_ENV) {
            match raw.parse::<usize>() {
                Ok(parsed) => {
                    let clamped = parsed.max(1);
                    tracing::info!(workers = clamped, raw = %raw, "Resolved BSL_LSP_WORKERS");
                    return clamped;
                }
                Err(_) => tracing::warn!(
                    raw = %raw,
                    "BSL_LSP_WORKERS is not a valid usize; falling back to default"
                ),
            }
        }
        let cpus = available_parallelism().map(NonZeroUsize::get).unwrap_or(1);
        cpus.clamp(1, DEFAULT_MAX_WORKERS)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn bounded_pool_executes_more_jobs_than_workers() {
        let workers = 2;
        let handle = TaskPool::<usize>::new_with_workers(workers);
        let counter = Arc::new(AtomicUsize::new(0));

        let jobs = 20;
        for _ in 0..jobs {
            let counter = Arc::clone(&counter);
            handle.pool.spawn(move || {
                let v = counter.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(2));
                v + 1
            });
        }

        let mut received = 0;
        while received < jobs {
            match handle.receiver.recv_timeout(Duration::from_secs(2)) {
                Ok(_) => received += 1,
                Err(err) => panic!("missing result after {received} of {jobs}: {err}"),
            }
        }
        assert_eq!(counter.load(Ordering::SeqCst), jobs);
    }

    #[test]
    fn job_channel_capacity_scales_with_workers() {
        for workers in [1usize, 4, 8] {
            let handle = TaskPool::<()>::new_with_workers(workers);
            assert_eq!(
                handle.pool.capacity(),
                Some(workers * BACKPRESSURE_FACTOR),
                "queue capacity must be workers × BACKPRESSURE_FACTOR for backpressure"
            );
        }
    }

    #[test]
    fn panicking_job_does_not_kill_pool() {
        let handle = TaskPool::<&'static str>::new_with_workers(1);
        handle.pool.spawn(|| panic!("intentional"));
        // Pool must still accept and complete subsequent jobs after a panic.
        handle.pool.spawn(|| "ok");
        let result = handle
            .receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("second job should run after panicking peer");
        assert_eq!(result, "ok");
    }
}
