//! SelfInsertion diagnostic.
//!
//! Detects insertion of a collection into itself via Insert/Add methods.
//!
//! ## Why?
//! Inserting a collection into itself:
//! - Creates infinite recursion or corrupted data structures
//! - Results in undefined behavior
//! - Indicates a logic error in the code
//!
//! ## Bad practice
//! ```bsl
//! Товары.Добавить(Товары);           // Error: array into itself
//! Структура.Вставить("Ключ", Структура);  // Error: structure into itself
//! ```
//!
//! ## Good practice
//! ```bsl
//! Товары.Добавить(Товар);            // Add different item
//! Структура.Вставить("Ключ", Значение);   // Insert different value
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Error (MAJOR)
//!
//! ## Implementation

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
    tags: &[MetadataTag::Standard, MetadataTag::Unpredictable, MetadataTag::Performance],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch.rs when `BodyDiagnostic::SelfInsertion` is encountered.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::SelfInsertion,
        "Удалите вставку коллекции в саму себя",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;
    #[test]
    fn test_array_add_self() {
        let code =
            "Процедура Тест()\nТовары = Новый Массив();\nТовары.Добавить(Товары);\nКонецПроцедуры";
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SelfInsertion).collect();
        assert_eq!(diags.len(), 1);
        assert_diagnostic_range(code, diags[0], 2, 0, 23);
    }

    #[test]
    fn test_structure_insert_self() {
        let code = "Процедура Тест()\nНастройки = Новый Структура();\nНастройки.Вставить(\"Ключ\", Настройки);\nКонецПроцедуры";
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SelfInsertion).collect();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_different_objects_ok() {
        let code = "Процедура Тест()\nМассив1 = Новый Массив();\nМассив2 = Новый Массив();\nМассив1.Добавить(Массив2);\nКонецПроцедуры";
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SelfInsertion).collect();
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_other_method_ok() {
        let code = "Процедура Тест()\nМодуль.ВыполнитьПроверку(Модуль);\nКонецПроцедуры";
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SelfInsertion).collect();
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_english_methods() {
        let code = "Procedure Test()\nArr = New Array();\nArr.Add(Arr);\nEndProcedure";
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SelfInsertion).collect();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_insert_english() {
        let code = "Procedure Test()\nMap = New Map();\nMap.Insert(\"key\", Map);\nEndProcedure";
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SelfInsertion).collect();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура Тест()
    НастройкиПроверки = Новый Структура();
    НастройкиПроверки.Вставить("ВыполнятьВФоне", Истина);
    НастройкиПроверки.Вставить("ТутЯ", НастройкиПроверки);

    Товары = Новый Массив();
    Товары.Добавить(Товар1);
    Товары.Добавить(Товар2);
    Товары.Добавить(Товар3);
    Товары.Добавить(Товары);
    Товары.Добавить(Товар4);
    Товары.Добавить(Товар5);

    ОбщийМодуль.ВыполнитьПроверку(ОбщийМодуль);

    Переменная = Переменная.Метод();
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::SelfInsertion).collect();

        // Expected: 2 diagnostics
        // Line 3: НастройкиПроверки.Вставить("ТутЯ", НастройкиПроверки);
        // Line 9: Товары.Добавить(Товары);
        assert_eq!(diags.len(), 2, "Should find 2 diagnostics");

        // Verify positions match bsl-language-server implementation
        assert_diagnostic_range(code, diags[0], 3, 4, 57);
        assert_diagnostic_range(code, diags[1], 9, 4, 27);
    }
}
