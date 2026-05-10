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

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Deprecated, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::DeprecatedCurrentDate` is encountered.
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    // Check if the diagnostic is disabled
    let code = DiagnosticCode::DeprecatedCurrentDate;

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
    use crate::Severity;
    use expect_test::expect;
    #[test]
    fn test_deprecated_russian() {
        let code = r#"
Процедура Тест()
    Дата = ТекущаяДата();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedCurrentDate)
            .collect();

        expect![[r#"
            DeprecatedCurrentDate @ 3:12..3:23
              message: Используйте "ТекущаяДатаСеанса" вместо устаревшего "ТекущаяДата"
              severity: Major"#]]
        .assert_eq(&format_diags(code, &deprecated_diags));
        assert_eq!(deprecated_diags[0].severity, Severity::Major); // ERROR + MAJOR maps to Major
        assert!(deprecated_diags[0].message.contains("ТекущаяДатаСеанса")); // snapshot-skip: message-substring assertion intentionally retained.
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedCurrentDate)
            .collect();

        expect![[r#"
            DeprecatedCurrentDate @ 3:12..3:23
              message: Use "CurrentSessionDate" instead of deprecated "CurrentDate"
              severity: Major"#]]
        .assert_eq(&format_diags(code, &deprecated_diags));
        assert!(deprecated_diags[0].message.contains("CurrentSessionDate")); // snapshot-skip: message-substring assertion intentionally retained.
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedCurrentDate)
            .collect();

        // Should not trigger for method calls
        expect![[r#""#]].assert_eq(&format_diags(code, &deprecated_diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedCurrentDate)
            .collect();

        expect![[r#"
            DeprecatedCurrentDate @ 3:13..3:24
              message: Используйте "ТекущаяДатаСеанса" вместо устаревшего "ТекущаяДата"
              severity: Major
            DeprecatedCurrentDate @ 4:13..4:24
              message: Используйте "ТекущаяДатаСеанса" вместо устаревшего "ТекущаяДата"
              severity: Major
            DeprecatedCurrentDate @ 5:13..5:24
              message: Используйте "ТекущаяДатаСеанса" вместо устаревшего "ТекущаяДата"
              severity: Major"#]]
        .assert_eq(&format_diags(code, &deprecated_diags));
    }

    #[test]
    fn test_two_procs_one_each_language() {
        // One deprecated call in Russian and one in English;
        // session date and object method calls should not trigger.
        let code = r#"
Процедура А()
    ДатаПроверки = ТекущаяДата();
КонецПроцедуры

Процедура Б()
    ДатаПроверки = ТекущаяДатаСеанса();
    Модуль.ТекущаяДата();
КонецПроцедуры

Procedure A()
    CheckDate = CurrentDate();
EndProcedure"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedCurrentDate)
            .collect();
        expect![[r#"
            DeprecatedCurrentDate @ 3:20..3:31
              message: Используйте "ТекущаяДатаСеанса" вместо устаревшего "ТекущаяДата"
              severity: Major
            DeprecatedCurrentDate @ 12:17..12:28
              message: Use "CurrentSessionDate" instead of deprecated "CurrentDate"
              severity: Major"#]]
        .assert_eq(&format_diags(code, &deprecated_diags));
    }
}
