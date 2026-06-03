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
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_union_all_from_fixture() {
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnionAll,
            expect![[r#"
            UnionAll @ 22:6..22:16
              message: Использование ключевого слова ОБЪЕДИНИТЬ без ВСЕ приводит к излишней обработке для удаления дубликатов. Используйте ОБЪЕДИНИТЬ ВСЕ
              severity: Information
            UnionAll @ 57:6..57:16
              message: Использование ключевого слова ОБЪЕДИНИТЬ без ВСЕ приводит к излишней обработке для удаления дубликатов. Используйте ОБЪЕДИНИТЬ ВСЕ
              severity: Information"#]],
        );
    }

    #[test]
    fn test_simple_english() {
        let code = r#"
Procedure Test()
    Query = "SELECT * FROM T1 UNION SELECT * FROM T2";
EndProcedure
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnionAll,
            expect![[r#"
            UnionAll @ 3:31..3:36
              message: Использование ключевого слова ОБЪЕДИНИТЬ без ВСЕ приводит к излишней обработке для удаления дубликатов. Используйте ОБЪЕДИНИТЬ ВСЕ
              severity: Information"#]],
        );
    }

    #[test]
    fn test_simple_russian() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ОБЪЕДИНИТЬ ВЫБРАТЬ * ИЗ Т2";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnionAll,
            expect![[r#"
            UnionAll @ 3:31..3:41
              message: Использование ключевого слова ОБЪЕДИНИТЬ без ВСЕ приводит к излишней обработке для удаления дубликатов. Используйте ОБЪЕДИНИТЬ ВСЕ
              severity: Information"#]],
        );
    }

    #[test]
    fn test_no_false_positives_union_all() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ОБЪЕДИНИТЬ ВСЕ ВЫБРАТЬ * ИЗ Т2";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::UnionAll, expect![[r#""#]]);
    }

    #[test]
    fn test_multiple_unions() {
        let code = r#"
Процедура Тест()
    Query = "SELECT * FROM T1 UNION SELECT * FROM T2 UNION SELECT * FROM T3";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnionAll,
            expect![[r#"
            UnionAll @ 3:31..3:36
              message: Использование ключевого слова ОБЪЕДИНИТЬ без ВСЕ приводит к излишней обработке для удаления дубликатов. Используйте ОБЪЕДИНИТЬ ВСЕ
              severity: Information
            UnionAll @ 3:54..3:59
              message: Использование ключевого слова ОБЪЕДИНИТЬ без ВСЕ приводит к излишней обработке для удаления дубликатов. Используйте ОБЪЕДИНИТЬ ВСЕ
              severity: Information"#]],
        );
    }

    #[test]
    fn test_mixed_union_and_union_all() {
        let code = r#"
Процедура Тест()
    Query = "SELECT * FROM T1 UNION ALL SELECT * FROM T2 UNION SELECT * FROM T3";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnionAll,
            expect![[r#"
            UnionAll @ 3:58..3:63
              message: Использование ключевого слова ОБЪЕДИНИТЬ без ВСЕ приводит к излишней обработке для удаления дубликатов. Используйте ОБЪЕДИНИТЬ ВСЕ
              severity: Information"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnionAll,
            expect![[r#"
            UnionAll @ 6:15..6:25
              message: Использование ключевого слова ОБЪЕДИНИТЬ без ВСЕ приводит к излишней обработке для удаления дубликатов. Используйте ОБЪЕДИНИТЬ ВСЕ
              severity: Information"#]],
        );
    }
}
