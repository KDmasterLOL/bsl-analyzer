//! Clean-room acceptance tests for the Slice 1 tokens of
//! `SdblTokenKind`.
//!
//! Sources:
//! - ITS lexical elements — <https://its.1c.ru/db/pubqlang/content/12/hdoc>
//! - ITS query-language structure — <https://its.1c.ru/db/pubqlang/content/10/hdoc>
//! - Local mini-specs in the module headers of
//!   `crates/lexer/src/sdbl/mod.rs` and
//!   `crates/lexer/src/sdbl/strings_mode.rs`.
//!
//! Per-test docstring shorthand: `ITS pubqlang/N` refers to the
//! documentation sub-tree rooted at
//! `https://its.1c.ru/db/pubqlang/content/N/hdoc` for the given `N`.
//!
//! These tests were authored against those specifications, not
//! against the existing `tokenize_sdbl` output. Where the current
//! implementation carries a documented pre-refactor behaviour that
//! diverges from a strict reading of the ITS wording (notably the
//! doubled-quote escape inside strings), the assertion mirrors the
//! locally-documented behaviour and a comment at the assertion notes
//! the gap for a later slice.

use lexer::sdbl::{tokenize_sdbl, SdblToken, SdblTokenKind};

fn kinds(tokens: &[SdblToken]) -> Vec<SdblTokenKind> {
    tokens.iter().map(|t| t.kind).collect()
}

// ---------------------------------------------------------------------------
// Whitespace and line structure
// ---------------------------------------------------------------------------

/// ITS pubqlang/12 — lexical elements: inter-token whitespace is
/// insignificant and collapses into a single whitespace run.
#[test]
fn whitespace_collapses_to_single_token() {
    let tokens = tokenize_sdbl("   \t ");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Whitespace]);
    assert_eq!(tokens[0].text.as_str(), "   \t ");
}

/// ITS pubqlang/12 — lexical elements: newline terminates a line.
#[test]
fn newline_is_its_own_token() {
    let tokens = tokenize_sdbl("\n");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Newline]);
}

/// ITS pubqlang/12 — lexical elements: CR is part of the whitespace
/// class, LF is the line terminator. `\r\n` therefore splits into
/// `Whitespace` + `Newline`.
#[test]
fn carriage_return_is_horizontal_whitespace() {
    let tokens = tokenize_sdbl("\r\n");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Whitespace, SdblTokenKind::Newline]);
}

// ---------------------------------------------------------------------------
// Separators and single-char markers
// ---------------------------------------------------------------------------

/// ITS pubqlang/12 — separators: `( ) . , ;` each stand alone.
#[test]
fn separators_produce_individual_tokens() {
    let tokens = tokenize_sdbl("().,;");
    assert_eq!(
        kinds(&tokens),
        vec![
            SdblTokenKind::LParen,
            SdblTokenKind::RParen,
            SdblTokenKind::Dot,
            SdblTokenKind::Comma,
            SdblTokenKind::Semicolon,
        ]
    );
}

/// ITS pubqlang/10 temp-table marker (`#`) and parameter prefix (`&`),
/// plus the local BSL multiline continuation bar (`|`): each stands
/// alone when not followed by an identifier.
#[test]
fn single_char_markers_produce_individual_tokens() {
    let tokens = tokenize_sdbl("#&|");
    assert_eq!(
        kinds(&tokens),
        vec![SdblTokenKind::Hash, SdblTokenKind::Ampersand, SdblTokenKind::Bar,]
    );
}

// ---------------------------------------------------------------------------
// Comparison operators
// ---------------------------------------------------------------------------

/// ITS pubqlang/10 — comparison operators: all six forms.
#[test]
fn comparison_operators_all_six() {
    let tokens = tokenize_sdbl("= <> <= < >= >");
    let op_kinds: Vec<_> =
        tokens.iter().filter(|t| t.kind != SdblTokenKind::Whitespace).map(|t| t.kind).collect();
    assert_eq!(
        op_kinds,
        vec![
            SdblTokenKind::Eq,
            SdblTokenKind::Neq,
            SdblTokenKind::Le,
            SdblTokenKind::Lt,
            SdblTokenKind::Ge,
            SdblTokenKind::Gt,
        ]
    );
}

/// ITS pubqlang/10 — longest-match: `<=` is a single operator, not
/// `<` followed by `=`.
#[test]
fn le_longest_match_beats_lt_plus_eq() {
    let tokens = tokenize_sdbl("<=");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Le]);
}

/// ITS pubqlang/10 — longest-match: `>=` is a single operator.
#[test]
fn ge_longest_match_beats_gt_plus_eq() {
    let tokens = tokenize_sdbl(">=");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Ge]);
}

/// ITS pubqlang/10 — longest-match: `<>` is a single operator.
#[test]
fn neq_longest_match_beats_lt_plus_gt() {
    let tokens = tokenize_sdbl("<>");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Neq]);
}

// ---------------------------------------------------------------------------
// Arithmetic operators
// ---------------------------------------------------------------------------

/// ITS pubqlang/10 — arithmetic operators: all five forms.
#[test]
fn arithmetic_operators_all_five() {
    let tokens = tokenize_sdbl("+-*/%");
    assert_eq!(
        kinds(&tokens),
        vec![
            SdblTokenKind::Plus,
            SdblTokenKind::Minus,
            SdblTokenKind::Star,
            SdblTokenKind::Slash,
            SdblTokenKind::Percent,
        ]
    );
}

/// ITS pubqlang/12 — numeric literals have no leading sign; a
/// leading `-` in front of a number is its own arithmetic-operator
/// token, parser-level unary-minus.
#[test]
fn leading_minus_is_its_own_token() {
    let tokens = tokenize_sdbl("-5");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Minus, SdblTokenKind::Decimal]);
}

// ---------------------------------------------------------------------------
// Numeric literals
// ---------------------------------------------------------------------------

/// ITS pubqlang/12 — integer literal: a run of decimal digits.
#[test]
fn integer_literal() {
    let tokens = tokenize_sdbl("42");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Decimal]);
    assert_eq!(tokens[0].text.as_str(), "42");
}

/// ITS pubqlang/12 — fractional literal: `DIGITS"."DIGITS`.
/// Longest-match must pick the whole run rather than `3` + `.` + `14`.
#[test]
fn fractional_literal_beats_integer_plus_dot_plus_integer() {
    let tokens = tokenize_sdbl("3.14");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Float]);
    assert_eq!(tokens[0].text.as_str(), "3.14");
}

/// ITS pubqlang/12 — the fractional form requires at least one digit
/// before the `.`; there is no leading-dot literal. `.5` splits as
/// `Dot` + `Decimal`.
#[test]
fn leading_dot_is_not_a_fractional_literal() {
    let tokens = tokenize_sdbl(".5");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Dot, SdblTokenKind::Decimal]);
}

// ---------------------------------------------------------------------------
// Date literal
// ---------------------------------------------------------------------------

/// ITS pubqlang/12 — date literal, 8-digit calendar-date form.
#[test]
fn date_literal_eight_digits() {
    let tokens = tokenize_sdbl("'20240101'");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Date]);
}

/// ITS pubqlang/12 — date literal, 14-digit calendar-date-plus-time
/// form.
#[test]
fn date_literal_fourteen_digits() {
    let tokens = tokenize_sdbl("'20240101120000'");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Date]);
}

/// ITS pubqlang/12 — date literal requires 8 or 14 digits between
/// the apostrophes. A 7-digit run is below the minimum and must not
/// be recognised as a `Date`; the lexer falls back to error tokens
/// on the apostrophes.
#[test]
fn seven_digit_apostrophe_form_is_not_a_date() {
    let tokens = tokenize_sdbl("'1234567'");
    assert!(
        tokens.iter().all(|t| t.kind != SdblTokenKind::Date),
        "7-digit apostrophe form must not be recognised as a Date; got {:?}",
        kinds(&tokens)
    );
}

/// ITS pubqlang/12 — date literal permits at most 14 digits between
/// the apostrophes. A 15-digit run exceeds the maximum and must not
/// be recognised as a `Date`.
#[test]
fn fifteen_digit_apostrophe_form_is_not_a_date() {
    let tokens = tokenize_sdbl("'123456789012345'");
    assert!(
        tokens.iter().all(|t| t.kind != SdblTokenKind::Date),
        "15-digit apostrophe form must not be recognised as a Date; got {:?}",
        kinds(&tokens)
    );
}

// ---------------------------------------------------------------------------
// Identifier
// ---------------------------------------------------------------------------

/// ITS pubqlang/12 — identifier: ASCII letter-plus-digits-plus-underscore.
#[test]
fn ascii_identifier() {
    let tokens = tokenize_sdbl("Name_1");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Ident]);
}

/// ITS pubqlang/12 — identifier: Unicode letter class covers
/// Cyrillic.
#[test]
fn cyrillic_identifier() {
    let tokens = tokenize_sdbl("Наименование");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Ident]);
}

/// ITS pubqlang/12 — identifier: leading underscore is allowed.
#[test]
fn leading_underscore_identifier() {
    let tokens = tokenize_sdbl("_private");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Ident]);
}

/// ITS pubqlang/12 — identifiers must start with a letter or
/// underscore; a leading digit splits the run into `Decimal` +
/// `Ident`.
#[test]
fn leading_digit_does_not_form_a_single_identifier() {
    let tokens = tokenize_sdbl("1abc");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Decimal, SdblTokenKind::Ident]);
}

// ---------------------------------------------------------------------------
// Parameter reference
// ---------------------------------------------------------------------------

/// ITS pubqlang/10 — parameter: `&` immediately followed by an
/// identifier forms a single `Parameter` token.
#[test]
fn parameter_ascii_name() {
    let tokens = tokenize_sdbl("&Start");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Parameter]);
}

/// ITS pubqlang/10 — parameter: the identifier half may be Cyrillic.
#[test]
fn parameter_cyrillic_name() {
    let tokens = tokenize_sdbl("&Имя");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Parameter]);
}

/// ITS pubqlang/10 — a lone `&` not followed by an identifier
/// character falls through to the bare `Ampersand` separator.
#[test]
fn bare_ampersand_is_not_a_parameter() {
    let tokens = tokenize_sdbl("&");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Ampersand]);
}

// ---------------------------------------------------------------------------
// Temporary-table marker
// ---------------------------------------------------------------------------

/// ITS pubqlang/10 — `#` alone is the temporary-table marker token.
#[test]
fn hash_standalone() {
    let tokens = tokenize_sdbl("#");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Hash]);
}

/// ITS pubqlang/10 — temp-table references are written `#Name` but
/// the lexer emits two tokens (`Hash` + `Ident`) so the parser can
/// resolve the temp-table context explicitly.
#[test]
fn hash_plus_identifier_is_two_tokens() {
    let tokens = tokenize_sdbl("#TempT");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Hash, SdblTokenKind::Ident]);
}

// ---------------------------------------------------------------------------
// Line comment (local spec)
// ---------------------------------------------------------------------------

/// Local spec: a line comment matches `//` followed by any run of
/// non-newline characters; the newline itself terminates the comment
/// and surfaces as a separate `Newline` token.
#[test]
fn line_comment_stops_at_newline() {
    let tokens = tokenize_sdbl("// hello\n");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Comment, SdblTokenKind::Newline]);
    assert_eq!(tokens[0].text.as_str(), "// hello");
}

/// Local spec: a line comment without a trailing newline extends to
/// EOF.
#[test]
fn line_comment_without_trailing_newline() {
    let tokens = tokenize_sdbl("// end of file");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::Comment]);
}

// ---------------------------------------------------------------------------
// String literal (strings-mode mini-spec)
// ---------------------------------------------------------------------------

/// Mini-spec rule for a single-segment literal: the scanner emits
/// three `String` tokens — opening quote, content, closing quote.
#[test]
fn string_basic_open_content_close() {
    let tokens = tokenize_sdbl(r#""Hello""#);
    assert_eq!(
        kinds(&tokens),
        vec![SdblTokenKind::String, SdblTokenKind::String, SdblTokenKind::String]
    );
    assert_eq!(tokens[0].text.as_str(), "\"");
    assert_eq!(tokens[1].text.as_str(), "Hello");
    assert_eq!(tokens[2].text.as_str(), "\"");
}

/// Mini-spec EOF rule: any still-accumulated content is emitted and
/// no closing-quote token is produced.
#[test]
fn string_unterminated_at_eof() {
    let tokens = tokenize_sdbl("\"oops");
    assert_eq!(kinds(&tokens), vec![SdblTokenKind::String, SdblTokenKind::String]);
    assert_eq!(tokens[0].text.as_str(), "\"");
    assert_eq!(tokens[1].text.as_str(), "oops");
}

/// Mini-spec line-break rule: on a line break, emit the current run
/// and consume the newline plus any leading spaces/tabs of the
/// continuation line. The `|` of the continuation is part of the
/// next run, not stripped.
#[test]
fn string_multiline_with_bar_continuation() {
    let tokens = tokenize_sdbl("\"head\n    | tail\"");
    assert_eq!(
        kinds(&tokens),
        vec![
            SdblTokenKind::String,
            SdblTokenKind::String,
            SdblTokenKind::String,
            SdblTokenKind::String,
        ]
    );
    assert_eq!(tokens[1].text.as_str(), "head");
    assert_eq!(tokens[2].text.as_str(), "| tail");
}

/// Mini-spec content integrity: punctuation inside the body must not
/// be reinterpreted as SDBL tokens. This is load-bearing for the
/// parser, which treats string bodies as opaque.
#[test]
fn string_body_with_parentheses_is_preserved() {
    let tokens = tokenize_sdbl("\"(a, b)\"");
    assert!(tokens.iter().all(|t| t.kind == SdblTokenKind::String));
    assert_eq!(tokens[1].text.as_str(), "(a, b)");
}

/// Mini-spec doubled-quote rule (preserved pre-refactor behaviour):
/// a `""` escape resets the accumulation anchor past the pair, so
/// content scanned before `""` in the same run is not emitted as its
/// own token. Aligning this with the ITS literal-quote escape is a
/// later-slice follow-up.
#[test]
fn string_doubled_quote_escape_observed_behaviour() {
    let tokens = tokenize_sdbl(r#""A""B""#);
    assert_eq!(
        kinds(&tokens),
        vec![SdblTokenKind::String, SdblTokenKind::String, SdblTokenKind::String]
    );
    assert_eq!(tokens[0].text.as_str(), "\"");
    assert_eq!(tokens[1].text.as_str(), "B");
    assert_eq!(tokens[2].text.as_str(), "\"");
}
