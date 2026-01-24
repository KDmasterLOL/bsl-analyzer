//! Diagnostic: UnaryPlusInConcatenation
//!
//! Detects accidental double plus in concatenation: `"str" + + expr`.
//! The second plus is treated as unary operator, causing type conversion error at runtime.
//!
//! ## Severity
//! Blocker
//!
//! ## Example
//! ```bsl
//! // Bad - accidental double plus
//! Плохо = "Строка1" + + "Строка2";
//! Плохо = "Строка" + + Переменная;
//!
//! // OK - unary plus on numeric literal
//! Допустимо = "Строка" + + 5;
//!
//! // Good - single plus
//! Хорошо = "Строка1" + "Строка2";
//! ```
//!
//! ## Source
//! bsl-language-server/UnaryPlusInConcatenationDiagnostic.java

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::UnaryPlusInConcatenation;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Унарный плюс в конкатенации строк".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;

    #[test]
    fn test_basic() {
        let code = r#"Процедура Тест()
    Плохо = "Строка1" + + "Строка2";
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UnaryPlusInConcatenation)
            .collect();
        assert_eq!(diags.len(), 1, "Expected 1 UnaryPlusInConcatenation diagnostic");

        assert_diagnostic_range(code, diags[0], 1, 24, 25);
    }

    #[test]
    fn test_numeric_literal_ok() {
        let code = r#"Процедура Тест()
    Допустимо = "Хорошо" + + 5;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnaryPlusInConcatenation),
            "Unary plus on numeric literal should not trigger diagnostic"
        );
    }

    #[test]
    fn test_fixture() {
        let fixture_content =
            include_str!("../../test_data/UnaryPlusInConcatenationDiagnostic.bsl");
        let code = format!("Процедура Тест()\n{}\nКонецПроцедуры", fixture_content);

        let diagnostics = check_hir_diagnostic(&code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UnaryPlusInConcatenation)
            .collect();

        assert_eq!(
            diags.len(),
            3,
            "Expected 3 UnaryPlusInConcatenation diagnostics, got {}",
            diags.len()
        );

        assert_diagnostic_range(&code, diags[0], 6, 20, 21);
        assert_diagnostic_range(&code, diags[1], 9, 33, 34);
        assert_diagnostic_range(&code, diags[2], 24, 21, 22);
    }
}
