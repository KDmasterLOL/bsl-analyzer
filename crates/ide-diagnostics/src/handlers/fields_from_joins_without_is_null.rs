//! FieldsFromJoinsWithoutIsNull diagnostic.
//!
//! Checks that fields from LEFT/RIGHT/FULL JOINs are protected with NULL checks.
//!
//! ## Why?
//! When using LEFT, RIGHT, or FULL JOINs in SDBL queries, fields from the joined table
//! can be NULL even if rows exist. Accessing these fields without NULL protection can cause:
//! - Unexpected query results
//! - Runtime errors in 1C:Enterprise
//! - Incorrect business logic execution
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT Employee.Ref FROM Document.Order AS Orders
//!         |LEFT JOIN Catalog.Employees AS Employee
//!         |  ON Orders.Employee = Employee.Ref";
//!         // Error: Employee.Ref can be NULL, needs ISNULL() or IS NULL check
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Option 1: Use ISNULL function
//! Query = "SELECT ISNULL(Employee.Ref, NULL) FROM Document.Order AS Orders
//!         |LEFT JOIN Catalog.Employees AS Employee
//!         |  ON Orders.Employee = Employee.Ref";
//!
//! // Option 2: Use IS NULL operator
//! Query = "SELECT Employee.Ref FROM Document.Order AS Orders
//!         |LEFT JOIN Catalog.Employees AS Employee
//!         |  ON Orders.Employee = Employee.Ref
//!         |WHERE Employee.Ref IS NOT NULL";
//!
//! // Option 3: Use INNER JOIN instead (if semantically correct)
//! Query = "SELECT Employee.Ref FROM Document.Order AS Orders
//!         |INNER JOIN Catalog.Employees AS Employee
//!         |  ON Orders.Employee = Employee.Ref";
//! ```
//!
//! ## Rules
//! - Checks LEFT JOIN, RIGHT JOIN, FULL JOIN (INNER JOIN is safe)
//! - Fields must be protected with:
//!   - `ISNULL(field, defaultValue)` function
//!   - `field IS NULL` or `field IS NOT NULL` operator
//!   - `NOT (field IS NULL)` negation pattern
//!   - Global WHERE clause with `IS NOT NULL` exempts all field usage
//! - Bilingual support: ЛЕВОЕ/LEFT, ПРАВОЕ/RIGHT, ПОЛНОЕ/FULL
//! - Checks three contexts: SELECT, WHERE, JOIN ON conditions
//!
//! ## Implementation
//!

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use sdbl_hir;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Suspicious, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Single-pass dispatch for FieldsFromJoinsWithoutIsNull.
pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::FieldsFromJoinWithoutNullCheck {
        join_type,
        unprotected_fields,
        ..
    } = diag
    {
        let code = DiagnosticCode::FieldsFromJoinsWithoutIsNull;
        let join_type_str = match join_type {
            sdbl_hir::JoinType::Left => "ЛЕВОГО СОЕДИНЕНИЯ",
            sdbl_hir::JoinType::Right => "ПРАВОГО СОЕДИНЕНИЯ",
            sdbl_hir::JoinType::Full => "ПОЛНОГО СОЕДИНЕНИЯ",
            _ => "СОЕДИНЕНИЯ",
        };
        let message = format!(
            "Для полей из {} добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ",
            join_type_str
        );
        for field_ref in unprotected_fields {
            let bsl_range = mapper.map_range(field_ref.range, query_text);
            diagnostics.push(Diagnostic {
                code,
                message: message.clone(),
                severity: ctx.severity(code),
                range: bsl_range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }
}

/// Runs the FieldsFromJoinsWithoutIsNull diagnostic (standalone, used in tests).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::FieldsFromJoinsWithoutIsNull,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_left_join_unprotected_field() {
        // Test1: single LEFT JOIN, unprotected field in SELECT
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Сотрудники.Ссылка
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
    |ПО Склады.Кладовщик = Сотрудники.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 4:14..5:6
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_left_join_with_isnull_protected() {
        // Test3: ISNULL-protected field should NOT trigger, bare field SHOULD
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Сотрудники3.Ссылка,
    |ЕСТЬNULL(Сотрудники3.Ссылка, 0)
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники3
    |ПО Склады.Кладовщик = Сотрудники3.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 4:14..4:32
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_left_join_where_clause_unprotected() {
        // Test4: unprotected field in WHERE clause triggers
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Склады.Ссылка
    |ИЗ Справочник.Склады КАК Склады
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники4
    |ПО Склады.Кладовщик = Сотрудники4.Ссылка
    |ГДЕ Сотрудники4.Флаг
    |И ЕСТЬNULL(Сотрудники4.Флаг, Истина)
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 8:10..9:6
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_right_join_unprotected_field() {
        // Test5: RIGHT JOIN, unprotected field in SELECT
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Склады5.Ссылка,
    |ЕСТЬNULL(Склады5.Ссылка, 0)
    |ИЗ Справочник.Склады КАК Склады5
    |ПРАВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники5
    |ПО Склады5.Кладовщик = Сотрудники5.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 4:14..4:28
                  message: Для полей из ПРАВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_inner_join_no_diagnostic() {
        // Test6: INNER JOIN - never triggers
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Склады6.Ссылка
    |ИЗ Справочник.Склады КАК Склады6
    |ВНУТРЕННЕЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники6
    |ПО Склады6.Кладовщик = Сотрудники6.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_full_join_multiple_unprotected_fields() {
        // Test8: FULL JOIN, 3 unprotected fields -> 3 diagnostics
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Сотрудники8.Ссылка,
    |Склады8.Ссылка,
    |Сотрудники8.Организация,
    |ЕСТЬNULL(Сотрудники8.Ссылка, 0),
    |ЕСТЬNULL(Склады8.Ссылка, 0)
    |ИЗ Справочник.Склады КАК Склады8
    |ПОЛНОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники8
    |ПО Склады8.Кладовщик = Сотрудники8.Ссылка
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 4:14..4:32
                  message: Для полей из ПОЛНОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical
                FieldsFromJoinsWithoutIsNull @ 5:6..5:20
                  message: Для полей из ПОЛНОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical
                FieldsFromJoinsWithoutIsNull @ 6:6..6:29
                  message: Для полей из ПОЛНОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_left_join_is_not_null_in_where_exempts() {
        // Test9/Test10: IS NOT NULL in WHERE exempts all field usage
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Сотрудники9.Ссылка
    |ИЗ Справочник.Склады КАК Склады9
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники9
    |ПО Склады9.Кладовщик = Сотрудники9.Ссылка
    |ГДЕ (Сотрудники9.Реквизит ЕСТЬ НЕ NULL)
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_is_null_in_where_does_not_exempt_select() {
        // Test13: IS NULL in WHERE does not exempt SELECT fields
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Сотрудники13.Ссылка
    |ИЗ Справочник.Склады КАК Склады13
    |ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники13
    |ПО Склады13.Кладовщик = Сотрудники13.Реквизит
    |ГДЕ Сотрудники13.Реквизит ЕСТЬ NULL
    |";
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 4:14..5:6
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn test_no_diagnostic_for_fields_in_join_conditions() {
        // Fields from a LEFT JOIN used in other JOINs' ON conditions should NOT trigger.
        // Using a joined table's field in another JOIN's ON condition is standard practice -
        // NULL in ON simply means "no match", which is expected LEFT JOIN behavior.
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ РАЗЛИЧНЫЕ
    |    ПартнерыКИ.Ссылка КАК Партнер,
    |    ЕСТЬNULL(КИПочта.АдресЭП, """") КАК email,
    |    ВЫБОР
    |        КОГДА КартыЛояльности.Ссылка ЕСТЬ NULL
    |            ТОГДА ЛОЖЬ
    |        ИНАЧЕ ИСТИНА
    |    КОНЕЦ КАК ЕстьДействующаяКарта
    |ПОМЕСТИТЬ ИнформацияКлиент
    |ИЗ
    |    Справочник.Партнеры.КонтактнаяИнформация КАК ПартнерыКИ
    |        ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Партнеры.КонтактнаяИнформация КАК КИПочта
    |        ПО ПартнерыКИ.Ссылка = КИПочта.Ссылка
    |        ЛЕВОЕ СОЕДИНЕНИЕ РегистрСведений.СостояниеАдресовЭлектроннойПочты КАК СостояниеАдресов
    |        ПО (КИПочта.Ссылка = СостояниеАдресов.Партнер)
    |            И (КИПочта.АдресЭП = СостояниеАдресов.АдресЭП)
    |        ЛЕВОЕ СОЕДИНЕНИЕ Справочник.КартыЛояльности КАК КартыЛояльности
    |        ПО ПартнерыКИ.Ссылка = КартыЛояльности.Партнер";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_no_diagnostic_for_inner_joined_table_with_nested_left_join() {
        // When INNER JOIN has a nested LEFT JOIN, fields from the INNER-joined table
        // should NOT trigger - they are guaranteed non-NULL by the INNER JOIN.
        // Only fields from the LEFT-joined table are potentially NULL.
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ
    |    ЧекККМ.Ссылка КАК Документ,
    |    ЕСТЬNULL(ДокЗаказ.НомерДокумента, """") КАК НомерЗаказа
    |ИЗ
    |    Документ.ЧекККМ.Товары КАК ЧекККМТовары
    |        ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.ЧекККМ КАК ЧекККМ
    |            ЛЕВОЕ СОЕДИНЕНИЕ Документ.ЗаказКлиента КАК ДокЗаказ
    |            ПО ЧекККМ.ЗаказКлиента = ДокЗаказ.Ссылка
    |        ПО ЧекККМТовары.Ссылка = ЧекККМ.Ссылка";
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_diagnostic_highlights_field_not_join() {
        // Verify diagnostic highlights the unprotected field, not the JOIN clause
        let code = r#"Процедура Тест()
    Запрос = Новый Запрос("ВЫБРАТЬ
        |    ЗадачиИсполнителей.Исполнитель,
        |    ИсполнениеРезультатыПроверки.Комментарий КАК Комментарий
        |ИЗ
        |    БизнесПроцесс.Исполнение.РезультатыПроверки КАК ИсполнениеРезультатыПроверки
        |        ЛЕВОЕ СОЕДИНЕНИЕ Задача.ЗадачаИсполнителя КАК ЗадачиИсполнителей
        |        ПО ИсполнениеРезультатыПроверки.ЗадачаИсполнителя = ЗадачиИсполнителей.Ссылка
        |ГДЕ
        |    ИсполнениеРезультатыПроверки.ЗадачаПроверяющего = &ЗадачаПроверяющего");
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 3:14..3:44
                  message: Для полей из ЛЕВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn track3_full_outer_join_classification_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
        "SELECT Employees.Ref AS EmployeeRef,
        |       Warehouses.Ref AS WarehouseRef
        |FROM Catalog.Warehouses AS Warehouses
        |FULL OUTER JOIN Catalog.Employees AS Employees
        |ON Warehouses.Manager = Employees.Ref";
КонецПроцедуры"#,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 4:17..4:31
                  message: Для полей из ПОЛНОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical
                FieldsFromJoinsWithoutIsNull @ 5:17..5:32
                  message: Для полей из ПОЛНОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }

    #[test]
    fn track3_left_outer_join_isnull_wrapped_field_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
        "ВЫБРАТЬ
        |   ЕСТЬNULL(Сотрудники.Ссылка, ЗНАЧЕНИЕ(Справочник.Сотрудники.ПустаяСсылка)) КАК Сотрудник
        |ИЗ
        |   Справочник.Склады КАК Склады
        |   ЛЕВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
        |   ПО Склады.Кладовщик = Сотрудники.Ссылка";
КонецПроцедуры"#,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#""#]],
        );
    }

    #[test]
    fn track3_right_outer_join_classification_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
        "ВЫБРАТЬ
        |   Склады.Ссылка КАК Склад
        |ИЗ
        |   Справочник.Склады КАК Склады
        |   ПРАВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ Справочник.Сотрудники КАК Сотрудники
        |   ПО Склады.Кладовщик = Сотрудники.Ссылка";
КонецПроцедуры"#,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            expect![[r#"
                FieldsFromJoinsWithoutIsNull @ 5:13..5:27
                  message: Для полей из ПРАВОГО СОЕДИНЕНИЯ добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ
                  severity: Critical"#]],
        );
    }
}
