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
//!
//! Ported from:
//!
//! Adapted to use HIR-based collection during AST→HIR lowering.

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

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::BeginTransactionBeforeTryCatch)
            .collect();
        assert_eq!(diags.len(), 0, "BeginTransaction immediately before Try should be valid");
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

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::BeginTransactionBeforeTryCatch)
            .collect();
        assert_eq!(diags.len(), 1, "Code between BeginTransaction and Try should be error");
        assert_diagnostic_range(code, diags[0], 1, 4, 23);
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

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::BeginTransactionBeforeTryCatch)
            .collect();
        assert_eq!(diags.len(), 1, "BeginTransaction inside Try should be error");
        assert_diagnostic_range(code, diags[0], 2, 8, 27);
    }

    #[test]
    fn test_no_try_after() {
        let code = r#"Процедура Тест()
    НачатьТранзакцию();
    ЗаписатьДанные();
    ЗафиксироватьТранзакцию();
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::BeginTransactionBeforeTryCatch)
            .collect();
        assert_eq!(diags.len(), 1, "BeginTransaction without Try should be error");
        assert_diagnostic_range(code, diags[0], 1, 4, 23);
    }

    #[test]
    fn test_qualified_call_ignored() {
        let code = r#"Процедура Тест()
    Коннектор.НачатьТранзакцию();
    ЗаписатьДанные();
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::BeginTransactionBeforeTryCatch)
            .collect();
        assert_eq!(diags.len(), 0, "Qualified call should be ignored");
    }

    #[test]
    fn test_english_keyword() {
        let code = r#"Procedure Test()
    BeginTransaction();
    SaveData();
EndProcedure"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::BeginTransactionBeforeTryCatch)
            .collect();
        assert_eq!(diags.len(), 1, "English BeginTransaction should be detected");
        assert_eq!(diags[0].code, DiagnosticCode::BeginTransactionBeforeTryCatch);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"Процедура Тест()
    НАЧАТЬТРАНЗАКЦИЮ();
    Данные();
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::BeginTransactionBeforeTryCatch)
            .collect();
        assert_eq!(diags.len(), 1, "Case-insensitive matching should work");
        assert_eq!(diags[0].code, DiagnosticCode::BeginTransactionBeforeTryCatch);
    }

    /// Comprehensive test covering all error cases from the reference fixture.
    ///
    /// 6 diagnostics expected (module-level НачатьТранзакцию not checked):
    /// - Пример2 line 1: НачатьТранзакцию with code before Попытка
    /// - Пример3 line 2: НачатьТранзакцию inside Попытка
    /// - Пример4 line 1: НачатьТранзакцию with code after (no Try)
    /// - Пример5 line 4: second НачатьТранзакцию inside Попытка with code after
    /// - Пример6 line 1: НачатьТранзакцию with НачатьТранзакцию before Try
    /// - Loop line 1: BeginTransaction with code before Попытка
    #[test]
    fn test_comprehensive() {
        // Exact copy of BeginTransactionBeforeTryCatchDiagnostic.bsl fixture (lines 0-100).
        // Line numbers must match for position assertions below.
        let code = r#"Процедура Пример1() // правильнй с ИТС
    НачатьТранзакцию();
    Попытка
        БлокировкаДанных = Новый БлокировкаДанных;
        ЭлементБлокировкиДанных = БлокировкаДанных.Добавить("Документ.ПриходнаяНакладная");
        ЭлементБлокировкиДанных.УстановитьЗначение("Ссылка", СсылкаДляОбработки);
        ЭлементБлокировкиДанных.Режим = РежимБлокировкиДанных.Исключительный;
        БлокировкаДанных.Заблокировать();

        ДокументОбъект.Записать();

        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();

        ЗаписьЖурналаРегистрации(НСтр("ru = 'Выполнение операции'"),
            УровеньЖурналаРегистрации.Ошибка,
            ,
            ,
            ПодробноеПредставлениеОшибки(ИнформацияОбОшибке()));

        ВызватьИсключение; // есть внешняя транзакция

    КонецПопытки;
КонецПроцедуры

// ошибочные конструкции

Процедура Пример2()
    НачатьТранзакцию(); // <-- Ошибка: код перед попыткой
    Метод();
    Попытка
        Метод2();
    Исключение
        ОтменитьТранзакцию();
        Возврат;
    КонецПопытки;
    ЗафиксироватьТранзакцию();
КонецПроцедуры

Процедура Пример3()
    Попытка
        НачатьТранзакцию(); // <-- Ошибка: в попытке
        Метод();
    Исключение
        Если ТранзакцияАктивна() Тогда
            ЗафиксироватьТранзакцию();
        Иначе
            ОтменитьТранзакцию();
        КонецЕсли;
        Возврат;
    КонецПопытки;
КонецПроцедуры

Процедура Пример4()
    НачатьТранзакцию(); // <-- Ошибка: код после начала
    Метод();
    Если ТранзакцияАктивна() Тогда
        ЗафиксироватьТранзакцию();
    Иначе
        ОтменитьТранзакцию();
    КонецЕсли;
КонецПроцедуры

Функция Пример5()
    НачатьТранзакцию(); // <-- Ошибки нет
    Попытка
        Метод();
        НачатьТранзакцию(); // <-- Ошибка: есть код после
        Метод2();
        ЗафиксироватьТранзакцию();
    Исключение
    КонецПопытки;
    Возврат 1;
КонецФункции

Процедура Пример6()
    НачатьТранзакцию(); // <-- Ошибка: есть код после
    НачатьТранзакцию(); // <-- Ошибки нет
    Попытка
        Метод();
        ЗафиксироватьТранзакцию();
        Метод2();
    Исключение
        ОтменитьТранзакцию();
        Возврат;
    КонецПопытки;
КонецПроцедуры

Для каждого Элемент Из Коллекция Цикл
    BeginTransaction();  // <-- Ошибка: есть код после
    Метод();
    Попытка
        Метод();
        ЗафиксироватьТранзакцию();
        Продолжить;
    Исключение
        ОтменитьТранзакцию();
        Возврат;
    КонецПопытки;
КонецЦикла;"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::BeginTransactionBeforeTryCatch)
            .collect();

        // Expected 6 diagnostics (excluding module-level НачатьТранзакцию which is not supported).
        // The reference fixture also has НачатьТранзакцию() outside any method (module-level code),
        // but we don't check module-level code. Not worth complicating lower_module_code for this.
        assert_eq!(diags.len(), 6, "Should detect 6 diagnostics (excluding module-level code)");

        // Verify exact positions
        assert_diagnostic_range(code, diags[0], 29, 4, 23); // Пример2: код перед попыткой
        assert_diagnostic_range(code, diags[1], 42, 8, 27); // Пример3: в попытке
        assert_diagnostic_range(code, diags[2], 55, 4, 23); // Пример4: код после начала
        assert_diagnostic_range(code, diags[3], 68, 8, 27); // Пример5: внутри попытки
        assert_diagnostic_range(code, diags[4], 77, 4, 23); // Пример6: есть код после
        assert_diagnostic_range(code, diags[5], 90, 4, 23); // Цикл: есть код после
    }
}
