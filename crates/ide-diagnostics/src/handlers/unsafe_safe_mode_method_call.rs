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
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

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

    #[test]
    fn test_safe_direct_assignment() {
        let code = r#"
Процедура Тест()
    Перем1 = БезопасныйРежим();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnsafeSafeModeMethodCall,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_safe_method_argument() {
        let code = r#"
Процедура Тест()
    Перем2 = Метод(БезопасныйРежим());
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnsafeSafeModeMethodCall,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_safe_explicit_comparison() {
        let code = r#"
Процедура Тест()
    Если БезопасныйРежим() <> Ложь Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnsafeSafeModeMethodCall,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_unsafe_sole_condition() {
        let code = r#"
Процедура Тест()
    Если БезопасныйРежим() Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnsafeSafeModeMethodCall,
            expect![[r#"
                UnsafeSafeModeMethodCall @ 3:10..3:25
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker"#]],
        );
    }

    #[test]
    fn test_unsafe_with_not() {
        let code = r#"
Процедура Тест()
    Если Не БезопасныйРежим() Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnsafeSafeModeMethodCall,
            expect![[r#"
                UnsafeSafeModeMethodCall @ 3:13..3:28
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker"#]],
        );
    }

    #[test]
    fn test_unsafe_with_or() {
        let code = r#"
Процедура Тест()
    Если БезопасныйРежим() ИЛИ Тест = Истина Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnsafeSafeModeMethodCall,
            expect![[r#"
                UnsafeSafeModeMethodCall @ 3:10..3:25
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker"#]],
        );
    }

    #[test]
    fn test_comprehensive_fixture() {
        check_diagnostics_snapshot_for(
            FIXTURE,
            DiagnosticCode::UnsafeSafeModeMethodCall,
            expect![[r#"
                UnsafeSafeModeMethodCall @ 2:10..2:25
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 4:18..4:33
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 8:13..8:28
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 12:34..12:49
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 15:48..15:63
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 17:51..17:66
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 19:35..19:50
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 21:35..21:50
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 24:21..24:36
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 27:10..27:25
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker"#]],
        );
    }

    #[test]
    fn test_comprehensive_fixture_positions() {
        check_diagnostics_snapshot_for(
            FIXTURE,
            DiagnosticCode::UnsafeSafeModeMethodCall,
            expect![[r#"
                UnsafeSafeModeMethodCall @ 2:10..2:25
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 4:18..4:33
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 8:13..8:28
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 12:34..12:49
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 15:48..15:63
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 17:51..17:66
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 19:35..19:50
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 21:35..21:50
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 24:21..24:36
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker
                UnsafeSafeModeMethodCall @ 27:10..27:25
                  message: Use explicit comparison with boolean when calling SafeMode method
                  severity: Blocker"#]],
        );
    }

    #[test]
    fn test_safe_comparison() {
        let code = r#"
Процедура Тест()
    Если БезопасныйРежим() = Истина Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::UnsafeSafeModeMethodCall,
            expect![[r#""#]],
        );
    }
}
