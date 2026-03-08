use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Sql, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Single-pass dispatch for IncorrectUseLikeInQuery.
pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::IncorrectUseLikeInQuery { range } = diag {
        crate::sdbl_utils::dispatch_simple(
            ctx,
            DiagnosticCode::IncorrectUseLikeInQuery,
            "Нужно исправить выражение в соответствии со стандартом",
            *range,
            mapper,
            query_text,
            diagnostics,
        );
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::IncorrectUseLikeInQuery,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_sdbl_diagnostic};
    use crate::DiagnosticCode;
    #[test]
    fn test_incorrect_use_like_in_query_from_fixture() {
        let code = r#"Процедура Тест()
    ТекстЗапроса =
    "ВЫБРАТЬ
    |   Таблица.Поле1,
    |   Таблица.Поле1 ПОДОБНО ""Строка"" КАК Поле2,             // <-- Нет ошибки
    |   Таблица.Поле1 ПОДОБНО &Параметр КАК Поле3,              // <-- Нет ошибки
    |   Таблица.Поле1 ПОДОБНО Таблица.Поле99 КАК Поле4,         // <-- Ошибка
    |   ""Строка"" ПОДОБНО ((((Таблица.Поле1)))) КАК Поле5,     // <-- Ошибка
    |   &Параметр ПОДОБНО Таблица.Поле1 КАК Поле6,              // <-- Ошибка
    |   &Параметр ПОДОБНО &Параметр2 КАК Поле7,                 // <-- Нет ошибки
    |   &Параметр ПОДОБНО ""Строка"" КАК Поле8,                 // <-- Нет ошибки
    |   &Параметр ПОДОБНО ПОДСТРОКА(""Строка"", 1, 1) КАК Поле9 // <-- Нет ошибки
    |ИЗ
    |   Документ.Документ КАК Таблица
    |   ЛЕВОЕ СОЕДИНЕНИЕ (
    |       ВЫБРАТЬ
    |          Таблица.Поле1,
    |          Таблица.Поле1 ПОДОБНО ""Строка"" КАК Поле2,      // <-- Нет ошибки
    |           Таблица.Поле1 ПОДОБНО &Параметр КАК Поле3,      // <-- Нет ошибки
    |           Таблица.Поле1 ПОДОБНО Таблица.Поле99 КАК Поле4, // <-- Ошибка
    |           ""Строка"" ПОДОБНО Таблица.Поле1 КАК Поле5,     // <-- Ошибка
    |           &Параметр ПОДОБНО Таблица.Поле1 КАК Поле6       // <-- Ошибка
    |       ИЗ
    |           Документ.Документ2 КАК Таблица) КАК Таблица2
    |       ПО Таблица.Поле1 ПОДОБНО Таблица2.Поле1             // <-- Ошибка
    |           И Таблица.Поле1 ПОДОБНО ""Строка""              // <-- Нет ошибки
    |           И Таблица.Поле1 ПОДОБНО &Параметр               // <-- Нет ошибки
    |           И &Параметр ПОДОБНО Таблица.Поле1               // <-- Ошибка
    |ГДЕ
    |   Таблица.Поле1 ПОДОБНО Таблица2.Поле1                    // <-- Ошибка
    |   И Таблица.Поле1 ПОДОБНО ""Строка""                      // <-- Нет ошибки
    |   И Таблица.Поле1 ПОДОБНО &Параметр                       // <-- Нет ошибки
    |   И &Параметр ПОДОБНО Таблица.Поле1";                     // <-- Ошибка

КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 10, "Expected 10 incorrect LIKE usages");

        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::IncorrectUseLikeInQuery);
        }

        let mut sorted_diagnostics = diagnostics.clone();
        sorted_diagnostics.sort_by_key(|d| d.range.start());

        // Verify diagnostic positions
        assert_diagnostic_range(code, &sorted_diagnostics[0], 6, 8, 44);
        assert_diagnostic_range(code, &sorted_diagnostics[1], 7, 8, 48);
        assert_diagnostic_range(code, &sorted_diagnostics[2], 8, 8, 39);
        assert_diagnostic_range(code, &sorted_diagnostics[3], 19, 16, 52);
        assert_diagnostic_range(code, &sorted_diagnostics[4], 20, 16, 48);
        assert_diagnostic_range(code, &sorted_diagnostics[5], 21, 16, 47);
        assert_diagnostic_range(code, &sorted_diagnostics[6], 24, 15, 51);
        assert_diagnostic_range(code, &sorted_diagnostics[7], 27, 18, 49);
        assert_diagnostic_range(code, &sorted_diagnostics[8], 29, 8, 44);
        assert_diagnostic_range(code, &sorted_diagnostics[9], 32, 10, 41);
    }

    #[test]
    fn test_correct_like_with_literal() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле1 ПОДОБНО ""шаблон"" КАК Результат ИЗ Т1";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "String literal should be valid pattern");
    }

    #[test]
    fn test_correct_like_with_parameter() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле1 ПОДОБНО &Параметр КАК Результат ИЗ Т1";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Parameter should be valid pattern");
    }

    #[test]
    fn test_incorrect_like_with_column_ref() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле1 ПОДОБНО Таблица.Поле2 КАК Результат ИЗ Т1";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Column reference should trigger diagnostic");
    }

    #[test]
    fn test_correct_like_with_function() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ &Параметр ПОДОБНО ПОДСТРОКА(""Строка"", 1, 1) КАК Результат ИЗ Т1";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Function call should be valid pattern");
    }
}
