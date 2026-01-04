//! DeprecatedMessage diagnostic.
//!
//! Detects usage of deprecated global `Сообщить()` / `Message()` methods.
//!
//! ## Why?
//! The global `Сообщить()` / `Message()` method is deprecated:
//! - Low level API without structured logging
//! - No severity levels or categorization
//! - Output goes to user messages which may be inappropriate
//! - Better alternatives exist for different scenarios
//!
//! ## Bad practice
//! ```bsl
//! Процедура Тест()
//!     Сообщить("Операция выполнена"); // ❌ Global Сообщить() is deprecated
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Тест()
//!     // ✅ For user notifications - use ОбщегоНазначения.СообщитьПользователю()
//!     ОбщегоНазначения.СообщитьПользователю("Операция выполнена");
//!
//!     // ✅ For logging - use ЗаписьЖурналаРегистрации()
//!     ЗаписьЖурналаРегистрации("ИмяСобытия", УровеньЖурналаРегистрации.Информация);
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Information (MINOR)
//! - **Tags:** STANDARD, DEPRECATED
//! - **Minutes to fix:** 2
//!
//! ## Implementation
//! Ported from:
//! - DeprecatedMessageDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxToken};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::DeprecatedMessage) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();
    let mut seen_ranges = std::collections::HashSet::new();

    // ✅ OPTIMIZATION: Collect tokens ONCE instead of O(N²) nested tree traversal
    let tokens: Vec<_> = root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    // Find all global method calls (IDENT + LPAREN without preceding DOT)
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
                    let method_name = token.text().to_string();
                    if is_deprecated_message(&method_name) {
                        let diagnostic = create_diagnostic(token, &method_name);
                        if seen_ranges.insert(diagnostic.range) {
                            diagnostics.push(diagnostic);
                        }
                    }
                }
            }
        }
    }

    diagnostics
}

fn is_deprecated_message(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "сообщить" || lower == "message"
}

fn create_diagnostic(token: &SyntaxToken, method_name: &str) -> Diagnostic {
    let message = get_message(method_name);
    let range = token.text_range();

    Diagnostic {
        code: DiagnosticCode::DeprecatedMessage,
        message,
        severity: Severity::Information,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

fn get_message(method_name: &str) -> String {
    let lower = method_name.to_lowercase();
    if lower == "сообщить" {
        "Используйте \"ОбщегоНазначения.СообщитьПользователю\" вместо устаревшего \"Сообщить\""
            .to_string()
    } else {
        "Use \"CommonUse.MessageToUser\" instead of deprecated \"Message\"".to_string()
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
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_deprecated_russian() {
        let code = r#"
Процедура Тест()
    Сообщить("Операция выполнена");
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedMessage);
        assert_eq!(diagnostics[0].severity, Severity::Information);
    }

    #[test]
    fn test_deprecated_english() {
        let code = r#"
Procedure Test()
    Message("Operation completed");
EndProcedure
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DeprecatedMessage);
    }

    #[test]
    fn test_object_method_excluded() {
        let code = r#"
Процедура Тест()
    Модуль.Сообщить("Операция выполнена");
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    СООБЩИТЬ("A");
    сообщить("B");
    Сообщить("C");
    СообЩить("D");
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 4);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/DeprecatedMessageDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(input);

        assert_eq!(diagnostics.len(), 2, "Expected 2 diagnostics");

        assert_diagnostic_range(&file_content, &diagnostics[0], 4, 8, 15);

        assert_diagnostic_range(&file_content, &diagnostics[1], 10, 0, 8);
    }
}
