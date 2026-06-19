//! Pure helpers for routing diagnostics through an `&ИзменениеИКонтроль` effective
//! module (Phase 2, increment 3). The Salsa wiring that constructs the effective
//! context and runs the inference collector lives in `lib.rs`; everything here is
//! db-free so it can be unit-tested in isolation.

use hir::{Origin, Segment};
use syntax::{SyntaxKind, SyntaxNode, TextRange, TextSize};

use crate::Diagnostic;

/// Body (`STMT_LIST`) ranges of every `&ИзменениеИКонтроль` method in an extension
/// module. Standalone diagnostics of the inference class that fall inside one of these
/// are copied-base false positives (the verbatim base statements reference base-module
/// siblings absent from the standalone ext file) — the orchestrator suppresses them and
/// republishes the merged-module result instead.
pub(crate) fn cav_body_ranges(root: &SyntaxNode) -> Vec<TextRange> {
    root.children()
        .filter(|n| matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF))
        .filter(|method| hir::extract_change_and_validate(method).is_some())
        .filter_map(|method| {
            method.children().find(|n| n.kind() == SyntaxKind::STMT_LIST).map(|n| n.text_range())
        })
        .collect()
}

/// `true` when `range` is fully contained in any of `ranges`.
pub(crate) fn range_inside_any(range: TextRange, ranges: &[TextRange]) -> bool {
    ranges.iter().any(|r| r.contains_range(range))
}

/// Keep only the diagnostics whose (effective-coordinate) range falls fully inside an
/// `Inserted` segment, remapping each to the extension-source range it was authored at.
/// Effective diagnostics outside any `Inserted` segment — copied base statements and the
/// untouched base methods/module code — are dropped: they belong to the base file's own
/// diagnostics, not the extension's.
///
/// Within a single [`Segment`] the effective and ext runs are equal-length and contiguous
/// (the merge engine emits a fresh segment at every origin/contiguity break), so the remap
/// is an exact linear shift. A diagnostic spanning two segments has no single containing
/// `Inserted` segment and is dropped (a negligible recall gap for the single-token
/// resolution diagnostics published here).
pub(crate) fn remap_inserted(diags: Vec<Diagnostic>, segments: &[Segment]) -> Vec<Diagnostic> {
    diags
        .into_iter()
        .filter_map(|mut diag| {
            let seg = segments
                .iter()
                .find(|s| s.origin == Origin::Inserted && s.effective.contains_range(diag.range))?;
            let offset = diag.range.start() - seg.effective.start();
            let start = seg.ext.start() + offset;
            let len: TextSize = diag.range.len();
            diag.range = TextRange::at(start, len);
            Some(diag)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiagnosticCode, Severity};

    fn parse_root(code: &str) -> SyntaxNode {
        parser::parse(code).syntax_node()
    }

    fn diag(range: TextRange) -> Diagnostic {
        Diagnostic {
            code: DiagnosticCode::UnresolvedMethodCall,
            message: "x".into(),
            severity: Severity::Warning,
            range,
            tags: Vec::new(),
            fixes: Vec::new(),
        }
    }

    fn r(start: u32, end: u32) -> TextRange {
        TextRange::new(TextSize::new(start), TextSize::new(end))
    }

    #[test]
    fn cav_body_ranges_only_change_and_validate_methods() {
        // One change-and-validate method + one ordinary method; only the former's body
        // range is collected.
        let code = "&ИзменениеИКонтроль(\"M\")\n\
Процедура Расш1_M()\n\
\tА = 1;\n\
КонецПроцедуры\n\
\n\
Процедура Обычная()\n\
\tБ = 2;\n\
КонецПроцедуры";
        let ranges = cav_body_ranges(&parse_root(code));
        assert_eq!(ranges.len(), 1, "exactly one change-and-validate body");
        // The collected range covers the CAV body and excludes the ordinary method.
        let slice = &code[usize::from(ranges[0].start())..usize::from(ranges[0].end())];
        assert!(slice.contains("А = 1;"), "covers the CAV body: {slice:?}");
        assert!(!slice.contains("Б = 2;"), "excludes the ordinary method: {slice:?}");
    }

    #[test]
    fn remap_inserted_keeps_inserted_shifts_to_ext() {
        // effective 100..110 maps to ext 40..50 (Inserted); a diagnostic at effective
        // 102..105 remaps to ext 42..45.
        let segments = vec![
            Segment { effective: r(0, 100), ext: r(0, 40), origin: Origin::Copied },
            Segment { effective: r(100, 110), ext: r(40, 50), origin: Origin::Inserted },
        ];
        let kept = remap_inserted(vec![diag(r(102, 105))], &segments);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].range, r(42, 45), "linear shift into ext coordinates");
    }

    #[test]
    fn remap_inserted_drops_copied_and_unmapped() {
        let segments = vec![
            Segment { effective: r(0, 100), ext: r(0, 40), origin: Origin::Copied },
            Segment { effective: r(100, 110), ext: r(40, 50), origin: Origin::Inserted },
        ];
        // Inside a Copied segment → dropped; outside every segment → dropped.
        let kept = remap_inserted(vec![diag(r(10, 20)), diag(r(200, 210))], &segments);
        assert!(kept.is_empty(), "non-inserted diagnostics are dropped");
    }

    #[test]
    fn remap_inserted_drops_span_across_segments() {
        let segments = vec![
            Segment { effective: r(0, 100), ext: r(0, 40), origin: Origin::Copied },
            Segment { effective: r(100, 110), ext: r(40, 50), origin: Origin::Inserted },
        ];
        // 95..105 straddles the Copied→Inserted boundary → no single containing Inserted
        // segment → dropped.
        let kept = remap_inserted(vec![diag(r(95, 105))], &segments);
        assert!(kept.is_empty(), "boundary-spanning diagnostics are dropped");
    }
}
