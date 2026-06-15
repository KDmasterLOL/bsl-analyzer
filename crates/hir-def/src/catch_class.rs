use crate::body::Body;
use crate::hir::{Expr, ExprIdx, Stmt, StmtIdx};
use bsl_platform::security::{registry, Category};
use stdx::case::CaseExt;

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

    let mut flags = RecoveryFlags::default();
    scan_stmts(body, except, true, &mut flags);
    let RecoveryFlags { raise: has_raise, log: has_log, rollback: has_rollback, other: has_other } =
        flags;

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

#[derive(Default)]
struct RecoveryFlags {
    raise: bool,
    log: bool,
    rollback: bool,
    other: bool,
}

/// Collects recovery actions on every control-flow path of the handler:
/// a `Raise` (or log/rollback call) inside an `Если`/loop branch is a real
/// conditional rethrow, so the branching statements themselves are
/// transparent. `raise_escapes` is false inside a nested `Попытка` body —
/// a `Raise` there is caught by the nested handler and never leaves the
/// outer one; only the nested handler's own statements rethrow outward.
fn scan_stmts(body: &Body, stmts: &[StmtIdx], raise_escapes: bool, flags: &mut RecoveryFlags) {
    for &stmt_idx in stmts.iter() {
        match body.stmt_idx(stmt_idx) {
            Stmt::Raise { .. } => {
                if raise_escapes {
                    flags.raise = true;
                } else {
                    flags.other = true;
                }
            }
            Stmt::Expr(expr_id) => match recovery_kind(body, *expr_id) {
                RecoveryKind::Log => flags.log = true,
                RecoveryKind::Rollback => flags.rollback = true,
                RecoveryKind::None => flags.other = true,
            },
            Stmt::If(if_stmt) => {
                scan_stmts(body, &if_stmt.then_branch, raise_escapes, flags);
                for (_, branch) in if_stmt.elsif_branches.iter() {
                    scan_stmts(body, branch, raise_escapes, flags);
                }
                if let Some(branch) = &if_stmt.else_branch {
                    scan_stmts(body, branch, raise_escapes, flags);
                }
            }
            Stmt::PreprocIf(preproc) => {
                scan_stmts(body, &preproc.then_branch, raise_escapes, flags);
                for (_, _, branch) in preproc.elsif_branches.iter() {
                    scan_stmts(body, branch, raise_escapes, flags);
                }
                if let Some(branch) = &preproc.else_branch {
                    scan_stmts(body, branch, raise_escapes, flags);
                }
            }
            Stmt::While { body: loop_body, .. }
            | Stmt::For { body: loop_body, .. }
            | Stmt::ForEach { body: loop_body, .. } => {
                scan_stmts(body, loop_body, raise_escapes, flags);
            }
            Stmt::Try { body: try_body, except } => {
                scan_stmts(body, try_body, false, flags);
                scan_stmts(body, except, raise_escapes, flags);
            }
            _ => flags.other = true,
        }
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
    let lookup = |name: &str| {
        if let Some(kind) = reg.lookup_global(name).map(classify) {
            if kind != RecoveryKind::None {
                return kind;
            }
        }
        if name_is_error_report_sink(name) {
            RecoveryKind::Log
        } else {
            RecoveryKind::None
        }
    };
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

/// Recognizes user/BSP error-reporting helpers by name: a recording or
/// notifying verb combined with an error/log/problem/warning noun
/// (`ЗаписатьОшибкуВЖурналРегистрации`, `ДобавитьСообщениеДляЖурналаРегистрации`,
/// `СообщитьОПроблеме`, `ПоказатьПредупреждение`, …).
///
/// The platform security registry only knows global logging primitives, but the
/// bulk of BSP/application handlers report through such named helpers. Pure
/// formatters and getters (`ПодробноеПредставлениеОшибки`, `ОписаниеОшибки`,
/// `ИнформацияОбОшибке`) carry the noun but no recording verb, so they are not
/// treated as logging and a handler that only formats the error stays `Silent`.
fn name_is_error_report_sink(name: &str) -> bool {
    const VERBS: &[&str] = &[
        "запис",     // Записать… / Запись…
        "добав",     // ДобавитьОшибку / ДобавитьСообщение…
        "зафиксир",  // ЗафиксироватьОшибку
        "зарегистр", // ЗарегистрироватьОшибку
        "регистрац", // РегистрацияОшибки
        "вывест",    // ВывестиОшибку
        "показа",    // ПоказатьОшибку / ПоказатьПредупреждение
        "сообщи",    // СообщитьОбОшибке / СообщитьОПроблеме (но не `Сообщение…`)
        "оповест",   // Оповестить…
        "сохран",    // СохранитьОшибкуВЖурнал
    ];
    const NOUNS: &[&str] = &["ошибк", "журнал", "проблем", "предупрежд"];

    let lower = name.fold_lower();
    VERBS.iter().any(|verb| lower.contains(verb)) && NOUNS.iter().any(|noun| lower.contains(noun))
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
    fn conditional_raise_in_if_is_raises_only() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        Если ИнформацияОбОшибке().Описание <> ТекстИсключенияДублирование Тогда
            ВызватьИсключение;
        КонецЕсли;
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::RaisesOnly);
    }

    #[test]
    fn raise_in_else_branch_counts() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        Если ИзвестнаяОшибка Тогда
            Лог = 1;
        Иначе
            ВызватьИсключение;
        КонецЕсли;
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::Mixed);
    }

    #[test]
    fn conditional_log_is_logs_only() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        Если НужноЛогировать Тогда
            Сообщить("error");
        КонецЕсли;
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::LogsOnly);
    }

    #[test]
    fn raise_in_loop_counts() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        Для Каждого Ошибка Из Ошибки Цикл
            ВызватьИсключение;
        КонецЦикла;
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::RaisesOnly);
    }

    #[test]
    fn conditional_swallow_is_still_silent() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        Если Условие Тогда
            Х = 1;
        КонецЕсли;
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::Silent);
    }

    #[test]
    fn raise_inside_nested_try_body_does_not_count() {
        // The nested handler catches it before it can leave the outer one.
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        Попытка
            ВызватьИсключение;
        Исключение
        КонецПопытки;
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::Silent);
    }

    #[test]
    fn raise_inside_nested_try_handler_counts() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        Попытка
            ЗаписатьОшибку();
        Исключение
            ВызватьИсключение;
        КонецПопытки;
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::Mixed);
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

    #[test]
    fn named_log_helper_is_logs_only() {
        for callee in [
            "ЗаписатьОшибкуВЖурналРегистрации",
            "ДобавитьСообщениеДляЖурналаРегистрации",
            "СообщитьОПроблеме",
            "ПоказатьПредупреждение",
            "ОбработкаОшибок.ЗаписатьОшибку",
        ] {
            let code = format!(
                r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        {callee}(ИнформацияОбОшибке());
    КонецПопытки;
КонецПроцедуры
"#
            );
            let body = parse_lower(&code);
            let except = first_try_except(&body);
            assert_eq!(
                classify_catch_body(&body, except),
                CatchBodyClass::LogsOnly,
                "callee `{callee}` should be recognized as logging"
            );
        }
    }

    #[test]
    fn error_formatter_only_stays_silent() {
        // A getter/formatter carries the error noun but no recording verb: the
        // handler still does nothing with the formatted error.
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        ПодробноеПредставлениеОшибки(ИнформацияОбОшибке());
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::Silent);
    }

    #[test]
    fn error_message_constructor_statement_stays_silent() {
        // `Сообщение…` is a noun (constructor/getter), not the `Сообщить` verb:
        // a bare constructor statement still does nothing with the error.
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        СообщениеОбОшибке(ИнформацияОбОшибке());
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::Silent);
    }

    #[test]
    fn error_accumulator_helper_is_logs_only() {
        // The `СписокОшибок`/`ДобавитьОшибкуПользователю` pattern is the standard
        // BSP way to surface collected errors, so it counts as reporting.
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        ДобавитьОшибкуПользователю(СписокОшибок, "Поле", ОписаниеОшибки(), Неопределено);
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::LogsOnly);
    }

    #[test]
    fn rollback_plus_named_log_is_mixed() {
        let body = parse_lower(
            r#"
Процедура Тест()
    Попытка
        Действие();
    Исключение
        ОтменитьТранзакцию();
        ЗаписатьОшибкуВЖурналРегистрации(ИнформацияОбОшибке());
    КонецПопытки;
КонецПроцедуры
"#,
        );
        let except = first_try_except(&body);
        assert_eq!(classify_catch_body(&body, except), CatchBodyClass::Mixed);
    }
}
