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
    use expect_test::expect;
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        expect![[r#"
            IfElseDuplicatedCondition @ 7:15..7:21
              message: Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции 1)
              severity: Warning"#]].assert_eq(&format_diags(code, &dupl_diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &dupl_diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        expect![[r#"
            IfElseDuplicatedCondition @ 5:15..5:21
              message: Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции 1)
              severity: Warning"#]].assert_eq(&format_diags(code, &dupl_diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        expect![[r#"
            IfElseDuplicatedCondition @ 5:15..5:27
              message: Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции 1)
              severity: Warning"#]].assert_eq(&format_diags(code, &dupl_diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &dupl_diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        expect![[r#"
            IfElseDuplicatedCondition @ 5:15..5:28
              message: Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции 1)
              severity: Warning"#]].assert_eq(&format_diags(code, &dupl_diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        // Should find 2 diagnostics:
        // 1. Inner if: п = 2 duplicate
        // 2. Outer if: п = 1 duplicate
        expect![[r#"
            IfElseDuplicatedCondition @ 6:19..6:25
              message: Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции 1)
              severity: Warning
            IfElseDuplicatedCondition @ 9:15..9:21
              message: Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции 1)
              severity: Warning"#]].assert_eq(&format_diags(code, &dupl_diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        // п = 1 is duplicated twice (3rd and 5th branch), П = 1 normalized is also п = 1 (4th duplicate)
        expect![[r#"
            IfElseDuplicatedCondition @ 7:15..7:21
              message: Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции 2)
              severity: Warning
            IfElseDuplicatedCondition @ 11:15..11:27
              message: Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции 2)
              severity: Warning"#]].assert_eq(&format_diags(code, &dupl_diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();

        // Inner: п = 2 duplicated once; outer: п = 1 duplicated once
        expect![[r#"
            IfElseDuplicatedCondition @ 10:19..10:25
              message: Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции 2)
              severity: Warning
            IfElseDuplicatedCondition @ 15:15..15:21
              message: Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции 2)
              severity: Warning"#]].assert_eq(&format_diags(code, &dupl_diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(no_dup_code, &dupl_diags));

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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCondition)
            .collect();
        expect![[r#"
            IfElseDuplicatedCondition @ 5:15..5:28
              message: Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции 1)
              severity: Warning
            IfElseDuplicatedCondition @ 7:15..7:28
              message: Дублированное условие в конструкции 'Если...Тогда...ИначеЕсли' (уже использовано в позиции 1)
              severity: Warning"#]].assert_eq(&format_diags(dup_code, &dupl_diags2));
    }
}
