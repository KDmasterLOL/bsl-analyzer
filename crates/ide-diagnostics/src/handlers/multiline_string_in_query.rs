//! MultilineStringInQuery diagnostic.
//!
//! Detects multiline string literals in SDBL queries.
//!
//! ## Why?
//! Multiline string literals in SDBL queries are very rare and usually indicate
//! an error from incorrect number of double quotes. In SDBL, to represent an
//! empty string you should use """" (4 quotes), not "" (2 quotes).
//!
//! ## Bad practice
//! ```bsl
//! Query.Text = "SELECT
//! |   ЕСТЬNULL(Field, "") AS Code  // Wrong: "" becomes multiline string
//! |FROM Table";
//! ```
//!
//! ## Good practice
//! ```bsl
//! Query.Text = "SELECT
//! |   ЕСТЬNULL(Field, """") AS Code  // Correct: """" is empty string in SDBL
//! |FROM Table";
//! ```
//!
//! ## Implementation
//!
//! Migrated to HIR-based approach for consistency with other SDBL diagnostics.
//! Diagnostics are collected during HIR lowering when processing string literals.

use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use tracing::debug;

/// Runs the MultilineStringInQuery diagnostic.
///
/// Uses SDBL HIR with diagnostics collected during lowering.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::MultilineStringInQuery) {
        return Vec::new();
    }

    // Get SDBL HIR with collected diagnostics
    let sdbl_hirs = ctx.db.sdbl_hir_in_file(ctx.file_id);

    let input = ctx.db.file_text_input(ctx.file_id);
    let bsl_source = input.text(ctx.db);

    // Get SDBL queries for position mapping
    let sdbl_queries = ctx.db.all_sdbl_in_file(ctx.file_id);

    // Build shared line index (optimization)
    use crate::sdbl_utils::build_line_index_shared;
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    // Helper function to recursively extract diagnostics from HIR and UNION subqueries
    fn extract_diagnostics(
        hir: &sdbl_hir::SdblHir,
        mapper: &SdblPositionMapper,
        query_text: &str,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Extract diagnostics from current query
        for hir_diag in &hir.diagnostics {
            if let sdbl_hir::SdblDiagnostic::MultilineString { range } = hir_diag {
                let bsl_range = mapper.map_range(*range, query_text);

                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::MultilineStringInQuery,
                    message: "Check if multiline literal is correct".to_string(),
                    severity: Severity::Critical,
                    range: bsl_range,
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }

        // Recursively extract diagnostics from UNION subqueries
        for union in &hir.unions {
            extract_diagnostics(&union.query, mapper, query_text, diagnostics);
        }
    }

    // Process HIR diagnostics
    for ((_expr_id, sdbl_hir), (_query_expr_id, query_info)) in
        sdbl_hirs.iter().zip(sdbl_queries.iter())
    {
        let mapper = SdblPositionMapper::new_from_range_with_line_index(
            query_info.bsl_literal_range,
            &bsl_source,
            &line_starts,
        );

        // Extract diagnostics recursively (including UNION subqueries)
        extract_diagnostics(sdbl_hir, &mapper, &query_info.query_text, &mut diagnostics);
    }

    debug!(
        time_ms = start.elapsed().as_millis(),
        diagnostics_found = diagnostics.len(),
        "MultilineStringInQuery completed (HIR-based)"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_sdbl_diagnostic;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let diagnostics = check_sdbl_diagnostic(code, check);
        // Extract content from the code (remove fixture header if present)
        let content = code.strip_prefix("//- /test.bsl\n").unwrap_or(code).to_string();
        (diagnostics, content)
    }

    #[test]
    fn test_multiline_string_in_query() {
        let code = include_str!("../../test_data/MultilineStringInQueryDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 3);

        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::MultilineStringInQuery);
            assert_eq!(diag.severity, Severity::Critical);
            assert_eq!(diag.message, "Check if multiline literal is correct");
        }

        use crate::test_utils::assert_diagnostic_range_multiline;

        assert_diagnostic_range_multiline(&file_content, &diagnostics[0], 5, 8, 6, 5);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[1], 6, 30, 10, 10);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[2], 15, 60, 16, 68);
    }
}
