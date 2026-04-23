//! Detects `Число()` / `Number()` calls inside `try` blocks.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::TryNumber` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::TryNumber,
        "Не используйте try-catch для приведения к числу",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;
    #[test]
    fn test_comprehensive() {
        let code = r#"Попытка

Исключение
    А = Число(Б);
КонецПопытки


Попытка
А = ЧислО(Б);
Б = NumbeR(4);

    Попытка
    В = Number(4);

    Исключение

    КонецПопытки
Исключение

КонецПопытки

F = Number();
А = Число(Б);
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TryNumber).collect();

        assert_eq!(diagnostics.len(), 3, "Expected 3 diagnostics");

        assert_diagnostic_range(code, diagnostics[0], 8, 4, 12);
        assert_diagnostic_range(code, diagnostics[1], 9, 4, 13);
        assert_diagnostic_range(code, diagnostics[2], 12, 8, 17);
    }

    #[test]
    fn test_hir_detection() {
        let code = r#"
Процедура Тест()
    Попытка
        А = Число(Б);
    Исключение
    КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let try_number: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TryNumber).collect();

        assert_eq!(try_number.len(), 1, "HIR should detect TryNumber");
    }

    #[test]
    fn test_number_in_except_not_detected() {
        let code = r#"
Процедура Тест()
Попытка
Исключение
    А = Число(Б);
КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TryNumber).collect();
        assert_eq!(diagnostics.len(), 0, "Number in except block should not be detected");
    }

    #[test]
    fn test_number_outside_try_not_detected() {
        let code = r#"
Процедура Тест()
F = Number();
А = Число(Б);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TryNumber).collect();
        assert_eq!(diagnostics.len(), 0, "Number outside try block should not be detected");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
Попытка
    А = ЧИСЛО(Б);
    Б = Number(4);
КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TryNumber).collect();
        assert_eq!(diagnostics.len(), 2, "Should be case-insensitive");
    }

    #[test]
    fn test_nested_try() {
        let code = r#"
Процедура Тест()
Попытка
    Попытка
        В = Number(4);
    КонецПопытки;
КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TryNumber).collect();
        assert_eq!(diagnostics.len(), 1, "Should detect in nested try blocks");
    }
}
