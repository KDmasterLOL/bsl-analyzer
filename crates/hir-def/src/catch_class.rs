//! Catch-body classifier (Track 2 Phase D §2.1).
//!
//! Classifies the body of an `Исключение` / `Except` clause into one of
//! six categories so the §2.2 `MissingCodeTryCatchEx` handler can
//! decide whether to emit a diagnostic without re-implementing the
//! raise/log/rollback recognition itself.
//!
//! Pure HIR-side function — no Salsa, no I/O, no AST access. Comments
//! are not visible at HIR level (the parser drops trivia), so
//! "comment-only" catch bodies are indistinguishable from truly empty
//! ones here; the handler's existing `commentAsCode` config still
//! lives at the AST layer for that distinction.
//!
//! Logging API names come from `bsl_platform::security::registry()`
//! filtered to `Category::Logging`. The §1.1 security registry is the
//! single source of truth for this set; extending the list of
//! recognised logging APIs is a registry-only change, no classifier
//! edits needed.

use crate::body::Body;
use crate::hir::{Expr, ExprIdx, Stmt, StmtIdx};
use bsl_platform::security::{registry, Category};

/// One of six structural classes the §2 catch-body classifier produces
/// from an `Исключение` / `Except` clause body. Source order is the
/// HIR statement order produced by lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatchBodyClass {
    /// Zero HIR statements (HIR strips trivia, so a comment-only body
    /// also lands here — the AST-side `commentAsCode` config in the
    /// handler distinguishes the two).
    Empty,
    /// Exclusively `Raise` / `Raise <expr>` statements (re-raises).
    RaisesOnly,
    /// Exclusively logging-API calls (registry `Category::Logging`).
    LogsOnly,
    /// Catch body whose only **recovery** action is a transaction
    /// rollback (`Category::Transaction`, e.g.
    /// `ОтменитьТранзакцию()`). May also contain unrelated "other"
    /// statements (assigns, control flow, unknown calls) — what
    /// matters for this class is the absence of `Raise` and logging.
    /// Rollback reverts state but doesn't record or propagate the
    /// failure, so the §2.2 handler still emits — with a
    /// rollback-specific message recommending to add logging or
    /// `Raise` so the error is observable.
    RollbackOnly,
    /// A combination of recovery actions (`Raise` + logging + rollback
    /// in any pairing). The exception is propagated or recorded
    /// somewhere, so the handler skips emission.
    Mixed,
    /// Has statements but none are `Raise`, logging, or rollback —
    /// silently swallows the exception. The §2.2 handler emits
    /// `MissingCodeTryCatchEx` for this class.
    Silent,
}

/// Classify the body of an `Исключение` clause.
///
/// The slice is the `except` field of `Stmt::Try { except, .. }`.
/// Returns one of the six [`CatchBodyClass`] variants; the §2.2
/// handler interprets the result.
pub fn classify_catch_body(body: &Body, except: &[StmtIdx]) -> CatchBodyClass {
    if except.is_empty() {
        return CatchBodyClass::Empty;
    }

    let mut has_raise = false;
    let mut has_log = false;
    let mut has_rollback = false;
    let mut has_other = false;

    for &stmt_idx in except.iter() {
        match body.stmt_idx(stmt_idx) {
            Stmt::Raise { .. } => has_raise = true,
            Stmt::Expr(expr_id) => match recovery_kind(body, *expr_id) {
                RecoveryKind::Log => has_log = true,
                RecoveryKind::Rollback => has_rollback = true,
                RecoveryKind::None => has_other = true,
            },
            // Anything else (assigns, var decls, control flow, etc.) is
            // "other" — those don't propagate the exception, so a body
            // made of only "other" statements is `Silent`.
            _ => has_other = true,
        }
    }

    // Pure single-class outcomes first (no `other` and exactly one
    // recovery kind seen) — these give the most informative message.
    if let Some(pure) = match (has_raise, has_log, has_rollback, has_other) {
        (true, false, false, false) => Some(CatchBodyClass::RaisesOnly),
        (false, true, false, false) => Some(CatchBodyClass::LogsOnly),
        (false, false, true, false) => Some(CatchBodyClass::RollbackOnly),
        (false, false, false, true) => Some(CatchBodyClass::Silent),
        _ => None,
    } {
        return pure;
    }

    // Mixed kinds or recovery kinds + `other`. If at least one kind
    // propagates / records the failure (raise or log), it's Mixed —
    // the exception is observable. Rollback-without-log-or-raise
    // (even mixed with `other`) still doesn't propagate, so it stays
    // `RollbackOnly`-like — the handler emits with the rollback
    // message.
    if has_raise || has_log {
        CatchBodyClass::Mixed
    } else if has_rollback {
        CatchBodyClass::RollbackOnly
    } else {
        CatchBodyClass::Silent
    }
}

/// Internal helper: which recovery class does a top-level
/// statement-expression call belong to?
#[derive(Clone, Copy, PartialEq, Eq)]
enum RecoveryKind {
    Log,
    Rollback,
    None,
}

/// Classify a top-level statement-expression call as one of the
/// known recovery kinds in the §1.1 registry. The §2 catch-body
/// classifier needs to track logging and rollback separately because
/// rollback alone doesn't propagate or record the failure (rollback
/// reverts state but loses the error), while logging records it for
/// post-mortem inspection.
///
/// Recognises three call shapes that BSL lowering produces:
/// - **Unqualified global call**: `Сообщить(...)` →
///   `Expr::Call { callee: Expr::Path("Сообщить") }`.
/// - **Method call** (single-receiver): `obj.foo(...)` →
///   `Expr::MethodCall { method: "foo" }`.
/// - **Field-callee call** (qualified module.method): the typical BSL
///   pattern `ОбщегоНазначения.СообщитьПользователю(...)` lowers as
///   `Expr::Call { callee: Expr::Field { field: "СообщитьПользователю" } }`,
///   not as a method call. We extract the field name and look it up.
///
/// In all three shapes only the method name is checked against the
/// registry; receiver-side filtering would need cross-module
/// resolution which is out of scope for the §2 catch-body classifier.
fn recovery_kind(body: &Body, expr_id: ExprIdx) -> RecoveryKind {
    let reg = registry();
    let classify = |entry: &bsl_platform::security::SecurityEntry| match entry.category {
        Category::Logging => RecoveryKind::Log,
        Category::Transaction => RecoveryKind::Rollback,
        _ => RecoveryKind::None,
    };
    let lookup = |name: &str| reg.lookup_global(name).map(classify).unwrap_or(RecoveryKind::None);
    match body.expr_idx(expr_id) {
        Expr::Call { callee, .. } => match body.expr_idx(*callee) {
            Expr::Path(name) => lookup(name.as_str()),
            // `Module.method(args)` lowering: callee is a field
            // expression carrying the method name.
            Expr::Field { field, .. } => lookup(field.as_str()),
            _ => RecoveryKind::None,
        },
        Expr::MethodCall { method, .. } => lookup(method.as_str()),
        _ => RecoveryKind::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::lower_method;
    use syntax::SyntaxKind;

    fn parse_lower(code: &str) -> Body {
        let parse = parser::parse(code);
        let root = parse.syntax_node();
        let method_node = root
            .descendants()
            .find(|n| matches!(n.kind(), SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF))
            .expect("Should have a method");
        let is_function = method_node.kind() == SyntaxKind::FUNCTION_DEF;
        lower_method(&method_node, is_function).body
    }

    fn first_try_except(body: &Body) -> &[StmtIdx] {
        for &stmt_idx in body.body_stmts.iter() {
            if let Stmt::Try { except, .. } = body.stmt_idx(stmt_idx) {
                return except;
            }
        }
        &[]
    }

    #[test]
    fn empty_except_is_empty() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::Empty);
    }

    #[test]
    fn raise_only_is_raises_only() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::RaisesOnly);
    }

    #[test]
    fn message_call_is_logs_only() {
        // `Сообщить` is in `Category::Logging` per the §1.1 registry.
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        Сообщить("error");
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::LogsOnly);
    }

    #[test]
    fn raise_plus_log_is_mixed() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        Сообщить("error");
        ВызватьИсключение;
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::Mixed);
    }

    #[test]
    fn silent_swallow_is_silent() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        Х = 1;
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::Silent);
    }

    /// Codex round-A regression guard: rollback alone reverts state
    /// but doesn't propagate or record the failure — it gets its own
    /// `RollbackOnly` class so the handler emits with rollback-specific
    /// guidance (recommend adding logging or `Raise`).
    #[test]
    fn rollback_only_is_rollback_only() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        ОтменитьТранзакцию();
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::RollbackOnly);
    }

    /// Rollback paired with logging or raise IS proper recovery
    /// (failure is observable) → Mixed, no diagnostic emit.
    #[test]
    fn rollback_plus_log_is_mixed() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        ОтменитьТранзакцию();
        Сообщить("error");
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::Mixed);
    }

    /// Codex round-A regression guard: qualified module calls
    /// (`ОбщегоНазначения.СообщитьПользователю(...)`) lower as
    /// `Expr::Call` whose `callee` is `Expr::Field`, NOT as
    /// `Expr::MethodCall`. The classifier must look up the field name
    /// against the registry — without this, very common BSL logging
    /// patterns false-positive as Silent.
    #[test]
    fn qualified_logging_call_is_logs_only() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        ОбщегоНазначения.СообщитьПользователю("error");
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::LogsOnly);
    }

    #[test]
    fn unknown_call_is_silent() {
        // A non-logging call (e.g. business-logic call inside the
        // catch) does NOT propagate the exception — silent swallow.
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        ОбработатьОшибку();
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::Silent);
    }
}
