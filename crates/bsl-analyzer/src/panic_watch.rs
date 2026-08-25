//! Which salsa revision a real panic happened in, so a propagated
//! *cancellation* is not reported to the client as one.
//!
//! Salsa hands a waiting thread [`salsa::Cancelled::PropagatedPanic`] whenever
//! the thread owning its query unwinds — a pending write included, because the
//! claim guard reserves `WaitResult::Cancelled` for a thread-LOCAL cancellation
//! only. The payload cannot tell the two apart, but the panic hook can: salsa
//! throws cancellations with `resume_unwind`, which deliberately skips the hook,
//! while a real panic always runs it.
//!
//! The revision is the whole point, and it is taken from salsa rather than
//! guessed. A panicking query poisons its memo for the revision it ran in, and
//! salsa replays that panic to later readers of the same revision (`execute.rs`,
//! `previous_iteration`) — readers that never blocked on anything. A reader of a
//! LATER revision is provably safe from it, so nothing narrower than the true
//! revision boundary may be used: an LRU eviction takes `&mut` on the database
//! without starting a revision, and a proxy that counted writes would drift
//! exactly there and hide the panic.
//!
//! A panic with no database attached to its thread cannot be replayed: nobody
//! can block on a thread that is not executing a query. Those are not stamped.

use std::sync::Mutex;

use salsa::plumbing::{current_revision, with_attached_database};
use salsa::Revision;

/// Newest revision a real panic was seen in.
static LAST_PANIC: Mutex<Option<Revision>> = Mutex::new(None);

static INSTALLED: std::sync::Once = std::sync::Once::new();

/// Chain a stamping hook in front of the current one.
///
/// Without it every propagated unwind reads as "nothing panicked", which would
/// hide real panics behind a retryable error code.
pub fn install() {
    INSTALLED.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Salsa attaches the database to a thread for the duration of a
            // tracked-fn body, which is exactly the window where a panic can
            // poison a memo or strand a waiter.
            if let Some(revision) = with_attached_database(current_revision) {
                let mut last = lock();
                if last.is_none_or(|seen| seen < revision) {
                    *last = Some(revision);
                }
            }
            previous(info);
        }));
    });
}

/// Could a real panic reach a reader running in `revision`?
///
/// True for a panic of that same revision — the poisoned-memo replay and the
/// stranded-waiter case alike — and for anything newer, which is over-reporting
/// by design: reporting a panic that was not this request's is the safe error,
/// hiding one is not.
pub fn panicked_in(revision: Revision) -> bool {
    lock().is_some_and(|seen| seen >= revision)
}

fn lock() -> std::sync::MutexGuard<'static, Option<Revision>> {
    // The guarded data is a plain `Option<Revision>`; a poisoned lock carries no
    // broken invariant, so recovering is the whole handling.
    LAST_PANIC.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Forget every stamp. Tests share the process — and every fresh database
/// starts at the same revision — so a peer's stamp would otherwise answer for
/// this test's database.
#[doc(hidden)]
pub fn reset_for_test() {
    *lock() = None;
}

/// Serialises tests that panic on purpose against tests that assert no panic was
/// seen. The stamp is process-wide, so a panicking peer running in parallel
/// would otherwise flip a verdict that has nothing to do with it.
#[doc(hidden)]
pub fn panic_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide::RootDatabaseImpl;
    use salsa::Database as _;

    #[test]
    fn a_cancellation_is_not_a_panic() {
        let _guard = panic_test_guard();
        install();
        reset_for_test();
        let db = RootDatabaseImpl::default();
        let revision = current_revision(&db);

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            db.attach(|_| std::panic::resume_unwind(Box::new(salsa::Cancelled::PendingWrite)))
        }));

        assert!(!panicked_in(revision), "a cancellation unwinds past the hook");
    }

    #[test]
    fn a_panic_inside_a_query_is_stamped_with_its_revision() {
        let _guard = panic_test_guard();
        install();
        reset_for_test();
        let db = RootDatabaseImpl::default();
        let revision = current_revision(&db);

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            db.attach(|_| panic!("a query panicking"))
        }));

        assert!(panicked_in(revision), "a real panic inside a query must be visible");
    }

    /// The assumption the whole design rests on, asserted rather than believed:
    /// evicting the LRU takes `&mut` on the database but is not a revision
    /// boundary, while an input write is. Anything that counts `&mut` accesses
    /// instead of asking salsa drifts apart from the poison exactly here.
    #[test]
    fn an_lru_eviction_is_not_a_revision_boundary_but_a_write_is() {
        let mut db = RootDatabaseImpl::default();
        let before = current_revision(&db);

        db.enforce_lru();
        assert_eq!(current_revision(&db), before, "LRU eviction must not start a revision");

        db.set_workspace_load_complete(true);
        assert!(current_revision(&db) > before, "an input write must start a revision");
    }

    #[test]
    fn a_panic_with_no_database_attached_is_not_stamped() {
        let _guard = panic_test_guard();
        install();
        reset_for_test();
        // A fresh database is at the same revision as the panic below, so a
        // stamp would be indistinguishable from the case above — which is the
        // point: nothing may be stamped when nobody could have blocked on it.
        let db = RootDatabaseImpl::default();
        let revision = current_revision(&db);

        let _ = std::panic::catch_unwind(|| panic!("a panic outside any query"));

        assert!(
            !panicked_in(revision),
            "a thread not executing a query cannot strand a waiter or poison a memo"
        );
    }
}
