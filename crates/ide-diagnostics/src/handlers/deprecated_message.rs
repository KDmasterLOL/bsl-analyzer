//! DeprecatedMessage diagnostic.
//!
//! Detects usage of deprecated global `Сообщить()` / `Message()` methods.
//!
//! ## Why?
//! The global `Сообщить()` / `Message()` method is deprecated:
//! - Low level API without structured logging
//! - No severity levels or categorization
//! - Output goes to user messages which may be inappropriate
//! - Better alternatives exist for different scenarios
//!
//! ## Bad practice
//! ```bsl
//! Процедура Тест()
//!     Сообщить("Операция выполнена"); // ❌ Global Сообщить() is deprecated
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Тест()
//!     // ✅ For user notifications - use ОбщегоНазначения.СообщитьПользователю()
//!     ОбщегоНазначения.СообщитьПользователю("Операция выполнена");
//!
//!     // ✅ For logging - use ЗаписьЖурналаРегистрации()
//!     ЗаписьЖурналаРегистрации("ИмяСобытия", УровеньЖурналаРегистрации.Информация);
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Information (MINOR)
//! - **Tags:** STANDARD, DEPRECATED
//! - **Minutes to fix:** 2
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! Ported from:
//! - DeprecatedMessageDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::DeprecatedMessage` is encountered.
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    // Check if the diagnostic is disabled
    if ctx.config.is_disabled(DiagnosticCode::DeprecatedMessage) {
        return None;
    }

    let message = get_message(name);

    Some(Diagnostic {
        code: DiagnosticCode::DeprecatedMessage,
        message,
        severity: Severity::Information,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

fn get_message(method_name: &str) -> String {
    let lower = method_name.to_lowercase();
    if lower == "сообщить" {
        "Используйте \"ОбщегоНазначения.СообщитьПользователю\" вместо устаревшего \"Сообщить\""
            .to_string()
    } else {
        "Use \"CommonUse.MessageToUser\" instead of deprecated \"Message\"".to_string()
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
    Сообщить("Операция выполнена");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DeprecatedMessage).collect();

        assert_eq!(deprecated_diags.len(), 1);
        assert_eq!(deprecated_diags[0].severity, Severity::Information);
        assert!(deprecated_diags[0].message.contains("СообщитьПользователю"));
    }

    #[test]
    fn test_deprecated_english() {
        let code = r#"
Procedure Test()
    Message("Operation completed");
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DeprecatedMessage).collect();

        assert_eq!(deprecated_diags.len(), 1);
        assert!(deprecated_diags[0].message.contains("MessageToUser"));
    }

    #[test]
    fn test_object_method_excluded() {
        let code = r#"
Процедура Тест()
    Модуль.Сообщить("Операция выполнена");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DeprecatedMessage).collect();

        // Should not trigger for method calls
        assert_eq!(deprecated_diags.len(), 0);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    СООБЩИТЬ("A");
    сообщить("B");
    Сообщить("C");
    СообЩить("D");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DeprecatedMessage).collect();

        assert_eq!(deprecated_diags.len(), 4);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/DeprecatedMessageDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(input);

        let deprecated_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DeprecatedMessage).collect();

        assert_eq!(deprecated_diags.len(), 2, "Expected 2 diagnostics");

        // Verify diagnostic positions match Java test expectations
        assert_diagnostic_range(input, deprecated_diags[0], 4, 8, 15);
        assert_diagnostic_range(input, deprecated_diags[1], 10, 0, 8);
    }
}
