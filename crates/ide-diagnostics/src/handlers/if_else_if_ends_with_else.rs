//! IfElseIfEndsWithElse diagnostic
//!
//! Detects if-elseif chains that don't end with else clause.
//!
//! ## Why?
//! If-elseif chains without else can lead to unhandled cases:
//! - All possible branches should be covered
//! - Else clause makes code intentions explicit
//! - Prevents silent bugs from unhandled conditions
//! - Better code readability
//!
//! ## Bad practice
//! ```bsl
//! Процедура Тест(Значение)
//!     Если Значение = 1 Тогда
//!         // ...
//!     ИначеЕсли Значение = 2 Тогда
//!         // ...
//!     КонецЕсли; // Missing else!
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Тест(Значение)
//!     Если Значение = 1 Тогда
//!         // ...
//!     ИначеЕсли Значение = 2 Тогда
//!         // ...
//!     Иначе
//!         // Handle other cases
//!     КонецЕсли;
//! КонецПроцедуры
//! ```
//!
//! ## Source
//! Source: bsl-language-server/src/main/java/.../diagnostics/IfElseIfEndsWithElseDiagnostic.java
//! Source: bsl-language-server-rust/crates/bsl-diagnostics/src/rules/if_else_if_ends_with_else.rs

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::IfElseIfEndsWithElse) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() == SyntaxKind::IF_STMT {
            check_if_statement(&node, &mut diagnostics);
        }
    }

    diagnostics
}

fn check_if_statement(if_stmt: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    let mut has_elsif = false;
    let mut has_else = false;

    // Check children for elsif and else clauses
    for child in if_stmt.children() {
        if child.kind() == SyntaxKind::ELSIF_CLAUSE {
            has_elsif = true;
        } else if child.kind() == SyntaxKind::ELSE_CLAUSE {
            has_else = true;
        }
    }

    // If has elsif but no else - report diagnostic on KW_END_IF token
    if has_elsif && !has_else {
        // Find KW_END_IF token
        let endif_token = if_stmt
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == SyntaxKind::KW_END_IF);

        // Get the range for the diagnostic
        let range = if let Some(token) = endif_token {
            token.text_range()
        } else {
            // Fallback: use the last token of the if statement
            if_stmt.last_token().map(|t| t.text_range()).unwrap_or(if_stmt.text_range())
        };

        diagnostics.push(Diagnostic {
            code: DiagnosticCode::IfElseIfEndsWithElse,
            message: "Конструкция Если-ИначеЕсли должна заканчиваться блоком Иначе".to_string(),
            severity: Severity::Major,
            range,
            tags: vec![],
            fixes: vec![],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
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
        (diagnostics, fixture.files[&file_id].content.to_string())
    }

    #[test]
    fn test_if_elsif_without_else() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    ИначеЕсли Значение = 2 Тогда
        Сообщить("Два");
    КонецЕсли;
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);

        // Should detect missing else
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::IfElseIfEndsWithElse);
    }

    #[test]
    fn test_if_elsif_with_else() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    ИначеЕсли Значение = 2 Тогда
        Сообщить("Два");
    Иначе
        Сообщить("Другое");
    КонецЕсли;
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);

        // Should not detect - has else
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_simple_if_without_elsif() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    КонецЕсли;
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);

        // Should not detect - no elsif
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_if_else_without_elsif() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    Иначе
        Сообщить("Другое");
    КонецЕсли;
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);

        // Should not detect - no elsif
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_multiple_elsif_without_else() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    ИначеЕсли Значение = 2 Тогда
        Сообщить("Два");
    ИначеЕсли Значение = 3 Тогда
        Сообщить("Три");
    КонецЕсли;
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);

        // Should detect missing else
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_multiple_if_statements() {
        let code = r#"Процедура Тест(Значение)
    Если Значение = 1 Тогда
        Сообщить("Один");
    ИначеЕсли Значение = 2 Тогда
        Сообщить("Два");
    КонецЕсли;

    Если Значение = 3 Тогда
        Сообщить("Три");
    ИначеЕсли Значение = 4 Тогда
        Сообщить("Четыре");
    Иначе
        Сообщить("Другое");
    КонецЕсли;
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);

        // Should detect only first if (missing else)
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_nested_if_elsif() {
        let code = r#"Процедура Тест(Значение1, Значение2)
    Если Значение1 = 1 Тогда
        Если Значение2 = 1 Тогда
            Сообщить("1-1");
        ИначеЕсли Значение2 = 2 Тогда
            Сообщить("1-2");
        КонецЕсли;
    ИначеЕсли Значение1 = 2 Тогда
        Сообщить("2");
    КонецЕсли;
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);

        // Should detect both (nested and outer)
        assert_eq!(diagnostics.len(), 2);
    }

    /// Test with actual fixture file from bsl-language-server
    /// Expected: 1 diagnostic at line 20, columns 0-9 (КонецЕсли)
    #[test]
    fn test_if_else_if_ends_with_else() {
        let code = include_str!("../../tests/fixtures/IfElseIfEndsWithElseDiagnostic.bsl");

        let (diagnostics, file_content) = check_diagnostic(code);

        // Java test expects: assertThat(diagnostics).hasSize(1);
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic");

        // Verify the diagnostic range matches Java implementation
        // Java: assertThat(diagnostics, true).hasRange(20, 0, 20, 9);
        assert_diagnostic_range(&file_content, &diagnostics[0], 20, 0, 9);
    }
}
