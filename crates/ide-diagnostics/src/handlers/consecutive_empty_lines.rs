//! ConsecutiveEmptyLines diagnostic.
//!
//! Checks for too many consecutive empty lines.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};

/// Runs the ConsecutiveEmptyLines diagnostic.
pub fn check(_ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // TODO: Implement
    Vec::new()
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_consecutive_empty_lines() {
        // TODO: Add tests
    }
}
