use crate::define_metadata;
use crate::metadata::*;
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

pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::SelectTopWithoutOrderBy {
        top_value,
        in_union,
        has_where,
        range,
    } = diag
    {
        let skip_select_top_one = ctx
            .config
            .get_bool(DiagnosticCode::SelectTopWithoutOrderBy, "skipSelectTopOne")
            .unwrap_or(DEFAULT_SKIP_SELECT_TOP_ONE);

        let should_report = if *in_union {
            true
        } else if *top_value == 1 || *top_value == 0 {
            !skip_select_top_one && !*has_where
        } else {
            true
        };

        if should_report {
            let code = DiagnosticCode::SelectTopWithoutOrderBy;
            diagnostics.push(Diagnostic {
                code,
                message: "Измените запрос, добавив сортировку".to_string(),
                severity: ctx.severity(code),
                range: mapper.map_range(*range, query_text),
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::SelectTopWithoutOrderBy,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    fn check_snapshot(code: &str, expected: expect_test::Expect) {
        check_diagnostics_snapshot_for(code, DiagnosticCode::SelectTopWithoutOrderBy, expected);
    }
    #[test]
    fn test_top_10_in_batch_order_by_in_other_query() {
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
        check_snapshot(
            code,
            expect![[r#"
            SelectTopWithoutOrderBy @ 3:23..3:32
              message: Измените запрос, добавив сортировку
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_top_10_in_where_in_subquery() {
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
        check_snapshot(
            code,
            expect![[r#"
            SelectTopWithoutOrderBy @ 9:30..9:39
              message: Измените запрос, добавив сортировку
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_top_1_in_where_in_subquery_skipped_by_default() {
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
        check_snapshot(code, expect![[r#""#]]);
    }

    #[test]
    fn test_top_10_in_nested_from_subquery() {
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
        check_snapshot(
            code,
            expect![[r#"
            SelectTopWithoutOrderBy @ 6:27..6:36
              message: Измените запрос, добавив сортировку
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_top_1_in_nested_from_subquery_skipped_by_default() {
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
        check_snapshot(code, expect![[r#""#]]);
    }

    #[test]
    fn test_top_10_in_where_in_subquery_order_by_only_in_outer() {
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
        check_snapshot(
            code,
            expect![[r#"
            SelectTopWithoutOrderBy @ 9:30..9:39
              message: Измените запрос, добавив сортировку
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_union_with_where_subquery_and_union_members() {
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
        check_snapshot(
            code,
            expect![[r#"
            SelectTopWithoutOrderBy @ 9:30..9:39
              message: Измените запрос, добавив сортировку
              severity: Warning
            SelectTopWithoutOrderBy @ 16:23..16:32
              message: Измените запрос, добавив сортировку
              severity: Warning
            SelectTopWithoutOrderBy @ 23:23..23:31
              message: Измените запрос, добавив сортировку
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_complex_union_with_nested_subqueries() {
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
        check_snapshot(
            code,
            expect![[r#"
            SelectTopWithoutOrderBy @ 12:34..12:43
              message: Измените запрос, добавив сортировку
              severity: Warning
            SelectTopWithoutOrderBy @ 19:26..19:35
              message: Измените запрос, добавив сортировку
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_parameter_substitution_in_union_no_top() {
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
        check_snapshot(code, expect![[r#""#]]);
    }

    #[test]
    fn test_top_0_with_order_by() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 0
             |   Истина КАК Поле
             |УПОРЯДОЧИТЬ ПО
             |   Поле";
КонецПроцедуры
"#;
        check_snapshot(code, expect![[r#""#]]);
    }

    #[test]
    fn test_simple_top_without_order_by_russian() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 10 * ИЗ Справочник.Товары";
КонецПроцедуры
"#;
        check_snapshot(
            code,
            expect![[r#"
            SelectTopWithoutOrderBy @ 3:23..3:32
              message: Измените запрос, добавив сортировку
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_simple_top_with_order_by() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 10 * ИЗ Справочник.Товары УПОРЯДОЧИТЬ ПО Наименование";
КонецПроцедуры
"#;
        check_snapshot(code, expect![[r#""#]]);
    }

    #[test]
    fn test_top_1_with_where_clause() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 1 * ИЗ Справочник.Товары ГДЕ Код = &Код";
КонецПроцедуры
"#;
        check_snapshot(code, expect![[r#""#]]);
    }

    #[test]
    fn test_top_1_without_where_skipped_by_default() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ ПЕРВЫЕ 1 * ИЗ Справочник.Товары";
КонецПроцедуры
"#;
        check_snapshot(code, expect![[r#""#]]);
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
        check_snapshot(
            code,
            expect![[r#"
            SelectTopWithoutOrderBy @ 3:23..3:32
              message: Измените запрос, добавив сортировку
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    Query = "SELECT TOP 10 * FROM Catalog.Products";
EndProcedure
"#;
        check_snapshot(
            code,
            expect![[r#"
            SelectTopWithoutOrderBy @ 3:21..3:27
              message: Измените запрос, добавив сортировку
              severity: Warning"#]],
        );
    }
}
