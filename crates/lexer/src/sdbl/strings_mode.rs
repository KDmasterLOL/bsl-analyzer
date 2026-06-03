use smol_str::SmolStr;

use super::{SdblToken, SdblTokenKind};

pub(super) struct StringsRun {
    pub tokens: Vec<SdblToken>,
    pub end_pos: usize,
}

pub(super) fn scan(input: &str, start_pos: usize) -> StringsRun {
    let bytes = input.as_bytes();
    let mut pos = start_pos;

    if pos >= bytes.len() || bytes[pos] != b'"' {
        return StringsRun { tokens: Vec::new(), end_pos: pos };
    }

    let mut tokens = Vec::new();

    tokens.push(make_string_token(input, pos, pos + 1));
    pos += 1;

    loop {
        let run_start = pos;

        while pos < bytes.len() && bytes[pos] != b'"' && bytes[pos] != b'\n' && bytes[pos] != b'\r'
        {
            pos += 1;
        }

        if pos >= bytes.len() {
            if run_start < pos {
                tokens.push(make_string_token(input, run_start, pos));
            }
            break;
        }

        match bytes[pos] {
            b'"' => {
                if pos + 1 < bytes.len() && bytes[pos + 1] == b'"' {
                    pos += 2;
                    continue;
                }
                if run_start < pos {
                    tokens.push(make_string_token(input, run_start, pos));
                }
                tokens.push(make_string_token(input, pos, pos + 1));
                pos += 1;
                break;
            }
            b'\n' | b'\r' => {
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
