//! Diagnostic: SelfAssign
//!
//! Detects self-assignment patterns like `a = a` or `Obj.X = Obj.X`.
//!
//! ## Severity
//! Major
//!
//! ## Example
//! ```bsl
//! // Bad - self-assignment
//! А = А;
//! СтруктураДанных.Поле = СтруктураДанных.Поле;
//!
//! // Good
//! А = Б;
//! СтруктураДанных.Поле = НовоеЗначение;
//! ```
//!
//! ## Source
//! bsl-language-server/bsl-language-server

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
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
/// Called from lib.rs dispatch when `BodyDiagnostic::SelfAssign` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::SelfAssign,
        "Присваивание переменной самой себе",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;
    #[test]
    fn test_self_assign() {
        let code = r#"Процедура Тест()
    А = А;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let self_assign_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SelfAssign).collect();
        assert_eq!(self_assign_diags.len(), 1, "Expected 1 SelfAssign diagnostic");

        // Check position: "А = А" on line 1 (semicolon excluded from range)
        // Columns 4-9 (0-indexed: 4 spaces indent, then "А = А")
        assert_diagnostic_range(code, self_assign_diags[0], 1, 4, 9);
    }

    #[test]
    fn test_self_assign_case_insensitive() {
        // BSL is case-insensitive: А = а should be detected
        let code = r#"Процедура Тест()
    А = а;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let self_assign_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SelfAssign).collect();
        assert_eq!(self_assign_diags.len(), 1, "Case-insensitive self-assign should be detected");
    }

    #[test]
    fn test_no_self_assign() {
        let code = r#"Процедура Тест()
    А = Б;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::SelfAssign),
            "Normal assignment should not trigger SelfAssign"
        );
    }

    /// Test based on test fixture content
    /// Expected: hasSize(2), hasRange(4, 0, 4, 5), hasRange(7, 0, 7, 33)
    ///
    /// NOTE: HIR lowering only processes method bodies, so we wrap fixture content in a procedure.
    /// Line numbers in this test are offset by +1 due to wrapping.
    #[test]
    fn test_fixture_self_assign() {
        // Content from SelfAssignDiagnostic.bsl wrapped in a procedure
        // (HIR only processes method bodies)
        let code = r#"Процедура Тест()
    Если А = 1 Тогда
    КонецЕсли;

    A = 1;
    А = а; //Раз

    Структура.Чтото = Структура.ЧтотоДругое;
    Структура.Чтото = СтруКтура.ЧТото; // Два

    НовыйУникальныйИдентификатор = Новый УникальныйИдентификатор;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let self_assign_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SelfAssign).collect();

        // Expected 2 diagnostics (our lines are +1 due to procedure wrapper):
        // Line 5 (line 4, 1-indexed): А = а; - simple path self-assign
        // Line 8 (line 7, 1-indexed): Структура.Чтото = СтруКтура.ЧТото; - property self-assign
        //
        // Line 7: Структура.Чтото = Структура.ЧтотоДругое; - NOT self-assign (different props)
        assert_eq!(
            self_assign_diags.len(),
            2,
            "Should detect exactly 2 SelfAssign diagnostics , got {}",
            self_assign_diags.len()
        );

        // Check first diagnostic: line 5, "А = а"
        assert_diagnostic_range(code, self_assign_diags[0], 5, 4, 9);

        // Check second diagnostic: line 8, "Структура.Чтото = СтруКтура.ЧТото"
        assert_diagnostic_range(code, self_assign_diags[1], 8, 4, 37);
    }
}
