//! Full-text search index for platform documentation.
//!
//! Uses Snowball stemming (Russian) with an inverted index for fast lookup
//! by description text, method/type names, and parameter names.

use crate::generated;
use crate::types::{GlobalFunction, PlatformMethod, PlatformType};
use rust_stemmers::{Algorithm, Stemmer};
use rustc_hash::{FxHashMap, FxHashSet};
use smol_str::SmolStr;

const MAX_RESULTS: usize = 10;

/// What kind of platform entity a search result refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocKind {
    Type,
    Method,
    GlobalFunction,
}

/// A single search result with relevance score.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub kind: DocKind,
    pub index: usize,
    pub score: f32,
}

/// Full-text search index over platform documentation.
pub struct SearchIndex {
    /// stem -> [(doc_kind, doc_index, field_weight)]
    inverted: FxHashMap<SmolStr, Vec<(DocKind, usize, f32)>>,
    stemmer: Stemmer,
}

// Field weights for ranking
const WEIGHT_NAME: f32 = 3.0;
const WEIGHT_DESCRIPTION: f32 = 1.0;
const WEIGHT_PARAMS: f32 = 0.5;
const WEIGHT_SEE_ALSO: f32 = 0.5;

impl SearchIndex {
    /// Build the search index from platform data.
    pub fn new(
        types: &[PlatformType],
        methods: &[PlatformMethod],
        global_functions: &[GlobalFunction],
    ) -> Self {
        let stemmer = Stemmer::create(Algorithm::Russian);
        let mut inverted: FxHashMap<SmolStr, Vec<(DocKind, usize, f32)>> = FxHashMap::default();

        // Index type names
        for (idx, ty) in types.iter().enumerate() {
            Self::index_text(&stemmer, &mut inverted, &ty.name, DocKind::Type, idx, WEIGHT_NAME);
            Self::index_text(
                &stemmer,
                &mut inverted,
                &ty.english_name,
                DocKind::Type,
                idx,
                WEIGHT_NAME,
            );
        }

        // Index methods: names + docs
        for (idx, method) in methods.iter().enumerate() {
            Self::index_text(
                &stemmer,
                &mut inverted,
                &method.name,
                DocKind::Method,
                idx,
                WEIGHT_NAME,
            );
            Self::index_text(
                &stemmer,
                &mut inverted,
                &method.english_name,
                DocKind::Method,
                idx,
                WEIGHT_NAME,
            );
            // Index type name with lower weight so "Массив" boosts array methods
            Self::index_text(
                &stemmer,
                &mut inverted,
                &method.type_name,
                DocKind::Method,
                idx,
                WEIGHT_PARAMS,
            );
        }

        // Index method docs (descriptions, params, see_also)
        for raw_docs in generated::METHOD_DOCS.iter() {
            // Find method index by id
            if let Some(method_idx) = methods.iter().position(|m| m.id == raw_docs.method_id) {
                Self::index_text(
                    &stemmer,
                    &mut inverted,
                    raw_docs.description,
                    DocKind::Method,
                    method_idx,
                    WEIGHT_DESCRIPTION,
                );
                for param in raw_docs.params.iter() {
                    Self::index_text(
                        &stemmer,
                        &mut inverted,
                        param.name,
                        DocKind::Method,
                        method_idx,
                        WEIGHT_PARAMS,
                    );
                    Self::index_text(
                        &stemmer,
                        &mut inverted,
                        param.description,
                        DocKind::Method,
                        method_idx,
                        WEIGHT_PARAMS,
                    );
                }
                for sa in raw_docs.see_also.iter() {
                    Self::index_text(
                        &stemmer,
                        &mut inverted,
                        sa,
                        DocKind::Method,
                        method_idx,
                        WEIGHT_SEE_ALSO,
                    );
                }
            }
        }

        // Index global functions: names + docs
        for (idx, func) in global_functions.iter().enumerate() {
            Self::index_text(
                &stemmer,
                &mut inverted,
                &func.name,
                DocKind::GlobalFunction,
                idx,
                WEIGHT_NAME,
            );
            Self::index_text(
                &stemmer,
                &mut inverted,
                &func.english_name,
                DocKind::GlobalFunction,
                idx,
                WEIGHT_NAME,
            );
        }

        // Index global function docs
        for raw_docs in generated::GLOBAL_FUNCTION_DOCS.iter() {
            if let Some(func_idx) = global_functions.iter().position(|f| f.id == raw_docs.method_id)
            {
                Self::index_text(
                    &stemmer,
                    &mut inverted,
                    raw_docs.description,
                    DocKind::GlobalFunction,
                    func_idx,
                    WEIGHT_DESCRIPTION,
                );
                for param in raw_docs.params.iter() {
                    Self::index_text(
                        &stemmer,
                        &mut inverted,
                        param.name,
                        DocKind::GlobalFunction,
                        func_idx,
                        WEIGHT_PARAMS,
                    );
                    Self::index_text(
                        &stemmer,
                        &mut inverted,
                        param.description,
                        DocKind::GlobalFunction,
                        func_idx,
                        WEIGHT_PARAMS,
                    );
                }
                for sa in raw_docs.see_also.iter() {
                    Self::index_text(
                        &stemmer,
                        &mut inverted,
                        sa,
                        DocKind::GlobalFunction,
                        func_idx,
                        WEIGHT_SEE_ALSO,
                    );
                }
            }
        }

        Self { inverted, stemmer }
    }

    /// Search the index and return top results sorted by relevance.
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let query_words = split_words(query);
        let stems: Vec<SmolStr> = query_words
            .iter()
            .filter(|w| w.len() >= 2)
            .map(|word| SmolStr::from(self.stemmer.stem(word).as_ref()))
            .collect();

        if stems.is_empty() {
            return vec![];
        }

        // Accumulate scores: (DocKind, index) -> total_score
        let mut scores: FxHashMap<(DocKind, usize), f32> = FxHashMap::default();

        for stem in &stems {
            if let Some(postings) = self.inverted.get(stem) {
                for &(kind, idx, weight) in postings {
                    *scores.entry((kind, idx)).or_default() += weight;
                }
            }
        }

        // Boost documents that match more query terms
        let num_stems = stems.len() as f32;
        let mut term_hits: FxHashMap<(DocKind, usize), usize> = FxHashMap::default();
        for stem in &stems {
            if let Some(postings) = self.inverted.get(stem) {
                for &(kind, idx, _) in postings {
                    *term_hits.entry((kind, idx)).or_default() += 1;
                }
            }
        }

        // Coverage bonus: documents matching all query terms get a boost
        for ((kind, idx), hits) in &term_hits {
            let coverage = *hits as f32 / num_stems;
            if let Some(score) = scores.get_mut(&(*kind, *idx)) {
                *score *= 1.0 + coverage;
            }
        }

        let mut results: Vec<SearchResult> = scores
            .into_iter()
            .map(|((kind, index), score)| SearchResult { kind, index, score })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(MAX_RESULTS);
        results
    }

    fn index_text(
        stemmer: &Stemmer,
        inverted: &mut FxHashMap<SmolStr, Vec<(DocKind, usize, f32)>>,
        text: &str,
        kind: DocKind,
        index: usize,
        weight: f32,
    ) {
        let mut seen = FxHashSet::default();
        for word in split_words(text) {
            if word.len() < 2 {
                continue;
            }
            let stem = SmolStr::from(stemmer.stem(&word).as_ref());
            if seen.insert(stem.clone()) {
                inverted.entry(stem).or_default().push((kind, index, weight));
            }
        }
    }
}

/// Split text into lowercase words, handling CamelCase identifiers.
///
/// "ТекущаяДата" → ["текущая", "дата"]
/// "НачатьТранзакцию" → ["начать", "транзакцию"]
/// "Обычный текст, разделённый пробелами" → ["обычный", "текст", "разделённый", "пробелами"]
fn split_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();

    // First split on non-alphanumeric boundaries
    for token in text.split(|c: char| !c.is_alphanumeric()) {
        if token.is_empty() {
            continue;
        }
        // Split CamelCase: detect transitions from lowercase to uppercase
        let mut current = String::new();
        let mut prev_lower = false;
        for ch in token.chars() {
            if prev_lower && ch.is_uppercase() && !current.is_empty() {
                words.push(current.to_lowercase());
                current = String::new();
            }
            current.push(ch);
            prev_lower = ch.is_lowercase();
        }
        if !current.is_empty() {
            words.push(current.to_lowercase());
        }
    }

    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlatformDataInner;

    #[test]
    fn test_search_by_name() {
        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        let results = data.search("Массив");
        assert!(!results.is_empty(), "Should find results for 'Массив'");
        // Type "Массив" should be among top results
        assert!(
            results.iter().any(|r| r.kind == DocKind::Type),
            "Should find a type result for 'Массив'"
        );
    }

    #[test]
    fn test_search_by_description() {
        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        let results = data.search("сортировка");
        assert!(!results.is_empty(), "Should find results for 'сортировка'");
    }

    #[test]
    fn test_search_multi_word() {
        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        let results = data.search("текущая дата время");
        assert!(!results.is_empty(), "Should find results for date/time query");
    }

    #[test]
    fn test_search_empty_query() {
        let data = PlatformDataInner::instance();
        let results = data.search("");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_result_limit() {
        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        let results = data.search("значение");
        assert!(results.len() <= MAX_RESULTS);
    }
}
