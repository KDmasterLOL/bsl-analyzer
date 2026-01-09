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
//!
//! **Architecture:** HIR-based diagnostic with AST fallback for commentAsCode.
//!
//! ### HIR approach
//! - Scans `Stmt::Try { body, except }` in method bodies and module-level code
//! - Empty except detected via `except.is_empty()`
//! - For commentAsCode option: uses source_map to get range, then checks AST for comments
//!
//! ### Advantages over AST
//! - Semantic analysis - operates on lowered HIR representation
//! - Salsa caching - benefits from automatic invalidation
//! - Simpler code - direct check on except array
//! - Better error recovery - HIR handles parse errors gracefully

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use hir_def::{
    hir::{Stmt, StmtId},
    ModuleId,
};
use syntax::SyntaxKind;

/// Main entry point for MissingCodeTryCatchEx diagnostic.
///
/// HIR-based check for empty exception handlers in try-catch blocks.
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

    let mut diagnostics = Vec::new();
    let module_id = ModuleId { file_id: ctx.file_id };
    let module_bodies = ctx.db.module_bodies(module_id);

    // 3. Check method bodies
    for (_local_id, body, source_map) in module_bodies.method_bodies() {
        check_body_for_empty_except(body, source_map, comment_as_code, ctx, &mut diagnostics);
    }

    // 4. Check module-level code
    if let Some(lower_result) = module_bodies.module_code_result() {
        check_body_for_empty_except(
            &lower_result.body,
            &lower_result.source_map,
            comment_as_code,
            ctx,
            &mut diagnostics,
        );
    }

    // 5. Sort diagnostics by position
    diagnostics.sort_by_key(|d| d.range.start());

    diagnostics
}

/// Check a single body (method or module-level code) for empty except blocks.
fn check_body_for_empty_except(
    body: &hir_def::Body,
    source_map: &hir_def::body::BodySourceMap,
    comment_as_code: bool,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Recursively scan all statements
    for stmt_id in body.body_stmts.iter() {
        check_stmt_recursive(*stmt_id, body, source_map, comment_as_code, ctx, diagnostics);
    }
}

/// Recursively check statement and nested statements for Try blocks.
fn check_stmt_recursive(
    stmt_id: StmtId,
    body: &hir_def::Body,
    source_map: &hir_def::body::BodySourceMap,
    comment_as_code: bool,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let stmt = body.stmt(stmt_id);

    // Check if this is a Try statement
    if let Stmt::Try { body: try_body, except } = stmt {
        // Empty except block?
        if except.is_empty() {
            // If commentAsCode=true, check if except block has comments via AST fallback
            if comment_as_code {
                // Need to get the range of the except clause and check for comments
                // For now, we'll use source_map to get the stmt range, then parse AST
                if let Some(stmt_range) = source_map.stmt_range(stmt_id) {
                    // Parse AST at this range to check for comments
                    let parse = ctx.db.parse(ctx.file_id);
                    let root = parse.syntax_node();

                    // Find the TRY_STMT node at this range
                    if let Some(try_node) = root
                        .descendants()
                        .find(|n| n.kind() == SyntaxKind::TRY_STMT && n.text_range() == stmt_range)
                    {
                        // Find EXCEPT_CLAUSE and check for comments
                        if let Some(except_clause) =
                            try_node.children().find(|n| n.kind() == SyntaxKind::EXCEPT_CLAUSE)
                        {
                            if has_comments_in_node(&except_clause) {
                                // Has comments, skip diagnostic
                            } else {
                                // No comments, report diagnostic
                                report_diagnostic_at_except(&try_node, diagnostics);
                            }
                        } else {
                            // No except clause found (shouldn't happen), skip
                        }
                    } else {
                        // Couldn't find AST node, report diagnostic anyway
                        if let Some(range) = source_map.stmt_range(stmt_id) {
                            diagnostics.push(Diagnostic {
                                code: DiagnosticCode::MissingCodeTryCatchEx,
                                message: "Отсутствует код в блоке исключения".to_string(),
                                severity: Severity::Error,
                                range,
                                tags: vec![],
                                fixes: vec![],
                            });
                        }
                    }
                } else {
                    // No source range, skip
                }
            } else {
                // commentAsCode=false, report diagnostic directly
                // Find the EXCEPT keyword position via AST fallback
                if let Some(stmt_range) = source_map.stmt_range(stmt_id) {
                    let parse = ctx.db.parse(ctx.file_id);
                    let root = parse.syntax_node();

                    if let Some(try_node) = root
                        .descendants()
                        .find(|n| n.kind() == SyntaxKind::TRY_STMT && n.text_range() == stmt_range)
                    {
                        report_diagnostic_at_except(&try_node, diagnostics);
                    } else {
                        // Fallback: use stmt range
                        diagnostics.push(Diagnostic {
                            code: DiagnosticCode::MissingCodeTryCatchEx,
                            message: "Отсутствует код в блоке исключения".to_string(),
                            severity: Severity::Error,
                            range: stmt_range,
                            tags: vec![],
                            fixes: vec![],
                        });
                    }
                }
            }
        }

        // Recursively check nested statements in try body
        for &nested_stmt_id in try_body.iter() {
            check_stmt_recursive(
                nested_stmt_id,
                body,
                source_map,
                comment_as_code,
                ctx,
                diagnostics,
            );
        }

        // Recursively check nested statements in except body
        for &nested_stmt_id in except.iter() {
            check_stmt_recursive(
                nested_stmt_id,
                body,
                source_map,
                comment_as_code,
                ctx,
                diagnostics,
            );
        }
    } else {
        // Recursively check nested statements for other statement types
        match stmt {
            Stmt::If { then_branch, elsif_branches, else_branch, .. } => {
                for &nested in then_branch.iter() {
                    check_stmt_recursive(
                        nested,
                        body,
                        source_map,
                        comment_as_code,
                        ctx,
                        diagnostics,
                    );
                }
                for (_, branch) in elsif_branches.iter() {
                    for &nested in branch.iter() {
                        check_stmt_recursive(
                            nested,
                            body,
                            source_map,
                            comment_as_code,
                            ctx,
                            diagnostics,
                        );
                    }
                }
                if let Some(branch) = else_branch {
                    for &nested in branch.iter() {
                        check_stmt_recursive(
                            nested,
                            body,
                            source_map,
                            comment_as_code,
                            ctx,
                            diagnostics,
                        );
                    }
                }
            }
            Stmt::While { body: while_body, .. } => {
                for &nested in while_body.iter() {
                    check_stmt_recursive(
                        nested,
                        body,
                        source_map,
                        comment_as_code,
                        ctx,
                        diagnostics,
                    );
                }
            }
            Stmt::For { body: for_body, .. } | Stmt::ForEach { body: for_body, .. } => {
                for &nested in for_body.iter() {
                    check_stmt_recursive(
                        nested,
                        body,
                        source_map,
                        comment_as_code,
                        ctx,
                        diagnostics,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Check if a node contains any comments (AST fallback).
fn has_comments_in_node(node: &syntax::SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::COMMENT)
}

/// Report diagnostic at EXCEPT keyword position (AST fallback for precise location).
fn report_diagnostic_at_except(try_node: &syntax::SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    // Find EXCEPT keyword
    if let Some(except_token) = try_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::KW_EXCEPT)
    {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::MissingCodeTryCatchEx,
            message: "Отсутствует код в блоке исключения".to_string(),
            severity: Severity::Error,
            range: except_token.text_range(),
            tags: vec![],
            fixes: vec![],
        });
    }
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

        // Set up source root for HIR-based diagnostics
        use ide_db::base_db::{SourceRoot, SourceRootId};
        use vfs::VfsPath;

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file content
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
            file_set: None,
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
