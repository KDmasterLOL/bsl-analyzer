//! Multiline string scanner for SDBL.
//!
//! Pure state machine that advances over a run of `"`-delimited text,
//! emitting one or more `String` tokens. Isolated from `logos` so the
//! behaviour of BSL-embedded SDBL multiline strings can be reasoned
//! about without the derive-macro regex pipeline. The scanner is
//! intentionally framework-free: it owns only the substring it walks
//! and does not depend on any `SdblTokenKind` regex.

use smol_str::SmolStr;

use super::{SdblToken, SdblTokenKind};

/// Result of a strings-mode scan: the emitted tokens and the position
/// at which the top-level SDBL tokeniser should resume.
pub(super) struct StringsRun {
    pub tokens: Vec<SdblToken>,
    pub end_pos: usize,
}

/// Scan a `"`-delimited string starting at `start_pos` in `input`.
///
/// `start_pos` must point at an opening `"`. If it does not, the
/// scanner returns an empty run so the caller can fall back to the
/// logos-driven tokeniser.
pub(super) fn scan(input: &str, start_pos: usize) -> StringsRun {
    let mut tokens = Vec::new();
    let mut pos = start_pos;
    let bytes = input.as_bytes();

    if pos >= bytes.len() || bytes[pos] != b'"' {
        return StringsRun { tokens, end_pos: pos };
    }

    let opening_quote_pos = pos;
    tokens.push(SdblToken {
        kind: SdblTokenKind::String,
        text: SmolStr::new(&input[opening_quote_pos..opening_quote_pos + 1]),
        offset: opening_quote_pos,
    });
    pos += 1;

    loop {
        let content_start = pos;

        while pos < bytes.len() && bytes[pos] != b'"' && bytes[pos] != b'\n' && bytes[pos] != b'\r'
        {
            pos += 1;
        }

        if pos >= bytes.len() {
            if content_start < pos {
                let text = SmolStr::new(&input[content_start..pos]);
                tokens.push(SdblToken { kind: SdblTokenKind::String, text, offset: content_start });
            }
            break;
        }

        if bytes[pos] == b'"' {
            if pos + 1 < bytes.len() && bytes[pos + 1] == b'"' {
                pos += 2;
                continue;
            } else {
                if content_start < pos {
                    let text = SmolStr::new(&input[content_start..pos]);
                    tokens.push(SdblToken {
                        kind: SdblTokenKind::String,
                        text,
                        offset: content_start,
                    });
                }
                tokens.push(SdblToken {
                    kind: SdblTokenKind::String,
                    text: SmolStr::new(&input[pos..pos + 1]),
                    offset: pos,
                });
                pos += 1;
                break;
            }
        }

        if bytes[pos] == b'\n' || bytes[pos] == b'\r' {
            if content_start < pos {
                let text = SmolStr::new(&input[content_start..pos]);
                tokens.push(SdblToken { kind: SdblTokenKind::String, text, offset: content_start });
            }

            while pos < bytes.len()
                && (bytes[pos] == b'\n'
                    || bytes[pos] == b'\r'
                    || bytes[pos] == b' '
                    || bytes[pos] == b'\t')
            {
                pos += 1;
            }
        }
    }

    StringsRun { tokens, end_pos: pos }
}
