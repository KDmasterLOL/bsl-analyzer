//! DeletingCollectionItem diagnostic.
//!
//! Detects deletion of collection items within a ForEach loop iterating over that same collection.
//!
//! ## Why?
//! Deleting elements during `ForEach` iteration causes:
//! - Collection indices to change during iteration
//! - Some elements to be skipped
//! - Potential runtime errors
//! - Unexpected behavior in production code
//!
//! ## Bad practice
//! ```bsl
//! Для Каждого Элемент Из Коллекция Цикл
//!     Коллекция.Удалить(Элемент); // Error: Deleting from iterated collection!
//! КонецЦикла;
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Option 1: Reverse loop by index
//! Для Индекс = Коллекция.Количество() - 1 По 0 Цикл -1
//!     Коллекция.Удалить(Индекс);
//! КонецЦикла;
//!
//! // Option 2: Collect items to delete
//! УдаляемыеЭлементы = Новый Массив;
//! Для Каждого Элемент Из Коллекция Цикл
//!     Если УсловиеУдаления(Элемент) Тогда
//!         УдаляемыеЭлементы.Добавить(Элемент);
//!     КонецЕсли;
//! КонецЦикла;
//!
//! Для Каждого Элемент Из УдаляемыеЭлементы Цикл
//!     Коллекция.Удалить(Элемент);
//! КонецЦикла;
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Error (MAJOR)
//! - **Tags:** STANDARD, ERROR
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//! Ported from:
//! - DeletingCollectionItemDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - deleting_collection_item.rs (bsl-language-server-rust) - Rust reference (tree-sitter)
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::DeletingCollectionItem` is encountered.
pub fn from_hir(
    collection: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::DeletingCollectionItem;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Удаление элемента из коллекции '{}' во время итерации по ней может \
             привести к пропуску элементов или ошибкам. Используйте обратный цикл \
             по индексу или соберите элементы для удаления отдельно",
            collection
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

pub fn check(_ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // All diagnostics are now collected during HIR lowering
    // This function is kept for compatibility with existing diagnostic infrastructure
    // Real diagnostics are emitted via from_hir() dispatch in lib.rs
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};

    #[test]
    fn test_simple_deletion() {
        let code = r#"
Процедура Тест()
Для Каждого Элемент Из Коллекция Цикл
    Коллекция.Удалить(Элемент);
КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        assert_eq!(diags.len(), 1, "Should detect deletion in ForEach");
    }

    #[test]
    fn test_different_collection_ok() {
        let code = r#"
Процедура Тест()
Для Каждого Элемент Из Коллекция1 Цикл
    Коллекция2.Удалить(Элемент);
КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        assert_eq!(diags.len(), 0, "Different collection should be OK");
    }

    #[test]
    fn test_global_delete_ok() {
        let code = r#"
Процедура Тест()
Для Каждого Элемент Из Коллекция Цикл
    Удалить(Элемент);
КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        assert_eq!(diags.len(), 0, "Global Удалить() should be OK");
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
for each elem in mass do
    mass.delete(elem);
enddo;
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        assert_eq!(diags.len(), 1, "Should detect English delete");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Procedure Test()
for each elem in Mass().mass1.mass2() do
    mass().mAss1.mass2().delete(elem+1);
enddo;
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        assert_eq!(diags.len(), 1, "Should match case-insensitively");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/DeletingCollectionItemDiagnostic.bsl");

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();

        assert_eq!(diags.len(), 8, "Should match Java: 8 diagnostics");

        // Verify positions match Java implementation
        assert_diagnostic_range(code, diags[0], 17, 8, 47);
        assert_diagnostic_range(code, diags[1], 23, 4, 21);
        assert_diagnostic_range(code, diags[2], 28, 4, 25);
        assert_diagnostic_range(code, diags[3], 33, 4, 30);
        assert_diagnostic_range(code, diags[4], 39, 8, 34);
        assert_diagnostic_range(code, diags[5], 45, 4, 23);
        assert_diagnostic_range(code, diags[6], 50, 4, 37);
        assert_diagnostic_range(code, diags[7], 55, 4, 39);
    }

    #[test]
    fn test_break_after_delete_simple() {
        // Простой тест: Delete + Прервать в ForEach - безопасный паттерн
        // Simple test: Delete + Break in ForEach - safe pattern
        let code = r#"
Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Если УсловиеУдаления(Элемент) Тогда
            Коллекция.Удалить(Элемент);
            Прервать;
        КонецЕсли;
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();

        assert_eq!(diags.len(), 0, "Delete + Break should be safe");
    }

    #[test]
    fn test_break_after_delete_nested_loops() {
        // Реальный паттерн из doc3: вложенные циклы с Delete + Break
        // Real pattern from doc3: nested loops with Delete + Break
        // Catalogs/ГруппыКонтактовПользователей/Ext/ObjectModule.bsl
        let code = r#"
Процедура ПередЗаписью()
    // Внешний цикл по элементам для удаления
    Для Каждого Эл Из ДополнительныеСвойства.РабочаяГруппаУдалить Цикл
        // Внутренний цикл поиска
        Для Каждого Эл2 Из РабочаяГруппа Цикл
            Если Эл2.Участник = Эл.Участник Тогда
                // Удаление + выход - безопасно
                РабочаяГруппа.Удалить(Эл2);
                Прервать;
            КонецЕсли;
        КонецЦикла;
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();

        assert_eq!(diags.len(), 0, "Delete + Break in nested loops should be safe");
    }

    #[test]
    fn test_return_after_delete() {
        // Delete + Возврат - тоже безопасный паттерн
        // Delete + Return is also safe
        let code = r#"
Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Если НужноУдалить(Элемент) Тогда
            Коллекция.Удалить(Элемент);
            Возврат;
        КонецЕсли;
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();

        assert_eq!(diags.len(), 0, "Delete + Return should be safe");
    }

    #[test]
    fn test_delete_without_break_still_error() {
        // Delete без Break/Return - всё ещё ошибка
        // Delete without Break/Return - still an error
        let code = r#"
Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Если УсловиеУдаления(Элемент) Тогда
            Коллекция.Удалить(Элемент);
            // Нет Break - итерация продолжается, это опасно
        КонецЕсли;
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();

        assert_eq!(diags.len(), 1, "Delete without Break should trigger error");
    }
}
