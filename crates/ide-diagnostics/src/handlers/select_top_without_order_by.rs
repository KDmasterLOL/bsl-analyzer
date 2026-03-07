use crate::define_metadata;
use crate::metadata::*;
use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use sdbl_hir;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Sql, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

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
    fn test_top_10_in_batch_order_by_in_other_query() {
        // TOP 10 in first batch query; second query has ORDER BY - still 1 diagnostic
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 10
             |   Справочник.Ссылка
             |ИЗ
             |   Справочник.Контрагенты КАК Справочник
             |;
             |///////////////////////////////////////////////
             |ВЫБРАТЬ
             |   Справочник.Ссылка
             |ИЗ
             |   Справочник.Пользователи КАК Справочник
             |УПОРЯДОЧИТЬ ПО
             |   Справочник.Код";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "TOP 10 without ORDER BY should trigger even when another batch query has ORDER BY"
        );
        assert_eq!(diagnostics[0].code, DiagnosticCode::SelectTopWithoutOrderBy);
        assert_diagnostic_range(code, &diagnostics[0], 2, 22, 31);
    }

    #[test]
    fn test_top_10_in_where_in_subquery() {
        // TOP 10 inside a WHERE ... IN (...) subquery - 1 diagnostic
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |   Справочник.Ссылка
             |ИЗ
             |   Справочник.Контрагенты КАК Справочник
             |ГДЕ
             |   Справочник.Ссылка В (
             |       ВЫБРАТЬ ПЕРВЫЕ 10
             |           Ссылка
             |       ИЗ
             |           Справочник.Контрагенты)";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "TOP 10 in WHERE IN subquery without ORDER BY should trigger"
        );
        assert_eq!(diagnostics[0].code, DiagnosticCode::SelectTopWithoutOrderBy);
        assert_diagnostic_range(code, &diagnostics[0], 8, 29, 38);
    }

    #[test]
    fn test_top_1_in_where_in_subquery_skipped_by_default() {
        // TOP 1 inside a WHERE ... IN (...) subquery - 0 diagnostics with default skipSelectTopOne=true
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |   Справочник.Ссылка
             |ИЗ
             |   Справочник.Контрагенты КАК Справочник
             |ГДЕ
             |   Справочник.Ссылка В (
             |       ВЫБРАТЬ ПЕРВЫЕ 1
             |           Ссылка
             |       ИЗ
             |           Справочник.Контрагенты)";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "TOP 1 in WHERE IN subquery should be skipped by default skipSelectTopOne=true"
        );
    }

    #[test]
    fn test_top_10_in_nested_from_subquery() {
        // TOP 10 inside a nested FROM subquery - outer query has ORDER BY but inner does not - 1 diagnostic
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 5
             |   Справочник.Ссылка КАК Ссылка
             |ИЗ
             |   (ВЫБРАТЬ ПЕРВЫЕ 10
             |       Контрагенты.Ссылка КАК Ссылка
             |   ИЗ
             |       Справочник.Контрагенты КАК Контрагенты) КАК Справочник
             |УПОРЯДОЧИТЬ ПО
             |   Ссылка";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "TOP 10 in nested FROM subquery without ORDER BY should trigger"
        );
        assert_eq!(diagnostics[0].code, DiagnosticCode::SelectTopWithoutOrderBy);
        assert_diagnostic_range(code, &diagnostics[0], 5, 26, 35);
    }

    #[test]
    fn test_top_1_in_nested_from_subquery_skipped_by_default() {
        // TOP 1 inside a nested FROM subquery - 0 diagnostics with default skipSelectTopOne=true
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 5
             |   Справочник.Ссылка КАК Ссылка
             |ИЗ
             |   (ВЫБРАТЬ ПЕРВЫЕ 1
             |       Контрагенты.Ссылка КАК Ссылка
             |   ИЗ
             |       Справочник.Контрагенты КАК Контрагенты) КАК Справочник
             |УПОРЯДОЧИТЬ ПО
             |   Ссылка";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "TOP 1 in nested FROM subquery should be skipped by default skipSelectTopOne=true"
        );
    }

    #[test]
    fn test_top_10_in_where_in_subquery_order_by_only_in_outer() {
        // TOP 10 in WHERE IN subquery; ORDER BY is only on the outer query - still 1 diagnostic
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |   Справочник.Ссылка
             |ИЗ
             |   Справочник.Контрагенты КАК Справочник
             |ГДЕ
             |   Справочник.Ссылка В (
             |       ВЫБРАТЬ ПЕРВЫЕ 10
             |           Ссылка
             |       ИЗ
             |           Справочник.Контрагенты)
             |УПОРЯДОЧИТЬ ПО
             |   Справочник.Код";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            1,
            "TOP 10 in WHERE IN subquery should trigger even when outer query has ORDER BY"
        );
        assert_eq!(diagnostics[0].code, DiagnosticCode::SelectTopWithoutOrderBy);
        assert_diagnostic_range(code, &diagnostics[0], 8, 29, 38);
    }

    #[test]
    fn test_union_with_where_subquery_and_union_members() {
        // Query with WHERE IN subquery having TOP 10, then UNION ALL with TOP 10 and TOP 1 - 3 diagnostics
        // TOP in UNION members is always reported regardless of skipSelectTopOne
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |   Справочник.Ссылка
             |ИЗ
             |   Справочник.Контрагенты КАК Справочник
             |ГДЕ
             |   Справочник.Ссылка В (
             |       ВЫБРАТЬ ПЕРВЫЕ 10
             |           Ссылка
             |       ИЗ
             |           Справочник.Контрагенты)
             |
             |ОБЪЕДИНИТЬ ВСЕ
             |
             |ВЫБРАТЬ ПЕРВЫЕ 10
             |   Справочник.Ссылка
             |ИЗ
             |   Справочник.Контрагенты КАК Справочник
             |
             |ОБЪЕДИНИТЬ ВСЕ
             |
             |ВЫБРАТЬ ПЕРВЫЕ 1
             |   Справочник.Ссылка
             |ИЗ
             |   Справочник.Контрагенты КАК Справочник
             |УПОРЯДОЧИТЬ ПО
             |   Ссылка";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            3,
            "Expected 3 diagnostics: WHERE subquery TOP 10, UNION TOP 10, UNION TOP 1"
        );
        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::SelectTopWithoutOrderBy);
        }
        // WHERE IN subquery TOP 10
        assert_diagnostic_range(code, &diagnostics[0], 8, 29, 38);
        // UNION ALL member TOP 10
        assert_diagnostic_range(code, &diagnostics[1], 15, 22, 31);
        // UNION ALL member TOP 1 (always reported in UNION)
        assert_diagnostic_range(code, &diagnostics[2], 22, 22, 30);
    }

    #[test]
    fn test_complex_union_with_nested_subqueries() {
        // Outer TOP 1 with ORDER BY (no diag); inner subquery has UNION with TOP 10 in WHERE and TOP 10 in UNION member
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 1
             |   ПодЗапрос.Ссылка
             |ИЗ
             |   (ВЫБРАТЬ
             |       Справочник.Ссылка
             |   ИЗ
             |       Справочник.Контрагенты КАК Справочник
             |   ГДЕ
             |       Справочник.Ссылка В (
             |           ВЫБРАТЬ ПЕРВЫЕ 10
             |               Ссылка
             |           ИЗ
             |               Справочник.Контрагенты)
             |
             |   ОБЪЕДИНИТЬ ВСЕ
             |
             |   ВЫБРАТЬ ПЕРВЫЕ 10
             |       Справочник.Ссылка
             |   ИЗ
             |       Справочник.Контрагенты КАК Справочник) КАК ПодЗапрос
             |
             |УПОРЯДОЧИТЬ ПО
             |   Ссылка";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            2,
            "Expected 2 diagnostics: nested WHERE subquery TOP 10 and nested UNION TOP 10"
        );
        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::SelectTopWithoutOrderBy);
        }
        // Nested WHERE IN subquery TOP 10
        assert_diagnostic_range(code, &diagnostics[0], 11, 33, 42);
        // Nested UNION ALL member TOP 10
        assert_diagnostic_range(code, &diagnostics[1], 18, 25, 34);
    }

    #[test]
    fn test_parameter_substitution_in_union_no_top() {
        // Query with &Parameter substitution in UNION member, no TOP - 0 diagnostics
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |   ОстаткиНоменклатуры.Номенклатура КАК Номенклатура
             |ПОМЕСТИТЬ ВТ_ОстаткиНоменклатуры
             |ИЗ
             |   (ВЫБРАТЬ
             |       ОстаткиНаКонецПериода.Номенклатура КАК Номенклатура
             |   ИЗ
             |       РегистрНакопления.ТоварыОрганизаций.Остатки() КАК ОстаткиНаКонецПериода
             |
             |   ОБЪЕДИНИТЬ ВСЕ
             |
             |   ВЫБРАТЬ &ТекстОстаткиПоМесяцам) КАК ОстаткиНоменклатуры";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Query with &Parameter substitution and no TOP should not trigger"
        );
    }

    #[test]
    fn test_top_0_with_order_by() {
        // TOP 0 with ORDER BY - 0 diagnostics
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 0
             |   Истина КАК Поле
             |УПОРЯДОЧИТЬ ПО
             |   Поле";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "TOP 0 with ORDER BY should not trigger");
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
