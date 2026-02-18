//! Diagnostic: UsingCancelParameter
//!
//! Detects invalid assignments to Cancel/Отказ parameter in event handlers.
//!
//! The Cancel parameter should only be set to True or combined with OR operation
//! to preserve any previously set cancel flag from other handlers.
//!
//! ## Severity
//! Major
//!
//! ## Example
//! ```bsl
//! // Good - setting to True
//! Отказ = Истина;
//!
//! // Good - preserving existing value with OR
//! Отказ = Отказ ИЛИ НашаПроверка();
//! Отказ = НашаПроверка() ИЛИ Отказ;
//!
//! // Bad - setting to False (may reset cancel from other handlers)
//! Отказ = Ложь;
//!
//! // Bad - overwriting without OR (loses previous value)
//! Отказ = НашаПроверка();
//!
//! // Bad - using AND instead of OR
//! Отказ = Отказ И НашаПроверка();
//! ```
//!
//! ## Source
//! bsl-language-server/UsingCancelParameterDiagnostic.java

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::UsingCancelParameter,
        "Неправильное использование параметра \"Отказ\"",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;
    #[test]
    fn test_cancel_assign_false() {
        let code = r#"Процедура Обработчик(Отказ)
    Отказ = Ложь;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingCancelParameter).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 1, 4, 17);
    }

    #[test]
    fn test_cancel_assign_true_ok() {
        let code = r#"Процедура Обработчик(Отказ)
    Отказ = Истина;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingCancelParameter).collect();
        assert!(diags.is_empty(), "Assigning True should be allowed");
    }

    #[test]
    fn test_cancel_or_expr_ok() {
        let code = r#"Процедура Обработчик(Отказ)
    Отказ = Отказ ИЛИ НашаПроверка();
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingCancelParameter).collect();
        assert!(diags.is_empty(), "OR with Cancel should be allowed");
    }

    #[test]
    fn test_cancel_expr_or_cancel_ok() {
        let code = r#"Процедура Обработчик(Отказ)
    Отказ = НашаПроверка() ИЛИ Отказ;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingCancelParameter).collect();
        assert!(diags.is_empty(), "expr OR Cancel should be allowed");
    }

    #[test]
    fn test_cancel_and_not_allowed() {
        let code = r#"Процедура Обработчик(Отказ)
    Отказ = Отказ И НашаПроверка();
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingCancelParameter).collect();
        assert_eq!(diags.len(), 1, "AND with Cancel should NOT be allowed");
    }

    #[test]
    fn test_cancel_method_call() {
        let code = r#"Процедура Обработчик(Отказ)
    Отказ = Метод(Отказ);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingCancelParameter).collect();
        assert_eq!(diags.len(), 1, "Method call without OR should not be allowed");
    }

    #[test]
    fn test_no_cancel_param_no_diagnostic() {
        let code = r#"Процедура Обработчик(Параметр)
    Отказ = Ложь;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingCancelParameter).collect();
        assert!(diags.is_empty(), "No Cancel param - no diagnostic");
    }

    #[test]
    fn test_fixture_using_cancel_parameter() {
        let code = include_str!("../test_data/UsingCancelParameterDiagnostic.bsl");

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UsingCancelParameter).collect();

        assert_eq!(diags.len(), 6, "Expected 6 diagnostics (matching Java)");

        assert_diagnostic_range(code, diags[0], 7, 8, 21);
        assert_diagnostic_range(code, diags[1], 14, 4, 27);
        assert_diagnostic_range(code, diags[2], 42, 4, 65);
        assert_diagnostic_range(code, diags[3], 43, 4, 65);
        assert_diagnostic_range(code, diags[4], 44, 4, 65);
        assert_diagnostic_range(code, diags[5], 45, 4, 69);
    }
}
