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

/// Creates a diagnostic from HIR lowering data for any ternary `?(...)` usage.
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
    use expect_test::expect;

    #[test]
    fn test_from_java_fixture() {
        let code = r#"
ПериодПо = ?(Шапка.ЭтоУвольнение
           , Шапка.Дата
           , ?(Шапка.ЭтоАванс
             , Дата(Год(Шапка.ПериодРегистрации)
                   , Месяц(Шапка.ПериодРегистрации)
                   , 15
                   )
             , КонецМесяца(Шапка.ПериодРегистрации)
             )
            );

Статус = ?(ПолучитьСкидку() > МаксимальныйПроцент, "Особый клиент", "Обычный клиент");

Если ?(ПолучитьСкидку() > МаксимальныйПроцент, Истина, Ложь) Тогда
    Возврат Истина;
Иначе
    Возврат Ложь;
КонецЕсли;"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TernaryOperatorUsage,
            expect![[r#"
                TernaryOperatorUsage @ 2:12..11:14
                  message: Используйте конструкцию Если-Иначе вместо тернарного оператора
                  severity: Information
                TernaryOperatorUsage @ 4:14..10:15
                  message: Используйте конструкцию Если-Иначе вместо тернарного оператора
                  severity: Information
                TernaryOperatorUsage @ 13:10..13:86
                  message: Используйте конструкцию Если-Иначе вместо тернарного оператора
                  severity: Information
                TernaryOperatorUsage @ 15:6..15:61
                  message: Используйте конструкцию Если-Иначе вместо тернарного оператора
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_simple_ternary() {
        let code = r#"Процедура Тест()
    Результат = ?(Условие, Истина, Ложь);
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TernaryOperatorUsage,
            expect![[r#"
                TernaryOperatorUsage @ 2:17..2:41
                  message: Используйте конструкцию Если-Иначе вместо тернарного оператора
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_nested_ternary() {
        let code = r#"Процедура Тест()
    Результат = ?(Условие1, ?(Условие2, 1, 2), 3);
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TernaryOperatorUsage,
            expect![[r#"
                TernaryOperatorUsage @ 2:17..2:50
                  message: Используйте конструкцию Если-Иначе вместо тернарного оператора
                  severity: Information
                TernaryOperatorUsage @ 2:29..2:46
                  message: Используйте конструкцию Если-Иначе вместо тернарного оператора
                  severity: Information"#]],
        );
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

        assert_eq!(diags.len(), 0, "Should be disabled by default"); // snapshot-skip: custom default-config assertion intentionally retained.
    }
}
