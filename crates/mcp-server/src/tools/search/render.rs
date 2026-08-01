use super::types::SEARCH_SCHEMA_VERSION;
use crate::tools::response::structured_with_text;
use bsl_search::{FusedHit, LexicalHit, SearchHit, SemanticHit};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};
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

/// How many leading snippet lines the listing shows. The structured view mirrors exactly
/// these lines rather than the whole body: a consumer that needs the rest fetches it by path
/// or `graph source`, and the output budget stays predictable either way.
const SNIPPET_LINES: usize = 5;

/// What the envelope around the hits costs: `schema_version`, `shown`, `total`, the optional
/// `budget_exhausted` / `degraded` keys, and the hit array's own brackets. An upper bound, not
/// a measurement — the point is that the wrapper is charged, not that it is charged exactly.
pub(super) const ENVELOPE_OVERHEAD_BYTES: usize = 128;

/// One hit rendered in both views: the text block a person reads and the JSON object a machine
/// reads. Rendering the pair up front lets [`budgeted_hits`] charge the budget for what the
/// response actually carries, and keeps the two views from ever disagreeing on which hits made
/// the cut.
struct HitBlock {
    text: String,
    json: Value,
}

/// A budgeted hit listing: the text (truncation note already appended), the parallel JSON
/// array, and the counts that let a structured consumer tell a short answer from a cut one.
pub(super) struct RenderedHits {
    pub(super) text: String,
    pub(super) hits: Vec<Value>,
    pub(super) shown: usize,
    pub(super) total: usize,
    pub(super) budget_exhausted: bool,
}

impl RenderedHits {
    /// The listing as a whole response, its own text serving as the mirror. Callers that wrap
    /// the listing in extra prose — a leading legend, a trailing degradation note — compose
    /// that text themselves and call [`hits_response`] directly.
    pub(super) fn into_response(mut self) -> CallToolResult {
        let text = std::mem::take(&mut self.text);
        hits_response(text, self, None)
    }
}

/// The leading snippet lines and how many were dropped, borrowed from `text` so the text and
/// JSON views render from one split rather than two.
fn snippet_head(text: &str) -> (Vec<&str>, usize) {
    let head: Vec<&str> = text.lines().take(SNIPPET_LINES).collect();
    let dropped = text.lines().count() - head.len();
    (head, dropped)
}

/// Append the snippet to both views of one hit. `snippet` is already redacted by the caller.
fn push_snippet(text: &mut String, json: &mut Value, snippet: &str) {
    let (head, dropped) = snippet_head(snippet);
    for line in &head {
        let _ = writeln!(text, "  │ {line}");
    }
    if dropped > 0 {
        let _ = writeln!(text, "  │ ... ({dropped} more lines)");
    }
    if head.is_empty() {
        return;
    }
    json["snippet"] = json!(head.join("\n"));
    if dropped > 0 {
        json["snippet_truncated_lines"] = json!(dropped);
    }
}

/// A relevance score rounded to the 3 decimals the text listing prints, so the structured view
/// carries the same number a reader sees rather than the f32's full expansion. It rounds
/// through the very formatter the listing uses: `{:.3}` breaks a tie to even, while arithmetic
/// rounding breaks it away from zero, and `0.0625` would then print `0.062` next to `0.063`.
fn rounded_score(score: f32) -> Value {
    let text = format!("{score:.3}");
    json!(text.parse::<f64>().unwrap_or(f64::from(score)))
}

/// A symbol name for the text listing; the structured view omits the field instead of
/// inventing this placeholder, because a chunk with no symbol is a file header, not a symbol
/// named `<header>`.
fn display_name(symbol_name: &str) -> &str {
    if symbol_name.is_empty() {
        "<header>"
    } else {
        symbol_name
    }
}

/// Keep as many leading hits as fit `max_output_tokens` (~4 chars/token), shrinking at hit
/// boundaries so neither a positionally-parsed text block nor a JSON object is cut mid-way,
/// then append a one-line truncation note when hits were dropped. The note goes AFTER the
/// hits, so a client parsing `graph_id:` lines positionally from the top is never shifted.
///
/// The budget covers the text and the JSON array together: the response carries both, so
/// charging only the text would overshoot the caller's ceiling by roughly the JSON's size.
/// At least one hit is always kept — a single oversized hit is delivered and flagged by
/// `budget_exhausted` rather than dropped into an empty-looking answer.
fn budgeted_hits(blocks: Vec<HitBlock>, max_output_tokens: usize) -> RenderedHits {
    let budget = max_output_tokens.saturating_mul(4);
    let total = blocks.len();
    // The hits are not the whole response: they get wrapped in [`hits_response`]'s envelope
    // keys and the `[` + `]` of their own array. Charging for that up front is what keeps
    // `budget_exhausted: false` from appearing on a response that is already over its ceiling.
    let mut used = ENVELOPE_OVERHEAD_BYTES;
    let mut shown = 0usize;
    for (i, block) in blocks.iter().enumerate() {
        let json_len = serde_json::to_string(&block.json).map(|s| s.len()).unwrap_or(0);
        let sep = usize::from(i > 0); // comma between JSON items
        let next = used + block.text.len() + json_len + sep;
        if next > budget && shown > 0 {
            break;
        }
        used = next;
        shown = i + 1;
    }

    let mut text = String::new();
    let mut hits = Vec::with_capacity(shown);
    for block in blocks.into_iter().take(shown) {
        text.push_str(&block.text);
        hits.push(block.json);
    }
    // An empty listing cannot overflow anything: the `[` + `]` framing alone must not raise the
    // flag when a caller passes a budget of zero.
    let budget_exhausted = shown < total || (shown > 0 && used > budget);
    if budget_exhausted {
        let _ = writeln!(
            text,
            "-- showing {shown} of {total} results (truncated to fit max_output_tokens; raise the budget or narrow the query) --"
        );
    }
    RenderedHits { text, hits, shown, total, budget_exhausted }
}

/// The text a hit-listing action emits when the ranked list is empty. Kept verbatim: it is
/// the sentence clients have parsed since before the structured envelope existed.
const NO_HITS_TEXT: &str = "No results found.";

/// Wrap a rendered listing as a tool result: the text listing stays the content block, and the
/// same hits go out as `structuredContent` so a consumer reads fields instead of parsing
/// columns. `total` is the ranked list's length before the budget cut it — already bounded by
/// the request's `limit`, so it is not the corpus-wide match count. `degraded` names a modality
/// that could not serve (semantic down, index warming); it is carried structurally even when
/// the listing is empty, because "nothing found" and "nothing found with half the search
/// working" are different answers.
pub(super) fn hits_response(
    text: String,
    rendered: RenderedHits,
    degraded: Option<&str>,
) -> CallToolResult {
    let mut body = json!({
        "schema_version": SEARCH_SCHEMA_VERSION,
        "hits": rendered.hits,
        "shown": rendered.shown,
        "total": rendered.total,
    });
    if rendered.budget_exhausted {
        body["budget_exhausted"] = json!(true);
    }
    if let Some(reason) = degraded {
        body["degraded"] = json!(reason);
    }
    structured_with_text(text, body)
}

/// The empty-listing result, with the same envelope a populated one carries so a consumer
/// never has to tell "no hits" from "no structured output" by absence.
pub(super) fn no_hits_response(degraded: Option<&str>) -> CallToolResult {
    hits_response(
        NO_HITS_TEXT.to_owned(),
        RenderedHits {
            text: String::new(),
            hits: Vec::new(),
            shown: 0,
            total: 0,
            budget_exhausted: false,
        },
        degraded,
    )
}

pub(super) fn format_code_hits(
    hits: &[FusedHit],
    engine_root: Option<&Path>,
    graph_root: Option<&Path>,
    max_output_tokens: usize,
) -> RenderedHits {
    let blocks = hits
        .iter()
        .enumerate()
        .map(|(i, fused)| {
            let rank = i + 1;
            let hit = &fused.hit;
            let mut text = String::new();
            let _ = writeln!(
                text,
                "#{} [{}] {}:{}-{} :: {} ({})",
                rank,
                fused.modality.tag(),
                hit.file_path,
                hit.line_start + 1,
                hit.line_end,
                display_name(&hit.symbol_name),
                hit.kind,
            );
            let mut json = json!({
                "rank": rank,
                "modality": fused.modality.tag(),
                "path": hit.file_path,
                "line_start": hit.line_start + 1,
                "line_end": hit.line_end,
                "kind": hit.kind,
            });
            if !hit.symbol_name.is_empty() {
                json["symbol"] = json!(hit.symbol_name);
            }
            if let Some(id) = graph_id_for_hit(hit, engine_root, graph_root) {
                let _ = writeln!(text, "  graph_id: {id}");
                json["graph_id"] = json!(id);
            }
            push_snippet(&mut text, &mut json, &crate::tools::redact::redact_secrets(&hit.text));
            text.push('\n');
            HitBlock { text, json }
        })
        .collect();
    budgeted_hits(blocks, max_output_tokens)
}

pub(super) fn format_doc_hits(hits: &[SearchHit], max_output_tokens: usize) -> RenderedHits {
    let blocks = hits
        .iter()
        .enumerate()
        .map(|(i, hit)| {
            let rank = i + 1;
            let mut text = String::new();
            let _ =
                writeln!(text, "#{} [{:.3}] {} ({})", rank, hit.score, hit.symbol_name, hit.kind);
            let mut json = json!({
                "rank": rank,
                "score": rounded_score(hit.score),
                "path": hit.file_path,
                "line_start": hit.line_start + 1,
                "line_end": hit.line_end,
                "kind": hit.kind,
            });
            if !hit.symbol_name.is_empty() {
                json["symbol"] = json!(hit.symbol_name);
            }
            push_snippet(&mut text, &mut json, &hit.text);
            text.push('\n');
            HitBlock { text, json }
        })
        .collect();
    budgeted_hits(blocks, max_output_tokens)
}

pub(super) fn format_lexical_doc_hits(
    hits: &[LexicalHit],
    max_output_tokens: usize,
) -> RenderedHits {
    let blocks = hits
        .iter()
        .enumerate()
        .map(|(i, hit)| {
            let rank = i + 1;
            let mut text = String::new();
            let _ = writeln!(
                text,
                "#{} [{:.3}] {}:{}-{} :: {} ({})",
                rank,
                hit.rank,
                hit.path,
                hit.line_start + 1,
                hit.line_end,
                display_name(&hit.symbol_name),
                hit.kind,
            );
            let mut json = json!({
                "rank": rank,
                "score": rounded_score(hit.rank),
                "path": hit.path,
                "line_start": hit.line_start + 1,
                "line_end": hit.line_end,
                "kind": hit.kind,
            });
            if !hit.symbol_name.is_empty() {
                json["symbol"] = json!(hit.symbol_name);
            }
            push_snippet(&mut text, &mut json, &hit.text);
            text.push('\n');
            HitBlock { text, json }
        })
        .collect();
    budgeted_hits(blocks, max_output_tokens)
}

pub(super) fn format_semantic_doc_hits(
    hits: &[SemanticHit],
    max_output_tokens: usize,
) -> RenderedHits {
    let blocks = hits
        .iter()
        .enumerate()
        .map(|(i, hit)| {
            let rank = i + 1;
            let mut text = String::new();
            let _ = writeln!(
                text,
                "#{} [{:.3}] {}:{}-{} :: {} ({})",
                rank,
                hit.score,
                hit.path,
                hit.line_start + 1,
                hit.line_end,
                display_name(&hit.symbol_name),
                hit.kind,
            );
            text.push('\n');
            let mut json = json!({
                "rank": rank,
                "score": rounded_score(hit.score),
                "path": hit.path,
                "line_start": hit.line_start + 1,
                "line_end": hit.line_end,
                "kind": hit.kind,
            });
            if !hit.symbol_name.is_empty() {
                json["symbol"] = json!(hit.symbol_name);
            }
            HitBlock { text, json }
        })
        .collect();
    budgeted_hits(blocks, max_output_tokens)
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
    use super::{format_code_hits, format_doc_hits, graph_id_for_hit, no_hits_response};
    use bsl_search::{FusedHit, Modality, SearchHit};
    use serde_json::json;
    use std::path::Path;

    fn fused(symbol: &str, modality: Modality) -> FusedHit {
        FusedHit { hit: code_hit("CommonModules/М/Ext/Module.bsl", symbol, "procedure"), modality }
    }

    #[test]
    fn format_code_hits_shows_modality_tag() {
        let hits = vec![
            fused("Оба", Modality::Both),
            fused("Лекс", Modality::Lexical),
            fused("Сем", Modality::Semantic),
        ];
        let out = format_code_hits(&hits, None, None, usize::MAX);

        assert!(out.text.contains("#1 [L+S]"), "both-modality hit tagged L+S: {}", out.text);
        assert!(out.text.contains("#2 [L]"), "lexical-only hit tagged L: {}", out.text);
        assert!(out.text.contains("#3 [S]"), "semantic-only hit tagged S: {}", out.text);
        assert_eq!(out.hits[0]["modality"], "L+S");
        assert_eq!(out.hits[1]["modality"], "L");
        assert_eq!(out.hits[2]["modality"], "S");
    }

    #[test]
    fn code_hit_structure_carries_every_field_the_listing_prints() {
        let mut hit = code_hit("CommonModules/Утилиты/Ext/Module.bsl", "ПроверитьИНН", "procedure");
        hit.text = (1..=7).map(|i| format!("строка {i}")).collect::<Vec<_>>().join("\n");
        hit.line_start = 180;
        hit.line_end = 201;
        let hits = vec![FusedHit { hit, modality: Modality::Lexical }];

        let out = format_code_hits(&hits, None, None, usize::MAX);

        assert_eq!(
            out.hits[0],
            json!({
                "rank": 1,
                "modality": "L",
                "path": "CommonModules/Утилиты/Ext/Module.bsl",
                "line_start": 181,
                "line_end": 201,
                "symbol": "ПроверитьИНН",
                "kind": "procedure",
                "graph_id": "method/common/Утилиты/ПроверитьИНН",
                "snippet": "строка 1\nстрока 2\nстрока 3\nстрока 4\nстрока 5",
                "snippet_truncated_lines": 2,
            }),
        );
        // The listing and the structure describe one answer: every value above is on screen.
        assert!(
            out.text.contains(
                "#1 [L] CommonModules/Утилиты/Ext/Module.bsl:181-201 :: ПроверитьИНН (procedure)"
            ),
            "{}",
            out.text
        );
        assert!(
            out.text.contains("  graph_id: method/common/Утилиты/ПроверитьИНН"),
            "{}",
            out.text
        );
        assert!(out.text.contains("  │ ... (2 more lines)"), "{}", out.text);
        assert!(!out.budget_exhausted);
        assert_eq!((out.shown, out.total), (1, 1));
    }

    #[test]
    fn code_hit_structure_omits_absent_symbol_graph_id_and_snippet() {
        let hits = vec![fused("", Modality::Semantic)];

        let out = format_code_hits(&hits, None, None, usize::MAX);

        assert_eq!(
            out.hits[0],
            json!({
                "rank": 1,
                "modality": "S",
                "path": "CommonModules/М/Ext/Module.bsl",
                "line_start": 1,
                "line_end": 1,
                "kind": "procedure",
            }),
        );
        // The text needs a name in the column; the structure says "no symbol" by absence
        // rather than inventing the placeholder as a value.
        assert!(out.text.contains(":: <header> (procedure)"), "{}", out.text);
    }

    #[test]
    fn budget_covers_the_text_and_the_structure_together() {
        let hits: Vec<FusedHit> =
            (1..=5).map(|i| fused(&format!("Процедура{i}"), Modality::Lexical)).collect();

        let full = format_code_hits(&hits, None, None, usize::MAX);
        assert_eq!((full.shown, full.total), (5, 5));
        assert!(!full.budget_exhausted);

        // A budget that fits the text of all five but not the text plus the JSON array must
        // drop hits: the response carries both, so charging for the text alone would overshoot.
        let text_only_tokens = full.text.len().div_ceil(4);
        let cut = format_code_hits(&hits, None, None, text_only_tokens);
        assert!(cut.shown < 5, "structure must be charged too: shown={}", cut.shown);
        assert_eq!(cut.hits.len(), cut.shown);
        assert_eq!(cut.total, 5);
        assert!(cut.budget_exhausted);
        assert!(
            cut.text.contains(&format!("-- showing {} of 5 results", cut.shown)),
            "{}",
            cut.text
        );
    }

    #[test]
    fn one_oversized_hit_is_delivered_and_flagged() {
        let hits = vec![fused("Процедура", Modality::Lexical)];

        let out = format_code_hits(&hits, None, None, 1);

        assert_eq!((out.shown, out.total), (1, 1));
        assert!(out.budget_exhausted, "an over-budget single hit must still say so");
    }

    #[test]
    fn doc_hit_structure_rounds_the_score_the_listing_prints() {
        let hits = vec![SearchHit {
            collection: "platform".to_owned(),
            root_id: bsl_search::CONFIGURATION_ROOT_ID.to_owned(),
            file_path: "Массив.html".to_owned(),
            symbol_name: "Массив".to_owned(),
            kind: "type".to_owned(),
            text: "Описание".to_owned(),
            line_start: 0,
            line_end: 4,
            score: 0.123_456_7,
        }];

        let out = format_doc_hits(&hits, usize::MAX);

        assert_eq!(out.hits[0]["score"], json!(0.123));
        assert!(out.text.contains("#1 [0.123] Массив (type)"), "{}", out.text);
        assert_eq!(out.hits[0]["snippet"], "Описание");
        assert_eq!(out.hits[0]["path"], "Массив.html");
    }

    #[test]
    fn score_ties_round_the_same_way_in_both_views() {
        // 0.0625 is exact in binary and lands exactly halfway at 3 decimals: `{:.3}` breaks the
        // tie to even (0.062), arithmetic rounding breaks it away from zero (0.063).
        let hits = vec![SearchHit {
            collection: "platform".to_owned(),
            root_id: bsl_search::CONFIGURATION_ROOT_ID.to_owned(),
            file_path: "Массив.html".to_owned(),
            symbol_name: "Массив".to_owned(),
            kind: "type".to_owned(),
            text: String::new(),
            line_start: 0,
            line_end: 1,
            score: 0.0625,
        }];

        let out = format_doc_hits(&hits, usize::MAX);

        assert!(out.text.contains("[0.062]"), "{}", out.text);
        assert_eq!(out.hits[0]["score"], json!(0.062));
    }

    #[test]
    fn empty_listing_still_carries_the_envelope() {
        let result = no_hits_response(Some("semantic skipped: runtime initialization failed"));

        assert_eq!(result.content[0].raw.as_text().expect("text").text, "No results found.");
        let body = result.structured_content.expect("structured envelope");
        assert_eq!(body["hits"], json!([]));
        assert_eq!(body["total"], 0);
        assert_eq!(body["degraded"], "semantic skipped: runtime initialization failed");
        assert!(body.get("budget_exhausted").is_none(), "nothing was cut: {body}");
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
