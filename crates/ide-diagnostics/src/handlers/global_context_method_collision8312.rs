use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_12,
    tags: &[MetadataTag::Error, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    method_name: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::GlobalContextMethodCollision8312;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Имя метода \"{}\" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12",
            method_name
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::{DiagnosticCode, Severity};
    use expect_test::expect;
    #[test]
    fn test_8312() {
        let code = r#"Функция ПроверитьБит()
КонецФункции

Функция ПроверитьПоБитовойМаске()
КонецФункции

Функция УстановитьБит()
КонецФункции

Функция ПобитовоеИ()
КонецФункции

Функция ПобитовоеИли()
КонецФункции

Функция ПобитовоеНе()
КонецФункции

Функция ПобитовоеИНе()
КонецФункции

Функция ПобитовоеИсключительноеИли()
КонецФункции

Функция ПобитовыйСдвигВлево()
КонецФункции

Функция ПобитовыйСдвигВправо()
КонецФункции

Функция CheckBit()
КонецФункции

Функция CheckByBitMask()
КонецФункции

Функция SetBit()
КонецФункции

Функция BitwiseAnd()
КонецФункции

Функция BitwiseOr()
КонецФункции

Функция BitwiseNot()
КонецФункции

Функция BitwiseAndNot()
КонецФункции

Функция BitwiseXor()
КонецФункции

Функция BitwiseShiftLeft()
КонецФункции

Функция BitwiseShiftRight()
КонецФункции

Функция _ПроверитьБит()
КонецФункции

Функция ПроверитьПоБитовойМаске_()
КонецФункции

Функция БИТУстановитьБит()
КонецФункции
"#;

        let diagnostics = check_hir_diagnostic(code);
        let collision_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::GlobalContextMethodCollision8312)
            .collect();

        expect![[r#"
            GlobalContextMethodCollision8312 @ 1:9..1:21
              message: Имя метода "ПроверитьБит" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 4:9..4:32
              message: Имя метода "ПроверитьПоБитовойМаске" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 7:9..7:22
              message: Имя метода "УстановитьБит" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 10:9..10:19
              message: Имя метода "ПобитовоеИ" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 13:9..13:21
              message: Имя метода "ПобитовоеИли" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 16:9..16:20
              message: Имя метода "ПобитовоеНе" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 19:9..19:21
              message: Имя метода "ПобитовоеИНе" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 22:9..22:35
              message: Имя метода "ПобитовоеИсключительноеИли" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 25:9..25:28
              message: Имя метода "ПобитовыйСдвигВлево" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 28:9..28:29
              message: Имя метода "ПобитовыйСдвигВправо" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 31:9..31:17
              message: Имя метода "CheckBit" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 34:9..34:23
              message: Имя метода "CheckByBitMask" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 37:9..37:15
              message: Имя метода "SetBit" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 40:9..40:19
              message: Имя метода "BitwiseAnd" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 43:9..43:18
              message: Имя метода "BitwiseOr" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 46:9..46:19
              message: Имя метода "BitwiseNot" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 49:9..49:22
              message: Имя метода "BitwiseAndNot" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 52:9..52:19
              message: Имя метода "BitwiseXor" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 55:9..55:25
              message: Имя метода "BitwiseShiftLeft" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 58:9..58:26
              message: Имя метода "BitwiseShiftRight" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker"#]].assert_eq(&format_diags(code, &collision_diags));

        for (i, diag) in collision_diags.iter().enumerate() {
            assert_eq!(
                diag.severity,
                Severity::Blocker,
                "Diagnostic {} should have Blocker severity",
                i
            );
        }
    }

    #[test]
    fn test_no_collision_with_prefix_suffix() {
        let code = r#"Функция _ПроверитьБит()
КонецФункции

Функция ПроверитьПоБитовойМаске_()
КонецФункции

Функция БИТУстановитьБит()
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let collision_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::GlobalContextMethodCollision8312)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &collision_diags));
    }

    #[test]
    fn test_case_insensitive_russian() {
        let code = r#"Функция ПРОВЕРИТЬБИТ()
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let collision_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::GlobalContextMethodCollision8312)
            .collect();

        expect![[r#"
            GlobalContextMethodCollision8312 @ 1:9..1:21
              message: Имя метода "ПРОВЕРИТЬБИТ" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker"#]].assert_eq(&format_diags(code, &collision_diags));
    }

    #[test]
    fn test_case_insensitive_english() {
        let code = r#"Function CheckBit()
EndFunction"#;

        let diagnostics = check_hir_diagnostic(code);
        let collision_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::GlobalContextMethodCollision8312)
            .collect();

        expect![[r#"
            GlobalContextMethodCollision8312 @ 1:10..1:18
              message: Имя метода "CheckBit" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker"#]].assert_eq(&format_diags(code, &collision_diags));
    }

    #[test]
    fn test_multiple_collisions() {
        let code = r#"Функция ПроверитьБит()
КонецФункции

Функция CheckBit()
КонецФункции

Функция ПобитовоеИ()
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let collision_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::GlobalContextMethodCollision8312)
            .collect();

        expect![[r#"
            GlobalContextMethodCollision8312 @ 1:9..1:21
              message: Имя метода "ПроверитьБит" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 4:9..4:17
              message: Имя метода "CheckBit" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker
            GlobalContextMethodCollision8312 @ 7:9..7:19
              message: Имя метода "ПобитовоеИ" конфликтует с методом глобального контекста, появившимся в версии платформы 8.3.12
              severity: Blocker"#]].assert_eq(&format_diags(code, &collision_diags));
    }

    #[test]
    fn test_no_collision() {
        let code = r#"Функция МояФункция()
КонецФункции

Функция ВычислитьСумму()
КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let collision_diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::GlobalContextMethodCollision8312)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &collision_diags));
    }
}
