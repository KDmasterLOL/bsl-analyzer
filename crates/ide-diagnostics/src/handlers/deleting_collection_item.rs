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
//!
//! Integrated through local HIR body diagnostics and adapted to the local
//! syntax and semantic model.

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
    tags: &[MetadataTag::Standard, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{check_hir_diagnostic, format_diags};
    use expect_test::expect;
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#"
            DeletingCollectionItem @ 4:5..4:31
              message: Удаление элемента из коллекции 'Коллекция ' во время итерации по ней может привести к пропуску элементов или ошибкам. Используйте обратный цикл по индексу или соберите элементы для удаления отдельно
              severity: Major"#]].assert_eq(&format_diags(code, &diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#"
            DeletingCollectionItem @ 4:5..4:22
              message: Удаление элемента из коллекции 'mass ' во время итерации по ней может привести к пропуску элементов или ошибкам. Используйте обратный цикл по индексу или соберите элементы для удаления отдельно
              severity: Major"#]].assert_eq(&format_diags(code, &diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#"
            DeletingCollectionItem @ 4:5..4:40
              message: Удаление элемента из коллекции 'Mass().mass1.mass2() ' во время итерации по ней может привести к пропуску элементов или ошибкам. Используйте обратный цикл по индексу или соберите элементы для удаления отдельно
              severity: Major"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_good_different_collection_nested_field() {
        // НеЭтаКоллекция.Удалить while iterating Коллекция — no error
        let code = r#"
Процедура Тест()
Для Каждого Элемент Из Коллекция Цикл
    Если Элемент < 10 Тогда
        НеЭтаКоллекция.Удалить(Элемент);
    КонецЕсли;
КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_good_global_delete_nested_field() {
        // Global Удалить() while iterating — no error
        let code = r#"
Процедура Тест()
Для Каждого Элемент Из Коллекция Цикл
    Если Элемент < 10 Тогда
        Удалить(Элемент);
    КонецЕсли;
КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_error_chained_field_collection() {
        // error1: Коллекция.ЕщеКоллекция.Удалить while iterating Коллекция.ЕщеКоллекция
        let code = r#"
Процедура Тест()
Для Каждого Элемент Из Коллекция.ЕщеКоллекция Цикл
    Если Элемент < 10 Тогда
        Коллекция.ЕщеКоллекция.Удалить(Элемент);
    КонецЕсли;
КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#"
            DeletingCollectionItem @ 5:9..5:48
              message: Удаление элемента из коллекции 'Коллекция.ЕщеКоллекция ' во время итерации по ней может привести к пропуску элементов или ошибкам. Используйте обратный цикл по индексу или соберите элементы для удаления отдельно
              severity: Major"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_error_simple_english() {
        // error2: mass.delete(elem)
        let code = r#"
Procedure Test()
for each elem in mass do
    mass.delete(elem);
enddo;
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#"
            DeletingCollectionItem @ 4:5..4:22
              message: Удаление элемента из коллекции 'mass ' во время итерации по ней может привести к пропуску элементов или ошибкам. Используйте обратный цикл по индексу или соберите элементы для удаления отдельно
              severity: Major"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_error_parenthesized_arg() {
        // error3: mass.delete((elem)) — extra parens around arg
        let code = r#"
Procedure Test()
for each elem in mass do
    mass.delete( (elem ));
enddo;
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#"
            DeletingCollectionItem @ 4:5..4:26
              message: Удаление элемента из коллекции 'mass ' во время итерации по ней может привести к пропуску элементов или ошибкам. Используйте обратный цикл по индексу или соберите элементы для удаления отдельно
              severity: Major"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_error_simple_russian_no_condition() {
        // error4: Коллекция.Удалить(Элемент) directly in loop without If
        let code = r#"
Процедура Тест()
Для Каждого Элемент Из Коллекция Цикл
    Коллекция.Удалить(Элемент);
КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#"
            DeletingCollectionItem @ 4:5..4:31
              message: Удаление элемента из коллекции 'Коллекция ' во время итерации по ней может привести к пропуску элементов или ошибкам. Используйте обратный цикл по индексу или соберите элементы для удаления отдельно
              severity: Major"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_error_inside_if_in_loop() {
        // error5: delete inside If inside ForEach
        let code = r#"
Процедура Тест()
Для Каждого Элемент Из Коллекция Цикл
    Если Элемент < 10 Тогда
        Коллекция.Удалить(Элемент);
    КонецЕсли;
КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#"
            DeletingCollectionItem @ 5:9..5:35
              message: Удаление элемента из коллекции 'Коллекция ' во время итерации по ней может привести к пропуску элементов или ошибкам. Используйте обратный цикл по индексу или соберите элементы для удаления отдельно
              severity: Major"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_error_expression_arg() {
        // error6: mass.delete(elem+1)
        let code = r#"
Procedure Test()
for each elem in mass do
    mass.delete(elem+1);
enddo;
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#"
            DeletingCollectionItem @ 4:5..4:24
              message: Удаление элемента из коллекции 'mass ' во время итерации по ней может привести к пропуску элементов или ошибкам. Используйте обратный цикл по индексу или соберите элементы для удаления отдельно
              severity: Major"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_error_chained_method_calls() {
        // error7: mass.mass1().mass2 chained collection
        let code = r#"
Procedure Test()
for each elem in mass.mass1().mass2 do
    mass.mass1().mass2.delete(elem+1);
enddo;
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#"
            DeletingCollectionItem @ 4:5..4:38
              message: Удаление элемента из коллекции 'mass.mass1().mass2 ' во время итерации по ней может привести к пропуску элементов или ошибкам. Используйте обратный цикл по индексу или соберите элементы для удаления отдельно
              severity: Major"#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_good_different_collection_after_loop() {
        // good: Коллекция1.Удалить while iterating Коллекция — no error
        let code = r#"
Процедура Тест()
Для Каждого Элемент Из Коллекция Цикл
    Если Элемент < 10 Тогда
        Коллекция1.Удалить(Элемент);
    КонецЕсли;
КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_break_after_delete_nested_loops() {
        // Вложенные циклы с Delete + Break — безопасный паттерн
        // Nested loops with Delete + Break — safe pattern
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();

        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
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
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::DeletingCollectionItem)
            .collect();

        expect![[r#"
            DeletingCollectionItem @ 5:13..5:39
              message: Удаление элемента из коллекции 'Коллекция ' во время итерации по ней может привести к пропуску элементов или ошибкам. Используйте обратный цикл по индексу или соберите элементы для удаления отдельно
              severity: Major"#]].assert_eq(&format_diags(code, &diags));
    }
}
