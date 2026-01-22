//! DisableSafeMode diagnostic.
//!
//! Detects calls that disable safe mode in 1C:Enterprise.
//!
//! ## Why?
//! Disabling safe mode creates serious security vulnerabilities:
//! - Allows execution of potentially dangerous operations
//! - Bypasses 1C:Enterprise security restrictions
//! - May violate security policies
//! - Creates attack vectors for malicious code
//!
//! Safe mode prevents:
//! - File system access
//! - External component execution
//! - COM object creation
//! - Operating system calls
//!
//! ## Bad practice
//! ```bsl
//! Процедура ОпаснаяПроцедура()
//!     // Disabling safe mode - DANGEROUS!
//!     УстановитьБезопасныйРежим(Ложь);
//!     УстановитьОтключениеБезопасногоРежима(Истина);
//!
//!     // Cannot verify safety at compile time
//!     Режим = Ложь;
//!     УстановитьБезопасныйРежим(Режим);
//! КонецПроцедуры
//!
//! Procedure DangerousProcedure()
//!     SetSafeMode(False);
//!     SetSafeModeDisabled(True);
//! EndProcedure
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура БезопаснаяПроцедура()
//!     // Enabling safe mode - GOOD!
//!     УстановитьБезопасныйРежим(Истина);
//! КонецПроцедуры
//!
//! Procedure SafeProcedure()
//!     SetSafeMode(True);
//! EndProcedure
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Warning (MAJOR)
//! - **Tags:** SUSPICIOUS, BADPRACTICE
//! - **Minutes to fix:** 15
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! Ported from:
//! - DisableSafeModeDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - disable_safe_mode.rs (bsl-language-server-rust) - Rust reference

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::DisableSafeMode` is encountered.
pub fn from_hir(
    method_name: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    // Check if the diagnostic is disabled
    let code = DiagnosticCode::DisableSafeMode;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let message = get_message(method_name);

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

fn get_message(method_name: &str) -> String {
    let lower = method_name.to_lowercase();
    match lower.as_str() {
        "установитьбезопасныйрежим" | "setsafemode" => {
            "Отключение безопасного режима создает уязвимость безопасности. \
             Используйте УстановитьБезопасныйРежим(Истина) / SetSafeMode(True)"
                .to_string()
        }
        "установитьотключениебезопасногорежима" | "setsafemodedisabled" => {
            "Отключение безопасного режима через УстановитьОтключениеБезопасногоРежима \
             создает уязвимость безопасности"
                .to_string()
        }
        _ => "Отключение безопасного режима создает уязвимость безопасности".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::Severity;

    #[test]
    fn test_set_safe_mode_false() {
        let code = r#"
Процедура Тест()
    УстановитьБезопасныйРежим(Ложь);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 1);
        assert_eq!(safe_mode_diags[0].severity, Severity::Major); // VULNERABILITY + MAJOR maps to Major
    }

    #[test]
    fn test_set_safe_mode_true() {
        let code = r#"
Процедура Тест()
    УстановитьБезопасныйРежим(Истина);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 0);
    }

    #[test]
    fn test_set_safe_mode_variable() {
        let code = r#"
Процедура Тест()
    Значение = Ложь;
    УстановитьБезопасныйРежим(Значение);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 1);
    }

    #[test]
    fn test_set_disabled_true() {
        let code = r#"
Процедура Тест()
    УстановитьОтключениеБезопасногоРежима(Истина);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 1);
    }

    #[test]
    fn test_set_disabled_false() {
        let code = r#"
Процедура Тест()
    УстановитьОтключениеБезопасногоРежима(Ложь);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 0);
    }

    #[test]
    fn test_object_method_excluded() {
        let code = r#"
Процедура Тест()
    Модуль.УстановитьБезопасныйРежим(Ложь);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 0);
    }

    #[test]
    fn test_bilingual() {
        let code = r#"
Процедура Тест()
    SetSafeMode(False);
    SetSafeModeDisabled(True);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 2);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    УСТАНОВИТЬБЕЗОПАСНЫЙРЕЖИМ(ЛОЖЬ);
    установитьбезопасныйрежим(ложь);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 2);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/DisableSafeModeDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(input);

        let safe_mode_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DisableSafeMode).collect();

        assert_eq!(safe_mode_diags.len(), 4, "Expected 4 diagnostics to match Java");

        // Verify diagnostic positions match Java test expectations
        assert_diagnostic_range(input, safe_mode_diags[0], 2, 4, 29);
        assert_diagnostic_range(input, safe_mode_diags[1], 5, 4, 29);
        assert_diagnostic_range(input, safe_mode_diags[2], 9, 4, 41);
        assert_diagnostic_range(input, safe_mode_diags[3], 12, 4, 41);
    }
}
