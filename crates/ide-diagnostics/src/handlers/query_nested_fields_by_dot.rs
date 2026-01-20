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
//! Uses SDBL HIR with diagnostics collected during lowering.
//! Detects:
//! 1. ColumnRef with 3+ parts (e.g., `T.Ссылка.Организация`)
//! 2. ColumnRef with 2+ parts inside virtual table parameters (implicit join)
//! 3. FunctionCall (CAST) with 2+ member_access fields

use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use sdbl_hir;
use tracing::debug;

/// Runs the QueryNestedFieldsByDot diagnostic.
///
/// Uses SDBL HIR with diagnostics collected during lowering.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::QueryNestedFieldsByDot) {
        return Vec::new();
    }

    // Get SDBL HIR with collected diagnostics
    let sdbl_hirs = ctx.sdbl_hir_in_file();

    let bsl_source = ctx.file_text();

    // Get cached SDBL queries for position mapping
    let sdbl_queries = ctx.all_sdbl_in_file();

    // Build shared line index
    use crate::sdbl_utils::build_line_index_shared;
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    // Iterate SDBL HIRs and corresponding query infos in parallel
    // Both are sorted by position in file, so we can zip them
    for ((_expr_id, sdbl_package), (_query_expr_id, query_info)) in
        sdbl_hirs.iter().zip(sdbl_queries.iter())
    {
        let mapper = SdblPositionMapper::from_query_info(query_info, &bsl_source, &line_starts);

        // Emit diagnostics from HIR
        for hir_diag in sdbl_package.all_diagnostics() {
            if let sdbl_hir::SdblDiagnostic::QueryNestedFieldsByDot { range } = hir_diag {
                let bsl_range = mapper.map_range(*range, &query_info.query_text);

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
        "QueryNestedFieldsByDot completed"
    );

    diagnostics
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

        // Expected: 12 diagnostics.
        //
        // Found diagnostics:
        // Query 1 (SELECT + WHERE):
        // - Line 22: ЗаказКлиентаТовары.Ссылка.Организация (3 parts)
        // - Line 23: ЗаказКлиентаТовары.Ссылка.Контрагент (3 parts)
        // - Line 24: ЗаказКлиентаТовары.Ссылка.Партнер (3 parts)
        // - Line 25: ЗаказКлиентаТовары.Ссылка.ОбъектРасчетов (3 parts)
        // - Line 30: ЗаказКлиентаТовары.Ссылка.Дата (3 parts, WHERE clause)
        //
        // Query 2 (virtual table params):
        // - Line 54: АналитикаУчетаПоПартнерам.Партнер (2 parts in virtual table)
        // - Line 55: АналитикаУчетаПоПартнерам.Контрагент (2 parts in virtual table)
        // - Line 56: АналитикаУчетаПоПартнерам.Организация (2 parts in virtual table)
        //
        // Query 4 (JOIN ON clause):
        // - Line 102: ВТ_РасчетыСКлиентами.АналитикаУчетаПоПартнерам.Партнер (3 parts)
        // - Line 103: ВТ_РасчетыСКлиентами.АналитикаУчетаПоПартнерам.Контрагент (3 parts)
        // - Line 104: ВТ_РасчетыСКлиентами.АналитикаУчетаПоПартнерам.Организация (3 parts)
        //
        // Query 5 (CAST member access):
        // - Line 116: ВЫРАЗИТЬ(...).Валюта.Наценка (2 fields after CAST)
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
