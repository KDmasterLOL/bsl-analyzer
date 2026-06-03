use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::IfElseIfEndsWithElse,
        "Конструкция Если-ИначеЕсли должна заканчиваться блоком Иначе",
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
    fn test_if_elsif_without_else() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    ИначеЕсли Значение = 2 Тогда
        Сообщить("Два");
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfElseIfEndsWithElse,
            expect![[r#"
                IfElseIfEndsWithElse @ 6:5..6:14
                  message: Конструкция Если-ИначеЕсли должна заканчиваться блоком Иначе
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_if_elsif_with_else() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    ИначеЕсли Значение = 2 Тогда
        Сообщить("Два");
    Иначе
        Сообщить("Другое");
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfElseIfEndsWithElse,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_simple_if_without_elsif() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfElseIfEndsWithElse,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_if_else_without_elsif() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    Иначе
        Сообщить("Другое");
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfElseIfEndsWithElse,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_multiple_elsif_without_else() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    ИначеЕсли Значение = 2 Тогда
        Сообщить("Два");
    ИначеЕсли Значение = 3 Тогда
        Сообщить("Три");
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfElseIfEndsWithElse,
            expect![[r#"
                IfElseIfEndsWithElse @ 8:5..8:14
                  message: Конструкция Если-ИначеЕсли должна заканчиваться блоком Иначе
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_multiple_if_statements() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    ИначеЕсли Значение = 2 Тогда
        Сообщить("Два");
    КонецЕсли;

    Если Значение = 3 Тогда
        Сообщить("Три");
    ИначеЕсли Значение = 4 Тогда
        Сообщить("Четыре");
    Иначе
        Сообщить("Другое");
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfElseIfEndsWithElse,
            expect![[r#"
                IfElseIfEndsWithElse @ 6:5..6:14
                  message: Конструкция Если-ИначеЕсли должна заканчиваться блоком Иначе
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_nested_if_elsif() {
        let code = r#"Процедура Тест(Значение1, Значение2)
    Если Значение1 = 1 Тогда
        Если Значение2 = 1 Тогда
            Сообщить("1-1");
        ИначеЕсли Значение2 = 2 Тогда
            Сообщить("1-2");
        КонецЕсли;
    ИначеЕсли Значение1 = 2 Тогда
        Сообщить("2");
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfElseIfEndsWithElse,
            expect![[r#"
                IfElseIfEndsWithElse @ 7:9..7:18
                  message: Конструкция Если-ИначеЕсли должна заканчиваться блоком Иначе
                  severity: Warning
                IfElseIfEndsWithElse @ 10:5..10:14
                  message: Конструкция Если-ИначеЕсли должна заканчиваться блоком Иначе
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_fizzbuzz_without_else_warns() {
        let code = r#"Процедура Тест(x)
    Если x % 15 = 0 Тогда
        Результат = "FizzBuzz";
    ИначеЕсли x % 3 = 0 Тогда
        Результат = "Fizz";
    ИначеЕсли x % 5 = 0 Тогда
        Результат = "Buzz";
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfElseIfEndsWithElse,
            expect![[r#"
                IfElseIfEndsWithElse @ 8:5..8:14
                  message: Конструкция Если-ИначеЕсли должна заканчиваться блоком Иначе
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_fizzbuzz_with_else_passes() {
        let code = r#"Процедура Тест(x)
    Если x % 15 = 0 Тогда
        Результат = "FizzBuzz";
    ИначеЕсли x % 3 = 0 Тогда
        Результат = "Fizz";
    ИначеЕсли x % 5 = 0 Тогда
        Результат = "Buzz";
    Иначе
        Результат = x;
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::IfElseIfEndsWithElse,
            expect![[r#""#]],
        );
    }
}
