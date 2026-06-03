use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Performance],
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
    if let sdbl_hir::SdblDiagnostic::RefOveruse { range } = diag {
        crate::sdbl_utils::dispatch_simple(
            ctx,
            DiagnosticCode::RefOveruse,
            "Избавьтесь от получения поля \"Ссылка\" в запросе.",
            *range,
            mapper,
            query_text,
            diagnostics,
        );
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(ctx, DiagnosticCode::RefOveruse, dispatch)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_ref_overuse_field_ref_in_middle() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   Контрагенты.Ссылка.ЮрФизЛицо КАК ЮрФизЛицо
    |ИЗ
    |   Справочник.Контрагенты КАК Контрагенты";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::RefOveruse, expect![[r#""#]]);
    }

    #[test]
    fn test_ref_overuse_field_ref_at_end() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   СлужебныеФайлы.Файл.Ссылка КАК Ссылка
    |ИЗ
    |   РегистрСведений.СлужебныеФайлы КАК СлужебныеФайлы";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::RefOveruse, expect![[r#""#]]);
    }

    #[test]
    fn test_ref_overuse_double_ref() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   Таблица.Ссылка.Ссылка КАК Ссылка
    |ИЗ
    |   &Таблица КАК Таблица";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::RefOveruse, expect![[r#""#]]);
    }

    #[test]
    fn test_no_false_positive_simple_ref() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   Контрагенты.Ссылка КАК Контрагент
    |ИЗ
    |   Справочник.Контрагенты КАК Контрагенты";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::RefOveruse, expect![[r#""#]]);
    }

    #[test]
    fn test_no_false_positive_tabular_section() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   Таблица.Ссылка КАК Ссылка
    |ИЗ
    |   Документ.Документ1.ТабличнаяЧасть1 КАК Таблица";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::RefOveruse, expect![[r#""#]]);
    }

    #[test]
    fn test_ref_overuse_mdo_type_prefix() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ Документ.Документ1.Файл.Ссылка КАК п1";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::RefOveruse, expect![[r#""#]]);
    }

    #[test]
    fn test_ref_overuse_in_where_clause() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   Контрагенты.Ссылка КАК Контрагент
    |ИЗ
    |   Справочник.Контрагенты КАК Контрагенты
    |ГДЕ
    |   Контрагенты.Ссылка.ИНН = &ИНН";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::RefOveruse, expect![[r#""#]]);
    }

    #[test]
    fn test_ref_overuse_nested_in_case() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   ВЫБОР
    |       КОГДА Пользователи.Ссылка.ПометкаУдаления
    |           ТОГДА Пользователи.Ссылка.ТекущееПодразделение.Ссылка
    |       ИНАЧЕ Пользователи.Ссылка.ТекущееПодразделение
    |   КОНЕЦ КАК Поле1
    |ИЗ
    |   Справочник.Пользователи.ДополнительныеРеквизиты КАК Пользователи";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::RefOveruse, expect![[r#""#]]);
    }

    #[test]
    fn test_tabular_section_ref_to_owner_field() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   Исполнители.Ссылка КАК Ссылка,
    |   Исполнители.Ссылка.НомерИтерации КАК НомерИтерации
    |ИЗ
    |   БизнесПроцесс.Согласование.Исполнители КАК Исполнители";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::RefOveruse, expect![[r#""#]]);
    }

    #[test]
    fn test_tabular_section_ref_to_owner_nested_field_is_error() {
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   Товары.Ссылка.Организация.ИНН КАК ИННОрганизации
    |ИЗ
    |   Документ.Заказ.Товары КАК Товары";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::RefOveruse, expect![[r#""#]]);
    }
}
