//! JoinWithSubQuery diagnostic.
//!
//! Detects SDBL joins that use a subquery as one of the joined sources.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Standard, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Single-pass dispatch for JoinWithSubQuery.
pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::JoinWithSubQuery { range } = diag {
        crate::sdbl_utils::dispatch_simple(ctx, DiagnosticCode::JoinWithSubQuery, "Не используйте соединение с подзапросами. Соединения с подзапросами вызывают серьезные проблемы с производительностью", *range, mapper, query_text, diagnostics);
    }
}

/// Runs the JoinWithSubQuery diagnostic (standalone, used in tests).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(ctx, DiagnosticCode::JoinWithSubQuery, dispatch)
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;
    use crate::{DiagnosticCode, Severity};
    #[test]
    fn test_join_with_sub_query_multi_case() {
        let code = r#"Процедура Тест1()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Заказы.Ссылка
    |Из Документ.ЗаказПокупателя КАК Заказы
    |Левое соединение (Выбрать Источник.Регистратор Из РегистрСведений.Лимиты КАК Источник Где Источник.Склад = &Склад) КАК Лимиты
    |По Заказы.Ссылка = Лимиты.Регистратор"; //<-- ошибка

КонецПроцедуры

Процедура Тест2()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Продажи.Ссылка
    |Из Документ.РеализацияТоваровУслуг КАК Продажи
    |Левое соединение
    |(Выбрать Остатки.Номенклатура Из РегистрНакопления.ОстаткиТоваров.Остатки КАК Остатки Где Остатки.Склад = &Склад) КАК Остатки //<-- ошибка
    |По Продажи.Товар = Остатки.Номенклатура";

КонецПроцедуры

Процедура Тест3()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Маршруты.Ссылка
    |Из Справочник.МаршрутыДоставки КАК Маршруты
    |Правое соединение
    |(Выбрать Зоны.Ссылка Из Справочник.ЗоныДоставки КАК Зоны Где Зоны.Город = &Город) КАК Зоны //<-- ошибка
    |По Маршруты.Зона = Зоны.Ссылка";

КонецПроцедуры

Процедура Тест4()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Клиенты.Ссылка
    | Из (Выбрать СписокКлиентов.Ссылка Из Справочник.Клиенты КАК СписокКлиентов Где СписокКлиентов.Группа = &Группа) КАК Клиенты Левое соединение //<-- ошибка
    |
    |(Выбрать Договоры.Владелец Из Справочник.ДоговорыКонтрагентов КАК Договоры Где Договоры.ВидДоговора = &ВидДоговора) КАК Договоры //<-- ошибка
    |По Клиенты.Ссылка = Договоры.Владелец";

КонецПроцедуры

Процедура Тест5()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Контрагенты.Ссылка
    |
    |Из(Выбрать ЧерныйСписок.Ссылка Из Справочник.Контрагенты КАК ЧерныйСписок Где ЧерныйСписок.ПометкаУдаления = Истина) КАК ЧерныйСписок //<-- ошибка
    |Правое соединение Справочник.Контрагенты КАК Контрагенты
    |По Контрагенты.Ссылка = ЧерныйСписок.Ссылка";

КонецПроцедуры

Процедура Тест6()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать *
    |Из (Выбрать Секции.Ссылка
    |Из(Выбрать Отбор.Секция Из РегистрСведений.НастройкиСклада КАК Отбор //<-- ошибка
    |Где Отбор.Склад = &Склад) КАК Отбор
    |Правое соединение Справочник.СекцииСклада КАК Секции
    |По Секции.Ссылка = Отбор.Секция) КАК Итог";

КонецПроцедуры

Процедура Тест7()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Категории.Ссылка
    | Из (Выбрать СписокКатегорий.Ссылка Из Справочник.КатегорииНоменклатуры КАК СписокКатегорий Где СписокКатегорий.ЭтоГруппа = Ложь) КАК Категории,
    |(Выбрать Группы.Ссылка Из Справочник.КатегорииНоменклатуры КАК Группы Где Группы.ЭтоГруппа = Истина) КАК Группы";

КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 7, "Expected 7 JoinWithSubQuery diagnostics");

        // Verify all are correct type and severity
        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::JoinWithSubQuery);
            // CodeSmell + Major → Warning (per metadata mapping)
            assert_eq!(diag.severity, Severity::Warning);
        }
    }

    #[test]
    fn test_simple_left_join_with_subquery() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ЛЕВОЕ СОЕДИНЕНИЕ (ВЫБРАТЬ * ИЗ Т2) КАК С ПО Т1.ID = С.ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "LEFT JOIN with subquery should trigger");
    }

    #[test]
    fn test_no_false_positive_table_join() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Справочник.Товары ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Цены ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "JOIN with table should not trigger");
    }

    #[test]
    fn test_no_false_positive_subquery_without_join() {
        let code = r#"
Процедура Тест7()
    Запрос = "ВЫБРАТЬ * ИЗ (ВЫБРАТЬ * ИЗ Т1) КАК С1, (ВЫБРАТЬ * ИЗ Т2) КАК С2";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Subqueries in FROM without JOINs should not trigger");
    }

    #[test]
    fn test_right_join_with_subquery() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ПРАВОЕ СОЕДИНЕНИЕ (ВЫБРАТЬ * ИЗ Т2) КАК С ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "RIGHT JOIN with subquery should trigger");
    }

    #[test]
    fn test_inner_join_with_subquery() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Т1 ВНУТРЕННЕЕ СОЕДИНЕНИЕ (ВЫБРАТЬ * ИЗ Т2) ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "INNER JOIN with subquery should trigger");
    }

    #[test]
    fn test_subquery_in_from_with_joins() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ (ВЫБРАТЬ * ИЗ Т1) КАК С ЛЕВОЕ СОЕДИНЕНИЕ Т2 ПО ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Subquery in FROM with JOINs should trigger");
    }

    #[test]
    fn test_multiline_subquery_in_from_with_joins() {
        let code = r#"
Процедура Тест()
    Запрос = "Выбрать Т.Ссылка
    | Из (Выбрать СС.Ссылка Из Справочник.Справочник1 КАК СС Где СС.Ссылка = &Параметр) как СПр Левое соединение
    |(Выбрать СС.Ссылка Из Справочник.Справочник2 КАК СС Где СС.Ссылка = &Параметр) КАК Т
    |По СПр.Поле1 = Т.Ссылка";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            2,
            "Should detect both: subquery in FROM with JOINs + subquery in JOIN"
        );
    }
}
