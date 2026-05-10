//! Reports `ЗафиксироватьТранзакцию` / `CommitTransaction` calls in invalid positions.

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
        DiagnosticCode::CommitTransactionOutsideTryCatch,
        "Вызов 'ЗафиксироватьТранзакцию'/'CommitTransaction' должен быть размещен в блоке 'Попытка' с обработчиком 'Исключение'",
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
    fn test_valid_inside_try() {
        let code = r#"Процедура Пример1()
    НачатьТранзакцию();
    Попытка
        БлокировкаДанных = Новый БлокировкаДанных;
        ДокументОбъект.Записать();
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::CommitTransactionOutsideTryCatch)
            .collect();
        assert_eq!(diags.len(), 0, "CommitTransaction properly protected should be valid");
    }

    #[test]
    fn test_outside_try() {
        let code = r#"Процедура Пример2()
    НачатьТранзакцию();
    Метод();
    ЗафиксироватьТранзакцию();
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::CommitTransactionOutsideTryCatch)
            .collect();
        assert_eq!(diags.len(), 1, "CommitTransaction outside try should be error");
        assert_diagnostic_range(code, diags[0], 3, 4, 30);
    }

    #[test]
    fn test_in_exception_handler() {
        let code = r#"Процедура Пример3()
    НачатьТранзакцию();
    Попытка
        Метод();
    Исключение
        Если ТранзакцияАктивна() Тогда
            ЗафиксироватьТранзакцию();
        Иначе
            ОтменитьТранзакцию();
        КонецЕсли;
    КонецПопытки;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::CommitTransactionOutsideTryCatch)
            .collect();
        assert_eq!(diags.len(), 1, "CommitTransaction in except handler should be error");
        assert_diagnostic_range(code, diags[0], 6, 12, 38);
    }

    #[test]
    fn test_code_after_commit() {
        let code = r#"Процедура Пример6()
    НачатьТранзакцию();
    Попытка
        Метод();
        ЗафиксироватьТранзакцию();
        Метод2();
    Исключение
        ОтменитьТранзакцию();
        Возврат;
    КонецПопытки;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::CommitTransactionOutsideTryCatch)
            .collect();
        assert_eq!(diags.len(), 1, "Code after CommitTransaction should be error");
        assert_diagnostic_range(code, diags[0], 4, 8, 34);
    }

    #[test]
    fn test_qualified_call_ignored() {
        let code = r#"Процедура Тест()
    Коннектор.ЗафиксироватьТранзакцию();
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::CommitTransactionOutsideTryCatch)
            .collect();
        assert_eq!(diags.len(), 0, "Qualified call should be ignored");
    }

    #[test]
    fn test_english_keyword() {
        let code = r#"Procedure Test()
    BeginTransaction();
    Method();
    CommitTransaction();
EndProcedure"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::CommitTransactionOutsideTryCatch)
            .collect();
        assert_eq!(diags.len(), 1, "English CommitTransaction should be detected");
        assert_diagnostic_range(code, diags[0], 3, 4, 24);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"Процедура Тест()
    НачатьТранзакцию();
    Метод();
    ЗАФИКСИРОВАТЬТРАНЗАКЦИЮ();
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::CommitTransactionOutsideTryCatch)
            .collect();
        assert_eq!(diags.len(), 1, "Case-insensitive matching should work");
    }

    #[test]
    fn test_multiple_procedures_seven_errors() {
        // Seven error patterns in a single file (module-level code not supported)
        let code = r#"Процедура Пример1()
    НачатьТранзакцию();
    Попытка
        БлокировкаДанных = Новый БлокировкаДанных;
        ДокументОбъект.Записать();
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры

Процедура Пример2()
    НачатьТранзакцию();
    Попытка
        Метод();
    Исключение
        ОтменитьТранзакцию();
        Возврат;
    КонецПопытки;
    ЗафиксироватьТранзакцию();
КонецПроцедуры

Процедура Пример3()
    НачатьТранзакцию();
    Попытка
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
    НачатьТранзакцию();
    Метод();
    Если ТранзакцияАктивна() Тогда
        ЗафиксироватьТранзакцию();
    Иначе
        ОтменитьТранзакцию();
    КонецЕсли;
КонецПроцедуры

Функция Пример5()
    НачатьТранзакцию();
    Метод();
    ЗафиксироватьТранзакцию();
    Возврат 1;
КонецФункции

Процедура Пример6()
    НачатьТранзакцию();
    Попытка
        Метод();
        ЗафиксироватьТранзакцию();
        Метод2();
    Исключение
        ОтменитьТранзакцию();
        Возврат;
    КонецПопытки;
КонецПроцедуры

Процедура Пример7()
    НачатьТранзакцию();
    Попытка
        Метод();
        ЗафиксироватьТранзакцию();
        Возврат;
    Исключение
        ОтменитьТранзакцию();
        Возврат;
    КонецПопытки;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::CommitTransactionOutsideTryCatch)
            .collect();

        // Пример1 is correct (no error), Пример2-7 each have 1 error = 6 errors
        assert_eq!(diags.len(), 6, "Should detect 6 diagnostics");
    }

    #[test]
    fn test_commit_in_loop_after_code() {
        // CommitTransaction with code after it inside a loop
        let code = r#"Процедура Тест()
    Для каждого Элемент Из Коллекция Цикл
        НачатьТранзакцию();
        Попытка
            Метод();
            ЗафиксироватьТранзакцию();
            Продолжить;
        Исключение
            ОтменитьТранзакцию();
            Возврат;
        КонецПопытки;
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::CommitTransactionOutsideTryCatch)
            .collect();
        assert_eq!(diags.len(), 1, "CommitTransaction with code after in loop should be error");
    }

    #[test]
    fn test_single_sub_outside_try() {
        // Single procedure with CommitTransaction outside try-catch
        let code = r#"Процедура Тестовая()
    НачатьТранзакцию();
    А = 1;
    ЗафиксироватьТранзакцию();
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::CommitTransactionOutsideTryCatch)
            .collect();

        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 3, 4, 30);
    }

    #[test]
    fn test_nested_if_commit_in_try_snapshot() {
        // Track 3 Phase C §4.2: documents the current gap. The
        // direct-statement check does not descend into IF_STMT inside
        // the try body, so this nested CommitTransaction is not emitted
        // even though code can continue after the if.
        check_diagnostics_snapshot_for(
            r#"Процедура Тест(Условие)
    НачатьТранзакцию();
    Попытка
        Если Условие Тогда
            ЗафиксироватьТранзакцию();
        КонецЕсли;
        ДействиеПослеФиксации();
    Исключение
        ОтменитьТранзакцию();
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры"#,
            DiagnosticCode::CommitTransactionOutsideTryCatch,
            expect![[r#""#]],
        );
    }
}
