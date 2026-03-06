use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::TernaryOperatorUsage,
        "Используйте конструкцию Если-Иначе вместо тернарного оператора",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::{DiagnosticCode, DiagnosticsConfig};

    fn config_with_ternary_enabled() -> DiagnosticsConfig {
        let mut config = DiagnosticsConfig::default();
        config.enabled.push(DiagnosticCode::TernaryOperatorUsage);
        config
    }

    #[test]
    fn test_from_java_fixture() {
        let code = include_str!("../../test_data/TernaryOperatorUsageDiagnostic.bsl");
        let diagnostics =
            check_hir_diagnostic_with_config(code, config_with_ternary_enabled(), |ctx| {
                crate::diagnostics(ctx)
            });
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TernaryOperatorUsage).collect();

        assert_eq!(diags.len(), 4);
        // bsl-language-server positions: hasRange(1, 11, 10, 13) - 0-based line numbers
        assert_diagnostic_range_multiline(code, diags[0], 1, 11, 10, 13);
        // bsl-language-server positions: hasRange(3, 13, 9, 14)
        assert_diagnostic_range_multiline(code, diags[1], 3, 13, 9, 14);
        // bsl-language-server positions: hasRange(12, 9, 12, 85)
        assert_diagnostic_range(code, diags[2], 12, 9, 85);
        // bsl-language-server positions: hasRange(14, 5, 14, 60)
        assert_diagnostic_range(code, diags[3], 14, 5, 60);
    }

    #[test]
    fn test_simple_ternary() {
        let code = r#"Процедура Тест()
    Результат = ?(Условие, Истина, Ложь);
КонецПроцедуры"#;
        let diagnostics =
            check_hir_diagnostic_with_config(code, config_with_ternary_enabled(), |ctx| {
                crate::diagnostics(ctx)
            });
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TernaryOperatorUsage).collect();

        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_nested_ternary() {
        let code = r#"Процедура Тест()
    Результат = ?(Условие1, ?(Условие2, 1, 2), 3);
КонецПроцедуры"#;
        let diagnostics =
            check_hir_diagnostic_with_config(code, config_with_ternary_enabled(), |ctx| {
                crate::diagnostics(ctx)
            });
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TernaryOperatorUsage).collect();

        assert_eq!(diags.len(), 2, "Should find both outer and inner ternary operators");
    }

    #[test]
    fn test_disabled_by_default() {
        let code = r#"Процедура Тест()
    Результат = ?(Условие, Истина, Ложь);
КонецПроцедуры"#;
        // Use default config (not all_enabled) to test that diagnostic is disabled by default
        let diagnostics =
            check_hir_diagnostic_with_config(code, DiagnosticsConfig::default(), |ctx| {
                crate::diagnostics(ctx)
            });
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TernaryOperatorUsage).collect();

        assert_eq!(diags.len(), 0, "Should be disabled by default");
    }
}
