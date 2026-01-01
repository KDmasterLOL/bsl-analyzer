//! EmptyRegion diagnostic.
//!
//! Detects empty code regions that contain only comments, whitespace, or nested empty regions.
//!
//! ## Why?
//! Empty regions serve no purpose and clutter the code. They should either contain
//! meaningful code or be removed entirely.
//!
//! ## Bad practice
//! ```bsl
//! #Область ПустаяОбласть
//! // Только комментарий
//! #КонецОбласти
//! ```
//!
//! ## Good practice
//! ```bsl
//! #Область ПолезнаяОбласть
//! Перем Счетчик;
//! #КонецОбласти
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Information (INFO)
//! - **Tags:** STANDARD
//! - **Minutes to fix:** 1
//!
//! ## Nested Regions
//! Handles nested empty regions correctly:
//! - Reports both inner and outer if both empty
//! - Reports only inner if outer has code
//!
//! ## Implementation
//! Ported from:
//! - EmptyRegionDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - empty_region.rs (bsl-language-server-rust) - Algorithm reference
//!
//! Adapted to use Rowan SyntaxNode and PreRegionDir AST helper.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use syntax::{ast::AstNode, ast::PreRegionDir, SyntaxKind, SyntaxNode};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("EmptyRegion::check").entered();

    if ctx.config.is_disabled(DiagnosticCode::EmptyRegion) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() == SyntaxKind::PRE_REGION_DIR {
            if let Some(region) = PreRegionDir::cast(node.clone()) {
                if is_empty_region(&node) {
                    if let Some(name) = region.name() {
                        diagnostics.push(create_diagnostic(name, node.text_range()));
                    }
                }
            }
        }
    }

    tracing::debug!(count = diagnostics.len(), "EmptyRegion diagnostics found");
    diagnostics
}

fn is_empty_region(region_node: &SyntaxNode) -> bool {
    for child in region_node.children() {
        if is_meaningful_content(&child) {
            return false;
        }
        if child.kind() == SyntaxKind::PRE_REGION_DIR && !is_empty_region(&child) {
            return false;
        }
    }
    true
}

fn is_meaningful_content(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::PROCEDURE_DEF
            | SyntaxKind::FUNCTION_DEF
            | SyntaxKind::VAR_DEF
            | SyntaxKind::ASSIGN_STMT
            | SyntaxKind::CALL_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::RAISE_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::GOTO_STMT
            | SyntaxKind::LABEL_STMT
            | SyntaxKind::EXECUTE_STMT
            | SyntaxKind::ADD_HANDLER_STMT
            | SyntaxKind::REMOVE_HANDLER_STMT
    )
}

fn create_diagnostic(region_name: String, range: TextRange) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::EmptyRegion,
        message: format!("Область '{}' пуста", region_name),
        severity: Severity::Information,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        test_utils::assert_diagnostic_range_multiline, DiagnosticsConfig, DiagnosticsContext,
    };
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
    fn test_comprehensive() {
        let code = include_str!("../../test_data/EmptyRegionDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 3, "Should match Java: 3 diagnostics");

        assert_diagnostic_range_multiline(code, &diagnostics[0], 0, 0, 2, 13);
        assert!(diagnostics[0].message.contains("Тест"));

        assert_diagnostic_range_multiline(code, &diagnostics[1], 10, 0, 15, 13);
        assert!(diagnostics[1].message.contains("ВнешняяОбласть"));

        assert_diagnostic_range_multiline(code, &diagnostics[2], 12, 0, 14, 13);
        assert!(diagnostics[2].message.contains("ВнутренняяОбласть"));
    }

    #[test]
    fn test_region_with_variables() {
        let code = r#"
#Область Переменные
Перем А;
#КонецОбласти
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Region with variable is not empty");
    }

    #[test]
    fn test_region_with_function() {
        let code = r#"
#Область ПрограммныйИнтерфейс
Функция Тест()
КонецФункции
#КонецОбласти
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Region with function is not empty");
    }

    #[test]
    fn test_nested_both_empty() {
        let code = r#"
#Область Внешняя
    #Область Внутренняя
    #КонецОбласти
#КонецОбласти
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 2, "Both nested empty regions reported");
    }

    #[test]
    fn test_nested_outer_has_code() {
        let code = r#"
#Область Внешняя
    Перем А;
    #Область Внутренняя
    #КонецОбласти
#КонецОбласти
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Only inner empty region reported");
        assert!(diagnostics[0].message.contains("Внутренняя"));
    }

    #[test]
    fn test_bilingual_keywords() {
        let code = r#"
#Region Test
// comment only
#EndRegion

#Область Тест
// comment only
#КонецОбласти
        "#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 2, "Both English and Russian empty regions reported");
    }
}
