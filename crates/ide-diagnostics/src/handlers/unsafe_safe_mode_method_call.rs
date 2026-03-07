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

    const FIXTURE: &str = r#"Процедура Тест()
    Если БезопасныйРежим() ИЛИ Тест = Истина Тогда  // Срабатывание
         // Логика выполнения в безопасном режиме...
    ИначеЕсли Не БезопасныйРежим() Тогда // Срабатывание
        // Логика выполнения в небезопасном режиме...
    КонецЕсли;

    Если Не БезопасныйРежим() Тогда // Срабатывание
         // Логика выполнения в небезопасном режиме...
    КонецЕсли;

    Если Условие И (Условие2 Или БезопасныйРежим()) Тогда // Есть срабатывание
    КонецЕсли;

    ФинальноеУсловие = Условие И (Условие2 Или БезопасныйРежим());  // Есть срабатывание

    ФинальноеУсловие = Условие И (Условие2 Или Не БезопасныйРежим());  // Есть срабатывание

    ФинальноеУсловие = Условие И (БезопасныйРежим() Или Условие2);  // Есть срабатывание

    Если Условие И (Условие2 И Не БезопасныйРежим())) Тогда // Есть срабатывание
    КонецЕсли;

    Если Условие И (БезопасныйРежим() И Условие)) Тогда // Есть срабатывание
    КонецЕсли;

    Если БезопасныйРежим() Тогда //Есть срабатывание
        // Логика выполнения в безопасном режиме...
    КонецЕсли;

    Если БезопасныйРежим() <> Ложь Тогда // Нет срабатывания
        // Логика выполнения в безопасном режиме...
    КонецЕсли;

    Если Тест() ИЛИ Тест = Истина Тогда  // Нет срабатывания
        // код
    КонецЕсли;

    Если Истина Тогда
        Перем1 = БезопасныйРежим();  // Нет срабатывания

        Перем2 = Метод(БезопасныйРежим());  // Нет срабатывания
    КонецЕсли;

    ФинальноеУсловие1 = Условие1 И (Условие12 Или БезопасныйРежим() = Истина);  // Нет срабатывания

    ФинальноеУсловие2 = Условие2 И (Ложь <> БезопасныйРежим() Или Условие2);  // Нет срабатывания

КонецПроцедуры"#;

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
        let code = FIXTURE;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 10, "Expected exactly 10 diagnostics");
    }

    #[test]
    fn test_comprehensive_fixture_positions() {
        let code = FIXTURE;
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
