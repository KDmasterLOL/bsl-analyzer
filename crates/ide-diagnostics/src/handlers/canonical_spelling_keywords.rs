//! CanonicalSpellingKeywords diagnostic.
//!
//! Checks that keywords are spelled in canonical form.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};

/// Runs the CanonicalSpellingKeywords diagnostic.
pub fn check(_ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // TODO: Implement
    Vec::new()
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_canonical_spelling() {
        // TODO: Add tests
    }
}
