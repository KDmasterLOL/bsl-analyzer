//! ExtraCommas diagnostic
//!
//! Detects trailing commas in function/method call argument lists.
//!
//! **Source (Java):** bsl-language-server/ExtraCommasDiagnostic.java
//! **Source (Rust tree-sitter):** bsl-language-server-rust/rules/extra_commas.rs
//! **Test file:** ExtraCommasDiagnostic.bsl
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - detects trailing commas during HIR lowering.
//!
//! ## Why?
//! Trailing commas in BSL function calls are syntax errors or cause unexpected behavior.
//! They reduce code readability and can lead to confusion with optional parameters.
//!
//! ## Bad practice
//! ```bsl
//! Результат = Метод(Парам1, Парам2,);     // Trailing comma
//! Результат = Метод(Парам1, Парам2,,,);   // Multiple trailing commas
//! ```
//!
//! ## Good practice
//! ```bsl
//! Результат = Метод(Парам1, Парам2);
//! Результат = Метод(Парам1, , Парам2);    // Empty arg is OK
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when ExtraCommas diagnostic is emitted during lowering.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::ExtraCommas;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Trailing comma".to_string(),
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
    fn test_extra_commas() {
        let code = include_str!("../../test_data/ExtraCommasDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();

        assert_eq!(extra_diags.len(), 6, "Expected 6 diagnostics");

        // Line 9 (0-indexed line 8): Метод1(Парам1, , Парам2,)
        assert_diagnostic_range(code, extra_diags[0], 8, 35, 36);

        // Line 10: Метод2(Парам1, Парам2,,,)
        assert_diagnostic_range(code, extra_diags[1], 9, 35, 36);

        // Line 11: Модуль.Метод3(Парам1, Парам2, Парам3,, )
        assert_diagnostic_range(code, extra_diags[2], 10, 49, 50);

        // Line 12: Модуль.Метод4(Парам1, , Парам2,,,,)
        assert_diagnostic_range(code, extra_diags[3], 11, 45, 46);

        // Line 14: Если Метод5(Парам1, , Парам2,,,,) Тогда
        assert_diagnostic_range(code, extra_diags[4], 13, 31, 32);

        // Line 18: Если Модуль.Метод6(Парам1, , Парам2,,,,) Тогда
        assert_diagnostic_range(code, extra_diags[5], 17, 38, 39);
    }

    #[test]
    fn test_no_trailing_commas() {
        let code = r#"
Результат = Метод(Парам1, Парам2);
Результат = Метод(Парам1, , Парам2);
"#;
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        assert_eq!(extra_diags.len(), 0);
    }

    #[test]
    fn test_single_trailing_comma() {
        let code = r#"
Результат = Метод(А, Б,);
"#;
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        assert_eq!(extra_diags.len(), 1);
    }

    #[test]
    fn test_multiple_trailing_commas() {
        let code = r#"
Результат = Метод(А, Б,,,);
"#;
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        // Only first trailing comma is reported
        assert_eq!(extra_diags.len(), 1);
    }

    #[test]
    fn test_empty_call() {
        let code = r#"
Результат = Метод();
"#;
        let diagnostics = check_hir_diagnostic(code);
        let extra_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExtraCommas).collect();
        assert_eq!(extra_diags.len(), 0);
    }
}
