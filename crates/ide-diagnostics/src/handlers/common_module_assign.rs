//! CommonModuleAssign diagnostic
//!
//! Cannot assign value to CommonModule (will cause runtime error).
//!
//! Ported from: CommonModuleAssignDiagnostic.java

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use bsl_metadata::traits::MdObject;
use syntax::ast::{AssignStmt, AstNode, FieldExpr, IndexExpr};
use syntax::SyntaxKind;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CommonModuleAssign) {
        return Vec::new();
    }

    let config_path = match ctx.configuration_path.or(ctx.workspace_root) {
        Some(path) => path,
        None => return Vec::new(),
    };

    let config_path_str = config_path.to_string_lossy().to_string();
    let path_input = ide_db::metadata::ConfigurationPathInput::new(ctx.db, config_path_str);
    let configuration = ide_db::metadata::load_configuration(ctx.db, path_input);

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if let Some(assign_stmt) = AssignStmt::cast(node) {
            if let Some(identifier) = extract_simple_identifier(&assign_stmt) {
                let module_exists = configuration
                    .common_modules()
                    .iter()
                    .any(|m| m.name().to_lowercase() == identifier.to_lowercase());

                if module_exists {
                    let lvalue_node = assign_stmt
                        .syntax()
                        .children()
                        .find(|n| n.kind() != SyntaxKind::EQ && !n.kind().is_trivia())
                        .unwrap_or_else(|| assign_stmt.syntax().clone());

                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::CommonModuleAssign,
                        message: format!(
                            "Недопустимо присваивание значения общему модулю '{}'",
                            identifier
                        ),
                        severity: Severity::Error,
                        range: lvalue_node.text_range(),
                        tags: vec![],
                        fixes: vec![],
                    });
                }
            }
        }
    }

    diagnostics
}

fn extract_simple_identifier(assign_stmt: &AssignStmt) -> Option<String> {
    let lvalue = assign_stmt.syntax().children().next()?;

    if FieldExpr::can_cast(lvalue.kind()) || IndexExpr::can_cast(lvalue.kind()) {
        return None;
    }

    if lvalue.kind() == SyntaxKind::IDENT {
        return Some(lvalue.text().to_string());
    }

    lvalue.first_token().and_then(|t| {
        if t.kind() == SyntaxKind::IDENT {
            Some(t.text().to_string())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();
        db.set_file_text(file_id, code);
        let db = Rc::new(db) as Rc<dyn RootDatabase>;

        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_no_workspace() {
        let code = r#"
Перем СвойМодуль;
СвойМодуль = 1;
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_property_access() {
        let code = r#"
СвойМодуль.Свойство = 1;
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }
}
