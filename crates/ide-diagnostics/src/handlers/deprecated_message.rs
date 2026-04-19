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

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::DeprecatedMessage` is encountered.
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    // Check if the diagnostic is disabled
    let code = DiagnosticCode::DeprecatedMessage;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let message = get_message(name);

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
    use crate::Severity;
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
    fn test_inside_if_block() {
        // MessaGe() inside an If block triggers, Модуль.Сообщить() does not
        let code = r#"
Процедура А()
    Если Истина Тогда
        MessaGe("А");
        Модуль.Сообщить();
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DeprecatedMessage).collect();
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("MessageToUser"));
    }

    #[test]
    fn test_module_level_call() {
        // Сообщить() at module level triggers, Модуль.Сообщить() does not
        let code = r#"
Сообщить("А");
Модуль.Сообщить();
ДругойМетод();
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DeprecatedMessage).collect();
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("СообщитьПользователю"));
    }
}
