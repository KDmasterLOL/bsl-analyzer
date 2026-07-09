//! Merging of independent diagnostic quick fixes into a single, conflict-free edit set
//! for batch application (LSP `source.fixAll`, a CLI `--fix-all`, …). Pure domain logic
//! over [`Fix`]/[`TextEdit`] with no frontend dependency.

use crate::{Fix, TextEdit, TextRange};

/// Whether two edits cannot both be applied in one batch: their ranges overlap, or they
/// are two insertions at the same offset (whose combined order would be undefined).
fn edits_conflict(a: TextRange, b: TextRange) -> bool {
    if a.is_empty() && b.is_empty() {
        return a.start() == b.start();
    }
    a.start() < b.end() && b.start() < a.end()
}

/// Merge the edits of `fixes` into a non-overlapping, position-sorted set.
///
/// Each fix is atomic: if any of its edits would conflict with an already-accepted edit,
/// the whole fix is dropped. Fixes are visited in a deterministic order (by their
/// earliest edit offset), so the result is stable for a given input.
pub fn merge_fixes<'a>(fixes: impl IntoIterator<Item = &'a Fix>) -> Vec<TextEdit> {
    let mut fixes: Vec<&Fix> = fixes.into_iter().filter(|fix| !fix.edits.is_empty()).collect();
    fixes.sort_by_key(|fix| fix.edits.iter().map(|edit| edit.range.start()).min());

    let mut accepted_ranges: Vec<TextRange> = Vec::new();
    let mut out: Vec<TextEdit> = Vec::new();
    for fix in fixes {
        let conflicts = fix.edits.iter().any(|edit| {
            accepted_ranges.iter().any(|accepted| edits_conflict(edit.range, *accepted))
        });
        if conflicts {
            continue;
        }
        for edit in &fix.edits {
            accepted_ranges.push(edit.range);
            out.push(edit.clone());
        }
    }

    out.sort_by_key(|edit| edit.range.start());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use line_index::TextSize;

    fn edit(start: u32, end: u32, new_text: &str) -> TextEdit {
        TextEdit {
            range: TextRange::new(TextSize::new(start), TextSize::new(end)),
            new_text: new_text.to_string(),
        }
    }

    fn fix(edits: Vec<TextEdit>) -> Fix {
        Fix::safe("test", edits)
    }

    #[test]
    fn empty_input_yields_no_edits() {
        assert!(merge_fixes(std::iter::empty::<&Fix>()).is_empty());
    }

    #[test]
    fn non_overlapping_fixes_are_all_kept_and_sorted() {
        let fixes = [fix(vec![edit(50, 55, "b")]), fix(vec![edit(10, 12, "a")])];
        let merged = merge_fixes(fixes.iter());
        let ranges: Vec<_> = merged.iter().map(|e| (e.range.start().into(), &e.new_text)).collect();
        assert_eq!(ranges, vec![(10u32, &"a".to_string()), (50u32, &"b".to_string())]);
    }

    #[test]
    fn overlapping_fix_is_dropped_first_wins() {
        // Both touch [10, 20); the earlier-offset fix is accepted, the later dropped.
        let fixes = [fix(vec![edit(10, 20, "keep")]), fix(vec![edit(15, 25, "drop")])];
        let merged = merge_fixes(fixes.iter());
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].new_text, "keep");
    }

    #[test]
    fn two_insertions_at_same_offset_conflict() {
        let fixes = [fix(vec![edit(10, 10, "a")]), fix(vec![edit(10, 10, "b")])];
        assert_eq!(merge_fixes(fixes.iter()).len(), 1);
    }

    #[test]
    fn multi_edit_fix_is_atomic() {
        // The earlier single-edit fix is accepted first; the multi-edit fix conflicts on
        // its second edit, so neither of its edits lands — including the harmless one.
        let blocker = fix(vec![edit(5, 8, "z")]);
        let multi = fix(vec![edit(20, 22, "x"), edit(6, 7, "y")]);
        let merged = merge_fixes([&blocker, &multi]);
        assert_eq!(merged.len(), 1, "the conflicting multi-edit fix must be fully dropped");
        assert_eq!(merged[0].new_text, "z");
    }
}
