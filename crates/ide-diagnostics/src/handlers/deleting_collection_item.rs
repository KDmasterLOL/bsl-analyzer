//! DeletingCollectionItem diagnostic.
//!
//! Detects deletion of collection items within a ForEach loop iterating over that same collection.
//!
//! ## Why?
//! Deleting elements during `ForEach` iteration causes:
//! - Collection indices to change during iteration
//! - Some elements to be skipped
//! - Potential runtime errors
//! - Unexpected behavior in production code
//!
//! ## Bad practice
//! ```bsl
//! Для Каждого Элемент Из Коллекция Цикл
//!     Коллекция.Удалить(Элемент); // Error: Deleting from iterated collection!
//! КонецЦикла;
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Option 1: Reverse loop by index
//! Для Индекс = Коллекция.Количество() - 1 По 0 Цикл -1
//!     Коллекция.Удалить(Индекс);
//! КонецЦикла;
//!
//! // Option 2: Collect items to delete
//! УдаляемыеЭлементы = Новый Массив;
//! Для Каждого Элемент Из Коллекция Цикл
//!     Если УсловиеУдаления(Элемент) Тогда
//!         УдаляемыеЭлементы.Добавить(Элемент);
//!     КонецЕсли;
//! КонецЦикла;
//!
//! Для Каждого Элемент Из УдаляемыеЭлементы Цикл
//!     Коллекция.Удалить(Элемент);
//! КонецЦикла;
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Error (MAJOR)
//! - **Tags:** STANDARD, ERROR
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//! Ported from:
//! - DeletingCollectionItemDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//! - deleting_collection_item.rs (bsl-language-server-rust) - Rust reference (tree-sitter)
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use syntax::{SyntaxKind, SyntaxNode};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::DeletingCollectionItem) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();
    check_node(&root, &mut diagnostics);
    diagnostics
}

fn check_node(node: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    match node.kind() {
        SyntaxKind::FOR_EACH_STMT => {
            check_for_each(node, diagnostics);
            for child in node.children() {
                check_node(&child, diagnostics);
            }
        }
        _ => {
            for child in node.children() {
                check_node(&child, diagnostics);
            }
        }
    }
}

fn check_for_each(node: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    let collection_expr = match extract_collection_expression(node) {
        Some(expr) => expr,
        None => return,
    };

    let body = match find_loop_body(node) {
        Some(b) => b,
        None => return,
    };

    let delete_calls = find_delete_calls(&body, &collection_expr);

    for range in delete_calls {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::DeletingCollectionItem,
            message: format!(
                "Удаление элемента из коллекции '{}' во время итерации по ней может \
                 привести к пропуску элементов или ошибкам. Используйте обратный цикл \
                 по индексу или соберите элементы для удаления отдельно",
                collection_expr
            ),
            severity: Severity::Error,
            range,
            tags: vec![],
            fixes: vec![],
        });
    }
}

fn extract_collection_expression(node: &SyntaxNode) -> Option<String> {
    let mut found_in = false;

    for child in node.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::KW_IN {
                found_in = true;
            }
        } else if let Some(child_node) = child.as_node() {
            if found_in && child_node.kind() != SyntaxKind::KW_DO {
                return Some(child_node.text().to_string());
            }
        }
    }

    None
}

fn find_loop_body(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.children().find(|n| n.kind() == SyntaxKind::STMT_LIST)
}

fn find_delete_calls(body: &SyntaxNode, collection: &str) -> Vec<TextRange> {
    let mut ranges = Vec::new();
    let collection_lower = collection.to_lowercase().trim().to_string();

    for node in body.descendants() {
        if matches!(node.kind(), SyntaxKind::CALL_STMT | SyntaxKind::CALL_EXPR)
            && is_delete_call(&node)
            && matches_collection(&node, &collection_lower)
        {
            ranges.push(node.text_range());
        }
    }

    ranges
}

fn is_delete_call(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens().filter_map(|el| el.into_token()).any(|t| {
        if t.kind() == SyntaxKind::IDENT {
            let lower = t.text().to_lowercase();
            matches!(lower.as_str(), "удалить" | "delete")
        } else {
            false
        }
    })
}

fn matches_collection(call_stmt: &SyntaxNode, collection_lower: &str) -> bool {
    let mut call_prefix = String::new();

    for element in call_stmt.descendants_with_tokens() {
        if let Some(token) = element.as_token() {
            match token.kind() {
                SyntaxKind::IDENT | SyntaxKind::DOT | SyntaxKind::L_PAREN | SyntaxKind::R_PAREN => {
                    call_prefix.push_str(token.text());
                }
                _ => {}
            }
        }
    }

    let call_lower = call_prefix.to_lowercase();
    call_lower.starts_with(&format!("{}.", collection_lower))
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
    fn test_simple_deletion() {
        let code = r#"
Для Каждого Элемент Из Коллекция Цикл
    Коллекция.Удалить(Элемент);
КонецЦикла;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect deletion in ForEach");
    }

    #[test]
    fn test_different_collection_ok() {
        let code = r#"
Для Каждого Элемент Из Коллекция1 Цикл
    Коллекция2.Удалить(Элемент);
КонецЦикла;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Different collection should be OK");
    }

    #[test]
    fn test_global_delete_ok() {
        let code = r#"
Для Каждого Элемент Из Коллекция Цикл
    Удалить(Элемент);
КонецЦикла;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Global Удалить() should be OK");
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
for each elem in mass do
    mass.delete(elem);
enddo;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should detect English delete");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
for each elem in Mass().mass1.mass2() do
    mass().mAss1.mass2().delete(elem+1);
enddo;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Should match case-insensitively");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/DeletingCollectionItemDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 8, "Should match Java: 8 diagnostics");

        assert_diagnostic_range(&file_content, &diagnostics[0], 17, 8, 47);
        assert_diagnostic_range(&file_content, &diagnostics[1], 23, 4, 21);
        assert_diagnostic_range(&file_content, &diagnostics[2], 28, 4, 25);
        assert_diagnostic_range(&file_content, &diagnostics[3], 33, 4, 30);
        assert_diagnostic_range(&file_content, &diagnostics[4], 39, 8, 34);
        assert_diagnostic_range(&file_content, &diagnostics[5], 45, 4, 23);
        assert_diagnostic_range(&file_content, &diagnostics[6], 50, 4, 37);
        assert_diagnostic_range(&file_content, &diagnostics[7], 55, 4, 39);
    }
}
