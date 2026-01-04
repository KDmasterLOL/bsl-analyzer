use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::NestedTernaryOperator) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::IF_STMT => {
                if let Some(condition) = find_if_condition(&node) {
                    find_and_report_ternaries(&condition, &mut diagnostics);
                }
            }
            SyntaxKind::ELSIF_CLAUSE => {
                if let Some(condition) = find_elsif_condition(&node) {
                    find_and_report_ternaries(&condition, &mut diagnostics);
                }
            }
            SyntaxKind::TERNARY_EXPR => {
                for nested in node.descendants().skip(1) {
                    if nested.kind() == SyntaxKind::TERNARY_EXPR {
                        diagnostics.push(make_diagnostic(&nested));
                    }
                }
            }
            _ => {}
        }
    }

    diagnostics
}

fn find_if_condition(if_stmt: &SyntaxNode) -> Option<SyntaxNode> {
    if_stmt.children().find(|n| n.kind() == SyntaxKind::EXPR)
}

fn find_elsif_condition(elsif_clause: &SyntaxNode) -> Option<SyntaxNode> {
    elsif_clause.children().find(|n| n.kind() == SyntaxKind::EXPR)
}

fn find_and_report_ternaries(condition: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    for node in condition.descendants() {
        if node.kind() == SyntaxKind::TERNARY_EXPR {
            diagnostics.push(make_diagnostic(&node));
        }
    }
}

fn make_diagnostic(node: &SyntaxNode) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::NestedTernaryOperator,
        message: "Не рекомендуется использовать вложенный тернарный оператор".to_string(),
        severity: Severity::Warning,
        range: node.text_range(),
        tags: vec![],
        fixes: vec![],
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

        (check(&ctx), file_content)
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/NestedTernaryOperatorDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(
            diagnostics.len(),
            4,
            "Should find exactly 4 diagnostics (matching Java implementation)"
        );

        assert_diagnostic_range_multiline(&file_content, &diagnostics[0], 2, 13, 8, 14);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[1], 13, 5, 13, 50);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[2], 13, 73, 13, 104);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[3], 22, 12, 22, 71);
    }

    #[test]
    fn test_no_diagnostic_for_simple_ternary() {
        let code = r#"
Результат = ?(Условие, Истина, Ложь);
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert!(diagnostics.is_empty(), "Simple ternary should not trigger diagnostic");
    }

    #[test]
    fn test_nested_ternary_in_assignment() {
        let code = r#"
Результат = ?(Условие1, ?(Условие2, 1, 2), 3);
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Nested ternary in assignment should trigger diagnostic");
    }

    #[test]
    fn test_ternary_in_if_condition() {
        let code = r#"
Если ?(А, Б, В) = 1 Тогда
    Х = 1;
КонецЕсли;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Ternary in if condition should trigger diagnostic");
    }

    #[test]
    fn test_ternary_in_elsif_condition() {
        let code = r#"
Если Условие Тогда
    Х = 1;
ИначеЕсли ?(А, Б, В) = 1 Тогда
    Х = 2;
КонецЕсли;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Ternary in elsif condition should trigger diagnostic");
    }

    #[test]
    fn test_disabled() {
        let code = r#"
Результат = ?(Условие1, ?(Условие2, 1, 2), 3);
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::NestedTernaryOperator);

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
        assert!(diagnostics.is_empty(), "Disabled diagnostic should not find anything");
    }
}
