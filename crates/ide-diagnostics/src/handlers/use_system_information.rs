use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::UseSystemInformation,
        "Use of system information",
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
    fn test_java_fixture() {
        let code = r#"Функция ТипТекущейПлатформы() Экспорт
    СистемнаяИнформация = Новый СистемнаяИнформация; // Range(1, 26, 1, 51)
    Возврат СистемнаяИнформация.ТипПлатформы;
КонецФункции

СистемнаяИнформация = Новый СистемнаяИнформация();   // Range(5, 22, 5, 49)
СистемнаяИнформация = Новый("СистемнаяИнформация");  // Range(6, 22, 6, 50)
СистемнаяИнформация = Новый SystemInfo;              // Range(7, 22, 7, 38)
СистемнаяИнформация = Новый("SystemInfo");           // Range(8, 22, 8, 41)
СистемнаяИнформация = Новый("СистемнаяИнформация2");

ИмяТипа = "СистемнаяИнформация";
СистемнаяИнформация = Новый(ИмяТипа);
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UseSystemInformation,
            expect![[r#"
            UseSystemInformation @ 2:27..2:52
              message: Use of system information
              severity: Warning
            UseSystemInformation @ 6:23..6:50
              message: Use of system information
              severity: Warning
            UseSystemInformation @ 7:23..7:51
              message: Use of system information
              severity: Warning
            UseSystemInformation @ 8:23..8:39
              message: Use of system information
              severity: Warning
            UseSystemInformation @ 9:23..9:42
              message: Use of system information
              severity: Warning"#]],
        );
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UseSystemInformation,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    А = Новый СИСТЕМНАЯИНФОРМАЦИЯ;
    Б = Новый systeminfo;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UseSystemInformation,
            expect![[r#"
            UseSystemInformation @ 3:9..3:34
              message: Use of system information
              severity: Warning
            UseSystemInformation @ 4:9..4:25
              message: Use of system information
              severity: Warning"#]],
        );
    }
}
