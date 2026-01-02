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

    // Optimized: Collect all nodes in one pass
    let mut unary_exprs = Vec::new();
    let mut binary_exprs = Vec::new();
    let mut node_info = std::collections::HashMap::new();

    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::UNARY_EXPR => {
                let has_not = has_not_token(&node);
                node_info.insert(node.text_range().start(), (has_not, false));
                unary_exprs.push(node);
            }
            SyntaxKind::BINARY_EXPR => {
                let has_neq = has_neq_token(&node);
                node_info.insert(node.text_range().start(), (has_neq, false));
                binary_exprs.push(node);
            }
            _ => {}
        }
    }

    // Check Pattern 1: Double NOT
    for node in &unary_exprs {
        if let Some(range) = check_double_not_optimized(node, &unary_exprs, &node_info) {
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

    // Check Pattern 2a: NOT wrapping NEQ
    for node in &unary_exprs {
        if let Some(range) = check_not_wrapping_neq_optimized(node, &binary_exprs, &node_info) {
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

    // Check Pattern 2b: NOT on left operand of NEQ
    for node in &binary_exprs {
        if let Some(range) = check_not_on_left_neq_optimized(node, &unary_exprs, &node_info) {
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

/// Pattern 1: Detect Не (Не X) - double NOT (optimized)
fn check_double_not_optimized(
    node: &SyntaxNode,
    unary_exprs: &[SyntaxNode],
    node_info: &std::collections::HashMap<syntax::TextSize, (bool, bool)>,
) -> Option<TextRange> {
    // 1. Check if this node has NOT operator (pre-computed)
    let (has_not, _) = node_info.get(&node.text_range().start())?;
    if !has_not {
        return None;
    }

    // 2. Check if any descendant unary_expr (from pre-collected list) has NOT
    let node_range = node.text_range();
    for descendant in unary_exprs {
        // Skip self
        if descendant.text_range() == node_range {
            continue;
        }

        // Check if descendant is inside this node
        if !node_range.contains_range(descendant.text_range()) {
            continue;
        }

        // Check if descendant has NOT (pre-computed)
        if let Some((desc_has_not, _)) = node_info.get(&descendant.text_range().start()) {
            if *desc_has_not {
                // 3. Filter: skip if logical operators inside
                if contains_logical_operators(node) {
                    return None;
                }

                // 4. Filter: skip if text ends with "=" (incomplete due to parse error)
                let text = node.text().to_string();
                if text.trim_end().ends_with('=') {
                    return None;
                }

                // 5. Return entire outer UnaryExpr range
                return Some(node_range);
            }
        }
    }

    None
}

/// Pattern 2a: Detect Не (X <> Y) - NOT wrapping NEQ (optimized)
fn check_not_wrapping_neq_optimized(
    node: &SyntaxNode,
    binary_exprs: &[SyntaxNode],
    node_info: &std::collections::HashMap<syntax::TextSize, (bool, bool)>,
) -> Option<TextRange> {
    // 1. Check if this node has NOT operator (pre-computed)
    let (has_not, _) = node_info.get(&node.text_range().start())?;
    if !has_not {
        return None;
    }

    // 2. Check if any descendant binary_expr (from pre-collected list) has NEQ
    let node_range = node.text_range();
    for descendant in binary_exprs {
        // Check if descendant is inside this node
        if !node_range.contains_range(descendant.text_range()) {
            continue;
        }

        // Check if descendant has NEQ (pre-computed)
        if let Some((desc_has_neq, _)) = node_info.get(&descendant.text_range().start()) {
            if *desc_has_neq {
                // 3. Filter: skip if logical operators inside
                if contains_logical_operators(node) {
                    return None;
                }

                // 4. Return entire UnaryExpr range
                return Some(node_range);
            }
        }
    }

    None
}

/// Pattern 2b: Detect (Не X) <> Y - NOT on left operand (optimized)
fn check_not_on_left_neq_optimized(
    node: &SyntaxNode,
    _unary_exprs: &[SyntaxNode],
    node_info: &std::collections::HashMap<syntax::TextSize, (bool, bool)>,
) -> Option<TextRange> {
    // 1. Check if this node has NEQ operator (pre-computed)
    let (has_neq, _) = node_info.get(&node.text_range().start())?;
    if !has_neq {
        return None;
    }

    // 2. Check if left child is UnaryExpr with NOT operator
    let node_range = node.text_range();
    for child in node.children() {
        if child.kind() != SyntaxKind::UNARY_EXPR {
            continue;
        }

        // Check if child has NOT (pre-computed)
        if let Some((child_has_not, _)) = node_info.get(&child.text_range().start()) {
            if *child_has_not {
                // 3. Filter: skip if logical operators inside
                if contains_logical_operators(node) {
                    return None;
                }

                // 4. Return entire BinaryExpr range
                return Some(node_range);
            }
        }
    }

    None
}

/// Pattern 1: Detect Не (Не X) - double NOT
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
