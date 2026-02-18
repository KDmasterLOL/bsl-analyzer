//! FunctionReturnsSamePrimitive diagnostic
//!
//! Detects functions that always return the same primitive value in all branches.
//!
//! **Source (Java):** bsl-language-server/FunctionReturnsSamePrimitiveDiagnostic.java
//! **Source (Rust tree-sitter):** bsl-language-server-rust/rules/function_returns_same_primitive.rs
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

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

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
    #[test]
    fn test_fixture() {
        let code = include_str!("../../test_data/FunctionReturnsSamePrimitiveDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let func_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionReturnsSamePrimitive)
            .collect();

        // With default parameters (skipAttachable=true, caseSensitiveForString=false)
        // Java expects 5 diagnostics at:
        // - Line 0 (ПроверитьСтроку), cols 8-23
        // - Line 25 (Метод1), cols 8-14
        // - Line 35 (СтавкаНДС), cols 8-17
        // - Line 62 (КакаяТоКоманда), cols 8-22
        // - Line 82 (ПроверкаРегистраДляСтрок), cols 8-32

        assert_eq!(func_diags.len(), 5, "Expected 5 diagnostics with default config");

        // Java test uses 0-based line numbers
        // Our fixture is identical to Java (no extra comments at start)

        // ПроверитьСтроку - line 0
        assert_diagnostic_range(code, func_diags[0], 0, 8, 23);

        // Метод1 - line 25
        assert_diagnostic_range(code, func_diags[1], 25, 8, 14);

        // СтавкаНДС - line 35
        assert_diagnostic_range(code, func_diags[2], 35, 8, 17);

        // КакаяТоКоманда - line 62
        assert_diagnostic_range(code, func_diags[3], 62, 8, 22);

        // ПроверкаРегистраДляСтрок - line 82
        assert_diagnostic_range(code, func_diags[4], 82, 8, 32);
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
