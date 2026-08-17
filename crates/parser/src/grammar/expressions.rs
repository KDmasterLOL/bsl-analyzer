use crate::Sig;

use crate::event::NodeKind;
use crate::parser::token_set::TokenSet;
use crate::parser::{CompletedMarker, Parser};

// Keywords accepted as a member name when the name is on a *new* line after the
// dot. There the construct is ambiguous with a dangling dot before a fresh
// statement, so we stay conservative and never swallow block-structuring
// keywords (КонецФункции, Функция, …) — that would destroy error recovery.
const CROSS_LINE_PROPERTY_NAME_TOKENS: TokenSet = TokenSet::new(&[
    T![Ident],
    T![KwProcedure],
    T![KwFunction],
    T![KwExecute],
    T![KwGoto],
    T![KwBreak],
    T![KwContinue],
    T![KwTo],
    T![KwTrue],
    T![KwFalse],
    T![KwUndefined],
    T![KwNull],
    T![KwNew],
]);

pub fn expression(p: &mut Parser) {
    or_expr(p);
}

pub fn postfix_expression_for_assignment(p: &mut Parser) -> bool {
    postfix_expr_with_call_info(p)
}

fn or_expr(p: &mut Parser) {
    let mut lhs = p.start();
    and_expr(p);

    while p.at(T![KwOr]) {
        p.check_iteration_limit();
        let m = lhs.complete(p, NodeKind::Expr).precede(p);
        p.bump();
        let rhs = p.start();
        and_expr(p);
        rhs.complete(p, NodeKind::Expr);
        lhs = m.complete(p, NodeKind::BinaryExpr).precede(p);
    }

    lhs.complete(p, NodeKind::Expr);
}

fn and_expr(p: &mut Parser) {
    let mut lhs = p.start();
    not_expr(p);

    while p.at(T![KwAnd]) {
        p.check_iteration_limit();
        let m = lhs.complete(p, NodeKind::Expr).precede(p);
        p.bump();
        let rhs = p.start();
        not_expr(p);
        rhs.complete(p, NodeKind::Expr);
        lhs = m.complete(p, NodeKind::BinaryExpr).precede(p);
    }

    lhs.complete(p, NodeKind::Expr);
}

fn not_expr(p: &mut Parser) {
    if p.at(T![KwNot]) {
        let m = p.start();
        p.bump();
        not_expr(p);
        m.complete(p, NodeKind::UnaryExpr);
    } else {
        comparison_expr(p);
    }
}

fn comparison_expr(p: &mut Parser) {
    let mut lhs = p.start();
    additive_expr(p);
    let mut saw_comparison = false;

    while matches!(p.current(), Some(T![Eq] | T![Neq] | T![Lt] | T![Le] | T![Gt] | T![Ge])) {
        saw_comparison = true;
        let m = lhs.complete(p, NodeKind::Expr).precede(p);
        p.bump();
        let rhs = p.start();
        additive_expr(p);
        rhs.complete(p, NodeKind::Expr);
        lhs = m.complete(p, NodeKind::BinaryExpr).precede(p);
    }

    if saw_comparison {
        lhs.complete(p, NodeKind::Expr);
    } else {
        lhs.abandon(p);
    }
}

fn additive_expr(p: &mut Parser) {
    let mut lhs = p.start();
    multiplicative_expr(p);

    while matches!(p.current(), Some(T![Plus]) | Some(T![Minus])) {
        p.check_iteration_limit();
        let m = lhs.complete(p, NodeKind::Expr).precede(p);
        p.bump();
        let rhs = p.start();
        multiplicative_expr(p);
        rhs.complete(p, NodeKind::Expr);
        lhs = m.complete(p, NodeKind::BinaryExpr).precede(p);
    }

    lhs.complete(p, NodeKind::Expr);
}

fn multiplicative_expr(p: &mut Parser) {
    let mut lhs = p.start();
    unary_expr(p);

    while matches!(p.current(), Some(T![Star]) | Some(T![Slash]) | Some(T![Percent])) {
        p.check_iteration_limit();
        let m = lhs.complete(p, NodeKind::Expr).precede(p);
        p.bump();
        let rhs = p.start();
        unary_expr(p);
        rhs.complete(p, NodeKind::Expr);
        lhs = m.complete(p, NodeKind::BinaryExpr).precede(p);
    }

    lhs.complete(p, NodeKind::Expr);
}

fn unary_expr(p: &mut Parser) {
    match p.current() {
        Some(T![Plus]) | Some(T![Minus]) => {
            p.bump();
            unary_expr(p);
        }
        _ => postfix_expr(p),
    }
}

fn postfix_expr(p: &mut Parser) {
    postfix_expr_with_call_info(p);
}

fn postfix_expr_with_call_info(p: &mut Parser) -> bool {
    let Some(mut lhs) = primary_expr(p) else {
        return false;
    };

    let mut is_valid_statement = false;

    loop {
        p.check_iteration_limit();
        match p.current() {
            Some(T![Dot]) => {
                let m = lhs.precede(p);
                p.bump();
                let crossed_newline = p.a_line_break_precedes();
                let is_orphaned_declaration = crossed_newline && p.at_declaration_start();
                // On the same line `expr.keyword` is unambiguously a member access —
                // BSL keywords are not reserved as property/field/enum-value names
                // (ВСД.for, XDTO.return, Условие.Иначе, Перечисления.X.ИЛИ). Across a
                // newline the construct is ambiguous with a dangling dot before a new
                // statement, so we keep the conservative whitelist to preserve in-method
                // error recovery (do not swallow КонецФункции/Функция/… as a member).
                let at_property_name = if crossed_newline {
                    p.at_ts(CROSS_LINE_PROPERTY_NAME_TOKENS)
                } else {
                    p.at(T![Ident]) || p.current().is_some_and(Sig::is_keyword)
                };
                if at_property_name && !is_orphaned_declaration {
                    p.bump();
                    lhs = m.complete(p, NodeKind::FieldExpr);
                    is_valid_statement = false;
                } else {
                    p.error_custom_no_bump("ожидалось имя свойства после '.'");
                    m.complete(p, NodeKind::FieldExpr);
                    break;
                }
            }
            Some(T![LBracket]) => {
                let m = lhs.precede(p);
                p.bump();
                p.within_boundary(super::at_closing_bracket, |p| {
                    expression(p);
                });
                p.expect(T![RBracket]);
                lhs = m.complete(p, NodeKind::IndexExpr);
                is_valid_statement = true;
            }
            Some(T![LParen]) => {
                let m = lhs.precede(p);
                arg_list(p);
                lhs = m.complete(p, NodeKind::CallExpr);
                is_valid_statement = true;
            }
            _ => break,
        }
    }

    is_valid_statement
}

fn primary_expr(p: &mut Parser) -> Option<CompletedMarker> {
    match p.current() {
        Some(T![Decimal]) | Some(T![Float]) | Some(T![Date]) => {
            let m = p.start();
            p.bump();
            Some(m.complete(p, NodeKind::Literal))
        }
        Some(T![String]) | Some(T![StringStart]) => Some(string_literal(p)),
        Some(T![StringPart]) | Some(T![StringTail]) => {
            p.error_custom("неожиданный фрагмент строки");
            None
        }
        Some(T![KwTrue]) | Some(T![KwFalse]) => {
            let m = p.start();
            p.bump();
            Some(m.complete(p, NodeKind::Literal))
        }
        Some(T![KwUndefined]) | Some(T![KwNull]) => {
            let m = p.start();
            p.bump();
            Some(m.complete(p, NodeKind::Literal))
        }
        Some(T![KwAwait]) => Some(await_expr(p)),
        Some(T![Ident]) => {
            let m = p.start();
            p.bump();
            Some(m.complete(p, NodeKind::Ident))
        }
        Some(T![LParen]) => {
            let m = p.start();
            p.bump();
            p.within_boundary(super::at_closing_paren, |p| {
                expression(p);
            });
            p.expect(T![RParen]);
            Some(m.complete(p, NodeKind::ParenExpr))
        }
        Some(T![KwNew]) => Some(new_expr(p)),
        Some(T![Question]) => Some(ternary_expr(p)),
        _ => {
            p.error_unexpected();
            None
        }
    }
}

fn string_literal(p: &mut Parser) -> CompletedMarker {
    let m = p.start();

    loop {
        match p.current() {
            Some(T![String]) => {
                p.bump();
            }
            Some(T![StringStart]) => {
                p.bump();
                string_continuation_tail(p);
            }
            _ => break,
        }

        if !at_adjacent_string_literal(p) {
            break;
        }
    }

    m.complete(p, NodeKind::Literal)
}

fn at_adjacent_string_literal(p: &Parser) -> bool {
    matches!(p.current(), Some(T![String] | T![StringStart]))
}

fn string_continuation_tail(p: &mut Parser) {
    loop {
        p.check_iteration_limit();
        match p.current() {
            Some(T![StringTail]) | Some(T![String]) => {
                p.bump();
                break;
            }
            Some(T![StringPart]) => {
                p.bump();
            }
            None => {
                p.error_custom("незакрытая многострочная строка");
                break;
            }
            _ => {
                p.error_custom("неожиданный токен в многострочной строке");
                break;
            }
        }
    }
}

fn await_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.start();
    p.bump();
    expression(p);
    m.complete(p, NodeKind::AwaitExpr)
}

fn new_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.start();
    p.bump();

    if p.at(T![Ident]) {
        p.bump();
    }

    if p.at(T![LParen]) {
        arg_list(p);
    }
    m.complete(p, NodeKind::NewExpr)
}

/// Whether the token here carries on the expression this one sits inside —
/// an operator of any precedence level, or the dot and bracket of a postfix
/// chain. No rule declares these as a boundary, because a rule that reaches
/// one consumes it and loops; a rule giving up in front of one has to leave
/// it for that loop.
fn continues_the_surrounding_expression(p: &Parser) -> bool {
    matches!(
        p.current(),
        Some(
            T![KwOr]
                | T![KwAnd]
                | T![Eq]
                | T![Neq]
                | T![Lt]
                | T![Le]
                | T![Gt]
                | T![Ge]
                | T![Plus]
                | T![Minus]
                | T![Star]
                | T![Slash]
                | T![Percent]
                | T![Dot]
                | T![LBracket]
        )
    )
}

fn ternary_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.start();
    p.bump();

    // Every other bracketed rule is entered with its opener already in hand.
    // The ternary's opener is an `expect`, so it can be absent — and with no
    // group open, nothing standing here is the ternary's to take. Not the
    // commas: an enclosing list writes those after a `?` too, and a successful
    // `expect` does not ask whose separator it is. Not the closer either.
    // Reading on would spend another rule's punctuation on operands that were
    // never written.
    if !p.eat(T![LParen]) {
        // Giving up must not spend a token either — but only where something
        // else will use it. `expect` already leaves an enclosing boundary
        // alone; what nobody declares is the operator that carries on the
        // expression around the `?`. Anything past those two is stray, and
        // leaving that behind means no rule ever takes it.
        if continues_the_surrounding_expression(p) {
            p.expect_no_bump(T![LParen]);
        } else {
            p.expect(T![LParen]);
        }
        return m.complete(p, NodeKind::TernaryExpr);
    }

    p.within_boundary(super::at_paren_list_punctuation, |p| {
        expression(p);
        p.expect(T![Comma]);
        expression(p);
        p.expect(T![Comma]);
        expression(p);
    });
    p.expect(T![RParen]);
    m.complete(p, NodeKind::TernaryExpr)
}

fn arg_list(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.within_boundary(super::at_paren_list_punctuation, |p| {
        if !p.at(T![RParen]) {
            if !p.at(T![Comma]) && !p.at(T![RParen]) {
                expression(p);
            }

            while p.eat(T![Comma]) {
                p.check_iteration_limit();
                if !p.at(T![Comma]) && !p.at(T![RParen]) {
                    expression(p);
                }
            }
        }
    });

    p.expect(T![RParen]);

    m.complete(p, NodeKind::ArgList);
}
