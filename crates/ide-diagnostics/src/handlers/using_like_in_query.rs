use crate::define_metadata;
use crate::metadata::*;
use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use sdbl_hir;
use tracing::debug;

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

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    let code = DiagnosticCode::UsingLikeInQuery;

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
            if let sdbl_hir::SdblDiagnostic::UsingLikeInQuery { range } = hir_diag {
                let bsl_range = mapper.map_range(*range, &query_info.query_text);

                diagnostics.push(Diagnostic {
                    code,
                    message: "Измените выражение, чтобы не использовать 'ПОДОБНО'".to_string(),
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
        "UsingLikeInQuery completed"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_sdbl_diagnostic};
    use crate::DiagnosticCode;
    #[test]
    fn test_using_like_in_query_from_fixture() {
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
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 21, "Expected 21 LIKE usages");

        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::UsingLikeInQuery);
            assert!(diag.message.contains("ПОДОБНО"));
        }

        let mut sorted_diagnostics = diagnostics.clone();
        sorted_diagnostics.sort_by_key(|d| d.range.start());

        // Verify all 21 diagnostic positions match bsl-language-server test expectations
        // Note: Some positions differ by 1 char due to quote correction differences
        assert_diagnostic_range(code, &sorted_diagnostics[0], 4, 8, 40);
        assert_diagnostic_range(code, &sorted_diagnostics[1], 5, 8, 39);
        assert_diagnostic_range(code, &sorted_diagnostics[2], 6, 8, 44);
        assert_diagnostic_range(code, &sorted_diagnostics[3], 7, 8, 48);
        assert_diagnostic_range(code, &sorted_diagnostics[4], 8, 8, 39);
        assert_diagnostic_range(code, &sorted_diagnostics[5], 9, 8, 36);
        assert_diagnostic_range(code, &sorted_diagnostics[6], 10, 8, 36);
        assert_diagnostic_range(code, &sorted_diagnostics[7], 11, 8, 53);
        assert_diagnostic_range(code, &sorted_diagnostics[8], 17, 15, 47);
        assert_diagnostic_range(code, &sorted_diagnostics[9], 18, 16, 47);
        assert_diagnostic_range(code, &sorted_diagnostics[10], 19, 16, 52);
        assert_diagnostic_range(code, &sorted_diagnostics[11], 20, 16, 48);
        assert_diagnostic_range(code, &sorted_diagnostics[12], 21, 16, 47);
        assert_diagnostic_range(code, &sorted_diagnostics[13], 24, 15, 51);
        assert_diagnostic_range(code, &sorted_diagnostics[14], 25, 18, 50);
        assert_diagnostic_range(code, &sorted_diagnostics[15], 26, 18, 49);
        assert_diagnostic_range(code, &sorted_diagnostics[16], 27, 18, 49);
        assert_diagnostic_range(code, &sorted_diagnostics[17], 29, 8, 44);
        assert_diagnostic_range(code, &sorted_diagnostics[18], 30, 10, 42);
        assert_diagnostic_range(code, &sorted_diagnostics[19], 31, 10, 41);
        assert_diagnostic_range(code, &sorted_diagnostics[20], 32, 10, 41);
    }

    #[test]
    fn test_simple_like_english() {
        let code = r#"
Procedure Test()
    Query = "SELECT Field1 LIKE 'pattern' AS Result FROM T1";
EndProcedure
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "LIKE should trigger diagnostic");
    }

    #[test]
    fn test_simple_like_russian() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле1 ПОДОБНО ""шаблон"" КАК Результат ИЗ Т1";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "ПОДОБНО should trigger diagnostic");
    }

    #[test]
    fn test_not_like() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле1 НЕ ПОДОБНО ""шаблон"" КАК Результат ИЗ Т1";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "NOT LIKE should also trigger diagnostic");
    }

    #[test]
    fn test_like_in_where() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ГДЕ Поле1 ПОДОБНО ""шаблон""";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "LIKE in WHERE should trigger diagnostic");
    }

    #[test]
    fn test_like_in_join() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ЛЕВОЕ СОЕДИНЕНИЕ Т2 ПО Т1.Поле1 ПОДОБНО Т2.Поле2";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "LIKE in JOIN condition should trigger diagnostic");
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 3, "Should detect all 3 LIKE usages");
    }
}
