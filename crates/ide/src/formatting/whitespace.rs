use syntax::SyntaxKind;

use super::FormattingConfig;

pub fn needs_space_before(kind: SyntaxKind, config: &FormattingConfig) -> bool {
    match kind {
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

        SyntaxKind::EQ => config.space_around_assignment,

        SyntaxKind::KW_AND | SyntaxKind::KW_OR => true,

        _ => false,
    }
}

pub fn needs_space_after(kind: SyntaxKind, config: &FormattingConfig) -> bool {
    match kind {
        SyntaxKind::COMMA => config.space_after_comma,

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

        SyntaxKind::EQ => config.space_around_assignment,

        SyntaxKind::KW_AND | SyntaxKind::KW_OR => true,

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

pub fn forbids_space_before(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::COMMA
            | SyntaxKind::SEMICOLON
            | SyntaxKind::R_PAREN
            | SyntaxKind::R_BRACKET
            | SyntaxKind::DOT
            | SyntaxKind::L_BRACKET
    )
}

pub fn forbids_space_after(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::L_PAREN | SyntaxKind::L_BRACKET | SyntaxKind::DOT | SyntaxKind::TILDE
    )
}

pub(super) fn forbids_space_before_paren(prev_kind: SyntaxKind) -> bool {
    matches!(
        prev_kind,
        SyntaxKind::IDENT
            | SyntaxKind::KW_NEW
            | SyntaxKind::R_PAREN
            | SyntaxKind::R_BRACKET
            | SyntaxKind::KW_EXECUTE
    )
}

pub(super) fn is_likely_unary(kind: SyntaxKind, prev_kind: Option<SyntaxKind>) -> bool {
    if !matches!(kind, SyntaxKind::MINUS | SyntaxKind::PLUS) {
        return false;
    }

    match prev_kind {
        None => true,
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
