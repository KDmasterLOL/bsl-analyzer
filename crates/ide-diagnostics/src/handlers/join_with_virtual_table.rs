//! JoinWithVirtualTable diagnostic.
//!
//! Detects usage of virtual tables in JOIN operations in SDBL queries.
//!
//! ## Why?
//! Joins with virtual tables cause performance issues in 1C:Enterprise.
//! Virtual tables (СрезПоследних, Остатки, Обороты, etc.) are computed on-the-fly
//! and joining with them creates unpredictable performance characteristics.
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT T.Ref FROM Catalog.Items AS Items
//!          LEFT JOIN InformationRegister.Prices.SliceLast AS T
//!          ON Items.Ref = T.Item";
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Use temporary tables or separate queries instead:
//! Query = "SELECT Prices.* INTO TempPrices
//!          FROM InformationRegister.Prices.SliceLast AS Prices;
//!
//!          SELECT T.Ref FROM Catalog.Items AS Items
//!          LEFT JOIN TempPrices AS T ON Items.Ref = T.Item";
//! ```
//!
//! ## Implementation
//! Ported from:
//! - JoinWithVirtualTableDiagnostic.java (bsl-language-server)

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

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    let code = DiagnosticCode::JoinWithVirtualTable;

    if ctx.is_disabled_with_metadata(code) {
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

        for hir_diag in sdbl_package.all_diagnostics() {
            if let sdbl_hir::SdblDiagnostic::JoinWithVirtualTable { range, .. } = hir_diag {
                let bsl_range = mapper.map_range(*range, &query_info.query_text);

                diagnostics.push(Diagnostic {
                    code,
                    message: "Не следует использовать соединения с виртуальными таблицами"
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
        "JoinWithVirtualTable completed"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;
    use crate::{DiagnosticCode, Severity};

    #[test]
    fn test_join_with_virtual_table_from_fixture() {
        use crate::test_utils::assert_diagnostic_range;
        let code = include_str!("../../test_data/JoinWithVirtualTableDiagnostic.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 5, "Expected 5 JoinWithVirtualTable diagnostics");

        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::JoinWithVirtualTable);
            assert_eq!(diag.severity, Severity::Warning);
        }

        // Sort diagnostics by position for predictable order
        let mut sorted = diagnostics.clone();
        sorted.sort_by_key(|d| d.range.start());

        // Verify positions match Java test expectations (lines must match, columns +-1)
        // Java: hasRange(3, 84, 119)
        assert_diagnostic_range(code, &sorted[0], 3, 84, 120);
        // Java: hasRange(12, 5, 56)
        assert_diagnostic_range(code, &sorted[1], 12, 5, 56);
        // Java: hasRange(22, 5, 56)
        assert_diagnostic_range(code, &sorted[2], 22, 5, 56);
        // Java: hasRange(31, 9, 53)
        assert_diagnostic_range(code, &sorted[3], 31, 9, 53);
        // Java: hasRange(33, 5, 56)
        assert_diagnostic_range(code, &sorted[4], 33, 5, 56);
    }

    #[test]
    fn test_simple_join_with_virtual_table() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ЛЕВОЕ СОЕДИНЕНИЕ РегистрСведений.Курсы.СрезПоследних КАК Т ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "JOIN with virtual table should trigger");
    }

    #[test]
    fn test_no_false_positive_regular_table() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Справочник.Товары ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Цены ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "JOIN with regular table should not trigger");
    }

    #[test]
    fn test_no_false_positive_virtual_table_without_join() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрНакопления.Склады.Остатки(Склад = &Параметр) КАК Р";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Virtual table in FROM without JOIN should not trigger");
    }

    #[test]
    fn test_virtual_table_in_from_with_join() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ РегистрСведений.Курсы.СрезПоследних(&Период) КАК К ЛЕВОЕ СОЕДИНЕНИЕ Т2 ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Virtual table in FROM with JOIN should trigger");
    }

    #[test]
    fn test_multiple_virtual_tables_in_joins() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ *
    |ИЗ Справочник.Товары
    |ЛЕВОЕ СОЕДИНЕНИЕ РегистрСведений.Курсы.СрезПоследних КАК К ПО ID
    |ЛЕВОЕ СОЕДИНЕНИЕ РегистрНакопления.Склады.Остатки КАК О ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 2, "Should detect both virtual tables in JOINs");
    }
}
