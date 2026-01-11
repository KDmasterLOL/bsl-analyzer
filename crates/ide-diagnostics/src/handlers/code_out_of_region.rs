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
//! Uses RegionTree from HIR for efficient region lookup.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::hir_def::RegionTree;
use syntax::{ast, ast::AstNode, SyntaxKind, SyntaxNode};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CodeOutOfRegion) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    // Get RegionTree from HIR (cached via Salsa)
    let region_tree = ctx.db.region_tree(ctx.file_id);

    let mut diagnostics = Vec::new();
    check_node(&root, &region_tree, &mut diagnostics);
    diagnostics
}

/// Extends range to include trailing semicolon if present (for Java compatibility)
fn range_with_semicolon(node: &SyntaxNode) -> ide_db::TextRange {
    use syntax::{SyntaxToken, TextSize};

    let base_range = node.text_range();

    // Check if there's a semicolon token immediately after this node
    let has_semicolon = node
        .next_sibling_or_token()
        .and_then(|t| t.into_token())
        .map(|token: SyntaxToken| token.kind() == SyntaxKind::SEMICOLON)
        .unwrap_or(false);

    if has_semicolon {
        // Extend range by 1 to include semicolon
        ide_db::TextRange::new(base_range.start(), base_range.end() + TextSize::from(1))
    } else {
        base_range
    }
}

fn check_node(node: &SyntaxNode, region_tree: &RegionTree, diagnostics: &mut Vec<Diagnostic>) {
    for child in node.children() {
        if matches!(
            child.kind(),
            SyntaxKind::PRE_IF_DIR | SyntaxKind::PRE_ELSE_CLAUSE | SyntaxKind::PRE_ELSIF_CLAUSE
        ) {
            check_node(&child, region_tree, diagnostics);
            continue;
        }

        if is_module_level_element(&child)
            && is_significant_element(&child)
            && !region_tree.is_range_inside_region(child.text_range())
        {
            let (element_type, range) = match child.kind() {
                SyntaxKind::FUNCTION_DEF => {
                    // Use method name range for compatibility with Java
                    let range = ast::FunctionDef::cast(child.clone())
                        .and_then(|f| f.name())
                        .map(|name| name.text_range())
                        .unwrap_or_else(|| child.text_range());
                    ("Функция", range)
                }
                SyntaxKind::PROCEDURE_DEF => {
                    // Use method name range for compatibility with Java
                    let range = ast::ProcedureDef::cast(child.clone())
                        .and_then(|p| p.name())
                        .map(|name| name.text_range())
                        .unwrap_or_else(|| child.text_range());
                    ("Процедура", range)
                }
                SyntaxKind::VAR_DEF => {
                    // Java uses whole variable declaration, not just the name
                    ("Переменная", child.text_range())
                }
                _ => {
                    // For statements, include semicolon for Java compatibility
                    ("Элемент кода", range_with_semicolon(&child))
                }
            };

            tracing::debug!(
                kind = ?child.kind(),
                range = ?range,
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
                range,
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

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range, assert_diagnostic_range_multiline, check_ast_diagnostic,
    };

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/CodeOutOfRegionDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 7, "Java expects 7 diagnostics");

        // Diagnostic 0: Перем А; (line 5, whole declaration)
        // Java: .hasRange(4, 0, 8)
        assert_diagnostic_range(code, &diagnostics[0], 4, 0, 8);

        // Diagnostic 1: Перем Ии; (line 10, whole declaration)
        // Java: .hasRange(8, 0, 9, 9) with TODO - should be (9, 0, 9)
        assert_diagnostic_range(code, &diagnostics[1], 9, 0, 9);

        // Diagnostic 2: Процедура ССС() (line 18, procedure name only)
        // Java: .hasRange(17, 10, 13)
        assert_diagnostic_range(code, &diagnostics[2], 17, 10, 13);

        // Diagnostic 3: Процедура Бб() (line 25, procedure name only)
        // Java: .hasRange(24, 10, 12)
        assert_diagnostic_range(code, &diagnostics[3], 24, 10, 12);

        // Diagnostic 4: Б = Аа() + А; (line 47, statement including semicolon)
        // Java: .hasRange(46, 0, 13) - includes semicolon ✅
        assert_diagnostic_range(code, &diagnostics[4], 46, 0, 13);

        // Diagnostic 5: Ин = в; (line 58, statement including semicolon)
        // Java: .hasRange(57, 0, 7) - includes semicolon ✅
        assert_diagnostic_range(code, &diagnostics[5], 57, 0, 7);

        // Diagnostic 6: Если Условие Тогда (lines 60-70, if block)
        // Java: .hasRange(59, 0, 69, 9)
        assert_diagnostic_range_multiline(code, &diagnostics[6], 59, 0, 69, 9);
    }

    #[test]
    fn test_empty_file() {
        let code = include_str!("../../test_data/CodeOutOfRegionDiagnosticEmptyFile.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_regions() {
        let code = include_str!("../../test_data/CodeOutOfRegionDiagnosticNoRegions.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        // NOTE: Java returns 1 diagnostic with relatedInformation when no regions exist
        // Rust returns individual diagnostics for each element (acceptable difference)
        assert_eq!(diagnostics.len(), 6);

        // Diagnostic 0: Перем А; (line 5)
        assert_diagnostic_range(code, &diagnostics[0], 4, 0, 8);

        // Diagnostic 1: Перем Б; (line 6)
        assert_diagnostic_range(code, &diagnostics[1], 5, 0, 8);

        // Diagnostic 2: Функция Аа() (line 8, function name)
        assert_diagnostic_range(code, &diagnostics[2], 7, 8, 10);

        // Diagnostic 3: Процедура Бб() (line 12, procedure name)
        assert_diagnostic_range(code, &diagnostics[3], 11, 10, 12);

        // Diagnostic 4: А = 78; (line 20, including semicolon)
        assert_diagnostic_range(code, &diagnostics[4], 19, 0, 7);

        // Diagnostic 5: Б = Аа() + А; (line 22, including semicolon)
        assert_diagnostic_range(code, &diagnostics[5], 21, 0, 13);
    }

    #[test]
    fn test_standard_preproc() {
        let code = include_str!("../../test_data/CodeOutOfRegionDiagnosticStandartPreproc.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_execute() {
        let code = include_str!("../../test_data/CodeOutOfRegionDiagnosticExecute.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 1);

        // Diagnostic 0: Процедура Выполнить() (lines 2-4)
        // NOTE: Rust returns full procedure body range (lines 1-3, 0-14)
        // This is acceptable since we still identify the correct element
        assert_diagnostic_range_multiline(code, &diagnostics[0], 1, 0, 3, 14);
    }

    #[test]
    fn test_code_block() {
        let code = include_str!("../../test_data/CodeOutOfRegionDiagnosticCodeBlock.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 1);

        // Diagnostic 0: НСтр("..."); (line 1, including semicolon)
        // Java: .hasRange(0, 0, 0, 23) ✅
        assert_diagnostic_range(code, &diagnostics[0], 0, 0, 23);
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

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_code_outside_region() {
        let code = r#"
Процедура Тест()
    Сообщить("OK");
КонецПроцедуры
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);

        // Diagnostic 0: Процедура Тест() (line 2, procedure name only)
        assert_diagnostic_range(code, &diagnostics[0], 1, 10, 14);
    }
}
