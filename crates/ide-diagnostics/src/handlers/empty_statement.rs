//! EmptyStatement diagnostic
//!
//! Detects empty statements (standalone semicolons) in code.
//!
//! **Source (Java):** bsl-language-server/EmptyStatementDiagnostic.java
//! **Source (Rust tree-sitter):** bsl-language-server-rust/rules/empty_statement.rs
//!
//! Empty statements are usually typos or leftover from refactoring.
//! They make code less readable and can be confusing.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::SyntaxKind;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::EmptyStatement) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Traverse all nodes looking for EMPTY_STMT
    for node in root.descendants() {
        if node.kind() == SyntaxKind::EMPTY_STMT {
            // Check if parent or siblings contain ERROR nodes
            // (Java: !Trees.treeContainsErrors(previousNode))
            let has_error = node
                .parent()
                .map(|p| p.children().any(|c| c.kind() == SyntaxKind::ERROR))
                .unwrap_or(false);

            if !has_error {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::EmptyStatement,
                    message: "Empty statement".to_string(),
                    severity: Severity::Information,
                    range: node.text_range(),
                    tags: vec![],
                    fixes: vec![],
                });
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
            configuration_path_input: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_empty_statement() {
        let code = include_str!("../../test_data/EmptyStatementDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        // Java expects 2 diagnostics
        assert_eq!(diagnostics.len(), 2, "Expected 2 diagnostics");

        // Line 1 (0-indexed), cols 18-19: semicolon after "Тогда"
        assert_diagnostic_range(code, &diagnostics[0], 1, 18, 19);

        // Line 2 (0-indexed), cols 8-9: second semicolon in ";;"
        assert_diagnostic_range(code, &diagnostics[1], 2, 8, 9);
    }

    #[test]
    fn test_no_empty_statements() {
        let code = r#"
Процедура Тест()
    А = 1;
    Б = 2;
    Возврат А + Б;
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_double_semicolon() {
        let code = r#"
Процедура Тест()
    А = 1;;
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Expected 1 empty statement");
    }
}
