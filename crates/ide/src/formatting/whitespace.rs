//! Whitespace policy tables consumed by the IR formatter (`ir.rs`).
//!
//! Each helper answers a single question about a [`SyntaxKind`] (or pair of
//! kinds) — there is no formatting logic here, just rule lookups.

use syntax::SyntaxKind;

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
            // `[` in BSL is only ever index access; no space between target and `[`.
            | SyntaxKind::L_BRACKET
    )
}

/// Checks if there should be no space after a token.
pub fn forbids_space_after(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::L_PAREN | SyntaxKind::L_BRACKET | SyntaxKind::DOT | SyntaxKind::TILDE // Label prefix ~
    )
}

/// Checks if there should be no space before opening paren (function call).
pub(super) fn forbids_space_before_paren(prev_kind: SyntaxKind) -> bool {
    matches!(
        prev_kind,
        SyntaxKind::IDENT
            | SyntaxKind::KW_NEW
            | SyntaxKind::R_PAREN
            | SyntaxKind::R_BRACKET
            // `Выполнить`/`Execute` is reserved by the lexer but appears as a
            // method name in expressions like `Х.Выполнить()`. Treat the
            // following `(` as a call argument list, not a control-keyword arg.
            | SyntaxKind::KW_EXECUTE
    )
}

/// Checks if this is likely a unary operator based on previous token.
pub(super) fn is_likely_unary(kind: SyntaxKind, prev_kind: Option<SyntaxKind>) -> bool {
    if !matches!(kind, SyntaxKind::MINUS | SyntaxKind::PLUS) {
        return false;
    }

    match prev_kind {
        None => true, // At start
        Some(prev) => {
            matches!(
                prev,
                SyntaxKind::L_PAREN
                    | SyntaxKind::L_BRACKET
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
                    | SyntaxKind::KW_NOT
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
