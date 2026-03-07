//! Test utilities for bsl-analyzer.
//!
//! This crate provides common testing utilities.

pub use expect_test::{expect, Expect};

/// Asserts that actual equals expected using expect-test.
pub fn check(actual: &str, expect: Expect) {
    expect.assert_eq(actual);
}

/// Normalizes line endings to LF.
pub fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Extracts cursor position marked by `$0` in the input.
pub fn extract_cursor(input: &str) -> (String, Option<usize>) {
    if let Some(pos) = input.find("$0") {
        let text = format!("{}{}", &input[..pos], &input[pos + 2..]);
        (text, Some(pos))
    } else {
        (input.to_string(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_cursor() {
        let (text, pos) = extract_cursor("hello$0world");
        assert_eq!(text, "helloworld");
        assert_eq!(pos, Some(5));
    }
}
