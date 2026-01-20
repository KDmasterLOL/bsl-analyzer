//! QueryNestedFieldsByDot diagnostic.
//!
//! Detects nested field dereference by dot in SDBL queries (N+1 problem).
//!
//! ## Why?
//! Accessing reference fields through multiple dots (e.g., `T.Ссылка.Организация`)
//! causes N+1 query problem - for each row, an additional database query is executed.
//!
//! ## Bad practice
//! ```bsl
//! Query.Text = "SELECT
//! |   T.Ссылка.Организация AS Organization  // N+1 problem
//! |FROM Document.Order.Items AS T";
//! ```
//!
//! ## Good practice
//! Use JOINs to fetch related data in a single query.
//!
//! ## Implementation
//!
//! Analyzes SDBL HIR structure to find:
//! 1. ColumnRef with 3+ parts (e.g., `T.Ссылка.Организация`)
//! 2. ColumnRef with 2+ parts inside virtual table parameters (implicit join)
//! 3. FunctionCall (CAST) with 2+ member_access fields

use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use sdbl_hir::{ExprHir, FunctionKind, InValues, TableRef};
use syntax::TextRange;
use tracing::debug;

/// Runs the QueryNestedFieldsByDot diagnostic.
///
/// Analyzes SDBL HIR structure to find nested field dereferences.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::QueryNestedFieldsByDot) {
        return Vec::new();
    }

    let sdbl_hirs = ctx.sdbl_hir_in_file();
    let bsl_source = ctx.file_text();
    let sdbl_queries = ctx.all_sdbl_in_file();

    use crate::sdbl_utils::build_line_index_shared;
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    for ((_expr_id, sdbl_package), (_query_expr_id, query_info)) in
        sdbl_hirs.iter().zip(sdbl_queries.iter())
    {
        let mapper = SdblPositionMapper::from_query_info(query_info, &bsl_source, &line_starts);

        for query in sdbl_package.queries() {
            let mut ranges = Vec::new();
            collect_nested_field_ranges(&query.hir, &mut ranges);

            for range in ranges {
                let bsl_range = mapper.map_range(range, &query_info.query_text);
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::QueryNestedFieldsByDot,
                    message: "Обнаружено разыменование ссылочного поля".to_string(),
                    severity: Severity::Warning,
                    range: bsl_range,
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }
    }

    debug!(
        time_ms = start.elapsed().as_millis(),
        diagnostics_found = diagnostics.len(),
        "QueryNestedFieldsByDot completed (HIR analysis)"
    );

    diagnostics
}

/// Collect all nested field dereference ranges from HIR.
fn collect_nested_field_ranges(hir: &sdbl_hir::SdblHir, ranges: &mut Vec<TextRange>) {
    // Check SELECT fields
    for field in &hir.select.fields {
        collect_from_expr(&field.expr, false, ranges);
    }

    // Check FROM tables (including virtual table params)
    for table in &hir.from {
        collect_from_table_ref(table, ranges);
    }

    // Check JOINs
    for join in &hir.joins {
        collect_from_table_ref(&join.table, ranges);
        if let Some(ref cond) = join.condition {
            collect_from_expr(cond, false, ranges);
        }
    }

    // Check WHERE
    if let Some(ref where_expr) = hir.where_clause {
        collect_from_expr(where_expr, false, ranges);
    }

    // Check GROUP BY
    if let Some(ref group_by) = hir.group_by {
        for expr in &group_by.exprs {
            collect_from_expr(expr, false, ranges);
        }
    }

    // Check HAVING
    if let Some(ref having) = hir.having {
        collect_from_expr(having, false, ranges);
    }

    // Check ORDER BY
    if let Some(ref order_by) = hir.order_by {
        for item in &order_by.items {
            collect_from_expr(&item.expr, false, ranges);
        }
    }

    // Check UNION subqueries recursively
    for union in &hir.unions {
        collect_nested_field_ranges(&union.query, ranges);
    }
}

/// Collect from table reference (including virtual table params).
fn collect_from_table_ref(table: &TableRef, ranges: &mut Vec<TextRange>) {
    // Check virtual table parameters - here even 2-part paths are dereferences
    if table.is_virtual_table {
        for param in &table.virtual_table_params {
            collect_from_expr(param, true, ranges);
        }
    }

    // Check subqueries
    for subquery in &table.subquery {
        collect_nested_field_ranges(subquery, ranges);
    }
}

/// Collect nested field references from expression.
///
/// `in_virtual_table_params`: if true, even 2-part column refs are considered dereferences.
fn collect_from_expr(expr: &ExprHir, in_virtual_table_params: bool, ranges: &mut Vec<TextRange>) {
    match expr {
        ExprHir::ColumnRef { parts, range, .. } => {
            // Check for nested field dereference
            let is_nested = if in_virtual_table_params {
                // Inside virtual table params: 2+ parts (if not MDO type)
                parts.len() >= 2 && !is_mdo_type(parts[0].as_str())
            } else {
                // Normal context: 3+ parts (if not MDO type)
                parts.len() >= 3 && !is_mdo_type(parts[0].as_str())
            };

            if is_nested {
                ranges.push(*range);
            }
        }

        ExprHir::FunctionCall { function, args, member_access, range, .. } => {
            // Check args recursively
            for arg in args {
                collect_from_expr(arg, in_virtual_table_params, ranges);
            }

            // CAST with 2+ member access fields is a dereference
            if matches!(function, FunctionKind::Cast) && member_access.len() > 1 {
                ranges.push(*range);
            }
        }

        ExprHir::BinaryOp { lhs, rhs, .. } => {
            collect_from_expr(lhs, in_virtual_table_params, ranges);
            collect_from_expr(rhs, in_virtual_table_params, ranges);
        }

        ExprHir::UnaryOp { expr: inner, .. } => {
            collect_from_expr(inner, in_virtual_table_params, ranges);
        }

        ExprHir::Case { operand, when_clauses, else_expr, .. } => {
            if let Some(op) = operand {
                collect_from_expr(op, in_virtual_table_params, ranges);
            }
            for clause in when_clauses {
                collect_from_expr(&clause.condition, in_virtual_table_params, ranges);
                collect_from_expr(&clause.result, in_virtual_table_params, ranges);
            }
            if let Some(else_e) = else_expr {
                collect_from_expr(else_e, in_virtual_table_params, ranges);
            }
        }

        ExprHir::Subquery { query, .. } => {
            collect_nested_field_ranges(query, ranges);
        }

        ExprHir::In { expr: inner, values, .. } => {
            collect_from_expr(inner, in_virtual_table_params, ranges);
            match values {
                InValues::List(items) => {
                    for item in items {
                        collect_from_expr(item, in_virtual_table_params, ranges);
                    }
                }
                InValues::Subquery(sq) => {
                    collect_nested_field_ranges(sq, ranges);
                }
            }
        }

        ExprHir::Between { expr: inner, low, high, .. } => {
            collect_from_expr(inner, in_virtual_table_params, ranges);
            collect_from_expr(low, in_virtual_table_params, ranges);
            collect_from_expr(high, in_virtual_table_params, ranges);
        }

        ExprHir::Like { expr: inner, pattern, escape, .. } => {
            collect_from_expr(inner, in_virtual_table_params, ranges);
            collect_from_expr(pattern, in_virtual_table_params, ranges);
            if let Some(esc) = escape {
                collect_from_expr(esc, in_virtual_table_params, ranges);
            }
        }

        ExprHir::IsNull { expr: inner, .. } => {
            collect_from_expr(inner, in_virtual_table_params, ranges);
        }

        ExprHir::Tuple { elements, .. } => {
            for elem in elements {
                collect_from_expr(elem, in_virtual_table_params, ranges);
            }
        }

        // Leaf nodes - no recursion needed
        ExprHir::Literal { .. } | ExprHir::Parameter { .. } | ExprHir::Missing { .. } => {}
    }
}

/// Check if string is an MDO type (Справочник, Документ, etc.)
fn is_mdo_type(s: &str) -> bool {
    sdbl_hir::is_mdo_type(s)
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_sdbl_diagnostic};
    use crate::{DiagnosticCode, Severity};

    #[test]
    fn test_query_nested_fields_by_dot() {
        let code = include_str!("../../test_data/QueryNestedFieldsByDotDiagnostic.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);

        // Expected: 9 diagnostics.
        //
        // Found diagnostics:
        // Query 1 (SELECT + WHERE):
        // - Line 22: ЗаказКлиентаТовары.Ссылка.Организация (3 parts)
        // - Line 23: ЗаказКлиентаТовары.Ссылка.Контрагент (3 parts)
        // - Line 24: ЗаказКлиентаТовары.Ссылка.Партнер (3 parts)
        // - Line 25: ЗаказКлиентаТовары.Ссылка.ОбъектРасчетов (3 parts)
        // - Line 30: ЗаказКлиентаТовары.Ссылка.Дата (3 parts, WHERE clause)
        //
        // Query 4 (JOIN ON clause):
        // - Line 102: ВТ_РасчетыСКлиентами.АналитикаУчетаПоПартнерам.Партнер (3 parts)
        // - Line 103: ВТ_РасчетыСКлиентами.АналитикаУчетаПоПартнерам.Контрагент (3 parts)
        // - Line 104: ВТ_РасчетыСКлиентами.АналитикаУчетаПоПартнерам.Организация (3 parts)
        //
        // Query 5 (CAST member access):
        // - Line 116: ВЫРАЗИТЬ(...).Валюта.Наценка (2 fields after CAST)
        //
        // NOT found (architectural difference with Java bsl-ls):
        // - Line 54: АналитикаУчетаПоПартнерам.Партнер etc. (only 2 parts)
        //   Java bsl-ls has special handling for virtual table params where
        //   even 2-part paths are considered dereferences. Our implementation
        //   requires 3+ parts consistently. This is a deliberate simplification.
        //
        // Java expects 12, we find 9 (difference: 3 virtual table 2-part paths).
        // Debug: print all diagnostics with their line positions
        for (i, diag) in diagnostics.iter().enumerate() {
            let offset: usize = diag.range.start().into();
            let line = code[..offset].matches('\n').count();
            eprintln!("Diagnostic {}: line {} range={:?}", i, line + 1, diag.range);
        }

        assert_eq!(diagnostics.len(), 12, "Expected 12 diagnostics, got {}", diagnostics.len());

        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::QueryNestedFieldsByDot);
            assert_eq!(diag.severity, Severity::Warning);
            assert_eq!(diag.message, "Обнаружено разыменование ссылочного поля");
        }

        // Verify first diagnostic position (line 22 in BSL file, 0-indexed = 21)
        // "|<tab><tab>ЗаказКлиентаТовары.Ссылка.Организация " - col 3 to 41 (0-indexed)
        assert_diagnostic_range(code, &diagnostics[0], 21, 3, 41);
    }

    #[test]
    fn test_no_false_positives_for_mdo_types() {
        // Should NOT trigger for MDO type paths like "Справочник.Валюты.Код"
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ Справочник.Валюты.Код ИЗ Справочник.Валюты";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert!(diagnostics.is_empty(), "MDO type paths should not trigger diagnostic");
    }

    #[test]
    fn test_no_false_positives_for_two_parts() {
        // Should NOT trigger for simple 2-part paths like "T.Поле"
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ T.Ссылка ИЗ Документ.Заказ КАК T";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert!(diagnostics.is_empty(), "Two-part paths should not trigger diagnostic");
    }
}
