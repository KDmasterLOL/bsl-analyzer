//! Reports `Исключение` / `Except` blocks that swallow errors silently
//! (no `Raise` / no logging API call) — including completely empty
//! blocks and blocks with only non-recovery code.
//!
//! Track 2 Phase D §2.2 migration: replaces the previous
//! "empty-only" heuristic with a [`hir::catch_class::CatchBodyClass`]
//! classifier-driven dispatch. The classifier categorises the
//! `Исключение` body into one of six classes; this handler emits
//! for `Empty` / `Silent` (and respects `commentAsCode` for the
//! `Empty` case via the existing AST fallback). `RaisesOnly` /
//! `LogsOnly` / `Mixed` are recognised as proper recovery paths
//! and skip emission.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::catch_class::{classify_catch_body, CatchBodyClass};
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

/// Main entry point for the empty-except diagnostic.
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

    // 3. Check all bodies (method bodies + module-level code)
    let mut diagnostics = crate::utils::for_each_body(ctx, |body, source_map, diags| {
        check_body_for_empty_except(body, source_map, comment_as_code, code, ctx, diags);
    });

    // 4. Sort diagnostics by position
    diagnostics.sort_by_key(|d| d.range.start());

    diagnostics
}

/// Check a single body (method or module-level code) for empty except blocks.
fn check_body_for_empty_except(
    body: &hir::Body,
    source_map: &hir::BodySourceMap,
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
    body: &hir::Body,
    source_map: &hir::BodySourceMap,
    comment_as_code: bool,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let stmt = body.stmt(stmt_id);

    // Check if this is a Try statement
    if let Stmt::Try { body: try_body, except } = stmt {
        let class = classify_catch_body(body, except);
        let should_emit = match class {
            // Empty body: emit unless commentAsCode is on AND the
            // EXCEPT_CLAUSE has comments (HIR drops trivia, so the
            // distinction lives at the AST layer).
            CatchBodyClass::Empty => {
                !(comment_as_code && except_clause_has_comments(stmt_id, source_map, ctx))
            }
            // Silent swallow: has statements but none propagate or
            // record the exception. Always emit.
            CatchBodyClass::Silent => true,
            // Rollback-only: reverts state but loses the failure.
            // Emit with rollback-specific guidance (recommend adding
            // log or `Raise` so the error is observable).
            CatchBodyClass::RollbackOnly => true,
            // Real recovery paths — proper raise / log / mixed.
            CatchBodyClass::RaisesOnly | CatchBodyClass::LogsOnly | CatchBodyClass::Mixed => false,
        };

        if should_emit {
            emit_at_except_keyword(stmt_id, source_map, code, ctx, diagnostics, class);
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

/// AST-side query: does the `EXCEPT_CLAUSE` corresponding to the
/// HIR `Stmt::Try` at `stmt_id` contain any comments? HIR strips
/// trivia, so the `commentAsCode` config still has to peek at the
/// parse tree.
fn except_clause_has_comments(
    stmt_id: StmtId,
    source_map: &hir::BodySourceMap,
    ctx: &DiagnosticsContext,
) -> bool {
    let Some(stmt_range) = source_map.stmt_range(stmt_id) else { return false };
    let parse = ctx.parse();
    let root = parse.syntax_node();
    let Some(try_node) = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::TRY_STMT && n.text_range() == stmt_range)
    else {
        return false;
    };
    let Some(except_clause) = try_node.children().find(|n| n.kind() == SyntaxKind::EXCEPT_CLAUSE)
    else {
        return false;
    };
    except_clause
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::COMMENT)
}

/// Push a diagnostic at the `Исключение` keyword. The message wording
/// branches on the catch-body class — `Empty` and `Silent` are
/// distinct quality issues even though they share the same diagnostic
/// code.
fn emit_at_except_keyword(
    stmt_id: StmtId,
    source_map: &hir::BodySourceMap,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
    class: CatchBodyClass,
) {
    let message = match class {
        CatchBodyClass::Silent => "Блок исключения молча подавляет ошибку: добавьте \
             `ВызватьИсключение` или вызов журналирования"
            .to_string(),
        CatchBodyClass::RollbackOnly => "Блок исключения только откатывает транзакцию, \
             но не фиксирует ошибку: добавьте логирование или `ВызватьИсключение`"
            .to_string(),
        _ => "Отсутствует код в блоке исключения".to_string(),
    };

    let range = source_map
        .stmt_range(stmt_id)
        .and_then(|stmt_range| {
            let parse = ctx.parse();
            let root = parse.syntax_node();
            let try_node = root
                .descendants()
                .find(|n| n.kind() == SyntaxKind::TRY_STMT && n.text_range() == stmt_range)?;
            try_node
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|tok| tok.kind() == SyntaxKind::KW_EXCEPT)
                .map(|tok| tok.text_range())
        })
        .or_else(|| source_map.stmt_range(stmt_id));

    if let Some(range) = range {
        diagnostics.push(Diagnostic {
            code,
            message,
            severity: ctx.severity(code),
            range,
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

    /// Track 2 Phase D §2.2 — `RaisesOnly` catch-body classification:
    /// re-raising the exception via `ВызватьИсключение;` is proper
    /// recovery, no diagnostic.
    #[test]
    fn raises_only_does_not_emit() {
        let code = r#"Процедура Тест()
    Попытка
        Действие();
    Исключение
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Re-raise must not be flagged as silent swallow");
    }

    /// Track 2 Phase D §2.2 — `LogsOnly` catch-body classification:
    /// `Сообщить` is in `Category::Logging` per the §1.1 registry, so
    /// the catch handler is recognised as recording the error.
    #[test]
    fn logs_only_does_not_emit() {
        let code = r#"Процедура Тест()
    Попытка
        Действие();
    Исключение
        Сообщить("error");
    КонецПопытки;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Logging-API call must not be flagged");
    }

    /// Track 2 Phase D §2.2 — `Silent` catch-body classification: the
    /// except has statements but none of them propagate or log the
    /// exception (an unknown user-defined call cannot be statically
    /// proven to re-raise/log without inter-procedural analysis, so
    /// the classifier reports Silent and the handler emits).
    #[test]
    fn silent_swallow_emits() {
        let code = r#"Процедура Тест()
    Попытка
        Действие();
    Исключение
        ОбработатьОшибку();
    КонецПопытки;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Silent swallow must be flagged");
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingCodeTryCatchEx);
        assert!(
            diagnostics[0].message.contains("молча"),
            "Silent message should mention the swallow, got: {}",
            diagnostics[0].message
        );
    }

    /// Track 2 Phase D §2.2 — `RollbackOnly` catch-body
    /// classification: rolling back the transaction reverts state but
    /// doesn't record or propagate the failure. The handler emits a
    /// rollback-specific message recommending to add logging or
    /// `ВызватьИсключение`.
    #[test]
    fn rollback_only_emits_with_rollback_message() {
        let code = r#"Процедура Тест()
    Попытка
        Действие();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Rollback-alone must emit");
        assert_eq!(diagnostics[0].code, DiagnosticCode::MissingCodeTryCatchEx);
        assert!(
            diagnostics[0].message.contains("откатывает"),
            "Rollback message should mention rollback, got: {}",
            diagnostics[0].message
        );
    }

    /// Rollback + logging is proper recovery — the failure is
    /// observable, no diagnostic.
    #[test]
    fn rollback_plus_log_does_not_emit() {
        let code = r#"Процедура Тест()
    Попытка
        Действие();
    Исключение
        ОтменитьТранзакцию();
        Сообщить("error");
    КонецПопытки;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Rollback + log must not emit");
    }

    /// Track 2 Phase D §2.2 — `Mixed` catch-body classification:
    /// raise + log together is a legitimate "log then re-raise"
    /// pattern, no diagnostic.
    #[test]
    fn mixed_does_not_emit() {
        let code = r#"Процедура Тест()
    Попытка
        Действие();
    Исключение
        Сообщить("error");
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Log-then-reraise must not be flagged");
    }

    #[test]
    fn test_missing_code_try_catch_ex() {
        // Inline version of MissingCodeTryCatchExDiagnostic.bsl.
        // Uses 4-space indentation to match original column positions.
        let code = r#"Процедура Проц1()
    Попытка
        Действие();
    Исключение
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры

Процедура Проц11()
    Попытка
        Действие();
    Исключение

        // просто коментарий

        ВызватьИсключение;
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
    // §2.2: nested try without re-raise IS Silent — outer except
    // has Stmt::Try as "other", and the classifier conservatively
    // treats any stmt-kind that isn't Raise / log-call as Silent
    // (cannot prove the nested try ultimately propagates without
    // inter-procedural analysis).
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

        // Track 2 Phase D §2.2: 4 expected emits — the §2 classifier
        // adds Silent detection for catch bodies whose contents are
        // not Raise / log-call. The outer Проц3 except contains a
        // nested Try (which the classifier treats as "other"), so it
        // is now flagged as Silent in addition to the prior 3 Empty
        // cases.
        assert_eq!(diagnostics.len(), 4, "Should detect 3 Empty + 1 Silent");

        // Line 23 (Проц2 Empty)
        assert_diagnostic_range(code, &diagnostics[0], 23, 4, 14);
        // Line 32 (Функ1 Empty — HIR drops comments)
        assert_diagnostic_range(code, &diagnostics[1], 32, 4, 14);
        // Line 50 (Проц3 outer Silent — new in §2.2)
        assert_diagnostic_range(code, &diagnostics[2], 50, 4, 14);
        // Line 54 (Проц3 inner Empty)
        assert_diagnostic_range(code, &diagnostics[3], 54, 8, 18);
    }

    #[test]
    fn test_comment_as_code() {
        // Same code as test_missing_code_try_catch_ex.
        let code = r#"Процедура Проц1()
    Попытка
        Действие();
    Исключение
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры

Процедура Проц11()
    Попытка
        Действие();
    Исключение

        // просто коментарий

        ВызватьИсключение;
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
    // §2.2: nested try without re-raise IS Silent — outer except
    // has Stmt::Try as "other", and the classifier conservatively
    // treats any stmt-kind that isn't Raise / log-call as Silent
    // (cannot prove the nested try ultimately propagates without
    // inter-procedural analysis).
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

        // Track 2 Phase D §2.2 + commentAsCode=true: Функ1's
        // Empty-with-comments is suppressed (line 32 gone). Проц2
        // Empty stays (no comments in its except clause). Проц3 outer
        // Silent (nested Try) stays — `commentAsCode` only affects
        // the Empty branch. Проц3 inner Empty stays.
        assert_eq!(diagnostics.len(), 3, "1 Empty suppressed + 2 Empty + 1 Silent remain");

        // Line 23 (Проц2 Empty — no comments to suppress)
        assert_diagnostic_range(code, &diagnostics[0], 23, 4, 14);
        // Line 50 (Проц3 outer Silent — commentAsCode irrelevant)
        assert_diagnostic_range(code, &diagnostics[1], 50, 4, 14);
        // Line 54 (Проц3 inner Empty)
        assert_diagnostic_range(code, &diagnostics[2], 54, 8, 18);
    }

    #[test]
    fn test_valid_exception_handlers() {
        // Track 2 Phase D §2.2: "valid exception handler" now means
        // re-raise or log via the §1.1 registry. An unknown
        // user-defined call is conservatively classified as Silent
        // because we can't prove it propagates the exception. The
        // fixture uses `ВызватьИсключение;` to exercise the
        // RaisesOnly → no-emit path.
        let code = r#"
Процедура Проц1()
    Попытка
        Действие();
    Исключение
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры
"#;

        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Valid exception handler should not trigger diagnostic");
    }
}
