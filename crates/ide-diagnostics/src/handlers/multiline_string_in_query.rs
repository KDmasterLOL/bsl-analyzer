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
    let sdbl_hirs = ctx.sdbl_hir_in_file();

    let bsl_source = ctx.file_text();

    // Get SDBL queries for position mapping
    let sdbl_queries = ctx.all_sdbl_in_file();

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
    for ((_expr_id, sdbl_package), (_query_expr_id, query_info)) in
        sdbl_hirs.iter().zip(sdbl_queries.iter())
    {
        let mapper = SdblPositionMapper::from_query_info(query_info, &bsl_source, &line_starts);

        // Extract diagnostics recursively from all queries (including UNION subqueries)
        for query in sdbl_package.queries() {
            extract_diagnostics(&query.hir, &mapper, &query_info.query_text, &mut diagnostics);
        }
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
    use super::check;
    use crate::test_utils::{assert_diagnostic_range_multiline, check_sdbl_diagnostic};
    use crate::{DiagnosticCode, Severity};

    #[test]
    fn test_multiline_string_in_query() {
        let code = include_str!("../../test_data/MultilineStringInQueryDiagnostic.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 3);

        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::MultilineStringInQuery);
            assert_eq!(diag.severity, Severity::Critical);
            assert_eq!(diag.message, "Check if multiline literal is correct");
        }

        assert_diagnostic_range_multiline(code, &diagnostics[0], 5, 8, 6, 5);
        assert_diagnostic_range_multiline(code, &diagnostics[1], 6, 31, 10, 10);
        assert_diagnostic_range_multiline(code, &diagnostics[2], 15, 60, 16, 68);
    }
}
