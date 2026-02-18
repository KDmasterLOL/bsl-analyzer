//! CreateQueryInCycle diagnostic.
//!
//! Detects when Query/QueryBuilder/ReportBuilder objects have their Execute() method
//! called inside loops, which is a critical performance anti-pattern.
//!
//! ## Why?
//! Calling Execute() on a Query inside a loop causes:
//! - Severe performance degradation (N database round-trips instead of 1)
//! - Increased database load
//! - Potential timeout errors on large datasets
//! - Inefficient use of database connections
//!
//! ## Bad practice
//! ```bsl
//! Для Каждого ИД Из МассивИД Цикл
//!     Запрос = Новый Запрос;
//!     Запрос.Текст = "SELECT ...";
//!     Запрос.УстановитьПараметр("ID", ИД);
//!     Результат = Запрос.Выполнить(); // Error: Execute in loop!
//! КонецЦикла;
//! ```
//!
//! ## Good practice
//! ```bsl
//! Запрос = Новый Запрос;
//! Запрос.Текст = "SELECT ...";
//!
//! Для Каждого ИД Из МассивИД Цикл
//!     Запрос.УстановитьПараметр("ID", ИД);
//!     Результат = Запрос.Выполнить(); // OK: Set parameters, execute once
//! КонецЦикла;
//! ```
//!
//! Better: Use array parameters and execute query only once.
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Error (CRITICAL)
//! - **Tags:** PERFORMANCE
//! - **Minutes to fix:** 20
//!
//! ## Implementation
//! Migrated to HIR-based approach for consistency with other diagnostics.
//! Diagnostics are collected during HIR lowering when Query.Execute() is called inside loops.
//!
//! See:
//! - `crates/hir-def/src/body/lower/mod.rs` - LoweringCtx with loop_depth and query_vars tracking
//! - `crates/hir-def/src/body/lower/stmt.rs` - Loop handling and query variable tracking
//! - `crates/hir-def/src/body/lower/expr.rs` - Execute() call detection

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 20,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::CreateQueryInCycle` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::CreateQueryInCycle,
        "Выполнение запроса в цикле приводит к деградации производительности. \
         Создайте запрос один раз до цикла и изменяйте только параметры внутри цикла",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_hir_diagnostic;
    #[test]
    fn test_query_in_for_loop() {
        let code = r#"
Процедура Тест()
Запрос = Новый Запрос();
Для Каждого ИД Из МассивИД Цикл
    Запрос.Выполнить();
КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);

        // Find CreateQueryInCycle diagnostic (may not be first due to other diagnostics)
        let query_diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CreateQueryInCycle).collect();
        assert_eq!(query_diagnostics.len(), 1, "Expected exactly 1 CreateQueryInCycle diagnostic");
    }

    #[test]
    fn test_query_outside_loop() {
        let code = r#"
Процедура Тест(МассивИД)
    Запрос = Новый Запрос;

    Для Каждого ИД Из МассивИД Цикл
        Запрос.УстановитьПараметр("Код", ИД);
        Результат = Запрос.Выполнить();
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let query_diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CreateQueryInCycle).collect();
        assert_eq!(query_diagnostics.len(), 1, "Expected exactly 1 CreateQueryInCycle diagnostic");
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    For Each Item In Collection Do
        Query = New Query;
        Query.Execute();
    EndDo;
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let query_diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CreateQueryInCycle).collect();
        assert_eq!(query_diagnostics.len(), 1, "Expected exactly 1 CreateQueryInCycle diagnostic");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Для инт = 1 По 10 Цикл
        Запрос = Новый ЗАПРОС;
        Запрос.ВЫПОЛНИТЬ();
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let query_diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CreateQueryInCycle).collect();
        assert_eq!(query_diagnostics.len(), 1, "Expected exactly 1 CreateQueryInCycle diagnostic");
    }

    #[test]
    fn test_query_builder() {
        let code = r#"
Процедура Тест()
ПЗ = Новый ПостроительЗапроса;
Для инт = 1 По 10 Цикл
    ПЗ.Выполнить();
КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let query_diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::CreateQueryInCycle).collect();
        assert_eq!(
            query_diagnostics.len(),
            1,
            "Expected exactly 1 CreateQueryInCycle diagnostic for QueryBuilder"
        );
    }
}
