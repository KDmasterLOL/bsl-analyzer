//! Cross-modality fusion of lexical and semantic search results.
//!
//! Unlike [`crate::merge`] (which combines baseline + overlay *within* one modality),
//! this fuses the two *different* modalities — lexical FTS rank and semantic cosine
//! score — whose score scales are not comparable. Reciprocal Rank Fusion (RRF) sidesteps
//! that: it uses only each hit's rank within its own list, so no score calibration is
//! needed, and a hit ranked highly by both modalities is naturally boosted.

use std::collections::HashMap;

use crate::engine::SearchHit;

/// The standard RRF damping constant. Larger `k` flattens the contribution of top ranks,
/// making the fusion less sensitive to any single modality's exact ordering.
pub const RRF_K: f32 = 60.0;

/// Which retrieval modalities surfaced a fused hit. `Both` — found by lexical AND semantic —
/// is the strongest signal (the two independent rankers agreed), which a bare RRF score (a
/// small float clustered near `1/(k+1)`) does not make legible to the caller.
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

/// A fused search hit plus the modalities that surfaced it. The fused RRF score remains on
/// `hit.score` (it drives ordering); `modality` is the legible cross-modal-agreement signal.
#[derive(Debug, Clone)]
pub struct FusedHit {
    pub hit: SearchHit,
    pub modality: Modality,
}

/// A hit's identity across modalities: the same chunk found by lexical and semantic search
/// must collapse to one fused result. Mirrors [`crate::merge`]'s dedup key.
type FusionKey = (String, String, String, u32, u32);

fn fusion_key(hit: &SearchHit) -> FusionKey {
    (
        hit.collection.clone(),
        hit.file_path.clone(),
        hit.symbol_name.clone(),
        hit.line_start,
        hit.line_end,
    )
}

/// Fuse two ranked, already-deduplicated hit lists (each the final post-overlay-merge result
/// of one modality) into one list ordered by RRF score, capped at `limit`.
///
/// Each input list is treated as ranked by its position (rank 1 = first). A hit's fused score
/// is the sum of `1/(k + rank)` over the modalities that returned it, so a hit present in both
/// lists outranks one present in only a higher position of a single list. When the same hit
/// appears in both lists the lexical record is kept as the representative, because it carries
/// the source `text` (semantic hits do not) that the caller needs for the snippet and the
/// graph-id bridge. The fused score replaces the per-modality score so it is what callers
/// display. Ties break on the fusion key for determinism, independent of input order.
pub fn fuse_rrf(
    lexical: &[SearchHit],
    semantic: &[SearchHit],
    k: f32,
    limit: usize,
) -> Vec<FusedHit> {
    // Representative record per key, its accumulated RRF score, and which modalities surfaced
    // it (`from_lexical`, `from_semantic`). Lexical is inserted first so its (text-bearing)
    // record wins as the representative on collision.
    let mut acc: HashMap<FusionKey, (SearchHit, f32, bool, bool)> = HashMap::new();

    let mut absorb = |list: &[SearchHit], is_lexical: bool, prefer_existing_text: bool| {
        // A key that repeats within one list contributes only its best (first) rank — the
        // engine direct paths do not contractually dedup, and double-counting one document's
        // rank would distort the fused order. Rank advances only on a first-seen key.
        let mut seen_in_list: std::collections::HashSet<FusionKey> =
            std::collections::HashSet::new();
        let mut rank = 0usize;
        for hit in list {
            let key = fusion_key(hit);
            if !seen_in_list.insert(key.clone()) {
                continue;
            }
            let contribution = 1.0 / (k + (rank as f32) + 1.0);
            rank += 1;
            acc.entry(key)
                .and_modify(|(rep, score, from_lex, from_sem)| {
                    *score += contribution;
                    if is_lexical {
                        *from_lex = true;
                    } else {
                        *from_sem = true;
                    }
                    // Keep whichever record carries a snippet; the lexical pass runs first,
                    // so this only replaces an empty-text semantic representative.
                    if !prefer_existing_text && rep.text.is_empty() && !hit.text.is_empty() {
                        *rep = hit.clone();
                    }
                })
                .or_insert_with(|| (hit.clone(), contribution, is_lexical, !is_lexical));
        }
    };

    absorb(lexical, true, true);
    absorb(semantic, false, false);

    let mut fused: Vec<FusedHit> = acc
        .into_iter()
        .map(|(_, (mut rep, score, from_lex, from_sem))| {
            rep.score = score;
            let modality = match (from_lex, from_sem) {
                (true, true) => Modality::Both,
                (false, true) => Modality::Semantic,
                _ => Modality::Lexical,
            };
            FusedHit { hit: rep, modality }
        })
        .collect();

    fused.sort_by(|a, b| {
        b.hit
            .score
            .total_cmp(&a.hit.score)
            .then_with(|| fusion_key(&a.hit).cmp(&fusion_key(&b.hit)))
    });
    fused.truncate(limit);
    fused
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lexical(path: &str, symbol: &str) -> SearchHit {
        SearchHit {
            collection: "code".to_owned(),
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
    fn hit_in_both_lists_outranks_single_list_hits() {
        // `Shared` is rank-2 lexically and rank-2 semantically; each single-list hit is
        // rank-1 in its own list. The doubled contribution lifts `Shared` to the top.
        let lex = vec![lexical("a.bsl", "LexTop"), lexical("s.bsl", "Shared")];
        let sem = vec![semantic("b.bsl", "SemTop"), semantic("s.bsl", "Shared")];
        let fused = fuse_rrf(&lex, &sem, RRF_K, 10);
        assert_eq!(fused[0].hit.symbol_name, "Shared");
        // Found by both modalities — the strongest signal.
        assert_eq!(fused[0].modality, Modality::Both);
        // The representative kept the lexical record, so the snippet survives.
        assert_eq!(fused[0].hit.text, "procedure Shared");
        assert_eq!(fused.len(), 3);
    }

    #[test]
    fn semantic_only_hit_surfaces() {
        let lex = vec![lexical("a.bsl", "LexOnly")];
        let sem = vec![semantic("b.bsl", "SemOnly")];
        let fused = fuse_rrf(&lex, &sem, RRF_K, 10);
        // Both rank-1 in their own list → equal score → key tie-break (collection,path,...):
        // "a.bsl" < "b.bsl".
        assert_eq!(keys(&fused), vec!["LexOnly", "SemOnly"]);
        assert_eq!(fused[0].modality, Modality::Lexical);
        assert_eq!(fused[1].modality, Modality::Semantic);
        // A semantic-only hit has no snippet text.
        assert_eq!(fused[1].hit.text, "");
    }

    #[test]
    fn empty_semantic_degrades_to_lexical_order() {
        let lex = vec![lexical("a.bsl", "P1"), lexical("b.bsl", "P2"), lexical("c.bsl", "P3")];
        let fused = fuse_rrf(&lex, &[], RRF_K, 10);
        assert_eq!(keys(&fused), vec!["P1", "P2", "P3"]);
        assert!(fused.iter().all(|h| h.modality == Modality::Lexical));
    }

    #[test]
    fn limit_truncates_after_fusion() {
        let lex = vec![lexical("a.bsl", "P1"), lexical("b.bsl", "P2"), lexical("c.bsl", "P3")];
        let fused = fuse_rrf(&lex, &[], RRF_K, 2);
        assert_eq!(keys(&fused), vec!["P1", "P2"]);
    }

    #[test]
    fn duplicate_within_list_contributes_only_best_rank() {
        // `Dup` appears twice (ranks 1 and 2) but `Top` is the genuine rank-1 hit. If the
        // duplicate double-counted, `Dup` would wrongly outrank `Top`; it must not.
        let lex = vec![lexical("t.bsl", "Top"), lexical("a.bsl", "Dup"), lexical("a.bsl", "Dup")];
        let fused = fuse_rrf(&lex, &[], RRF_K, 10);
        assert_eq!(fused.len(), 2);
        assert_eq!(keys(&fused), vec!["Top", "Dup"]);
        // `Top` (rank 1) and `Dup` (best rank 2) keep their single-occurrence RRF scores.
        assert_eq!(fused[0].hit.score, 1.0 / (RRF_K + 1.0));
        assert_eq!(fused[1].hit.score, 1.0 / (RRF_K + 2.0));
    }
}
