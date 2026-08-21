//! The overlay retry driver: the ONE owner of every Embed pass over the workspace overlay,
//! startup included.
//!
//! The rule the driver enforces is an ownership invariant, not a schedule: the right to run
//! a pass is the CURRENT workspace-lease ownership until a live foreign owner is observed,
//! writes happen only under ownership, and a disarm is reserved for what cannot change
//! within this process (including terminal supersession). Transient refusal, a failed pass,
//! an incomplete scan, or an internally superseded overlay plan keeps the obligation alive.

use super::types::{OverlayWarmupState, SemanticRuntimeStatus, SharedSearchEngine};
use crate::workspace_lease::WorkspaceLease;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[cfg(test)]
static RUN_PASS_FINISH_HOOK: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);

/// The driver's own tick: how often it re-checks the condition with no kick arriving. A
/// safety net for a signal that went quiet (a notification lost, a backoff that expired
/// with no drift), not the primary wake-up — kicks are.
const TICK: Duration = Duration::from_secs(30);

/// Backoff schedule for consecutive not-clean outcomes: exponential from one tick, capped.
/// Pure so the schedule is testable apart from the thread.
fn retry_delay(streak: u32) -> Duration {
    const CAP: Duration = Duration::from_secs(30 * 60);
    if streak == 0 {
        return Duration::ZERO;
    }
    TICK.checked_mul(1u32 << streak.min(6).saturating_sub(1)).map(|d| d.min(CAP)).unwrap_or(CAP)
}

struct RetryState {
    /// Bumped by every [`OverlayRetry::kick_fresh`]. A pass captures it before running;
    /// if it moved while the pass ran, the pass's outcome predates a fresh fact (an engine
    /// publication, new drift) and must not arm the backoff over it.
    fresh_epoch: u64,
    /// Consecutive passes that did not end clean; indexes into [`retry_delay`].
    streak: u32,
    /// No pass starts before this instant (backoff gate).
    next_allowed: Instant,
    /// The last outcome demands another pass even if the cache signals read clean —
    /// `Failed` before Phase C leaves no trace in the cache, `Superseded` leaves the
    /// fresher state that may itself be incomplete.
    obligation: bool,
    /// Terminal: nothing this process can retry into existence (no embedder/root, terminal
    /// engine init failure, or an observed live foreign workspace owner).
    disarmed: bool,
}

pub(crate) struct OverlayRetry {
    engine: SharedSearchEngine,
    overlay_warmup: Arc<Mutex<OverlayWarmupState>>,
    semantic_runtime: Arc<Mutex<SemanticRuntimeStatus>>,
    lease: WorkspaceLease,
    state: Mutex<RetryState>,
    wake: Condvar,
    stop: AtomicBool,
    /// Completed pass count — observability and the single-flight proof in tests.
    passes: AtomicUsize,
}

impl OverlayRetry {
    /// Build the driver and start its worker thread. The worker is the ONLY place a pass
    /// runs, so single-flight holds by construction: kicks merely wake it.
    pub(crate) fn spawn(
        engine: SharedSearchEngine,
        overlay_warmup: Arc<Mutex<OverlayWarmupState>>,
        semantic_runtime: Arc<Mutex<SemanticRuntimeStatus>>,
        lease: WorkspaceLease,
    ) -> Arc<Self> {
        let retry = Arc::new(Self {
            engine,
            overlay_warmup,
            semantic_runtime,
            lease,
            state: Mutex::new(RetryState {
                fresh_epoch: 0,
                streak: 0,
                next_allowed: Instant::now(),
                obligation: false,
                disarmed: false,
            }),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
            passes: AtomicUsize::new(0),
        });
        let worker = Arc::clone(&retry);
        std::thread::Builder::new()
            .name("bsl-search-overlay-retry".to_owned())
            .spawn(move || worker.run())
            .ok();
        retry
    }

    /// Fresh drift arrived: whatever kept previous passes incomplete may have changed, so
    /// the backoff resets and the worker wakes.
    pub(crate) fn kick_fresh(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.fresh_epoch = state.fresh_epoch.wrapping_add(1);
            state.streak = 0;
            state.next_allowed = Instant::now();
        }
        self.wake.notify_all();
    }

    /// Terminal disarm: nothing this process can retry into existence.
    pub(crate) fn disarm(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.disarmed = true;
        }
        self.wake.notify_all();
    }

    /// Stop the worker. Called by `SharedState::shutdown` BEFORE the lease is released, so
    /// a scheduled retry cannot publish after the workspace was handed over.
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake.notify_all();
    }

    /// Completed passes so far — the single-flight proof in tests.
    #[cfg(test)]
    pub(crate) fn pass_count(&self) -> usize {
        self.passes.load(Ordering::SeqCst)
    }

    fn run(self: Arc<Self>) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        loop {
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            if state.disarmed {
                state = match self.wake.wait(state) {
                    Ok(state) => state,
                    Err(_) => return,
                };
                continue;
            }
            let now = Instant::now();
            if now < state.next_allowed {
                let delay = state.next_allowed - now;
                let (next, _) = match self.wake.wait_timeout(state, delay) {
                    Ok(pair) => pair,
                    Err(_) => return,
                };
                state = next;
                continue;
            }
            if !self.should_run(state.obligation) {
                if self.lease.is_superseded() {
                    self.disarm_superseded(&mut state);
                    continue;
                }
                let (next, _) = match self.wake.wait_timeout(state, TICK) {
                    Ok(pair) => pair,
                    Err(_) => return,
                };
                state = next;
                continue;
            }
            // Run the pass with the state lock RELEASED: a pass embeds for minutes, and
            // kick/disarm/stop must stay responsive meanwhile.
            let epoch_before = state.fresh_epoch;
            drop(state);
            let outcome = self.run_pass();
            self.passes.fetch_add(1, Ordering::SeqCst);
            state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            self.settle_outcome(&mut state, outcome, epoch_before);
        }
    }

    /// Whether a pass is due: an obligation carried by the driver itself, or any cache
    /// signal. An unpublished engine reads as "due" — the pass classifies it (a transient
    /// `Skipped` before the async init publishes, a backoff-paced retry after).
    fn should_run(&self, obligation: bool) -> bool {
        if !self.lease.owns_caches_now() {
            // A transient UNCLAIMED/lock refusal retries; the caller separately disarms only
            // when this fresh check latched an irreversible foreign-owner observation.
            return false;
        }
        if obligation {
            return true;
        }
        let signals = match self.engine.lock() {
            Ok(guard) => match guard.as_ref() {
                Some(engine) => engine.workspace_overlay_retry_signals().ok(),
                None => None,
            },
            Err(_) => None,
        };
        signals.map(|signals| signals.demands_a_pass()).unwrap_or(true)
    }

    fn run_pass(&self) -> OverlayWarmupState {
        // The syncing status is shown only for a pass that can actually reach the engine;
        // flipping it while the engine is absent would mask a terminal init `Failed`.
        let engine_present =
            self.engine.lock().map(|guard| guard.as_ref().is_some()).unwrap_or(false);
        if engine_present && self.lease.is_superseded() {
            super::SharedState::set_semantic_runtime_status(
                &self.semantic_runtime,
                SemanticRuntimeStatus::Failed(
                    "workspace cache ownership was superseded; reconnect to use the new daemon"
                        .to_owned(),
                ),
            );
        } else if engine_present {
            super::SharedState::set_semantic_runtime_status(
                &self.semantic_runtime,
                SemanticRuntimeStatus::OverlaySyncing,
            );
        }
        let stop = &self.stop;
        super::SharedState::run_overlay_warmup(
            &self.engine,
            &self.overlay_warmup,
            &self.lease,
            &|| !stop.load(Ordering::SeqCst),
        );
        let outcome = self
            .overlay_warmup
            .lock()
            .map(|state| state.clone())
            .unwrap_or(OverlayWarmupState::Pending);
        #[cfg(test)]
        if let Some(hook) = RUN_PASS_FINISH_HOOK.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
        {
            hook();
        }
        if engine_present && self.lease.is_superseded() {
            super::SharedState::set_semantic_runtime_status(
                &self.semantic_runtime,
                SemanticRuntimeStatus::Failed(
                    "workspace cache ownership was superseded; reconnect to use the new daemon"
                        .to_owned(),
                ),
            );
        } else if engine_present {
            super::SharedState::set_semantic_runtime_status(
                &self.semantic_runtime,
                SemanticRuntimeStatus::Ready,
            );
        }
        outcome
    }

    fn settle_outcome(
        &self,
        state: &mut RetryState,
        outcome: OverlayWarmupState,
        epoch_before: u64,
    ) {
        if self.lease.is_superseded() {
            self.disarm_superseded(state);
            return;
        }
        match outcome {
            OverlayWarmupState::NoLocalDiffs | OverlayWarmupState::Synced { .. } => {
                state.streak = 0;
                state.obligation = false;
                state.next_allowed = Instant::now();
            }
            OverlayWarmupState::Incomplete { .. }
            | OverlayWarmupState::Superseded
            | OverlayWarmupState::Failed(_) => {
                state.streak = state.streak.saturating_add(1);
                state.obligation = true;
                state.next_allowed = Instant::now() + retry_delay(state.streak);
            }
            OverlayWarmupState::Skipped(reason) => {
                if reason.contains("engine unavailable") || reason.contains("lock error") {
                    // Transient: the engine publishes asynchronously and a kick from the
                    // armed watcher can outrun it. The obligation stands.
                    state.streak = state.streak.saturating_add(1);
                    state.obligation = true;
                    state.next_allowed = Instant::now() + retry_delay(state.streak);
                } else {
                    // No embedder / no workspace root: nothing to retry into existence.
                    state.disarmed = true;
                }
            }
            OverlayWarmupState::Pending => {
                state.streak = state.streak.saturating_add(1);
                state.obligation = true;
                state.next_allowed = Instant::now() + retry_delay(state.streak);
            }
        }
        // A fresh kick landed WHILE the pass ran: its notification found no waiter, and the
        // arming above would bury it under a backoff. The outcome predates the fresh fact,
        // so the next check happens immediately.
        if state.fresh_epoch != epoch_before {
            state.streak = 0;
            state.next_allowed = Instant::now();
        }
    }

    fn disarm_superseded(&self, state: &mut RetryState) {
        state.disarmed = true;
        state.obligation = false;
        super::SharedState::set_semantic_runtime_status(
            &self.semantic_runtime,
            SemanticRuntimeStatus::Failed(
                "workspace cache ownership was superseded; reconnect to use the new daemon"
                    .to_owned(),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{
        mock_embedding_env, mock_semantic_config, spawn_mock_embedding_server, ENV_LOCK,
    };
    use super::*;
    use bsl_search::SearchEngine;
    use tempfile::tempdir;

    /// The schedule itself: no delay after a clean pass, exponential growth from one tick,
    /// a hard cap — apart from the thread, so the shape is provable.
    #[test]
    fn the_retry_delay_grows_exponentially_to_a_cap() {
        assert_eq!(retry_delay(0), Duration::ZERO);
        assert_eq!(retry_delay(1), Duration::from_secs(30));
        assert_eq!(retry_delay(2), Duration::from_secs(60));
        assert_eq!(retry_delay(3), Duration::from_secs(120));
        assert_eq!(retry_delay(6), Duration::from_secs(960));
        assert_eq!(retry_delay(7), Duration::from_secs(960), "the exponent is clamped");
        assert_eq!(retry_delay(u32::MAX), Duration::from_secs(960));
    }

    fn state() -> RetryState {
        RetryState {
            fresh_epoch: 0,
            streak: 0,
            next_allowed: Instant::now(),
            obligation: false,
            disarmed: false,
        }
    }

    fn driver_over(engine: SharedSearchEngine, lease: WorkspaceLease) -> Arc<OverlayRetry> {
        OverlayRetry::spawn(
            engine,
            Arc::new(Mutex::new(OverlayWarmupState::Pending)),
            Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            lease,
        )
    }

    fn wait_for(deadline_ms: u64, mut check: impl FnMut() -> bool) -> bool {
        for _ in 0..(deadline_ms / 10) {
            if check() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        check()
    }

    /// The outcome classification: clean resets, not-clean (including a failed persist and a
    /// superseded publish) grows the streak and keeps the obligation, a TRANSIENT skip (the
    /// engine has not published yet) retries, a TERMINAL skip (no embedder, no root) disarms.
    #[test]
    fn outcomes_classify_into_retry_reset_and_disarm() {
        let dummy = driver_over(
            Arc::new(Mutex::new(None)),
            crate::workspace_lease::WorkspaceLease::unmanaged(),
        );
        dummy.stop();

        let mut s = state();
        s.streak = 3;
        s.obligation = true;
        dummy.settle_outcome(&mut s, OverlayWarmupState::NoLocalDiffs, 0);
        assert_eq!((s.streak, s.obligation, s.disarmed), (0, false, false));

        let mut s = state();
        dummy.settle_outcome(
            &mut s,
            OverlayWarmupState::Incomplete {
                unreadable: 0,
                canonical_fallbacks: 0,
                read_failures: 0,
                persist_failed: true,
            },
            0,
        );
        assert_eq!((s.streak, s.obligation, s.disarmed), (1, true, false));

        let mut s = state();
        dummy.settle_outcome(&mut s, OverlayWarmupState::Superseded, 0);
        assert_eq!((s.streak, s.obligation, s.disarmed), (1, true, false));

        let mut s = state();
        dummy.settle_outcome(&mut s, OverlayWarmupState::Failed("x".to_owned()), 0);
        assert_eq!((s.streak, s.obligation, s.disarmed), (1, true, false));

        let mut s = state();
        dummy.settle_outcome(
            &mut s,
            OverlayWarmupState::Skipped("engine unavailable".to_owned()),
            0,
        );
        assert_eq!((s.streak, s.obligation, s.disarmed), (1, true, false), "transient skip");

        let mut s = state();
        dummy.settle_outcome(
            &mut s,
            OverlayWarmupState::Skipped("no embedder configured".to_owned()),
            0,
        );
        assert!(s.disarmed, "terminal skip disarms");
    }

    /// A fresh kick that lands WHILE a pass runs must not be buried by that pass's stale
    /// outcome: the settle sees the epoch moved and schedules the next check immediately.
    #[test]
    fn a_fresh_kick_during_a_pass_overrides_the_stale_backoff() {
        let dummy = driver_over(
            Arc::new(Mutex::new(None)),
            crate::workspace_lease::WorkspaceLease::unmanaged(),
        );
        dummy.stop();
        let mut s = state();
        // The pass captured epoch 4; a kick_fresh bumped it to 5 mid-pass.
        s.fresh_epoch = 5;
        dummy.settle_outcome(
            &mut s,
            OverlayWarmupState::Skipped("engine unavailable".to_owned()),
            4,
        );
        assert_eq!(s.streak, 0, "the stale outcome must not bury the fresh fact");
        assert!(s.next_allowed <= Instant::now(), "the next check happens now");
        assert!(s.obligation, "the obligation itself stands");
    }

    /// Concurrent kicks collapse into ONE pass: the worker is the only runner, and after a
    /// clean first pass the condition goes quiet, so no kick storm re-runs it.
    #[test]
    fn concurrent_kicks_collapse_into_one_pass() {
        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mock = spawn_mock_embedding_server(vec![1.0, 0.0, 0.0]);
        let _env = mock_embedding_env(&mock);
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let mut engine =
            SearchEngine::new(&workspace.join("search.db"), mock_semantic_config(&mock)).unwrap();
        let (roots, _) = bsl_search::WorkspaceRoots::build(workspace, workspace, &[]);
        engine.set_workspace_roots(roots);
        let engine_arc: SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let retry = driver_over(engine_arc, crate::workspace_lease::WorkspaceLease::unmanaged());
        let kickers: Vec<_> = (0..8)
            .map(|_| {
                let retry = Arc::clone(&retry);
                std::thread::spawn(move || retry.kick_fresh())
            })
            .collect();
        for kicker in kickers {
            kicker.join().unwrap();
        }
        assert!(wait_for(5_000, || retry.pass_count() >= 1), "the first pass runs");
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(retry.pass_count(), 1, "a kick storm still yields one pass");
        retry.stop();
    }

    /// A pass before the engine publishes is a TRANSIENT skip: the obligation survives, and
    /// once the engine appears the next kick completes the startup pass — it was never lost.
    #[test]
    fn a_transient_engine_absence_keeps_the_obligation() {
        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mock = spawn_mock_embedding_server(vec![1.0, 0.0, 0.0]);
        let _env = mock_embedding_env(&mock);
        let engine_arc: SharedSearchEngine = Arc::new(Mutex::new(None));
        let retry = driver_over(
            Arc::clone(&engine_arc),
            crate::workspace_lease::WorkspaceLease::unmanaged(),
        );
        retry.kick_fresh();
        assert!(wait_for(5_000, || retry.pass_count() >= 1), "the blind pass runs");

        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let mut engine =
            SearchEngine::new(&workspace.join("search.db"), mock_semantic_config(&mock)).unwrap();
        let (roots, _) = bsl_search::WorkspaceRoots::build(workspace, workspace, &[]);
        engine.set_workspace_roots(roots);
        *engine_arc.lock().unwrap() = Some(engine);
        retry.kick_fresh();
        assert!(
            wait_for(5_000, || {
                engine_arc
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|engine| {
                        engine
                            .workspace_overlay_retry_signals()
                            .map(|signals| signals.initialized)
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
            }),
            "the startup pass completes once the engine appears"
        );
        retry.stop();
    }

    /// `stop()` freezes the driver before the lease handover: no pass starts after it.
    #[test]
    fn stop_freezes_the_driver() {
        let retry = driver_over(
            Arc::new(Mutex::new(None)),
            crate::workspace_lease::WorkspaceLease::unmanaged(),
        );
        retry.stop();
        retry.kick_fresh();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(retry.pass_count(), 0, "a stopped driver runs nothing");
    }

    #[test]
    fn temporary_lock_refusal_remains_retryable() {
        let dir = tempdir().unwrap();
        let cache = crate::cache::WorkspaceCacheLayout::for_workspace(dir.path());
        let (retry, lease) =
            crate::workspace_lease::WorkspaceLease::while_cache_lock_held(&cache, || {
                let lease = crate::workspace_lease::WorkspaceLease::claim_cache(&cache);
                let retry = driver_over(Arc::new(Mutex::new(None)), lease.clone());
                retry.kick_fresh();
                std::thread::sleep(Duration::from_millis(100));
                assert_eq!(retry.pass_count(), 0, "no pass starts while ownership is unavailable");
                assert!(!lease.is_superseded(), "lock contention is not terminal");
                (retry, lease)
            });

        retry.kick_fresh();
        assert!(wait_for(5_000, || retry.pass_count() >= 1), "the unclaimed lease retries");
        assert!(!lease.is_superseded());
        assert!(!retry.state.lock().unwrap().disarmed);
        retry.stop();
    }

    /// Observing a live foreign owner is terminal: releasing that owner and kicking again
    /// cannot start another pass or report the old daemon semantically ready.
    #[test]
    fn observed_supersession_never_resumes() {
        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mock = spawn_mock_embedding_server(vec![1.0, 0.0, 0.0]);
        let _env = mock_embedding_env(&mock);
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let mut engine =
            SearchEngine::new(&workspace.join("search.db"), mock_semantic_config(&mock)).unwrap();
        let (roots, _) = bsl_search::WorkspaceRoots::build(workspace, workspace, &[]);
        engine.set_workspace_roots(roots);
        let engine_arc: SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let mine = crate::workspace_lease::WorkspaceLease::claim(workspace);
        let runtime = Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled));
        let retry = OverlayRetry::spawn(
            Arc::clone(&engine_arc),
            Arc::new(Mutex::new(OverlayWarmupState::Pending)),
            Arc::clone(&runtime),
            mine.clone(),
        );
        assert!(wait_for(5_000, || retry.pass_count() >= 1), "the startup pass runs");

        // A newer daemon takes the workspace; a pending signal appears meanwhile.
        let newer = crate::workspace_lease::WorkspaceLease::claim(workspace);
        std::fs::write(workspace.join("New.bsl"), "Процедура Новая()\nКонецПроцедуры").unwrap();
        engine_arc
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .mark_workspace_path_dirty(workspace.join("New.bsl"))
            .unwrap();
        let before = retry.pass_count();
        retry.kick_fresh();
        assert!(wait_for(5_000, || mine.is_superseded()), "the fresh check observes takeover");
        assert_eq!(retry.pass_count(), before, "a superseded daemon runs no pass");

        // The newer daemon exits cleanly; the old process remains terminally read-only.
        newer.release();
        retry.kick_fresh();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(retry.pass_count(), before, "owner release cannot re-arm a terminal driver");
        assert!(matches!(*runtime.lock().unwrap(), SemanticRuntimeStatus::Failed(_)));
        retry.stop();
    }

    #[test]
    fn takeover_during_a_pass_never_reports_ready() {
        struct ResetHook;
        impl Drop for ResetHook {
            fn drop(&mut self) {
                *RUN_PASS_FINISH_HOOK.lock().unwrap_or_else(|p| p.into_inner()) = None;
            }
        }
        let _reset = ResetHook;
        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mock = spawn_mock_embedding_server(vec![1.0, 0.0, 0.0]);
        let _env = mock_embedding_env(&mock);
        let dir = tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        let mut engine =
            SearchEngine::new(&workspace.join("search.db"), mock_semantic_config(&mock)).unwrap();
        let (roots, _) = bsl_search::WorkspaceRoots::build(&workspace, &workspace, &[]);
        engine.set_workspace_roots(roots);
        let mine = crate::workspace_lease::WorkspaceLease::claim(&workspace);
        let mine_in_hook = mine.clone();
        let newer = Arc::new(Mutex::new(None));
        let newer_in_hook = Arc::clone(&newer);
        *RUN_PASS_FINISH_HOOK.lock().unwrap() = Some(Box::new(move || {
            *newer_in_hook.lock().unwrap() =
                Some(crate::workspace_lease::WorkspaceLease::claim(&workspace));
            assert!(!mine_in_hook.owns_caches_now());
        }));
        let runtime = Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled));
        let retry = OverlayRetry::spawn(
            Arc::new(Mutex::new(Some(engine))),
            Arc::new(Mutex::new(OverlayWarmupState::Pending)),
            Arc::clone(&runtime),
            mine.clone(),
        );
        assert!(wait_for(5_000, || mine.is_superseded()));
        assert!(matches!(*runtime.lock().unwrap(), SemanticRuntimeStatus::Failed(_)));
        assert!(newer.lock().unwrap().is_some());
        retry.stop();
    }

    /// Phase C runs under the REAL fence: with the lease taken over between the phases, the
    /// publish is refused and nothing lands — fingerprint rows would otherwise suppress the
    /// new owner's re-reads after a restart.
    #[test]
    fn a_lost_lease_refuses_the_publish_fence() {
        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mock = spawn_mock_embedding_server(vec![1.0, 0.0, 0.0]);
        let _env = mock_embedding_env(&mock);
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let mut engine =
            SearchEngine::new(&workspace.join("search.db"), mock_semantic_config(&mock)).unwrap();
        let (roots, _) = bsl_search::WorkspaceRoots::build(workspace, workspace, &[]);
        engine.set_workspace_roots(roots);
        let engine_arc: SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));
        let overlay_warmup = Arc::new(Mutex::new(OverlayWarmupState::Pending));

        let mine = crate::workspace_lease::WorkspaceLease::claim(workspace);
        let _newer = crate::workspace_lease::WorkspaceLease::claim(workspace);
        super::super::SharedState::run_overlay_warmup(&engine_arc, &overlay_warmup, &mine, &|| {
            true
        });
        let outcome = overlay_warmup.lock().unwrap().clone();
        assert!(
            matches!(&outcome, OverlayWarmupState::Failed(reason) if reason.contains("ownership")),
            "the fenced publish refuses a superseded daemon, got {outcome:?}"
        );
        let initialized = engine_arc
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .workspace_overlay_retry_signals()
            .unwrap()
            .initialized;
        assert!(!initialized, "nothing was published");
    }

    /// The end-to-end catch-up: an incomplete pass (an unreadable subtree) keeps the
    /// obligation, and once the subtree is restored the driver's next pass settles clean —
    /// no restart required.
    #[cfg(unix)]
    #[test]
    fn an_incomplete_pass_is_caught_up_after_the_subtree_returns() {
        use std::os::unix::fs::PermissionsExt;
        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mock = spawn_mock_embedding_server(vec![1.0, 0.0, 0.0]);
        let _env = mock_embedding_env(&mock);
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let closed = workspace.join("closed");
        std::fs::create_dir(&closed).unwrap();
        std::fs::write(closed.join("Hidden.bsl"), "Процедура Скрытая()\nКонецПроцедуры").unwrap();
        let mut engine =
            SearchEngine::new(&workspace.join("search.db"), mock_semantic_config(&mock)).unwrap();
        let (roots, _) = bsl_search::WorkspaceRoots::build(workspace, workspace, &[]);
        engine.set_workspace_roots(roots);
        let engine_arc: SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&closed).is_ok() {
            // Running as root: permissions cannot hide anything.
            std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        let overlay_warmup = Arc::new(Mutex::new(OverlayWarmupState::Pending));
        let retry = OverlayRetry::spawn(
            Arc::clone(&engine_arc),
            Arc::clone(&overlay_warmup),
            Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            crate::workspace_lease::WorkspaceLease::unmanaged(),
        );
        assert!(wait_for(5_000, || retry.pass_count() >= 1), "the incomplete pass runs");
        assert!(
            matches!(overlay_warmup.lock().unwrap().clone(), OverlayWarmupState::Incomplete { .. }),
            "the first pass is honest about its coverage"
        );

        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).unwrap();
        retry.kick_fresh();
        assert!(
            wait_for(10_000, || {
                matches!(overlay_warmup.lock().unwrap().clone(), OverlayWarmupState::Synced { .. })
            }),
            "the returned subtree is caught up without a restart, got {:?}",
            overlay_warmup.lock().unwrap().clone()
        );
        let rescan = engine_arc
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .workspace_overlay_needs_full_rescan()
            .unwrap();
        assert!(!rescan, "the clean pass settles the withheld removals");
        retry.stop();
    }

    /// The semantic side has its own durable signal: an entry the ReuseOnly path built
    /// lexical-only (its mark consumed) still drives a pass that attaches the vectors.
    #[test]
    fn a_lexical_only_entry_drives_a_vector_catch_up_pass() {
        let _lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let mock = spawn_mock_embedding_server(vec![1.0, 0.0, 0.0]);
        let _env = mock_embedding_env(&mock);
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let mut engine =
            SearchEngine::new(&workspace.join("search.db"), mock_semantic_config(&mock)).unwrap();
        let (roots, _) = bsl_search::WorkspaceRoots::build(workspace, workspace, &[]);
        engine.set_workspace_roots(roots);
        engine.enable_workspace_watcher_mode();
        let engine_arc: SharedSearchEngine = Arc::new(Mutex::new(Some(engine)));

        let overlay_warmup = Arc::new(Mutex::new(OverlayWarmupState::Pending));
        let retry = OverlayRetry::spawn(
            Arc::clone(&engine_arc),
            Arc::clone(&overlay_warmup),
            Arc::new(Mutex::new(SemanticRuntimeStatus::Disabled)),
            crate::workspace_lease::WorkspaceLease::unmanaged(),
        );
        assert!(wait_for(5_000, || retry.pass_count() >= 1), "the startup pass runs");

        // An edit lands; the interactive ReuseOnly path consumes the mark WITHOUT vectors.
        std::fs::write(workspace.join("New.bsl"), "Процедура Новая()\nКонецПроцедуры").unwrap();
        {
            let guard = engine_arc.lock().unwrap();
            let engine = guard.as_ref().unwrap();
            engine.mark_workspace_path_dirty(workspace.join("New.bsl")).unwrap();
            engine.workspace_overlay_stats().unwrap();
            let signals = engine.workspace_overlay_retry_signals().unwrap();
            assert_eq!(signals.pending_dirty_paths, 0, "the ReuseOnly path consumed the mark");
            assert_eq!(signals.unembedded_entries, 1, "the entry serves lexical-only");
        }
        retry.kick_fresh();
        assert!(
            wait_for(10_000, || {
                engine_arc
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .workspace_overlay_retry_signals()
                    .unwrap()
                    .unembedded_entries
                    == 0
            }),
            "the driver's pass attaches the missing vectors"
        );
        retry.stop();
    }
}
