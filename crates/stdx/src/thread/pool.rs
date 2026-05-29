use std::{
    marker::PhantomData,
    panic::{self, UnwindSafe},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use crossbeam_channel::{Receiver, Sender};
use crossbeam_utils::sync::WaitGroup;

use crate::thread::{Builder, JoinHandle, ThreadIntent};

pub struct Pool {
    job_sender: Sender<Job>,
    _handles: Box<[JoinHandle]>,
    extant_tasks: Arc<AtomicUsize>,
}

struct Job {
    requested_intent: ThreadIntent,
    f: Box<dyn FnOnce() + Send + UnwindSafe + 'static>,
}

impl Pool {
    #[must_use]
    pub fn new(threads: usize) -> Self {
        const STACK_SIZE: usize = 8 * 1024 * 1024;
        const INITIAL_INTENT: ThreadIntent = ThreadIntent::Worker;

        let (job_sender, job_receiver) = crossbeam_channel::unbounded();
        let extant_tasks = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::with_capacity(threads);
        for idx in 0..threads {
            let handle = Builder::new(INITIAL_INTENT, format!("Worker{idx}",))
                .stack_size(STACK_SIZE)
                .allow_leak(true)
                .spawn({
                    let extant_tasks = Arc::clone(&extant_tasks);
                    let job_receiver: Receiver<Job> = job_receiver.clone();
                    move || {
                        let mut current_intent = INITIAL_INTENT;
                        for job in job_receiver {
                            if job.requested_intent != current_intent {
                                job.requested_intent.apply_to_current_thread();
                                current_intent = job.requested_intent;
                            }
                            drop(panic::catch_unwind(job.f));
                            extant_tasks.fetch_sub(1, Ordering::SeqCst);
                        }
                    }
                })
                .expect("failed to spawn thread");

            handles.push(handle);
        }

        Self { _handles: handles.into_boxed_slice(), extant_tasks, job_sender }
    }

    pub fn spawn<F>(&self, intent: ThreadIntent, f: F)
    where
        F: FnOnce() + Send + UnwindSafe + 'static,
    {
        let f = Box::new(move || {
            if cfg!(debug_assertions) {
                intent.assert_is_used_on_current_thread();
            }
            f();
        });

        let job = Job { requested_intent: intent, f };
        self.extant_tasks.fetch_add(1, Ordering::SeqCst);
        self.job_sender.send(job).unwrap();
    }

    pub fn scoped<'pool, 'scope, F, R>(&'pool self, f: F) -> R
    where
        F: FnOnce(&Scope<'pool, 'scope>) -> R,
    {
        let wg = WaitGroup::new();
        let scope = Scope { pool: self, wg, _marker: PhantomData };
        let r = f(&scope);
        scope.wg.wait();
        r
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.extant_tasks.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct Scope<'pool, 'scope> {
    pool: &'pool Pool,
    wg: WaitGroup,
    _marker: PhantomData<fn(&'scope ()) -> &'scope ()>,
}

impl<'scope> Scope<'_, 'scope> {
    pub fn spawn<F>(&self, intent: ThreadIntent, f: F)
    where
        F: 'scope + FnOnce() + Send + UnwindSafe,
    {
        let wg = self.wg.clone();
        let f = Box::new(move || {
            if cfg!(debug_assertions) {
                intent.assert_is_used_on_current_thread();
            }
            f();
            drop(wg);
        });

        let job = Job {
            requested_intent: intent,
            f: unsafe {
                std::mem::transmute::<
                    Box<dyn 'scope + FnOnce() + Send + UnwindSafe>,
                    Box<dyn 'static + FnOnce() + Send + UnwindSafe>,
                >(f)
            },
        };
        self.pool.extant_tasks.fetch_add(1, Ordering::SeqCst);
        self.pool.job_sender.send(job).unwrap();
    }
}
