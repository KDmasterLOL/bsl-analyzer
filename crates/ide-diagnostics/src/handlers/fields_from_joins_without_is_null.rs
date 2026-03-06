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
//! Ported from:
//!

use crate::define_metadata;
use crate::metadata::*;
use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use tracing::debug;

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

/// Runs the FieldsFromJoinsWithoutIsNull diagnostic.
///
/// Uses SDBL HIR with diagnostics collected during lowering.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    let code = DiagnosticCode::FieldsFromJoinsWithoutIsNull;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // Get SDBL HIR with collected diagnostics
    let sdbl_hirs = ctx.sdbl_hir_in_file();

    let bsl_source = ctx.file_text();

    // Get SDBL queries for position mapping
    let sdbl_queries = ctx.all_sdbl_in_file();

    // Build shared line index (optimization)
    use crate::sdbl_utils::build_line_index_shared;
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    // Helper function to recursively extract diagnostics from HIR and UNION subqueries
    fn extract_diagnostics(
        hir: &sdbl_hir::SdblHir,
        mapper: &SdblPositionMapper,
        query_text: &str,
        code: DiagnosticCode,
        ctx: &DiagnosticsContext,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // Extract diagnostics from current query
        for hir_diag in &hir.diagnostics {
            if let sdbl_hir::SdblDiagnostic::FieldsFromJoinWithoutNullCheck {
                join_type,
                range: _,
                unprotected_fields,
            } = hir_diag
            {
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

                // Create one diagnostic per unprotected field, highlighting the field itself
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

        // Recursively extract diagnostics from UNION subqueries
        for union in &hir.unions {
            extract_diagnostics(&union.query, mapper, query_text, code, ctx, diagnostics);
        }
    }

    // Process HIR diagnostics
    for ((_expr_id, sdbl_package), (_query_expr_id, query_info)) in
        sdbl_hirs.iter().zip(sdbl_queries.iter())
    {
        let mapper = SdblPositionMapper::from_query_info(query_info, &bsl_source, &line_starts);

        // Extract diagnostics recursively from all queries (including UNION subqueries)
        for query in sdbl_package.queries() {
            extract_diagnostics(
                &query.hir,
                &mapper,
                &query_info.query_text,
                code,
                ctx,
                &mut diagnostics,
            );
        }
    }

    debug!(
        time_ms = start.elapsed().as_millis(),
        diagnostics_found = diagnostics.len(),
        "FieldsFromJoinsWithoutIsNull completed (HIR-based)"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;

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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Unprotected field from LEFT JOIN");
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Only bare field triggers, ISNULL-wrapped is safe");
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Unprotected field in WHERE triggers");
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Unprotected field from RIGHT JOIN");
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "INNER JOIN should never trigger");
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 3, "3 unprotected fields from FULL JOIN");
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "IS NOT NULL in WHERE exempts field");
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
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "IS NULL in WHERE does not protect SELECT field");
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

        let diagnostics = check_sdbl_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic for unprotected field");

        // Extract the highlighted text from the diagnostic range
        let diag = &diagnostics[0];
        let highlighted = &code[diag.range.start().into()..diag.range.end().into()];

        // Should highlight "ЗадачиИсполнителей.Исполнитель", not "ЛЕВОЕ СОЕДИНЕНИЕ..."
        assert!(
            highlighted.contains("ЗадачиИсполнителей"),
            "Diagnostic should highlight the field 'ЗадачиИсполнителей.Исполнитель', got: '{}'",
            highlighted
        );
        assert!(
            !highlighted.contains("СОЕДИНЕНИЕ") && !highlighted.contains("JOIN"),
            "Diagnostic should NOT highlight the JOIN clause, got: '{}'",
            highlighted
        );
    }
}
