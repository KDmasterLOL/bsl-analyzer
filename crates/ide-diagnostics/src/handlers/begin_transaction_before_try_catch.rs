//! BeginTransactionBeforeTryCatch diagnostic.
//!
//! Checks that `BeginTransaction()`/`НачатьТранзакцию()` calls are immediately followed by `Try-Catch` blocks.
//!
//! ## Why?
//! Starting a transaction without proper error handling is dangerous:
//! - Uncommitted transactions can lock database
//! - Data corruption if transaction is not rolled back on error
//! - Resource leaks
//! - Must ensure transaction is always finalized (commit or rollback)
//!
//! ## Bad practice
//! ```bsl
//! Процедура Тест()
//!     НачатьТранзакцию();
//!     // If error occurs here, transaction is left open!
//!     ЗаписатьДанные();
//!     ЗафиксироватьТранзакцию();
//! КонецПроцедуры
//!
//! Процедура Тест2()
//!     НачатьТранзакцию();
//!     Метод(); // ← Code between BeginTransaction and Try
//!     Попытка
//!         ЗаписатьДанные();
//!         ЗафиксироватьТранзакцию();
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
//!         ЗафиксироватьТранзакцию();
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
//! - BeginTransactionBeforeTryCatchDiagnostic.java (bsl-language-server) - PRIMARY
//! - begin_transaction_before_try_catch.rs (bsl-language-server-rust) - REFERENCE
//!
//! Adapted to use Rowan SyntaxNode instead of tree-sitter.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode};

/// Main entry point for BeginTransactionBeforeTryCatch diagnostic.
///
/// Detects three violation patterns:
/// 1. Code between BeginTransaction and Try: `BeginTransaction(); Code(); Try...` → ERROR
/// 2. BeginTransaction inside Try block: `Try { BeginTransaction(); ... }` → ERROR
/// 3. BeginTransaction without subsequent Try: `BeginTransaction(); /* no Try */` → ERROR
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::BeginTransactionBeforeTryCatch) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Find all statement lists (procedure bodies, loops, module-level code blocks)
    for stmt_list in root.descendants().filter(|n| n.kind() == SyntaxKind::STMT_LIST) {
        check_stmt_list(&stmt_list, &mut diagnostics);
    }

    // Also check module-level statements (direct children of root)
    check_stmt_list(&root, &mut diagnostics);

    diagnostics
}

/// Check a single statement list for BeginTransaction violations.
///
/// Uses a state machine approach (adapted from Java implementation):
/// - Track the last seen BeginTransaction call
/// - If we see a Try statement, it "consumes" the pending BeginTransaction (valid case)
/// - If we see ANY other statement while BeginTransaction is pending → ERROR
fn check_stmt_list(stmt_list: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    // Get all named statements (skip whitespace/comments)
    let statements: Vec<_> = stmt_list.children().filter(is_statement).collect();

    let mut pending_begin_transaction: Option<SyntaxNode> = None;

    for stmt in statements {
        // If we see a Try, it "consumes" pending BeginTransaction (valid case)
        if stmt.kind() == SyntaxKind::TRY_STMT {
            pending_begin_transaction = None;
            continue;
        }

        // Check if current statement is global BeginTransaction
        let is_begin_trans = is_global_begin_transaction_call(&stmt);

        if is_begin_trans {
            // If we have pending BeginTransaction, current one creates error on pending
            if let Some(node) = pending_begin_transaction.take() {
                diagnostics.push(make_diagnostic(&node));
            }

            // Violation: BeginTransaction inside Try body
            if is_inside_try_body(&stmt) {
                diagnostics.push(make_diagnostic(&stmt));
            } else {
                // Store as pending (will be consumed by Try or reported as error)
                pending_begin_transaction = Some(stmt.clone());
            }
        } else {
            // Any other statement (not Try, not BeginTransaction) while pending → ERROR
            if let Some(node) = pending_begin_transaction.take() {
                diagnostics.push(make_diagnostic(&node));
            }
        }
    }

    // If there's still pending BeginTransaction at end of list → ERROR
    if let Some(node) = pending_begin_transaction {
        diagnostics.push(make_diagnostic(&node));
    }
}

/// Check if a statement is a global BeginTransaction/НачатьТранзакцию call.
///
/// Filters out:
/// - Non-CALL_STMT nodes
/// - Qualified calls like `Connector.BeginTransaction()`
/// - Calls with different method names
///
/// Matches (case-insensitive):
/// - `НачатьТранзакцию()`
/// - `BeginTransaction()`
fn is_global_begin_transaction_call(stmt: &SyntaxNode) -> bool {
    // Must be CALL_STMT
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
    name == "начатьтранзакцию" || name == "begintransaction"
}

/// Check if a node is inside a Try-Catch block body.
///
/// Walks up the AST tree looking for TRY_STMT ancestors.
///
/// Note: Our parser structures Try statements as TRY_STMT nodes.
/// We check if the node is a descendant of any TRY_STMT.
fn is_inside_try_body(node: &SyntaxNode) -> bool {
    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        if parent.kind() == SyntaxKind::TRY_STMT {
            // Found a Try statement ancestor - node is inside Try body
            return true;
        }
        current = parent;
    }
    false
}

/// Filter out whitespace, comments, and other non-statement nodes.
fn is_statement(node: &SyntaxNode) -> bool {
    !matches!(node.kind(), SyntaxKind::WHITESPACE | SyntaxKind::COMMENT | SyntaxKind::NEWLINE)
}

/// Create a diagnostic for BeginTransaction violation.
fn make_diagnostic(node: &SyntaxNode) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::BeginTransactionBeforeTryCatch,
        message: "Метод 'НачатьТранзакцию' должен быть за пределами блока 'Попытка-Исключение' непосредственно перед оператором 'Попытка'".to_string(),
        severity: Severity::Error,
        range: node.text_range(),
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

        // Use Rc instead of Arc since tests are single-threaded
        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = crate::DiagnosticsConfig::default();
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
    fn test_valid_before_try() {
        let code = r#"Процедура Тест()
    НачатьТранзакцию();
    Попытка
        ЗаписатьДанные();
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "BeginTransaction immediately before Try should be valid");
    }

    #[test]
    fn test_code_between() {
        let code = r#"Процедура Тест()
    НачатьТранзакцию();
    Метод();
    Попытка
        ЗаписатьДанные();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры"#;

        let (diagnostics, file_content) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Code between BeginTransaction and Try should be error");
        assert_diagnostic_range(&file_content, &diagnostics[0], 1, 4, 22);
    }

    #[test]
    fn test_inside_try() {
        let code = r#"Процедура Тест()
    Попытка
        НачатьТранзакцию();
        ЗаписатьДанные();
        ЗафиксироватьТранзакцию();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры"#;

        let (diagnostics, file_content) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "BeginTransaction inside Try should be error");
        assert_diagnostic_range(&file_content, &diagnostics[0], 2, 8, 26);
    }

    #[test]
    fn test_no_try_after() {
        let code = r#"Процедура Тест()
    НачатьТранзакцию();
    ЗаписатьДанные();
    ЗафиксироватьТранзакцию();
КонецПроцедуры"#;

        let (diagnostics, file_content) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "BeginTransaction without Try should be error");
        assert_diagnostic_range(&file_content, &diagnostics[0], 1, 4, 22);
    }

    #[test]
    fn test_qualified_call_ignored() {
        let code = r#"Процедура Тест()
    Коннектор.НачатьТранзакцию();
    ЗаписатьДанные();
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Qualified call should be ignored");
    }

    #[test]
    fn test_english_keyword() {
        let code = r#"Procedure Test()
    BeginTransaction();
    SaveData();
EndProcedure"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "English BeginTransaction should be detected");
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"Процедура Тест()
    НАЧАТЬТРАНЗАКЦИЮ();
    Данные();
КонецПроцедуры"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1, "Case-insensitive matching should work");
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/BeginTransactionBeforeTryCatchDiagnostic.bsl");

        let (diagnostics, file_content) = check_diagnostic(code);

        // Java expects 7 diagnostics with exact ranges (from BeginTransactionBeforeTryCatchDiagnosticTest.java)
        // Lines are 1-indexed in Java, 0-indexed in our fixture (with prefix "//- /test.bsl\n")
        assert_eq!(diagnostics.len(), 7, "Should match Java implementation (7 diagnostics)");

        // Verify exact positions match Java test expectations
        // Java format: .hasRange(line, startCol, line, endCol) where line is 1-indexed
        // Our format: assert_diagnostic_range(content, diag, line, startCol, endCol) where line is 0-indexed
        assert_diagnostic_range(&file_content, &diagnostics[0], 29, 4, 22); // Line 30 in Java (НачатьТранзакцію)
        assert_diagnostic_range(&file_content, &diagnostics[1], 42, 8, 26); // Line 43 in Java
        assert_diagnostic_range(&file_content, &diagnostics[2], 55, 4, 22); // Line 56 in Java
        assert_diagnostic_range(&file_content, &diagnostics[3], 68, 8, 26); // Line 69 in Java
        assert_diagnostic_range(&file_content, &diagnostics[4], 77, 4, 22); // Line 78 in Java
        assert_diagnostic_range(&file_content, &diagnostics[5], 90, 4, 22); // Line 91 in Java
        assert_diagnostic_range(&file_content, &diagnostics[6], 102, 0, 18); // Line 103 in Java (НачатьТранзакцию)
    }
}
