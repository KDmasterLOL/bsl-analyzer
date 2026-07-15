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
    pub files_swept: usize,
    pub files_total: usize,
    pub truncated: bool,
}
