//! FieldsFromJoinsWithoutIsNull diagnostic.
//!
//! Checks that fields from LEFT/RIGHT/FULL JOINs are protected with NULL checks.
//!
//! ## Why?
//! When using LEFT, RIGHT, or FULL JOINs in SDBL queries, fields from the joined table
//! can be NULL even if rows exist. Accessing these fields without NULL protection can cause:
//! - Unexpected query results
//! - Runtime errors in 1C:Enterprise
//! - Incorrect business logic execution
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT Employee.Ref FROM Document.Order AS Orders
//!         |LEFT JOIN Catalog.Employees AS Employee
//!         |  ON Orders.Employee = Employee.Ref";
//!         // Error: Employee.Ref can be NULL, needs ISNULL() or IS NULL check
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Option 1: Use ISNULL function
//! Query = "SELECT ISNULL(Employee.Ref, NULL) FROM Document.Order AS Orders
//!         |LEFT JOIN Catalog.Employees AS Employee
//!         |  ON Orders.Employee = Employee.Ref";
//!
//! // Option 2: Use IS NULL operator
//! Query = "SELECT Employee.Ref FROM Document.Order AS Orders
//!         |LEFT JOIN Catalog.Employees AS Employee
//!         |  ON Orders.Employee = Employee.Ref
//!         |WHERE Employee.Ref IS NOT NULL";
//!
//! // Option 3: Use INNER JOIN instead (if semantically correct)
//! Query = "SELECT Employee.Ref FROM Document.Order AS Orders
//!         |INNER JOIN Catalog.Employees AS Employee
//!         |  ON Orders.Employee = Employee.Ref";
//! ```
//!
//! ## Rules
//! - Checks LEFT JOIN, RIGHT JOIN, FULL JOIN (INNER JOIN is safe)
//! - Fields must be protected with:
//!   - `ISNULL(field, defaultValue)` function
//!   - `field IS NULL` or `field IS NOT NULL` operator
//!   - `NOT (field IS NULL)` negation pattern
//!   - Global WHERE clause with `IS NOT NULL` exempts all field usage
//! - Bilingual support: ЛЕВОЕ/LEFT, ПРАВОЕ/RIGHT, ПОЛНОЕ/FULL
//! - Checks three contexts: SELECT, WHERE, JOIN ON conditions
//!
//! ## Implementation
//!
//! Ported from:
//! - FieldsFromJoinsWithoutIsNullDiagnostic.java (bsl-language-server)
//! - Rust SDBL utilities (bsl-language-server-rust)
//!
//! Source: `~/src/lsp/bsl-language-server/src/test/resources/diagnostics/FieldsFromJoinsWithoutIsNullDiagnostic.bsl`

use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use tracing::debug;

/// Runs the FieldsFromJoinsWithoutIsNull diagnostic.
///
/// Uses SDBL HIR with diagnostics collected during lowering.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::FieldsFromJoinsWithoutIsNull) {
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
            if let sdbl_hir::SdblDiagnostic::FieldsFromJoinWithoutNullCheck {
                join_type,
                range,
                unprotected_fields: _, // Future: use for RelatedInformation
            } = hir_diag
            {
                let bsl_range = mapper.map_range(*range, query_text);

                let join_type_str = match join_type {
                    sdbl_hir::JoinType::Left => "LEFT JOIN",
                    sdbl_hir::JoinType::Right => "RIGHT JOIN",
                    sdbl_hir::JoinType::Full => "FULL JOIN",
                    _ => "JOIN",
                };

                let message = format!(
                    "For fields from {} add field checks via IS NULL or use conversion via ISNULL or use INNER JOIN",
                    join_type_str
                );

                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::FieldsFromJoinsWithoutIsNull,
                    message,
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
        let mapper = SdblPositionMapper::new_from_range_with_line_index(
            query_info.bsl_literal_range,
            &bsl_source,
            &line_starts,
        );

        // Extract diagnostics recursively from all queries (including UNION subqueries)
        for query in sdbl_package.queries() {
            extract_diagnostics(&query.hir, &mapper, &query_info.query_text, &mut diagnostics);
        }
    }

    debug!(
        time_ms = start.elapsed().as_millis(),
        diagnostics_found = diagnostics.len(),
        "FieldsFromJoinsWithoutIsNull completed (HIR-based)"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to run diagnostic on BSL code
    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        use crate::test_utils::check_sdbl_diagnostic;

        check_sdbl_diagnostic(code, check)
    }

    #[test]
    fn test_fields_from_joins_without_is_null() {
        let code = include_str!("../../test_data/FieldsFromJoinsWithoutIsNullDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        // HIR-based implementation with WHERE IS NOT NULL protection semantics.
        // Java reference implementation expects 9 diagnostics.
        // WHERE IS NOT NULL for any field from a table protects the entire table.

        if diagnostics.len() != 9 {
            // Debug: print all diagnostic locations
            eprintln!("\n=== Found {} diagnostics (expected 9) ===", diagnostics.len());
            for (i, diag) in diagnostics.iter().enumerate() {
                let start_line = code[..diag.range.start().into()].lines().count();
                eprintln!("Diagnostic {}: line {} - {}", i, start_line, diag.message);
            }
        }

        // HIR-based implementation with recursive UNION subquery checking.
        // Each UNION subquery has independent scope and WHERE protection.
        assert_eq!(diagnostics.len(), 9, "Expected 9 diagnostics matching Java implementation");
    }
}
