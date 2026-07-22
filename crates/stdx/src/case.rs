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

/// Allocation-free case-insensitive substring search.
///
/// `needle_lower` must already be per-char-lowercase, i.e.
/// `needle_lower == fold_lower_per_char(needle_lower)` — a `debug_assert!`
/// enforces this. The haystack is folded on the fly, one char at a time (same
/// arithmetic fast path as [`fold_lower_per_char`], with the same Unicode
/// fallback for chars outside it), so no `String` is ever allocated for
/// either side. An empty `needle_lower` matches everywhere, like
/// [`str::contains`].
pub fn contains_ignore_case(haystack: &str, needle_lower: &str) -> bool {
    debug_assert_eq!(
        needle_lower,
        fold_lower_per_char(needle_lower),
        "needle_lower must already be per-char-lowercase"
    );

    let Some(needle_first) = needle_lower.chars().next() else {
        return true;
    };

    let mut window = haystack.chars();
    loop {
        let mut probe = window.clone();
        let Some(h) = probe.next() else { return false };

        // First-char filter: most candidate positions fail here, so the full
        // window comparison (and its multi-char-expansion bookkeeping) only
        // runs when it might actually match.
        if fold_one(h) == needle_first && window_matches(window.clone(), needle_lower) {
            return true;
        }

        window.next();
    }
}

/// Folds a single char the same way [`fold_lower_per_char`] would, for the
/// [`contains_ignore_case`] first-char filter (which only ever needs one
/// output char, never a multi-char expansion).
#[inline]
fn fold_one(c: char) -> char {
    fold_char_fast(c).unwrap_or_else(|| c.to_lowercase().next().unwrap_or(c))
}

/// Compares a haystack char window (starting at `hay`) against `needle`
/// (already per-char-lowercase) using per-char fold semantics. Handles a
/// haystack char whose lowercase mapping expands to multiple chars (e.g.
/// Turkish `İ`) by queuing the expansion's remaining chars in `pending`
/// before pulling the next haystack char — the same multi-char handling
/// `fold_lower_per_char` does via `String`, without allocating here.
fn window_matches(mut hay: std::str::Chars<'_>, needle: &str) -> bool {
    let mut needle_chars = needle.chars();
    let mut pending: Option<std::char::ToLowercase> = None;

    loop {
        let Some(expected) = needle_chars.next() else { return true };

        let actual = match pending.as_mut().and_then(|iter| iter.next()) {
            Some(c) => c,
            // `pending` was `None`, or its expansion is spent — either way,
            // clear it and pull the next haystack char.
            None => {
                pending = None;
                match hay.next() {
                    None => return false,
                    Some(h) => match fold_char_fast(h) {
                        Some(fc) => fc,
                        None => {
                            let mut lc = h.to_lowercase();
                            let first =
                                lc.next().expect("char::to_lowercase yields at least one char");
                            pending = Some(lc);
                            first
                        }
                    },
                }
            }
        };

        if actual != expected {
            return false;
        }
    }
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

    #[test]
    fn contains_ascii_mixed_case() {
        assert!(contains_ignore_case("Вызвать СтрШаблон(x)", "стршаблон"));
        assert!(contains_ignore_case("CALL StrTemplate(x)", "strtemplate"));
        assert!(contains_ignore_case("call STRTEMPLATE(x)", "strtemplate"));
        assert!(!contains_ignore_case("Вызвать СтрШаблон(x)", "strtemplate"));
        assert!(!contains_ignore_case("no match here", "strtemplate"));
    }

    #[test]
    fn contains_cyrillic_mixed_case() {
        assert!(contains_ignore_case("Процедура СтрШаблон", "стршаблон"));
        assert!(contains_ignore_case("ПРОЦЕДУРА СТРШАБЛОН", "стршаблон"));
        assert!(!contains_ignore_case("Процедура Иное", "стршаблон"));
    }

    #[test]
    fn contains_yo() {
        assert!(contains_ignore_case("нашёл Ёлку", "ёлку"));
        assert!(contains_ignore_case("нашёл ЁЛКУ", "ёлку"));
        assert!(!contains_ignore_case("нашёл Елку", "ёлку"));
    }

    #[test]
    fn contains_empty_needle_matches_everywhere() {
        // Matches `str::contains("")` semantics.
        assert!(contains_ignore_case("anything", ""));
        assert!(contains_ignore_case("", ""));
    }

    #[test]
    fn contains_needle_longer_than_haystack() {
        assert!(!contains_ignore_case("abc", "abcdef"));
        assert!(!contains_ignore_case("", "a"));
    }

    #[test]
    fn contains_pattern_at_end_of_text() {
        assert!(contains_ignore_case("вызвать СтрШаблон", "стршаблон"));
        assert!(contains_ignore_case("prefix STRTEMPLATE", "strtemplate"));
    }

    #[test]
    fn contains_agrees_with_fold_lower_per_char_contains() {
        let cases: &[(&str, &str)] = &[
            ("Вызвать СтрШаблон(x)", "стршаблон"),
            ("CALL StrTemplate(x)", "strtemplate"),
            ("no match here", "strtemplate"),
            ("нашёл Ёлку", "ёлку"),
            ("нашёл Елку", "ёлку"),
            ("Таблица1Row.Добавить()", "row"),
            ("", "a"),
            ("abc", ""),
        ];
        for (haystack, needle_lower) in cases {
            assert_eq!(
                contains_ignore_case(haystack, needle_lower),
                fold_lower_per_char(haystack).contains(needle_lower),
                "mismatch for haystack {haystack:?}, needle {needle_lower:?}"
            );
        }
    }

    #[test]
    fn contains_handles_multichar_expansion_in_haystack() {
        // 'İ' lowercases to two chars ("i" + combining dot above); the needle
        // must still be found across the expansion boundary.
        assert!(contains_ignore_case("xİy", "i\u{307}"));
        assert_eq!(
            contains_ignore_case("xİy", "i\u{307}"),
            fold_lower_per_char("xİy").contains("i\u{307}")
        );
    }
}
