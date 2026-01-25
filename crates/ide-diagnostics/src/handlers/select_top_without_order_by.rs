use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use sdbl_hir;

const DEFAULT_SKIP_SELECT_TOP_ONE: bool = true;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::SelectTopWithoutOrderBy;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let skip_select_top_one =
        ctx.config.get_bool(code, "skipSelectTopOne").unwrap_or(DEFAULT_SKIP_SELECT_TOP_ONE);

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
            if let sdbl_hir::SdblDiagnostic::SelectTopWithoutOrderBy {
                top_value,
                in_union,
                has_where,
                range,
            } = hir_diag
            {
                // In UNION: always report (TOP in UNION is always problematic)
                // Otherwise: apply skipSelectTopOne logic
                let should_report = if *in_union {
                    true
                } else if *top_value == 1 || *top_value == 0 {
                    // TOP 1 / TOP 0: skip if skipSelectTopOne=true OR if has WHERE clause
                    !skip_select_top_one && !*has_where
                } else {
                    // TOP N (N > 1): always report when no ORDER BY
                    true
                };

                if should_report {
                    let bsl_range = mapper.map_range(*range, &query_info.query_text);

                    diagnostics.push(Diagnostic {
                        code,
                        message: "Измените запрос, добавив сортировку".to_string(),
                        severity: ctx.severity(code),
                        range: bsl_range,
                        tags: ctx.tags(code),
                        fixes: vec![],
                    });
                }
            }
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_sdbl_diagnostic};
    use crate::DiagnosticCode;

    #[test]
    fn test_from_fixture() {
        let code = include_str!("../test_data/SelectTopWithoutOrderByDiagnostic.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);

        // With default skipSelectTopOne=true, expect 10 diagnostics
        assert_eq!(diagnostics.len(), 10, "Expected 10 diagnostics with default settings");

        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::SelectTopWithoutOrderBy);
        }

        // Fixture line numbers are 0-indexed
        // Процедура Первые10БезСортировки (line 37 in file = 36 0-indexed)
        assert_diagnostic_range(code, &diagnostics[0], 36, 13, 22);
        // Процедура Первые10БезСортировкиИСортировкаВДругомЗапросе (line 55)
        assert_diagnostic_range(code, &diagnostics[1], 54, 13, 22);
        // Процедура Первые10СВПараметрахБезСортировки - subquery (line 77)
        assert_diagnostic_range(code, &diagnostics[2], 76, 20, 29);
        // Процедура Первые10ВоВложенномБезСортировки - nested (line 102)
        assert_diagnostic_range(code, &diagnostics[3], 101, 15, 24);
        // Процедура Первые10СВПараметрахБезСортировкиССортировкойВоВнешнем (line 133)
        assert_diagnostic_range(code, &diagnostics[4], 132, 20, 29);
        // Процедура Первые10ВОбъединенииСУпорядочить - subquery in WHERE (line 149)
        assert_diagnostic_range(code, &diagnostics[5], 148, 20, 29);
        // Процедура Первые10ВОбъединенииСУпорядочить - UNION query (line 156)
        assert_diagnostic_range(code, &diagnostics[6], 155, 13, 22);
        // Процедура Первые10ВОбъединенииСУпорядочить - UNION query TOP 1 (line 163)
        assert_diagnostic_range(code, &diagnostics[7], 162, 13, 21);
        // Nested subquery with UNION - subquery in WHERE (line 182)
        assert_diagnostic_range(code, &diagnostics[8], 181, 24, 33);
        // Nested subquery with UNION - UNION query (line 189)
        assert_diagnostic_range(code, &diagnostics[9], 188, 16, 25);
    }

    #[test]
    fn test_simple_top_without_order_by_russian() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 10 * ИЗ Справочник.Товары";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::SelectTopWithoutOrderBy);
        // Line 2, "ПЕРВЫЕ 10" starts at col 22, ends at col 31
        assert_diagnostic_range(code, &diagnostics[0], 2, 22, 31);
    }

    #[test]
    fn test_simple_top_with_order_by() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 10 * ИЗ Справочник.Товары УПОРЯДОЧИТЬ ПО Наименование";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "TOP with ORDER BY should not trigger");
    }

    #[test]
    fn test_top_1_with_where_clause() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 1 * ИЗ Справочник.Товары ГДЕ Код = &Код";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "TOP 1 with WHERE should not trigger");
    }

    #[test]
    fn test_top_1_without_where_skipped_by_default() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 1 * ИЗ Справочник.Товары";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "TOP 1 without WHERE should be skipped by default");
    }

    #[test]
    fn test_top_in_union() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 10 * ИЗ Т1
             |ОБЪЕДИНИТЬ ВСЕ
             |ВЫБРАТЬ * ИЗ Т2";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "TOP in UNION should always trigger");
        // Line 2, "ПЕРВЫЕ 10" at col 22-31
        assert_diagnostic_range(code, &diagnostics[0], 2, 22, 31);
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    Query = "SELECT TOP 10 * FROM Catalog.Products";
EndProcedure
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "TOP without ORDER BY should trigger");
        // Line 2, "TOP 10" at col 20-26
        assert_diagnostic_range(code, &diagnostics[0], 2, 20, 26);
    }
}
