//! Secret redaction for BSL source that leaves the MCP server.
//!
//! A keyword-triggered safety net against exfiltrating hardcoded credentials.
//! When a source body mentions a secret keyword anywhere, **every** string
//! literal in that body is redacted to `"***"`. This deliberately over-redacts
//! (non-secret strings in a flagged body are also masked) rather than risk
//! under-redaction: a line-local approach misses multi-line `|`-continued
//! strings and assignments whose literal sits on a different line than the
//! keyword. It cannot catch unlabelled secrets (no keyword) — that is the
//! documented limit of keyword-based redaction.

/// Substrings (case-insensitive, RU + EN) that mark a body as secret-bearing.
const SENSITIVE: &[&str] =
    &["пароль", "password", "token", "токен", "secret", "секрет", "apikey", "ключапи"];

/// Redact a method/source body's string literals if it mentions a secret keyword.
pub(crate) fn redact_secrets(src: &str) -> String {
    let lower = src.to_lowercase();
    if SENSITIVE.iter().any(|kw| lower.contains(kw)) {
        redact_all_strings(src)
    } else {
        src.to_string()
    }
}

/// Replace the contents of every BSL string literal with `***`, tracking string
/// state across the whole input. Handles `""` (an embedded escaped quote) and
/// multi-line strings (a literal stays open until its closing quote, so newlines
/// and `|` continuations inside it are masked too).
fn redact_all_strings(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false;

    while let Some(c) = chars.next() {
        if c == '"' {
            if !in_string {
                out.push('"');
                out.push_str("***");
                in_string = true;
            } else if chars.peek() == Some(&'"') {
                // Escaped quote inside the literal — stays open, content masked.
                chars.next();
            } else {
                out.push('"');
                in_string = false;
            }
        } else if !in_string {
            out.push(c);
        }
        // Characters inside a string literal are dropped (already masked by `***`).
    }
    if in_string {
        // Unterminated literal — close it so output stays well-formed.
        out.push('"');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::redact_secrets;

    #[test]
    fn non_sensitive_body_is_untouched() {
        let src = "Имя = \"Иванов\";\nГород = \"Москва\";";
        assert_eq!(redact_secrets(src), src);
    }

    #[test]
    fn redacts_all_literals_in_a_sensitive_body() {
        let src = "Пароль = \"hunter2\";\nИмя = \"Иванов\";";
        let out = redact_secrets(src);
        assert!(out.contains("Пароль = \"***\";"), "{out}");
        // Over-redacts other literals in the flagged body — the safe direction.
        assert!(!out.contains("Иванов"), "{out}");
    }

    #[test]
    fn redacts_multiline_continued_string() {
        // The secret value spans a `|` continuation line — must not leak.
        let src = "Токен = \"first\n|secretpart\";";
        let out = redact_secrets(src);
        assert!(!out.contains("secretpart"), "{out}");
        assert!(!out.contains("first"), "{out}");
    }

    #[test]
    fn redacts_when_literal_is_on_a_later_line_than_keyword() {
        let src = "// задаём пароль ниже\nЗначение = \"s3cr3t\";";
        let out = redact_secrets(src);
        assert!(!out.contains("s3cr3t"), "{out}");
    }

    #[test]
    fn handles_embedded_escaped_quotes() {
        let src = "Password = \"a\"\"b\";\nX = 1;";
        let out = redact_secrets(src);
        assert!(!out.contains("a\"\"b"), "{out}");
        // Structure after the literal survives.
        assert!(out.contains("X = 1;"), "{out}");
    }
}
