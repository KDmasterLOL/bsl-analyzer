//! Whitespace normalization.
//!
//! Handles:
//! - Spaces around operators
//! - Spaces after commas
//! - Trailing whitespace removal

// These functions are prepared for future token-level whitespace normalization
#![allow(dead_code)]

use syntax::{SyntaxKind, SyntaxToken};

use super::FormattingConfig;

/// Checks if a token kind requires a space before it.
pub fn needs_space_before(kind: SyntaxKind, config: &FormattingConfig) -> bool {
    match kind {
        // Binary operators
        SyntaxKind::PLUS
        | SyntaxKind::MINUS
        | SyntaxKind::STAR
        | SyntaxKind::SLASH
        | SyntaxKind::PERCENT
        | SyntaxKind::LT
        | SyntaxKind::LE
        | SyntaxKind::GT
        | SyntaxKind::GE
        | SyntaxKind::NEQ => config.space_around_binary_ops,

        // Comparison (always space for readability)
        SyntaxKind::EQ => config.space_around_assignment,

        // Logical operators (keywords)
        SyntaxKind::KW_AND | SyntaxKind::KW_OR => true,

        _ => false,
    }
}

/// Checks if a token kind requires a space after it.
pub fn needs_space_after(kind: SyntaxKind, config: &FormattingConfig) -> bool {
    match kind {
        // Comma
        SyntaxKind::COMMA => config.space_after_comma,

        // Binary operators
        SyntaxKind::PLUS
        | SyntaxKind::MINUS
        | SyntaxKind::STAR
        | SyntaxKind::SLASH
        | SyntaxKind::PERCENT
        | SyntaxKind::LT
        | SyntaxKind::LE
        | SyntaxKind::GT
        | SyntaxKind::GE
        | SyntaxKind::NEQ => config.space_around_binary_ops,

        // Assignment
        SyntaxKind::EQ => config.space_around_assignment,

        // Logical operators
        SyntaxKind::KW_AND | SyntaxKind::KW_OR => true,

        // Keywords that should have space after
        SyntaxKind::KW_IF
        | SyntaxKind::KW_ELSIF
        | SyntaxKind::KW_WHILE
        | SyntaxKind::KW_FOR
        | SyntaxKind::KW_RETURN
        | SyntaxKind::KW_VAR
        | SyntaxKind::KW_NEW
        | SyntaxKind::KW_NOT
        | SyntaxKind::KW_IN
        | SyntaxKind::KW_TO
        | SyntaxKind::KW_EACH => true,

        _ => false,
    }
}

/// Checks if there should be no space before a token.
pub fn forbids_space_before(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::COMMA
            | SyntaxKind::SEMICOLON
            | SyntaxKind::R_PAREN
            | SyntaxKind::R_BRACKET
            | SyntaxKind::DOT
    )
}

/// Checks if there should be no space after a token.
pub fn forbids_space_after(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::L_PAREN | SyntaxKind::L_BRACKET | SyntaxKind::DOT | SyntaxKind::KW_NOT // Унарный оператор НЕ может быть без пробела перед скобкой
    )
}

/// Checks if a token is the unary minus (before a number or identifier).
pub fn is_unary_minus(token: &SyntaxToken) -> bool {
    if token.kind() != SyntaxKind::MINUS {
        return false;
    }

    // Check previous non-trivia token
    let prev = token.prev_token();
    match prev {
        Some(t) => {
            let kind = t.kind();
            // Unary if after operator, open paren, comma, or at start
            matches!(
                kind,
                SyntaxKind::L_PAREN
                    | SyntaxKind::COMMA
                    | SyntaxKind::EQ
                    | SyntaxKind::PLUS
                    | SyntaxKind::MINUS
                    | SyntaxKind::STAR
                    | SyntaxKind::SLASH
                    | SyntaxKind::LT
                    | SyntaxKind::LE
                    | SyntaxKind::GT
                    | SyntaxKind::GE
                    | SyntaxKind::NEQ
                    | SyntaxKind::KW_RETURN
                    | SyntaxKind::KW_AND
                    | SyntaxKind::KW_OR
            )
        }
        None => true, // At start of expression
    }
}

/// Trims trailing whitespace from a line.
pub fn trim_trailing_whitespace(line: &str) -> &str {
    line.trim_end_matches([' ', '\t'])
}

/// Normalizes internal whitespace (multiple spaces to one).
pub fn normalize_internal_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_was_space = false;

    for c in text.chars() {
        if c == ' ' || c == '\t' {
            if !prev_was_space {
                result.push(' ');
                prev_was_space = true;
            }
        } else {
            result.push(c);
            prev_was_space = false;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trim_trailing() {
        assert_eq!(trim_trailing_whitespace("hello  "), "hello");
        assert_eq!(trim_trailing_whitespace("hello\t"), "hello");
        assert_eq!(trim_trailing_whitespace("hello"), "hello");
        assert_eq!(trim_trailing_whitespace(""), "");
    }

    #[test]
    fn test_normalize_whitespace() {
        assert_eq!(normalize_internal_whitespace("a  b"), "a b");
        assert_eq!(normalize_internal_whitespace("a   b   c"), "a b c");
        assert_eq!(normalize_internal_whitespace("a\t\tb"), "a b");
        assert_eq!(normalize_internal_whitespace("ab"), "ab");
    }

    #[test]
    fn test_needs_space() {
        let config = FormattingConfig::default();
        assert!(needs_space_before(SyntaxKind::PLUS, &config));
        assert!(needs_space_after(SyntaxKind::COMMA, &config));
        assert!(!needs_space_before(SyntaxKind::COMMA, &config));
    }

    #[test]
    fn test_forbids_space() {
        assert!(forbids_space_before(SyntaxKind::COMMA));
        assert!(forbids_space_before(SyntaxKind::R_PAREN));
        assert!(forbids_space_after(SyntaxKind::L_PAREN));
        assert!(forbids_space_after(SyntaxKind::DOT));
    }
}
