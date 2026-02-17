//! MissingReturnedValueDescription diagnostic.
//!
//! **HIR-based implementation** using structured documentation from `method.docs()`.
//!
//! Checks that:
//! 1. Functions have return value descriptions in comments ("Возвращаемое значение:")
//! 2. Procedures do NOT have return value descriptions
//! 3. In strict mode, each returned type has a description (not just type name)
//!
//! ## Configuration
//!
//! - `allowShortDescriptionReturnValues` (boolean, default: true)
//!   - `true`: Only require return block presence
//!   - `false`: Require description text for each type
//!
//! ## Examples
//!
//! ### Bad practice (function without return description)
//! ```bsl
//! // Вычисляет сумму
//! Функция ВычислитьСумму(А, Б)
//!     Возврат А + Б;
//! КонецФункции
//! ```
//!
//! ### Good practice
//! ```bsl
//! // Вычисляет сумму
//! //
//! // Возвращаемое значение:
//! //  Число - сумма двух чисел
//! Функция ВычислитьСумму(А, Б)
//!     Возврат А + Б;
//! КонецФункции
//! ```
//!
//! ### Bad practice (procedure with return description)
//! ```bsl
//! // Выводит сообщение
//! //
//! // Возвращаемое значение:
//! //  Строка
//! Процедура ВывестиСообщение()
//!     Сообщить("Привет");
//! КонецПроцедуры
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir_def::item_tree::ModItem;
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Run the MissingReturnedValueDescription diagnostic.
///
/// Uses HIR-based documentation API (`method.docs()`) instead of ad-hoc comment parsing.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::MissingReturnedValueDescription;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    // Get module data from HIR
    let module_data = ctx.module_data();

    // Check all procedures
    for method_id in &module_data.procedures {
        if let Some(diag) = check_procedure_hir(ctx, *method_id, code) {
            diagnostics.push(diag);
        }
    }

    // Check all functions
    for method_id in &module_data.functions {
        if let Some(diag) = check_function_hir(ctx, *method_id, code) {
            diagnostics.push(diag);
        }
    }

    diagnostics
}

/// Check a function for missing or invalid return description (HIR-based).
fn check_function_hir(
    ctx: &DiagnosticsContext,
    method_id: hir_def::MethodId,
    code: DiagnosticCode,
) -> Option<Diagnostic> {
    // Get item tree via ctx (works in both LSP and streaming mode)
    let tree = ctx.item_tree();

    // Get function info from item tree
    let func_info =
        tree.top_level_items().get(method_id.local_id as usize).and_then(|item| match item {
            ModItem::Function(func_idx) => {
                let func = tree.function(*func_idx);
                Some((func.is_export, func.name_range))
            }
            _ => None,
        });

    let (is_export, name_range) = func_info?;

    // Only check export functions (public API)
    if !is_export {
        return None;
    }

    // Get documentation via ctx (works in both LSP and streaming mode)
    // If no documentation, require it for export functions
    let docs = match ctx.method_docs(method_id) {
        Some(d) => d,
        None => {
            return Some(create_diagnostic(
                name_range,
                "Добавьте описание возвращаемого значения функции",
                code,
                ctx,
            ));
        }
    };

    // If it's a hyperlink reference, skip validation
    if docs.is_hyperlink() {
        return None;
    }

    // Check if return value section exists
    if docs.returned_value.is_empty() {
        return Some(create_diagnostic(
            name_range,
            "Добавьте описание возвращаемого значения функции",
            code,
            ctx,
        ));
    }

    // Get configuration parameter
    let allow_short = ctx
        .config
        .get_bool(
            DiagnosticCode::MissingReturnedValueDescription,
            "allowShortDescriptionReturnValues",
        )
        .unwrap_or(true);

    // Strict mode - all types must have descriptions
    if !allow_short {
        let types_without_desc: Vec<&str> = docs
            .returned_value
            .iter()
            .filter_map(|type_doc| {
                // Type has no description AND no sub-parameters (structured type)
                if type_doc.description.is_none() && type_doc.parameters.is_empty() {
                    Some(type_doc.name.as_str())
                } else {
                    None
                }
            })
            .collect();

        if !types_without_desc.is_empty() {
            let types_list = types_without_desc.join(", ");
            let message = format!(
                "Необходимо добавить описание типов \"{}\" возвращаемого значения",
                types_list
            );
            return Some(create_diagnostic(name_range, &message, code, ctx));
        }
    }

    None
}

/// Check a procedure for invalid return description (HIR-based).
fn check_procedure_hir(
    ctx: &DiagnosticsContext,
    method_id: hir_def::MethodId,
    code: DiagnosticCode,
) -> Option<Diagnostic> {
    // Get item tree via ctx (works in both LSP and streaming mode)
    let tree = ctx.item_tree();

    // Get procedure name range from item tree
    let name_range =
        tree.top_level_items().get(method_id.local_id as usize).and_then(|item| match item {
            ModItem::Procedure(proc_idx) => Some(tree.procedure(*proc_idx).name_range),
            _ => None,
        })?;

    // Get documentation via ctx (works in both LSP and streaming mode)
    let docs = ctx.method_docs(method_id)?;

    // Procedures must NOT have return value descriptions
    if !docs.returned_value.is_empty() {
        return Some(create_diagnostic(
            name_range,
            "Удалите описание возвращаемого значения для процедуры",
            code,
            ctx,
        ));
    }

    None
}

/// Create a diagnostic with the given message.
///
/// The diagnostic range is set to the method name (identifier only).
fn create_diagnostic(
    range: TextRange,
    message: &str,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: message.to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_function_without_comments() {
        let code = "Функция Example()\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Non-export function without comments should not trigger");
    }

    #[test]
    fn test_export_function_without_comments() {
        // Export function without any comments should trigger diagnostic
        let code = "Функция Example() Экспорт\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            1,
            "Export function without any comments should trigger diagnostic"
        );
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingReturnedValueDescription);
        assert!(diagnostics[0].message.contains("Добавьте описание"));
        // Line 0, function name
        assert_diagnostic_range(code, &diagnostics[0], 0, 8, 15);
    }

    #[test]
    fn test_function_with_description_no_return() {
        let code = "// Описание вроде\nФункция Example() Экспорт\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingReturnedValueDescription);
        assert!(diagnostics[0].message.contains("Добавьте описание"));
        // Line 1 (0-indexed), "Example" at columns 8-15
        assert_diagnostic_range(code, &diagnostics[0], 1, 8, 15);
    }

    #[test]
    fn test_function_with_empty_return_block() {
        let code =
            "// Описание вроде\n// Возвращаемое значение:\nФункция Example() Экспорт\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingReturnedValueDescription);
        assert!(diagnostics[0].message.contains("Добавьте описание"));
        // Line 2, "Example"
        assert_diagnostic_range(code, &diagnostics[0], 2, 8, 15);
    }

    #[test]
    fn test_function_with_complete_description() {
        let code = "// Описание вроде\n// Возвращаемое значение:\n// Строка - строка типа\nФункция Example()\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Function with complete description should be OK");
    }

    #[test]
    fn test_procedure_with_return_description() {
        let code =
            "// Описание вроде\n// Возвращаемое значение:\n// Строка - строка типа\nПроцедура Example()\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingReturnedValueDescription);
        assert!(diagnostics[0].message.contains("Удалите описание"));
        // Line 3, "Example"
        assert_diagnostic_range(code, &diagnostics[0], 3, 10, 17);
    }

    #[test]
    fn test_procedure_without_return() {
        let code = "// Описание вроде\nПроцедура Example()\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Procedure without return description should be OK");
    }

    #[test]
    fn test_function_with_type_no_description_default_mode() {
        let code = "// Описание вроде\n// Возвращаемое значение:\n// Строка\nФункция Example()\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);
        // Default mode (allowShortDescriptionReturnValues=true): type name alone is OK
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_function_with_type_no_description_strict_mode() {
        let code = "// Описание вроде\n// Возвращаемое значение:\n// Строка\nФункция Example() Экспорт\nКонецФункции";

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingReturnedValueDescription,
            serde_json::json!({"allowShortDescriptionReturnValues": false}),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingReturnedValueDescription);
        assert!(diagnostics[0].message.contains("Необходимо добавить описание типов"));
        assert!(diagnostics[0].message.contains("Строка"));
        assert_diagnostic_range(code, &diagnostics[0], 3, 8, 15);
    }

    #[test]
    fn test_function_with_hyperlink_reference() {
        let code = "// См. Пример7()\nФункция Example()\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);
        // Hyperlink references bypass validation
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_function_with_multiple_types_no_description_strict() {
        let code = "// Описание вроде\n// Возвращаемое значение:\n// - Строка\n// - булево\nФункция Example() Экспорт\nКонецФункции";

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingReturnedValueDescription,
            serde_json::json!({"allowShortDescriptionReturnValues": false}),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Строка"));
        assert!(diagnostics[0].message.contains("булево"));
        assert_diagnostic_range(code, &diagnostics[0], 4, 8, 15);
    }

    #[test]
    fn test_english_keywords() {
        let code =
            "// Description\n// Returns:\n// String - result\nFunction Example()\nEndFunction";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "English keywords should work");
    }

    #[test]
    fn test_structure_with_nested_fields() {
        // Real-world example from user: function with structured return type
        let code = r#"// Возвращает структуру с доступными публикациями HTTP-сервисов ERP.
//
// Возвращаемое значение:
//   Структура - Структура с ключами-названиями сервисов и значениями-URL путями к публикациям:
//     * ПОЗК - Строка - Публикация для работы с производственными заказами.
//     * ДанныеДО - Строка - Публикация для получения данных документооборота.
//     * ДанныеДООтветственный - Строка - Публикация для получения данных об ответственных.
//     * Рецептура - Строка - Публикация для работы с рецептурами.
//
Функция ПубликацииERP() Экспорт
    Структура = Новый Структура;
    Структура.Вставить("ПОЗК", "/hs/pozk/getdirection");
    Структура.Вставить("ДанныеДО", "/hs/dodata/statusdocument");
    Структура.Вставить("ДанныеДООтветственный", "/hs/dodata/responsible");
    Структура.Вставить("Рецептура", "/hs/recipe/changestatus");
    Возврат Структура;
КонецФункции"#;
        let diagnostics = check_ast_diagnostic(code, check);

        // Should have NO diagnostics - return value is properly documented
        assert_eq!(
            diagnostics.len(),
            0,
            "Function with structured return type description should be valid"
        );
    }

    #[test]
    fn test_diagnostic_range_for_export_function() {
        // Test that diagnostic highlights only the function name, not "() Экспорт"
        let code = "// Описание\nФункция ПубликацииERP() Экспорт\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 1, "Should have one diagnostic");

        // Extract actual highlighted text
        let range = diagnostics[0].range;
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        let highlighted_text = &code[start..end];

        eprintln!("Highlighted text: '{}'", highlighted_text);
        eprintln!("Expected: 'ПубликацииERP'");

        // Should highlight only the function name
        assert_eq!(
            highlighted_text, "ПубликацииERP",
            "Should highlight only function name, not parameters or modifiers"
        );
    }

    #[test]
    fn test_diagnostic_range_mixed_cyrillic_latin() {
        // Real example from user: mixed Cyrillic+Latin function name
        // NOTE: This test verifies diagnostic GENERATES correct byte-based TextRange.
        // The incorrect column positions reported by user (columns 16-33 instead of 8-18)
        // are due to LSP server not converting byte positions to UTF-16 code units.
        // See: crates/bsl-analyzer/src/lsp/to_proto.rs:range() - needs to use position_utf16()
        let code = "// Описание\nФункция ЗапросВERP(СервисПубликации, ПараметрыЗапроса, Сессия = Неопределено) Экспорт\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 1, "Should have one diagnostic");

        // Extract actual highlighted text
        let range = diagnostics[0].range;
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        let highlighted_text = &code[start..end];

        // Should highlight only the function name
        assert_eq!(
            highlighted_text, "ЗапросВERP",
            "Should highlight only function name 'ЗапросВERP', got '{}'",
            highlighted_text
        );
    }

    #[test]
    fn test_non_export_function_no_diagnostic() {
        // Non-export (private) functions don't require return value documentation
        let code = "// Описание\nФункция НастройкиПодключения(СервисПубликации)\n\tВозврат Новый Структура;\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            0,
            "Non-export functions should not require return value documentation"
        );
    }

    #[test]
    fn test_export_function_requires_documentation() {
        // Export functions must have return value documentation
        let code =
            "// Описание\nФункция НастройкиПодключения(СервисПубликации) Экспорт\n\tВозврат Новый Структура;\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            1,
            "Export function without return docs should trigger diagnostic"
        );
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingReturnedValueDescription);
        assert!(diagnostics[0].message.contains("Добавьте описание"));
        // Line 1, function name
        assert_diagnostic_range(code, &diagnostics[0], 1, 8, 28);
    }

    #[test]
    fn test_export_function_with_complete_docs_ok() {
        // Export function with complete documentation should pass
        let code = "// Описание\n// Возвращаемое значение:\n//  Структура - настройки подключения\nФункция НастройкиПодключения(СервисПубликации) Экспорт\n\tВозврат Новый Структура;\nКонецФункции";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 0, "Export function with return docs should be OK");
    }

    #[test]
    fn test_fields_without_main_type_triggers_diagnostic() {
        // This is CORRECT behavior - diagnostic should trigger when fields (*)
        // are listed without specifying the main return type.
        //
        // INCORRECT format (should trigger diagnostic):
        //   Возвращаемое значение:
        //     * Field1 - Type1 - description
        //     * Field2 - Type2 - description
        //
        // CORRECT format (should NOT trigger):
        //   Возвращаемое значение:
        //     Структура:
        //     * Field1 - Type1 - description
        //     * Field2 - Type2 - description
        //
        // This matches bsl-language-server behavior: sub-parameters (fields with *)
        // are added to the last type. If there's no type, they're ignored and the
        // return value block is considered empty.
        let code = r#"// Пакет ответа результата вызова метода HTTP.
//
// Возвращаемое значение:
//   * Метод - Строка - имя HTTP-метода запроса
//   * URL - Строка - итоговый URL, по которому был выполнен запрос.
//   * КодСостояния - Число - Код состояния ответа.
//   * Заголовки - Соответствие - Заголовки ответа.
//   * Тело - ДвоичныеДанные - Тело ответа.
//   * Кодировка - Строка - код кодировки ответа.
//
Функция НовыйОтвет() Экспорт
    Возврат Новый Структура;
КонецФункции"#;
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            1,
            "Should trigger diagnostic - fields without main type declaration"
        );
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingReturnedValueDescription);
        assert!(
            diagnostics[0].message.contains("Добавьте описание"),
            "Expected 'Добавьте описание' in message, got: {}",
            diagnostics[0].message
        );
        // Line 10, function name НовыйОтвет
        assert_diagnostic_range(code, &diagnostics[0], 10, 8, 18);
    }

    #[test]
    fn test_fields_with_main_type_ok() {
        // CORRECT format - main type declared before fields
        let code = r#"// Пакет ответа результата вызова метода HTTP.
//
// Возвращаемое значение:
//   Структура:
//   * Метод - Строка - имя HTTP-метода запроса
//   * URL - Строка - итоговый URL, по которому был выполнен запрос.
//   * КодСостояния - Число - Код состояния ответа.
//
Функция НовыйОтвет() Экспорт
    Возврат Новый Структура;
КонецФункции"#;
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            0,
            "Should NOT trigger - main type 'Структура:' is declared before fields"
        );
    }
}
