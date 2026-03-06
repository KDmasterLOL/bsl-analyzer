//! IfElseDuplicatedCondition diagnostic
//!
//! Detects identical conditions in if/elsif chains.
//!
//! ## Why?
//! When if/elsif branches have identical conditions, the second branch will never
//! be executed. This usually indicates a copy-paste error or logic mistake.
//!
//! ## Bad practice
//! ```bsl
//! Если п = 1 Тогда
//!     т = 1;
//! ИначеЕсли п = 2 Тогда
//!     т = 2;
//! ИначеЕсли п = 1 Тогда    // Will never execute!
//!     т = 3;
//! КонецЕсли;
//! ```
//!
//! ## Good practice
//! ```bsl
//! Если п = 1 Тогда
//!     т = 1;
//! ИначеЕсли п = 2 Тогда
//!     т = 2;
//! ИначеЕсли п = 3 Тогда    // Fixed condition
//!     т = 3;
//! КонецЕсли;
//! ```
//!
//! ## Implementation
//!
//! Migrated to HIR-based collection.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when IfElseDuplicatedCondition diagnostic is emitted during lowering.
pub fn from_hir(
    first_occurrence_index: usize,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::IfElseDuplicatedCondition;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции {})",
            first_occurrence_index + 1
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;
    #[test]
    fn test_simple_duplicate() {
        let code = r#"
Процедура Тест()
    Если x = 1 Тогда
        т = 1;
    ИначеЕсли x = 2 Тогда
        т = 2;
    ИначеЕсли x = 1 Тогда
        т = 3;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        assert_eq!(dupl_diags.len(), 1, "Expected 1 diagnostic for duplicate x = 1 condition");
    }

    #[test]
    fn test_no_duplicates() {
        let code = r#"
Процедура Тест()
    Если x = 1 Тогда
        т = 1;
    ИначеЕсли x = 2 Тогда
        т = 2;
    ИначеЕсли x = 3 Тогда
        т = 3;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        assert_eq!(dupl_diags.len(), 0, "Should not report different conditions");
    }

    #[test]
    fn test_case_insensitive_variables() {
        let code = r#"
Процедура Тест()
    Если п = 1 Тогда
        т = 1;
    ИначеЕсли П = 1 Тогда
        т = 2;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        assert_eq!(
            dupl_diags.len(),
            1,
            "Should detect п = 1 and П = 1 as identical (case-insensitive)"
        );
    }

    #[test]
    fn test_whitespace_normalization() {
        let code = r#"
Процедура Тест()
    Если п = 1 Тогда
        т = 1;
    ИначеЕсли П     =   1 Тогда
        т = 2;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        assert_eq!(
            dupl_diags.len(),
            1,
            "Should detect conditions as identical despite whitespace differences"
        );
    }

    #[test]
    fn test_string_case_sensitive() {
        let code = r#"
Процедура Тест()
    Если (Знак = "Ё") Тогда
        Возврат 0;
    ИначеЕсли (Знак = "ё") Тогда
        Возврат 1;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        assert_eq!(dupl_diags.len(), 0, "String literals should be case-sensitive: 'Ё' != 'ё'");
    }

    #[test]
    fn test_string_same_case() {
        let code = r#"
Процедура Тест()
    Если (Знак = "ё") Тогда
        Возврат 0;
    ИначеЕсли (Знак = "ё") Тогда
        Возврат 1;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        assert_eq!(dupl_diags.len(), 1, "Should detect identical string literal conditions");
    }

    #[test]
    fn test_nested_if_independent() {
        let code = r#"
Процедура Тест()
    Если п = 1 Тогда
        Если п = 2 Тогда
            т = 1;
        ИначеЕсли п = 2 Тогда
            т = 2;
        КонецЕсли;
    ИначеЕсли п = 1 Тогда
        т = 3;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        // Should find 2 diagnostics:
        // 1. Inner if: п = 2 duplicate
        // 2. Outer if: п = 1 duplicate
        assert_eq!(dupl_diags.len(), 2, "Should detect duplicates in both outer and inner if");
    }

    /// Triple duplicate condition group: п = 1 appears three times, two warnings
    #[test]
    fn test_triple_duplicate_condition() {
        let code = r#"
Процедура Тест()
    Если п = 0 Тогда
        т = 0;
    ИначеЕсли п = 1 Тогда
        т = 1;
    ИначеЕсли п = 1 Тогда
        т = 2;
    ИначеЕсли п = 2 Тогда
        т = 3;
    ИначеЕсли П     =   1 Тогда
        т = 4;
    Иначе
        т = -1;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        // п = 1 is duplicated twice (3rd and 5th branch), П = 1 normalized is also п = 1 (4th duplicate)
        assert_eq!(dupl_diags.len(), 2, "Should find 2 duplicates of п = 1");
    }

    /// Nested if with duplicate in both outer and inner chains
    #[test]
    fn test_nested_and_outer_duplicates() {
        let code = r#"
Процедура Тест()
    Если п = 0 Тогда
        т = 0;
    ИначеЕсли п = 1 Тогда
        Если п = 1 Тогда
            т = 1;
        ИначеЕсли п = 2 Тогда
            т = 2;
        ИначеЕсли п = 2 Тогда
            т = 3;
        Иначе
            т = 4;
        КонецЕсли;
    ИначеЕсли п = 1 Тогда
        т = 4;
    Иначе
        т = -1;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        // Inner: п = 2 duplicated once; outer: п = 1 duplicated once
        assert_eq!(dupl_diags.len(), 2, "Should find 2 duplicates (inner and outer)");
    }

    /// String case-sensitive: "Ё" != "ё" so no duplicate; "ё" = "ё" is duplicate
    #[test]
    fn test_string_case_sensitive_fixture() {
        let no_dup_code = r#"
Процедура Тест()
    Если (Знак = "Ё") Тогда
        Возврат 0;
    ИначеЕсли (ЗНак = "ё") Тогда
        Возврат 1;
    Иначе
        Возврат 2;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(no_dup_code);
        let dupl_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();
        assert_eq!(dupl_diags.len(), 0, "Different string case should not be duplicate");

        let dup_code = r#"
Процедура Тест()
    Если (Знак = "ё") Тогда
        Возврат 0;
    ИначеЕсли (Знак = "ё") Тогда
        Возврат 1;
    ИначеЕсли (ЗНак = "ё") Тогда
        Возврат 2;
    Иначе
        Возврат 3;
    КонецЕсли;
КонецПроцедуры
"#;

        let diagnostics2 = check_hir_diagnostic(dup_code);
        let dupl_diags2: Vec<_> = diagnostics2
            .iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();
        assert_eq!(dupl_diags2.len(), 2, "Same string literal should produce 2 duplicates");
    }
}
