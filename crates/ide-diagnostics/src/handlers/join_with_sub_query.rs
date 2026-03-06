//! JoinWithSubQuery diagnostic.
//!
//! Detects usage of subqueries in JOIN operations in SDBL queries.
//!
//! ## Why?
//! Joins with subqueries cause severe performance issues in 1C:Enterprise.
//! The query optimizer struggles with subqueries in JOINs, leading to:
//! - Extremely slow query execution, especially under low server load
//! - Unpredictable performance (fast sometimes, very slow other times)
//! - Significant execution time differences across different DBMS
//! - Performance degradation over time as statistics become stale
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT T.Ref FROM Catalog.Items AS Ref
//!          LEFT JOIN (SELECT S.Ref FROM Catalog.Suppliers WHERE S.Active = TRUE) AS T
//!          ON Ref.Supplier = T.Ref";
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Use temporary tables or metadata objects instead:
//! Query = "SELECT Suppliers.Ref INTO TempSuppliers
//!          FROM Catalog.Suppliers AS Suppliers WHERE Suppliers.Active = TRUE;
//!
//!          SELECT T.Ref FROM Catalog.Items AS Ref
//!          LEFT JOIN TempSuppliers AS T ON Ref.Supplier = T.Ref";
//! ```
//!
//! ## Implementation
//! Ported from:

use crate::define_metadata;
use crate::metadata::*;
use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use sdbl_hir;
use tracing::debug;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Standard, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Runs the JoinWithSubQuery diagnostic.
///
/// Uses SDBL HIR with diagnostics collected during lowering.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    let code = DiagnosticCode::JoinWithSubQuery;

    if ctx.is_disabled_with_metadata(code) {
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
            if let sdbl_hir::SdblDiagnostic::JoinWithSubQuery { range } = hir_diag {
                let bsl_range = mapper.map_range(*range, &query_info.query_text);

                diagnostics.push(Diagnostic {
                    code,
                    message: "Don't use a join with sub queries. \
                              Joins with subqueries cause severe performance issues."
                        .to_string(),
                    severity: ctx.severity(code),
                    range: bsl_range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
            }
        }
    }

    debug!(
        time_ms = start.elapsed().as_millis(),
        diagnostics_found = diagnostics.len(),
        "JoinWithSubQuery completed"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;
    use crate::{DiagnosticCode, Severity};
    #[test]
    fn test_join_with_sub_query_from_fixture() {
        let code = include_str!("../../test_data/JoinWithSubQueryDiagnostic.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 7, "Expected 7 JoinWithSubQuery diagnostics");

        // Verify all are correct type and severity
        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::JoinWithSubQuery);
            // CodeSmell + Major → Warning (per metadata mapping)
            assert_eq!(diag.severity, Severity::Warning);
        }
    }

    #[test]
    fn test_simple_left_join_with_subquery() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ЛЕВОЕ СОЕДИНЕНИЕ (ВЫБРАТЬ * ИЗ Т2) КАК С ПО Т1.ID = С.ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "LEFT JOIN with subquery should trigger");
    }

    #[test]
    fn test_no_false_positive_table_join() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Справочник.Товары ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Цены ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "JOIN with table should not trigger");
    }

    #[test]
    fn test_no_false_positive_subquery_without_join() {
        let code = r#"
Процедура Тест7()
    Запрос = "ВЫБРАТЬ * ИЗ (ВЫБРАТЬ * ИЗ Т1) КАК С1, (ВЫБРАТЬ * ИЗ Т2) КАК С2";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Subqueries in FROM without JOINs should not trigger");
    }

    #[test]
    fn test_right_join_with_subquery() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ПРАВОЕ СОЕДИНЕНИЕ (ВЫБРАТЬ * ИЗ Т2) КАК С ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "RIGHT JOIN with subquery should trigger");
    }

    #[test]
    fn test_inner_join_with_subquery() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ВНУТРЕННЕЕ СОЕДИНЕНИЕ (ВЫБРАТЬ * ИЗ Т2) ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "INNER JOIN with subquery should trigger");
    }

    #[test]
    fn test_subquery_in_from_with_joins() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ (ВЫБРАТЬ * ИЗ Т1) КАК С ЛЕВОЕ СОЕДИНЕНИЕ Т2 ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Subquery in FROM with JOINs should trigger");
    }

    #[test]
    fn test_multiline_subquery_in_from_with_joins() {
        // This matches Тест4 from the fixture
        let code = r#"
Процедура Тест()
    Запрос = "Выбрать Т.Ссылка
    | Из (Выбрать СС.Ссылка Из Справочник.Справочник1 КАК СС Где СС.Ссылка = &Параметр) как СПр Левое соединение
    |(Выбрать СС.Ссылка Из Справочник.Справочник2 КАК СС Где СС.Ссылка = &Параметр) КАК Т
    |По СПр.Поле1 = Т.Ссылка";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            2,
            "Should detect both: subquery in FROM with JOINs + subquery in JOIN"
        );
    }
}
