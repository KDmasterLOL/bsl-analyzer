//! JoinWithSubQuery diagnostic.
//!
//! Detects usage of subqueries in JOIN operations in SDBL queries.
//!
//! ## Why?
//! Joins with subqueries cause severe performance issues in 1C:Enterprise.
//! The query optimizer struggles with subqueries in JOINs, leading to:
//! - Extremely slow query execution, especially under low server load
//! - Unpredictable performance (fast sometimes, very slow other times)
//! - Significant execution time differences across different DBMS
//! - Performance degradation over time as statistics become stale
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT T.Ref FROM Catalog.Items AS Ref
//!          LEFT JOIN (SELECT S.Ref FROM Catalog.Suppliers WHERE S.Active = TRUE) AS T
//!          ON Ref.Supplier = T.Ref";
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Use temporary tables or metadata objects instead:
//! Query = "SELECT Suppliers.Ref INTO TempSuppliers
//!          FROM Catalog.Suppliers AS Suppliers WHERE Suppliers.Active = TRUE;
//!
//!          SELECT T.Ref FROM Catalog.Items AS Ref
//!          LEFT JOIN TempSuppliers AS T ON Ref.Supplier = T.Ref";
//! ```
//!
//! ## Implementation
//! Ported from:

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
        crate::sdbl_utils::dispatch_simple(ctx, DiagnosticCode::JoinWithSubQuery, "Don't use a join with sub queries. Joins with subqueries cause severe performance issues.", *range, mapper, query_text, diagnostics);
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
    fn test_join_with_sub_query_from_fixture() {
        // Inline fixture: 7 JoinWithSubQuery diagnostics across 6 procedures
        // Тест1: 1 (LEFT JOIN subquery inline)
        // Тест2: 1 (LEFT JOIN subquery multiline)
        // Тест3: 1 (RIGHT JOIN subquery multiline)
        // Тест4: 2 (subquery in FROM + subquery in JOIN)
        // Тест5: 1 (subquery in FROM with RIGHT JOIN)
        // Тест6: 1 (nested: subquery in FROM with RIGHT JOIN inside outer SELECT)
        // Тест7: 0 (subqueries in FROM without JOINs - no trigger)
        let code = r#"Процедура Тест1()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Ссылка Из Справочник.Справочник1 СПр Левое соединение (Выбрать СС.Ссылка Из Справочник.Справочник2 КАК СС Где СС.Ссылка = &Параметр) КАК Т По СПр.Поле1 = Т.Ссылка"; //<-- ошибка

КонецПроцедуры

Процедура Тест2()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Ссылка Из Справочник.Справочник1
    |СПр Левое соединение
    |(Выбрать СС.Ссылка Из Справочник.Справочник2 КАК СС Где СС.Ссылка = &Параметр) КАК Т //<-- ошибка
    |По СПр.Поле1 = Т.Ссылка";

КонецПроцедуры

Процедура Тест3()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Ссылка Из Справочник.Справочник1
    |СПр Правое соединение
    |(Выбрать СС.Ссылка Из Справочник.Справочник2 КАК СС Где СС.Ссылка = &Параметр) КАК Т //<-- ошибка
    |По СПр.Поле1 = Т.Ссылка";

КонецПроцедуры

Процедура Тест4()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Ссылка
    | Из (Выбрать СС.Ссылка Из Справочник.Справочник1 КАК СС Где СС.Ссылка = &Параметр) как СПр Левое соединение //<-- ошибка

    |(Выбрать СС.Ссылка Из Справочник.Справочник2 КАК СС Где СС.Ссылка = &Параметр) КАК Т //<-- ошибка
    // комментарий
    |По СПр.Поле1 = Т.Ссылка";

КонецПроцедуры

Процедура Тест5()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Ссылка




    |Из(Выбрать СС.Ссылка Из Справочник.Справочник2 КАК СС Где СС.Ссылка = &Параметр) КАК Т //<-- ошибка
    // комментарий
    // комментарий
    |Правое соединение Справочник.Справочник1 СПр
    // комментарий


    // комментарий


    // комментарий
    |По СПр.Поле1 = Т.Ссылка";

КонецПроцедуры

Процедура Тест6()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать * Из (Выбрать Т.Ссылка
    |Из(Выбрать СС.Ссылка Из Справочник.Справочник2 КАК СС //<-- ошибка
    |Где СС.Ссылка = &Параметр) КАК Т
    |Правое соединение Справочник.Справочник1 СПр
    |По СПр.Поле1 = Т.Ссылка) КАК ПодЗапрос";

КонецПроцедуры

Процедура Тест7()

    Запрос = Новый Запрос;
    Запрос.Текст = "Выбрать Т.Ссылка
    | Из (Выбрать СС.Ссылка Из Справочник.Справочник1 КАК СС Где СС.Ссылка = &Параметр) как СПр,
    |(Выбрать СС.Ссылка Из Справочник.Справочник2 КАК СС Где СС.Ссылка = &Параметр) КАК Т";

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
        // This matches Тест4 from the fixture
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
