//! Per-line token classification used by on-type formatting.
//!
//! These helpers operate on a single source line and answer questions like
//! "does this line start a block?" or "is it a middle keyword (Иначе /
//! Исключение)?". The IR formatter (`ir.rs`) does *not* use them — it works
//! at the token-graph level directly. They live here because on-type
//! formatting fires per-keystroke and needs a cheap line-local view that
//! doesn't require re-parsing the whole document.

use lexer::{tokenize, TokenKind};

/// Information about tokens in a line for formatting decisions.
pub(crate) struct LineTokens {
    pub first: Option<TokenKind>,
    pub last: Option<TokenKind>,
    pub has_then: bool,
}

/// Analyzes a line and extracts token information for formatting.
pub(crate) fn analyze_line_tokens(line: &str) -> LineTokens {
    let tokens = tokenize(line);

    let meaningful: Vec<_> = tokens
        .iter()
        .filter(|t| {
            !matches!(t.kind, TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comment)
        })
        .collect();

    let first = meaningful.first().map(|t| t.kind);
    let last = meaningful.last().map(|t| t.kind);
    let has_then = meaningful.iter().any(|t| t.kind == TokenKind::KwThen);

    LineTokens { first, last, has_then }
}

/// Checks if the first token is a block-ending keyword.
pub(crate) fn is_line_block_end(tokens: &LineTokens) -> bool {
    matches!(
        tokens.first,
        Some(TokenKind::KwEndProcedure)
            | Some(TokenKind::KwEndFunction)
            | Some(TokenKind::KwEndIf)
            | Some(TokenKind::KwEndDo)
            | Some(TokenKind::KwEndTry)
            | Some(TokenKind::PreEndRegion)
            | Some(TokenKind::PreEndIf)
            | Some(TokenKind::PreEndInsert)
            | Some(TokenKind::PreEndDelete)
    )
}

/// Checks if the line is a middle keyword (needs dedent for itself).
/// Middle keywords: Иначе, ИначеЕсли, Исключение, standalone Тогда/Цикл,
/// or continuation lines (ИЛИ/И) ending with Тогда/Цикл.
pub(crate) fn is_line_middle_keyword(tokens: &LineTokens) -> bool {
    let starts_middle = matches!(
        tokens.first,
        Some(TokenKind::KwElse)
            | Some(TokenKind::KwElsIf)
            | Some(TokenKind::KwExcept)
            | Some(TokenKind::PreElse)
            | Some(TokenKind::PreElsIf)
    );

    if starts_middle {
        return true;
    }

    if matches!(tokens.first, Some(TokenKind::KwThen) | Some(TokenKind::KwDo)) {
        return true;
    }

    let ends_with_then_or_do =
        matches!(tokens.last, Some(TokenKind::KwThen) | Some(TokenKind::KwDo));
    let starts_block_keyword = matches!(
        tokens.first,
        Some(TokenKind::KwIf)
            | Some(TokenKind::KwElsIf)
            | Some(TokenKind::KwFor)
            | Some(TokenKind::KwWhile)
            | Some(TokenKind::PreIf)
            | Some(TokenKind::PreElsIf)
    );

    ends_with_then_or_do && !starts_block_keyword
}

/// Checks if the line starts a block (increases indent for following lines).
pub(crate) fn is_line_block_start(tokens: &LineTokens) -> bool {
    let first = tokens.first;

    if matches!(first, Some(TokenKind::KwProcedure) | Some(TokenKind::KwFunction)) {
        return true;
    }

    if matches!(first, Some(TokenKind::KwIf)) {
        return true;
    }

    if matches!(first, Some(TokenKind::KwThen)) {
        return true;
    }

    if tokens.last == Some(TokenKind::KwThen) {
        return true;
    }

    if matches!(first, Some(TokenKind::KwFor) | Some(TokenKind::KwWhile)) {
        return true;
    }

    if matches!(first, Some(TokenKind::KwDo)) {
        return true;
    }

    if tokens.last == Some(TokenKind::KwDo) {
        return true;
    }

    if matches!(first, Some(TokenKind::KwElsIf)) && tokens.has_then {
        return true;
    }

    if matches!(first, Some(TokenKind::KwElse)) {
        return true;
    }

    if matches!(first, Some(TokenKind::KwTry)) {
        return true;
    }

    if matches!(first, Some(TokenKind::KwExcept)) {
        return true;
    }

    if matches!(
        first,
        Some(TokenKind::PreRegion)
            | Some(TokenKind::PreIf)
            | Some(TokenKind::PreElse)
            | Some(TokenKind::PreElsIf)
            | Some(TokenKind::PreInsert)
            | Some(TokenKind::PreDelete)
    ) {
        return true;
    }

    false
}
