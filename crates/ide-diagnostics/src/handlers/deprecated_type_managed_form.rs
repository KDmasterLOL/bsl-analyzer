//! DeprecatedTypeManagedForm diagnostic.
//!
//! Detects usage of deprecated `Тип("УправляемаяФорма")` / `Type("ManagedForm")` type.
//!
//! ## Why?
//! Starting from 1C:Enterprise 8.3.14, the type "УправляемаяФорма" (ManagedForm) was renamed
//! to "ФормаКлиентскогоПриложения" (ClientApplicationForm) for better clarity:
//! - More descriptive name indicating client application context
//! - Aligns with platform's terminology updates
//! - Improves code readability
//!
//! ## Bad practice
//! ```bsl
//! Процедура ПроверитьФорму()
//!     Если ТипЗнч(Форма) = Тип("УправляемаяФорма") Тогда  // ❌ Deprecated type
//!         // ...
//!     КонецЕсли;
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура ПроверитьФорму()
//!     // ✅ Use modern type name
//!     Если ТипЗнч(Форма) = Тип("ФормаКлиентскогоПриложения") Тогда
//!         // ...
//!     КонецЕсли;
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Information (MINOR)
//! - **Tags:** STANDARD, DEPRECATED
//! - **Minutes to fix:** 1
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! Ported from:
//! - DeprecatedTypeManagedFormDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - deprecated_type_managed_form.rs (bsl-language-server-rust) - Rust reference

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::DeprecatedTypeManagedForm` is encountered.
pub fn from_hir(type_name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    // Check if the diagnostic is disabled
    if ctx.config.is_disabled(DiagnosticCode::DeprecatedTypeManagedForm) {
        return None;
    }

    let message = get_message(type_name);

    Some(Diagnostic {
        code: DiagnosticCode::DeprecatedTypeManagedForm,
        message,
        severity: Severity::Information,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

fn get_message(arg_value: &str) -> String {
    let lower = arg_value.to_lowercase();
    if lower == "управляемаяформа" {
        "Использование устаревшего типа \"УправляемаяФорма\". \
         Рекомендуется использовать \"ФормаКлиентскогоПриложения\""
            .to_string()
    } else {
        "Usage of deprecated type \"ManagedForm\". \
         Recommended to use \"ClientApplicationForm\""
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn test_deprecated_type_russian() {
        let code = r#"
Процедура Тест()
    Если ТипЗнч(Форма) = Тип("УправляемаяФорма") Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedTypeManagedForm)
            .collect();

        assert_eq!(deprecated_diags.len(), 1);
        assert_eq!(deprecated_diags[0].severity, Severity::Information);
        assert!(deprecated_diags[0].message.contains("УправляемаяФорма"));
    }

    #[test]
    fn test_deprecated_type_english() {
        let code = r#"
Procedure Test()
    If TypeOf(Form) = Type("ManagedForm") Then
        Return;
    EndIf;
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedTypeManagedForm)
            .collect();

        assert_eq!(deprecated_diags.len(), 1);
        assert!(deprecated_diags[0].message.contains("ManagedForm"));
    }

    #[test]
    fn test_string_literal_not_detected() {
        let code = r#"
Процедура Тест()
    Сообщить("УправляемаяФорма");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedTypeManagedForm)
            .collect();

        assert_eq!(deprecated_diags.len(), 0);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Т1 = ТИП("УПРАВЛЯЕМАЯФОРМА");
    Т2 = тип("управляемаяформа");
    Т3 = Тип("УправляемаяФорма");
    Т4 = TYPE("MANAGEDFORM");
    Т5 = type("managedform");
    Т6 = Type("ManagedForm");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedTypeManagedForm)
            .collect();

        assert_eq!(deprecated_diags.len(), 6);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/DeprecatedTypeManagedForm.bsl");
        let diagnostics = check_hir_diagnostic(input);

        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedTypeManagedForm)
            .collect();

        assert_eq!(deprecated_diags.len(), 2, "Expected 2 diagnostics");

        // Verify diagnostic positions match Java test expectations
        assert_diagnostic_range_multiline(input, deprecated_diags[0], 1, 29, 1, 47);
        assert_diagnostic_range_multiline(input, deprecated_diags[1], 11, 27, 11, 40);
    }
}
