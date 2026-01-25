use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::WrongUseOfRollbackTransactionMethod;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: message_ru(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

fn message_ru() -> String {
    "Вызов 'ОтменитьТранзакцию'/'RollbackTransaction' должен находиться в блоке обработки исключений первым оператором".to_string()
}

#[allow(dead_code)]
fn message_en() -> String {
    "Call to 'RollbackTransaction' must be the first statement in the exception handler".to_string()
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;

    #[test]
    fn test_valid_first_in_except() {
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
            .filter(|d| d.code == DiagnosticCode::WrongUseOfRollbackTransactionMethod)
            .collect();
        assert_eq!(diags.len(), 0, "RollbackTransaction first in except should be valid");
    }

    #[test]
    fn test_not_first_in_except() {
        let code = r#"Процедура Тест()
    НачатьТранзакцию();
    Попытка
        ЗафиксироватьТранзакцию();
    Исключение
        Сообщить("Ошибка");
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WrongUseOfRollbackTransactionMethod)
            .collect();
        assert_eq!(diags.len(), 1, "RollbackTransaction not first in except should be error");
        assert_diagnostic_range(code, diags[0], 6, 8, 29);
    }

    #[test]
    fn test_outside_try_catch() {
        let code = r#"Процедура Тест()
    НачатьТранзакцию();
    ОтменитьТранзакцию();
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WrongUseOfRollbackTransactionMethod)
            .collect();
        assert_eq!(diags.len(), 1, "RollbackTransaction outside try-catch should be error");
        assert_diagnostic_range(code, diags[0], 2, 4, 25);
    }

    #[test]
    fn test_in_try_body() {
        let code = r#"Процедура Тест()
    НачатьТранзакцию();
    Попытка
        ОтменитьТранзакцию();
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WrongUseOfRollbackTransactionMethod)
            .collect();
        assert_eq!(diags.len(), 1, "RollbackTransaction in try body should be error");
        assert_diagnostic_range(code, diags[0], 3, 8, 29);
    }

    #[test]
    fn test_qualified_call_ignored() {
        let code = r#"Процедура Тест()
    Коннектор.ОтменитьТранзакцию();
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WrongUseOfRollbackTransactionMethod)
            .collect();
        assert_eq!(diags.len(), 0, "Qualified call should be ignored");
    }

    #[test]
    fn test_english_keyword() {
        let code = r#"Procedure Test()
    BeginTransaction();
    RollbackTransaction();
EndProcedure"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WrongUseOfRollbackTransactionMethod)
            .collect();
        assert_eq!(diags.len(), 1, "English RollbackTransaction should be detected");
        assert_diagnostic_range(code, diags[0], 2, 4, 26);
    }

    #[test]
    fn test_comprehensive() {
        let code =
            include_str!("../../test_data/WrongUseOfRollbackTransactionMethodDiagnostic.bsl");

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WrongUseOfRollbackTransactionMethod)
            .collect();

        assert_eq!(diags.len(), 3);
        assert_diagnostic_range(code, diags[0], 7, 8, 29);
        assert_diagnostic_range(code, diags[1], 11, 4, 25);
        assert_diagnostic_range(code, diags[2], 29, 4, 26);
    }
}
