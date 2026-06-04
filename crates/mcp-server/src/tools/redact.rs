//! Secret redaction for BSL source that leaves the MCP server.
//!
//! A keyword-triggered safety net against exfiltrating hardcoded credentials. Within each
//! statement (up to its `;`), a string literal is masked to `"***"` only when a *sensitive
//! marker* precedes it — a sensitive-named identifier (`Токен = "…"`) or a key-like sensitive
//! string (`Вставить("Пароль", "…")`). This targets the secret *value* while leaving the
//! body's other literals — structure field lists, localized messages, type names — intact.
//!
//! It deliberately does not mask everything in a flagged body, so it can miss a secret whose
//! variable name is not itself sensitive (e.g. a keyword that appears only in a comment several
//! statements away). That is the accepted limit of marker-targeted redaction; a multi-line
//! `|`-continued value IS covered, because a literal stays open until its closing quote.

/// Substrings (case-insensitive, RU + EN) that mark an identifier or key as secret-bearing.
const SENSITIVE: &[&str] =
    &["пароль", "password", "token", "токен", "secret", "секрет", "apikey", "ключапи"];

/// Whether `s` contains any sensitive substring (case-insensitive).
fn is_sensitive(s: &str) -> bool {
    let lower = s.to_lowercase();
    SENSITIVE.iter().any(|kw| lower.contains(kw))
}

/// Redact the secret values in a method/source body, leaving non-secret literals readable.
pub(crate) fn redact_secrets(src: &str) -> String {
    // Nothing sensitive anywhere — return untouched (and skip the scan).
    if !is_sensitive(src) {
        return src.to_string();
    }
    redact_targeted(src)
}

/// Single pass: emit the source verbatim except that a string literal is replaced with `"***"`
/// when an earlier marker in the same statement armed masking. `armed` resets at every `;`.
fn redact_targeted(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    // A sensitive marker has appeared earlier in the current statement, so the next literal is
    // its secret value. Reset at each statement boundary (`;`).
    let mut armed = false;
    // The identifier/word currently being accumulated; its sensitivity is judged when it ends.
    let mut word = String::new();

    while let Some(c) = chars.next() {
        if c == '"' {
            // The word immediately before a literal (an assignment LHS like `Токен`) is a marker.
            if is_sensitive(&word) {
                armed = true;
            }
            word.clear();

            // Consume the whole literal, honouring `""` escapes and multi-line continuations.
            let mut content = String::new();
            loop {
                match chars.next() {
                    Some('"') if chars.peek() == Some(&'"') => {
                        chars.next();
                        content.push('"');
                        content.push('"');
                    }
                    Some('"') | None => break,
                    Some(ch) => content.push(ch),
                }
            }

            if armed {
                out.push('"');
                out.push_str("***");
                out.push('"');
            } else {
                out.push('"');
                out.push_str(&content);
                out.push('"');
                // A key-like sensitive string (a single token, e.g. a `Вставить("Токен", …)`
                // key) arms masking of the value that follows it. A sentence that merely mentions
                // a keyword (a localized message) has internal whitespace and does NOT arm.
                if !content.trim().chars().any(char::is_whitespace) && is_sensitive(&content) {
                    armed = true;
                }
            }
        } else if c == ';' {
            // Statement boundary: a new statement starts unarmed.
            word.clear();
            armed = false;
            out.push(';');
        } else if c.is_alphanumeric() || c == '_' {
            word.push(c);
            out.push(c);
        } else {
            if is_sensitive(&word) {
                armed = true;
            }
            word.clear();
            out.push(c);
        }
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
    fn masks_secret_value_but_keeps_other_literals() {
        // The targeted rule masks only the value assigned to a sensitive-named identifier; an
        // unrelated literal in a later statement of the same body stays readable.
        let src = "Пароль = \"hunter2\";\nИмя = \"Иванов\";";
        let out = redact_secrets(src);
        assert!(out.contains("Пароль = \"***\";"), "secret value masked: {out}");
        assert!(out.contains("Иванов"), "non-secret literal kept: {out}");
    }

    #[test]
    fn masks_value_after_a_sensitive_key_but_keeps_the_key_and_messages() {
        // The SMS-style shape: a sensitive *key* arms masking of its value, while the key name,
        // structure field lists, and localized messages survive.
        let src = "Стр = Новый Структура(\"Имя, Город\");\n\
                   Стр.Вставить(\"Токен\", \"abc123\");\n\
                   Сообщить(\"Не удалось отправить\");";
        let out = redact_secrets(src);
        assert!(out.contains("\"Имя, Город\""), "field list kept: {out}");
        assert!(out.contains("\"Токен\""), "key name kept: {out}");
        assert!(out.contains("\"***\""), "token value masked: {out}");
        assert!(!out.contains("abc123"), "token value must not leak: {out}");
        assert!(out.contains("Не удалось отправить"), "message kept: {out}");
    }

    #[test]
    fn redacts_multiline_continued_string() {
        // The secret value spans a `|` continuation line — the full literal must be masked.
        let src = "Токен = \"first\n|secretpart\";";
        let out = redact_secrets(src);
        assert!(!out.contains("secretpart"), "{out}");
        assert!(!out.contains("first"), "{out}");
    }

    #[test]
    fn redacts_when_keyword_precedes_the_value_via_comment() {
        // A keyword in a comment with no intervening `;` arms the following value.
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
