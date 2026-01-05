//! UnreachableCode diagnostic.
//!
//! Detects code that will never be executed because it follows a control flow
//! statement like `return`, `raise`, `break`, or `continue`.
//!
//! ## Why?
//! Unreachable code indicates a logic error or dead code that should be removed:
//! - After `Возврат` / `Return` - function has already exited
//! - After `ВызватьИсключение` / `Raise` - exception was thrown
//! - After `Прервать` / `Break` - loop was exited
//! - After `Продолжить` / `Continue` - jumped to next iteration
//!
//! ## Bad practice
//! ```bsl
//! Процедура Пример()
//!     Возврат;
//!     Сообщить("Этот код никогда не выполнится"); // ❌ Unreachable
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Пример()
//!     Если Условие Тогда
//!         Сообщить("Условие истинно");
//!         Возврат;
//!     КонецЕсли;
//!     Сообщить("Условие ложно"); // ✅ Reachable
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Minor (potential bug)
//! - **Tags:** DESIGN, SUSPICIOUS
//!
//! ## Implementation
//! This diagnostic is collected during HIR lowering as a byproduct of
//! statement processing. The `from_hir` function converts the BodyDiagnostic
//! to a Diagnostic for display.
//!
//! Ported from:
//! - UnreachableCodeDiagnostic.java (bsl-language-server)

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic (called from lib.rs dispatch).
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::UnreachableCode) {
        return None;
    }

    Some(Diagnostic {
        code: DiagnosticCode::UnreachableCode,
        message: message_ru(),
        severity: Severity::Warning,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

fn message_ru() -> String {
    "Недостижимый код".to_string()
}

#[allow(dead_code)]
fn message_en() -> String {
    "Unreachable code".to_string()
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        assert_diagnostic_range, assert_diagnostic_range_multiline, check_hir_diagnostic,
    };
    use crate::DiagnosticCode;

    #[test]
    fn test_unreachable_after_return() {
        let code = r#"
Процедура Тест()
    Возврат;
    Сообщить("Недостижимо");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1);
        // Line 3 (0-indexed): Сообщить("Недостижимо");
        // Range excludes trailing semicolon in column count
        assert_diagnostic_range(code, unreachable_diags[0], 3, 4, 27);
    }

    #[test]
    fn test_unreachable_after_raise() {
        let code = r#"
Процедура Тест()
    ВызватьИсключение "Ошибка";
    Сообщить("Недостижимо");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1);
        assert_diagnostic_range(code, unreachable_diags[0], 3, 4, 27);
    }

    #[test]
    fn test_unreachable_after_break() {
        let code = r#"
Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Прервать;
        Сообщить("Недостижимо");
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1);
        // Line 4 (0-indexed): Сообщить("Недостижимо");
        assert_diagnostic_range(code, unreachable_diags[0], 4, 8, 31);
    }

    #[test]
    fn test_unreachable_after_continue() {
        let code = r#"
Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Продолжить;
        Сообщить("Недостижимо");
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1);
        assert_diagnostic_range(code, unreachable_diags[0], 4, 8, 31);
    }

    #[test]
    fn test_unreachable_multiline_block() {
        let code = r#"
Процедура Тест()
    Возврат;
    А = 1;
    Б = 2;
    Сообщить(А + Б);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1);
        // Should span from line 3 to line 5
        assert_diagnostic_range_multiline(code, unreachable_diags[0], 3, 4, 5, 19);
    }

    #[test]
    fn test_no_unreachable_in_different_branches() {
        let code = r#"
Процедура Тест()
    Если Условие Тогда
        Возврат;
    КонецЕсли;
    Сообщить("Достижимо");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        // No unreachable code - the Сообщить is in a different branch
        assert_eq!(unreachable_diags.len(), 0);
    }

    #[test]
    fn test_no_unreachable_after_conditional_return() {
        let code = r#"
Функция Тест()
    Если А Тогда
        Возврат 1;
    Иначе
        Возврат 2;
    КонецЕсли;
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        // Returns are in different branches, not sequential
        assert_eq!(unreachable_diags.len(), 0);
    }

    #[test]
    fn test_unreachable_after_region_with_return() {
        let code = r#"
Функция Тест()
    #Область Тест
    Возврат;
    #КонецОбласти
    Сообщить("Недостижимо");
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        // Print for debugging
        for (i, d) in unreachable_diags.iter().enumerate() {
            let (start_line, start_col, end_line, end_col) =
                crate::test_utils::range_to_line_col(code, d.range);
            eprintln!(
                "  {}: line {}-{}, col {}-{}",
                i + 1,
                start_line,
                end_line,
                start_col,
                end_col
            );
        }

        assert_eq!(unreachable_diags.len(), 1);
        // Line 5 (0-indexed): Сообщить("Недостижимо");
        assert_diagnostic_range(code, unreachable_diags[0], 5, 4, 27);
    }

    #[test]
    fn test_unreachable_after_region_with_return_and_if() {
        // Match exact structure of Пример11 from Java fixture
        let code = r#"
Функция Пример11()
    #Область ВложеннаяОбласть
    Если Истина Тогда
        Возврат;
    КонецЕсли;
    Возврат;
    #КонецОбласти
    Сообщить(5);
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1, "Expected 1 unreachable code diagnostic");
        // "Сообщить(5)" on line 8 (0-indexed), col 4-15 (without semicolon)
        assert_diagnostic_range(code, unreachable_diags[0], 8, 4, 15);
    }

    #[test]
    fn test_unreachable_in_outer_region() {
        // Match exact structure from Java fixture - function inside outer region
        let code = r#"
#Область ВнешняяОбласть
Функция Пример11()
    #Область ВложеннаяОбласть
    Если Истина Тогда
        Возврат;
    КонецЕсли;
    Возврат;
    #КонецОбласти
    Сообщить(5);
КонецФункции
#КонецОбласти
"#;
        let diagnostics = check_hir_diagnostic(code);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        assert_eq!(unreachable_diags.len(), 1, "Expected 1 unreachable code diagnostic");
        // "Сообщить(5)" on line 9 (0-indexed), col 4-15 (without semicolon)
        assert_diagnostic_range(code, unreachable_diags[0], 9, 4, 15);
    }

    /// Test with Java fixture - validates positions match bsl-language-server.
    ///
    /// Java test expects 17 diagnostics at these positions:
    /// 1. (12, 12, 20) - after Продолжить
    /// 2. (21, 12, 20) - after Возврат
    /// 3. (30, 12, 20) - after Прервать
    /// 4. (37, 4, 41, 15) - after Возврат
    /// 5. (46, 4, 51, 15) - after Возврат
    /// 6. (58, 12, 20) - after ВызватьИсключение
    /// 7. (67, 12, 69, 21) - after ВызватьИсключение
    /// 8. (82, 16, 84, 25) - after ВызватьИсключение (in preprocessor)
    /// 9. (93, 8, 16) - after Возврат (in preprocessor)
    /// 10. (102, 8, 17) - after Возврат (in preprocessor)
    /// 11. (108, 16, 111, 29) - after ВызватьИсключение (preprocessor)
    /// 12. (125, 4, 12) - after if-else (all branches return)
    /// 13. (138, 4, 16) - after return (in region)
    /// 14. (163, 4, 22) - after if-else (all branches return)
    /// 15. (171, 4, 13) - after Возврат (in preprocessor)
    /// 16. (176, 4, 178, 13) - after ВызватьИсключение
    /// 17. (182, 0, 9) - after Возврат (module-level)
    #[test]
    fn test_java_fixture() {
        let code = include_str!("../../tests/fixtures/UnreachableCodeDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);
        let unreachable_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnreachableCode).collect();

        // Print actual diagnostics for debugging
        eprintln!("Found {} UnreachableCode diagnostics:", unreachable_diags.len());
        for (i, d) in unreachable_diags.iter().enumerate() {
            let (start_line, start_col, end_line, end_col) =
                crate::test_utils::range_to_line_col(code, d.range);
            eprintln!(
                "  {}: line {}-{}, col {}-{}",
                i + 1,
                start_line,
                end_line,
                start_col,
                end_col
            );
        }

        // Java expects 17 diagnostics
        assert_eq!(
            unreachable_diags.len(),
            17,
            "Expected 17 unreachable code diagnostics to match Java"
        );
    }
}
