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
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Sql, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    let code = DiagnosticCode::IncorrectUseLikeInQuery;

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
            if let sdbl_hir::SdblDiagnostic::IncorrectUseLikeInQuery { range } = hir_diag {
                let bsl_range = mapper.map_range(*range, &query_info.query_text);

                diagnostics.push(Diagnostic {
                    code,
                    message: "Нужно исправить выражение в соответствии со стандартом".to_string(),
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
        "IncorrectUseLikeInQuery completed"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_sdbl_diagnostic};
    use crate::DiagnosticCode;
    #[test]
    fn test_incorrect_use_like_in_query_from_fixture() {
        let code = include_str!("../test_data/IncorrectUseLikeInQueryDiagnostic.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 10, "Expected 10 incorrect LIKE usages");

        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::IncorrectUseLikeInQuery);
        }

        let mut sorted_diagnostics = diagnostics.clone();
        sorted_diagnostics.sort_by_key(|d| d.range.start());

        // Verify diagnostic positions (lines must match bsl-language-server, columns may differ by +-1)
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
