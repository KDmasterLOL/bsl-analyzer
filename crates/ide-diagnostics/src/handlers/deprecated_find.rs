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
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_6,
    tags: &[MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

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
    use expect_test::expect;
    #[test]
    fn test_deprecated_russian() {
        let code = r#"
Процедура Тест()
    Позиция = Найти("Строка", "о");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::DeprecatedFind).collect();

        expect![[r#"
            DeprecatedFind @ 3:15..3:20
              message: Используйте "СтрНайти" вместо устаревшего "Найти"
              severity: Information"#]]
        .assert_eq(&format_diags(code, &deprecated_diags));
        assert_eq!(deprecated_diags[0].severity, Severity::Information);
        assert!(deprecated_diags[0].message.contains("СтрНайти")); // snapshot-skip: message-substring assertion intentionally retained.
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
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::DeprecatedFind).collect();

        expect![[r#"
            DeprecatedFind @ 3:16..3:20
              message: Use "StrFind" instead of deprecated "Find"
              severity: Information"#]]
        .assert_eq(&format_diags(code, &deprecated_diags));
        assert!(deprecated_diags[0].message.contains("StrFind")); // snapshot-skip: message-substring assertion intentionally retained.
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
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::DeprecatedFind).collect();

        // Should not trigger for method calls
        expect![[r#""#]].assert_eq(&format_diags(code, &deprecated_diags));
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
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::DeprecatedFind).collect();

        expect![[r#"
            DeprecatedFind @ 3:12..3:17
              message: Используйте "СтрНайти" вместо устаревшего "Найти"
              severity: Information
            DeprecatedFind @ 4:12..4:17
              message: Используйте "СтрНайти" вместо устаревшего "Найти"
              severity: Information
            DeprecatedFind @ 5:12..5:17
              message: Используйте "СтрНайти" вместо устаревшего "Найти"
              severity: Information
            DeprecatedFind @ 6:12..6:17
              message: Используйте "СтрНайти" вместо устаревшего "Найти"
              severity: Information"#]]
        .assert_eq(&format_diags(code, &deprecated_diags));
    }

    #[test]
    fn test_in_proc_and_toplevel() {
        // One deprecated call inside a procedure and one at module top-level.
        let code = r#"
Процедура А()

   Если НайтИ(Сотрудник.Имя, "Борис") > 0 Тогда
       Сообщить(Сотрудник.Имя + " таб. №" + Сотрудник.Код);
   КонецЕсли;

КонецПроцедуры

If FinD("A", "B") Then
EndIf;"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::DeprecatedFind).collect();
        expect![[r#"
            DeprecatedFind @ 4:9..4:14
              message: Используйте "СтрНайти" вместо устаревшего "Найти"
              severity: Information
            DeprecatedFind @ 10:4..10:8
              message: Use "StrFind" instead of deprecated "Find"
              severity: Information"#]]
        .assert_eq(&format_diags(code, &deprecated_diags));
    }
}
