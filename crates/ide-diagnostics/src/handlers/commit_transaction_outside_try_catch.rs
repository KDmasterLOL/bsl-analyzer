//! CommitTransactionOutsideTryCatch diagnostic.
//!
//! Checks that `CommitTransaction()`/`ЗафиксироватьТранзакцию()` calls are properly protected by try-catch blocks.
//!
//! ## Why?
//! Committing a transaction should be inside try-catch to ensure:
//! - Rollback happens if commit fails or subsequent code throws
//! - Prevents partial data commits
//! - Proper error handling for transaction completion
//! - Database integrity protection
//!
//! CommitTransaction must be:
//! - Inside a Try block (not exception handler)
//! - Last statement in Try block (no code after)
//! - Try block must have Except handler
//!
//! ## Bad practice
//! ```bsl
//! Процедура Тест()
//!     НачатьТранзакцию();
//!     ЗаписатьДанные();
//!     ЗафиксироватьТранзакцию(); // Outside try-catch!
//! КонецПроцедуры
//!
//! Процедура Тест2()
//!     НачатьТранзакцию();
//!     Попытка
//!         ЗаписатьДанные();
//!         ЗафиксироватьТранзакцию();
//!         Метод2(); // Code after commit - wrong!
//!     Исключение
//!         ОтменитьТранзакцию();
//!     КонецПопытки;
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Тест()
//!     НачатьТранзакцию();
//!     Попытка
//!         ЗаписатьДанные();
//!         ЗафиксироватьТранзакцию(); // Last in try, before Except
//!     Исключение
//!         ОтменитьТранзакцию();
//!         ВызватьИсключение;
//!     КонецПопытки;
//! КонецПроцедуры
//! ```
//!
//! ## Implementation
//!
//! Ported from:
//! - CommitTransactionOutsideTryCatchDiagnostic.java (bsl-language-server) - PRIMARY
//! - commit_transaction_outside_try_catch.rs (bsl-language-server-rust) - REFERENCE
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode};

/// Main entry point for CommitTransactionOutsideTryCatch diagnostic.
///
/// Detects four violation patterns:
/// 1. Outside try-catch entirely
/// 2. Inside exception handler (should be in try body)
/// 3. Try without except clause
/// 4. Code after commit in try body
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CommitTransactionOutsideTryCatch) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Optimized: Build token stream once and find all commit calls
    let tokens: Vec<_> = root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    for (i, token) in tokens.iter().enumerate() {
        if token.kind() != SyntaxKind::IDENT {
            continue;
        }

        // Check if this is CommitTransaction/ЗафиксироватьТранзакцию
        let name = token.text().to_lowercase();
        if name != "зафиксироватьтранзакцию" && name != "committransaction" {
            continue;
        }

        // Check pattern: IDENT ( but not .IDENT(
        let next_is_lparen =
            tokens.get(i + 1).map(|t| t.kind() == SyntaxKind::L_PAREN).unwrap_or(false);

        if !next_is_lparen {
            continue;
        }

        let prev_is_dot = i
            .checked_sub(1)
            .and_then(|idx| tokens.get(idx))
            .map(|t| t.kind() == SyntaxKind::DOT)
            .unwrap_or(false);

        if prev_is_dot {
            continue; // Skip qualified calls
        }

        // Found global CommitTransaction call - get parent CALL_STMT
        if let Some(parent) = token.parent() {
            if let Some(call_stmt) = find_parent_call_stmt(&parent) {
                if !is_properly_protected(&call_stmt) {
                    diagnostics.push(make_diagnostic(&call_stmt));
                }
            }
        }
    }

    diagnostics
}

/// Find parent CALL_STMT node.
fn find_parent_call_stmt(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.ancestors().find(|n| n.kind() == SyntaxKind::CALL_STMT)
}

/// Check if a statement is a global CommitTransaction/ЗафиксироватьТранзакцию call (old version).
///
/// Filters out:
/// - Non-CALL_STMT nodes
/// - Qualified calls like `Connector.CommitTransaction()`
/// - Calls with different method names
///
/// Matches (case-insensitive):
/// - `ЗафиксироватьТранзакцию()`
/// - `CommitTransaction()`
#[allow(dead_code)]
fn is_global_commit_transaction_call(stmt: &SyntaxNode) -> bool {
    if stmt.kind() != SyntaxKind::CALL_STMT {
        return false;
    }

    // Skip if contains FIELD_EXPR (qualified call like Object.Method())
    if stmt.descendants().any(|n| n.kind() == SyntaxKind::FIELD_EXPR) {
        return false;
    }

    // Get first identifier token (method name)
    let ident = stmt
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT);

    let Some(ident) = ident else {
        return false;
    };

    let name = ident.text().to_lowercase();
    name == "зафиксироватьтранзакцию" || name == "committransaction"
}

/// Check if CommitTransaction call is properly protected.
///
/// A commit is properly protected if ALL of these are true:
/// 1. Inside TRY_STMT
/// 2. NOT inside EXCEPT_CLAUSE (should be in try body)
/// 3. TRY_STMT has EXCEPT_CLAUSE
/// 4. NO executable code after commit before EXCEPT_CLAUSE
fn is_properly_protected(commit: &SyntaxNode) -> bool {
    let Some((try_stmt, commit_ancestor)) = find_enclosing_try_stmt(commit) else {
        return false;
    };

    if is_inside_except_clause(commit, &try_stmt) {
        return false;
    }

    if !has_except_clause(&try_stmt) {
        return false;
    }

    if has_code_after_in_try_body(&commit_ancestor, &try_stmt) {
        return false;
    }

    true
}

/// Find the enclosing TRY_STMT and the commit's ancestor that is a direct child of it.
///
/// Returns (try_stmt, commit_ancestor) where commit_ancestor is the node containing
/// the commit that is a direct child of try_stmt.
fn find_enclosing_try_stmt(commit: &SyntaxNode) -> Option<(SyntaxNode, SyntaxNode)> {
    let mut current = commit.clone();

    while let Some(parent) = current.parent() {
        if parent.kind() == SyntaxKind::TRY_STMT {
            return Some((parent, current));
        }
        current = parent;
    }

    None
}

/// Check if a node is inside an exception handler (between EXCEPT_KEYWORD and ENDTRY_KEYWORD).
///
/// Commits should be in the try body, not in the exception handler.
fn is_inside_except_clause(commit: &SyntaxNode, try_stmt: &SyntaxNode) -> bool {
    // Find EXCEPT_CLAUSE in try_stmt children
    let except_clause = try_stmt.children().find(|n| n.kind() == SyntaxKind::EXCEPT_CLAUSE);

    let Some(except_clause) = except_clause else {
        return false;
    };

    // Check if commit is a descendant of except_clause
    except_clause.descendants().any(|n| n.text_range() == commit.text_range())
}

/// Check if try statement has an except clause.
fn has_except_clause(try_stmt: &SyntaxNode) -> bool {
    try_stmt.children().any(|n| n.kind() == SyntaxKind::EXCEPT_CLAUSE)
}

/// Check if there's executable code after commit in try body before EXCEPT_CLAUSE.
///
/// Algorithm:
/// 1. If commit_ancestor is STMT_LIST, check statements inside STMT_LIST after commit
/// 2. Otherwise, check siblings of commit_ancestor (before EXCEPT_CLAUSE)
fn has_code_after_in_try_body(commit_ancestor: &SyntaxNode, try_stmt: &SyntaxNode) -> bool {
    // If commit_ancestor is STMT_LIST, we need to look inside it
    if commit_ancestor.kind() == SyntaxKind::STMT_LIST {
        // Find the commit CALL_STMT within this STMT_LIST
        let commit_stmt = commit_ancestor
            .descendants()
            .find(|n| n.kind() == SyntaxKind::CALL_STMT && is_global_commit_transaction_call(n));

        let Some(commit_stmt) = commit_stmt else {
            return false;
        };

        // Check siblings after commit within STMT_LIST
        let mut found_commit = false;
        for child in commit_ancestor.children() {
            if found_commit && is_executable_statement(&child) {
                return true;
            }
            if child.text_range() == commit_stmt.text_range() {
                found_commit = true;
            }
        }

        return false;
    }

    // Otherwise, check siblings of commit_ancestor before EXCEPT_CLAUSE
    let children: Vec<_> = try_stmt.children().collect();

    let commit_pos =
        match children.iter().position(|n| n.text_range() == commit_ancestor.text_range()) {
            Some(pos) => pos,
            None => return false,
        };

    for child in &children[commit_pos + 1..] {
        if child.kind() == SyntaxKind::EXCEPT_CLAUSE {
            break;
        }
        if is_executable_statement(child) {
            return true;
        }
    }

    false
}

/// Check if a node is an executable statement.
///
/// Executable statements include:
/// - CALL_STMT, ASSIGN_STMT, RETURN_STMT
/// - Control flow: IF_STMT, FOR_STMT, WHILE_STMT, FOR_EACH_STMT
/// - Try blocks, loops: TRY_STMT, BREAK_STMT, CONTINUE_STMT
/// - Other: RAISE_STMT, GOTO_STMT, EXECUTE_STMT
///
/// Excludes:
/// - Keywords (KW_TRY, EXCEPT_KEYWORD, etc.)
/// - Whitespace, comments
/// - Statement lists (STMT_LIST)
fn is_executable_statement(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::CALL_STMT
            | SyntaxKind::ASSIGN_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::RAISE_STMT
            | SyntaxKind::GOTO_STMT
            | SyntaxKind::EXECUTE_STMT
    )
}

/// Create a diagnostic for CommitTransaction violation.
///
/// Extends range to include SEMICOLON token to match Java behavior.
fn make_diagnostic(node: &SyntaxNode) -> Diagnostic {
    use syntax::NodeOrToken;

    let mut range = node.text_range();

    // Extend range to include SEMICOLON token (for Java compatibility)
    if let Some(NodeOrToken::Token(token)) = node.next_sibling_or_token() {
        if token.kind() == SyntaxKind::SEMICOLON {
            range = range.cover(token.text_range());
        }
    }

    Diagnostic {
        code: DiagnosticCode::CommitTransactionOutsideTryCatch,
        message: "Вызов 'ЗафиксироватьТранзакцию'/'CommitTransaction' должен быть размещен в блоке 'Попытка' с обработчиком 'Исключение'".to_string(),
        severity: Severity::Error,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
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
        let config = crate::DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_valid_inside_try() {
        let code = r#"
Процедура Пример1()
    НачатьТранзакцию();
    Попытка
        БлокировкаДанных = Новый БлокировкаДанных;
        ДокументОбъект.Записать();
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_outside_try() {
        let code = r#"
Процедура Пример2()
    НачатьТранзакцию();
    Метод();
    ЗафиксироватьТранзакцию();
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(code, &diagnostics[0], 4, 4, 30);
    }

    #[test]
    fn test_in_exception_handler() {
        let code = r#"
Процедура Пример3()
    НачатьТранзакцию();
    Попытка
        Метод();
    Исключение
        Если ТранзакцияАктивна() Тогда
            ЗафиксироватьТранзакцию();
        Иначе
            ОтменитьТранзакцию();
        КонецЕсли;
    КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(code, &diagnostics[0], 7, 12, 38);
    }

    #[test]
    fn test_code_after_commit() {
        let code = r#"
Процедура Пример6()
    НачатьТранзакцию();
    Попытка
        Метод();
        ЗафиксироватьТранзакцию();
        Метод2();
    Исключение
        ОтменитьТранзакцию();
        Возврат;
    КонецПопытки;
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(code, &diagnostics[0], 5, 8, 34);
    }

    #[test]
    fn test_qualified_call_ignored() {
        let code = r#"
Процедура Тест()
    Коннектор.ЗафиксироватьТранзакцию();
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_english_keyword() {
        let code = r#"
Процедура Test()
    BeginTransaction();
    Method();
    CommitTransaction();
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(code, &diagnostics[0], 4, 4, 24);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    НачатьТранзакцию();
    Метод();
    ЗАФИКСИРОВАТЬТРАНЗАКЦИЮ();
КонецПроцедуры
"#;
        let diagnostics = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/CommitTransactionOutsideTryCatchDiagnostic.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 8);

        // Line numbers are 1-indexed in file, 0-indexed in Rowan
        assert_diagnostic_range(code, &diagnostics[0], 36, 4, 30);
        assert_diagnostic_range(code, &diagnostics[1], 45, 12, 38);
        assert_diagnostic_range(code, &diagnostics[2], 57, 8, 34);
        assert_diagnostic_range(code, &diagnostics[3], 66, 4, 30);
        assert_diagnostic_range(code, &diagnostics[4], 74, 8, 34);
        assert_diagnostic_range(code, &diagnostics[5], 86, 8, 34);
        assert_diagnostic_range(code, &diagnostics[6], 98, 8, 34);
        assert_diagnostic_range(code, &diagnostics[7], 106, 0, 26);
    }

    #[test]
    fn test_comprehensive_single_sub() {
        let code =
            include_str!("../../test_data/CommitTransactionOutsideTryCatchDiagnosticSingleSub.bsl");
        let diagnostics = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 1);
        assert_diagnostic_range(code, &diagnostics[0], 3, 4, 30);
    }
}
