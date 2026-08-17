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
    pub baseline: ide::diagnostics_baseline::DiagnosticsBaselineSummary,
    pub baseline_epoch: String,
}

impl WorkspaceSweep {
    /// A sweep cancelled before it selected a single file: nothing analysed, and the
    /// coverage numbers that do not depend on the selection still stated.
    pub(crate) fn nothing_swept(files_total: usize, files_unread: usize) -> Self {
        Self {
            aggregates: Vec::new(),
            files_swept: 0,
            files_total,
            files_out_of_scope: 0,
            files_unread,
            findings_ignored_by_author: 0,
            author_head: None,
            truncated: false,
            cancelled: true,
        }
    }
}
