//! UseLessForEach diagnostic.
//!
//! Detects unused iterators in "For Each" loops - when the loop iterates over a collection
//! but the iterator variable is never used in the loop body.
//!
//! ## Why?
//! An unused iterator indicates either:
//! - Programming error (forgetting to use the variable)
//! - Unnecessary iteration (should use a different approach)
//!
//! ## Bad practice
//! ```bsl
//! Для Каждого Итератор Из Коллекция Цикл
//!     Итератор(); // Calling iterator as a function is NOT valid usage
//! КонецЦикла;
//! ```
//!
//! ## Good practice
//! ```bsl
//! Для Каждого Элемент Из Коллекция Цикл
//!     Результат = Элемент.Свойство; // Property access
//! КонецЦикла;
//!
//! Для Каждого А Из Б Цикл
//!     А = Истина; // Assignment
//! КонецЦикла;
//!
//! Для Каждого Объект Из Б Цикл
//!     Объект.Метод(); // Method call on iterator
//! КонецЦикла;
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Error (CRITICAL)
//! - **Tags:** CLUMSY
//! - **Minutes to fix:** 2

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::Name;
use ide_db::TextRange;
use syntax::{SyntaxKind, SyntaxNode};
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Clumsy],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    iterator_name: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::UseLessForEach;
    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    // Skip if iterator name matches a module-level variable
    let symbol_tree = ctx.symbol_tree();
    if symbol_tree.find_variable(&Name::new(iterator_name)).is_some() {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Итератор не используется в теле цикла".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::UseLessForEach;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let symbol_tree = ctx.symbol_tree();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() != SyntaxKind::FOR_EACH_STMT {
            continue;
        }

        if let Some(diag) = check_for_each(&node, &symbol_tree, ctx) {
            diagnostics.push(diag);
        }
    }

    diagnostics
}

fn check_for_each(
    node: &SyntaxNode,
    symbol_tree: &hir_def::SymbolTree,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::UseLessForEach;

    let (iterator_name, iterator_range) = get_iterator_info(node)?;

    if symbol_tree.find_variable(&Name::new(&iterator_name)).is_some() {
        return None;
    }

    let stmt_list = find_stmt_list(node)?;
    let has_valid_usage = check_iterator_usage(&stmt_list, &iterator_name);

    if !has_valid_usage {
        return Some(Diagnostic {
            code,
            message: "Итератор не используется в теле цикла".to_string(),
            severity: ctx.severity(code),
            range: iterator_range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    None
}

fn get_iterator_info(for_each_node: &SyntaxNode) -> Option<(String, TextRange)> {
    let mut found_each = false;

    for element in for_each_node.children_with_tokens() {
        match element.kind() {
            SyntaxKind::KW_EACH => {
                found_each = true;
            }
            SyntaxKind::IDENT if found_each => {
                if let Some(token) = element.into_token() {
                    return Some((token.text().to_string(), token.text_range()));
                }
            }
            SyntaxKind::KW_IN => {
                break;
            }
            _ => {}
        }
    }

    None
}

fn find_stmt_list(for_each_node: &SyntaxNode) -> Option<SyntaxNode> {
    for_each_node.children().find(|child| child.kind() == SyntaxKind::STMT_LIST)
}

fn check_iterator_usage(stmt_list: &SyntaxNode, iterator_name: &str) -> bool {
    let iterator_lower = iterator_name.to_lowercase();

    for descendant in stmt_list.descendants_with_tokens() {
        if let Some(token) = descendant.into_token() {
            if token.kind() == SyntaxKind::IDENT
                && token.text().to_lowercase() == iterator_lower
                && is_valid_usage(&token)
            {
                return true;
            }
        }
    }

    false
}

fn is_valid_usage(token: &syntax::SyntaxToken) -> bool {
    !is_direct_function_call(token)
}

fn is_direct_function_call(token: &syntax::SyntaxToken) -> bool {
    let Some(ident_node) = token.parent() else {
        return false;
    };

    let Some(parent) = ident_node.parent() else {
        return false;
    };

    if parent.kind() == SyntaxKind::CALL_EXPR {
        return is_direct_callee_node(&ident_node, &parent);
    }

    if parent.kind() == SyntaxKind::CALL_STMT {
        for child in parent.children() {
            if child.kind() == SyntaxKind::CALL_EXPR {
                return is_direct_callee_node(&ident_node, &child);
            }
        }
    }

    false
}

fn is_direct_callee_node(ident_node: &SyntaxNode, call_expr: &SyntaxNode) -> bool {
    if let Some(first_child) = call_expr.first_child() {
        if first_child.text_range() == ident_node.text_range() {
            for child in call_expr.children() {
                if child.kind() == SyntaxKind::FIELD_EXPR {
                    return false;
                }
            }
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    use crate::DiagnosticCode;

    use super::check;

    #[test]
    fn test_unused_iterator() {
        let code = r#"
Для Каждого Итератор Из Коллекция Цикл
    Итератор();
КонецЦикла;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::UseLessForEach);
    }

    #[test]
    fn test_used_in_method_call() {
        let code = r#"
Для Каждого А Из Б Цикл
    КакойТОМетод(а);
КонецЦикла;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Iterator passed to method should count as usage");
    }

    #[test]
    fn test_used_in_assignment() {
        let code = r#"
Для Каждого А Из Б Цикл
    В = А;
КонецЦикла;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Iterator in right side of assignment should count as usage"
        );
    }

    #[test]
    fn test_iterator_assigned() {
        let code = r#"
Для Каждого А Из Б Цикл
    А = Истина;
КонецЦикла;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Iterator assigned should count as usage");
    }

    #[test]
    fn test_property_access() {
        let code = r#"
Для Каждого А Из Б Цикл
    А.Свойство = 1;
КонецЦикла;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Property access should count as usage");
    }

    #[test]
    fn test_in_condition() {
        let code = r#"
Для Каждого А Из Б Цикл
    Если А Тогда
    КонецЕсли;
КонецЦикла;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Iterator in condition should count as usage");
    }

    #[test]
    fn test_method_call_on_iterator() {
        let code = r#"
Для Каждого Объект Из Б Цикл
    Объект.Метод();
КонецЦикла;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Method call on iterator should count as usage");
    }

    #[test]
    fn test_chained_method_call() {
        let code = r#"
Для Каждого АСтруктура Из Б Цикл
    АСтруктура.Ключ.Метод();
КонецЦикла;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Chained method call should count as usage");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/UseLessForEachDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UseLessForEach).collect();

        assert_eq!(diags.len(), 2, "Should match Java: 2 diagnostics");

        assert_diagnostic_range(code, diags[0], 2, 12, 20);
        assert_diagnostic_range(code, diags[1], 39, 16, 26);
    }

    #[test]
    fn test_hir_unused_iterator() {
        use crate::test_utils::check_hir_diagnostic;

        let code = r#"
Процедура Тест()
    Для Каждого Итератор Из Коллекция Цикл
        Итератор();
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let filtered: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UseLessForEach).collect();
        assert_eq!(filtered.len(), 1, "HIR should detect unused iterator");
    }

    #[test]
    fn test_hir_used_iterator() {
        use crate::test_utils::check_hir_diagnostic;
        let code = r#"
Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Результат = Элемент.Свойство;
    КонецЦикла;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let filtered: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UseLessForEach).collect();
        assert_eq!(filtered.len(), 0, "HIR should not trigger for used iterator");
    }
}
