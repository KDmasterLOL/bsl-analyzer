//! UseSystemInformation diagnostic.
//!
//! Detects instantiation of СистемнаяИнформация / SystemInfo class.
//!
//! ## Why?
//! SystemInformation object provides access to system and configuration data
//! (computer name, RAM, processor info, etc.) that could be misused for:
//! - Information disclosure
//! - Fingerprinting attacks
//! - Security policy violations
//!
//! ## Bad practice
//! ```bsl
//! Процедура ПолучитьИнфо()
//!     СисИнфо = Новый СистемнаяИнформация;
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** No
//! - **Severity:** Critical
//! - **Type:** SECURITY_HOTSPOT

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::UseSystemInformation;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Use of system information".to_string(),
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
        let code = include_str!("../../test_data/UseSystemInformationDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UseSystemInformation).collect();

        assert_eq!(diags.len(), 5, "Expected 5 diagnostics, got {}", diags.len());

        // Line 2 (0-indexed: 1), cols 26-51: Новый СистемнаяИнформация
        assert_diagnostic_range(code, diags[0], 1, 26, 51);
        // Line 6 (0-indexed: 5), cols 22-49: Новый СистемнаяИнформация()
        assert_diagnostic_range(code, diags[1], 5, 22, 49);
        // Line 7 (0-indexed: 6), cols 22-50: Новый("СистемнаяИнформация")
        assert_diagnostic_range(code, diags[2], 6, 22, 50);
        // Line 8 (0-indexed: 7), cols 22-38: Новый SystemInfo
        assert_diagnostic_range(code, diags[3], 7, 22, 38);
        // Line 9 (0-indexed: 8), cols 22-41: Новый("SystemInfo")
        assert_diagnostic_range(code, diags[4], 8, 22, 41);
    }

    #[test]
    fn test_no_false_positives() {
        let code = r#"
Процедура Тест()
    СистемнаяИнформация = Новый("СистемнаяИнформация2");
    ИмяТипа = "СистемнаяИнформация";
    СистемнаяИнформация = Новый(ИмяТипа);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UseSystemInformation).collect();
        assert_eq!(diags.len(), 0, "Should not detect non-matching type names or variables");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    А = Новый СИСТЕМНАЯИНФОРМАЦИЯ;
    Б = Новый systeminfo;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UseSystemInformation).collect();
        assert_eq!(diags.len(), 2, "Should detect case-insensitive type names");
    }
}
