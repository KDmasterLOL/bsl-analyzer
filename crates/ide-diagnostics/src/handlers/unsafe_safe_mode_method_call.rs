use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_1,
    tags: &[MetadataTag::Deprecated, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::UnsafeSafeModeMethodCall,
        "Use explicit comparison with boolean when calling SafeMode method",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;

    fn filter(diagnostics: &[crate::Diagnostic]) -> Vec<&crate::Diagnostic> {
        diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnsafeSafeModeMethodCall).collect()
    }

    #[test]
    fn test_safe_direct_assignment() {
        let code = r#"
Процедура Тест()
    Перем1 = БезопасныйРежим();
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0);
    }

    #[test]
    fn test_safe_method_argument() {
        let code = r#"
Процедура Тест()
    Перем2 = Метод(БезопасныйРежим());
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0);
    }

    #[test]
    fn test_safe_explicit_comparison() {
        let code = r#"
Процедура Тест()
    Если БезопасныйРежим() <> Ложь Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0);
    }

    #[test]
    fn test_unsafe_sole_condition() {
        let code = r#"
Процедура Тест()
    Если БезопасныйРежим() Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 1);
    }

    #[test]
    fn test_unsafe_with_not() {
        let code = r#"
Процедура Тест()
    Если Не БезопасныйРежим() Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 1);
    }

    #[test]
    fn test_unsafe_with_or() {
        let code = r#"
Процедура Тест()
    Если БезопасныйРежим() ИЛИ Тест = Истина Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 1);
    }

    #[test]
    fn test_comprehensive_fixture() {
        let code = include_str!("../test_data/UnsafeSafeModeMethodCallDiagnostic.bsl");
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 10, "Expected exactly 10 diagnostics matching Java");
    }

    #[test]
    fn test_comprehensive_fixture_positions() {
        let code = include_str!("../test_data/UnsafeSafeModeMethodCallDiagnostic.bsl");
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);

        assert_eq!(diags.len(), 10);

        assert_diagnostic_range(code, diags[0], 1, 9, 24);
        assert_diagnostic_range(code, diags[1], 3, 17, 32);
        assert_diagnostic_range(code, diags[2], 7, 12, 27);
        assert_diagnostic_range(code, diags[3], 11, 33, 48);
        assert_diagnostic_range(code, diags[4], 14, 47, 62);
        assert_diagnostic_range(code, diags[5], 16, 50, 65);
        assert_diagnostic_range(code, diags[6], 18, 34, 49);
        assert_diagnostic_range(code, diags[7], 20, 34, 49);
        assert_diagnostic_range(code, diags[8], 23, 20, 35);
        assert_diagnostic_range(code, diags[9], 26, 9, 24);
    }

    #[test]
    fn test_safe_comparison() {
        let code = r#"
Процедура Тест()
    Если БезопасныйРежим() = Истина Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0);
    }
}
