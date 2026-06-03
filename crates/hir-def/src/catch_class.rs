use crate::body::Body;
use crate::hir::{Expr, ExprIdx, Stmt, StmtIdx};
use bsl_platform::security::{registry, Category};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatchBodyClass {
    Empty,
    RaisesOnly,
    LogsOnly,
    RollbackOnly,
    Mixed,
    Silent,
}

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
            _ => has_other = true,
        }
    }

    if let Some(pure) = match (has_raise, has_log, has_rollback, has_other) {
        (true, false, false, false) => Some(CatchBodyClass::RaisesOnly),
        (false, true, false, false) => Some(CatchBodyClass::LogsOnly),
        (false, false, true, false) => Some(CatchBodyClass::RollbackOnly),
        (false, false, false, true) => Some(CatchBodyClass::Silent),
        _ => None,
    } {
        return pure;
    }

    if has_raise || has_log {
        CatchBodyClass::Mixed
    } else if has_rollback {
        CatchBodyClass::RollbackOnly
    } else {
        CatchBodyClass::Silent
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RecoveryKind {
    Log,
    Rollback,
    None,
}

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
