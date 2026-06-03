use std::collections::HashSet;

use crate::domain::{LexicalHit, OverlayChange, SearchOverlay, SemanticHit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitSource {
    Baseline,
    Overlay,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MergedHit {
    pub collection: String,
    pub path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line_start: u32,
    pub line_end: u32,
    pub text: Option<String>,
    pub score: f32,
    pub source: HitSource,
}

pub struct MergeContext {
    pub hidden_paths: HashSet<(String, String)>,
}

pub fn build_merge_context(overlay: &SearchOverlay) -> MergeContext {
    let mut hidden_paths = HashSet::new();
    for change in &overlay.changes {
        match change {
            OverlayChange::DeleteFile(doc_path) => {
                hidden_paths.insert((doc_path.collection.clone(), doc_path.path.clone()));
            }
            OverlayChange::ReplaceFile(file_overlay) => {
                hidden_paths.insert((
                    file_overlay.target.collection.clone(),
                    file_overlay.target.path.clone(),
                ));
            }
        }
    }
    MergeContext { hidden_paths }
}

pub fn merge_context_for_collection(paths: &HashSet<String>, collection: &str) -> MergeContext {
    MergeContext {
        hidden_paths: paths.iter().map(|p| (collection.to_owned(), p.clone())).collect(),
    }
}

pub fn merge_lexical(
    baseline_hits: &[LexicalHit],
    overlay_hits: &[LexicalHit],
    context: &MergeContext,
    limit: usize,
) -> Vec<MergedHit> {
    let baseline_iter = baseline_hits
        .iter()
        .filter(|h| !context.hidden_paths.contains(&(h.collection.clone(), h.path.clone())))
        .map(|h| MergedHit {
            collection: h.collection.clone(),
            path: h.path.clone(),
            symbol_name: h.symbol_name.clone(),
            kind: h.kind.clone(),
            line_start: h.line_start,
            line_end: h.line_end,
            text: Some(h.text.clone()),
            score: h.rank,
            source: HitSource::Baseline,
        });

    let overlay_iter = overlay_hits.iter().map(|h| MergedHit {
        collection: h.collection.clone(),
        path: h.path.clone(),
        symbol_name: h.symbol_name.clone(),
        kind: h.kind.clone(),
        line_start: h.line_start,
        line_end: h.line_end,
        text: Some(h.text.clone()),
        score: h.rank,
        source: HitSource::Overlay,
    });

    merge_and_rank(baseline_iter, overlay_iter, limit)
}

pub fn merge_semantic(
    baseline_hits: &[SemanticHit],
    overlay_hits: &[SemanticHit],
    context: &MergeContext,
    limit: usize,
) -> Vec<MergedHit> {
    let baseline_iter = baseline_hits
        .iter()
        .filter(|h| !context.hidden_paths.contains(&(h.collection.clone(), h.path.clone())))
        .map(|h| MergedHit {
            collection: h.collection.clone(),
            path: h.path.clone(),
            symbol_name: h.symbol_name.clone(),
            kind: h.kind.clone(),
            line_start: h.line_start,
            line_end: h.line_end,
            text: None,
            score: h.score,
            source: HitSource::Baseline,
        });

    let overlay_iter = overlay_hits.iter().map(|h| MergedHit {
        collection: h.collection.clone(),
        path: h.path.clone(),
        symbol_name: h.symbol_name.clone(),
        kind: h.kind.clone(),
        line_start: h.line_start,
        line_end: h.line_end,
        text: None,
        score: h.score,
        source: HitSource::Overlay,
    });

    merge_and_rank(baseline_iter, overlay_iter, limit)
}

type DedupKey = (String, String, String, u32, u32);

fn dedup_key(hit: &MergedHit) -> DedupKey {
    (
        hit.collection.clone(),
        hit.path.clone(),
        hit.symbol_name.clone(),
        hit.line_start,
        hit.line_end,
    )
}

fn merge_and_rank(
    baseline: impl Iterator<Item = MergedHit>,
    overlay: impl Iterator<Item = MergedHit>,
    limit: usize,
) -> Vec<MergedHit> {
    let mut merged: Vec<MergedHit> = baseline.chain(overlay).collect();

    merged.sort_by(|a, b| {
        b.score.total_cmp(&a.score).then_with(|| {
            let ord_a = match a.source {
                HitSource::Overlay => 0u8,
                HitSource::Baseline => 1,
            };
            let ord_b = match b.source {
                HitSource::Overlay => 0u8,
                HitSource::Baseline => 1,
            };
            ord_a.cmp(&ord_b)
        })
    });

    let mut seen = HashSet::new();
    merged.retain(|hit| seen.insert(dedup_key(hit)));

    merged.truncate(limit);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{BaselineRef, CorpusId, DocumentPath, SearchOverlay};

    fn lexical(collection: &str, path: &str, symbol: &str, rank: f32) -> LexicalHit {
        LexicalHit {
            collection: collection.into(),
            path: path.into(),
            symbol_name: symbol.into(),
            kind: "function".into(),
            line_start: 1,
            line_end: 10,
            text: format!("text of {symbol}"),
            rank,
        }
    }

    fn semantic(collection: &str, path: &str, symbol: &str, score: f32) -> SemanticHit {
        SemanticHit {
            collection: collection.into(),
            path: path.into(),
            symbol_name: symbol.into(),
            kind: "function".into(),
            line_start: 1,
            line_end: 10,
            score,
        }
    }

    fn empty_context() -> MergeContext {
        MergeContext { hidden_paths: HashSet::new() }
    }

    fn context_hiding(paths: &[(&str, &str)]) -> MergeContext {
        MergeContext {
            hidden_paths: paths.iter().map(|(c, p)| (c.to_string(), p.to_string())).collect(),
        }
    }

    #[test]
    fn empty_overlay_returns_baseline_unchanged() {
        let baseline = vec![
            lexical("code", "src/a.bsl", "Proc1", 0.9),
            lexical("code", "src/b.bsl", "Proc2", 0.8),
        ];
        let result = merge_lexical(&baseline, &[], &empty_context(), 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].symbol_name, "Proc1");
        assert_eq!(result[1].symbol_name, "Proc2");
        assert!(result.iter().all(|h| h.source == HitSource::Baseline));
    }

    #[test]
    fn added_file_appears_from_overlay() {
        let baseline = vec![lexical("code", "src/a.bsl", "Proc1", 0.9)];
        let overlay = vec![lexical("code", "src/new.bsl", "NewProc", 0.95)];
        let result = merge_lexical(&baseline, &overlay, &empty_context(), 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].symbol_name, "NewProc");
        assert_eq!(result[0].source, HitSource::Overlay);
        assert_eq!(result[1].symbol_name, "Proc1");
        assert_eq!(result[1].source, HitSource::Baseline);
    }

    #[test]
    fn replaced_file_hides_baseline() {
        let baseline = vec![lexical("code", "src/a.bsl", "OldProc", 0.9)];
        let overlay = vec![lexical("code", "src/a.bsl", "NewProc", 0.85)];
        let ctx = context_hiding(&[("code", "src/a.bsl")]);
        let result = merge_lexical(&baseline, &overlay, &ctx, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].symbol_name, "NewProc");
        assert_eq!(result[0].source, HitSource::Overlay);
    }

    #[test]
    fn deleted_file_hides_baseline() {
        let baseline = vec![
            lexical("code", "src/a.bsl", "Proc1", 0.9),
            lexical("code", "src/b.bsl", "Proc2", 0.8),
        ];
        let ctx = context_hiding(&[("code", "src/a.bsl")]);
        let result = merge_lexical(&baseline, &[], &ctx, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].symbol_name, "Proc2");
    }

    #[test]
    fn lexical_baseline_filtered_by_replacement() {
        let baseline = vec![
            lexical("code", "src/replaced.bsl", "Old1", 0.95),
            lexical("code", "src/replaced.bsl", "Old2", 0.90),
            lexical("code", "src/kept.bsl", "Kept", 0.80),
        ];
        let overlay = vec![lexical("code", "src/replaced.bsl", "New1", 0.70)];
        let ctx = context_hiding(&[("code", "src/replaced.bsl")]);
        let result = merge_lexical(&baseline, &overlay, &ctx, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].symbol_name, "Kept");
        assert_eq!(result[0].source, HitSource::Baseline);
        assert_eq!(result[1].symbol_name, "New1");
        assert_eq!(result[1].source, HitSource::Overlay);
    }

    #[test]
    fn semantic_baseline_filtered_by_replacement() {
        let baseline = vec![
            semantic("code", "src/replaced.bsl", "OldSem", 0.95),
            semantic("code", "src/kept.bsl", "KeptSem", 0.80),
        ];
        let overlay = vec![semantic("code", "src/replaced.bsl", "NewSem", 0.70)];
        let ctx = context_hiding(&[("code", "src/replaced.bsl")]);
        let result = merge_semantic(&baseline, &overlay, &ctx, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].symbol_name, "KeptSem");
        assert_eq!(result[0].source, HitSource::Baseline);
        assert_eq!(result[1].symbol_name, "NewSem");
        assert_eq!(result[1].source, HitSource::Overlay);
    }

    #[test]
    fn overlay_wins_ties() {
        let baseline = vec![lexical("code", "src/a.bsl", "Proc", 0.85)];
        let overlay = vec![lexical("code", "src/b.bsl", "Proc", 0.85)];
        let result = merge_lexical(&baseline, &overlay, &empty_context(), 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].source, HitSource::Overlay);
        assert_eq!(result[1].source, HitSource::Baseline);
    }

    #[test]
    fn degraded_semantic_mode() {
        let baseline = vec![
            semantic("code", "src/a.bsl", "Sem1", 0.9),
            semantic("code", "src/b.bsl", "Sem2", 0.8),
        ];
        let result = merge_semantic(&baseline, &[], &empty_context(), 10);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|h| h.source == HitSource::Baseline));
        assert!(result.iter().all(|h| h.text.is_none()));
    }

    #[test]
    fn build_merge_context_collects_hidden_paths() {
        let mut overlay =
            SearchOverlay::new(BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "snap1"));
        overlay.delete_file(DocumentPath::new("code", "src/deleted.bsl"));
        overlay.replace_file(DocumentPath::new("code", "src/replaced.bsl"), vec![]);

        let ctx = build_merge_context(&overlay);
        assert!(ctx.hidden_paths.contains(&("code".into(), "src/deleted.bsl".into())));
        assert!(ctx.hidden_paths.contains(&("code".into(), "src/replaced.bsl".into())));
        assert_eq!(ctx.hidden_paths.len(), 2);
    }

    #[test]
    fn nan_scores_deterministic_ordering() {
        let baseline = vec![
            lexical("code", "src/a.bsl", "Good", 0.9),
            lexical("code", "src/b.bsl", "NanHit", f32::NAN),
        ];
        let result = merge_lexical(&baseline, &[], &empty_context(), 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].symbol_name, "NanHit");
        assert_eq!(result[1].symbol_name, "Good");
    }

    #[test]
    fn duplicate_hit_dedups_to_overlay() {
        let baseline = vec![lexical("code", "src/a.bsl", "Proc", 0.9)];
        let overlay = vec![lexical("code", "src/a.bsl", "Proc", 0.9)];
        let result = merge_lexical(&baseline, &overlay, &empty_context(), 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source, HitSource::Overlay);
    }

    #[test]
    fn limit_truncates_after_dedup() {
        let baseline = vec![
            lexical("code", "src/a.bsl", "P1", 0.9),
            lexical("code", "src/b.bsl", "P2", 0.8),
            lexical("code", "src/c.bsl", "P3", 0.7),
        ];
        let result = merge_lexical(&baseline, &[], &empty_context(), 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].symbol_name, "P1");
        assert_eq!(result[1].symbol_name, "P2");
    }

    #[test]
    fn limit_zero_returns_empty() {
        let baseline = vec![lexical("code", "src/a.bsl", "P1", 0.9)];
        let result = merge_lexical(&baseline, &[], &empty_context(), 0);
        assert!(result.is_empty());
    }

    #[test]
    fn end_to_end_context_into_merge() {
        let mut overlay =
            SearchOverlay::new(BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "snap1"));
        overlay.delete_file(DocumentPath::new("code", "src/deleted.bsl"));
        overlay.replace_file(DocumentPath::new("code", "src/replaced.bsl"), vec![]);

        let ctx = build_merge_context(&overlay);
        let baseline = vec![
            lexical("code", "src/deleted.bsl", "Gone", 0.95),
            lexical("code", "src/replaced.bsl", "Old", 0.90),
            lexical("code", "src/kept.bsl", "Kept", 0.80),
        ];
        let overlay_hits = vec![lexical("code", "src/replaced.bsl", "New", 0.70)];
        let result = merge_lexical(&baseline, &overlay_hits, &ctx, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].symbol_name, "Kept");
        assert_eq!(result[1].symbol_name, "New");
    }
}
