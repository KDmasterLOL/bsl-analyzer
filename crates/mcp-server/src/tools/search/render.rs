use bsl_search::{FusedHit, LexicalHit, SearchHit, SemanticHit};
use std::fmt::Write;
use std::path::Path;

/// Durable graph id for a code-search hit, when it names a method. Returns `None` for
/// headers and non-method symbols. Module-keyed methods (common/object/manager) resolve
/// regardless of root; form/command/file-module methods fall back to the
/// `method/file/<rel>::<name>` id the graph also mints.
///
/// The call graph keys file paths against `graph_root` (the repo root it was built from), but
/// search hit paths are relative to `engine_root` (the configuration root, e.g. `src/cf`,
/// nested under it). So we re-anchor the hit to an absolute path via `engine_root`, then let
/// [`ide::method_graph_id`] strip `graph_root` back down — yielding the same `src/cf/…` prefix
/// the graph minted, so a form/file method id resolves in `graph` instead of `not_found`.
/// (Module-keyed ids are prefix-independent, so they are unaffected by the re-anchoring.)
pub(super) fn graph_id_for_hit(
    hit: &SearchHit,
    engine_root: Option<&Path>,
    graph_root: Option<&Path>,
) -> Option<String> {
    if hit.symbol_name.is_empty() {
        return None;
    }
    let kind = hit.kind.to_lowercase();
    let is_method =
        ["proc", "func", "процед", "функц", "метод"].iter().any(|marker| kind.contains(marker));
    if !is_method {
        return None;
    }
    let hit_is_absolute = Path::new(&hit.file_path).is_absolute();
    match engine_root {
        Some(root) if !hit_is_absolute => {
            let abs = root.join(&hit.file_path);
            ide::method_graph_id(&abs.to_string_lossy(), &hit.symbol_name, graph_root)
        }
        _ if hit_is_absolute => ide::method_graph_id(&hit.file_path, &hit.symbol_name, graph_root),
        _ => ide::method_graph_id(&hit.file_path, &hit.symbol_name, None)
            .filter(|id| !id.starts_with("method/file/")),
    }
}

/// Render at most as many leading hits as fit `max_output_tokens` (~4 chars/token),
/// shrinking at hit boundaries so a positionally-parsed block is never cut mid-way, then
/// append a one-line truncation note when hits were dropped. The note goes AFTER the hits,
/// so a client parsing `graph_id:` lines positionally from the top is never shifted.
pub(super) fn budgeted_hits(
    count: usize,
    max_output_tokens: usize,
    render_prefix: impl Fn(usize) -> String,
) -> String {
    let shown = crate::tools::response::fit_item_count(count, max_output_tokens, |n| {
        render_prefix(n).len()
    });
    let mut out = render_prefix(shown);
    let over_budget = out.len() > max_output_tokens.saturating_mul(4);
    if shown < count || over_budget {
        let _ = writeln!(
            out,
            "-- showing {shown} of {count} results (truncated to fit max_output_tokens; raise the budget or narrow the query) --"
        );
    }
    out
}

pub(super) fn format_code_hits(
    hits: &[FusedHit],
    engine_root: Option<&Path>,
    graph_root: Option<&Path>,
    max_output_tokens: usize,
) -> String {
    budgeted_hits(hits.len(), max_output_tokens, |n| {
        let mut out = String::new();
        for (i, fused) in hits[..n].iter().enumerate() {
            let hit = &fused.hit;
            let name = if hit.symbol_name.is_empty() { "<header>" } else { &hit.symbol_name };
            let _ = writeln!(
                out,
                "#{} [{}] {}:{}-{} :: {} ({})",
                i + 1,
                fused.modality.tag(),
                hit.file_path,
                hit.line_start + 1,
                hit.line_end,
                name,
                hit.kind,
            );
            if let Some(id) = graph_id_for_hit(hit, engine_root, graph_root) {
                let _ = writeln!(out, "  graph_id: {id}");
            }

            let snippet = crate::tools::redact::redact_secrets(&hit.text);
            for line in snippet.lines().take(5) {
                let _ = writeln!(out, "  │ {line}");
            }
            let total_lines = snippet.lines().count();
            if total_lines > 5 {
                let _ = writeln!(out, "  │ ... ({} more lines)", total_lines - 5);
            }
            out.push('\n');
        }
        out
    })
}

pub(super) fn format_doc_hits(hits: &[SearchHit], max_output_tokens: usize) -> String {
    budgeted_hits(hits.len(), max_output_tokens, |n| {
        let mut out = String::new();
        for (i, hit) in hits[..n].iter().enumerate() {
            let _ =
                writeln!(out, "#{} [{:.3}] {} ({})", i + 1, hit.score, hit.symbol_name, hit.kind,);

            for line in hit.text.lines().take(5) {
                let _ = writeln!(out, "  │ {line}");
            }
            let total_lines = hit.text.lines().count();
            if total_lines > 5 {
                let _ = writeln!(out, "  │ ... ({} more lines)", total_lines - 5);
            }
            out.push('\n');
        }
        out
    })
}

pub(super) fn format_lexical_doc_hits(hits: &[LexicalHit], max_output_tokens: usize) -> String {
    budgeted_hits(hits.len(), max_output_tokens, |n| {
        let mut out = String::new();
        for (i, hit) in hits[..n].iter().enumerate() {
            let name = if hit.symbol_name.is_empty() { "<header>" } else { &hit.symbol_name };
            let _ = writeln!(
                out,
                "#{} [{:.3}] {}:{}-{} :: {} ({})",
                i + 1,
                hit.rank,
                hit.path,
                hit.line_start + 1,
                hit.line_end,
                name,
                hit.kind,
            );

            for line in hit.text.lines().take(5) {
                let _ = writeln!(out, "  │ {line}");
            }
            let total_lines = hit.text.lines().count();
            if total_lines > 5 {
                let _ = writeln!(out, "  │ ... ({} more lines)", total_lines - 5);
            }
            out.push('\n');
        }
        out
    })
}

pub(super) fn format_semantic_doc_hits(hits: &[SemanticHit], max_output_tokens: usize) -> String {
    budgeted_hits(hits.len(), max_output_tokens, |n| {
        let mut out = String::new();
        for (i, hit) in hits[..n].iter().enumerate() {
            let name = if hit.symbol_name.is_empty() { "<header>" } else { &hit.symbol_name };
            let _ = writeln!(
                out,
                "#{} [{:.3}] {}:{}-{} :: {} ({})",
                i + 1,
                hit.score,
                hit.path,
                hit.line_start + 1,
                hit.line_end,
                name,
                hit.kind,
            );
            out.push('\n');
        }
        out
    })
}

pub(super) fn format_baseline_ref(baseline: &bsl_search::BaselineRef) -> String {
    if let Some(snapshot_id) = &baseline.snapshot_id {
        return format!("snapshot {}", snapshot_id.0);
    }
    if let (Some(branch), Some(commit)) = (&baseline.branch, &baseline.commit) {
        return format!("branch {branch} @ {commit}");
    }
    if let Some(branch) = &baseline.branch {
        return format!("branch {branch}");
    }
    if let Some(commit) = &baseline.commit {
        return format!("commit {commit}");
    }
    format!("latest {}", baseline.corpus.as_str())
}

#[cfg(test)]
mod tests {
    use super::super::test_support::code_hit;
    use super::{format_code_hits, graph_id_for_hit};
    use bsl_search::{FusedHit, Modality};
    use std::path::Path;

    #[test]
    fn format_code_hits_shows_modality_tag() {
        let hits = vec![
            FusedHit {
                hit: code_hit("CommonModules/М/Ext/Module.bsl", "Оба", "procedure"),
                modality: Modality::Both,
            },
            FusedHit {
                hit: code_hit("CommonModules/М/Ext/Module.bsl", "Лекс", "procedure"),
                modality: Modality::Lexical,
            },
            FusedHit {
                hit: code_hit("CommonModules/М/Ext/Module.bsl", "Сем", "procedure"),
                modality: Modality::Semantic,
            },
        ];
        let out = format_code_hits(&hits, None, None, usize::MAX);

        assert!(out.contains("#1 [L+S]"), "both-modality hit tagged L+S: {out}");
        assert!(out.contains("#2 [L]"), "lexical-only hit tagged L: {out}");
        assert!(out.contains("#3 [S]"), "semantic-only hit tagged S: {out}");
    }

    #[test]
    fn graph_id_bridges_method_hits_in_modules() {
        let engine_root = Path::new("/repo/src/cf");
        let graph_root = Path::new("/repo");

        assert_eq!(
            graph_id_for_hit(
                &code_hit("CommonModules/Утилиты/Ext/Module.bsl", "ПроверитьИНН", "procedure"),
                Some(engine_root),
                Some(graph_root),
            ),
            Some("method/common/Утилиты/ПроверитьИНН".to_owned()),
        );
        assert_eq!(
            graph_id_for_hit(
                &code_hit("CommonModules/Утилиты/Ext/Module.bsl", "МодульнаяПерем", "variable"),
                Some(engine_root),
                Some(graph_root),
            ),
            None,
        );
        assert_eq!(
            graph_id_for_hit(
                &code_hit(
                    "Catalogs/Контрагенты/Forms/Форма/Ext/Form/Module.bsl",
                    "ПриОткрытии",
                    "procedure",
                ),
                Some(engine_root),
                Some(graph_root),
            ),
            Some(
                "method/file/src/cf/Catalogs/Контрагенты/Forms/Форма/Ext/Form/Module.bsl::ПриОткрытии"
                    .to_owned()
            ),
        );
        assert_eq!(
            graph_id_for_hit(
                &code_hit(
                    "Catalogs/Контрагенты/Forms/Форма/Ext/Form/Module.bsl",
                    "ПриОткрытии",
                    "procedure",
                ),
                None,
                None,
            ),
            None,
        );
        assert_eq!(
            graph_id_for_hit(
                &code_hit("CommonModules/Утилиты/Ext/Module.bsl", "ПроверитьИНН", "procedure"),
                None,
                None,
            ),
            Some("method/common/Утилиты/ПроверитьИНН".to_owned()),
        );
    }
}
