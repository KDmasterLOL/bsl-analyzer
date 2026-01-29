//! Whitespace normalization.
//!
//! Handles:
//! - Spaces around operators
//! - Spaces after commas
//! - Trailing whitespace removal

use lexer::{tokenize, TokenKind};
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
fn forbids_space_before_paren(prev_kind: SyntaxKind) -> bool {
    matches!(
        prev_kind,
        SyntaxKind::IDENT | SyntaxKind::KW_NEW | SyntaxKind::R_PAREN | SyntaxKind::R_BRACKET
    )
}

/// Checks if this is likely a unary operator based on previous token.
fn is_likely_unary(kind: SyntaxKind, prev_kind: Option<SyntaxKind>) -> bool {
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

/// Convert lexer TokenKind to syntax SyntaxKind.
fn token_to_syntax_kind(kind: TokenKind) -> SyntaxKind {
    match kind {
        // Keywords
        TokenKind::KwProcedure => SyntaxKind::KW_PROCEDURE,
        TokenKind::KwEndProcedure => SyntaxKind::KW_END_PROCEDURE,
        TokenKind::KwFunction => SyntaxKind::KW_FUNCTION,
        TokenKind::KwEndFunction => SyntaxKind::KW_END_FUNCTION,
        TokenKind::KwExport => SyntaxKind::KW_EXPORT,
        TokenKind::KwVal => SyntaxKind::KW_VAL,
        TokenKind::KwIf => SyntaxKind::KW_IF,
        TokenKind::KwThen => SyntaxKind::KW_THEN,
        TokenKind::KwElsIf => SyntaxKind::KW_ELSIF,
        TokenKind::KwElse => SyntaxKind::KW_ELSE,
        TokenKind::KwEndIf => SyntaxKind::KW_END_IF,
        TokenKind::KwFor => SyntaxKind::KW_FOR,
        TokenKind::KwEach => SyntaxKind::KW_EACH,
        TokenKind::KwIn => SyntaxKind::KW_IN,
        TokenKind::KwTo => SyntaxKind::KW_TO,
        TokenKind::KwWhile => SyntaxKind::KW_WHILE,
        TokenKind::KwDo => SyntaxKind::KW_DO,
        TokenKind::KwEndDo => SyntaxKind::KW_END_DO,
        TokenKind::KwReturn => SyntaxKind::KW_RETURN,
        TokenKind::KwContinue => SyntaxKind::KW_CONTINUE,
        TokenKind::KwBreak => SyntaxKind::KW_BREAK,
        TokenKind::KwGoto => SyntaxKind::KW_GOTO,
        TokenKind::KwTry => SyntaxKind::KW_TRY,
        TokenKind::KwExcept => SyntaxKind::KW_EXCEPT,
        TokenKind::KwEndTry => SyntaxKind::KW_END_TRY,
        TokenKind::KwRaise => SyntaxKind::KW_RAISE,
        TokenKind::KwVar => SyntaxKind::KW_VAR,
        TokenKind::KwNew => SyntaxKind::KW_NEW,
        TokenKind::KwExecute => SyntaxKind::KW_EXECUTE,
        TokenKind::KwAddHandler => SyntaxKind::KW_ADD_HANDLER,
        TokenKind::KwRemoveHandler => SyntaxKind::KW_REMOVE_HANDLER,
        TokenKind::KwAsync => SyntaxKind::KW_ASYNC,
        TokenKind::KwAwait => SyntaxKind::KW_AWAIT,
        TokenKind::KwAnd => SyntaxKind::KW_AND,
        TokenKind::KwOr => SyntaxKind::KW_OR,
        TokenKind::KwNot => SyntaxKind::KW_NOT,
        TokenKind::KwTrue => SyntaxKind::KW_TRUE,
        TokenKind::KwFalse => SyntaxKind::KW_FALSE,
        TokenKind::KwUndefined => SyntaxKind::KW_UNDEFINED,
        TokenKind::KwNull => SyntaxKind::KW_NULL,

        // Preprocessor
        TokenKind::PreIf => SyntaxKind::PRE_IF,
        TokenKind::PreElsIf => SyntaxKind::PRE_ELSIF,
        TokenKind::PreElse => SyntaxKind::PRE_ELSE,
        TokenKind::PreEndIf => SyntaxKind::PRE_END_IF,
        TokenKind::PreRegion => SyntaxKind::PRE_REGION,
        TokenKind::PreEndRegion => SyntaxKind::PRE_END_REGION,
        TokenKind::PreUse => SyntaxKind::PRE_USE,
        TokenKind::PreInsert => SyntaxKind::PRE_INSERT,
        TokenKind::PreEndInsert => SyntaxKind::PRE_END_INSERT,
        TokenKind::PreDelete => SyntaxKind::PRE_DELETE,
        TokenKind::PreEndDelete => SyntaxKind::PRE_END_DELETE,

        // Annotations
        TokenKind::AnnAtClient => SyntaxKind::ANN_AT_CLIENT,
        TokenKind::AnnAtServer => SyntaxKind::ANN_AT_SERVER,
        TokenKind::AnnAtServerNoContext => SyntaxKind::ANN_AT_SERVER_NO_CONTEXT,
        TokenKind::AnnAtClientAtServerNoContext => SyntaxKind::ANN_AT_CLIENT_AT_SERVER_NO_CONTEXT,
        TokenKind::AnnAtClientAtServer => SyntaxKind::ANN_AT_CLIENT_AT_SERVER,
        TokenKind::AnnBefore => SyntaxKind::ANN_BEFORE,
        TokenKind::AnnAfter => SyntaxKind::ANN_AFTER,
        TokenKind::AnnAround => SyntaxKind::ANN_AROUND,
        TokenKind::AnnChangeAndValidate => SyntaxKind::ANN_CHANGE_AND_VALIDATE,
        TokenKind::AnnCustom => SyntaxKind::ANN_CUSTOM,

        // Operators
        TokenKind::Eq => SyntaxKind::EQ,
        TokenKind::Neq => SyntaxKind::NEQ,
        TokenKind::Le => SyntaxKind::LE,
        TokenKind::Lt => SyntaxKind::LT,
        TokenKind::Ge => SyntaxKind::GE,
        TokenKind::Gt => SyntaxKind::GT,
        TokenKind::Plus => SyntaxKind::PLUS,
        TokenKind::Minus => SyntaxKind::MINUS,
        TokenKind::Star => SyntaxKind::STAR,
        TokenKind::Slash => SyntaxKind::SLASH,
        TokenKind::Percent => SyntaxKind::PERCENT,

        // Punctuation
        TokenKind::LParen => SyntaxKind::L_PAREN,
        TokenKind::RParen => SyntaxKind::R_PAREN,
        TokenKind::LBracket => SyntaxKind::L_BRACKET,
        TokenKind::RBracket => SyntaxKind::R_BRACKET,
        TokenKind::Dot => SyntaxKind::DOT,
        TokenKind::Comma => SyntaxKind::COMMA,
        TokenKind::Semicolon => SyntaxKind::SEMICOLON,
        TokenKind::Colon => SyntaxKind::COLON,
        TokenKind::Question => SyntaxKind::QUESTION,
        TokenKind::Tilde => SyntaxKind::TILDE,
        TokenKind::Bar => SyntaxKind::BAR,
        TokenKind::Hash => SyntaxKind::HASH,
        TokenKind::Ampersand => SyntaxKind::AMPERSAND,
        TokenKind::Exclamation => SyntaxKind::EXCLAMATION,

        // Literals
        TokenKind::Float => SyntaxKind::FLOAT,
        TokenKind::Decimal => SyntaxKind::DECIMAL,
        TokenKind::String => SyntaxKind::STRING,
        TokenKind::StringStart => SyntaxKind::STRING_START,
        TokenKind::StringTail => SyntaxKind::STRING_TAIL,
        TokenKind::StringPart => SyntaxKind::STRING_PART,
        TokenKind::Date => SyntaxKind::DATE,

        // Identifier
        TokenKind::Ident => SyntaxKind::IDENT,

        // Trivia
        TokenKind::Whitespace => SyntaxKind::WHITESPACE,
        TokenKind::Newline => SyntaxKind::NEWLINE,
        TokenKind::Comment => SyntaxKind::COMMENT,

        // Error
        TokenKind::Error => SyntaxKind::ERROR,
    }
}

/// Normalizes whitespace in a line of BSL code.
///
/// Uses lexer to tokenize the line and rebuilds it with proper spacing:
/// - Single space around binary operators
/// - Single space after commas
/// - No space before comma, semicolon, closing parens
/// - No space after opening parens, dot
pub fn normalize_line_whitespace(line: &str, config: &FormattingConfig) -> String {
    // Skip empty lines
    if line.trim().is_empty() {
        return String::new();
    }

    // Skip comment-only lines
    let trimmed = line.trim();
    if trimmed.starts_with("//") {
        return trimmed.to_string();
    }

    // Tokenize the line
    let tokens = tokenize(line);

    // Filter out whitespace and newlines, keeping only meaningful tokens
    let meaningful_tokens: Vec<_> = tokens
        .iter()
        .filter(|t| !matches!(t.kind, TokenKind::Whitespace | TokenKind::Newline))
        .collect();

    if meaningful_tokens.is_empty() {
        return String::new();
    }

    // Rebuild with proper spacing
    let mut result = String::with_capacity(line.len());
    let mut prev_syntax_kind: Option<SyntaxKind> = None;
    let mut prev_was_unary = false;

    for token in meaningful_tokens {
        let syntax_kind = token_to_syntax_kind(token.kind);
        let text = token.text.as_str();

        // Check if current token is a unary operator
        let is_unary = is_likely_unary(syntax_kind, prev_syntax_kind);

        // Determine if we need space before this token
        let need_space = if result.is_empty() || forbids_space_before(syntax_kind) || prev_was_unary
        {
            false
        } else if let Some(prev) = prev_syntax_kind {
            // No space before ( in function calls, or after tokens that forbid space
            if (syntax_kind == SyntaxKind::L_PAREN && forbids_space_before_paren(prev))
                || forbids_space_after(prev)
            {
                false
            } else if is_unary {
                // Unary operators need space before if previous token requires space after
                // e.g., "А = -1" not "А =-1"
                needs_space_after(prev, config)
            } else if needs_space_before(syntax_kind, config) || needs_space_after(prev, config) {
                true
            } else {
                // Default: space between tokens unless both are punctuation-like
                !is_punctuation(syntax_kind) || !is_punctuation(prev)
            }
        } else {
            false
        };

        if need_space {
            result.push(' ');
        }

        result.push_str(text);
        prev_syntax_kind = Some(syntax_kind);
        prev_was_unary = is_unary;
    }

    result
}

/// Checks if a token is punctuation (doesn't need spaces around by default).
fn is_punctuation(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::L_PAREN
            | SyntaxKind::R_PAREN
            | SyntaxKind::L_BRACKET
            | SyntaxKind::R_BRACKET
            | SyntaxKind::DOT
            | SyntaxKind::COMMA
            | SyntaxKind::SEMICOLON
            | SyntaxKind::COLON
            | SyntaxKind::TILDE
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_simple() {
        let config = FormattingConfig::default();
        assert_eq!(normalize_line_whitespace("А=1;", &config), "А = 1;");
        assert_eq!(normalize_line_whitespace("А  =  1 ;", &config), "А = 1;");
    }

    #[test]
    fn test_normalize_binary_ops() {
        let config = FormattingConfig::default();
        assert_eq!(normalize_line_whitespace("А=Б+В;", &config), "А = Б + В;");
        assert_eq!(normalize_line_whitespace("А=Б-В*Г/Д;", &config), "А = Б - В * Г / Д;");
    }

    #[test]
    fn test_normalize_comma() {
        let config = FormattingConfig::default();
        // Use valid identifiers (not keywords)
        assert_eq!(normalize_line_whitespace("Тест(А,Б,В)", &config), "Тест(А, Б, В)");
        assert_eq!(normalize_line_whitespace("Тест(А ,Б ,В)", &config), "Тест(А, Б, В)");
    }

    #[test]
    fn test_normalize_dot() {
        let config = FormattingConfig::default();
        assert_eq!(normalize_line_whitespace("А.Б.В", &config), "А.Б.В");
        assert_eq!(normalize_line_whitespace("А . Б . В", &config), "А.Б.В");
    }

    #[test]
    fn test_normalize_parens() {
        let config = FormattingConfig::default();
        // Use valid identifiers (not keywords like "Функция")
        assert_eq!(normalize_line_whitespace("Тест( А )", &config), "Тест(А)");
        assert_eq!(normalize_line_whitespace("( А + Б )", &config), "(А + Б)");
    }

    #[test]
    fn test_normalize_unary_minus() {
        let config = FormattingConfig::default();
        assert_eq!(normalize_line_whitespace("А=-1;", &config), "А = -1;");
        assert_eq!(normalize_line_whitespace("Тест(-1)", &config), "Тест(-1)");
    }

    #[test]
    fn test_normalize_comment() {
        let config = FormattingConfig::default();
        assert_eq!(normalize_line_whitespace("// Комментарий", &config), "// Комментарий");
    }

    #[test]
    fn test_normalize_keywords() {
        let config = FormattingConfig::default();
        assert_eq!(normalize_line_whitespace("Если А Тогда", &config), "Если А Тогда");
        assert_eq!(normalize_line_whitespace("Возврат А;", &config), "Возврат А;");
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
