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
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::OSUsersMethod,
            expect![[r#"
            OSUsersMethod @ 6:16..6:30
              message: Check for a potentially dangerous OSUsers method call
              severity: Warning
            OSUsersMethod @ 10:9..10:16
              message: Check for a potentially dangerous OSUsers method call
              severity: Warning
            OSUsersMethod @ 14:9..14:16
              message: Check for a potentially dangerous OSUsers method call
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    ПОЛЬЗОВАТЕЛИОС();
    OSUSERS();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::OSUsersMethod,
            expect![[r#"
            OSUsersMethod @ 3:5..3:19
              message: Check for a potentially dangerous OSUsers method call
              severity: Warning
            OSUsersMethod @ 4:5..4:12
              message: Check for a potentially dangerous OSUsers method call
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_no_false_positives() {
        let code = r#"
Процедура Тест()
    Переменная = ПользователиОС;
    МойМодуль.ПользователиОС();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::OSUsersMethod, expect![[r#""#]]);
    }
}
