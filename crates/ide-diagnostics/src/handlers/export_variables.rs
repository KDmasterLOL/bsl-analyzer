//! ExportVariables diagnostic
//!
//! Detects exported module variables.
//!
//! **Source (Java):** bsl-language-server/ExportVariablesDiagnostic.java
//! **Source (Rust tree-sitter):** bsl-language-server-rust/rules/export_variables.rs
//!
//! Exported module variables are considered bad practice because they create
//! tight coupling and make code harder to maintain. Use getter/setter methods instead.
//!
//! ## Bad practice
//! ```bsl
//! Перем МояПеременная Экспорт;  // Exported variable
//! ```
//!
//! ## Good practice
//! ```bsl
//! Перем МояПеременная;  // Private variable
//!
//! Функция ПолучитьМояПеременная() Экспорт
//!     Возврат МояПеременная;
//! КонецФункции
//!
//! Процедура УстановитьМояПеременная(Значение) Экспорт
//!     МояПеременная = Значение;
//! КонецПроцедуры
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{NodeOrToken, SyntaxKind, SyntaxToken};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::ExportVariables) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Traverse all VAR_DEF nodes looking for exported variables
    // Skip those inside functions/procedures (they can't be exported anyway)
    for node in root.descendants() {
        if node.kind() == SyntaxKind::VAR_DEF {
            // Check if inside a function/procedure
            let inside_method = node.ancestors().any(|ancestor| {
                matches!(ancestor.kind(), SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF)
            });

            if !inside_method {
                check_var_def(&node, &mut diagnostics);
            }
        }
    }

    diagnostics
}

/// Check a VAR_DEF node for exported variables
fn check_var_def(var_def: &syntax::SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    let tokens: Vec<_> = var_def.children_with_tokens().collect();

    // Look for IDENT + KW_EXPORT patterns
    for (i, element) in tokens.iter().enumerate() {
        if let Some(token) = element.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                // Check if this identifier is followed by KW_EXPORT
                if let Some(export_token) = find_next_export(&tokens[i + 1..]) {
                    let range =
                        ide_db::TextRange::cover(token.text_range(), export_token.text_range());

                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::ExportVariables,
                        message: "It is recommended not to use global variables. They often might cause issues that cannot be easily located".to_string(),
                        severity: Severity::Warning,
                        range,
                        tags: vec![],
                        fixes: vec![],
                    });
                }
            }
        }
    }
}

/// Find the next KW_EXPORT token, skipping whitespace
/// Returns the export token if found and it's the immediate next semantic token
fn find_next_export(
    remaining: &[NodeOrToken<syntax::SyntaxNode, syntax::SyntaxToken>],
) -> Option<SyntaxToken> {
    for item in remaining {
        if let Some(token) = item.as_token() {
            match token.kind() {
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => continue,
                SyntaxKind::KW_EXPORT => return Some(token.clone()),
                SyntaxKind::COMMA | SyntaxKind::SEMICOLON => return None,
                _ => return None,
            }
        } else if let Some(_node) = item.as_node() {
            // Hit a node, not an export
            return None;
        }
    }
    None
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

    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let config = Rc::new(DiagnosticsConfig::default());
        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_no_export() {
        let code = r#"
Перем МояПеременная;

Процедура Инициализация()
    МояПеременная = 0;
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Private variable should not trigger diagnostic");
    }

    #[test]
    fn test_simple_export() {
        let code = r#"Перем МояПеременная Экспорт;"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Exported variable should trigger diagnostic");
    }

    #[test]
    fn test_inside_procedure() {
        let code = r#"
Процедура Тест()
    Перем ПеременнаяМодуля, ПеременнаяЭкспорт;
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        // Variables inside procedures cannot be exported
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_bilingual() {
        let code_ru = r#"Перем МояПеременная Экспорт;"#;
        let diagnostics_ru = check_diagnostic(code_ru);
        assert_eq!(diagnostics_ru.len(), 1, "Russian keyword should work");

        let code_en = r#"Var MyVariable Export;"#;
        let diagnostics_en = check_diagnostic(code_en);
        assert_eq!(diagnostics_en.len(), 1, "English keyword should work");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/ExportVariablesDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        // Should find 2 exported variables (Перем1 and Перем53)
        assert_eq!(diagnostics.len(), 2, "Expected 2 exported variables");

        // Diagnostic 0: Перем1 Экспорт on line 0
        // Line 0: "Перем Перем1 Экспорт;"
        //         012345678901234567890
        //               ^     ^
        //               6     20
        assert_diagnostic_range(code, &diagnostics[0], 0, 6, 20);

        // Diagnostic 1: Перем53 Экспорт on line 2
        // Line 2: "Перем Перем53 Экспорт;"
        //         012345678901234567890
        //               ^       ^
        //               6       21
        assert_diagnostic_range(code, &diagnostics[1], 2, 6, 21);
    }
}
