//! OSUsersMethod diagnostic.
//!
//! Reports calls to the global method `ПользователиОС()` / `OSUsers()`.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

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
    crate::simple_hir_diagnostic(
        DiagnosticCode::OSUsersMethod,
        "Check for a potentially dangerous OSUsers method call",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;
    #[test]
    fn test_detects_os_users_calls() {
        let code = r#"Функция Тест1()
Сообщить("Здесь не должно сработать");
КонецФункции

Функция Тест2()
Пользователи = ПользователиОС(); // сработает здесь
КонецФункции

Функция Тест3()
Users = OSUsers(); // сработает здесь
КонецФункции

Функция Тест4()
Users = osUsers(); // сработает здесь
КонецФункции
"#;
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
