use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

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

pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::FullOuterJoin { range } = diag {
        crate::sdbl_utils::dispatch_simple(ctx, DiagnosticCode::FullOuterJoinQuery, "Использование FULL OUTER JOIN значительно снижает производительность запроса. Рассмотрите возможность переписать с использованием UNION и LEFT JOIN", *range, mapper, query_text, diagnostics);
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(ctx, DiagnosticCode::FullOuterJoinQuery, dispatch)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_fixture_full_outer_join_detected_left_join_not() {
        let code_test1 = r#"Процедура Тест1()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
                   |    Товары.Номенклатура КАК Номенклатура,
                   |    ЕСТЬNULL(ПланПродаж.Сумма, 0) КАК СуммаПлан,
                   |    ЕСТЬNULL(ФактическиеПродажи.Сумма, 0) КАК СуммаФакт
                   |ИЗ
                   |    Товары КАК Товары
                   |        ЛЕВОЕ СОЕДИНЕНИЕ ПланПродаж КАК ПланПродаж
                   |            ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ ФактическиеПродажи КАК ФактическиеПродажи
                   |            ПО ПланПродаж.Номенклатура = ФактическиеПродажи.Номенклатура
                   |        ПО Товары.Номенклатура = ПланПродаж.Номенклатура";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code_test1,
            DiagnosticCode::FullOuterJoinQuery,
            expect![[r#"
                FullOuterJoinQuery @ 10:33..12:29
                  message: Использование FULL OUTER JOIN значительно снижает производительность запроса. Рассмотрите возможность переписать с использованием UNION и LEFT JOIN
                  severity: Warning"#]],
        );

        let code_test2 = r#"Процедура Тест2()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
                   |    Товары.Номенклатура КАК Номенклатура
                   |ИЗ
                   |    Товары КАК Товары
                   |        ЛЕВОЕ СОЕДИНЕНИЕ ПланПродаж КАК ПланПродаж
                   |            ЛЕВОЕ СОЕДИНЕНИЕ ФактическиеПродажи КАК ФактическиеПродажи
                   |            ПО ПланПродаж.Номенклатура = ФактическиеПродажи.Номенклатура
                   |        ПО Товары.Номенклатура = ПланПродаж.Номенклатура";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code_test2,
            DiagnosticCode::FullOuterJoinQuery,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_simple_english() {
        let code = r#"
Procedure Test()
    Query = "SELECT * FROM T1 FULL JOIN T2 ON T1.ID = T2.ID";
EndProcedure
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FullOuterJoinQuery,
            expect![[r#"
            FullOuterJoinQuery @ 3:31..3:60
              message: Использование FULL OUTER JOIN значительно снижает производительность запроса. Рассмотрите возможность переписать с использованием UNION и LEFT JOIN
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_simple_russian() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ПОЛНОЕ СОЕДИНЕНИЕ Т2 ПО Т1.ID = Т2.ID";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FullOuterJoinQuery,
            expect![[r#"
            FullOuterJoinQuery @ 3:31..3:68
              message: Использование FULL OUTER JOIN значительно снижает производительность запроса. Рассмотрите возможность переписать с использованием UNION и LEFT JOIN
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_no_false_positives_left_join() {
        let code = r#"
Процедура Тест2()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
                   |    Товары.Номенклатура
                   |ИЗ
                   |    Товары КАК Товары
                   |        ЛЕВОЕ СОЕДИНЕНИЕ ПланПродаж";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::FullOuterJoinQuery, expect![[r#""#]]);
    }

    #[test]
    fn test_full_join_without_outer() {
        let code = r#"
Процедура Тест()
    Query = "SELECT * FROM T1 ПОЛНОЕ СОЕДИНЕНИЕ T2 ПО T1.ID = T2.ID";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FullOuterJoinQuery,
            expect![[r#"
            FullOuterJoinQuery @ 3:31..3:68
              message: Использование FULL OUTER JOIN значительно снижает производительность запроса. Рассмотрите возможность переписать с использованием UNION и LEFT JOIN
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_multiple_full_joins() {
        let code = r#"
Процедура Тест()
    Query = "SELECT * FROM T1 FULL OUTER JOIN T2 ON T1.A = T2.A FULL OUTER JOIN T3 ON T1.B = T3.B";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FullOuterJoinQuery,
            expect![[r#"
            FullOuterJoinQuery @ 3:31..3:65
              message: Использование FULL OUTER JOIN значительно снижает производительность запроса. Рассмотрите возможность переписать с использованием UNION и LEFT JOIN
              severity: Warning
            FullOuterJoinQuery @ 3:65..3:98
              message: Использование FULL OUTER JOIN значительно снижает производительность запроса. Рассмотрите возможность переписать с использованием UNION и LEFT JOIN
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_multiline_simple() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ *
             |ИЗ Товары
             |    ПОЛНОЕ СОЕДИНЕНИЕ Продажи
             |    ПО Товары.ID = Продажи.ID";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FullOuterJoinQuery,
            expect![[r#"
            FullOuterJoinQuery @ 5:19..6:44
              message: Использование FULL OUTER JOIN значительно снижает производительность запроса. Рассмотрите возможность переписать с использованием UNION и LEFT JOIN
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_multiline_with_comment() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ *
             |ИЗ Товары
             |    ПОЛНОЕ СОЕДИНЕНИЕ Продажи // тест
             |    ПО Товары.ID = Продажи.ID";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FullOuterJoinQuery,
            expect![[r#"
            FullOuterJoinQuery @ 5:19..6:44
              message: Использование FULL OUTER JOIN значительно снижает производительность запроса. Рассмотрите возможность переписать с использованием UNION и LEFT JOIN
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_nested_joins_like_fixture() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
                   |    Товары.Номенклатура
                   |ИЗ
                   |    Товары КАК Товары
                   |        ЛЕВОЕ СОЕДИНЕНИЕ ПланПродаж КАК ПланПродаж
                   |            ПОЛНОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ ФактическиеПродажи КАК ФактическиеПродажи
                   |            ПО ПланПродаж.Номенклатура = ФактическиеПродажи.Номенклатура
                   |        ПО Товары.Номенклатура = ПланПродаж.Номенклатура";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FullOuterJoinQuery,
            expect![[r#"
            FullOuterJoinQuery @ 9:33..11:29
              message: Использование FULL OUTER JOIN значительно снижает производительность запроса. Рассмотрите возможность переписать с использованием UNION и LEFT JOIN
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_with_function_calls_in_select() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
                   |    Товары.Номенклатура КАК Номенклатура,
                   |    ЕСТЬNULL(ПланПродаж.Сумма, 0) КАК СуммаПлан
                   |ИЗ
                   |    Товары КАК Товары
                   |        ПОЛНОЕ СОЕДИНЕНИЕ ПланПродаж
                   |        ПО Товары.ID = ПланПродаж.ID";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FullOuterJoinQuery,
            expect![[r#"
            FullOuterJoinQuery @ 9:29..10:57
              message: Использование FULL OUTER JOIN значительно снижает производительность запроса. Рассмотрите возможность переписать с использованием UNION и LEFT JOIN
              severity: Warning"#]],
        );
    }
}
