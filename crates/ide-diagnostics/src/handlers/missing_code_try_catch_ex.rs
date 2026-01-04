//! MissingCodeTryCatchEx diagnostic.
//!
//! Detects empty exception handlers in try-catch blocks.
//!
//! ## Why?
//! Empty exception handlers silently swallow errors, making debugging difficult:
//! - Errors are hidden from logs
//! - Application continues with invalid state
//! - Root cause analysis becomes impossible
//!
//! ## Bad practice
//! ```bsl
//! Попытка
//!     ОпаснаяОперация();
//! Исключение
//!     // Empty - error is silently ignored!
//! КонецПопытки;
//! ```
//!
//! ## Good practice
//! ```bsl
//! Попытка
//!     ОпаснаяОперация();
//! Исключение
//!     ЗаписатьЛогСобытий("Ошибка", УровеньЛога.Ошибка, ОписаниеОшибки());
//!     ВызватьИсключение;
//! КонецПопытки;
//! ```
//!
//! ## Configuration
//! - `commentAsCode` (boolean, default: false) - If true, exception blocks containing
//!   only comments are NOT considered empty
//!
//! ## Implementation
//! Ported from:
//! - MissingCodeTryCatchExDiagnostic.java (bsl-language-server) - PRIMARY

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// Main entry point for MissingCodeTryCatchEx diagnostic.
///
/// Detects empty exception handlers in try-catch blocks.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // 1. Early exit if disabled
    if ctx.config.is_disabled(DiagnosticCode::MissingCodeTryCatchEx) {
        return Vec::new();
    }

    // 2. Get commentAsCode parameter (default: false)
    let comment_as_code = ctx
        .config
        .get_bool(DiagnosticCode::MissingCodeTryCatchEx, "commentAsCode")
        .unwrap_or(false);

    // 3. Parse file and find all TRY_STMT nodes
    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for try_stmt in root.descendants().filter(|n| n.kind() == SyntaxKind::TRY_STMT) {
        check_try_statement(&try_stmt, comment_as_code, &mut diagnostics);
    }

    diagnostics
}

/// Check a single try-catch block for empty exception handler.
fn check_try_statement(
    try_stmt: &SyntaxNode,
    comment_as_code: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // 1. Find EXCEPT_CLAUSE child
    let Some(except_clause) = try_stmt.children().find(|n| n.kind() == SyntaxKind::EXCEPT_CLAUSE)
    else {
        return; // No except clause (malformed or incomplete parse)
    };

    // 2. Find STMT_LIST within EXCEPT_CLAUSE
    let Some(stmt_list) = except_clause.children().find(|n| n.kind() == SyntaxKind::STMT_LIST)
    else {
        // No STMT_LIST means empty exception handler
        report_diagnostic(try_stmt, diagnostics);
        return;
    };

    // 3. Check if STMT_LIST has any statement children
    let has_statements = stmt_list.children().any(|c| is_statement(c.kind()));

    if has_statements {
        return; // Has code, no diagnostic
    }

    // 4. If commentAsCode=true, check for comments
    if comment_as_code && has_comments_in_range(&except_clause) {
        return; // Has comments counting as code, no diagnostic
    }

    // 5. Report diagnostic on EXCEPT keyword
    report_diagnostic(try_stmt, diagnostics);
}

/// Check if a node represents a statement.
///
/// Reused pattern from empty_code_block.rs.
fn is_statement(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ASSIGN_STMT
            | SyntaxKind::CALL_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::RAISE_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::GOTO_STMT
            | SyntaxKind::LABEL_STMT
            | SyntaxKind::EXECUTE_STMT
            | SyntaxKind::ADD_HANDLER_STMT
            | SyntaxKind::REMOVE_HANDLER_STMT
    )
}

/// Check if the EXCEPT_CLAUSE contains any comments.
///
/// Used when commentAsCode=true to suppress diagnostic for exception handlers
/// that contain only comments.
fn has_comments_in_range(except_clause: &SyntaxNode) -> bool {
    except_clause
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::COMMENT)
}

/// Find the EXCEPT keyword token in a TRY_STMT.
///
/// The diagnostic is reported on this token.
fn find_except_keyword(try_stmt: &SyntaxNode) -> Option<SyntaxToken> {
    try_stmt
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::KW_EXCEPT)
}

/// Report a MissingCodeTryCatchEx diagnostic on the EXCEPT keyword.
fn report_diagnostic(try_stmt: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    let Some(except_token) = find_except_keyword(try_stmt) else {
        return; // Malformed syntax, skip
    };

    diagnostics.push(Diagnostic {
        code: DiagnosticCode::MissingCodeTryCatchEx,
        message: "Отсутствует код в блоке исключения".to_string(),
        severity: Severity::Error,
        range: except_token.text_range(),
        tags: vec![],
        fixes: vec![],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::assert_diagnostic_range, DiagnosticsConfig, DiagnosticsContext};
    use ide_db::base_db::SourceDatabase;
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let config = DiagnosticsConfig::default();
        check_diagnostic_with_config(code, config)
    }

    fn check_diagnostic_with_config(
        code: &str,
        config: DiagnosticsConfig,
    ) -> (Vec<Diagnostic>, String) {
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

        let db = Rc::new(db) as Rc<dyn ide_db::RootDatabase>;
        let config = Rc::new(config);
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
    fn test_missing_code_try_catch_ex() {
        let code = include_str!("../../test_data/MissingCodeTryCatchExDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        // Java expects 3 diagnostics at specific positions
        assert_eq!(diagnostics.len(), 3, "Should detect 3 empty exception handlers");

        // Line 23, columns 4-14 (Исключение keyword in Проц2)
        assert_diagnostic_range(&file_content, &diagnostics[0], 23, 4, 14);

        // Line 32, columns 4-14 (Исключение with only comments in Функ1)
        assert_diagnostic_range(&file_content, &diagnostics[1], 32, 4, 14);

        // Line 50, columns 8-18 (nested Исключение in Проц3)
        assert_diagnostic_range(&file_content, &diagnostics[2], 50, 8, 18);
    }

    #[test]
    fn test_comment_as_code() {
        let code = include_str!("../../test_data/MissingCodeTryCatchExDiagnostic.bsl");

        // Configure commentAsCode=true
        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("commentAsCode".to_string(), serde_json::Value::Bool(true));
        config
            .parameters
            .insert(DiagnosticCode::MissingCodeTryCatchEx, serde_json::Value::Object(params));

        let (diagnostics, file_content) = check_diagnostic_with_config(code, config);

        // Java expects 2 diagnostics (line 32 is now suppressed because it has comments)
        assert_eq!(diagnostics.len(), 2, "Should detect only 2 when comments count as code");

        // Line 23 still reported (no comments)
        assert_diagnostic_range(&file_content, &diagnostics[0], 23, 4, 14);

        // Line 50 still reported (no comments)
        assert_diagnostic_range(&file_content, &diagnostics[1], 50, 8, 18);
    }

    #[test]
    fn test_valid_exception_handlers() {
        let code = r#"
Процедура Проц1()
    Попытка
        Действие();
    Исключение
        ДействиеИсключения();
    КонецПопытки;
КонецПроцедуры
"#;

        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Valid exception handler should not trigger diagnostic");
    }
}
