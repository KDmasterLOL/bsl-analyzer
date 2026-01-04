//! IsInRoleMethod diagnostic.
//!
//! Detects incorrect usage of `IsInRole()` / `РольДоступна()` method for access checking
//! without proper `PrivilegedMode()` / `ПривилегированныйРежим()` protection.
//!
//! ## Why?
//! The `IsInRole()` method should be used ONLY when a role does not grant access rights to
//! metadata objects and serves only to define an additional access right. When used, it
//! MUST be combined with a check for `PrivilegedMode()`.
//!
//! Using `IsInRole()` without `PrivilegedMode()` check may lead to security vulnerabilities
//! where access control checks can be bypassed.
//!
//! ## Bad practice
//! ```bsl
//! Если РольДоступна("ТребуемаяРоль") Тогда
//!     // Выполнение кода
//! КонецЕсли;
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Option 1: Combined check with PrivilegedMode
//! Если РольДоступна("ТребуемаяРоль") ИЛИ ПривилегированныйРежим() Тогда
//!     // Выполнение кода
//! КонецЕсли;
//!
//! // Option 2: Use AccessRight instead
//! Если ПравоДоступа("Добавление", Метаданные.Справочники.Номенклатура) Тогда
//!     // Выполнение кода
//! КонецЕсли;
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Major
//! - **Tags:** ERROR
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//! Ported from:
//! - IsInRoleMethodDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//!
//! Uses stateful traversal to track:
//! - Variables containing `IsInRole()` results
//! - Variables containing `PrivilegedMode()` results
//! - If-statement expressions for protection checks

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use std::collections::HashSet;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::IsInRoleMethod) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut all_diagnostics = Vec::new();

    // Process each procedure/function independently to maintain proper scoping
    // Variables in different procedures should not interfere with each other
    for procedure in root.descendants() {
        if !matches!(procedure.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF) {
            continue;
        }

        let mut checker = IsInRoleChecker {
            is_in_role_vars: HashSet::new(),
            privileged_mode_vars: HashSet::new(),
            diagnostics: Vec::new(),
        };

        // Two-pass approach within each procedure:
        // Pass 1: Process all assignments to build variable tracking
        collect_variables(&procedure, &mut checker);

        // Pass 2: Check if-statements for diagnostics
        check_node(&procedure, &mut checker);

        all_diagnostics.extend(checker.diagnostics);
    }

    all_diagnostics.sort_by_key(|d| d.range.start());
    all_diagnostics
}

struct IsInRoleChecker {
    is_in_role_vars: HashSet<String>,
    privileged_mode_vars: HashSet<String>,
    diagnostics: Vec<Diagnostic>,
}

fn collect_variables(node: &SyntaxNode, checker: &mut IsInRoleChecker) {
    if node.kind() == SyntaxKind::ASSIGN_STMT {
        handle_assignment(node, checker);
    }

    for child in node.children() {
        collect_variables(&child, checker);
    }
}

fn check_node(node: &SyntaxNode, checker: &mut IsInRoleChecker) {
    if node.kind() == SyntaxKind::IF_STMT {
        check_if_statement(node, checker);
    }

    for child in node.children() {
        check_node(&child, checker);
    }
}

fn handle_assignment(assign_stmt: &SyntaxNode, checker: &mut IsInRoleChecker) {
    let tokens: Vec<_> =
        assign_stmt.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    let mut var_name: Option<String> = None;
    let mut eq_index: Option<usize> = None;

    for (i, token) in tokens.iter().enumerate() {
        if token.kind() == SyntaxKind::IDENT && var_name.is_none() {
            var_name = Some(token.text().to_string());
        }
        if token.kind() == SyntaxKind::EQ {
            eq_index = Some(i);
            break;
        }
    }

    // CRITICAL: Remove variable from BOTH sets (reassignment clears tracking)
    if let Some(ref var) = var_name {
        let var_lower = var.to_lowercase();
        checker.is_in_role_vars.remove(&var_lower);
        checker.privileged_mode_vars.remove(&var_lower);
    }

    if let Some(eq_idx) = eq_index {
        let rhs_tokens = &tokens[eq_idx + 1..];

        for (i, token) in rhs_tokens.iter().enumerate() {
            if token.kind() == SyntaxKind::IDENT && is_is_in_role_method(token.text()) {
                let next_is_lparen =
                    rhs_tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);
                if next_is_lparen {
                    if let Some(ref var) = var_name {
                        checker.is_in_role_vars.insert(var.to_lowercase());
                    }
                    break;
                }
            }
        }

        for (i, token) in rhs_tokens.iter().enumerate() {
            if token.kind() == SyntaxKind::IDENT && is_privileged_mode_method(token.text()) {
                let next_is_lparen =
                    rhs_tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);
                if next_is_lparen {
                    if let Some(ref var) = var_name {
                        checker.privileged_mode_vars.insert(var.to_lowercase());
                    }
                    break;
                }
            }
        }
    }
}

fn check_if_statement(if_stmt: &SyntaxNode, checker: &mut IsInRoleChecker) {
    if let Some(expr) = find_if_expression(if_stmt) {
        check_expression(&expr, checker);
    }

    for child in if_stmt.children() {
        if child.kind() == SyntaxKind::ELSIF_CLAUSE {
            check_elsif_clause(&child, checker);
        }
    }
}

fn check_elsif_clause(elsif_clause: &SyntaxNode, checker: &mut IsInRoleChecker) {
    if let Some(expr) = find_elsif_expression(elsif_clause) {
        check_expression(&expr, checker);
    }
}

fn check_expression(expr: &SyntaxNode, checker: &mut IsInRoleChecker) {
    let direct_call_ranges = find_is_in_role_calls(expr);
    for range in direct_call_ranges {
        if !has_privileged_mode_protection(expr, checker) {
            checker.diagnostics.push(create_diagnostic(range));
        }
    }

    // Exclude method calls (IDENT followed by LPAREN)
    let tokens: Vec<_> = expr.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    for (i, token) in tokens.iter().enumerate() {
        if token.kind() != SyntaxKind::IDENT {
            continue;
        }

        let next_is_lparen =
            tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);
        if next_is_lparen {
            continue;
        }

        let ident_text = token.text().to_lowercase();
        if checker.is_in_role_vars.contains(&ident_text)
            && !has_privileged_mode_protection(expr, checker)
        {
            checker.diagnostics.push(create_diagnostic(token.text_range()));
        }
    }
}

fn has_privileged_mode_protection(expr: &SyntaxNode, checker: &IsInRoleChecker) -> bool {
    if contains_privileged_mode_call(expr) {
        return true;
    }

    // Exclude method calls (IDENT followed by LPAREN)
    let tokens: Vec<_> = expr.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    for (i, token) in tokens.iter().enumerate() {
        if token.kind() != SyntaxKind::IDENT {
            continue;
        }

        let next_is_lparen =
            tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);
        if next_is_lparen {
            continue;
        }

        let ident_text = token.text().to_lowercase();
        if checker.privileged_mode_vars.contains(&ident_text) {
            return true;
        }
    }

    false
}

fn is_is_in_role_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "рольдоступна" | "isinrole")
}

fn is_privileged_mode_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "привилегированныйрежим" | "privilegedmode")
}

fn contains_privileged_mode_call(node: &SyntaxNode) -> bool {
    let tokens: Vec<_> = node.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    for (i, token) in tokens.iter().enumerate() {
        if token.kind() == SyntaxKind::IDENT && is_privileged_mode_method(token.text()) {
            let next_is_lparen =
                tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);
            if next_is_lparen {
                return true;
            }
        }
    }
    false
}

fn find_is_in_role_calls(expr: &SyntaxNode) -> Vec<TextRange> {
    let mut ranges = Vec::new();
    let tokens: Vec<_> = expr.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    for (i, token) in tokens.iter().enumerate() {
        if token.kind() == SyntaxKind::IDENT && is_is_in_role_method(token.text()) {
            let next_is_lparen =
                tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);
            if next_is_lparen {
                // Find the full call expression range (from IDENT to closing RPAREN)
                if let Some(call_range) = find_call_expression_range(&tokens, i) {
                    ranges.push(call_range);
                } else {
                    // Fallback to just the identifier if we can't find the full range
                    ranges.push(token.text_range());
                }
            }
        }
    }

    ranges
}

fn find_call_expression_range(tokens: &[SyntaxToken], ident_index: usize) -> Option<TextRange> {
    let start_offset = tokens.get(ident_index)?.text_range().start();

    let mut paren_depth = 0;
    let mut end_offset = None;

    for token in tokens.iter().skip(ident_index + 1) {
        match token.kind() {
            SyntaxKind::L_PAREN => paren_depth += 1,
            SyntaxKind::R_PAREN => {
                if paren_depth == 1 {
                    end_offset = Some(token.text_range().end());
                    break;
                }
                paren_depth -= 1;
            }
            _ => {}
        }
    }

    end_offset.map(|end| TextRange::new(start_offset, end))
}

fn find_if_expression(if_stmt: &SyntaxNode) -> Option<SyntaxNode> {
    if_stmt.children().find(|n| n.kind() == SyntaxKind::EXPR)
}

fn find_elsif_expression(elsif_clause: &SyntaxNode) -> Option<SyntaxNode> {
    elsif_clause.children().find(|n| n.kind() == SyntaxKind::EXPR)
}

fn create_diagnostic(range: TextRange) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::IsInRoleMethod,
        message: "Для проверки прав доступа в коде следует использовать метод ПравоДоступа"
            .to_string(),
        severity: Severity::Major,
        range,
        tags: vec![],
        fixes: vec![],
    }
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
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/IsInRoleMethodDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 3, "Should match Java: 3 diagnostics");

        // Line 33 (0-indexed 32), cols 9-35: Direct РольДоступна() in if
        assert_diagnostic_range(&file_content, &diagnostics[0], 32, 9, 35);

        // Line 39 (0-indexed 38), cols 9-23: Variable ДоступРазрешен in if
        assert_diagnostic_range(&file_content, &diagnostics[1], 38, 9, 23);

        // Line 57 (0-indexed 56), cols 14-40: Direct РольДоступна() in elsif
        assert_diagnostic_range(&file_content, &diagnostics[2], 56, 14, 40);
    }

    #[test]
    fn test_direct_call_without_protection() {
        let code = r#"
Процедура Тест()
    Если РольДоступна("Роль") Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_direct_call_with_protection() {
        let code = r#"
Процедура Тест()
    Если РольДоступна("Роль") ИЛИ ПривилегированныйРежим() Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_variable_without_protection() {
        let code = r#"
Процедура Тест()
    Доступ = РольДоступна("Роль");
    Если Доступ Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_variable_with_protection() {
        let code = r#"
Процедура Тест()
    Доступ = РольДоступна("Роль");
    Если Доступ ИЛИ ПривилегированныйРежим() Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_variable_reassignment_clears_tracking() {
        let code = r#"
Процедура Тест()
    Доступ = РольДоступна("Роль");
    Доступ = Ложь;
    Если Доступ Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Reassignment should clear tracking");
    }

    #[test]
    fn test_elsif_clause() {
        let code = r#"
Процедура Тест()
    Если Ложь Тогда
    ИначеЕсли РольДоступна("Роль") Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Если РОЛЬДОСТУПНА("Роль") Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    If IsInRole("Role") Then
    EndIf;
EndProcedure
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_privileged_mode_variable() {
        let code = r#"
Процедура Тест()
    ПР = ПривилегированныйРежим();
    Если РольДоступна("Роль") ИЛИ ПР Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "PrivilegedMode variable should protect");
    }
}
