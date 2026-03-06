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

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::UnaryPlusInConcatenation,
        "Унарный плюс в конкатенации строк",
        range,
        ctx,
    )
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
        let code = r#"Процедура Тест()

// Проверка не сработает
Хорошо = "Строка1" + "Строка2";

// Проверка сработает
Плохо = "Строка1" + + "Строка2";

// Проверка сработает
Плохо = "Строка0" + ("Строка1" + + "Строка2");

// Проверка не сработает
ОченьХорошо = Хорошо + Плохо;

// Проверка не сработает
Допустимо = Хорошо + + 5;

// Проверка не сработает
ТожеДопустимо = "Хорошо" + + 5;

// Проверка не сработает
ВообщеМинус = 5 + - 5;

// Проверка сработает
ОченьПлохо = Плохо + + Допустимо;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
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

        assert_diagnostic_range(code, diags[0], 6, 20, 21);
        assert_diagnostic_range(code, diags[1], 9, 33, 34);
        assert_diagnostic_range(code, diags[2], 24, 21, 22);
    }
}
