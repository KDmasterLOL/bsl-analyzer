//! UsingFindElementByString diagnostic.
//!
//! Detects usage of FindByName, FindByCode, FindByNumber methods with hardcoded literal arguments.
//!
//! ## Severity
//! MAJOR (CODE_SMELL)
//!
//! ## Tags
//! STANDARD, BADPRACTICE, PERFORMANCE
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! Detection points:
//! - Qualified calls in `hir-def/body/lower/expr.rs` (lower_call_expr)
//!
//! Detected patterns:
//! - Method.НайтиПоНаименованию("literal") / Method.FindByDescription("literal")
//! - Method.НайтиПоКоду("literal") / Method.FindByCode("literal")
//! - Method.НайтиПоКоду(123) / Method.FindByCode(123)
//! - Method.НайтиПоНомеру("literal") / Method.FindByNumber("literal")
//! - Method.НайтиПоНаименованию() (empty call)
//!
//! ## Examples
//!
//! ```bsl
//! // ❌ Bad: Hardcoded string literals
//! Должность = Справочники.Должности.НайтиПоНаименованию("Бухгалтер");
//! Валюта = Справочники.Валюты.НайтиПоКоду("777");
//! Документ = Документы.Реализация.НайтиПоНомеру("0000-000001", ТекущаяДата());
//!
//! // ❌ Bad: Hardcoded numeric literals
//! Валюта = Справочники.Валюты.НайтиПоКоду(777);
//! Документ = БизнесПроцессы.БП1.НайтиПоНомеру(333);
//!
//! // ✅ OK: Variable argument
//! Наименование = "Бухгалтер";
//! Должность = Справочники.Должности.НайтиПоНаименованию(Наименование);
//! ```
//!
//! ## References
//! - Java implementation: bsl-language-server/diagnostics/UsingFindElementByStringDiagnostic.java

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::UsingFindElementByString,
        "Использование НайтиПоНаименованию, НайтиПоКоду и НайтиПоНомеру",
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
        let code = include_str!("../../test_data/UsingFindElementByStringDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingFindElementByString)
            .collect();

        assert_eq!(diags.len(), 9, "Expected 9 diagnostics, got {}", diags.len());

        // Line 7 (0-indexed): найтиПонаименованию("Ведущий бухгалтер")
        assert_diagnostic_range(code, diags[0], 7, 38, 78);
        // Line 9 (0-indexed): НайтиПоНаименованию()
        assert_diagnostic_range(code, diags[1], 9, 40, 61);
        // Line 13 (0-indexed): НайтиПоНаименованию("Бухгалтер")
        assert_diagnostic_range(code, diags[2], 13, 27, 59);
        // Line 24 (0-indexed): НайтиПоКоду("777")
        assert_diagnostic_range(code, diags[3], 24, 35, 53);
        // Line 27 (0-indexed): НайтиПоКоду(777)
        assert_diagnostic_range(code, diags[4], 27, 35, 51);
        // Line 30 (0-indexed): НайтиПоНаименованию("777")
        assert_diagnostic_range(code, diags[5], 30, 27, 53);
        // Line 39 (0-indexed): НайтиПоНомеру("0000-000001", ТекущаяДата())
        assert_diagnostic_range(code, diags[6], 39, 67, 110);
        // Line 41 (0-indexed): НайтиПоНомеру(333)
        assert_diagnostic_range(code, diags[7], 41, 35, 53);
        // Line 44 (0-indexed): НайтиПоНомеру("333")
        assert_diagnostic_range(code, diags[8], 44, 29, 49);
    }

    #[test]
    fn test_find_by_description_string() {
        let code = r#"
Процедура Тест()
    Должность = Справочники.Должности.НайтиПоНаименованию("Бухгалтер");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingFindElementByString)
            .collect();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_find_by_code_number() {
        let code = r#"
Процедура Тест()
    Валюта = Справочники.Валюты.НайтиПоКоду(777);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingFindElementByString)
            .collect();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_find_by_number_string() {
        let code = r#"
Процедура Тест()
    Документ = Документы.Реализация.НайтиПоНомеру("0000-000001");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingFindElementByString)
            .collect();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_empty_call() {
        let code = r#"
Процедура Тест()
    Должность = Справочники.Должности.НайтиПоНаименованию();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingFindElementByString)
            .collect();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_variable_argument_no_trigger() {
        let code = r#"
Процедура Тест()
    Наименование = "Бухгалтер";
    Должность = Справочники.Должности.НайтиПоНаименованию(Наименование);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingFindElementByString)
            .collect();
        assert_eq!(diags.len(), 0, "Variable argument should not trigger diagnostic");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Справочники.Должности.НАЙТИПОНАИМЕНОВАНИЮ("Бухгалтер");
    Catalogs.Positions.FINDBYDESCRIPTION("Accountant");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingFindElementByString)
            .collect();
        assert_eq!(diags.len(), 2, "Should be case-insensitive");
    }

    #[test]
    fn test_english_names() {
        let code = r#"
Procedure Test()
    Position = Catalogs.Positions.FindByDescription("Accountant");
    Currency = Catalogs.Currencies.FindByCode("USD");
    Document = Documents.Sales.FindByNumber("0001");
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingFindElementByString)
            .collect();
        assert_eq!(diags.len(), 3, "Should detect all English variants");
    }
}
