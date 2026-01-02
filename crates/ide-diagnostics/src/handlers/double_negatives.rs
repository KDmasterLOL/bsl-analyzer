//! DoubleNegatives diagnostic
//!
//! Detects double negative expressions that make code harder to understand.
//!
//! ## Patterns
//! 1. **Double NOT:** `НЕ (НЕ Condition)` - redundant double negation
//! 2. **Negated NOT_EQUAL:** `НЕ (X <> Y)` or `(НЕ X) <> Y` - logically equivalent to `X = Y`
//!
//! ## Why?
//! Double negatives are harder to read and understand. They can be simplified:
//! - `НЕ (НЕ X)` → `X`
//! - `НЕ (X <> Y)` → `X = Y`
//! - `(НЕ X) <> Y` → `X = Y`

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use syntax::{SyntaxKind, SyntaxNode};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::DoubleNegatives) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        // Pattern 1: Double NOT (Не (Не X))
        if let Some(range) = check_double_not(&node) {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::DoubleNegatives,
                message: "Using double negatives complicates understanding of code".to_string(),
                severity: Severity::Warning,
                range,
                tags: vec![],
                fixes: vec![],
            });
        }

        // Pattern 2a: NOT wrapping NEQ (Не (X <> Y))
        if let Some(range) = check_not_wrapping_neq(&node) {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::DoubleNegatives,
                message: "Using double negatives complicates understanding of code".to_string(),
                severity: Severity::Warning,
                range,
                tags: vec![],
                fixes: vec![],
            });
        }

        // Pattern 2b: NOT on left operand of NEQ ((Не X) <> Y)
        if let Some(range) = check_not_on_left_neq(&node) {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::DoubleNegatives,
                message: "Using double negatives complicates understanding of code".to_string(),
                severity: Severity::Warning,
                range,
                tags: vec![],
                fixes: vec![],
            });
        }
    }

    diagnostics
}

/// Check if node contains NOT operator token (direct children only)
fn has_not_token(node: &SyntaxNode) -> bool {
    node.children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::KW_NOT)
}

/// Check if node contains <> operator token (direct children only)
fn has_neq_token(node: &SyntaxNode) -> bool {
    node.children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::NEQ)
}

/// Check if node or descendants contain AND/OR operators
fn contains_logical_operators(node: &SyntaxNode) -> bool {
    for descendant in node.descendants() {
        if descendant
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .any(|tok| matches!(tok.kind(), SyntaxKind::KW_AND | SyntaxKind::KW_OR))
        {
            return true;
        }
    }
    false
}

/// Pattern 1: Detect Не (Не X) - double NOT
fn check_double_not(node: &SyntaxNode) -> Option<TextRange> {
    // 1. Check if node is UnaryExpr with NOT operator
    if node.kind() != SyntaxKind::UNARY_EXPR {
        return None;
    }

    if !has_not_token(node) {
        return None;
    }

    // 2. Check if descendants contain another UnaryExpr with NOT (skip self)
    for descendant in node.descendants().skip(1) {
        if descendant.kind() == SyntaxKind::UNARY_EXPR && has_not_token(&descendant) {
            // 3. Filter: skip if logical operators inside
            if contains_logical_operators(node) {
                return None;
            }

            // 4. Filter: skip if text ends with "= " (incomplete due to parse error)
            // This happens when "Не (Не А = ..." has parse error on "="
            let text = node.text().to_string();
            if text.trim_end().ends_with('=') {
                return None;
            }

            // 5. Return entire outer UnaryExpr range
            return Some(node.text_range());
        }
    }

    None
}

/// Pattern 2a: Detect Не (X <> Y) - NOT wrapping NEQ
fn check_not_wrapping_neq(node: &SyntaxNode) -> Option<TextRange> {
    // 1. Check if node is UnaryExpr with NOT operator
    if node.kind() != SyntaxKind::UNARY_EXPR {
        return None;
    }

    if !has_not_token(node) {
        return None;
    }

    // 2. Check if descendants contain BinaryExpr with NEQ operator (skip self)
    for descendant in node.descendants().skip(1) {
        if descendant.kind() == SyntaxKind::BINARY_EXPR && has_neq_token(&descendant) {
            // 3. Filter: skip if logical operators inside
            if contains_logical_operators(node) {
                return None;
            }

            // 4. Return entire UnaryExpr range
            return Some(node.text_range());
        }
    }

    None
}

/// Pattern 2b: Detect (Не X) <> Y - NOT on left operand
fn check_not_on_left_neq(node: &SyntaxNode) -> Option<TextRange> {
    // 1. Check if node is BinaryExpr with NEQ operator
    if node.kind() != SyntaxKind::BINARY_EXPR {
        return None;
    }

    if !has_neq_token(node) {
        return None;
    }

    // 2. Check if left child is UnaryExpr with NOT operator
    for child in node.children() {
        if child.kind() == SyntaxKind::UNARY_EXPR && has_not_token(&child) {
            // 3. Filter: skip if logical operators inside
            if contains_logical_operators(node) {
                return None;
            }

            // 4. Return entire BinaryExpr range
            return Some(node.text_range());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let config = crate::DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, fixture.files[&file_id].content.to_string())
    }

    #[test]
    fn test_no_double_negative() {
        let code = "А = Не Значение;";
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_double_not_russian() {
        let code = "Б = Не (Не Значение);";
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_not_neq_russian() {
        // Pattern `Не Отказ <> Ложь` without parentheses
        // NOW DETECTED after parser improvements (typed expression nodes)
        let code = "А = Не Отказ <> Ложь;";
        let (diagnostics, _) = check_diagnostic(code);

        // Parser improvement: now detects this pattern!
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_not_wrapping_neq() {
        let code = "А = Не (Отказ <> Ложь);";
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_not_equal_not_detected() {
        // Uses = instead of <>, should NOT detect
        let code = "А = Не Отказ = Ложь;";
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_with_logical_operators_inside() {
        // AND inside NOT expression, should skip
        let code = "А = Не (А <> Неопределено и Б = 5);";
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_double_not_with_and_inside() {
        // AND inside double NOT, should skip
        let code = "Б = Не (Не Значение И ДругоеЗначение);";
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/DoubleNegativesDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        // Debug: Print all detected diagnostics with line numbers and text
        eprintln!("\n=== Detected {} diagnostics ===", diagnostics.len());
        for (i, diag) in diagnostics.iter().enumerate() {
            let start_offset = diag.range.start().into();
            let end_offset = diag.range.end().into();

            // Calculate line number (1-indexed)
            let line_num = file_content[..start_offset].lines().count();

            // Extract the text
            let text = &file_content[start_offset..end_offset];
            eprintln!("#{}: Line {} - {:?}", i + 1, line_num, text);
        }
        eprintln!("=================================\n");

        // Expected 12 diagnostics based on comments in test file:
        // Line 2:  Не ТаблицаЗначений.Найти(...) <> Неопределено
        // Line 6:  Не Отказ <> Ложь
        // Line 7:  Не (Отказ <> Ложь)
        // Line 8:  Не НекотороеЗначение() <> Неопределено
        // Line 9:  Не Неопределено <> НекотороеЗначение()
        // Line 10: Не (А <> Неопределено)
        // Line 11: Не А <> Неопределено (part of larger expression)
        // Line 16: Не Таблица.Данные <> Неопределено
        // Line 20: Не Б <> Неопределено (nested in OR expression)
        // Line 29: Не (Отказ <> НеЛитерал)
        // Line 30: Не СложнаяФункция() <> НеЛитерал
        // Line 36: Не (Не Значение)

        assert_eq!(
            diagnostics.len(),
            12,
            "Expected exactly 12 diagnostics matching Java bsl-language-server"
        );
    }
}
