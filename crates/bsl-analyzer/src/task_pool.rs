use std::num::NonZeroUsize;
use std::sync::OnceLock;
use std::thread::available_parallelism;

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

const DEFAULT_MAX_WORKERS: usize = 8;

const WORKER_COUNT_ENV: &str = "BSL_LSP_WORKERS";

const BACKPRESSURE_FACTOR: usize = 4;

type Job = Box<dyn FnOnce() + Send + 'static>;

pub struct TaskPool<T> {
    pub sender: Sender<T>,
    job_sender: Sender<Job>,
}

pub struct Handle<T> {
    pub pool: TaskPool<T>,
    pub receiver: Receiver<T>,
    _workers: Vec<std::thread::JoinHandle<()>>,
}

impl<T: Send + 'static> TaskPool<T> {
    pub fn new_with_handle() -> Handle<T> {
        Self::new_with_workers(worker_count())
    }

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
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
    }
}

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
        handle.pool.spawn(|| "ok");
        let result = handle
            .receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("second job should run after panicking peer");
        assert_eq!(result, "ok");
    }
}
