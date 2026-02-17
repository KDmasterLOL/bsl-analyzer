//! OSUsersMethod diagnostic.
//!
//! Detects calls to ПользователиОС() / OSUsers() global method.
//!
//! ## Why?
//! OSUsers method returns information about operating system users.
//! This creates security vulnerabilities:
//! - Pass-the-hash attack vectors
//! - Information disclosure
//! - May violate security policies
//!
//! ## Bad practice
//! ```bsl
//! Процедура ПолучитьПользователей()
//!     Пользователи = ПользователиОС();
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Critical
//! - **Type:** SECURITY_HOTSPOT

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::OSUsersMethod;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Check for a potentially dangerous OSUsers method call".to_string(),
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
    fn test_java_fixture() {
        let code = include_str!("../../test_data/OSUsersMethodDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::OSUsersMethod).collect();

        assert_eq!(diags.len(), 3, "Expected 3 diagnostics");

        // Line 6 (1-indexed) = 5 (0-indexed), cols 15-29: ПользователиОС
        assert_diagnostic_range(code, diags[0], 5, 15, 29);
        // Line 10 (1-indexed) = 9 (0-indexed), cols 8-15: OSUsers
        assert_diagnostic_range(code, diags[1], 9, 8, 15);
        // Line 14 (1-indexed) = 13 (0-indexed), cols 8-15: osUsers
        assert_diagnostic_range(code, diags[2], 13, 8, 15);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    ПОЛЬЗОВАТЕЛИОС();
    OSUSERS();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::OSUsersMethod).collect();
        assert_eq!(diags.len(), 2, "Should detect uppercase method calls");
    }

    #[test]
    fn test_no_false_positives() {
        let code = r#"
Процедура Тест()
    Переменная = ПользователиОС;
    МойМодуль.ПользователиОС();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::OSUsersMethod).collect();
        assert_eq!(diags.len(), 0, "Should not detect non-call references or qualified calls");
    }
}
