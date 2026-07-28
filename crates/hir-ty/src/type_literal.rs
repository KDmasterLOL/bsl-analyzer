//! Имя типа, записанное значением, а не синтаксисом.
//!
//! `Новый Массив` называет тип идентификатором, `Новый("Массив")` и
//! `Новый(Тип("Массив"))` — строковым литералом. Для квалифицированных имён
//! (`РегистрСведенийКлючЗаписи.X`) строковая форма единственно возможная:
//! синтаксис `Новый` принимает ровно один сегмент.
//!
//! Распознавание живёт в `hir-ty`, а не в понижении: решение «вызов с этим
//! именем — платформенная `Тип`» требует знания платформы, а не разбора текста.

use hir_def::body::Body;
use hir_def::hir::{Expr, Literal};
use hir_def::Name;
use la_arena::Idx;

type ExprIdx = Idx<Expr>;

/// Строковый литерал, записанный прямо в позиции имени типа.
pub(crate) fn bare_string_literal(body: &Body, expr: ExprIdx) -> Option<&str> {
    match body.expr_idx(expr) {
        Expr::Literal(Literal::String(text)) => Some(text),
        _ => None,
    }
}

/// Строковый литерал внутри `Тип("X")` / `Type("X")` вместе с именем вызываемого
/// как оно записано: платформенную `Тип` перекрывает более близкое объявление, и
/// решать это может только тот, кто видит область видимости.
pub(crate) fn type_ctor_literal(body: &Body, expr: ExprIdx) -> Option<(&Name, &str)> {
    let Expr::Call { callee, args } = body.expr_idx(expr) else {
        return None;
    };
    let Expr::Path(callee_name) = body.expr_idx(*callee) else {
        return None;
    };
    if !crate::method_lookup::is_platform_name(callee_name, "Тип", "Type") {
        return None;
    }
    if args.len() != 1 {
        return None;
    }
    match body.expr_idx(args[0]) {
        Expr::Literal(Literal::String(text)) => Some((callee_name, text)),
        _ => None,
    }
}
