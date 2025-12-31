//! CodeOutOfRegion diagnostic.
//!
//! Detects code elements (variables, procedures, functions, statements)
//! located outside of region declarations (#Область/#Region).
//!
//! ## Why?
//! Code should be organized in regions:
//! - Better code structure
//! - Easier navigation in IDE
//! - Follows 1C coding standards
//! - Improves maintainability
//!
//! ## Bad practice
//! ```bsl
//! Перем МодульПеременная;  // Outside region!
//!
//! Процедура Тест()         // Outside region!
//!     Сообщить("OK");
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! #Область ПеременныеМодуля
//! Перем МодульПеременная;
//! #КонецОбласти
//!
//! #Область ПрограммныйИнтерфейс
//! Процедура Тест() Экспорт
//!     Сообщить("OK");
//! КонецПроцедуры
//! #КонецОбласти
//! ```
//!
//! ## Implementation
//!
//! Ported from:
//! - CodeOutOfRegionDiagnostic.java (bsl-language-server) - PRIMARY
//! - code_out_of_region.rs (bsl-language-server-rust) - REFERENCE
//!
//! Adapted to use Rowan SyntaxNode traversal.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CodeOutOfRegion) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();
    check_node(&root, &mut diagnostics);
    diagnostics
}

fn check_node(node: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    for child in node.children() {
        if matches!(
            child.kind(),
            SyntaxKind::PRE_IF_DIR | SyntaxKind::PRE_ELSE_CLAUSE | SyntaxKind::PRE_ELSIF_CLAUSE
        ) {
            check_node(&child, diagnostics);
            continue;
        }

        if is_module_level_element(&child)
            && is_significant_element(&child)
            && !is_inside_region(&child)
        {
            let element_type = match child.kind() {
                SyntaxKind::FUNCTION_DEF => "Функция",
                SyntaxKind::PROCEDURE_DEF => "Процедура",
                SyntaxKind::VAR_DEF => "Переменная",
                _ => "Элемент кода",
            };

            tracing::debug!(
                kind = ?child.kind(),
                range = ?child.text_range(),
                text = %child.text().to_string().lines().next().unwrap_or(""),
                "CodeOutOfRegion: found element outside region"
            );

            diagnostics.push(Diagnostic {
                code: DiagnosticCode::CodeOutOfRegion,
                message: format!(
                    "{} находится вне области (#Область/#Region). \
                     Весь код модуля должен быть организован в области для лучшей структуры.",
                    element_type
                ),
                severity: Severity::Information,
                range: child.text_range(),
                tags: vec![],
                fixes: vec![],
            });
        }
    }
}

fn is_module_level_element(node: &SyntaxNode) -> bool {
    let parent = match node.parent() {
        Some(p) => p,
        None => return false,
    };

    match parent.kind() {
        SyntaxKind::SOURCE_FILE | SyntaxKind::PRE_ELSE_CLAUSE | SyntaxKind::PRE_ELSIF_CLAUSE => {
            true
        }
        SyntaxKind::PRE_IF_DIR => {
            if matches!(node.kind(), SyntaxKind::CALL_STMT | SyntaxKind::ASSIGN_STMT) {
                has_preceding_definition(&parent, node)
            } else {
                true
            }
        }
        _ => false,
    }
}

fn has_preceding_definition(parent: &SyntaxNode, node: &SyntaxNode) -> bool {
    let node_start = node.text_range().start();
    for sibling in parent.children() {
        if sibling.text_range().start() < node_start
            && matches!(
                sibling.kind(),
                SyntaxKind::VAR_DEF
                    | SyntaxKind::PROCEDURE_DEF
                    | SyntaxKind::FUNCTION_DEF
                    | SyntaxKind::PRE_REGION_DIR
            )
        {
            return true;
        }
    }
    false
}

fn is_significant_element(node: &SyntaxNode) -> bool {
    match node.kind() {
        SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF | SyntaxKind::VAR_DEF => true,

        SyntaxKind::ASSIGN_STMT
        | SyntaxKind::CALL_STMT
        | SyntaxKind::IF_STMT
        | SyntaxKind::WHILE_STMT
        | SyntaxKind::FOR_STMT
        | SyntaxKind::FOR_EACH_STMT
        | SyntaxKind::TRY_STMT
        | SyntaxKind::RETURN_STMT
        | SyntaxKind::BREAK_STMT
        | SyntaxKind::CONTINUE_STMT
        | SyntaxKind::GOTO_STMT
        | SyntaxKind::EXECUTE_STMT
        | SyntaxKind::ADD_HANDLER_STMT
        | SyntaxKind::REMOVE_HANDLER_STMT => true,

        SyntaxKind::RAISE_STMT => false,

        SyntaxKind::PRE_REGION_DIR => contains_executable_code(node),

        _ => false,
    }
}

fn contains_executable_code(node: &SyntaxNode) -> bool {
    node.descendants().any(|n| match n.kind() {
        SyntaxKind::CALL_STMT
        | SyntaxKind::ASSIGN_STMT
        | SyntaxKind::IF_STMT
        | SyntaxKind::WHILE_STMT
        | SyntaxKind::FOR_STMT
        | SyntaxKind::FOR_EACH_STMT
        | SyntaxKind::TRY_STMT
        | SyntaxKind::RETURN_STMT
        | SyntaxKind::BREAK_STMT
        | SyntaxKind::CONTINUE_STMT => true,
        SyntaxKind::RAISE_STMT => false,
        _ => false,
    })
}

fn is_inside_region(node: &SyntaxNode) -> bool {
    node.ancestors().any(|ancestor| {
        if ancestor.kind() == SyntaxKind::PRE_REGION_DIR {
            if let Some(region_parent) = ancestor.parent() {
                matches!(
                    region_parent.kind(),
                    SyntaxKind::SOURCE_FILE
                        | SyntaxKind::PRE_IF_DIR
                        | SyntaxKind::PRE_ELSE_CLAUSE
                        | SyntaxKind::PRE_ELSIF_CLAUSE
                )
            } else {
                false
            }
        } else {
            false
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
        };

        check(&ctx)
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/CodeOutOfRegionDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 7, "Java expects 7 diagnostics");
    }

    #[test]
    fn test_empty_file() {
        let code = include_str!("../../test_data/CodeOutOfRegionDiagnosticEmptyFile.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_regions() {
        let code = include_str!("../../test_data/CodeOutOfRegionDiagnosticNoRegions.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 6);
    }

    #[test]
    fn test_standard_preproc() {
        let code = include_str!("../../test_data/CodeOutOfRegionDiagnosticStandartPreproc.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_execute() {
        let code = include_str!("../../test_data/CodeOutOfRegionDiagnosticExecute.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_code_block() {
        let code = include_str!("../../test_data/CodeOutOfRegionDiagnosticCodeBlock.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_code_in_region() {
        let code = r#"
#Область ПрограммныйИнтерфейс

Процедура Тест() Экспорт
    Сообщить("OK");
КонецПроцедуры

#КонецОбласти
"#;

        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_code_outside_region() {
        let code = r#"
Процедура Тест()
    Сообщить("OK");
КонецПроцедуры
"#;

        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }
}
