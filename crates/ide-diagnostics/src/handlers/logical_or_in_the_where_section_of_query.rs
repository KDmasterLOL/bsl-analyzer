//! LogicalOrInTheWhereSectionOfQuery diagnostic.
//!
//! Detects OR operators in WHERE clauses of SDBL queries.
//!
//! ## Why?
//! OR operators in WHERE clauses prevent the 1C:Enterprise query optimizer from using indexes
//! effectively. When the optimizer encounters OR conditions, it typically performs full table
//! scans instead of index seeks, leading to:
//! - Dramatically slower query execution (10x-100x slower)
//! - Higher memory consumption for large result sets
//! - Increased lock contention and blocking
//! - Poor scalability with large datasets
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT Name, Price
//!          FROM Products
//!          WHERE Type = 1 OR Category = 2";
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Use UNION instead to allow index usage on each condition:
//! Query = "SELECT Name, Price
//!          FROM Products
//!          WHERE Type = 1
//!          UNION
//!          SELECT Name, Price
//!          FROM Products
//!          WHERE Category = 2";
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
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Sql, MetadataTag::Performance, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Single-pass dispatch for LogicalOrInTheWhereSectionOfQuery.
pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::LogicalOrInWhere { range } = diag {
        crate::sdbl_utils::dispatch_simple(ctx, DiagnosticCode::LogicalOrInTheWhereSectionOfQuery, "Использование оператора ИЛИ в условии ГДЕ существенно снижает производительность запроса. Рассмотрите возможность переписать с использованием ОБЪЕДИНИТЬ или изменить структуру условий", *range, mapper, query_text, diagnostics);
    }
}

/// Runs the LogicalOrInTheWhereSectionOfQuery diagnostic (standalone, used in tests).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::LogicalOrInTheWhereSectionOfQuery,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_sdbl_diagnostic};
    #[test]
    fn test_from_fixture() {
        // Fixture from test_data/LogicalOrInTheWhereSectionOfQueryDiagnostic.bsl.
        // 6 OR diagnostics in WHERE clauses across multiple procedures.
        // Тест7 has no WHERE-clause ORs (OR in CASE/JOIN only) → no diagnostics from it.
        let code = r#"Процедура Тест1()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Таблица.Наименование
    |ИЗ Справочник.Товары КАК Таблица
    |ГДЕ
    |   Истина ИЛИ Цена = 2"; //<-- ошибка

КонецПроцедуры

Процедура Тест2()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Таблица.Наименование
    |ИЗ Справочник.Товары КАК Таблица
    |ГДЕ
    |   Таблица.Поле1 = &Значение1
    |   ИЛИ Таблица.Поле2 = &Значение2"; //<-- ошибка

КонецПроцедуры

Процедура Тест4()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Таблица.Наименование
    |ИЗ Справочник.Товары КАК Таблица
    |ГДЕ
    |   Таблица.Поле1 = &Значение1
    |   И (Таблица.Поле2 = &Значение2 ИЛИ Таблица.Поле3 = &Значение3)"; //<-- ошибка

КонецПроцедуры

Процедура Тест5()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ Таблица.Наименование
    |ИЗ Справочник.Товары КАК Таблица
    |ГДЕ
    |   Таблица.Поле1 = &Значение1
    |   ИЛИ //<-- ошибка
    |   (Таблица.Поле2 = &Значение2 ИЛИ Таблица.Поле3 = &Значение3)"; //<-- ошибка

КонецПроцедуры

Процедура Тест6()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "Выбрать Истина КАК Поле1
    |ИЗ Документ.РеализацияТоваровУслуг КАК Т
    |Где Истина
    |И Т.Ссылка В
    |    (Выбрать СС.Ссылка
    |     Из Справочник.Справочник2 КАК СС
    |     Где Истина Или Ложь)"; //<-- ошибка

КонецПроцедуры

Процедура Тест7()

    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ ВЫБОР КОГДА Таблица.Флаг ИЛИ Истина Тогда Истина Иначе Ложь КОНЕЦ
    |ИЗ Справочник.Товары КАК Таблица
    |РегистрНакопления.Склады.Остатки(, ) КАК Т
    |По Истина ИЛИ Таблица.Поле1 = Т.Товар
    |УПОРЯДОЧИТЬ ПО
    |   ВЫБОР КОГДА Таблица.Флаг ИЛИ Истина Тогда Истина Иначе Ложь КОНЕЦ";

КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 6, "Expected 6 diagnostics");

        // Uses 0-indexed lines (line 7 = 1-based line 8)
        assert_diagnostic_range(code, &diagnostics[0], 7, 15, 18);
        assert_diagnostic_range(code, &diagnostics[1], 19, 8, 11);
        assert_diagnostic_range(code, &diagnostics[2], 31, 38, 41);
        assert_diagnostic_range(code, &diagnostics[3], 43, 8, 11);
        assert_diagnostic_range(code, &diagnostics[4], 44, 36, 39);
        assert_diagnostic_range(code, &diagnostics[5], 58, 21, 24);
    }

    #[test]
    fn test_simple_or_in_where() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT Name FROM Products WHERE Type = 1 OR Category = 2";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_russian_or_keyword() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ * ИЗ Товары ГДЕ Цена = 100 ИЛИ Количество = 0";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_or_in_parentheses() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T WHERE A = 1 AND (B = 2 OR C = 3)";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect OR inside parentheses");
    }

    #[test]
    fn test_multiple_or_in_where() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T WHERE A = 1 OR B = 2 OR C = 3";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 2, "Should detect both OR operators");
    }

    #[test]
    fn test_nested_subquery() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T1 WHERE ID IN (SELECT ID FROM T2 WHERE A = 1 OR B = 2)";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect OR in nested subquery WHERE");
    }

    #[test]
    fn test_no_false_positives_case_expression() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT CASE WHEN Flag OR True THEN 1 ELSE 0 END AS Result FROM T WHERE ID = 1";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should NOT detect OR in CASE expression (not in WHERE)");
    }

    #[test]
    fn test_no_false_positives_join_on() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T1 LEFT JOIN T2 ON T1.A = T2.A OR T1.B = T2.B WHERE T1.ID = 1";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Should NOT detect OR in JOIN ON clause (different diagnostic)"
        );
    }

    #[test]
    fn test_no_where_clause() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM Products";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not fail on missing WHERE");
    }

    #[test]
    fn test_and_with_or_in_parentheses() {
        let code = r#"
Процедура Тест()
    Запрос.Текст = "ВЫБРАТЬ Таблица.Наименование
    |ИЗ Справочник.Товары КАК Таблица
    |ГДЕ
    |   Таблица.Поле1 = 1
    |   И (Таблица.Поле2 = 2 ИЛИ Таблица.Поле3 = 3)";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect OR inside parentheses after AND");
    }

    #[test]
    fn test_sdbl_with_parameters() {
        let code = r#"
Процедура Тест()
    Запрос = "SELECT * FROM T WHERE Field1 = &Param1 AND (Field2 = &Param2 OR Field3 = &Param3)";
КонецПроцедуры"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect OR with parameters");
    }
}
