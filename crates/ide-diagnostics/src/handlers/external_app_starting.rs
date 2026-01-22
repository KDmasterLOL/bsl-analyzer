//! ExternalAppStarting diagnostic.
//!
//! Detects calls to methods that start external applications or execute system commands.
//!
//! ## Why?
//! Starting external applications creates security vulnerabilities:
//! - Arbitrary command execution
//! - Bypasses 1C:Enterprise security model
//! - May violate security policies
//! - Creates attack vectors for code injection
//!
//! Methods that trigger this diagnostic:
//! - КомандаСистемы / System
//! - ЗапуститьСистему / RunSystem
//! - ЗапуститьПриложение / RunApp
//! - НачатьЗапускПриложения / BeginRunningApplication
//! - ЗапуститьПриложениеАсинх / RunAppAsync
//! - ЗапуститьПрограмму
//! - ОткрытьПроводник
//! - ОткрытьФайл
//!
//! ## Bad practice
//! ```bsl
//! Процедура ВыполнитьКоманду()
//!     КомандаСистемы("del /f /q *.*");
//!     ЗапуститьПриложение("calc.exe");
//!     ФайловаяСистемаКлиент.ЗапуститьПрограмму("cmd.exe");
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Warning (MAJOR)
//! - **Type:** SECURITY_HOTSPOT
//! - **Tags:** SUSPICIOUS
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - detects external app calls during HIR lowering.
//!
//! Ported from:
//! - ExternalAppStartingDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - external_app_starting.rs (bsl-language-server-rust) - Rust reference

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when ExternalAppStarting diagnostic is emitted during lowering.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::ExternalAppStarting;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "External application launch detected".to_string(),
        range,
        severity: ctx.severity(code),
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/ExternalAppStartingDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let ext_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExternalAppStarting).collect();

        assert_eq!(ext_diags.len(), 16, "Expected 16 diagnostics");

        assert_diagnostic_range(code, ext_diags[0], 8, 4, 18);
        assert_diagnostic_range(code, ext_diags[1], 9, 4, 23);
        assert_diagnostic_range(code, ext_diags[2], 10, 4, 23);
        assert_diagnostic_range(code, ext_diags[3], 12, 4, 26);
        assert_diagnostic_range(code, ext_diags[4], 18, 26, 44);
        assert_diagnostic_range(code, ext_diags[5], 19, 26, 44);
        assert_diagnostic_range(code, ext_diags[6], 20, 20, 38);
        assert_diagnostic_range(code, ext_diags[7], 21, 20, 38);
        assert_diagnostic_range(code, ext_diags[8], 23, 26, 42);
        assert_diagnostic_range(code, ext_diags[9], 24, 26, 37);
        assert_diagnostic_range(code, ext_diags[10], 25, 26, 37);
        assert_diagnostic_range(code, ext_diags[11], 35, 10, 34);
        assert_diagnostic_range(code, ext_diags[12], 53, 4, 20);
        assert_diagnostic_range(code, ext_diags[13], 54, 4, 20);
        assert_diagnostic_range(code, ext_diags[14], 55, 4, 20);
        assert_diagnostic_range(code, ext_diags[15], 56, 4, 20);
    }

    #[test]
    fn test_global_call() {
        let code = r#"
Процедура Тест()
    КомандаСистемы("cmd.exe");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let ext_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExternalAppStarting).collect();
        assert_eq!(ext_diags.len(), 1, "Should detect global method call");
    }

    #[test]
    fn test_object_method_call() {
        let code = r#"
Процедура Тест()
    ФайловаяСистемаКлиент.ЗапуститьПрограмму("calc.exe");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let ext_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExternalAppStarting).collect();
        assert_eq!(ext_diags.len(), 1, "Should detect object method call");
    }

    #[test]
    fn test_similar_name_ignored() {
        let code = r#"
Процедура Тест()
    МойМодуль.ЗапуститьВнешнееПриложение("cmd");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let ext_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExternalAppStarting).collect();
        assert_eq!(ext_diags.len(), 0, "Similar method names should be ignored");
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    System("cmd.exe");
    RunApp("calc.exe");
    RunSystem();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let ext_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExternalAppStarting).collect();
        assert_eq!(ext_diags.len(), 3, "Should detect English method names");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    КОМАНДАСИСТЕМЫ("cmd");
    ЗАПУСТИТЬПриложение("app");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let ext_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExternalAppStarting).collect();
        assert_eq!(ext_diags.len(), 2, "Should be case-insensitive");
    }

    #[test]
    fn test_no_args_not_detected() {
        let code = r#"
Процедура Тест()
    Переменная = КомандаСистемы;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let ext_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::ExternalAppStarting).collect();
        assert_eq!(ext_diags.len(), 0, "Method references without calls should be ignored");
    }
}
