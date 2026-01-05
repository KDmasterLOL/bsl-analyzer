//! Diagnostic: UnusedLocalVariable
//!
//! Detects local variables that are declared but never used.
//!
//! ## Severity
//! Info (with Unnecessary tag)
//!
//! ## Example
//! ```bsl
//! // Bad - unused variable
//! Процедура Тест()
//!     Перем НеИспользуется;  // Warning: unused
//!     Сообщить("Привет");
//! КонецПроцедуры
//!
//! // Good - variable is used
//! Процедура Тест()
//!     Перем Сообщение;
//!     Сообщение = "Привет";
//!     Сообщить(Сообщение);
//! КонецПроцедуры
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticTag, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::UnusedVariable` is encountered.
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::UnusedLocalVariable) {
        return None;
    }
    Some(Diagnostic {
        code: DiagnosticCode::UnusedLocalVariable,
        message: format!("Переменная \"{}\" объявлена, но не используется", name),
        severity: Severity::Information,
        range,
        tags: vec![DiagnosticTag::Unnecessary],
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;

    #[test]
    fn test_unused_var_in_procedure() {
        let code = r#"Процедура Тест()
    Перем НеИспользуется;
    Сообщить("Привет");
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(unused_diags.len(), 1, "Expected 1 UnusedLocalVariable diagnostic");
        // Check position: variable name "НеИспользуется" on line 1
        assert_diagnostic_range(code, unused_diags[0], 1, 10, 24);
    }

    #[test]
    fn test_used_var_no_diagnostic() {
        let code = r#"Процедура Тест()
    Перем Сообщение;
    Сообщение = "Привет";
    Сообщить(Сообщение);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Used variable should not trigger diagnostic"
        );
    }

    #[test]
    fn test_unused_loop_variable() {
        let code = r#"Процедура Тест()
    Для Индекс = 1 По 10 Цикл
        Сообщить("Итерация");
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(unused_diags.len(), 1, "Unused loop variable should trigger diagnostic");
        // "Индекс" on line 1, col 8-14 (after "    Для ")
        assert_diagnostic_range(code, unused_diags[0], 1, 8, 14);
    }

    #[test]
    fn test_used_loop_variable() {
        let code = r#"Процедура Тест()
    Для Индекс = 1 По 10 Цикл
        Сообщить(Индекс);
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Used loop variable should not trigger diagnostic"
        );
    }

    #[test]
    fn test_unused_foreach_variable() {
        let code = r#"Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Сообщить("Итерация");
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(unused_diags.len(), 1, "Unused foreach variable should trigger diagnostic");
        // "Элемент" on line 1, col 16-23 (after "    Для Каждого ")
        assert_diagnostic_range(code, unused_diags[0], 1, 16, 23);
    }

    #[test]
    fn test_multiple_unused_vars() {
        let code = r#"Процедура Тест()
    Перем А, Б, В;
    Сообщить(Б);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        // А and В are unused, Б is used
        assert_eq!(unused_diags.len(), 2, "Expected 2 unused variables (А and В)");
        // Check positions: "А" at col 10-11, "В" at col 16-17 on line 1
        assert!(
            unused_diags.iter().any(|d| d.message.contains("А")),
            "Should detect unused variable А"
        );
        assert!(
            unused_diags.iter().any(|d| d.message.contains("В")),
            "Should detect unused variable В"
        );
    }

    #[test]
    fn test_case_insensitive_usage() {
        let code = r#"Процедура Тест()
    Перем Переменная;
    ПЕРЕМЕННАЯ = 10;
    Сообщить(переменная);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Case-insensitive usage should count as used"
        );
    }

    #[test]
    fn test_assigned_but_never_read() {
        // Variable is assigned but its value is never read - should trigger diagnostic
        let code = r#"Процедура Тест()
    Перем ТолькоПрисвоение;
    ТолькоПрисвоение = 10;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(
            unused_diags.len(),
            1,
            "Variable assigned but never read should trigger diagnostic"
        );
        // "ТолькоПрисвоение" on line 1, col 10-26 (after "    Перем ")
        assert_diagnostic_range(code, unused_diags[0], 1, 10, 26);
    }

    #[test]
    fn test_assigned_and_read() {
        // Variable is assigned AND read - should NOT trigger
        let code = r#"Процедура Тест()
    Перем Значение;
    Значение = 10;
    Сообщить(Значение);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Variable that is read should not trigger diagnostic"
        );
    }

    #[test]
    fn test_multiple_assignments_no_read() {
        // Multiple assignments but never read
        let code = r#"Процедура Тест()
    Перем Результат;
    Результат = ПервоеДействие();
    Результат = ВтороеДействие();
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(unused_diags.len(), 1, "Variable never read should trigger diagnostic");
        // "Результат" on line 1, col 10-19 (after "    Перем ")
        assert_diagnostic_range(code, unused_diags[0], 1, 10, 19);
    }

    #[test]
    fn test_field_assignment_base_is_read() {
        // When assigning to Obj.Field, Obj IS read (we access it)
        let code = r#"Процедура Тест()
    Перем Структура;
    Структура = Новый Структура;
    Структура.Поле = 10;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Variable used as base for field assignment should count as read"
        );
    }

    #[test]
    fn test_index_assignment_base_is_read() {
        // When assigning to Arr[i], Arr IS read
        let code = r#"Процедура Тест()
    Перем Массив;
    Массив = Новый Массив;
    Массив[0] = 10;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Variable used as base for index assignment should count as read"
        );
    }

    /// Test based on Java fixture content (UnusedLocalVariableDiagnostic.bsl)
    ///
    /// Java test expects 5 diagnostics total:
    /// - Line 1, col 6-36: module variable (NOT YET IMPLEMENTED - module-level)
    /// - Line 19, col 10-35: `ЛокальнаяБезИспользования` - declared but never used ✓
    /// - Line 19, col 37-63: `ТолькоСПрисвоениемЗначения` - assigned but never read ✓
    /// - Line 24, col 4-28: `ВПроцедуреНеИспользуемая` - assigned but never read ✓
    /// - Line 83, col 0-25: module-level code (NOT YET IMPLEMENTED - module-level)
    ///
    /// This test covers only the local variables we currently handle (3 of 5).
    #[test]
    fn test_fixture_local_variables_in_function() {
        // Excerpt from Java fixture: function Вторая()
        let code = r#"Функция Вторая()
    Перем ЛокальнаяБезИспользования, ТолькоСПрисвоениемЗначения, ЛокальнаяСИспользованием;

    ЛокальнаяСИспользованием = 40;
    ТолькоСПрисвоениемЗначения = ВыполнитьДействие(ЛокальнаяСИспользованием);
    ВПроцедуреИспользуемая = Проверка();
    ВПроцедуреНеИспользуемая = Проверка();

    Если ВПроцедуреИспользуемая = Истина Тогда

       ТолькоСПрисвоениемЗначения = 39;

    КонецЕсли;

    ПеременнаяОбъектСИспользованием = Обработки.Проверка.Создать();
    ПеременнаяОбъектСИспользованием.Выполнить();

    ВПроцедуреИспользуемая2 = Новый Файл(ОбъединитьПути(".", "test_versions.mxl"));
    Ожидаем.Что(ВПроцедуреИспользуемая2.Существует(), "Файл отчета не был создан").ЭтоИстина();

КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        // Expected diagnostics for local variables within this function:
        // 1. ЛокальнаяБезИспользования - declared but never used
        // 2. ТолькоСПрисвоениемЗначения - assigned but never read
        // 3. ВПроцедуреНеИспользуемая - assigned but never read
        //
        // Note: Variables without Перем (implicit) are currently tracked the same way
        assert_eq!(
            unused_diags.len(),
            3,
            "Expected 3 unused local variables, got {}. Diagnostics: {:?}",
            unused_diags.len(),
            unused_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    /// Test module-level variable tracking.
    ///
    /// Module-level variables should be flagged if unused, unless exported.
    #[test]
    fn test_module_level_unused_variable() {
        // Module-level variable that is never used
        let code = r#"Перем НеИспользуемая;

Процедура Тест()
    Сообщить("Привет");
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(unused_diags.len(), 1, "Unused module variable should trigger diagnostic");
        // "НеИспользуемая" on line 0, col 6-20 (after "Перем ")
        assert_diagnostic_range(code, unused_diags[0], 0, 6, 20);
    }

    #[test]
    fn test_module_level_export_variable_not_flagged() {
        // Exported module-level variable should NOT be flagged
        let code = r#"Перем ЭкспортнаяПеременная Экспорт;

Процедура Тест()
    Сообщить("Привет");
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Exported variable should not trigger diagnostic"
        );
    }

    #[test]
    fn test_module_level_used_variable() {
        // Module-level variable used in a method should NOT be flagged
        let code = r#"Перем ИспользуемаяПеременная;

Процедура Тест()
    Сообщить(ИспользуемаяПеременная);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Used module variable should not trigger diagnostic"
        );
    }

    #[test]
    fn test_module_level_code_unused_variable() {
        // Module-level code: variable assigned but never read
        let code = r#"НеИспользуемаяВМодуле = 30;
ИспользуемаяВМодуле = 40;
Сообщить(ИспользуемаяВМодуле);"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(
            unused_diags.len(),
            1,
            "Module-level code should detect unused implicit variable"
        );
        // "НеИспользуемаяВМодуле" on line 0, col 0-21
        assert_diagnostic_range(code, unused_diags[0], 0, 0, 21);
    }

    /// Full Java fixture test.
    ///
    /// Java test expects 5 diagnostics:
    /// - hasRange(1, 6, 36): Line 1, `ПеременнаяМодуляНеИспользуемая` with `&НаКлиенте`
    /// - hasRange(19, 10, 35): Line 19, `ЛокальнаяБезИспользования`
    /// - hasRange(19, 37, 63): Line 19, `ТолькоСПрисвоениемЗначения`
    /// - hasRange(24, 4, 28): Line 24, `ВПроцедуреНеИспользуемая`
    /// - hasRange(83, 0, 25): Line 83, `ВнеПроцедурНеИспользуемая`
    ///
    /// Note: Java uses 0-indexed lines. Our implementation may differ because:
    /// - We don't handle `&НаКлиенте`/`&НаСервере` annotations (both vars with same name flagged)
    /// - We may detect additional unused variables Java doesn't flag
    #[test]
    fn test_java_fixture_full() {
        let code = include_str!("../../tests/fixtures/UnusedLocalVariableDiagnostic.bsl");

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        // Print all diagnostics for debugging
        println!("Found {} UnusedLocalVariable diagnostics:", unused_diags.len());
        for (i, diag) in unused_diags.iter().enumerate() {
            println!("  {}: {}", i + 1, diag.message);
        }
        println!("\nAll diagnostics ({}):", diagnostics.len());
        for (i, diag) in diagnostics.iter().enumerate() {
            println!("  {}: [{:?}] {}", i + 1, diag.code, diag.message);
        }

        // Check that we detect the key cases from Java test
        let messages: Vec<&str> = unused_diags.iter().map(|d| d.message.as_str()).collect();

        // These should be detected (matching Java):
        assert!(
            messages.iter().any(|m| m.contains("ЛокальнаяБезИспользования")),
            "Should detect ЛокальнаяБезИспользования"
        );
        assert!(
            messages.iter().any(|m| m.contains("ТолькоСПрисвоениемЗначения")),
            "Should detect ТолькоСПрисвоениемЗначения"
        );
        assert!(
            messages.iter().any(|m| m.contains("ВПроцедуреНеИспользуемая")),
            "Should detect ВПроцедуреНеИспользуемая"
        );
        assert!(
            messages.iter().any(|m| m.contains("ВнеПроцедурНеИспользуемая")),
            "Should detect ВнеПроцедурНеИспользуемая"
        );
        assert!(
            messages.iter().any(|m| m.contains("ПеременнаяМодуляНеИспользуемая")),
            "Should detect ПеременнаяМодуляНеИспользуемая"
        );

        // Java expects exactly 5 diagnostics.
        // Note: ПеременнаяМодуляНеИспользуемая appears twice in fixture:
        // - Line 2: &НаКлиенте Перем ПеременнаяМодуляНеИспользуемая; // Error (first declaration)
        // - Line 5: &НаСервере Перем ПеременнаяМодуляНеИспользуемая; // Ignored (duplicate name)
        //
        // Java ignores duplicate module variable declarations (VariableSymbolComputer.visitModuleVarDeclaration:88-89).
        // We now match this behavior by skipping duplicates in SymbolTreeBuilder.add_variable.
        assert_eq!(unused_diags.len(), 5, "Should detect 5 unused variables (matching Java)");
    }
}
