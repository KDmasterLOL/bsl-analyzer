//! BeginTransactionBeforeTryCatch diagnostic.
//!
//! Checks that `BeginTransaction()`/`НачатьТранзакцию()` calls are immediately followed by `Try-Catch` blocks.
//!
//! ## Why?
//! Starting a transaction without proper error handling is dangerous:
//! - Uncommitted transactions can lock database
//! - Data corruption if transaction is not rolled back on error
//! - Resource leaks
//! - Must ensure transaction is always finalized (commit or rollback)
//!
//! ## Bad practice
//! ```bsl
//! Процедура Тест()
//!     НачатьТранзакцию();
//!     // If error occurs here, transaction is left open!
//!     ЗаписатьДанные();
//!     ЗафиксироватьТранзакцию();
//! КонецПроцедуры
//!
//! Процедура Тест2()
//!     НачатьТранзакцию();
//!     Метод(); // ← Code between BeginTransaction and Try
//!     Попытка
//!         ЗаписатьДанные();
//!         ЗафиксироватьТранзакцию();
//!     Исключение
//!         ОтменитьТранзакцию();
//!     КонецПопытки;
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Тест()
//!     НачатьТранзакцию();
//!     Попытка
//!         ЗаписатьДанные();
//!         ЗафиксироватьТранзакцию();
//!     Исключение
//!         ОтменитьТранзакцию();
//!         ВызватьИсключение;
//!     КонецПопытки;
//! КонецПроцедуры
//! ```
//!
//! ## Implementation
//!
//! This diagnostic is collected during HIR lowering as a byproduct of statement processing.
//! The `from_hir` function converts the BodyDiagnostic to a Diagnostic for display.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

/// Creates diagnostic from HIR BodyDiagnostic (called from lib.rs dispatch).
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::BeginTransactionBeforeTryCatch,
        "Метод 'НачатьТранзакцию' должен быть за пределами блока 'Попытка-Исключение' непосредственно перед оператором 'Попытка'",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_valid_before_try() {
        let code = r#"Процедура Тест()
    НачатьТранзакцию();
    Попытка
        ЗаписатьДанные();
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::BeginTransactionBeforeTryCatch,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_code_between() {
        let code = r#"Процедура Тест()
    НачатьТранзакцию();
    Метод();
    Попытка
        ЗаписатьДанные();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::BeginTransactionBeforeTryCatch,
            expect![[r#"
                BeginTransactionBeforeTryCatch @ 2:5..2:24
                  message: Метод 'НачатьТранзакцию' должен быть за пределами блока 'Попытка-Исключение' непосредственно перед оператором 'Попытка'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_inside_try() {
        let code = r#"Процедура Тест()
    Попытка
        НачатьТранзакцию();
        ЗаписатьДанные();
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::BeginTransactionBeforeTryCatch,
            expect![[r#"
                BeginTransactionBeforeTryCatch @ 3:9..3:28
                  message: Метод 'НачатьТранзакцию' должен быть за пределами блока 'Попытка-Исключение' непосредственно перед оператором 'Попытка'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_no_try_after() {
        let code = r#"Процедура Тест()
    НачатьТранзакцию();
    ЗаписатьДанные();
    ЗафиксироватьТранзакцию();
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::BeginTransactionBeforeTryCatch,
            expect![[r#"
                BeginTransactionBeforeTryCatch @ 2:5..2:24
                  message: Метод 'НачатьТранзакцию' должен быть за пределами блока 'Попытка-Исключение' непосредственно перед оператором 'Попытка'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_qualified_call_ignored() {
        let code = r#"Процедура Тест()
    Коннектор.НачатьТранзакцию();
    ЗаписатьДанные();
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::BeginTransactionBeforeTryCatch,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_english_keyword() {
        let code = r#"Procedure Test()
    BeginTransaction();
    SaveData();
EndProcedure"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::BeginTransactionBeforeTryCatch,
            expect![[r#"
                BeginTransactionBeforeTryCatch @ 2:5..2:24
                  message: Метод 'НачатьТранзакцию' должен быть за пределами блока 'Попытка-Исключение' непосредственно перед оператором 'Попытка'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"Процедура Тест()
    НАЧАТЬТРАНЗАКЦИЮ();
    Данные();
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::BeginTransactionBeforeTryCatch,
            expect![[r#"
                BeginTransactionBeforeTryCatch @ 2:5..2:24
                  message: Метод 'НачатьТранзакцию' должен быть за пределами блока 'Попытка-Исключение' непосредственно перед оператором 'Попытка'
                  severity: Major"#]],
        );
    }

    /// Track 2 Phase D §2.3 mini-fix: preprocessor-aware
    /// recognition of `НачатьТранзакцию(); #Если ... Попытка ... #Иначе
    /// Попытка ... #КонецЕсли`. The fixture is BSL-safe — every active
    /// preprocessor branch starts with `Попытка` immediately after the
    /// shared outer `НачатьТранзакцию()`, so the runtime semantics are
    /// always `Begin; Try`. The false positive was closed by Track 6.3
    /// with per-branch preprocessor source-of-truth. The single-branch
    /// form (`#Если ... Try ... #КонецЕсли` without `#Иначе`) is
    /// intentionally NOT used here:
    /// on the inactive branch there would be `Begin; КонецПроцедуры`
    /// without a `Try`, which is a genuine violation the diagnostic is
    /// meant to flag.
    #[test]
    fn begin_in_preproc_then_try_outside() {
        let code = r#"Процедура Тест()
    НачатьТранзакцию();
    #Если Сервер Тогда
    Попытка
        ЗаписатьДанные();
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
        ВызватьИсключение;
    КонецПопытки;
    #Иначе
    Попытка
        ЗаписатьДанныеКлиента();
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
        ВызватьИсключение;
    КонецПопытки;
    #КонецЕсли
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::BeginTransactionBeforeTryCatch)
            .collect();
        assert_eq!(
            diags.len(),
            0,
            "BeginTransaction immediately followed by a preprocessor block \
             where every active branch starts with `Попытка` should be valid \
             after the preproc-aware path lands."
        );
    }

    #[test]
    fn begin_in_preproc_asymmetric_then_try_else_missing() {
        let code = r#"Процедура Тест()
    НачатьТранзакцию();
    #Если Сервер Тогда
        Попытка
            ЗаписатьДанные();
            ЗафиксироватьТранзакцию();
        Исключение
            ОтменитьТранзакцию();
            ВызватьИсключение;
        КонецПопытки;
    #Иначе
        ЗаписатьБезТранзакции();
    #КонецЕсли
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::BeginTransactionBeforeTryCatch,
            expect![[r#"
                BeginTransactionBeforeTryCatch @ 2:5..2:24
                  message: Метод 'НачатьТранзакцию' должен быть за пределами блока 'Попытка-Исключение' непосредственно перед оператором 'Попытка'
                  severity: Major"#]],
        );
    }

    #[test]
    fn begin_inside_preproc_unchanged_behavior() {
        let code = r#"Процедура Тест()
    #Если Сервер Тогда
        НачатьТранзакцию();
        Попытка
            ЗаписатьДанные();
        Исключение
            ОтменитьТранзакцию();
        КонецПопытки;
    #КонецЕсли
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::BeginTransactionBeforeTryCatch,
            expect![[r#""#]],
        );
    }

    /// Local integration fixture with several independent violations in one file.
    #[test]
    fn test_multiple_violations_in_one_module() {
        let code = r#"Процедура ПровестиДокумент()
    НачатьТранзакцию();
    ПодготовитьДанные();
    Попытка
        ЗаписатьДвижения();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры

Функция ОбновитьОстатки()
    Попытка
        НачатьТранзакцию();
        ПересчитатьОстатки();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
    Возврат Истина;
КонецФункции

Процедура СинхронизироватьСправочник()
    НачатьТранзакцию();
    ОбновитьКэш();
    ЗафиксироватьТранзакцию();
КонецПроцедуры

Для каждого СтрокаТовара Из ТаблицаТоваров Цикл
    НачатьТранзакцию();
    ЛогироватьСтроку(СтрокаТовара);
    Попытка
        ОбновитьСтроку(СтрокаТовара);
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецЦикла;"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::BeginTransactionBeforeTryCatch,
            expect![[r#"
                BeginTransactionBeforeTryCatch @ 2:5..2:24
                  message: Метод 'НачатьТранзакцию' должен быть за пределами блока 'Попытка-Исключение' непосредственно перед оператором 'Попытка'
                  severity: Major
                BeginTransactionBeforeTryCatch @ 13:9..13:28
                  message: Метод 'НачатьТранзакцию' должен быть за пределами блока 'Попытка-Исключение' непосредственно перед оператором 'Попытка'
                  severity: Major
                BeginTransactionBeforeTryCatch @ 22:5..22:24
                  message: Метод 'НачатьТранзакцию' должен быть за пределами блока 'Попытка-Исключение' непосредственно перед оператором 'Попытка'
                  severity: Major
                BeginTransactionBeforeTryCatch @ 28:5..28:24
                  message: Метод 'НачатьТранзакцию' должен быть за пределами блока 'Попытка-Исключение' непосредственно перед оператором 'Попытка'
                  severity: Major"#]],
        );
    }
}
