//! FunctionNameStartsWithGet diagnostic
//!
//! Detects functions with names starting with "Получить" (Russian for "Get").
//!
//! **Source (Java):** bsl-language-server/FunctionNameStartsWithGetDiagnostic.java
//! **Source (Rust tree-sitter):** bsl-language-server-rust/rules/function_name_starts_with_get.rs
//!
//! ## Why?
//! Function names starting with "Получить" are considered a code smell in 1C:Enterprise.
//! According to 1C coding standards, such names should be avoided and replaced with more
//! descriptive alternatives that don't use the "Получить" prefix.
//!
//! **Note:** This diagnostic only checks Russian "Получить" prefix, not English "Get".
//! This matches the behavior of bsl-language-server (Java implementation).
//!
//! ## Bad practice
//! ```bsl
//! Функция ПолучитьИмяПоКоду()  // Bad!
//!     Возврат "Имя";
//! КонецФункции
//! ```
//!
//! ## Good practice
//! ```bsl
//! Функция ИмяПоКоду()  // Good!
//!     Возврат "Имя";
//! КонецФункции
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{ast::AstNode, SyntaxKind};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::FunctionNameStartsWithGet) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Traverse all nodes looking for FUNCTION_DEF
    for node in root.descendants() {
        if node.kind() == SyntaxKind::FUNCTION_DEF {
            // Cast to FunctionDef AST node
            if let Some(func) = syntax::ast::FunctionDef::cast(node) {
                // Get function name
                if let Some(name_token) = func.name() {
                    let name_text = name_token.text();

                    // Check if name starts with "Получить" (case-insensitive)
                    // Java uses: Pattern get = CaseInsensitivePattern.compile("^Получить.*$");
                    if name_text.to_lowercase().starts_with("получить") {
                        diagnostics.push(Diagnostic {
                            code: DiagnosticCode::FunctionNameStartsWithGet,
                            message: format!(
                                "Имя функции '{}' не должно начинаться с 'Получить'",
                                name_text
                            ),
                            severity: Severity::Information,
                            range: name_token.text_range(),
                            tags: vec![],
                            fixes: vec![],
                        });
                    }
                }
            }
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range;
    use crate::{DiagnosticsConfig, DiagnosticsContext};
    use ide_db::base_db::SourceDatabase;
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        let config = Rc::new(DiagnosticsConfig::default());
        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_function_name_starts_with_get() {
        let code = include_str!("../test_data/FunctionNameStartsWithGetDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        // Java expects 1 diagnostic at line 0 (1-based line 1), cols 8-25
        // The diagnostic should be on "ПолучитьИмяПоКоду" (the function name)
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");

        // Line 1 (0-indexed line 1, after source comment), cols 8-25
        // "Функция ПолучитьИмяПоКоду()" - the name starts at col 8
        assert_diagnostic_range(&file_content, &diagnostics[0], 1, 8, 25);
        assert!(diagnostics[0].message.contains("ПолучитьИмяПоКоду"));
    }

    #[test]
    fn test_no_get_prefix() {
        let code = r#"
Функция ИмяПоКоду()
    Возврат "Имя";
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Should not detect functions without 'Получить' prefix");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Функция ПОЛУЧИТЬДАННЫЕ()
    Возврат "Данные";
КонецФункции

Функция получитьзначение()
    Возврат 42;
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 2, "Should detect case-insensitive 'Получить' variations");
    }

    #[test]
    fn test_procedure_not_detected() {
        let code = r#"
Процедура ПолучитьИмяПоКоду()
    // Процедура не должна срабатывать
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Should NOT detect procedures");
    }

    #[test]
    fn test_english_get_not_detected() {
        let code = r#"
Function GetNameByCode()
    Return "Name";
EndFunction
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(
            diagnostics.len(),
            0,
            "Should NOT detect English 'Get' prefix (only Russian 'Получить')"
        );
    }

    #[test]
    fn test_partial_match_not_detected() {
        let code = r#"
Функция НеПолучитьИмяПоКоду()
    Возврат "Имя";
КонецФункции
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(
            diagnostics.len(),
            0,
            "Should NOT detect names that don't START with 'Получить'"
        );
    }
}
