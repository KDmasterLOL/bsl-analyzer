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
            if let sdbl_hir::SdblDiagnostic::FieldsFromJoinWithoutNullCheck {
                join_type,
                range: _,
                unprotected_fields,
            } = hir_diag
            {
                let join_type_str = match join_type {
                    sdbl_hir::JoinType::Left => "ЛЕВОГО СОЕДИНЕНИЯ",
                    sdbl_hir::JoinType::Right => "ПРАВОГО СОЕДИНЕНИЯ",
                    sdbl_hir::JoinType::Full => "ПОЛНОГО СОЕДИНЕНИЯ",
                    _ => "СОЕДИНЕНИЯ",
                };

                let message = format!(
                    "Для полей из {} добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ",
                    join_type_str
                );

                // Create one diagnostic per unprotected field, highlighting the field itself
                for field_ref in unprotected_fields {
                    let bsl_range = mapper.map_range(field_ref.range, query_text);

                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::FieldsFromJoinsWithoutIsNull,
                        message: message.clone(),
                        severity: Severity::Critical,
                        range: bsl_range,
                        tags: vec![],
                        fixes: vec![],
                    });
                }
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
        "FieldsFromJoinsWithoutIsNull completed (HIR-based)"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;

    #[test]
    fn test_fields_from_joins_without_is_null() {
        let code = include_str!("../../test_data/FieldsFromJoinsWithoutIsNullDiagnostic.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);

        // Per-field diagnostic mode: one diagnostic per unprotected field reference.
        // Java implementation emitted 9 diagnostics (one per JOIN), but we now emit
        // one per field for better UX - highlighting the exact field that needs protection.
        // Test8 (FULL JOIN) has 3 unprotected fields: lines 99, 100, 101.

        if diagnostics.len() != 11 {
            // Debug: print all diagnostic locations
            eprintln!("\n=== Found {} diagnostics (expected 11) ===", diagnostics.len());
            for (i, diag) in diagnostics.iter().enumerate() {
                let start_line = code[..diag.range.start().into()].lines().count();
                eprintln!("Diagnostic {}: line {} - {}", i, start_line, diag.message);
            }
        }

        // Per-field implementation: one diagnostic per unprotected field reference.
        assert_eq!(diagnostics.len(), 11, "Expected 11 diagnostics (one per unprotected field)");
    }

    #[test]
    fn test_diagnostic_highlights_field_not_join() {
        // Verify diagnostic highlights the unprotected field, not the JOIN clause
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос("ВЫБРАТЬ
        |    ЗадачиИсполнителей.Исполнитель,
        |    ИсполнениеРезультатыПроверки.Комментарий КАК Комментарий
        |ИЗ
        |    БизнесПроцесс.Исполнение.РезультатыПроверки КАК ИсполнениеРезультатыПроверки
        |        ЛЕВОЕ СОЕДИНЕНИЕ Задача.ЗадачаИсполнителя КАК ЗадачиИсполнителей
        |        ПО ИсполнениеРезультатыПроверки.ЗадачаИсполнителя = ЗадачиИсполнителей.Ссылка
        |ГДЕ
        |    ИсполнениеРезультатыПроверки.ЗадачаПроверяющего = &ЗадачаПроверяющего");
КонецПроцедуры"#;

        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for unprotected field");

        // Extract the highlighted text from the diagnostic range
        let diag = &diagnostics[0];
        let highlighted = &code[diag.range.start().into()..diag.range.end().into()];

        // Should highlight "ЗадачиИсполнителей.Исполнитель", not "ЛЕВОЕ СОЕДИНЕНИЕ..."
        assert!(
            highlighted.contains("ЗадачиИсполнителей"),
            "Diagnostic should highlight the field 'ЗадачиИсполнителей.Исполнитель', got: '{}'",
            highlighted
        );
        assert!(
            !highlighted.contains("СОЕДИНЕНИЕ") && !highlighted.contains("JOIN"),
            "Diagnostic should NOT highlight the JOIN clause, got: '{}'",
            highlighted
        );
    }
}
