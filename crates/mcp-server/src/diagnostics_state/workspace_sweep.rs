use std::sync::Mutex;

use super::lifecycle::lock_recover;

/// Filters for a workspace sweep.
pub(crate) struct SweepOptions {
    pub min_severity: ide::SeverityBucket,
    /// Keep only these codes (empty = all).
    pub codes: Vec<String>,
    /// Cap on files swept (bounds the cost of an opt-in whole-config pass).
    pub max_files: usize,
}

/// One code's workspace-wide tally.
pub(crate) struct CodeAggregate {
    pub code: String,
    pub severity: ide::SeverityBucket,
    pub count: usize,
    pub files_affected: usize,
}

/// The result of a workspace sweep: per-code aggregates plus coverage bookkeeping.
pub(crate) struct WorkspaceSweep {
    pub aggregates: Vec<CodeAggregate>,
    /// Files actually analysed. Equals the capped request size on a completed sweep;
    /// smaller when the sweep was cancelled mid-flight.
    pub files_swept: usize,
    pub files_total: usize,
    /// Files excluded by the vendor-diff analysis scope (no changed lines vs the
    /// configured base); 0 when no scope is configured.
    pub files_out_of_scope: usize,
    /// Files counted in `files_total` that could not be swept because their bytes
    /// could not be read. Beside `files_out_of_scope` and for the same reason: a gap
    /// in coverage is reported, never quietly removed from the total.
    pub files_unread: usize,
    /// Findings dropped because every covered line is attributed to an
    /// `[analysis].ignored_authors` entry; 0 when the filter is off.
    pub findings_ignored_by_author: usize,
    /// HEAD commit the author filter attributed against, folded into the
    /// result id so a filter rebuild after a ref move changes the identity.
    pub author_head: Option<String>,
    pub truncated: bool,
    /// The sweep was cancelled mid-flight (MCP `notifications/cancelled` or transport
    /// shutdown); `aggregates` cover only the `files_swept` files processed before it.
    pub cancelled: bool,
}

/// Cancellation bridge for one workspace sweep: rayon workers register the salsa
/// token of their per-worker db clone before their first query, and the MCP
/// `notifications/cancelled` watcher cancels them all at once. Registration and
/// cancellation share one mutex, so a worker registering after the cancel request
/// has its token cancelled on the spot — there is no window in which a late worker
/// runs to completion. Cancelling touches only worker-clone tokens, never the
/// master db handle.
#[derive(Default)]
pub(crate) struct SweepCancel {
    inner: Mutex<SweepCancelInner>,
}

#[derive(Default)]
struct SweepCancelInner {
    cancel_requested: bool,
    tokens: Vec<salsa::CancellationToken>,
}

impl SweepCancel {
    /// Register a worker handle's token; cancels it immediately when cancellation
    /// was already requested.
    pub(crate) fn register(&self, token: salsa::CancellationToken) {
        let mut inner = lock_recover(&self.inner);
        if inner.cancel_requested {
            token.cancel();
        }
        inner.tokens.push(token);
    }

    /// Request cancellation: every registered worker token unwinds at its next salsa
    /// query boundary; workers not yet registered are cancelled by `register`.
    pub(crate) fn cancel_all(&self) {
        let mut inner = lock_recover(&self.inner);
        inner.cancel_requested = true;
        for token in &inner.tokens {
            token.cancel();
        }
    }

    /// Cheap file-boundary check for workers between queries (nothing to unwind from).
    pub(crate) fn is_cancelled(&self) -> bool {
        lock_recover(&self.inner).cancel_requested
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The single registration/cancellation mutex closes the race with workers that
    /// start after the cancel request: their token is cancelled at registration.
    #[test]
    fn register_after_cancel_is_cancelled_on_the_spot() {
        let cancel = SweepCancel::default();

        let early = salsa::CancellationToken::default();
        cancel.register(early.clone());
        assert!(!early.is_cancelled(), "no cancel requested yet");
        assert!(!cancel.is_cancelled());

        cancel.cancel_all();
        assert!(early.is_cancelled(), "cancel_all cancels every registered token");
        assert!(cancel.is_cancelled());

        let late = salsa::CancellationToken::default();
        cancel.register(late.clone());
        assert!(late.is_cancelled(), "a late registration is cancelled immediately");
    }
}
