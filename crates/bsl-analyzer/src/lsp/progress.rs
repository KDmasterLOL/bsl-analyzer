//! LSP WorkDoneProgress helpers.

/// Progress state for WorkDoneProgress notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Starting a new operation.
    Begin,
    /// Intermediate progress report.
    Report,
    /// Operation completed.
    End,
}

impl Progress {
    /// Returns `done / total`, treating an empty total as one item.
    pub fn fraction(done: usize, total: usize) -> f64 {
        assert!(done <= total);
        done as f64 / total.max(1) as f64
    }
}
