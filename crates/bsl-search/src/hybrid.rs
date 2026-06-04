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
) -> Vec<SearchHit> {
    // Representative record per key, plus its accumulated RRF score. Lexical is inserted
    // first so its (text-bearing) record wins as the representative on collision.
    let mut acc: HashMap<FusionKey, (SearchHit, f32)> = HashMap::new();

    let mut absorb = |list: &[SearchHit], prefer_existing_text: bool| {
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
                .and_modify(|(rep, score)| {
                    *score += contribution;
                    // Keep whichever record carries a snippet; the lexical pass runs first,
                    // so this only replaces an empty-text semantic representative.
                    if !prefer_existing_text && rep.text.is_empty() && !hit.text.is_empty() {
                        *rep = hit.clone();
                    }
                })
                .or_insert_with(|| (hit.clone(), contribution));
        }
    };

    absorb(lexical, true);
    absorb(semantic, false);

    let mut fused: Vec<SearchHit> = acc
        .into_iter()
        .map(|(_, (mut rep, score))| {
            rep.score = score;
            rep
        })
        .collect();

    fused.sort_by(|a, b| {
        b.score.total_cmp(&a.score).then_with(|| fusion_key(a).cmp(&fusion_key(b)))
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

    fn keys(hits: &[SearchHit]) -> Vec<&str> {
        hits.iter().map(|h| h.symbol_name.as_str()).collect()
    }

    #[test]
    fn hit_in_both_lists_outranks_single_list_hits() {
        // `Shared` is rank-2 lexically and rank-2 semantically; each single-list hit is
        // rank-1 in its own list. The doubled contribution lifts `Shared` to the top.
        let lex = vec![lexical("a.bsl", "LexTop"), lexical("s.bsl", "Shared")];
        let sem = vec![semantic("b.bsl", "SemTop"), semantic("s.bsl", "Shared")];
        let fused = fuse_rrf(&lex, &sem, RRF_K, 10);
        assert_eq!(fused[0].symbol_name, "Shared");
        // The representative kept the lexical record, so the snippet survives.
        assert_eq!(fused[0].text, "procedure Shared");
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
        // A semantic-only hit has no snippet text.
        assert_eq!(fused[1].text, "");
    }

    #[test]
    fn empty_semantic_degrades_to_lexical_order() {
        let lex = vec![lexical("a.bsl", "P1"), lexical("b.bsl", "P2"), lexical("c.bsl", "P3")];
        let fused = fuse_rrf(&lex, &[], RRF_K, 10);
        assert_eq!(keys(&fused), vec!["P1", "P2", "P3"]);
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
        assert_eq!(fused[0].score, 1.0 / (RRF_K + 1.0));
        assert_eq!(fused[1].score, 1.0 / (RRF_K + 2.0));
    }
}
