//! Fuzzy matching and match-quality scoring for unqualified completion.
//!
//! Mirrors the RDT1C autocomplete model where the dominant ordering key is the
//! *quality* of how the typed text hit the candidate (their `ПремияФильтра`):
//! an exact prefix beats a CamelCase sub-word hit, which beats an interior
//! substring, which beats a scattered subsequence. The actual subsequence
//! matching and intra-tier scoring is delegated to `nucleo-matcher`; the tier is
//! derived from the match indices it reports.

use nucleo_matcher::{Config, Matcher, Utf32Str};
use stdx::case::CaseExt;

/// Quality of a match, dominant completion sort key. Lower is better, so the
/// numeric value can be emitted directly into an LSP `sort_text` (sorted
/// lexicographically ascending by the client).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) enum MatchTier {
    /// Candidate starts with the typed text (`Спр` → `Справочники`).
    Prefix = 0,
    /// Contiguous match starting at a sub-word boundary — CamelCase hump or the
    /// char after a separator (`ОН` → `ОбщегоНазначения`).
    WordBoundary = 1,
    /// Contiguous match inside a sub-word.
    Substring = 2,
    /// Scattered subsequence.
    Fuzzy = 3,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct MatchResult {
    pub tier: MatchTier,
    /// Raw `nucleo` score, higher is a better match; used as an intra-tier
    /// tiebreak.
    pub score: u16,
}

/// A matcher bound to a single typed prefix, reused across all candidates of one
/// completion request. An empty prefix matches everything at [`MatchTier::Prefix`].
pub(super) struct PrefixMatcher {
    matcher: Matcher,
    needle_lower: String,
    needle_chars: Vec<char>,
    empty: bool,
}

impl PrefixMatcher {
    pub(super) fn new(prefix: &str) -> Self {
        let needle_lower = prefix.fold_lower();
        let needle_chars = needle_lower.chars().collect();
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            empty: needle_lower.is_empty(),
            needle_lower,
            needle_chars,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.empty
    }

    /// Cheap, allocation-light gate used at the candidate source to bound the set
    /// before precise scoring. A case-insensitive subsequence test: it admits
    /// exactly what [`PrefixMatcher::score`] would later match, so it never drops
    /// a valid candidate.
    pub(super) fn admits(&self, candidate: &str) -> bool {
        if self.empty {
            return true;
        }
        let mut needle = self.needle_chars.iter().copied().peekable();
        for hc in candidate.chars().flat_map(|c| c.to_lowercase()) {
            match needle.peek() {
                Some(&nc) if nc == hc => {
                    needle.next();
                }
                Some(_) => {}
                None => return true,
            }
        }
        needle.peek().is_none()
    }

    /// Stricter gate requiring the typed text as a contiguous substring (still
    /// covers prefix and CamelCase-contiguous hits, drops scattered ones). Used
    /// for the large platform / metadata candidate sets, where scattered fuzzy
    /// matches on a short prefix would flood the list with noise.
    pub(super) fn admits_contiguous(&self, candidate: &str) -> bool {
        if self.empty {
            return true;
        }
        candidate.fold_lower().contains(self.needle_lower.as_str())
    }

    /// Bilingual variant of [`PrefixMatcher::admits_contiguous`].
    pub(super) fn admits_contiguous_bilingual(&self, ru: &str, en: &str) -> bool {
        self.admits_contiguous(ru) || self.admits_contiguous(en)
    }

    /// Precise score + quality tier for a candidate. `None` when the typed text
    /// is not a subsequence of the candidate.
    pub(super) fn score(&mut self, candidate: &str) -> Option<MatchResult> {
        if self.empty {
            return Some(MatchResult { tier: MatchTier::Prefix, score: 0 });
        }

        let hay_lower = candidate.fold_lower();
        let mut hay_buf = Vec::new();
        let mut needle_buf = Vec::new();
        let hay = Utf32Str::new(&hay_lower, &mut hay_buf);
        let needle = Utf32Str::new(&self.needle_lower, &mut needle_buf);

        let mut indices = Vec::new();
        let score = self.matcher.fuzzy_indices(hay, needle, &mut indices)?;
        indices.sort_unstable();

        let tier = classify_tier(candidate, &indices);
        Some(MatchResult { tier, score })
    }
}

/// Score a completion candidate by its label and any bilingual aliases carried in
/// `filter_text` (platform/metadata items store `"<ru> <en>"`, possibly multi-word),
/// keeping the best tier. The whole `filter_text` is scored first so a contiguous
/// match that spans a space is never missed; individual tokens are scored too so a
/// single word starting with the input still earns the tighter [`MatchTier::Prefix`].
/// `None` when the typed text matches neither the label nor any alias.
pub(super) fn score_item(
    matcher: &mut PrefixMatcher,
    label: &str,
    filter_text: Option<&str>,
) -> Option<MatchResult> {
    let mut best = matcher.score(label);
    if let Some(filter) = filter_text {
        best = fold_best(matcher, best, filter);
        for alias in filter.split_whitespace() {
            best = fold_best(matcher, best, alias);
        }
    }
    best
}

/// Keep whichever of `best`/`score(text)` is the stronger match: lower tier wins;
/// on an equal tier, the higher raw score wins.
fn fold_best(
    matcher: &mut PrefixMatcher,
    best: Option<MatchResult>,
    text: &str,
) -> Option<MatchResult> {
    let Some(candidate) = matcher.score(text) else {
        return best;
    };
    match best {
        Some(current)
            if current.tier < candidate.tier
                || (current.tier == candidate.tier && current.score >= candidate.score) =>
        {
            Some(current)
        }
        _ => Some(candidate),
    }
}

/// Derive the quality tier from the matched character positions in the original
/// (case-preserving) candidate.
fn classify_tier(candidate: &str, indices: &[u32]) -> MatchTier {
    let Some(&first) = indices.first() else {
        return MatchTier::Fuzzy;
    };

    let contiguous = indices.windows(2).all(|w| w[1] == w[0] + 1);
    if !contiguous {
        return MatchTier::Fuzzy;
    }
    if first == 0 {
        return MatchTier::Prefix;
    }

    let chars: Vec<char> = candidate.chars().collect();
    let pos = first as usize;
    if pos < chars.len() && is_subword_start(&chars, pos) {
        MatchTier::WordBoundary
    } else {
        MatchTier::Substring
    }
}

/// A char begins a sub-word if it is an uppercase hump after a non-uppercase
/// char (CamelCase) or follows a non-alphanumeric separator.
fn is_subword_start(chars: &[char], pos: usize) -> bool {
    if pos == 0 {
        return true;
    }
    let cur = chars[pos];
    let prev = chars[pos - 1];
    (cur.is_uppercase() && !prev.is_uppercase()) || !prev.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tier(prefix: &str, candidate: &str) -> Option<MatchTier> {
        PrefixMatcher::new(prefix).score(candidate).map(|r| r.tier)
    }

    #[test]
    fn empty_prefix_matches_everything_as_prefix() {
        assert_eq!(tier("", "Справочники"), Some(MatchTier::Prefix));
    }

    #[test]
    fn exact_prefix_is_prefix_tier_case_insensitive() {
        assert_eq!(tier("спр", "Справочники"), Some(MatchTier::Prefix));
        assert_eq!(tier("СПР", "Справочники"), Some(MatchTier::Prefix));
        assert_eq!(tier("Спр", "Справочники"), Some(MatchTier::Prefix));
    }

    #[test]
    fn admits_contiguous_drops_scattered() {
        let m = PrefixMatcher::new("есл");
        assert!(!m.admits_contiguous("Перечисления"));
        assert!(m.admits_contiguous("ВводЕслиПусто"));
        assert!(m.admits("Перечисления"), "subsequence gate still admits scattered");
    }

    #[test]
    fn camel_case_abbreviation_is_word_boundary() {
        // `ОН` hits the `О`...`Н` humps of `ОбщегоНазначения` only as a
        // subsequence, but the contiguous interior hit `Назначения` start is a
        // boundary.
        assert_eq!(tier("Назн", "ОбщегоНазначения"), Some(MatchTier::WordBoundary));
    }

    #[test]
    fn interior_contiguous_is_substring() {
        assert_eq!(tier("бщег", "ОбщегоНазначения"), Some(MatchTier::Substring));
    }

    #[test]
    fn scattered_is_fuzzy() {
        assert_eq!(tier("обн", "ОбщегоНазначения"), Some(MatchTier::Fuzzy));
    }

    #[test]
    fn non_subsequence_does_not_match() {
        assert_eq!(tier("xyz", "Справочники"), None);
    }

    #[test]
    fn admits_mirrors_score_match() {
        let m = PrefixMatcher::new("обн");
        assert!(m.admits("ОбщегоНазначения"));
        assert!(!m.admits("Справочники"));
    }
}
