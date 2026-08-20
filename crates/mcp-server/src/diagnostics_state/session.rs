//! The one way a tool reads the resident analysis database.
//!
//! Every resident-backed answer is computed on a blocking thread, under the resident
//! mutex, and may take seconds on a large configuration. Three properties have to hold
//! for all of them, and holding them per tool is how they drift apart:
//!
//! - the answer runs on a PER-REQUEST database handle, so its Salsa cancellation token
//!   can be cancelled without touching the resident's master handle or any concurrent
//!   call (a handle clone carries a token of its own);
//! - that handle dies before the read returns: a live clone blocks every write to the
//!   database, and the incremental drift apply is a write;
//! - a cancelled call answers as a cancelled call, a call cut short by a writer answers
//!   as a retry, and a panic answers as a panic — three different things that all arrive
//!   as an unwind.
//!
//! [`resident_call`] owns all three. Tools describe what to compute and how to render
//! each outcome; they do not decide when to observe cancellation, because a decision
//! written in a tool can only be verified in that tool.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use super::lifecycle::DiagnosticsState;
use super::resident::DiagnosticsResident;
use super::types::ResidentOutcome;
use crate::cancel::{join_unless_cancelled, RequestCancel};

/// How a resident call ended, before any tool-specific rendering.
pub(crate) enum CallOutcome<T> {
    /// The call ran to completion; `T` is whatever the tool computed.
    Ready(T),
    /// The client sent `notifications/cancelled` (or the transport went away).
    Cancelled,
    /// A writer moved the database out from under the call. Nobody cancelled
    /// anything: the caller is owed a retry, not a report of its own cancellation.
    Superseded,
    /// The work panicked. Not cancellation, and never reported as one.
    Panicked,
}

/// A resident-reading call in progress. Handed to the tool's body so it can read the
/// resident as many times as its answer needs — all within one blocking task, one
/// cancellation registry, and one unwind boundary.
pub(crate) struct ResidentSession {
    diag: DiagnosticsState,
    cancel: Arc<RequestCancel>,
}

impl ResidentSession {
    /// One read of the resident, on a database handle owned by this request.
    ///
    /// The handle is cloned and its token registered inside the resident lock, and it
    /// is dropped before the lock is released — including when the body unwinds. The
    /// `unwind_if_revision_cancelled` checkpoints the analysis layers already carry
    /// observe this request's cancellation through that token.
    pub(crate) fn read<T>(
        &self,
        f: impl FnOnce(&DiagnosticsResident, &ide::Analysis, u64) -> T,
    ) -> ResidentOutcome<T> {
        // Before `diag.read`, because `read` polls for drift FIRST — and with a forced
        // scan or a degraded change hub that poll stats the whole tree. Registering the
        // salsa token inside the closure protects the queries and nothing before them,
        // so a session cancelled by now would still pay for that walk.
        if self.cancel.is_cancelled() {
            std::panic::resume_unwind(Box::new(salsa::Cancelled::Local));
        }
        self.diag.read(|resident, generation| {
            // The clone is a local: it cannot outlive this closure, and an unwind
            // through it drops it just the same. A clone that escaped would park the
            // next `set_file_text_source` on salsa's `while *clones != 1`.
            // The handle lives INSIDE the `Analysis`, so it does not move for the rest
            // of the read: `attach` remembers a raw pointer to the database, and a
            // handle that moved (or a second clone made downstream) would be a
            // different database to salsa — «Cannot change database mid-query».
            let analysis = ide::Analysis::from_database(resident.db().clone());
            let db = analysis.database();
            self.cancel.register(salsa::Database::cancellation_token(db));
            // Attached for the WHOLE body, and this is what makes the token durable.
            // Salsa resets a handle's local token when the OUTERMOST attach scope
            // exits, and every query called from plain code is its own outermost
            // scope: a cancel arriving while one of them runs is wiped on its way
            // out, and the next file-boundary checkpoint reads a clear token. Holding
            // the outermost scope here makes every query inside a nested one, so the
            // cancel survives from the moment it arrives until a checkpoint sees it.
            salsa::Database::attach(db, |_| f(resident, &analysis, generation))
        })
    }

    /// A read on an EMPTY database this request owns, for the answer a tool still
    /// serves when the resident is not there to answer it — the platform surface is in
    /// every handle, resident or not.
    ///
    /// It goes through the same door for the same reason: a handle built outside it
    /// carries a token nobody cancelled, so the `unwind_if_revision_cancelled`
    /// checkpoints on the way through would read a clear token and the abandoned work
    /// would run to the end of the platform catalogue for a response nobody reads.
    ///
    /// Not to be called from inside [`read`](Self::read): salsa allows one database per
    /// thread inside an attach scope and panics on a second.
    pub(crate) fn read_detached<T>(&self, f: impl FnOnce(&ide::Analysis) -> T) -> T {
        if self.cancel.is_cancelled() {
            std::panic::resume_unwind(Box::new(salsa::Cancelled::Local));
        }
        let analysis = ide::Analysis::new();
        let db = analysis.database();
        self.cancel.register(salsa::Database::cancellation_token(db));
        salsa::Database::attach(db, |_| f(&analysis))
    }

    /// Cheap check for loops between salsa queries, where there is nothing to unwind
    /// from and no query boundary to observe the token at.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// The request's cancellation registry, for work that fans out to its own database
    /// handles — the rayon sweep clones one per worker, and each registers here so a
    /// single cancel reaches them all.
    pub(crate) fn cancel(&self) -> &Arc<RequestCancel> {
        &self.cancel
    }

    /// Ask for a drift re-scan before the next read (storm-guarded by the state).
    pub(crate) fn force_rescan(&self) {
        self.diag.force_rescan();
    }

    /// The lifecycle report, for rendering a `loading` envelope.
    pub(crate) fn status_report(&self) -> super::types::StatusReport {
        self.diag.status_report()
    }
}

/// Run one resident-reading call under the rmcp per-request cancellation token.
///
/// The body runs on a blocking thread. When the token fires, this returns
/// [`CallOutcome::Cancelled`] immediately WITHOUT waiting for that thread: it may still
/// be queued behind another call on the resident mutex, and once it runs it unwinds at
/// its first salsa checkpoint and releases the mutex on its own.
pub(crate) async fn resident_call<T, F>(
    diag: DiagnosticsState,
    ct: tokio_util::sync::CancellationToken,
    body: F,
) -> CallOutcome<T>
where
    F: FnOnce(&ResidentSession) -> T + Send + 'static,
    T: Send + 'static,
{
    let cancel = Arc::new(RequestCancel::default());
    let session = ResidentSession { diag, cancel: Arc::clone(&cancel) };

    let join = tokio::task::spawn_blocking(move || {
        match salsa::Cancelled::catch(AssertUnwindSafe(|| body(&session))) {
            Ok(value) => CallOutcome::Ready(value),
            // This request's own token. Everything else that arrives as an unwind is
            // somebody else's event and must not be dressed up as the client's cancel.
            Err(salsa::Cancelled::Local) => CallOutcome::Cancelled,
            Err(salsa::Cancelled::PendingWrite) => CallOutcome::Superseded,
            Err(other) => {
                tracing::error!(?other, "resident call unwound on a salsa variant it cannot name");
                CallOutcome::Panicked
            }
        }
    });

    match join_unless_cancelled(ct, cancel, join).await {
        // Per the MCP cancellation spec the client ignores any response after its
        // `notifications/cancelled`, so there is nothing to wait for and nothing to
        // publish; the detached body unwinds and logs on its own.
        None => CallOutcome::Cancelled,
        Some(Ok(outcome)) => outcome,
        Some(Err(error)) => {
            // A real panic, not cancellation. The caller answers with a fixed sentence
            // (the payload is not the client's business), so the payload has to be
            // recorded HERE or it is lost: `JoinError`'s own text carries the panic
            // message and the thread it died on, and without this line debugging a
            // genuine bug in a resident tool is harder than in one that never moved
            // to this path.
            tracing::error!(%error, "resident call panicked");
            CallOutcome::Panicked
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics_state::test_support::{write, write_common_module};
    use crate::walk_probe::{await_walk_start, entered, install, reset, WALK_GATE};
    use std::time::{Duration, Instant};

    /// Callers of one popular name. Measured, not assigned: on this stand an
    /// uncancelled walk takes ~3.5 s in a debug build (200 callers took 0.35 s — far too
    /// short for a cancel to land mid-walk, and any gate built on it is green whatever
    /// the code does), while the resident itself builds in ~80 ms.
    const CALLERS: usize = 1000;

    const DECLARED: &str = "Объявление.ПриИзмененииПоля";

    fn stand(root: &std::path::Path) {
        write_common_module(
            root,
            "Объявление",
            true,
            "&НаСервере\nПроцедура ПриИзмененииПоля() Экспорт\nКонецПроцедуры\n",
        );
        for i in 0..CALLERS {
            write_common_module(
                root,
                &format!("Вызов{i:04}"),
                true,
                "&НаСервере\nПроцедура Тело() Экспорт\n    \
                 Объявление.ПриИзмененииПоля();\nКонецПроцедуры\n",
            );
        }
        write(root, "bsl-analyzer.toml", "[source]\nroot = \".\"\n");
    }

    fn ready_state(root: &std::path::Path) -> DiagnosticsState {
        let mut state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.drift_interval = Duration::from_millis(0);
        state.ensure_loading();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            match state.status() {
                super::super::types::DiagnosticsStatus::Ready { .. } => return state,
                super::super::types::DiagnosticsStatus::Failed(msg) => panic!("resident: {msg}"),
                other => {
                    assert!(Instant::now() < deadline, "resident never became ready: {other:?}");
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    /// The whole `references` answer for the popular name — anchor, walk and render.
    fn walk(session: &ResidentSession) -> ResidentOutcome<bool> {
        session.read(|resident, db, _| {
            let params = crate::tools::references::Params {
                symbol: Some(DECLARED),
                anchor_root_id: None,
                root_id: None,
                path: None,
                line: None,
                column: None,
                line_content: None,
                area_root_id: None,
                area_path_prefix: None,
                kinds: &[],
                include_declaration: Some(true),
                limit: None,
                max_files: None,
                include_preview: None,
            };
            crate::tools::references::answer(resident, db.database(), &params, 6000).is_ok()
        })
    }

    /// How long a second resident call waits behind a first one, with and without a
    /// cancel. The cancelled call must stop holding the resident; the uncancelled one is
    /// the positive control that proves the stand is big enough for the difference to
    /// exist at all.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_cancelled_call_frees_the_resident_slot() {
        let _serialised = WALK_GATE.lock().await;
        install();
        let dir = tempfile::tempdir().unwrap();
        stand(dir.path());

        let neighbour = |state: DiagnosticsState| async move {
            let started = Instant::now();
            let out = resident_call(state, tokio_util::sync::CancellationToken::new(), |s| {
                s.read(|resident, _, _| resident.file_count())
            })
            .await;
            assert!(matches!(out, CallOutcome::Ready(ResidentOutcome::Ready(_, _))));
            started.elapsed()
        };

        // Each phase gets its OWN resident. Reusing one would let the control warm every
        // salsa memo the subject then reads back in milliseconds — a gate that measures
        // memo warmth passes whether or not the cancel ever reaches the walk.
        let control = ready_state(dir.path());
        reset();
        let ct = tokio_util::sync::CancellationToken::new();
        let first = tokio::spawn(resident_call(control.clone(), ct, walk));
        await_walk_start();
        let waited_behind_a_live_call = neighbour(control.clone()).await;
        first.await.expect("the uncancelled call finishes");

        // Subject: the same sequence on an equally cold resident, cancelled once the
        // walk is under way.
        let subject = ready_state(dir.path());
        reset();
        let ct = tokio_util::sync::CancellationToken::new();
        let cancelled = tokio::spawn(resident_call(subject.clone(), ct.clone(), walk));
        await_walk_start();
        ct.cancel();
        let waited_behind_a_cancelled_call = neighbour(subject.clone()).await;
        assert!(
            matches!(cancelled.await.expect("joined"), CallOutcome::Cancelled),
            "the cancelled call must answer as cancelled"
        );

        assert!(
            waited_behind_a_live_call > Duration::from_secs(1),
            "positive control is inert: the neighbour waited only {waited_behind_a_live_call:?} \
             behind a LIVE call, so this stand cannot show a cancel freeing the slot"
        );
        assert!(
            waited_behind_a_cancelled_call * 3 < waited_behind_a_live_call,
            "a cancelled call still held the resident: {waited_behind_a_cancelled_call:?} \
             behind a cancelled call vs {waited_behind_a_live_call:?} behind a live one"
        );
    }

    /// The walk itself stops. Answering quickly proves nothing on its own — the join is
    /// released without waiting for the blocking task — so this counts the files the walk
    /// actually entered.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_cancelled_walk_stops_before_the_last_file() {
        let _serialised = WALK_GATE.lock().await;
        install();
        let dir = tempfile::tempdir().unwrap();
        stand(dir.path());

        // Control: an uncancelled walk enters every calling file, so the counter is
        // shown to be able to reach the total it is later asserted to fall short of.
        // On its own resident: a warmed one would make the subject's walk finish before
        // the cancel could land.
        let control = ready_state(dir.path());
        reset();
        let out = resident_call(control, tokio_util::sync::CancellationToken::new(), walk).await;
        assert!(matches!(out, CallOutcome::Ready(ResidentOutcome::Ready(true, _))));
        let full = entered();
        assert!(full >= CALLERS, "an uncancelled walk must enter every caller, entered {full}");

        let state = ready_state(dir.path());
        reset();
        let ct = tokio_util::sync::CancellationToken::new();
        let cancelled = tokio::spawn(resident_call(state.clone(), ct.clone(), walk));
        await_walk_start();
        ct.cancel();
        assert!(matches!(cancelled.await.expect("joined"), CallOutcome::Cancelled));

        // The blocking body is detached: take the resident lock to know it has unwound.
        let out = resident_call(state.clone(), tokio_util::sync::CancellationToken::new(), |s| {
            s.read(|resident, _, _| resident.file_count())
        })
        .await;
        assert!(matches!(out, CallOutcome::Ready(ResidentOutcome::Ready(_, _))));

        let seen = entered();
        assert!(
            seen < full,
            "the walk ran to completion despite the cancel: entered {seen} of {full}"
        );
    }

    /// A cancelled call must not leave its database handle alive: salsa blocks every
    /// write while a clone exists, and the incremental drift apply is a write. Nothing
    /// else on this path is: a full rebuild swaps in a NEW database and never writes to
    /// the old one, so it would pass with a leaked clone.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_cancelled_call_leaves_no_handle_blocking_the_incremental_apply() {
        let _serialised = WALK_GATE.lock().await;
        install();
        let dir = tempfile::tempdir().unwrap();
        stand(dir.path());
        let state = ready_state(dir.path());

        reset();
        let ct = tokio_util::sync::CancellationToken::new();
        let cancelled = tokio::spawn(resident_call(state.clone(), ct.clone(), walk));
        await_walk_start();
        ct.cancel();
        assert!(matches!(cancelled.await.expect("joined"), CallOutcome::Cancelled));

        // Edit a body: the next read polls drift and applies it in place — the write
        // that a leaked clone would park forever on `while *clones != 1`.
        std::fs::write(
            dir.path().join("CommonModules/Вызов0000/Ext/Module.bsl"),
            "&НаСервере\nПроцедура Тело() Экспорт\n    Объявление.ПриИзмененииПоля();\n    \
             Объявление.ПриИзмененииПоля();\nКонецПроцедуры\n",
        )
        .unwrap();

        // On a thread with a deadline: a blocked apply must fail this gate, not hang it.
        let (tx, rx) = std::sync::mpsc::channel();
        let applying = state.clone();
        std::thread::spawn(move || {
            let out = applying.read(|resident, _| resident.file_count());
            let _ = tx.send(matches!(out, ResidentOutcome::Ready(_, _)));
        });
        let applied = rx.recv_timeout(Duration::from_secs(30)).expect(
            "the incremental apply must not block: a handle from the cancelled call \
                     is still alive and salsa is waiting for it to drop",
        );
        assert!(applied, "the resident must still serve after the apply");
    }

    /// Cancellation reaches this request and nothing else: the resident and its master
    /// handle are untouched, so the next call answers in full.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_cancel_does_not_reach_the_next_call() {
        let _serialised = WALK_GATE.lock().await;
        install();
        let dir = tempfile::tempdir().unwrap();
        stand(dir.path());
        let state = ready_state(dir.path());

        reset();
        let ct = tokio_util::sync::CancellationToken::new();
        let cancelled = tokio::spawn(resident_call(state.clone(), ct.clone(), walk));
        await_walk_start();
        ct.cancel();
        assert!(matches!(cancelled.await.expect("joined"), CallOutcome::Cancelled));

        let after =
            resident_call(state.clone(), tokio_util::sync::CancellationToken::new(), walk).await;
        assert!(
            matches!(after, CallOutcome::Ready(ResidentOutcome::Ready(true, _))),
            "a call after a cancelled one must answer in full"
        );
    }

    /// A resident that is not ready yet must not outrun the token. The `loading` envelope
    /// is a body like any other, and a tool that decided to publish it BEFORE entering the
    /// door would answer a cancelled call with content — which is why no tool branches on
    /// the lifecycle before `resident_call` any more.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_loading_resident_does_not_outrun_the_token() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "bsl-analyzer.toml", "[source]\nroot = \".\"\n");
        // Deliberately never brought to Ready: this is the first-call state.
        let state = DiagnosticsState::for_workspace(dir.path().to_path_buf());

        let ct = tokio_util::sync::CancellationToken::new();
        ct.cancel();
        let outcome = resident_call(state, ct, |session| {
            session.read(|resident, _, _| resident.file_count())
        })
        .await;

        // The body is spawned either way and unwinds on its own; what the door owes is
        // that nothing it computed is PUBLISHED. Asserting the body never started would
        // be asserting a race, not a property.
        assert!(
            matches!(outcome, CallOutcome::Cancelled),
            "a cancelled call answered from the lifecycle instead of answering as cancelled"
        );
    }

    /// `DiagnosticsState::read` polls for drift BEFORE it takes the lock, and a forced
    /// scan stats the whole tree. The request's salsa token is registered inside that
    /// lock, so it protects the queries and nothing before them: a session cancelled by
    /// now must refuse at the door, not pay for a walk nobody will read.
    #[test]
    fn a_cancelled_session_does_not_pay_for_a_drift_scan() {
        use std::panic::AssertUnwindSafe;
        use std::sync::atomic::Ordering;

        let dir = tempfile::tempdir().unwrap();
        write_common_module(
            dir.path(),
            "Модуль",
            true,
            "&НаСервере\nПроцедура П() Экспорт\nКонецПроцедуры\n",
        );
        write(dir.path(), "bsl-analyzer.toml", "[source]\nroot = \".\"\n");
        let state = ready_state(dir.path());

        // The scan is armed exactly as a forced rescan arms it.
        *crate::diagnostics_state::lock_recover(&state.scan) = None;
        state.force_scan.store(true, Ordering::SeqCst);
        let before = state.scan_count();

        let cancel = Arc::new(RequestCancel::default());
        cancel.cancel_all();
        let session = ResidentSession { diag: state.clone(), cancel };
        let caught = salsa::Cancelled::catch(AssertUnwindSafe(|| {
            session.read(|resident, _, _| resident.file_count())
        }));

        assert!(matches!(caught, Err(salsa::Cancelled::Local)), "the read must refuse outright");
        assert_eq!(
            state.scan_count(),
            before,
            "a cancelled session walked the tree: the drift poll ran before the refusal"
        );
    }

    /// A read served without the resident answers to this request's cancel too. The
    /// handle is empty, but the platform surface it walks is not, and a handle built
    /// outside the door carries a token nobody ever cancels.
    ///
    /// The inner `attach` stands in for the salsa queries such a walk makes: each is its
    /// own outermost scope unless the door holds one, and leaving that scope clears the
    /// handle's token — so this gate colours both the registration and the attach.
    #[test]
    fn a_detached_read_answers_to_this_requests_cancel() {
        use std::panic::AssertUnwindSafe;

        let dir = tempfile::tempdir().unwrap();
        let cancel = Arc::new(RequestCancel::default());
        let session = ResidentSession {
            diag: DiagnosticsState::for_workspace(dir.path().to_path_buf()),
            cancel: Arc::clone(&cancel),
        };

        let caught = salsa::Cancelled::catch(AssertUnwindSafe(|| {
            session.read_detached(|analysis| {
                let db = analysis.database();
                // The notification lands while the walk is under way.
                cancel.cancel_all();
                salsa::Database::attach(db, |_| ());
                salsa::Database::unwind_if_revision_cancelled(db);
                "walked the whole platform catalogue"
            })
        }));

        assert!(
            matches!(caught, Err(salsa::Cancelled::Local)),
            "the detached read ran to the end for a cancelled request"
        );
    }

    /// Three unwinds arrive the same way and mean three different things. A live
    /// `PendingWrite` cannot be produced on this path — every write needs the resident
    /// mutex the reader holds — so the gate is put on the classification itself.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn each_kind_of_unwind_keeps_its_own_meaning() {
        let raise = |cancelled: salsa::Cancelled| {
            move |_: &ResidentSession| -> bool {
                std::panic::resume_unwind(Box::new(cancelled));
            }
        };
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "bsl-analyzer.toml", "[source]\nroot = \".\"\n");
        let state = DiagnosticsState::for_workspace(dir.path().to_path_buf());
        let ct = || tokio_util::sync::CancellationToken::new();

        assert!(matches!(
            resident_call(state.clone(), ct(), raise(salsa::Cancelled::Local)).await,
            CallOutcome::Cancelled
        ));
        assert!(
            matches!(
                resident_call(state.clone(), ct(), raise(salsa::Cancelled::PendingWrite)).await,
                CallOutcome::Superseded
            ),
            "a writer cutting the call short is not the client's cancellation"
        );
        assert!(
            matches!(
                resident_call(state.clone(), ct(), raise(salsa::Cancelled::PropagatedPanic)).await,
                CallOutcome::Panicked
            ),
            "a panic must never be dressed up as a cancellation"
        );
    }
}
