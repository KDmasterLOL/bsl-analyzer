use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::ProcedureReturnsValue,
        "Процедура не должна возвращать значение",
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
    fn test_procedure_with_return_value() {
        let code = r#"Процедура Тест()
    Возврат 42;
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ProcedureReturnsValue,
            expect![[r#"
            ProcedureReturnsValue @ 2:5..2:16
              message: Процедура не должна возвращать значение
              severity: Blocker"#]],
        );
    }

    #[test]
    fn test_procedure_without_return_value_ok() {
        let code = r#"Процедура Тест()
    Возврат;
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ProcedureReturnsValue,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_procedure_return_without_semicolon_before_endif() {
        let code = r#"Процедура Тест()
    Если Истина Тогда
        Возврат
    КонецЕсли;
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ProcedureReturnsValue,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_function_with_return_value_ok() {
        let code = r#"Функция Тест()
    Возврат 42;
КонецФункции"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ProcedureReturnsValue,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_fixture_java_compatibility() {
        let code = r#"Функция ОдноЗначение()
    Возврат "Значение";
КонецФункции

Процедура ПерваяПроцедура()

    Тест = 1;
    Возврат;
    Возврат Тест;

КонецПроцедуры

Процедура ПромежуточнаяПроцедура()

    Значение = Истина;
    Если Значение = Истина Тогда
        Возврат ОдноЗначение() + " 2";
    КонецЕсли;

КонецПроцедуры

Процедура ВтораяПроцедура()

    Накопитель = 1;
    Для Счетчик = 1 По 2 Цикл
        Накопитель = Накопитель + 1;

        Если Накопитель = 2 Тогда
            Возврат Накопитель;
        КонецЕсли;
    КонецЦикла;

    Возврат;

КонецПроцедуры

Процедура ТретьяПроцедура()
    Тест = 2;
    Если Тест = 2 Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ProcedureReturnsValue,
            expect![[r#"
            ProcedureReturnsValue @ 9:5..9:18
              message: Процедура не должна возвращать значение
              severity: Blocker
            ProcedureReturnsValue @ 17:9..17:39
              message: Процедура не должна возвращать значение
              severity: Blocker
            ProcedureReturnsValue @ 29:13..29:32
              message: Процедура не должна возвращать значение
              severity: Blocker"#]],
        );
    }
}
