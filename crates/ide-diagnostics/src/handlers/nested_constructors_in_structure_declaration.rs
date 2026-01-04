//! NestedConstructorsInStructureDeclaration diagnostic.
//!
//! Detects when Structure/FixedStructure constructors contain nested constructors
//! with parameters, which reduces code readability.
//!
//! ## Why?
//! Nested constructors in structure declarations make code harder to read and understand.
//! It's better to create nested structures as separate variables.
//!
//! ## Bad practice
//! ```bsl
//! Результат = Новый Структура("ДанныеНоменклатуры, Количество",
//!                              Новый Структура("Код, Наименование"),
//!                              10);
//! ```
//!
//! ## Good practice
//! ```bsl
//! ДанныеНоменклатуры = Новый Структура("Код, Наименование");
//! Результат = Новый Структура("ДанныеНоменклатуры, Количество",
//!                              ДанныеНоменклатуры,
//!                              10);
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Minor (Warning)
//! - **Tags:** BADPRACTICE, BRAINOVERLOAD
//! - **Minutes to fix:** 10
//!
//! ## Implementation
//! Ported from:
//! - NestedConstructorsInStructureDeclarationDiagnostic.java (bsl-language-server)
//!
//! Adapted to use Rowan SyntaxNode.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::NestedConstructorsInStructureDeclaration) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() != SyntaxKind::NEW_EXPR {
            continue;
        }

        if let Some(diagnostic) = check_new_expr(&node) {
            diagnostics.push(diagnostic);
        }
    }

    diagnostics
}

fn check_new_expr(node: &SyntaxNode) -> Option<Diagnostic> {
    if !is_structure_or_fixed_structure(node) {
        return None;
    }

    let params = get_call_params(node);
    if params.len() <= 1 {
        return None;
    }

    let nested_with_params: Vec<&SyntaxNode> =
        params.iter().filter(|p| is_new_expr_with_params(p)).collect();

    if !nested_with_params.is_empty() {
        return Some(make_diagnostic(node));
    }

    None
}

fn is_structure_or_fixed_structure(node: &SyntaxNode) -> bool {
    let tokens: Vec<_> = node.children_with_tokens().filter_map(|el| el.into_token()).collect();

    let mut found_new = false;
    for token in &tokens {
        if token.kind() == SyntaxKind::KW_NEW {
            found_new = true;
            continue;
        }

        if found_new && token.kind() == SyntaxKind::IDENT {
            let text = token.text().to_lowercase();
            return matches!(
                text.as_str(),
                "структура" | "structure" | "фиксированнаяструктура" | "fixedstructure"
            );
        }
    }

    false
}

fn get_call_params(node: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut params = Vec::new();

    for child in node.children() {
        if child.kind() == SyntaxKind::ARG_LIST {
            // Children of ARG_LIST are EXPR nodes separated by COMMA tokens
            for arg in child.children() {
                if arg.kind() == SyntaxKind::EXPR {
                    params.push(arg);
                }
            }
            break;
        }
    }

    params
}

fn is_new_expr_with_params(arg: &SyntaxNode) -> bool {
    // Java logic: filter params that START with NEW keyword
    // This excludes cases like FillStructure(New FixedStructure(...))
    // where the param starts with IDENT, not NEW

    // Get the first significant token in this argument
    let first_token = arg
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| !t.kind().is_trivia());

    // Must start with KW_NEW
    let Some(first) = first_token else {
        return false;
    };

    if first.kind() != SyntaxKind::KW_NEW {
        return false;
    }

    // Now find the first NEW_EXPR in this argument and check if it has non-empty params
    for descendant in arg.descendants() {
        if descendant.kind() == SyntaxKind::NEW_EXPR {
            if has_non_empty_params(&descendant) {
                return true;
            }
            // Only check the first NEW_EXPR (matches Java behavior)
            break;
        }
    }

    false
}

fn has_non_empty_params(new_expr: &SyntaxNode) -> bool {
    for child in new_expr.children() {
        if child.kind() == SyntaxKind::ARG_LIST {
            // Children of ARG_LIST are EXPR nodes
            for arg in child.children() {
                if arg.kind() == SyntaxKind::EXPR {
                    let has_content = arg.descendants_with_tokens().any(|el| {
                        matches!(
                            el.kind(),
                            SyntaxKind::IDENT
                                | SyntaxKind::STRING
                                | SyntaxKind::DECIMAL
                                | SyntaxKind::FLOAT
                        )
                    });
                    if has_content {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn make_diagnostic(node: &SyntaxNode) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::NestedConstructorsInStructureDeclaration,
        message: "Не используйте конструкторы с параметрами при объявлении структуры".to_string(),
        severity: Severity::Warning,
        range: node.text_range(),
        tags: vec![],
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range_multiline;
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
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_no_diagnostic_for_empty_structure() {
        let code = r#"
Результат = Новый Структура;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_for_single_param() {
        let code = r#"
А = Новый Структура(Новый ФиксированнаяСтруктура(Мок_ПараметрыПроцедуры));
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_for_nested_without_params() {
        let code = r#"
Результат = Новый Структура("МВТ, ТекстЗапроса, Параметры",
                             Новый МенеджерВременныхТаблиц,
                             ТекстЗапроса,
                             Новый Структура);
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_diagnostic_for_nested_with_params() {
        let code = r#"
Результат = Новый Структура("ДанныеНоменклатуры, Количество",
                             Новый Структура("Код, Наименование"),
                             10);
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_diagnostic_for_english_keywords() {
        let code = r#"
Result = New Structure("GoodsData, Count",
                        New Structure("Code, Name"),
                        10);
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_diagnostic_for_non_structure() {
        let code = r#"
Result = New Structure("field1, field2, field3", New Array(), New Array(), New Array());
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_comprehensive() {
        let code =
            include_str!("../../test_data/NestedConstructorsInStructureDeclarationDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(
            diagnostics.len(),
            8,
            "Should find exactly 8 diagnostics (matching Java implementation)"
        );

        // Verify exact positions matching bsl-language-server (Java) implementation
        // Java uses 0-indexed lines
        assert_diagnostic_range_multiline(&file_content, &diagnostics[0], 10, 16, 12, 36);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[1], 14, 16, 23, 62);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[2], 25, 16, 27, 96);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[3], 26, 32, 27, 95);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[4], 38, 13, 40, 31);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[5], 42, 13, 51, 50);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[6], 53, 13, 55, 79);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[7], 54, 28, 55, 78);
    }
}
