//! Reports `ПродолжитьВызов` / `ProceedWithCall` calls outside `&Вместо` methods.

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
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::WrongUseFunctionProceedWithCall,
        "Использовать функцию ПродолжитьВызов() можно только в расширениях и только в методах с аннотацией &Вместо.",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_diagnostics_snapshot_for;
    use expect_test::expect;
    #[test]
    fn test_wrong_use_function_proceed_with_call() {
        let code = r#"Процедура НегативныйТест1()
    ПродолжитьВЫЗОВ(); // <- Ошибка, вызов в методе без аннотации &Вместо
КонецПроцедуры

Процедура НегативныйТест2(Парам1)
    ProceedWithCall(Парам1); // <- Ошибка, вызов в методе без аннотации &Вместо
КонецПроцедуры

&Перед(НегативныйТест3)
Функция Расш_НегативныйТест3(Парам1, Парам2)
    Перем1 = ПродолжитьВызов(Парам1, Парам2); // <- Ошибка, вызов в методе без аннотации &Вместо
    Возврат Перем1;
КонецФункции

&После(НегативныйТест4)
Функция Расш_НегативныйТест4(Парам1, Парам2)
    Перем1 = ProceedWithCall(Парам1, Парам2); // <- Ошибка, вызов в методе без аннотации &Вместо
    Возврат Перем1;
КонецФункции

Процедура А()
    ПродолжитьВызовОбработчика();
    _ПродолжитьВызов();
    Модуль.ПродолжитьВызовОбработчика();
    Модуль._ПродолжитьВызов();
КонецПроцедуры

&Вместо(ПозитивныйТест1)
Процедура Расш_ПозитивныйТест1()
    ПродолжитьВЫЗОВ();
КонецПроцедуры

&Вместо(ПозитивныйТест2)
Процедура Расш_ПозитивныйТест2(Парам1)
    ProceedWithCall(Парам1);
КонецПроцедуры

&Вместо(ПозитивныйТест3)
Функция Расш_ПозитивныйТест3(Парам1, Парам2)
    Перем1 = ПродолжитьВызов(Парам1, Парам2);
    Возврат Перем1;
КонецФункции

&Вместо(ПозитивныйТест4)
Функция Расш_ПозитивныйТест4(Парам1, Парам2)
    Перем1 = ProceedWithCall(Парам1, Парам2);
    Возврат Перем1;
КонецФункции
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::WrongUseFunctionProceedWithCall,
            expect![[r#"
                WrongUseFunctionProceedWithCall @ 2:5..2:20
                  message: Использовать функцию ПродолжитьВызов() можно только в расширениях и только в методах с аннотацией &Вместо.
                  severity: Blocker
                WrongUseFunctionProceedWithCall @ 6:5..6:20
                  message: Использовать функцию ПродолжитьВызов() можно только в расширениях и только в методах с аннотацией &Вместо.
                  severity: Blocker
                WrongUseFunctionProceedWithCall @ 11:14..11:29
                  message: Использовать функцию ПродолжитьВызов() можно только в расширениях и только в методах с аннотацией &Вместо.
                  severity: Blocker
                WrongUseFunctionProceedWithCall @ 17:14..17:29
                  message: Использовать функцию ПродолжитьВызов() можно только в расширениях и только в методах с аннотацией &Вместо.
                  severity: Blocker"#]],
        );
    }

    #[test]
    fn test_proceed_with_call_in_around_method() {
        let code = r#"
&Вместо(ПозитивныйТест)
Процедура Расш_ПозитивныйТест()
    ПродолжитьВызов();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::WrongUseFunctionProceedWithCall,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_similar_function_names_not_flagged() {
        let code = r#"
Процедура Тест()
    ПродолжитьВызовОбработчика();
    _ПродолжитьВызов();
    Модуль.ПродолжитьВызовОбработчика();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::WrongUseFunctionProceedWithCall,
            expect![[r#""#]],
        );
    }
}
