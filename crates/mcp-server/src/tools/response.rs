//! Response-shaping helpers shared by the agent-facing tools.

use rmcp::model::CallToolResult;
use serde_json::Value;

/// Emit `body` as the MCP `structuredContent` field. rmcp mirrors the value as a
/// compact JSON text block for clients without structured-output support;
/// structured-aware hosts read `structuredContent` and ignore the mirror, so the
/// payload reaches a model exactly once either way.
pub fn structured(body: Value) -> CallToolResult {
    CallToolResult::structured(body)
}

/// Default output budget (in tokens, ~4 chars each) for the list/text tools that carry no
/// explicit per-action default, mirroring `graph`'s body budget. A small `limit` keeps most
/// responses well under this, so the default rarely bites; it is a ceiling, not a target.
pub const DEFAULT_OUTPUT_BUDGET_TOKENS: usize = 6000;

/// The output-budget convention shared by the list-returning tools, mirroring `graph`'s
/// body budget. Callers sort `items` into a stable order first, then drop trailing items
/// until the serialized array fits `max_output_tokens` (~4 chars each), keeping at least
/// one item so a single oversized item is still delivered. Returns `true` when anything
/// was dropped, so the caller can stamp `budget_exhausted` and a continuation hint.
///
/// Item sizes are measured once (O(n)); the `+1` per gap is the JSON comma, `+2` the
/// enclosing brackets — a close-enough upper bound on the array's own framing.
pub fn trim_items_to_budget(items: &mut Vec<Value>, max_output_tokens: usize) -> bool {
    let budget = max_output_tokens.saturating_mul(4);
    let mut used = 2usize; // `[` + `]`
    let mut keep = 0usize;
    for (i, item) in items.iter().enumerate() {
        let len = serde_json::to_string(item).map(|s| s.len()).unwrap_or(0);
        let sep = usize::from(i > 0); // comma between items
        let next = used + sep + len;
        if next > budget && keep > 0 {
            break;
        }
        used = next;
        keep = i + 1;
    }
    let dropped = keep < items.len();
    if dropped {
        items.truncate(keep.max(1));
    }
    // Exhausted means "the returned array may exceed the budget", so it fires either when we
    // dropped a trailing item OR when the single kept item is itself over budget (the keep>=1
    // floor delivers it anyway) — the flag never depends on how many siblings it had.
    dropped || used > budget
}

/// Truncate a Markdown/plain-text tool body to fit `max_output_tokens` (~4 chars each) at a
/// line boundary, appending `note` (a continuation hint) when it had to cut. Text tools carry
/// no structured envelope, so the trailing note IS the truncation marker. Returns whether it
/// truncated. A single line longer than the budget is cut at a char boundary rather than
/// dropped whole.
pub fn truncate_text_to_budget(text: &mut String, max_output_tokens: usize, note: &str) -> bool {
    let budget = max_output_tokens.saturating_mul(4);
    if text.len() <= budget {
        return false;
    }
    // Reserve room for the note so the final string (body + note) still fits the budget rather
    // than overshooting it by the note's length.
    let mut cap = budget.saturating_sub(note.len());
    while cap > 0 && !text.is_char_boundary(cap) {
        cap -= 1;
    }
    // Prefer a line boundary within budget; fall back to the char boundary for one oversized line.
    let cut = text[..cap].rfind('\n').map(|i| i + 1).unwrap_or(cap);
    text.truncate(cut);
    text.push_str(note);
    true
}

/// The largest count `n ≤ items` such that `render(n)` fits `max_output_tokens` (~4 chars
/// each), never below 1. For the text tools whose payload is line-oriented and positionally
/// parsed (`search` hit blocks): shrink the number of rendered items at item boundaries
/// rather than cutting a block mid-way. `render` is called O(items) times; callers keep the
/// item count small (a search `limit`), so this stays cheap.
pub fn fit_item_count(
    items: usize,
    max_output_tokens: usize,
    render: impl Fn(usize) -> usize,
) -> usize {
    let budget = max_output_tokens.saturating_mul(4);
    let mut n = items;
    while n > 1 && render(n) > budget {
        n -= 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn items(n: usize) -> Vec<serde_json::Value> {
        (0..n).map(|i| json!({ "code": format!("Rule{i:03}"), "text": "x".repeat(40) })).collect()
    }

    #[test]
    fn trim_keeps_all_when_within_budget() {
        let mut v = items(3);
        // 3 items × ~60 chars is far under 1000 tokens (~4000 chars).
        assert!(!trim_items_to_budget(&mut v, 1000));
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn trim_drops_trailing_items_over_budget() {
        let mut v = items(50);
        // 1 token ≈ 4 chars: only the first item (well over 4 chars) survives, never zero.
        let dropped = trim_items_to_budget(&mut v, 1);
        assert!(dropped);
        assert_eq!(v.len(), 1);
        // The stable-order head is what remains.
        assert_eq!(v[0]["code"], "Rule000");
    }

    #[test]
    fn trim_flags_single_oversized_item_even_though_none_dropped() {
        // One item bigger than the budget: the keep>=1 floor delivers it, but the caller must
        // still learn the output exceeds the budget — so exhausted is true with nothing dropped.
        let mut v = items(1);
        assert!(trim_items_to_budget(&mut v, 1));
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn fit_item_count_never_returns_zero() {
        // A single item bigger than the budget is still delivered (flagged elsewhere).
        assert_eq!(fit_item_count(10, 1, |n| n * 4000), 1);
        assert_eq!(fit_item_count(0, 1, |_| 10_000), 0);
    }

    #[test]
    fn truncate_text_cuts_at_line_boundary_and_notes() {
        let mut text = "line-a\nline-b\nline-c\nline-d\n".to_string();
        // Budget 5 tokens = 20 chars, note 12 chars: after reserving the note there is room for
        // the first whole line only.
        let cut = truncate_text_to_budget(&mut text, 5, "\n-- more --\n");
        assert!(cut);
        assert!(text.starts_with("line-a\n"));
        assert!(!text.contains("line-d"));
        assert!(text.ends_with("-- more --\n"));
        // Never leaves a half-written line before the note.
        assert!(!text.contains("line-b\nline-c"));
    }

    #[test]
    fn truncate_text_noop_when_within_budget() {
        let mut text = "short\n".to_string();
        assert!(!truncate_text_to_budget(&mut text, 1000, "-- more --"));
        assert_eq!(text, "short\n");
    }
}
