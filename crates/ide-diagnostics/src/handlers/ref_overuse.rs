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

use crate::sdbl_utils::SdblPositionMapper;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use sdbl_hir;
use tracing::debug;

/// Runs the RefOveruse diagnostic.
///
/// Uses SDBL HIR with diagnostics collected during lowering.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::RefOveruse) {
        return Vec::new();
    }

    // Get SDBL HIR with collected diagnostics
    let sdbl_hirs = ctx.sdbl_hir_in_file();

    let bsl_source = ctx.file_text();

    // Get cached SDBL queries for position mapping
    let sdbl_queries = ctx.all_sdbl_in_file();

    // Build shared line index
    use crate::sdbl_utils::build_line_index_shared;
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    // Iterate SDBL HIRs and corresponding query infos in parallel
    // Both are sorted by position in file, so we can zip them
    for ((_expr_id, sdbl_package), (_query_expr_id, query_info)) in
        sdbl_hirs.iter().zip(sdbl_queries.iter())
    {
        let mapper = SdblPositionMapper::from_query_info(query_info, &bsl_source, &line_starts);

        // Emit diagnostics from HIR
        for hir_diag in sdbl_package.all_diagnostics() {
            if let sdbl_hir::SdblDiagnostic::RefOveruse { range } = hir_diag {
                let bsl_range = mapper.map_range(*range, &query_info.query_text);

                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::RefOveruse,
                    message: "Избавьтесь от получения поля \"Ссылка\" в запросе.".to_string(),
                    severity: Severity::Warning,
                    range: bsl_range,
                    tags: vec![],
                    fixes: vec![],
                });
            }
        }
    }

    debug!(
        time_ms = start.elapsed().as_millis(),
        diagnostics_found = diagnostics.len(),
        "RefOveruse completed"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_sdbl_diagnostic;
    use crate::{DiagnosticCode, Severity};

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
        assert_eq!(diagnostics[0].severity, Severity::Warning);
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
        // Error inside CASE expression
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

        // Should detect:
        // - Пользователи.Ссылка.ПометкаУдаления (Ссылка in middle)
        // - Пользователи.Ссылка.ТекущееПодразделение.Ссылка (Ссылка in middle AND at end)
        // - Пользователи.Ссылка.ТекущееПодразделение (Ссылка in middle)
        assert!(
            diagnostics.len() >= 2,
            "Expected at least 2 diagnostics for CASE expression, got {}",
            diagnostics.len()
        );
    }
}
