use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use sdbl_hir;
use tracing::debug;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    let code = DiagnosticCode::UnionAll;

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
            if let sdbl_hir::SdblDiagnostic::UnionWithoutAll { range } = hir_diag {
                let bsl_range = mapper.map_range(*range, &query_info.query_text);

                diagnostics.push(Diagnostic {
                    code,
                    message: "Использование ключевого слова ОБЪЕДИНИТЬ без ВСЕ приводит к \
                              излишней обработке для удаления дубликатов. Используйте ОБЪЕДИНИТЬ ВСЕ"
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
        "UnionAll completed"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;
    use crate::DiagnosticCode;

    #[test]
    fn test_union_all_from_fixture() {
        use crate::test_utils::assert_diagnostic_range;

        let code = include_str!("../test_data/UnionAllDiagnostic.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 2, "Expected 2 UNION without ALL");

        assert_eq!(diagnostics[0].code, DiagnosticCode::UnionAll);
        assert!(diagnostics[0].message.contains("ОБЪЕДИНИТЬ"));

        assert_diagnostic_range(code, &diagnostics[0], 21, 5, 15);
        assert_diagnostic_range(code, &diagnostics[1], 56, 5, 15);
    }

    #[test]
    fn test_simple_english() {
        let code = r#"
Procedure Test()
    Query = "SELECT * FROM T1 UNION SELECT * FROM T2";
EndProcedure
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "UNION without ALL should trigger");
    }

    #[test]
    fn test_simple_russian() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ОБЪЕДИНИТЬ ВЫБРАТЬ * ИЗ Т2";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "ОБЪЕДИНИТЬ without ВСЕ should trigger");
    }

    #[test]
    fn test_no_false_positives_union_all() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ОБЪЕДИНИТЬ ВСЕ ВЫБРАТЬ * ИЗ Т2";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "UNION ALL should not trigger");
    }

    #[test]
    fn test_multiple_unions() {
        let code = r#"
Процедура Тест()
    Query = "SELECT * FROM T1 UNION SELECT * FROM T2 UNION SELECT * FROM T3";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 2, "Should detect 2 UNIONs without ALL");
    }

    #[test]
    fn test_mixed_union_and_union_all() {
        let code = r#"
Процедура Тест()
    Query = "SELECT * FROM T1 UNION ALL SELECT * FROM T2 UNION SELECT * FROM T3";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect 1 UNION without ALL");
    }

    #[test]
    fn test_multiline_query() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ *
             |ИЗ Товары
             |
             |ОБЪЕДИНИТЬ
             |
             |ВЫБРАТЬ *
             |ИЗ Продажи";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect UNION in multiline query");
    }
}
