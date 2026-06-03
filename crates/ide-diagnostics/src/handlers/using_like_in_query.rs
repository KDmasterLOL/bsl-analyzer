use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Adaptable,
};

pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::LikeUsage { range, .. } = diag {
        crate::sdbl_utils::dispatch_simple(
            ctx,
            DiagnosticCode::UsingLikeInQuery,
            "Измените выражение, чтобы не использовать 'ПОДОБНО'",
            *range,
            mapper,
            query_text,
            diagnostics,
        );
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(ctx, DiagnosticCode::UsingLikeInQuery, dispatch)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_detects_like_usages_in_query_fixture() {
        let code = r#"Процедура Тест()
    ТекстЗапроса =
    "ВЫБРАТЬ
    |   Таблица.Поле1,
    |   Таблица.Поле1 ПОДОБНО ""Строка"" КАК Поле2,             // <-- Ошибка
    |   Таблица.Поле1 ПОДОБНО &Параметр КАК Поле3,              // <-- Ошибка
    |   Таблица.Поле1 ПОДОБНО Таблица.Поле99 КАК Поле4,         // <-- Ошибка
    |   ""Строка"" ПОДОБНО ((((Таблица.Поле1)))) КАК Поле5,     // <-- Ошибка
    |   &Параметр ПОДОБНО Таблица.Поле1 КАК Поле6,              // <-- Ошибка
    |   &Параметр ПОДОБНО &Параметр2 КАК Поле7,                 // <-- Ошибка
    |   &Параметр ПОДОБНО ""Строка"" КАК Поле8,                 // <-- Ошибка
    |   &Параметр ПОДОБНО ПОДСТРОКА(""Строка"", 1, 1) КАК Поле9 // <-- Ошибка
    |ИЗ
    |   Документ.Документ КАК Таблица
    |   ЛЕВОЕ СОЕДИНЕНИЕ (
    |       ВЫБРАТЬ
    |          Таблица.Поле1,
    |          Таблица.Поле1 ПОДОБНО ""Строка"" КАК Поле2,      // <-- Ошибка
    |           Таблица.Поле1 ПОДОБНО &Параметр КАК Поле3,      // <-- Ошибка
    |           Таблица.Поле1 ПОДОБНО Таблица.Поле99 КАК Поле4, // <-- Ошибка
    |           ""Строка"" ПОДОБНО Таблица.Поле1 КАК Поле5,     // <-- Ошибка
    |           &Параметр ПОДОБНО Таблица.Поле1 КАК Поле6       // <-- Ошибка
    |       ИЗ
    |           Документ.Документ2 КАК Таблица) КАК Таблица2
    |       ПО Таблица.Поле1 ПОДОБНО Таблица2.Поле1             // <-- Ошибка
    |           И Таблица.Поле1 ПОДОБНО ""Строка""              // <-- Ошибка
    |           И Таблица.Поле1 ПОДОБНО &Параметр               // <-- Ошибка
    |           И &Параметр ПОДОБНО Таблица.Поле1               // <-- Ошибка
    |ГДЕ
    |   Таблица.Поле1 ПОДОБНО Таблица2.Поле1                    // <-- Ошибка
    |   И Таблица.Поле1 ПОДОБНО ""Строка""                      // <-- Ошибка
    |   И Таблица.Поле1 ПОДОБНО &Параметр                       // <-- Ошибка
    |   И &Параметр ПОДОБНО Таблица.Поле1";                     // <-- Ошибка

КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingLikeInQuery,
            expect![[r#"
                UsingLikeInQuery @ 5:9..5:41
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 6:9..6:40
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 7:9..7:45
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 8:9..8:49
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 9:9..9:40
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 10:9..10:37
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 11:9..11:37
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 12:9..12:54
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 18:16..18:48
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 19:17..19:48
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 20:17..20:53
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 21:17..21:49
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 22:17..22:48
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 25:16..25:52
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 26:19..26:51
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 27:19..27:50
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 28:19..28:50
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 30:9..30:45
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 31:11..31:43
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 32:11..32:42
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 33:11..33:42
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_simple_like_english() {
        let code = r#"
Procedure Test()
    Query = "SELECT Field1 LIKE 'pattern' AS Result FROM T1";
EndProcedure
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingLikeInQuery,
            expect![[r#"
                UsingLikeInQuery @ 3:21..3:34
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_simple_like_russian() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле1 ПОДОБНО ""шаблон"" КАК Результат ИЗ Т1";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingLikeInQuery,
            expect![[r#"
                UsingLikeInQuery @ 3:23..3:47
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_not_like() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле1 НЕ ПОДОБНО ""шаблон"" КАК Результат ИЗ Т1";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingLikeInQuery,
            expect![[r#"
                UsingLikeInQuery @ 3:23..3:50
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_like_in_where() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ГДЕ Поле1 ПОДОБНО ""шаблон""";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingLikeInQuery,
            expect![[r#"
                UsingLikeInQuery @ 3:35..3:59
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_like_in_join() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ЛЕВОЕ СОЕДИНЕНИЕ Т2 ПО Т1.Поле1 ПОДОБНО Т2.Поле2";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingLikeInQuery,
            expect![[r#"
                UsingLikeInQuery @ 3:54..3:79
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_multiple_likes() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |   Т1.Поле1 ПОДОБНО ""a"" КАК П1,
             |   Т1.Поле2 ПОДОБНО ""b"" КАК П2
             |ИЗ Т1
             |ГДЕ Т1.Поле3 ПОДОБНО ""c""";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UsingLikeInQuery,
            expect![[r#"
                UsingLikeInQuery @ 4:18..4:40
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 5:18..5:40
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major
                UsingLikeInQuery @ 7:19..7:41
                  message: Измените выражение, чтобы не использовать 'ПОДОБНО'
                  severity: Major"#]],
        );
    }
}
