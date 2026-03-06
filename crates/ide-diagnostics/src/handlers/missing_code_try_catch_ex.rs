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

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{IdConversion, Stmt, StmtId};
use syntax::SyntaxKind;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

/// Main entry point for MissingCodeTryCatchEx diagnostic.
///
/// HIR-based check for empty exception handlers in try-catch blocks.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::MissingCodeTryCatchEx;

    // 1. Early exit if disabled
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // 2. Get commentAsCode parameter (default: false)
    let comment_as_code = ctx
        .config
        .get_bool(DiagnosticCode::MissingCodeTryCatchEx, "commentAsCode")
        .unwrap_or(false);

    let mut diagnostics = Vec::new();
    let module_bodies = ctx.module_bodies();

    // 3. Check method bodies
    for (_local_id, body, source_map) in module_bodies.method_bodies() {
        check_body_for_empty_except(body, source_map, comment_as_code, code, ctx, &mut diagnostics);
    }

    // 4. Check module-level code
    if let Some(lower_result) = module_bodies.module_code_result() {
        check_body_for_empty_except(
            &lower_result.body,
            &lower_result.source_map,
            comment_as_code,
            code,
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
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Recursively scan all statements
    for stmt_id in body.body_stmts() {
        check_stmt_recursive(stmt_id, body, source_map, comment_as_code, code, ctx, diagnostics);
    }
}

/// Recursively check statement and nested statements for Try blocks.
fn check_stmt_recursive(
    stmt_id: StmtId,
    body: &hir_def::Body,
    source_map: &hir_def::body::BodySourceMap,
    comment_as_code: bool,
    code: DiagnosticCode,
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
                    let parse = ctx.parse();
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
                                report_diagnostic_at_except(&try_node, code, ctx, diagnostics);
                            }
                        } else {
                            // No except clause found (shouldn't happen), skip
                        }
                    } else {
                        // Couldn't find AST node, report diagnostic anyway
                        if let Some(range) = source_map.stmt_range(stmt_id) {
                            diagnostics.push(Diagnostic {
                                code,
                                message: "Отсутствует код в блоке исключения".to_string(),
                                severity: ctx.severity(code),
                                range,
                                tags: ctx.tags(code),
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
                    let parse = ctx.parse();
                    let root = parse.syntax_node();

                    if let Some(try_node) = root
                        .descendants()
                        .find(|n| n.kind() == SyntaxKind::TRY_STMT && n.text_range() == stmt_range)
                    {
                        report_diagnostic_at_except(&try_node, code, ctx, diagnostics);
                    } else {
                        // Fallback: use stmt range
                        diagnostics.push(Diagnostic {
                            code,
                            message: "Отсутствует код в блоке исключения".to_string(),
                            severity: ctx.severity(code),
                            range: stmt_range,
                            tags: ctx.tags(code),
                            fixes: vec![],
                        });
                    }
                }
            }
        }

        // Recursively check nested statements in try body
        for &nested_stmt_idx in try_body.iter() {
            check_stmt_recursive(
                StmtId::from_idx(nested_stmt_idx),
                body,
                source_map,
                comment_as_code,
                code,
                ctx,
                diagnostics,
            );
        }

        // Recursively check nested statements in except body
        for &nested_stmt_idx in except.iter() {
            check_stmt_recursive(
                StmtId::from_idx(nested_stmt_idx),
                body,
                source_map,
                comment_as_code,
                code,
                ctx,
                diagnostics,
            );
        }
    } else {
        // Recursively check nested statements for other statement types
        match stmt {
            Stmt::If(if_stmt) => {
                for &nested_idx in if_stmt.then_branch.iter() {
                    check_stmt_recursive(
                        StmtId::from_idx(nested_idx),
                        body,
                        source_map,
                        comment_as_code,
                        code,
                        ctx,
                        diagnostics,
                    );
                }
                for (_, branch) in if_stmt.elsif_branches.iter() {
                    for &nested_idx in branch.iter() {
                        check_stmt_recursive(
                            StmtId::from_idx(nested_idx),
                            body,
                            source_map,
                            comment_as_code,
                            code,
                            ctx,
                            diagnostics,
                        );
                    }
                }
                if let Some(ref branch) = if_stmt.else_branch {
                    for &nested_idx in branch.iter() {
                        check_stmt_recursive(
                            StmtId::from_idx(nested_idx),
                            body,
                            source_map,
                            comment_as_code,
                            code,
                            ctx,
                            diagnostics,
                        );
                    }
                }
            }
            Stmt::While { body: while_body, .. } => {
                for &nested_idx in while_body.iter() {
                    check_stmt_recursive(
                        StmtId::from_idx(nested_idx),
                        body,
                        source_map,
                        comment_as_code,
                        code,
                        ctx,
                        diagnostics,
                    );
                }
            }
            Stmt::For { body: for_body, .. } | Stmt::ForEach { body: for_body, .. } => {
                for &nested_idx in for_body.iter() {
                    check_stmt_recursive(
                        StmtId::from_idx(nested_idx),
                        body,
                        source_map,
                        comment_as_code,
                        code,
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
fn report_diagnostic_at_except(
    try_node: &syntax::SyntaxNode,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Find EXCEPT keyword
    if let Some(except_token) = try_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|tok| tok.kind() == SyntaxKind::KW_EXCEPT)
    {
        diagnostics.push(Diagnostic {
            code,
            message: "Отсутствует код в блоке исключения".to_string(),
            severity: ctx.severity(code),
            range: except_token.text_range(),
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_missing_code_try_catch_ex() {
        // Inline version of MissingCodeTryCatchExDiagnostic.bsl.
        // Uses 4-space indentation to match original column positions.
        let code = r#"Процедура Проц1()
    Попытка
        Действие();
    Исключение
        ДействиеИсключения();
    КонецПопытки;
КонецПроцедуры

Процедура Проц11()
    Попытка
        Действие();
    Исключение

        // просто коментарий

        ДействиеИсключения();
    КонецПопытки;
КонецПроцедуры

Процедура Проц2()
    // в исключении пустой блок, это ошибка
    Попытка
        Действие();
    Исключение

    КонецПопытки;
КонецПроцедуры

Функция Функ1()

    Попытка
        Действие();
    Исключение

        // в исключении просто комментарий, это ошибка
        // но иногда нет

    КонецПопытки;

    Возврат 1;
КонецФункции

Процедура Проц3()
    // в исключении другая попытка, не ошибка
    Попытка
        Действие();
    Исключение
        // в исключении пустой блок, это ошибка
        Попытка
            Действие2();
        Исключение

        КонецПопытки;
    КонецПопытки;
КонецПроцедуры"#;

        let diagnostics = check_ast_diagnostic(code, check);

        // Expected 3 diagnostics at specific positions
        assert_eq!(diagnostics.len(), 3, "Should detect 3 empty exception handlers");

        // Line 23, columns 4-14 (Исключение keyword in Проц2)
        assert_diagnostic_range(code, &diagnostics[0], 23, 4, 14);

        // Line 32, columns 4-14 (Исключение with only comments in Функ1)
        assert_diagnostic_range(code, &diagnostics[1], 32, 4, 14);

        // Line 50, columns 8-18 (nested Исключение in Проц3)
        assert_diagnostic_range(code, &diagnostics[2], 50, 8, 18);
    }

    #[test]
    fn test_comment_as_code() {
        // Same code as test_missing_code_try_catch_ex.
        let code = r#"Процедура Проц1()
    Попытка
        Действие();
    Исключение
        ДействиеИсключения();
    КонецПопытки;
КонецПроцедуры

Процедура Проц11()
    Попытка
        Действие();
    Исключение

        // просто коментарий

        ДействиеИсключения();
    КонецПопытки;
КонецПроцедуры

Процедура Проц2()
    // в исключении пустой блок, это ошибка
    Попытка
        Действие();
    Исключение

    КонецПопытки;
КонецПроцедуры

Функция Функ1()

    Попытка
        Действие();
    Исключение

        // в исключении просто комментарий, это ошибка
        // но иногда нет

    КонецПопытки;

    Возврат 1;
КонецФункции

Процедура Проц3()
    // в исключении другая попытка, не ошибка
    Попытка
        Действие();
    Исключение
        // в исключении пустой блок, это ошибка
        Попытка
            Действие2();
        Исключение

        КонецПопытки;
    КонецПопытки;
КонецПроцедуры"#;

        // Configure commentAsCode=true
        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("commentAsCode".to_string(), serde_json::Value::Bool(true));
        config
            .parameters
            .insert(DiagnosticCode::MissingCodeTryCatchEx, serde_json::Value::Object(params));

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Expected 2 diagnostics (line 32 is now suppressed because it has comments)
        assert_eq!(diagnostics.len(), 2, "Should detect only 2 when comments count as code");

        // Line 23 still reported (no comments)
        assert_diagnostic_range(code, &diagnostics[0], 23, 4, 14);

        // Line 50 still reported (no comments)
        assert_diagnostic_range(code, &diagnostics[1], 50, 8, 18);
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

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Valid exception handler should not trigger diagnostic");
    }
}
