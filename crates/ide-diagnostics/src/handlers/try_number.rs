use crate::define_metadata;
use crate::metadata::*;
use crate::AnalysisContext;
use crate::{Diagnostic, DiagnosticCode};
use hir::LocalRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: LocalRange, ctx: &AnalysisContext) -> Option<Diagnostic<LocalRange>> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::TryNumber,
        "Не используйте try-catch для приведения к числу",
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
    fn test_comprehensive() {
        let code = r#"Попытка

Исключение
    А = Число(Б);
КонецПопытки


Попытка
А = ЧислО(Б);
Б = NumbeR(4);

    Попытка
    В = Number(4);

    Исключение

    КонецПопытки
Исключение

КонецПопытки

F = Number();
А = Число(Б);
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TryNumber,
            expect![[r#"
            TryNumber @ 9:5..9:13
              message: Не используйте try-catch для приведения к числу
              severity: Warning
            TryNumber @ 10:5..10:14
              message: Не используйте try-catch для приведения к числу
              severity: Warning
            TryNumber @ 13:9..13:18
              message: Не используйте try-catch для приведения к числу
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_hir_detection() {
        let code = r#"
Процедура Тест()
    Попытка
        А = Число(Б);
    Исключение
    КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TryNumber,
            expect![[r#"
            TryNumber @ 4:13..4:21
              message: Не используйте try-catch для приведения к числу
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_number_in_except_not_detected() {
        let code = r#"
Процедура Тест()
Попытка
Исключение
    А = Число(Б);
КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::TryNumber, expect![[r#""#]]);
    }

    #[test]
    fn test_number_outside_try_not_detected() {
        let code = r#"
Процедура Тест()
F = Number();
А = Число(Б);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::TryNumber, expect![[r#""#]]);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
Попытка
    А = ЧИСЛО(Б);
    Б = Number(4);
КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TryNumber,
            expect![[r#"
            TryNumber @ 4:9..4:17
              message: Не используйте try-catch для приведения к числу
              severity: Warning
            TryNumber @ 5:9..5:18
              message: Не используйте try-catch для приведения к числу
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_nested_try() {
        let code = r#"
Процедура Тест()
Попытка
    Попытка
        В = Number(4);
    КонецПопытки;
КонецПопытки;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TryNumber,
            expect![[r#"
            TryNumber @ 5:13..5:22
              message: Не используйте try-catch для приведения к числу
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_try_with_mixed_body_still_flags_number_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Процедура Тест(Строка)
    Попытка
        Объект.Записать();
        Значение = Число(Строка);
        Сообщить(Значение);
    Исключение
        Сообщить("Ошибка");
    КонецПопытки;
КонецПроцедуры"#,
            DiagnosticCode::TryNumber,
            expect![[r#"
                TryNumber @ 4:20..4:33
                  message: Не используйте try-catch для приведения к числу
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_number_inside_if_in_try_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Процедура Тест(Значение)
    Попытка
        Если Значение <> "" Тогда
            Результат = Number(Значение);
        КонецЕсли;
    Исключение
        Сообщить("error");
    КонецПопытки;
КонецПроцедуры"#,
            DiagnosticCode::TryNumber,
            expect![[r#"
                TryNumber @ 4:25..4:41
                  message: Не используйте try-catch для приведения к числу
                  severity: Warning"#]],
        );
    }
}
