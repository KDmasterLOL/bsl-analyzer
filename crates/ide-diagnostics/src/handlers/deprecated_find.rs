//! DeprecatedFind diagnostic.
//!
//! Detects usage of deprecated global `Найти()` / `Find()` methods.
//!
//! ## Why?
//! The global `Найти()` / `Find()` method is deprecated since 1C:Enterprise 8.3.6:
//! - Ambiguous name (conflicts with collection methods)
//! - Use `СтрНайти()` / `StrFind()` for string search instead
//! - Use collection's `.Найти()` method for collections
//! - Better code clarity and type safety
//!
//! ## Bad practice
//! ```bsl
//! Процедура Тест()
//!     Позиция = Найти("Строка", "о"); // ❌ Global Найти() is deprecated
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Тест()
//!     // ✅ For string search - use СтрНайти()
//!     Позиция = СтрНайти("Строка", "о");
//!
//!     // ✅ For collection search - use collection method
//!     Индекс = Массив.Найти("Элемент");
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Information (MINOR)
//! - **Tags:** DEPRECATED
//! - **Minutes to fix:** 2
//!
//! ## Implementation
//! Ported from:
//! - DeprecatedFindDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - deprecated_find.rs (bsl-language-server-rust) - Rust reference
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::DeprecatedFind) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();
    let mut seen_ranges = std::collections::HashSet::new();

    for node in root.descendants() {
        if let Some(diagnostic) = check_call(&node) {
            if seen_ranges.insert(diagnostic.range) {
                diagnostics.push(diagnostic);
            }
        }
    }

    diagnostics
}

fn check_call(node: &SyntaxNode) -> Option<Diagnostic> {
    let (method_name_token, method_name) = extract_method_name(node)?;

    if !is_deprecated_find(&method_name) {
        return None;
    }

    Some(create_diagnostic(&method_name_token, &method_name))
}

fn extract_method_name(node: &SyntaxNode) -> Option<(SyntaxToken, String)> {
    let has_arg_list = node.descendants().any(|n| n.kind() == SyntaxKind::ARG_LIST);

    if !has_arg_list {
        return None;
    }

    let tokens: Vec<_> = node.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    let mut method_name_token: Option<SyntaxToken> = None;

    for (i, token) in tokens.iter().enumerate() {
        if token.kind() == SyntaxKind::IDENT {
            let next_is_lparen =
                tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

            if next_is_lparen {
                let prev_is_dot = i
                    .checked_sub(1)
                    .and_then(|idx| tokens.get(idx))
                    .map(|t| t.kind() == SyntaxKind::DOT)
                    .unwrap_or(false);

                if !prev_is_dot {
                    method_name_token = Some(token.clone());
                    break;
                }
            }
        }
    }

    method_name_token.map(|t| {
        let text = t.text().to_string();
        (t, text)
    })
}

fn is_deprecated_find(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "найти" || lower == "find"
}

fn create_diagnostic(token: &SyntaxToken, method_name: &str) -> Diagnostic {
    let message = get_message(method_name);
    let range = token.text_range();

    Diagnostic {
        code: DiagnosticCode::DeprecatedFind,
        message,
        severity: Severity::Information,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

fn get_message(method_name: &str) -> String {
    let lower = method_name.to_lowercase();
    if lower == "найти" {
        "Используйте \"СтрНайти\" вместо устаревшего \"Найти\"".to_string()
    } else {
        "Use \"StrFind\" instead of deprecated \"Find\"".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_deprecated_russian() {
        let code = r#"
Процедура Тест()
    Позиция = Найти("Строка", "о");
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedFind);
        assert_eq!(diagnostics[0].severity, Severity::Information);
    }

    #[test]
    fn test_deprecated_english() {
        let code = r#"
Procedure Test()
    Position = Find("String", "S");
EndProcedure
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedFind);
    }

    #[test]
    fn test_collection_method_excluded() {
        let code = r#"
Процедура Тест()
    Индекс = Массив.Найти("Элемент");
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Поз1 = НАЙТИ("A", "B");
    Поз2 = найти("C", "D");
    Поз3 = Найти("E", "F");
    Поз4 = НайтИ("G", "H");
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 4);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/DeprecatedFindDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(input);

        assert_eq!(diagnostics.len(), 2, "Expected 2 diagnostics");

        assert_diagnostic_range(&file_content, &diagnostics[0], 3, 8, 13);

        assert_diagnostic_range(&file_content, &diagnostics[1], 9, 3, 7);
    }
}
