//! DeprecatedFind diagnostic.
//!
//! Detects usage of deprecated global `Найти()` / `Find()` methods.
//!
//! ## Why?
//! The global `Найти()` / `Find()` method is deprecated since 1C:Enterprise 8.3.6:
//! - Ambiguous name (conflicts with collection methods)
//! - Use `СтрНайти()` / `StrFind()` for string search instead
//! - Use collection's `.Найти()` method for collections
//! - Better code clarity and type safety
//!
//! ## Bad practice
//! ```bsl
//! Процедура Тест()
//!     Позиция = Найти("Строка", "о"); // ❌ Global Найти() is deprecated
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Тест()
//!     // ✅ For string search - use СтрНайти()
//!     Позиция = СтрНайти("Строка", "о");
//!
//!     // ✅ For collection search - use collection method
//!     Индекс = Массив.Найти("Элемент");
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Information (MINOR)
//! - **Tags:** DEPRECATED
//! - **Minutes to fix:** 2
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! Ported from:
//! - DeprecatedFindDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - deprecated_find.rs (bsl-language-server-rust) - Rust reference

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::DeprecatedFind` is encountered.
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    // Check if the diagnostic is disabled
    let code = DiagnosticCode::DeprecatedFind;

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
    if lower == "найти" {
        "Используйте \"СтрНайти\" вместо устаревшего \"Найти\"".to_string()
    } else {
        "Use \"StrFind\" instead of deprecated \"Find\"".to_string()
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
    Позиция = Найти("Строка", "о");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DeprecatedFind).collect();

        assert_eq!(deprecated_diags.len(), 1);
        assert_eq!(deprecated_diags[0].severity, Severity::Information);
        assert!(deprecated_diags[0].message.contains("СтрНайти"));
    }

    #[test]
    fn test_deprecated_english() {
        let code = r#"
Procedure Test()
    Position = Find("String", "S");
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DeprecatedFind).collect();

        assert_eq!(deprecated_diags.len(), 1);
        assert!(deprecated_diags[0].message.contains("StrFind"));
    }

    #[test]
    fn test_collection_method_excluded() {
        let code = r#"
Процедура Тест()
    Индекс = Массив.Найти("Элемент");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DeprecatedFind).collect();

        // Should not trigger for method calls
        assert_eq!(deprecated_diags.len(), 0);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Поз1 = НАЙТИ("A", "B");
    Поз2 = найти("C", "D");
    Поз3 = Найти("E", "F");
    Поз4 = НайтИ("G", "H");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DeprecatedFind).collect();

        assert_eq!(deprecated_diags.len(), 4);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/DeprecatedFindDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(input);

        let deprecated_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::DeprecatedFind).collect();

        assert_eq!(deprecated_diags.len(), 2, "Expected 2 diagnostics");

        // Verify diagnostic positions match Java test expectations
        assert_diagnostic_range(input, deprecated_diags[0], 3, 8, 13);
        assert_diagnostic_range(input, deprecated_diags[1], 9, 3, 7);
    }
}
