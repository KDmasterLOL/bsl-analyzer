use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsConfig, DiagnosticsContext};

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

pub(crate) fn dispatch(
    config: &DiagnosticsConfig,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::LikeUsage { range, kind: sdbl_hir::LikeUsageKind::Incorrect } =
        diag
    {
        crate::sdbl_utils::dispatch_simple(
            config,
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
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_detects_invalid_like_patterns() {
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IncorrectUseLikeInQuery,
            expect![[r#"
                IncorrectUseLikeInQuery @ 7:9..7:45
                  message: Нужно исправить выражение в соответствии со стандартом
                  severity: Major
                IncorrectUseLikeInQuery @ 8:9..8:49
                  message: Нужно исправить выражение в соответствии со стандартом
                  severity: Major
                IncorrectUseLikeInQuery @ 9:9..9:40
                  message: Нужно исправить выражение в соответствии со стандартом
                  severity: Major
                IncorrectUseLikeInQuery @ 20:17..20:53
                  message: Нужно исправить выражение в соответствии со стандартом
                  severity: Major
                IncorrectUseLikeInQuery @ 21:17..21:49
                  message: Нужно исправить выражение в соответствии со стандартом
                  severity: Major
                IncorrectUseLikeInQuery @ 22:17..22:48
                  message: Нужно исправить выражение в соответствии со стандартом
                  severity: Major
                IncorrectUseLikeInQuery @ 25:16..25:52
                  message: Нужно исправить выражение в соответствии со стандартом
                  severity: Major
                IncorrectUseLikeInQuery @ 28:19..28:50
                  message: Нужно исправить выражение в соответствии со стандартом
                  severity: Major
                IncorrectUseLikeInQuery @ 30:9..30:45
                  message: Нужно исправить выражение в соответствии со стандартом
                  severity: Major
                IncorrectUseLikeInQuery @ 33:11..33:42
                  message: Нужно исправить выражение в соответствии со стандартом
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_correct_like_with_literal() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле1 ПОДОБНО ""шаблон"" КАК Результат ИЗ Т1";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IncorrectUseLikeInQuery,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_correct_like_with_parameter() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле1 ПОДОБНО &Параметр КАК Результат ИЗ Т1";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IncorrectUseLikeInQuery,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_incorrect_like_with_column_ref() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле1 ПОДОБНО Таблица.Поле2 КАК Результат ИЗ Т1";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IncorrectUseLikeInQuery,
            expect![[r#"
                IncorrectUseLikeInQuery @ 3:23..3:50
                  message: Нужно исправить выражение в соответствии со стандартом
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_correct_like_with_function() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ &Параметр ПОДОБНО ПОДСТРОКА(""Строка"", 1, 1) КАК Результат ИЗ Т1";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IncorrectUseLikeInQuery,
            expect![[r#""#]],
        );
    }
}
