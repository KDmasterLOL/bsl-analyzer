//! RefOveruse diagnostic.
//!
//! Detects redundant .Ссылка (Reference) field access in SDBL queries.
//!
//! ## Why?
//! Accessing `.Ссылка` on a reference field causes an implicit LEFT JOIN
//! with the source table, creating unnecessary database load.
//!
//! ## Bad practice
//! ```bsl
//! Query.Text = "SELECT
//! |   T.Файл.Ссылка AS FileRef  // Implicit JOIN
//! |FROM InformationRegister.ServiceFiles AS T";
//! ```
//!
//! ## Good practice
//! Remove redundant .Ссылка - the field is already a reference.
//!
//! ## Implementation
//!
//! Uses SDBL HIR with diagnostics collected during lowering.

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

/// Runs the RefOveruse diagnostic.
///
/// Uses SDBL HIR with diagnostics collected during lowering.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_simple(
        ctx,
        DiagnosticCode::RefOveruse,
        "Избавьтесь от получения поля \"Ссылка\" в запросе.",
        |diag| match diag {
            sdbl_hir::SdblDiagnostic::RefOveruse { range } => Some(*range),
            _ => None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;
    use crate::DiagnosticCode;
    #[test]
    fn test_ref_overuse_field_ref_in_middle() {
        // T.Ссылка.Field - accessing field through .Ссылка
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

        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for T.Ссылка.Field pattern");
        assert_eq!(diagnostics[0].code, DiagnosticCode::RefOveruse);
    }

    #[test]
    fn test_ref_overuse_field_ref_at_end() {
        // T.Field.Ссылка - accessing .Ссылка on a field
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

        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for T.Field.Ссылка pattern");
        assert_eq!(diagnostics[0].code, DiagnosticCode::RefOveruse);
    }

    #[test]
    fn test_ref_overuse_double_ref() {
        // T.Ссылка.Ссылка - double reference
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

        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for double Ссылка");
        assert_eq!(diagnostics[0].code, DiagnosticCode::RefOveruse);
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
        let code = r#"
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ Документ.Документ1.Файл.Ссылка КАК п1";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for MDO type prefix pattern");
    }

    #[test]
    fn test_ref_overuse_in_where_clause() {
        // Error in WHERE clause
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

        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for WHERE clause");
    }

    #[test]
    fn test_ref_overuse_nested_in_case() {
        // Error inside CASE expression (from tabular section)
        //
        // Source: Справочник.Пользователи.ДополнительныеРеквизиты (tabular section)
        // Alias: Пользователи
        //
        // Patterns analyzed:
        // - Пользователи.Ссылка.ПометкаУдаления: ТЧ.Ссылка.Поле (owner field) = OK
        // - Пользователи.Ссылка.ТекущееПодразделение: ТЧ.Ссылка.Поле (owner field) = OK
        // - Пользователи.Ссылка.ТекущееПодразделение.Ссылка: .Ссылка at end on ТекущееПодразделение = ERROR
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

        // Only 1 error: Пользователи.Ссылка.ТекущееПодразделение.Ссылка
        // The other patterns (ТЧ.Ссылка.Поле) are accessing owner's fields - OK
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for .Ссылка at end of path");
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
