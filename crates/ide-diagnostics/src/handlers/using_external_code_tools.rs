//! UsingExternalCodeTools diagnostic.
//!
//! Detects usage of external code execution mechanisms in BSL.
//!
//! ## Severity
//! CRITICAL (SECURITY_HOTSPOT)
//!
//! ## Tags
//! STANDARD, DESIGN
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! Detection points:
//! - Qualified calls in `hir-def/body/lower/expr.rs` (lower_call_expr)
//!
//! Detected patterns:
//! - ВнешниеОбработки.Создать() / ExternalDataProcessors.Create()
//! - ВнешниеОбработки.Подключить() / ExternalDataProcessors.Connect()
//! - ВнешниеОтчеты.Создать() / ExternalReports.Create()
//! - ВнешниеОтчеты.Подключить() / ExternalReports.Connect()
//! - РасширенияКонфигурации.Создать() / ConfigurationExtensions.Create()
//!
//! ## Examples
//!
//! ```bsl
//! // ❌ Bad: External data processors
//! ИмяОбработки = ВнешниеОбработки.Подключить("ПутьКОбработке", ЛОЖЬ);
//! Обработка = ВнешниеОбработки.Создать(ИмяОбработки);
//!
//! // ❌ Bad: External reports
//! ИмяОтчета = ExternalReports.Connect("Path", true);
//! Отчет = ExternalReports.Create(ИмяОтчета);
//!
//! // ❌ Bad: Configuration extensions
//! Расширение = РасширенияКонфигурации.Создать("ИмяРасширения");
//!
//! // ✅ OK: Not direct access to external code tools
//! Справочники.ВнешниеОбработки.Подключить("ПутьКОбработке", ЛОЖЬ);
//! ```
//!
//! ## Limitations
//! Server/client context is not currently analyzed - diagnostic fires in both contexts.
//!
//! ## References
//! - 1C Standard: https://its.1c.ru/db/v8std#content:669:hdoc

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::UsingExternalCodeTools` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::UsingExternalCodeTools,
        "Potentially unsafe use of external code tools",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура Тест()
    ИмяОбработки = ВнешниеОбработки.Подключить("ПутьКОбработке", ЛОЖЬ); // <-- Ошибка
    Обработка = ВнешниеОбработки.Создать(ИмяОбработки); // <-- Ошибка

    ИмяОтчета = ExternalReports.Connect("Path", true); // <-- Ошибка
    Отчет = ExternalReports.Create(ИмяОтчета); // <-- Ошибка

    Расширение = РасширенияКонфигурации.Создать("ИмяРасширения"); // <-- Ошибка
    СписокРасширений = Новый СписокЗначений;
    СписокРасширений.Добавить(РасширенияКонфигурации.Создать("ИмяРасширения2")); // <-- Ошибка
КонецПроцедуры

Процедура Тест2()
    Справочники.ВнешниеОбработки.Подключить("ПутьКОбработке", ЛОЖЬ); // <-- Не ошибка
    Обработка.ExternalReports.Connect("Path", true); // <-- не ошибка
    ExternalReports.Connect("Path", true).Create("name"); // <-- Ошибка
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingExternalCodeTools)
            .collect();

        assert_eq!(diags.len(), 7, "Expected 7 diagnostics");

        // 0-indexed lines from test file:
        // Line 1 (0-idx): ВнешниеОбработки.Подключить
        assert_diagnostic_range(code, diags[0], 1, 19, 70);
        // Line 2 (0-idx): ВнешниеОбработки.Создать
        assert_diagnostic_range(code, diags[1], 2, 16, 54);
        // Line 4 (0-idx): ExternalReports.Connect
        assert_diagnostic_range(code, diags[2], 4, 16, 53);
        // Line 5 (0-idx): ExternalReports.Create
        assert_diagnostic_range(code, diags[3], 5, 12, 45);
        // Line 7 (0-idx): РасширенияКонфигурации.Создать
        assert_diagnostic_range(code, diags[4], 7, 17, 64);
        // Line 9 (0-idx): РасширенияКонфигурации.Создать (inside list)
        assert_diagnostic_range(code, diags[5], 9, 30, 78);
        // Line 15 (0-idx): ExternalReports.Connect (chained call - only inner call detected)
        assert_diagnostic_range(code, diags[6], 15, 4, 41);
    }

    #[test]
    fn test_not_triggered_on_qualified_access() {
        let code = r#"
Процедура Тест()
    Справочники.ВнешниеОбработки.Подключить("ПутьКОбработке", ЛОЖЬ);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingExternalCodeTools)
            .collect();
        assert_eq!(diags.len(), 0, "Qualified access should not trigger diagnostic");
    }

    #[test]
    fn test_not_triggered_on_variable_access() {
        let code = r#"
Процедура Тест()
    Обработка.ExternalReports.Connect("Path", true);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingExternalCodeTools)
            .collect();
        assert_eq!(diags.len(), 0, "Variable access should not trigger diagnostic");
    }

    #[test]
    fn test_russian_names() {
        let code = r#"
Процедура Тест()
    ВнешниеОбработки.Создать("Имя");
    ВнешниеОтчеты.Подключить("Путь");
    РасширенияКонфигурации.Создать("Расширение");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingExternalCodeTools)
            .collect();
        assert_eq!(diags.len(), 3, "Should detect all Russian variants");
    }

    #[test]
    fn test_english_names() {
        let code = r#"
Procedure Test()
    ExternalDataProcessors.Create("Name");
    ExternalReports.Connect("Path");
    ConfigurationExtensions.Create("Extension");
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingExternalCodeTools)
            .collect();
        assert_eq!(diags.len(), 3, "Should detect all English variants");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    ВНЕШНИЕОБРАБОТКИ.СОЗДАТЬ("Имя");
    externaldataprocessors.create("Name");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingExternalCodeTools)
            .collect();
        assert_eq!(diags.len(), 2, "Should be case-insensitive");
    }

    #[test]
    fn test_local_variable_exclusion() {
        let code = r#"
Процедура Тест()
    ВнешниеОбработки = Новый Структура;
    ВнешниеОбработки.Создать("Имя");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingExternalCodeTools)
            .collect();
        assert_eq!(diags.len(), 0, "Local variable with same name should not trigger");
    }
}
