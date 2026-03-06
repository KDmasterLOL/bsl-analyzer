//! WrongUseFunctionProceedWithCall diagnostic
//!
//! Detects wrong usage of ПродолжитьВызов/ProceedWithCall function.
//!
//!
//! The ПродолжитьВызов/ProceedWithCall function can only be called inside extension
//! methods with &Вместо (&Around) annotation. Calling it from methods with &До (&Before),
//! &После (&After), or without extension annotation causes a runtime error.
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! The diagnostic is emitted in `hir-def/body/lower/expr.rs` when a global call
//! to ПродолжитьВызов/ProceedWithCall is encountered and the current method
//! does not have &Вместо annotation.

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
    use crate::test_utils::*;
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
        let diagnostics = check_hir_diagnostic(code);

        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WrongUseFunctionProceedWithCall)
            .collect();

        assert_eq!(diags.len(), 4, "Expected 4 diagnostics, got {}", diags.len());

        // Line 2 (1-indexed), cols 4-19 (0-indexed): ПродолжитьВЫЗОВ()
        assert_diagnostic_range(code, diags[0], 1, 4, 19);

        // Line 6 (1-indexed), cols 4-19 (0-indexed): ProceedWithCall(Парам1)
        assert_diagnostic_range(code, diags[1], 5, 4, 19);

        // Line 11 (1-indexed), cols 13-28 (0-indexed): ПродолжитьВызов() with &Перед
        assert_diagnostic_range(code, diags[2], 10, 13, 28);

        // Line 17 (1-indexed), cols 13-28 (0-indexed): ProceedWithCall() with &После
        assert_diagnostic_range(code, diags[3], 16, 13, 28);
    }

    #[test]
    fn test_proceed_with_call_in_around_method() {
        let code = r#"
&Вместо(ПозитивныйТест)
Процедура Расш_ПозитивныйТест()
    ПродолжитьВызов();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WrongUseFunctionProceedWithCall)
            .collect();
        assert_eq!(diags.len(), 0, "Should not trigger for &Вместо method");
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
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WrongUseFunctionProceedWithCall)
            .collect();
        assert_eq!(diags.len(), 0, "Similar function names should not be flagged");
    }
}
