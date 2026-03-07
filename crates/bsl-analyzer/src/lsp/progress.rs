//! LSP WorkDoneProgress utilities.
//!
//! This module provides types and helpers for reporting progress to LSP clients.

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
    /// Computes progress fraction (0.0 to 1.0) from done/total counts.
    pub fn fraction(done: usize, total: usize) -> f64 {
        assert!(done <= total);
        done as f64 / total.max(1) as f64
    }
}
