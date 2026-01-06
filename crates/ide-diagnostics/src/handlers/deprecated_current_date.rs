//! DeprecatedCurrentDate diagnostic.
//!
//! Detects usage of deprecated ТекущаяДата() / CurrentDate() methods.
//!
//! ## Why?
//! The `ТекущаяДата()` / `CurrentDate()` method returns server date/time but with unpredictable timezone behavior.
//! - On server: returns server's local time
//! - On client: may return incorrect time due to timezone discrepancies
//! - Causes bugs in multi-timezone deployments
//!
//! ## Bad practice
//! ```bsl
//! Процедура ПолучитьДату()
//!     Возврат ТекущаяДата(); // ❌ Unpredictable timezone!
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! // On server:
//! Процедура ПолучитьДату()
//!     Возврат ТекущаяДатаСеанса(); // ✅ Session date
//! КонецПроцедуры
//!
//! // On client:
//! Процедура ПолучитьДату()
//!     Возврат ОбщегоНазначенияКлиент.ДатаСеанса(); // ✅ From StandardLibrary
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Error (MAJOR)
//! - **Tags:** STANDARD, DEPRECATED, UNPREDICTABLE
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! Ported from:
//! - DeprecatedCurrentDateDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - deprecated_current_date.rs (bsl-language-server-rust) - Rust reference

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::DeprecatedCurrentDate` is encountered.
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    // Check if the diagnostic is disabled
    if ctx.config.is_disabled(DiagnosticCode::DeprecatedCurrentDate) {
        return None;
    }

    let message = get_message(name);

    Some(Diagnostic {
        code: DiagnosticCode::DeprecatedCurrentDate,
        message,
        severity: Severity::Error,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

fn get_message(method_name: &str) -> String {
    let lower = method_name.to_lowercase();
    if lower == "текущаядата" {
        "Используйте \"ТекущаяДатаСеанса\" вместо устаревшего \"ТекущаяДата\"".to_string()
    } else {
        "Use \"CurrentSessionDate\" instead of deprecated \"CurrentDate\"".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_deprecated_russian() {
        let code = r#"
Процедура Тест()
    Дата = ТекущаяДата();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedCurrentDate)
            .collect();

        assert_eq!(deprecated_diags.len(), 1);
        assert_eq!(deprecated_diags[0].severity, Severity::Error);
        assert!(deprecated_diags[0].message.contains("ТекущаяДатаСеанса"));
    }

    #[test]
    fn test_deprecated_english() {
        let code = r#"
Procedure Test()
    Date = CurrentDate();
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedCurrentDate)
            .collect();

        assert_eq!(deprecated_diags.len(), 1);
        assert!(deprecated_diags[0].message.contains("CurrentSessionDate"));
    }

    #[test]
    fn test_object_method_excluded() {
        let code = r#"
Процедура Тест()
    Дата = Модуль.ТекущаяДата();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedCurrentDate)
            .collect();

        // Should not trigger for method calls
        assert_eq!(deprecated_diags.len(), 0);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Дата1 = ТЕКУЩАЯДАТА();
    Дата2 = текущаядата();
    Дата3 = ТекущаяДата();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedCurrentDate)
            .collect();

        assert_eq!(deprecated_diags.len(), 3);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/DeprecatedCurrentDateDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(input);

        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedCurrentDate)
            .collect();

        assert_eq!(deprecated_diags.len(), 2, "Expected 2 diagnostics");

        // Verify diagnostic positions match Java test expectations
        assert_diagnostic_range(input, deprecated_diags[0], 2, 19, 30);
        assert_diagnostic_range(input, deprecated_diags[1], 11, 16, 27);
    }
}
