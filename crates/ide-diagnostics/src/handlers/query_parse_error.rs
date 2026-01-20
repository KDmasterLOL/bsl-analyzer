//! QueryParseError diagnostic.
//!
//! Detects SDBL queries with parse errors.
//!
//! ## Why?
//! SDBL query text must be syntactically correct and should open in the query builder.
//! Parse errors indicate incomplete or malformed queries that will fail at runtime.
//!
//! ## Bad practice
//! ```bsl
//! Query.Text = "SELECT Field
//!              |FROM Table AS";  // Incomplete alias
//! ```
//!
//! ## Good practice
//! ```bsl
//! Query.Text = "SELECT Field
//!              |FROM Table AS T";  // Complete query
//! ```
//!
//! ## Implementation
//!
//! This diagnostic operates at AST level (not HIR) because:
//! - SDBL HIR is only built for syntactically correct queries
//! - Parse errors are already available in `SdblQueryInfo.query_ast`
//! - Method `SdblQueryInfo.is_valid()` returns `false` when parse errors exist

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::SyntaxKind;
use tracing::debug;

/// Runs the QueryParseError diagnostic.
///
/// Checks SDBL queries for parse errors using `all_sdbl_in_file()`.
/// Detects errors by looking for ERROR nodes in the SDBL AST
/// (SDBL parser is error-tolerant and doesn't populate the errors list).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    use std::time::Instant;
    let start = Instant::now();

    if ctx.config.is_disabled(DiagnosticCode::QueryParseError) {
        return Vec::new();
    }

    let sdbl_queries = ctx.all_sdbl_in_file();
    let mut diagnostics = Vec::new();

    for (_query_expr_id, query_info) in sdbl_queries.iter() {
        // Check for ERROR nodes in AST (SDBL parser is error-tolerant)
        let has_error_nodes = query_info
            .query_ast
            .as_ref()
            .map(|ast| ast.syntax_node().descendants().any(|n| n.kind() == SyntaxKind::ERROR))
            .unwrap_or(true); // No AST means parse failed completely

        if has_error_nodes {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::QueryParseError,
                message: "Query text must be correct".to_string(),
                severity: Severity::Warning,
                range: query_info.bsl_literal_range,
                tags: vec![],
                fixes: vec![],
            });
        }
    }

    debug!(
        time_ms = start.elapsed().as_millis(),
        diagnostics_found = diagnostics.len(),
        "QueryParseError completed"
    );

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range_multiline, check_sdbl_diagnostic};
    use crate::{DiagnosticCode, Severity};

    #[test]
    fn test_query_parse_error_from_fixture() {
        let code = include_str!("../../test_data/QueryParseErrorDiagnostic.bsl");
        let diagnostics = check_sdbl_diagnostic(code, check);

        // Java expects 3 diagnostics:
        // - Lines 10-11: incomplete JOIN (first part of concatenated string)
        // - Lines 15-20: incomplete WHERE (Условие >)
        // - Lines 28-29: incomplete FROM in batch (we detect whole batch 23-30)
        assert_eq!(diagnostics.len(), 3, "Expected 3 parse error diagnostics");

        for diag in &diagnostics {
            assert_eq!(diag.code, DiagnosticCode::QueryParseError);
            assert_eq!(diag.severity, Severity::Warning);
        }
    }

    #[test]
    fn test_valid_query_no_diagnostic() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ Справочник.Контрагенты";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Valid query should not trigger diagnostic");
    }

    #[test]
    fn test_incomplete_where() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ Таблица ГДЕ Условие >";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Incomplete WHERE should trigger diagnostic");
        assert_eq!(diagnostics[0].code, DiagnosticCode::QueryParseError);
    }

    #[test]
    fn test_incomplete_from() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ  ";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Incomplete FROM should trigger diagnostic");
    }

    #[test]
    fn test_incomplete_select_with_from() {
        // Query must have SELECT + keyword (FROM/WHERE/etc) to be detected as SDBL
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ   ";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Incomplete FROM should trigger diagnostic");
    }

    #[test]
    fn test_multiline_incomplete_where() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле
             |ИЗ Таблица
             |ГДЕ Условие >";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Incomplete WHERE in multiline should trigger diagnostic");
        // Lines are 0-indexed: line 2 = "    Запрос = ..."
        assert_diagnostic_range_multiline(code, &diagnostics[0], 2, 13, 4, 28);
    }

    #[test]
    fn test_batch_with_partial_error() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ Поле ИЗ Таблица1;
             |ВЫБРАТЬ Поле2 ИЗ";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Batch with partial error should trigger one diagnostic");
    }

    #[test]
    fn test_select_constants_without_from() {
        // Valid SDBL: SELECT without FROM clause (returns constants)
        let code = r#"
Процедура Тест()
    ТекстЗапроса = "Выбрать 1 КАК ЧисловаяКонстанта, 2, ""Строка""";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "SELECT without FROM is valid SDBL, should not trigger diagnostic"
        );
    }

    #[test]
    fn test_complex_valid_query() {
        let code = r#"
Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |    Товары.Номенклатура КАК Номенклатура,
             |    ЕСТЬNULL(ПланПродаж.Сумма, 0) КАК СуммаПлан
             |ИЗ
             |    Товары КАК Товары
             |        ЛЕВОЕ СОЕДИНЕНИЕ ПланПродаж
             |        ПО Товары.ID = ПланПродаж.ID";
КонецПроцедуры
"#;
        let diagnostics = check_sdbl_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Valid complex query should not trigger diagnostic");
    }
}
