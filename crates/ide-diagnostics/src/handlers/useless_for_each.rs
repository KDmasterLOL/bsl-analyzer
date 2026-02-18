//! UseLessForEach diagnostic.
//!
//! Detects unused iterators in "For Each" loops - when the loop iterates over a collection
//! but the iterator variable is never used in the loop body.
//!
//! ## Why?
//! An unused iterator indicates either:
//! - Programming error (forgetting to use the variable)
//! - Unnecessary iteration (should use a different approach)
//!
//! ## Bad practice
//! ```bsl
//! Для Каждого Итератор Из Коллекция Цикл
//!     Итератор(); // Calling iterator as a function is NOT valid usage
//! КонецЦикла;
//! ```
//!
//! ## Good practice
//! ```bsl
//! Для Каждого Элемент Из Коллекция Цикл
//!     Результат = Элемент.Свойство; // Property access
//! КонецЦикла;
//!
//! Для Каждого А Из Б Цикл
//!     А = Истина; // Assignment
//! КонецЦикла;
//!
//! Для Каждого Объект Из Б Цикл
//!     Объект.Метод(); // Method call on iterator
//! КонецЦикла;
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Error (CRITICAL)
//! - **Tags:** CLUMSY
//! - **Minutes to fix:** 2

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::Name;
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Clumsy],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    iterator_name: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::UseLessForEach;
    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    // Skip if iterator name matches a module-level variable
    let symbol_tree = ctx.symbol_tree();
    if symbol_tree.find_variable(&Name::new(iterator_name)).is_some() {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Итератор не используется в теле цикла".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;

    fn filter(diagnostics: &[crate::Diagnostic]) -> Vec<&crate::Diagnostic> {
        diagnostics.iter().filter(|d| d.code == DiagnosticCode::UseLessForEach).collect()
    }

    #[test]
    fn test_unused_iterator() {
        let code = r#"
Процедура Тест()
    Для Каждого Итератор Из Коллекция Цикл
        Итератор();
    КонецЦикла;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::UseLessForEach);
    }

    #[test]
    fn test_used_in_method_call() {
        let code = r#"
Процедура Тест()
    Для Каждого А Из Б Цикл
        КакойТОМетод(а);
    КонецЦикла;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "Iterator passed to method should count as usage");
    }

    #[test]
    fn test_used_in_assignment() {
        let code = r#"
Процедура Тест()
    Для Каждого А Из Б Цикл
        В = А;
    КонецЦикла;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "Iterator in right side of assignment should count as usage");
    }

    #[test]
    fn test_iterator_assigned() {
        let code = r#"
Процедура Тест()
    Для Каждого А Из Б Цикл
        А = Истина;
    КонецЦикла;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "Iterator assigned should count as usage");
    }

    #[test]
    fn test_property_access() {
        let code = r#"
Процедура Тест()
    Для Каждого А Из Б Цикл
        А.Свойство = 1;
    КонецЦикла;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "Property access should count as usage");
    }

    #[test]
    fn test_in_condition() {
        let code = r#"
Процедура Тест()
    Для Каждого А Из Б Цикл
        Если А Тогда
        КонецЕсли;
    КонецЦикла;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "Iterator in condition should count as usage");
    }

    #[test]
    fn test_method_call_on_iterator() {
        let code = r#"
Процедура Тест()
    Для Каждого Объект Из Б Цикл
        Объект.Метод();
    КонецЦикла;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "Method call on iterator should count as usage");
    }

    #[test]
    fn test_chained_method_call() {
        let code = r#"
Процедура Тест()
    Для Каждого АСтруктура Из Б Цикл
        АСтруктура.Ключ.Метод();
    КонецЦикла;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "Chained method call should count as usage");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/UseLessForEachDiagnostic.bsl");
        let all = check_hir_diagnostic(code);
        let mut diags = filter(&all);
        diags.sort_by_key(|d| d.range.start());

        assert_eq!(diags.len(), 2, "Should match Java: 2 diagnostics");

        assert_diagnostic_range(code, diags[0], 2, 12, 20);
        assert_diagnostic_range(code, diags[1], 39, 16, 26);
    }

    #[test]
    fn test_used_iterator() {
        let code = r#"
Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Результат = Элемент.Свойство;
    КонецЦикла;
КонецПроцедуры
"#;
        let all = check_hir_diagnostic(code);
        assert_eq!(filter(&all).len(), 0, "HIR should not trigger for used iterator");
    }
}
