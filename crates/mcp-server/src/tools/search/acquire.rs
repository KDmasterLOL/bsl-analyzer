use super::AcquireFailure;
use bsl_search::SearchEngine;
use rmcp::ErrorData as McpError;
use std::sync::{Arc, Mutex, MutexGuard};

/// A poisoned engine lock means a prior operation panicked mid-search; the engine state may be
/// inconsistent and retrying is futile, so this is a hard internal error rather than the
/// "warming up / try again" advice a transient state would warrant.
pub(super) fn engine_lock_poisoned_error() -> McpError {
    McpError::internal_error(
        "search engine lock is poisoned (a prior operation panicked); restart the MCP server"
            .to_owned(),
        None,
    )
}

/// Acquire the engine guard, *blocking* (queueing) on contention instead of bailing out.
///
/// The engine owns a `!Sync` rusqlite connection, so every search must serialize on this lock
/// — that serialization is mandatory, not a coarseness to widen away (see
/// [`crate::state::SharedSearchEngine`]). What this MUST NOT do is surface ordinary contention
/// as a failure: an overlay prime, or a peer search inside its (now tightly bounded) embedding
/// round-trip, holds the lock for seconds, and a short `try_lock` budget turned that into a
/// misleading "overlay warming up" for every other `search_code` in a concurrent batch. So we
/// wait for the lock and return real results once it frees. Polling (rather than parking) keeps
/// the brief sleeps on the `spawn_blocking` thread without pulling in a timed-lock dependency.
pub(super) fn try_acquire_engine(
    engine: &Arc<Mutex<Option<SearchEngine>>>,
) -> Result<MutexGuard<'_, Option<SearchEngine>>, AcquireFailure> {
    // Bounds a pathological hang (a deadlock bug, a never-returning holder) without ever
    // tripping on the ordinary multi-second holds — an overlay prime or a slow embed. The query
    // embed under the lock is itself capped (see `Embedder::INTERACTIVE_TIMEOUT`), so this cap
    // only ever fires on a real stall, never on a routine concurrent search.
    const MAX_WAIT: std::time::Duration = std::time::Duration::from_secs(30);
    const POLL: std::time::Duration = std::time::Duration::from_millis(25);
    acquire_engine_within(engine, MAX_WAIT, POLL)
}

/// The acquire loop, parameterized over the wait budget so tests can exercise the timeout path
/// without a 30-second sleep. Production callers go through [`try_acquire_engine`].
pub(super) fn acquire_engine_within<'a>(
    engine: &'a Arc<Mutex<Option<SearchEngine>>>,
    max_wait: std::time::Duration,
    poll: std::time::Duration,
) -> Result<MutexGuard<'a, Option<SearchEngine>>, AcquireFailure> {
    let start = std::time::Instant::now();
    loop {
        match engine.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(std::sync::TryLockError::Poisoned(_)) => return Err(AcquireFailure::Poisoned),
            Err(std::sync::TryLockError::WouldBlock) => {
                if start.elapsed() >= max_wait {
                    return Err(AcquireFailure::TimedOut);
                }
                std::thread::sleep(poll);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{acquire_engine_within, try_acquire_engine, AcquireFailure};
    use bsl_search::SearchEngine;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier, Mutex};
    use std::time::{Duration, Instant};

    #[test]
    fn try_acquire_engine_queues_until_the_lock_frees() {
        let engine: Arc<Mutex<Option<SearchEngine>>> = Arc::new(Mutex::new(None));
        assert!(try_acquire_engine(&engine).is_ok());

        const HOLD: Duration = Duration::from_millis(300);
        let held = engine.lock().unwrap();
        let gate = Arc::new(Barrier::new(2));
        let entered = Arc::new(AtomicBool::new(false));
        let probe = {
            let engine = Arc::clone(&engine);
            let gate = Arc::clone(&gate);
            let entered = Arc::clone(&entered);
            std::thread::spawn(move || {
                gate.wait();
                entered.store(true, Ordering::SeqCst);
                let started = Instant::now();
                let acquired = try_acquire_engine(&engine).is_ok();
                (acquired, started.elapsed())
            })
        };
        gate.wait();
        std::thread::sleep(HOLD);
        assert!(entered.load(Ordering::SeqCst), "probe must reach the acquire under contention");
        drop(held);
        let (acquired, waited) = probe.join().unwrap();

        assert!(acquired, "acquire must succeed once the lock frees");
        assert!(waited >= HOLD / 2, "acquire returned too fast to have queued: {waited:?}");
    }

    #[test]
    fn acquire_engine_times_out_when_the_lock_stays_held() {
        let engine: Arc<Mutex<Option<SearchEngine>>> = Arc::new(Mutex::new(None));
        let held = engine.lock().unwrap();
        let cap = Duration::from_millis(150);
        let started = Instant::now();
        let outcome = acquire_engine_within(&engine, cap, Duration::from_millis(10));
        let waited = started.elapsed();
        drop(held);

        assert!(matches!(outcome, Err(AcquireFailure::TimedOut)));
        assert!(waited >= cap, "must wait out the cap before giving up: {waited:?}");
    }

    #[test]
    fn acquire_engine_reports_poison_immediately() {
        let engine: Arc<Mutex<Option<SearchEngine>>> = Arc::new(Mutex::new(None));
        let poisoner = {
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || {
                let _held = engine.lock().unwrap();
                panic!("poison the engine lock");
            })
        };
        assert!(poisoner.join().is_err());
        let started = Instant::now();
        let outcome =
            acquire_engine_within(&engine, Duration::from_secs(30), Duration::from_millis(10));

        assert!(matches!(outcome, Err(AcquireFailure::Poisoned)));
        assert!(started.elapsed() < Duration::from_secs(1), "poison must not block on the cap");
    }
}
