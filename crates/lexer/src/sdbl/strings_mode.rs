//! SDBL multiline-string scanner (clean-room).
//!
//! # Mini-spec
//!
//! An SDBL string literal is delimited by a pair of ASCII double-quote
//! characters (`"`). The scanner walks the source starting at an
//! opening `"` and produces a flat sequence of `String` tokens that,
//! taken together, reconstruct the literal.
//!
//! - **Opening quote.** The opening `"` is always emitted as a
//!   `String` token whose text is the single quote character, at the
//!   opening offset.
//!
//! - **Lone closing `"`.** After the opening quote, the scanner
//!   accumulates characters until it meets a single `"`. Any
//!   accumulated content is emitted as a `String` token at the offset
//!   where accumulation for this run started; the closing `"` is then
//!   emitted as a separate `String` token and the scanner returns.
//!
//! - **Doubled `""`.** A pair of adjacent double-quote characters is
//!   the literal-quote escape form. The scanner resets the
//!   accumulation anchor past the pair and continues scanning; any
//!   content scanned before the `""` inside the same run is not
//!   emitted as a token on its own. This is the observed pre-refactor
//!   behaviour and is preserved bit-for-bit by the Slice 1 clean-room
//!   rewrite; revisiting the escape treatment is deferred to a later
//!   slice.
//!
//! - **Line break (`\n` or `\r`).** The accumulated content, if any,
//!   is emitted as a `String` token. The scanner then consumes the
//!   line break together with any run of spaces and tabs that
//!   follows, modelling the BSL multiline-string convention in which
//!   each continuation line begins with indentation that is not part
//!   of the logical string body. Scanning resumes with a fresh
//!   accumulation run.
//!
//! - **End of input before a closing `"`.** Any still-accumulated
//!   content is emitted; no closing-quote token is produced.
//!
//! # Return shape
//!
//! The scanner returns a [`StringsRun`] carrying the produced tokens
//! and the absolute byte offset at which the outer tokeniser should
//! resume. All tokens are of kind [`SdblTokenKind::String`].

use smol_str::SmolStr;

use super::{SdblToken, SdblTokenKind};

/// Output of one [`scan`] invocation.
pub(super) struct StringsRun {
    pub tokens: Vec<SdblToken>,
    pub end_pos: usize,
}

/// Scan a `"`-delimited string starting at `start_pos` inside `input`.
///
/// Returns an empty run if `start_pos` does not point at an opening
/// `"`, so the caller can fall back to the logos-driven tokeniser
/// without risking spurious `String` tokens.
pub(super) fn scan(input: &str, start_pos: usize) -> StringsRun {
    let bytes = input.as_bytes();
    let mut pos = start_pos;

    if pos >= bytes.len() || bytes[pos] != b'"' {
        return StringsRun { tokens: Vec::new(), end_pos: pos };
    }

    let mut tokens = Vec::new();

    // Rule 1: emit the opening quote.
    tokens.push(make_string_token(input, pos, pos + 1));
    pos += 1;

    loop {
        let run_start = pos;

        // Advance until the next quote or line break.
        while pos < bytes.len() && bytes[pos] != b'"' && bytes[pos] != b'\n' && bytes[pos] != b'\r'
        {
            pos += 1;
        }

        // Rule 3: unterminated at EOF.
        if pos >= bytes.len() {
            if run_start < pos {
                tokens.push(make_string_token(input, run_start, pos));
            }
            break;
        }

        match bytes[pos] {
            b'"' => {
                // Rule 2b: doubled-quote escape resets the anchor.
                if pos + 1 < bytes.len() && bytes[pos + 1] == b'"' {
                    pos += 2;
                    continue;
                }
                // Rule 2a: lone closing quote.
                if run_start < pos {
                    tokens.push(make_string_token(input, run_start, pos));
                }
                tokens.push(make_string_token(input, pos, pos + 1));
                pos += 1;
                break;
            }
            b'\n' | b'\r' => {
                // Rule 2c: line break, then skip leading whitespace of
                // the next continuation line.
                if run_start < pos {
                    tokens.push(make_string_token(input, run_start, pos));
                }
                while pos < bytes.len() && matches!(bytes[pos], b'\n' | b'\r' | b' ' | b'\t') {
                    pos += 1;
                }
            }
            _ => unreachable!("inner loop exits only on a quote or line break"),
        }
    }

    StringsRun { tokens, end_pos: pos }
}

fn make_string_token(input: &str, start: usize, end: usize) -> SdblToken {
    SdblToken { kind: SdblTokenKind::String, text: SmolStr::new(&input[start..end]), offset: start }
}
