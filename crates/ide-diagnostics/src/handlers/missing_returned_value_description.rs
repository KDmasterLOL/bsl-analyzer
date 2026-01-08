//! MissingReturnedValueDescription diagnostic.
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

use crate::{
    method_description::parse_return_block_simple, Diagnostic, DiagnosticCode, DiagnosticsContext,
    Severity,
};
use syntax::{extract_leading_comments, SyntaxKind, SyntaxNode, TextRange};

/// Run the MissingReturnedValueDescription diagnostic.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::MissingReturnedValueDescription) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    // Get source text for comment extraction
    let file_text_input = ctx.db.file_text_input(ctx.file_id);
    let source_text = file_text_input.text(ctx.db);

    let mut diagnostics = Vec::new();

    // Check all functions and procedures
    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::FUNCTION_DEF => {
                if let Some(diag) = check_function(&node, &source_text, ctx) {
                    diagnostics.push(diag);
                }
            }
            SyntaxKind::PROCEDURE_DEF => {
                if let Some(diag) = check_procedure(&node, &source_text) {
                    diagnostics.push(diag);
                }
            }
            _ => {}
        }
    }

    diagnostics
}

/// Check a function for missing or invalid return description.
fn check_function(
    func_node: &SyntaxNode,
    source_text: &str,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    // Extract comments before the function
    let comments = extract_leading_comments(func_node, source_text)?;

    // Check if the first non-empty comment is a hyperlink reference (См./See)
    // This bypasses validation even without "Возвращаемое значение:"
    let first_comment = comments.iter().find(|c| !c.trim().is_empty())?;
    let first_trimmed = first_comment.trim().to_lowercase();
    if first_trimmed.starts_with("см.") || first_trimmed.starts_with("see ") {
        return None;
    }

    // Parse return block
    let return_info = parse_return_block_simple(&comments);

    // If it's a hyperlink reference after "Возвращаемое значение:", skip validation
    if return_info.is_hyperlink {
        return None;
    }

    // Get configuration parameter
    let allow_short = ctx
        .config
        .get_bool(
            DiagnosticCode::MissingReturnedValueDescription,
            "allowShortDescriptionReturnValues",
        )
        .unwrap_or(true);

    // Check 1: Function must have return keyword
    if !return_info.has_return_keyword {
        return Some(create_diagnostic(
            func_node,
            "Добавьте описание возвращаемого значения функции",
        ));
    }

    // Check 2: Return block must not be empty
    if return_info.types.is_empty() {
        return Some(create_diagnostic(
            func_node,
            "Добавьте описание возвращаемого значения функции",
        ));
    }

    // Check 3: Strict mode - all types must have descriptions
    if !allow_short {
        let types_without_desc: Vec<&str> = return_info
            .types
            .iter()
            .filter_map(
                |(type_name, desc)| {
                    if desc.is_none() {
                        Some(type_name.as_str())
                    } else {
                        None
                    }
                },
            )
            .collect();

        if !types_without_desc.is_empty() {
            let types_list = types_without_desc.join(", ");
            let message = format!(
                "Необходимо добавить описание типов \"{}\" возвращаемого значения",
                types_list
            );
            return Some(create_diagnostic(func_node, &message));
        }
    }

    None
}

/// Check a procedure for invalid return description.
fn check_procedure(proc_node: &SyntaxNode, source_text: &str) -> Option<Diagnostic> {
    // Extract comments before the procedure
    let comments = extract_leading_comments(proc_node, source_text)?;

    // Parse return block
    let return_info = parse_return_block_simple(&comments);

    // Procedures must NOT have return value descriptions
    if return_info.has_return_keyword {
        return Some(create_diagnostic(
            proc_node,
            "Удалите описание возвращаемого значения для процедуры",
        ));
    }

    None
}

/// Create a diagnostic with the given message.
///
/// The diagnostic range is set to the method name (first IDENT token before PARAM_LIST).
fn create_diagnostic(method_node: &SyntaxNode, message: &str) -> Diagnostic {
    let range = get_method_name_range(method_node).unwrap_or_else(|| method_node.text_range());

    Diagnostic {
        code: DiagnosticCode::MissingReturnedValueDescription,
        message: message.to_string(),
        severity: Severity::Major,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

/// Get the text range of the method name.
///
/// Returns the range of the first IDENT token that appears before PARAM_LIST.
/// This matches the behavior of FunctionShouldHaveReturn diagnostic.
fn get_method_name_range(method_node: &SyntaxNode) -> Option<TextRange> {
    let name_token = method_node
        .children_with_tokens()
        .take_while(|el| !matches!(el.kind(), SyntaxKind::PARAM_LIST))
        .filter_map(|el| el.into_token())
        .filter(|tok| !tok.kind().is_trivia())
        .find(|tok| tok.kind() == SyntaxKind::IDENT)?;

    Some(name_token.text_range())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::assert_diagnostic_range, DiagnosticsConfig};
    use ide_db::RootDatabase;
    use std::sync::Arc;

    /// Helper to run diagnostic on test code
    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        use ide_db::base_db::SourceDatabase;
        use ide_db::RootDatabaseImpl;
        use test_fixture::Fixture;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();

        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    /// Helper to run diagnostic with custom config
    fn check_diagnostic_with_config(
        code: &str,
        config: &DiagnosticsConfig,
    ) -> (Vec<Diagnostic>, String) {
        use ide_db::base_db::SourceDatabase;
        use ide_db::RootDatabaseImpl;
        use test_fixture::Fixture;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();

        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_function_without_comments() {
        let code = "Функция Example()\nКонецФункции";
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Function without comments should not trigger");
    }

    #[test]
    fn test_function_with_description_no_return() {
        let code = "// Описание вроде\nФункция Example()\nКонецФункции";
        let (diagnostics, file_content) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingReturnedValueDescription);
        assert!(diagnostics[0].message.contains("Добавьте описание"));
        // Line 1 (0-indexed), "Example" at columns 8-15
        assert_diagnostic_range(&file_content, &diagnostics[0], 1, 8, 15);
    }

    #[test]
    fn test_function_with_empty_return_block() {
        let code = "// Описание вроде\n// Возвращаемое значение:\nФункция Example()\nКонецФункции";
        let (diagnostics, file_content) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingReturnedValueDescription);
        assert!(diagnostics[0].message.contains("Добавьте описание"));
        // Line 2, "Example"
        assert_diagnostic_range(&file_content, &diagnostics[0], 2, 8, 15);
    }

    #[test]
    fn test_function_with_complete_description() {
        let code = "// Описание вроде\n// Возвращаемое значение:\n// Строка - строка типа\nФункция Example()\nКонецФункции";
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Function with complete description should be OK");
    }

    #[test]
    fn test_procedure_with_return_description() {
        let code =
            "// Описание вроде\n// Возвращаемое значение:\n// Строка - строка типа\nПроцедура Example()\nКонецПроцедуры";
        let (diagnostics, file_content) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingReturnedValueDescription);
        assert!(diagnostics[0].message.contains("Удалите описание"));
        // Line 3, "Example"
        assert_diagnostic_range(&file_content, &diagnostics[0], 3, 10, 17);
    }

    #[test]
    fn test_procedure_without_return() {
        let code = "// Описание вроде\nПроцедура Example()\nКонецПроцедуры";
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Procedure without return description should be OK");
    }

    #[test]
    fn test_function_with_type_no_description_default_mode() {
        let code = "// Описание вроде\n// Возвращаемое значение:\n// Строка\nФункция Example()\nКонецФункции";
        let (diagnostics, _) = check_diagnostic(code);
        // Default mode (allowShortDescriptionReturnValues=true): type name alone is OK
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_function_with_type_no_description_strict_mode() {
        let code = "// Описание вроде\n// Возвращаемое значение:\n// Строка\nФункция Example()\nКонецФункции";

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingReturnedValueDescription,
            serde_json::json!({"allowShortDescriptionReturnValues": false}),
        );

        let (diagnostics, file_content) = check_diagnostic_with_config(code, &config);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingReturnedValueDescription);
        assert!(diagnostics[0].message.contains("Необходимо добавить описание типов"));
        assert!(diagnostics[0].message.contains("Строка"));
        assert_diagnostic_range(&file_content, &diagnostics[0], 3, 8, 15);
    }

    #[test]
    fn test_function_with_hyperlink_reference() {
        let code = "// См. Пример7()\nФункция Example()\nКонецФункции";
        let (diagnostics, _) = check_diagnostic(code);
        // Hyperlink references bypass validation
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_function_with_multiple_types_no_description_strict() {
        let code = "// Описание вроде\n// Возвращаемое значение:\n// - Строка\n// - булево\nФункция Example()\nКонецФункции";

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingReturnedValueDescription,
            serde_json::json!({"allowShortDescriptionReturnValues": false}),
        );

        let (diagnostics, file_content) = check_diagnostic_with_config(code, &config);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Строка"));
        assert!(diagnostics[0].message.contains("булево"));
        assert_diagnostic_range(&file_content, &diagnostics[0], 4, 8, 15);
    }

    #[test]
    fn test_english_keywords() {
        let code =
            "// Description\n// Returns:\n// String - result\nFunction Example()\nEndFunction";
        let (diagnostics, _) = check_diagnostic(code);
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
        let (diagnostics, _file_content) = check_diagnostic(code);

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
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 1, "Should have one diagnostic");

        // Extract actual highlighted text
        let range = diagnostics[0].range;
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        let highlighted_text = &file_content[start..end];

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
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 1, "Should have one diagnostic");

        // Extract actual highlighted text
        let range = diagnostics[0].range;
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        let highlighted_text = &file_content[start..end];

        // Should highlight only the function name
        assert_eq!(
            highlighted_text, "ЗапросВERP",
            "Should highlight only function name 'ЗапросВERP', got '{}'",
            highlighted_text
        );
    }
}
