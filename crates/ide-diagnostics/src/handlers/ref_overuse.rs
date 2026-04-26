//! Reports redundant `.Ссылка` access on already-reference query fields.

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

/// Single-pass dispatch for RefOveruse.
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

/// Runs the RefOveruse diagnostic (standalone, used in tests).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(ctx, DiagnosticCode::RefOveruse, dispatch)
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;
    #[test]
    fn test_ref_overuse_field_ref_in_middle() {
        // T.Ссылка.Field - accessing field through .Ссылка
        // Without metadata, type cannot be resolved → no diagnostic emitted
        // TODO: Add tests with metadata context to verify RefOveruse detection
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   Контрагенты.Ссылка.ЮрФизЛицо КАК ЮрФизЛицо
    |ИЗ
    |   Справочник.Контрагенты КАК Контрагенты";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            0,
            "Without metadata, type cannot be resolved → no diagnostic emitted"
        );
    }

    #[test]
    fn test_ref_overuse_field_ref_at_end() {
        // T.Field.Ссылка - accessing .Ссылка on a field
        // Without metadata, type cannot be resolved → no diagnostic emitted
        // TODO: Add tests with metadata context to verify RefOveruse detection
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   СлужебныеФайлы.Файл.Ссылка КАК Ссылка
    |ИЗ
    |   РегистрСведений.СлужебныеФайлы КАК СлужебныеФайлы";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            0,
            "Without metadata, type cannot be resolved → no diagnostic emitted"
        );
    }

    #[test]
    fn test_ref_overuse_double_ref() {
        // T.Ссылка.Ссылка - double reference
        // Without metadata, type cannot be resolved → no diagnostic emitted
        // TODO: Add tests with metadata context to verify RefOveruse detection
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   Таблица.Ссылка.Ссылка КАК Ссылка
    |ИЗ
    |   &Таблица КАК Таблица";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            0,
            "Without metadata, type cannot be resolved → no diagnostic emitted"
        );
    }

    #[test]
    fn test_no_false_positive_simple_ref() {
        // T.Ссылка - simple reference field access (NOT an error)
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   Контрагенты.Ссылка КАК Контрагент
    |ИЗ
    |   Справочник.Контрагенты КАК Контрагенты";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert!(diagnostics.is_empty(), "Simple T.Ссылка should not trigger diagnostic");
    }

    #[test]
    fn test_no_false_positive_tabular_section() {
        // Tabular section's .Ссылка is a back-reference to parent (NOT an error)
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   Таблица.Ссылка КАК Ссылка
    |ИЗ
    |   Документ.Документ1.ТабличнаяЧасть1 КАК Таблица";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert!(diagnostics.is_empty(), "Tabular section's .Ссылка should not trigger diagnostic");
    }

    #[test]
    fn test_ref_overuse_mdo_type_prefix() {
        // Документ.Документ1.Файл.Ссылка - with MDO type prefix
        // Without metadata, type cannot be resolved → no diagnostic emitted
        // TODO: Add tests with metadata context to verify RefOveruse detection
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ Документ.Документ1.Файл.Ссылка КАК п1";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            0,
            "Without metadata, type cannot be resolved → no diagnostic emitted"
        );
    }

    #[test]
    fn test_ref_overuse_in_where_clause() {
        // Error in WHERE clause
        // Without metadata, type cannot be resolved → no diagnostic emitted
        // TODO: Add tests with metadata context to verify RefOveruse detection
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
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            0,
            "Without metadata, type cannot be resolved → no diagnostic emitted"
        );
    }

    #[test]
    fn test_ref_overuse_nested_in_case() {
        // Patterns inside CASE expression (from tabular section)
        //
        // Source: Справочник.Пользователи.ДополнительныеРеквизиты (tabular section)
        // Alias: Пользователи
        //
        // Without metadata, type cannot be resolved → no diagnostic emitted
        // TODO: Add tests with metadata context to verify RefOveruse detection
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
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            0,
            "Without metadata, type cannot be resolved → no diagnostic emitted"
        );
    }

    #[test]
    fn test_tabular_section_ref_to_owner_field() {
        // БизнесПроцесс.Согласование.Исполнители - табличная часть
        // Исполнители.Ссылка.НомерИтерации - обращение к реквизиту ВЛАДЕЛЬЦА через .Ссылка
        // Это НЕ ошибка, т.к. связь с владельцем встроена в структуру ТЧ и не вызывает JOIN.
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
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert!(
            diagnostics.is_empty(),
            "Tabular section .Ссылка.Field (owner's field) should NOT trigger diagnostic"
        );
    }

    #[test]
    fn test_tabular_section_ref_to_owner_nested_field_is_error() {
        // Но если после .Ссылка.Поле идёт ещё одно разыменование, это уже ошибка
        // Пример: ТЧ.Ссылка.Организация.ИНН - обращение к полю ОРГАНИЗАЦИИ через ссылку
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |   Товары.Ссылка.Организация.ИНН КАК ИННОрганизации
    |ИЗ
    |   Документ.Заказ.Товары КАК Товары";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        // Здесь .Ссылка.Организация - ок (владелец), но .Организация.ИНН - уже JOIN
        // Но наша диагностика RefOveruse ловит только .Ссылка, а не вложенные разыменования.
        // QueryNestedFieldsByDot должна поймать это.
        // RefOveruse здесь НЕ должна срабатывать, т.к. .Ссылка для ТЧ - это ок.
        assert!(
            diagnostics.is_empty(),
            "RefOveruse should not trigger for ТЧ.Ссылка.Field pattern (but QueryNestedFieldsByDot should)"
        );
    }
}
