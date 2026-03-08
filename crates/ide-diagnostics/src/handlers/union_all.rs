use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Sql, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Adaptable,
};

/// Single-pass dispatch for UnionAll.
pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::UnionWithoutAll { range } = diag {
        crate::sdbl_utils::dispatch_simple(ctx, DiagnosticCode::UnionAll, "Использование ключевого слова ОБЪЕДИНИТЬ без ВСЕ приводит к излишней обработке для удаления дубликатов. Используйте ОБЪЕДИНИТЬ ВСЕ", *range, mapper, query_text, diagnostics);
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(ctx, DiagnosticCode::UnionAll, dispatch)
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;
    use crate::DiagnosticCode;

    #[test]
    fn test_union_all_from_fixture() {
        use crate::test_utils::assert_diagnostic_range;
        let code = r#"Запрос = Новый Запрос(
    "ВЫБРАТЬ РАЗРЕШЕННЫЕ ПЕРВЫЕ 1
    |   Справочник1.Поле1 КАК Поле1,
    |   1 КАК Поле2,
    |   Справочник1.Поле3 КАК Поле3
    |ИЗ
    |   Справочник.Справочник1 КАК Справочник1
    |ГДЕ
    |	Справочник1.Ссылка = &Ссылка1
    |
    |ОБЪЕДИНИТЬ ВСЕ
    |
    |ВЫБРАТЬ ПЕРВЫЕ 1
    |   Справочник2.Поле1 КАК Поле1,
    |   2 КАК Поле2,
    |   Справочник2.Поле3 КАК Поле3
    |ИЗ
    |	Справочник.Справочник2 КАК Справочник2
    |ГДЕ
    |	Справочник2.Ссылка = &Ссылка2
    |
    |ОБЪЕДИНИТЬ
    |
    |ВЫБРАТЬ ПЕРВЫЕ 1
    |   Справочник3.Поле1 КАК Поле1,
    |	3 КАК Поле2,
    |	Справочник3.Поле3 КАК Поле3
    |ИЗ
    |	Справочник.Справочник3 КАК Справочник3
    |ГДЕ
    |   Справочник3.Ссылка = &Ссылка3"
);

Запрос = Новый Запрос(
    "ВЫБРАТЬ РАЗРЕШЕННЫЕ ПЕРВЫЕ 1
    |   Справочник1.Поле1 КАК Поле1,
    |   1 КАК Поле2,
    |   Справочник1.Поле3 КАК Поле3,
    |   ЕСТЬNULL(НачислениеЗарплатыМонтажникамОбороты.Заявка.Адрес, """") КАК Поле4
    |ИЗ
    |   Справочник.Справочник1 КАК Справочник1
    |ГДЕ
    |	Справочник1.Ссылка = &Ссылка1
    |
    |ОБЪЕДИНИТЬ ВСЕ
    |
    |ВЫБРАТЬ ПЕРВЫЕ 1
    |   Справочник2.Поле1 КАК Поле1,
    |   2 КАК Поле2,
    |   Справочник2.Поле3 КАК Поле3,
    |   ЕСТЬNULL(НачислениеЗарплатыМонтажникамОбороты.Заявка.Адрес, """") КАК Поле4
    |ИЗ
    |	Справочник.Справочник2 КАК Справочник2
    |ГДЕ
    |	Справочник2.Ссылка = &Ссылка2
    |
    |ОБЪЕДИНИТЬ
    |
    |ВЫБРАТЬ ПЕРВЫЕ 1
    |   Справочник3.Поле1 КАК Поле1,
    |	3 КАК Поле2,
    |	Справочник3.Поле3 КАК Поле3,
    |   ЕСТЬNULL(НачислениеЗарплатыМонтажникамОбороты.Заявка.Адрес, """") КАК Поле4
    |ИЗ
    |	Справочник.Справочник3 КАК Справочник3
    |ГДЕ
    |   Справочник3.Ссылка = &Ссылка3"
);
"#;
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
