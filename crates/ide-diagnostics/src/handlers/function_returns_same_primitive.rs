//! FunctionReturnsSamePrimitive diagnostic
//!
//! Detects functions that always return the same primitive value in all branches.
//!
//!
//! ## Why?
//! Functions that always return the same constant value are useless and indicate poor design:
//! - Should be replaced with a constant or variable
//! - Wastes performance on function calls
//! - Misleading - looks like computed value
//! - Harder to maintain
//!
//! ## Bad practice
//! ```bsl
//! Функция ПолучитьВерсию()
//!     Если Условие Тогда
//!         Возврат "1.0";
//!     Иначе
//!         Возврат "1.0";  // Always returns same value!
//!     КонецЕсли;
//! КонецФункции
//!
//! Функция ПроверкаДанных(Данные)
//!     Если ЭтоПравильно(Данные) Тогда
//!         Возврат Истина;
//!     Иначе
//!         Возврат Истина;  // Always returns True!
//!     КонецЕсли;
//! КонецФункции
//! ```
//!
//! ## Good practice
//! ```bsl
//! Перем Версия = "1.0";  // Use constant/variable
//!
//! Функция ПолучитьВерсию()
//!     Возврат ВычислитьВерсию();  // Computed value
//! КонецФункции
//!
//! Функция ПроверкаДанных(Данные)
//!     Если ЭтоПравильно(Данные) Тогда
//!         Возврат Истина;
//!     Иначе
//!         Возврат Ложь;  // Different values
//!     КонецЕсли;
//! КонецФункции
//! ```

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Design, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when FunctionReturnsSamePrimitive diagnostic is emitted during lowering.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::FunctionReturnsSamePrimitive,
        "Функция всегда возвращает одно и то же примитивное значение. \
         Замените функцию на константу или переменную модуля.",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;
    /// All branches always return True — should trigger.
    #[test]
    fn test_fixture_all_branches_return_true() {
        let code = r#"Функция ПроверитьСтроку(Знач СтрокаТаблицы)
    Если ЭтоХорошаяСтрока(СтрокаТаблицы) Тогда
        ДелаемЧтоТо();
        Возврат Истина;
    ИначеЕсли ЭтоТожеНеплохаяСтрока(СтрокаТаблицы) Тогда
        ДелаемДругоеЧтоТо();
        Возврат Истина;
     Иначе
        Возврат Истина;
    КонецЕсли;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 1, "Expected 1 diagnostic for ПроверитьСтроку");
        assert_diagnostic_range(code, func_diags[0], 0, 8, 23);
    }

    /// Same string in all branches — should trigger.
    #[test]
    fn test_fixture_all_branches_return_same_string() {
        let code = r#"Функция Метод1()
    Значение = "Фича";
    Если Фича = "Дирижабль" Тогда
        Возврат "Фича";
    ИначеЕсли Фича = "Ага" Тогда
        Возврат "Фича";
    КонецЕсли;
    Возврат "Фича";
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 1, "Expected 1 diagnostic for Метод1");
        assert_diagnostic_range(code, func_diags[0], 0, 8, 14);
    }

    /// Same number in all branches — should trigger.
    #[test]
    fn test_fixture_all_branches_return_same_number() {
        let code = r#"Функция СтавкаНДС(Ставка)
    Значение = 20;
    Если Ставка = "Ставка18" Тогда
        Возврат 20;
    КонецЕсли;
    Возврат 20;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 1, "Expected 1 diagnostic for СтавкаНДС");
        assert_diagnostic_range(code, func_diags[0], 0, 8, 17);
    }

    /// Attachable method with same Null — skipped by default.
    #[test]
    fn test_fixture_attachable_prefix_skipped() {
        let code = r#"Функция Подключаемый_КакаяТоКоманда(Команда)

    Если ЗначениеЗаполнено(ТекущаяДата) Тогда
        Возврат Null;
    КонецЕсли;

    Возврат NULL;

КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 0, "Attachable methods should be skipped");
    }

    /// Non-attachable function returning same Null — should trigger.
    #[test]
    fn test_fixture_non_attachable_null_triggers() {
        let code = r#"Функция КакаяТоКоманда(Команда)

    Если ЗначениеЗаполнено(ТекущаяДата) Тогда
        Возврат Null;
    КонецЕсли;

    Возврат NULL;

КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 1, "Expected 1 diagnostic for КакаяТоКоманда");
        assert_diagnostic_range(code, func_diags[0], 0, 8, 22);
    }

    /// Case-insensitive string comparison: "Значение", "значение", "ЗНАЧЕНИЕ" treated as same.
    #[test]
    fn test_fixture_case_insensitive_string_same() {
        let code = r#"Функция ПроверкаРегистраДляСтрок()

    Тип = 1;
    Если Тип = 1 Тогда
        Возврат "Значение";
    ИначеЕсли Тип = 2 Тогда
        Возврат "значение";
    Иначе
        Возврат "ЗНАЧЕНИЕ";
    КонецЕсли;

КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 1, "Expected 1 diagnostic for ПроверкаРегистраДляСтрок");
        assert_diagnostic_range(code, func_diags[0], 0, 8, 32);
    }

    #[test]
    fn test_single_return_no_diagnostic() {
        let code = r#"
Функция БудемТестироватьФункциональность()
    Возврат Ложь;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 0, "Single return should not trigger");
    }

    #[test]
    fn test_returns_variable_no_diagnostic() {
        let code = r#"
Функция СтавкаНДС2(Ставка)
    Значение = 20;
    Если Ставка = "Ставка18" Тогда
        Возврат Значение;
    КонецЕсли;
    Возврат Значение;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 0, "Returning variable should not trigger (not primitive)");
    }

    #[test]
    fn test_different_primitives_no_diagnostic() {
        let code = r#"
Функция Проверка(Условие)
    Если Условие Тогда
        Возврат Истина;
    Иначе
        Возврат Ложь;
    КонецЕсли;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 0, "Different primitive values should not trigger");
    }

    #[test]
    fn test_same_boolean_triggers() {
        let code = r#"
Функция ПроверитьСтроку(СтрокаТаблицы)
    Если Условие1 Тогда
        Возврат Истина;
    ИначеЕсли Условие2 Тогда
        Возврат Истина;
    Иначе
        Возврат Истина;
    КонецЕсли;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 1, "Same boolean should trigger");
    }

    #[test]
    fn test_same_number_triggers() {
        let code = r#"
Функция СтавкаНДС(Ставка)
    Если Ставка = "Ставка18" Тогда
        Возврат 20;
    КонецЕсли;
    Возврат 20;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 1, "Same number should trigger");
    }

    #[test]
    fn test_same_string_triggers() {
        let code = r#"
Функция Метод1()
    Если Фича = "Дирижабль" Тогда
        Возврат "Фича";
    ИначеЕсли Фича = "Ага" Тогда
        Возврат "Фича";
    КонецЕсли;
    Возврат "Фича";
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 1, "Same string should trigger");
    }

    #[test]
    fn test_null_case_insensitive() {
        let code = r#"
Функция КакаяТоКоманда(Команда)
    Если ЗначениеЗаполнено(ТекущаяДата) Тогда
        Возврат Null;
    КонецЕсли;
    Возврат NULL;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(
            func_diags.len(),
            1,
            "Null and NULL should be treated as same (case-insensitive)"
        );
    }

    #[test]
    fn test_attachable_skipped() {
        let code = r#"
Функция Подключаемый_КакаяТоКоманда(Команда)
    Если ЗначениеЗаполнено(ТекущаяДата) Тогда
        Возврат Null;
    КонецЕсли;
    Возврат NULL;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 0, "Attachable methods should be skipped by default");
    }

    #[test]
    fn test_attachable_english_skipped() {
        let code = r#"
Function Attachable_RandomAction(Command)
    If ValueIsFilled(CurrentDate) Then
        Return Undefined;
    EndIf;
    Return Undefined;
EndFunction
"#;
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();
        assert_eq!(func_diags.len(), 0, "Attachable_ (English) methods should be skipped");
    }
}
