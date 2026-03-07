//! Diagnostic documentation module.
//!
//! Provides access to diagnostic descriptions in Russian and English.
//! Documentation is embedded at compile time from MD files in docs/ directory.

// Include generated code from build.rs
include!(concat!(env!("OUT_DIR"), "/docs_generated.rs"));

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticCode;

    #[test]
    fn test_get_docs_line_length() {
        let docs = get_docs(DiagnosticCode::LineLength);
        assert!(!docs.name_ru.is_empty(), "LineLength should have Russian name");
        assert!(!docs.description_ru.is_empty(), "LineLength should have Russian description");
    }

    #[test]
    fn test_get_docs_cyclomatic_complexity() {
        let docs = get_docs(DiagnosticCode::CyclomaticComplexity);
        assert!(docs.description_ru.contains("Цикломатическая"));
    }

    #[test]
    fn test_documented_codes_not_empty() {
        // Check that we have a reasonable number of documented codes
        assert!(DOCUMENTED_CODES.len() >= 170, "Should have at least 170 documented codes");
    }

    #[test]
    fn test_unknown_code_returns_empty() {
        // ParseError might not have docs, test that it doesn't panic
        let docs = get_docs(DiagnosticCode::ParseError);
        // Either has docs or is empty, but shouldn't panic
        let _ = docs.name_ru;
    }
}
