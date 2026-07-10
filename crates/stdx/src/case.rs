//! Case-insensitive string comparison tuned for BSL's bilingual alphabet.
//!
//! BSL identifiers and keywords are ASCII or Russian Cyrillic, and comparing
//! them via `a.to_lowercase() == b.to_lowercase()` is a hot-path anti-pattern:
//! it allocates two `String`s, folds both sides in full even when the first
//! characters already differ, and every Cyrillic char pays a binary search in
//! the Unicode conversion tables. The helpers here fold per char with an
//! arithmetic fast path for ASCII and the Russian block, allocate nothing, and
//! exit on the first mismatch.
//!
//! Semantics match comparing `chars().flat_map(char::to_lowercase)` streams:
//! context-sensitive mappings of `str::to_lowercase` (Greek final sigma) are
//! not applied, which no BSL keyword or identifier comparison can reach.

/// Folds a char to lowercase when the mapping is trivially single-char:
/// ASCII, the Russian А-Я block, and Ё. Returns `None` for anything that
/// needs the full Unicode tables.
#[inline]
fn fold_char_fast(c: char) -> Option<char> {
    match c {
        'A'..='Z' => Some((c as u8 | 0x20) as char),
        c if c.is_ascii() => Some(c),
        'А'..='Я' => char::from_u32(c as u32 + 0x20),
        'Ё' => Some('ё'),
        'а'..='я' | 'ё' => Some(c),
        _ => None,
    }
}

/// Unicode-aware case-insensitive comparison without allocating.
pub fn eq_ignore_case(a: &str, b: &str) -> bool {
    let mut a_chars = a.chars();
    let mut b_chars = b.chars();
    loop {
        match (a_chars.next(), b_chars.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) => match (fold_char_fast(x), fold_char_fast(y)) {
                (Some(fx), Some(fy)) => {
                    if fx != fy {
                        return false;
                    }
                }
                // Multi-char lowercase expansions can desynchronise the
                // char-by-char walk, so restart stream comparison from the
                // current pair. Everything before it folded 1:1.
                _ => return eq_ignore_case_slow(x, y, a_chars, b_chars),
            },
            _ => return false,
        }
    }
}

/// Drop-in replacement for `str::to_lowercase`.
///
/// Output is byte-identical to `str::to_lowercase` for every input: strings
/// made of ASCII and the Russian block fold arithmetically in one pass, and
/// the first char outside that alphabet falls back to the standard library
/// for the whole string (preserving its context-sensitive mappings). Folded
/// results from either path can therefore be mixed freely, including as
/// persisted map keys.
pub trait CaseExt {
    fn fold_lower(&self) -> String;
}

impl CaseExt for str {
    fn fold_lower(&self) -> String {
        // ASCII and Russian-block chars keep their UTF-8 length when folded,
        // so the fast path never reallocates.
        let mut out = String::with_capacity(self.len());
        for c in self.chars() {
            match fold_char_fast(c) {
                Some(folded) => out.push(folded),
                None => return self.to_lowercase(),
            }
        }
        out
    }
}

/// Folds per char with no contextual mappings, so two strings produce the
/// same key **iff** [`eq_ignore_case`] holds for them. `fold_lower` is not
/// that key: its `str::to_lowercase` fallback applies contextual mappings
/// (Greek final sigma), splitting `eq_ignore_case`-equal strings into
/// different keys. Use this for match buckets, `fold_lower` for display or
/// `to_lowercase`-compatible persisted keys.
pub fn fold_lower_per_char(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match fold_char_fast(c) {
            Some(folded) => out.push(folded),
            None => out.extend(c.to_lowercase()),
        }
    }
    out
}

#[cold]
fn eq_ignore_case_slow(
    x: char,
    y: char,
    a_rest: std::str::Chars<'_>,
    b_rest: std::str::Chars<'_>,
) -> bool {
    let a_folded = x.to_lowercase().chain(a_rest.flat_map(char::to_lowercase));
    let b_folded = y.to_lowercase().chain(b_rest.flat_map(char::to_lowercase));
    a_folded.eq(b_folded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii() {
        assert!(eq_ignore_case("Procedure", "PROCEDURE"));
        assert!(eq_ignore_case("Procedure", "procedure"));
        assert!(!eq_ignore_case("Procedure", "Procedures"));
        assert!(!eq_ignore_case("Procedures", "Procedure"));
        assert!(!eq_ignore_case("Procedure", "Procedurf"));
    }

    #[test]
    fn cyrillic() {
        assert!(eq_ignore_case("Процедура", "ПРОЦЕДУРА"));
        assert!(eq_ignore_case("Процедура", "процедура"));
        assert!(eq_ignore_case("ПроЦеДурА", "процедура"));
        assert!(!eq_ignore_case("Процедура", "Процедуры"));
    }

    #[test]
    fn yo_is_not_ascii_foldable() {
        assert!(eq_ignore_case("Ёлка", "ёлка"));
        assert!(eq_ignore_case("ёЖ", "Ёж"));
        assert!(!eq_ignore_case("Ёлка", "Елка"));
    }

    #[test]
    fn mixed_scripts_and_empty() {
        assert!(eq_ignore_case("Таблица1Row", "таблица1row"));
        assert!(eq_ignore_case("", ""));
        assert!(!eq_ignore_case("", "a"));
        assert!(!eq_ignore_case("а", ""));
    }

    #[test]
    fn fast_fold_agrees_with_unicode_tables() {
        for c in ('\0'..='\u{2FF}').chain('\u{400}'..='\u{4FF}') {
            if let Some(folded) = fold_char_fast(c) {
                let expected: Vec<char> = c.to_lowercase().collect();
                assert_eq!(vec![folded], expected, "mismatch for {c:?}");
            }
        }
    }

    #[test]
    fn fold_lower_matches_std_to_lowercase() {
        for s in [
            "Процедура",
            "PROCEDURE",
            "Ёлка123_Test",
            "ОДОΣ",      // Greek triggers the fallback: contextual final sigma
            "ΟΔΟΣ ΟΔΟΣ", // word-final position matters
            "İstanbul",  // multi-char expansion
            "",
            "ß",
        ] {
            assert_eq!(s.fold_lower(), s.to_lowercase(), "mismatch for {s:?}");
        }
    }

    #[test]
    fn per_char_fold_key_agrees_with_eq_ignore_case() {
        // Same key <=> eq_ignore_case, including where contextual
        // `to_lowercase` (and therefore `fold_lower`) disagrees.
        let cases =
            [("Процедура", "ПРОЦЕДУРА"), ("İ", "i\u{307}"), ("ΟΔΟΣ", "οδοσ"), ("ΟΔΟΣ", "οδος")];
        for (a, b) in cases {
            assert_eq!(
                fold_lower_per_char(a) == fold_lower_per_char(b),
                eq_ignore_case(a, b),
                "key/eq mismatch for {a:?} vs {b:?}"
            );
        }
        assert_ne!("ΟΔΟΣ".fold_lower(), fold_lower_per_char("ΟΔΟΣ"), "final sigma is contextual");
    }

    #[test]
    fn slow_path_handles_multichar_expansion() {
        // 'İ' lowercases to two chars; the streams stay aligned.
        assert!(eq_ignore_case("İ", "i\u{307}"));
        assert!(eq_ignore_case("xİ", "xi\u{307}"));
        assert!(!eq_ignore_case("İ", "i"));
        // Final sigma keeps per-char fold semantics (no contextual mapping).
        assert!(!eq_ignore_case("ΟΔΟΣ", "οδος"));
        assert!(eq_ignore_case("ΟΔΟΣ", "οδοσ"));
    }
}
