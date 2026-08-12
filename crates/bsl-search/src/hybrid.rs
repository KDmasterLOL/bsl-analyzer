//! Cross-modality fusion of lexical and semantic search results.
//!
//! Unlike [`crate::merge`] (which combines baseline + overlay *within* one modality),
//! this fuses the two *different* modalities — lexical FTS rank and semantic cosine
//! score — whose score scales are not comparable.
//!
//! Blind Reciprocal Rank Fusion (interleaving the two lists by `1/(k+rank)`) was measured to
//! dilute both regimes: it buried an exact symbol match under unrelated lexical neighbours and
//! drowned the semantic answer to a natural-language query in lexical noise. [`fuse_smart`]
//! instead self-gates on query shape: an identifier-shaped query floats its exact symbol matches
//! to the top, a natural-language query falls straight through to semantic order, with no
//! classifier and one rule.

use crate::workspace_roots::FileKey;
use std::collections::{HashMap, HashSet};

use crate::engine::SearchHit;

/// Which retrieval modalities surfaced a fused hit. `Both` — found by lexical AND semantic —
/// signals that the two independent rankers agreed on the chunk, which the caller surfaces as a
/// cross-modal-agreement tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modality {
    Lexical,
    Semantic,
    Both,
}

impl Modality {
    /// A compact agent-facing tag for the search listing.
    pub fn tag(self) -> &'static str {
        match self {
            Modality::Lexical => "L",
            Modality::Semantic => "S",
            Modality::Both => "L+S",
        }
    }
}

/// A fused search hit plus the modalities that surfaced it. `hit.score` is a synthetic,
/// strictly-decreasing-by-final-rank value (so any downstream stable sort preserves the fused
/// order); it is not displayed. `modality` is the legible cross-modal-agreement signal.
#[derive(Debug, Clone)]
pub struct FusedHit {
    pub hit: SearchHit,
    pub modality: Modality,
}

/// A hit's identity across modalities: the same chunk found by lexical and semantic search
/// must collapse to one fused result. Mirrors [`crate::merge`]'s dedup key.
type FusionKey = (String, FileKey, String, u32, u32);

fn fusion_key(hit: &SearchHit) -> FusionKey {
    (
        hit.collection.clone(),
        FileKey::new(&hit.root_id, &hit.file_path),
        hit.symbol_name.clone(),
        hit.line_start,
        hit.line_end,
    )
}

/// The Unicode script of a character for the Latin↔Cyrillic boundary test. Only the two scripts
/// BSL identifiers mix matter; everything else (digits, punctuation) is `0` and never triggers a
/// boundary on its own.
fn script_class(c: char) -> u8 {
    if c.is_ascii_alphabetic() {
        1 // Latin
    } else if ('\u{0400}'..='\u{04FF}').contains(&c) {
        2 // Cyrillic
    } else {
        0
    }
}

/// Strip leading/trailing non-alphanumeric characters from a query token so call syntax does not
/// defeat the shape test or symbol equality (`ПроверитьИНН()` → `ПроверитьИНН`). Internal
/// punctuation (a dotted call) is left intact — it simply never equals a bare `symbol_name`.
fn strip_punct(token: &str) -> &str {
    token.trim_matches(|c: char| !c.is_alphanumeric())
}

/// Whether a query token is identifier-shaped: it carries an *internal* transition that a natural
/// language word does not — a lower→upper camelCase hump, a Latin↔Cyrillic script change, a
/// letter↔digit boundary, or an internal underscore. A lone sentence-case word (`Записать`,
/// `Отправка`) has no such internal transition and is deliberately *not* identifier-shaped; that
/// is what keeps natural-language query terms out of the exact tier so they fall through to
/// semantic order. The token is expected to have its surrounding punctuation already stripped, so
/// any remaining underscore is internal (BSL identifiers admit `_`).
fn is_identifier_shaped(token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    chars.windows(2).any(|w| {
        let (a, b) = (w[0], w[1]);
        if a == '_' || b == '_' {
            return true;
        }
        if a.is_lowercase() && b.is_uppercase() {
            return true;
        }
        let (sa, sb) = (script_class(a), script_class(b));
        if sa != 0 && sb != 0 && sa != sb {
            return true;
        }
        (a.is_alphabetic() && b.is_numeric()) || (a.is_numeric() && b.is_alphabetic())
    })
}

/// Intent-self-gating fusion that replaces blind rank interleaving.
///
/// Both inputs are ranked, already-deduplicated, post-overlay-merge results of one modality
/// (lexical bm25 order, semantic cosine order). Two things are computed independently:
///
/// 1. **Identity & modality** — one keyed pass over BOTH full lists records, per fusion key, the
///    representative record and which modalities surfaced it. The representative prefers the
///    text-bearing lexical record (semantic Postgres hits carry empty `text`) so the snippet and
///    graph-id bridge survive. Modality is derived from full-list membership, before truncation.
/// 2. **Order** — `exact_tier ++ semantic ++ lexical_remainder`:
///    - `exact_tier`: lexical hits, in bm25 order, whose `symbol_name` equals an *identifier-shaped*
///      query term — or, when the whole query is a single token, equals that token even if it is
///      sentence-case (so `Записать`, `HTTP`, `ИНН`, `Форма2` still float). Empty symbol names
///      (module headers) never match.
///    - then the semantic list (the best tail for natural-language queries),
///    - then any lexical hit not yet placed.
///
/// A natural-language query has no identifier-shaped term and (being multi-token) does not trip
/// the single-token gate, so its exact tier is empty and ordering collapses to semantic-primary.
/// The final order decides neither representative identity nor modality. Capped at `limit`.
pub fn fuse_smart(
    lexical: &[SearchHit],
    semantic: &[SearchHit],
    query: &str,
    limit: usize,
) -> Vec<FusedHit> {
    // 1. Keyed accumulator over the FULL lists: representative + modality membership. Lexical is
    // absorbed first so its text-bearing record is the representative; a semantic-only key keeps
    // its own (empty-text) record.
    let mut acc: HashMap<FusionKey, (SearchHit, bool, bool)> = HashMap::new();
    let mut absorb = |list: &[SearchHit], is_lexical: bool| {
        let mut seen_in_list: HashSet<FusionKey> = HashSet::new();
        for hit in list {
            let key = fusion_key(hit);
            if !seen_in_list.insert(key.clone()) {
                continue;
            }
            acc.entry(key)
                .and_modify(|(rep, from_lex, from_sem)| {
                    if is_lexical {
                        *from_lex = true;
                    } else {
                        *from_sem = true;
                    }
                    if rep.text.is_empty() && !hit.text.is_empty() {
                        *rep = hit.clone();
                    }
                })
                .or_insert_with(|| (hit.clone(), is_lexical, !is_lexical));
        }
    };
    absorb(lexical, true);
    absorb(semantic, false);

    // 2. Exact-tier gate. Identifier-shaped terms always qualify; a single-token query also lets
    // that one token match a symbol by exact name even when it is sentence-case.
    let terms = crate::lexical::query_terms(query);
    let id_tokens: HashSet<&str> = terms
        .iter()
        .map(|t| strip_punct(t))
        .filter(|s| !s.is_empty() && is_identifier_shaped(s))
        .collect();
    // The single-token gate keys off the RAW word count, before the case-insensitive dedup in
    // `query_terms`: a repeated word (`Найти найти`) is still a multi-word phrase and must not be
    // treated as a lone-symbol query.
    let raw_word_count =
        query.split_whitespace().filter(|t| t.chars().any(char::is_alphanumeric)).count();
    let single_token: Option<String> = (raw_word_count == 1)
        .then(|| terms.first().map(|t| strip_punct(t).to_lowercase()))
        .flatten()
        .filter(|s| !s.is_empty());
    let is_exact_symbol = |symbol: &str| {
        if symbol.is_empty() {
            return false;
        }
        let lowered = symbol.to_lowercase();
        id_tokens.iter().any(|t| t.to_lowercase() == lowered)
            || single_token.as_deref() == Some(lowered.as_str())
    };

    // 3. Build the deterministic key order: exact tier, then semantic, then lexical remainder.
    let mut ordered: Vec<FusionKey> = Vec::with_capacity(acc.len());
    let mut placed: HashSet<FusionKey> = HashSet::new();
    for hit in lexical {
        if is_exact_symbol(&hit.symbol_name) {
            let key = fusion_key(hit);
            if placed.insert(key.clone()) {
                ordered.push(key);
            }
        }
    }
    for hit in semantic.iter().chain(lexical.iter()) {
        let key = fusion_key(hit);
        if placed.insert(key.clone()) {
            ordered.push(key);
        }
    }

    let total = ordered.len();
    ordered
        .into_iter()
        .take(limit)
        .enumerate()
        .filter_map(|(idx, key)| {
            acc.remove(&key).map(|(mut rep, from_lex, from_sem)| {
                // Strictly decreasing by final rank; not displayed, only keeps a downstream sort
                // stable.
                rep.score = (total - idx) as f32;
                let modality = match (from_lex, from_sem) {
                    (true, true) => Modality::Both,
                    (false, true) => Modality::Semantic,
                    _ => Modality::Lexical,
                };
                FusedHit { hit: rep, modality }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lexical(path: &str, symbol: &str) -> SearchHit {
        SearchHit {
            collection: "code".to_owned(),
            root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
            file_path: path.to_owned(),
            symbol_name: symbol.to_owned(),
            kind: "procedure".to_owned(),
            text: format!("procedure {symbol}"),
            line_start: 1,
            line_end: 10,
            score: 0.0,
        }
    }

    fn semantic(path: &str, symbol: &str) -> SearchHit {
        SearchHit { text: String::new(), ..lexical(path, symbol) }
    }

    fn keys(hits: &[FusedHit]) -> Vec<&str> {
        hits.iter().map(|h| h.hit.symbol_name.as_str()).collect()
    }

    #[test]
    fn identifier_shape_classifies_internal_transitions() {
        // camelCase hump, Latin↔Cyrillic change, letter↔digit boundary → identifier-shaped.
        assert!(is_identifier_shaped("ВызватьHTTPМетод"));
        assert!(is_identifier_shaped("getValue"));
        assert!(is_identifier_shaped("HTTPМетод"));
        assert!(is_identifier_shaped("Форма2"));
        // Lone sentence-case / lowercase words have no internal transition.
        assert!(!is_identifier_shaped("Записать"));
        assert!(!is_identifier_shaped("отправка"));
        // An all-caps acronym has no internal transition either; it relies on the single-token gate.
        assert!(!is_identifier_shaped("HTTP"));
        assert!(!is_identifier_shaped("ИНН"));
        // An internal underscore (a valid BSL identifier separator) is a transition NL lacks.
        assert!(is_identifier_shaped("Проверить_ИНН"));
    }

    #[test]
    fn repeated_word_is_not_a_single_token_query() {
        // `Найти найти` dedups to one term but is a multi-word phrase: it must NOT trip the
        // single-token gate, so a sentence-case symbol does not float.
        let lex = vec![lexical("a.bsl", "Прочее"), lexical("z.bsl", "Найти")];
        let fused = fuse_smart(&lex, &[], "Найти найти", 10);
        // No exact tier (multi-word, no identifier-shaped term) → lexical order is preserved.
        assert_eq!(keys(&fused), vec!["Прочее", "Найти"]);
    }

    #[test]
    fn underscore_identifier_floats_in_multi_word_query() {
        let lex = vec![lexical("a.bsl", "Прочее"), lexical("b.bsl", "Проверить_ИНН")];
        let fused = fuse_smart(&lex, &[], "вызвать Проверить_ИНН тут", 10);
        assert_eq!(fused[0].hit.symbol_name, "Проверить_ИНН");
    }

    #[test]
    fn exact_tier_floats_camelcase_match_above_higher_bm25_hit() {
        // `Other` is rank-1 lexically but is not the queried identifier; the exact symbol match
        // must float to the top even though it is lower in bm25 order.
        let lex = vec![lexical("a.bsl", "Other"), lexical("b.bsl", "ВызватьHTTPМетод")];
        let fused = fuse_smart(&lex, &[], "ВызватьHTTPМетод", 10);
        assert_eq!(fused[0].hit.symbol_name, "ВызватьHTTPМетод");
        assert_eq!(keys(&fused), vec!["ВызватьHTTPМетод", "Other"]);
    }

    #[test]
    fn single_token_gate_floats_sentence_case_symbol() {
        // `Записать` is not identifier-shaped, but a single-token query equal to a symbol name
        // still floats it (covers method names like `Записать`/`HTTP`/`ИНН`).
        let lex = vec![lexical("a.bsl", "ПодготовитьДанные"), lexical("z.bsl", "Записать")];
        let fused = fuse_smart(&lex, &[], "Записать", 10);
        assert_eq!(fused[0].hit.symbol_name, "Записать");
    }

    #[test]
    fn natural_language_query_leads_with_semantic_order() {
        // Multi-token, no identifier-shaped term → exact tier empty → semantic order leads, then
        // the lexical remainder.
        let lex = vec![lexical("a.bsl", "Запись"), lexical("b.bsl", "Курс")];
        let sem = vec![semantic("s.bsl", "ПолучитьКурсВалюты"), semantic("a.bsl", "Запись")];
        let fused = fuse_smart(&lex, &sem, "получить курс валюты на дату", 10);
        assert_eq!(fused[0].hit.symbol_name, "ПолучитьКурсВалюты");
        // `Запись` is in both lists → its representative carries lexical text and is tagged Both.
        let zapis = fused.iter().find(|h| h.hit.symbol_name == "Запись").unwrap();
        assert_eq!(zapis.modality, Modality::Both);
    }

    #[test]
    fn collision_keeps_lexical_text_and_tags_both() {
        let lex = vec![lexical("s.bsl", "Shared")];
        let sem = vec![semantic("s.bsl", "Shared")];
        let fused = fuse_smart(&lex, &sem, "получить общие данные", 10);
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].modality, Modality::Both);
        // The text-bearing lexical record won as the representative.
        assert_eq!(fused[0].hit.text, "procedure Shared");
    }

    #[test]
    fn empty_semantic_degrades_to_exact_then_lexical() {
        // Degrade path: no semantic list. The exact identifier still floats; the rest keeps
        // lexical order.
        let lex = vec![
            lexical("a.bsl", "Прочее"),
            lexical("b.bsl", "ПроверитьИНН"),
            lexical("c.bsl", "Ещё"),
        ];
        let fused = fuse_smart(&lex, &[], "ПроверитьИНН()", 10);
        assert_eq!(keys(&fused), vec!["ПроверитьИНН", "Прочее", "Ещё"]);
        assert!(fused.iter().all(|h| h.modality == Modality::Lexical));
    }

    #[test]
    fn semantic_only_hit_surfaces_without_text() {
        let lex = vec![lexical("a.bsl", "LexOnly")];
        let sem = vec![semantic("b.bsl", "SemOnly")];
        let fused = fuse_smart(&lex, &sem, "найти что-нибудь полезное", 10);
        let sem_hit = fused.iter().find(|h| h.hit.symbol_name == "SemOnly").unwrap();
        assert_eq!(sem_hit.modality, Modality::Semantic);
        assert_eq!(sem_hit.hit.text, "");
    }

    #[test]
    fn limit_truncates_after_ordering() {
        let lex = vec![lexical("a.bsl", "P1"), lexical("b.bsl", "P2"), lexical("c.bsl", "P3")];
        let fused = fuse_smart(&lex, &[], "несколько разных слов", 2);
        assert_eq!(fused.len(), 2);
    }

    #[test]
    fn duplicate_within_list_collapses_to_one_hit() {
        let lex = vec![lexical("t.bsl", "Top"), lexical("a.bsl", "Dup"), lexical("a.bsl", "Dup")];
        let fused = fuse_smart(&lex, &[], "несколько слов запроса", 10);
        assert_eq!(fused.len(), 2);
        assert_eq!(keys(&fused), vec!["Top", "Dup"]);
    }
}
