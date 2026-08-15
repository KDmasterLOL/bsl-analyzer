use crate::Sig;

use crate::event::NodeKind;
use crate::parser::Parser;

use super::expressions;

pub fn stmt_list(p: &mut Parser, terminator: Sig) {
    let m = p.start();

    // The list ends at its own terminator and at every closer further out.
    // Rules inside refuse to consume an enclosing closer, so a list waiting
    // only for its own would wait for a token nothing will reach.
    while !p.at_end() && !p.at(terminator) && !p.at_enclosing_boundary() {
        p.check_iteration_limit();

        if p.at_end() || p.at(terminator) || p.at_enclosing_boundary() {
            break;
        }

        statement(p);
    }

    m.complete(p, NodeKind::StmtList);
}

pub fn statement(p: &mut Parser) {
    match p.current() {
        Some(T![Semicolon]) => {
            let m = p.start();
            p.bump();
            m.complete(p, NodeKind::EmptyStmt);
        }
        Some(T![KwReturn]) => return_stmt(p),
        Some(T![KwIf]) => if_stmt(p),
        Some(T![KwWhile]) => while_stmt(p),
        Some(T![KwFor]) => for_stmt(p),
        Some(T![KwTry]) => try_stmt(p),
        Some(T![KwRaise]) => raise_stmt(p),
        Some(T![KwBreak]) => {
            let m = p.start();
            p.bump();
            p.eat(T![Semicolon]);
            m.complete(p, NodeKind::BreakStmt);
        }
        Some(T![KwContinue]) => {
            let m = p.start();
            p.bump();
            p.eat(T![Semicolon]);
            m.complete(p, NodeKind::ContinueStmt);
        }
        Some(T![KwGoto]) => goto_stmt(p),
        Some(T![Tilde]) => label_stmt(p),
        Some(T![KwExecute]) => execute_stmt(p),
        Some(T![KwAddHandler]) => add_handler_stmt(p),
        Some(T![KwRemoveHandler]) => remove_handler_stmt(p),
        Some(T![KwVar]) => super::items::var_declaration(p),
        Some(T![PreRegion]) => super::preprocessor_region(p),
        Some(T![PreEndRegion]) => super::preprocessor_end_region(p),
        Some(T![PreIf]) => super::preprocessor_if(p),
        Some(T![PreDelete]) => super::preprocessor_delete(p),
        Some(T![PreInsert]) => super::preprocessor_insert(p),
        Some(T![Ident]) => {
            assignment_or_call(p);
        }
        _ => {
            if p.current().is_some() {
                let m = p.start();
                expressions::expression(p);
                m.complete(p, NodeKind::CallStmt);
                p.eat(T![Semicolon]);
            }
        }
    }
}

fn return_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    // `Возврат` may carry a value or not, and what says it does not is the
    // end of the statement or the end of something enclosing it. The blocks
    // that are open have already stated their closers, so asking them beats
    // keeping a list here — a list has no way to know that `КонецЦикла` with
    // no loop open closes nothing.
    if !at_end_of_statement(p) {
        expressions::expression(p);
    }

    p.eat(T![Semicolon]);

    m.complete(p, NodeKind::ReturnStmt);
}

// A block states the words that close it while it parses what it encloses, so
// that a rule tripping over one of them reports the trip and leaves the word
// for the block. Without that, the count of false "closer is missing" reports
// is the nesting depth: one unclosed call inside `Попытка` inside `Процедура`
// costs `Исключение`, `КонецПопытки` and `КонецПроцедуры` all three.

fn at_end_do(p: &Parser) -> bool {
    p.at(T![KwEndDo])
}

// A header waits for the word that ends it as surely as the block waits for
// the word that closes it, and a condition never written leaves the parser
// standing on that word. `Если Тогда` used to consume the `Тогда` in front of
// it and then report it missing.

fn at_then(p: &Parser) -> bool {
    p.at(T![KwThen])
}

/// The comma between the two operands of a handler statement. It stands at
/// the statement's own level, where no group is open to speak for it.
fn at_handler_comma(p: &Parser) -> bool {
    p.at(T![Comma])
}

fn at_do(p: &Parser) -> bool {
    p.at(T![KwDo])
}

fn at_to_or_do(p: &Parser) -> bool {
    matches!(p.current(), Some(T![KwTo] | T![KwDo]))
}

fn at_if_closer(p: &Parser) -> bool {
    matches!(p.current(), Some(T![KwElsIf] | T![KwElse] | T![KwEndIf]))
}

fn at_try_closer(p: &Parser) -> bool {
    matches!(p.current(), Some(T![KwExcept] | T![KwEndTry]))
}

fn if_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.within_boundary(at_if_closer, |p| {
        p.within_boundary(at_then, expressions::expression);

        p.expect(T![KwThen]);

        stmt_list_inner(p, &[T![KwElsIf], T![KwElse], T![KwEndIf]]);

        while p.at(T![KwElsIf]) {
            p.check_iteration_limit();
            let em = p.start();
            p.bump();
            p.within_boundary(at_then, expressions::expression);
            p.expect(T![KwThen]);
            stmt_list_inner(p, &[T![KwElsIf], T![KwElse], T![KwEndIf]]);
            em.complete(p, NodeKind::ElseIfClause);
        }

        if p.at(T![KwElse]) {
            let em = p.start();
            p.bump();
            stmt_list_inner(p, &[T![KwEndIf]]);
            em.complete(p, NodeKind::ElseClause);
        }
    });

    p.expect(T![KwEndIf]);

    p.eat(T![Semicolon]);

    m.complete(p, NodeKind::IfStmt);
}

fn while_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.within_boundary(at_end_do, |p| {
        p.within_boundary(at_do, expressions::expression);

        p.expect(T![KwDo]);

        stmt_list(p, T![KwEndDo]);
    });

    p.expect(T![KwEndDo]);

    p.eat(T![Semicolon]);

    m.complete(p, NodeKind::WhileStmt);
}

fn for_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    if p.at(T![KwEach]) {
        p.within_boundary(at_end_do, |p| {
            p.bump();

            if p.at(T![Ident]) {
                p.bump();
            }

            p.within_boundary(at_do, |p| {
                p.expect(T![KwIn]);

                expressions::expression(p);
            });

            p.expect(T![KwDo]);

            stmt_list(p, T![KwEndDo]);
        });

        p.expect(T![KwEndDo]);

        p.eat(T![Semicolon]);

        m.complete(p, NodeKind::ForEachStmt);
    } else {
        p.within_boundary(at_end_do, |p| {
            if p.at(T![Ident]) {
                p.bump();
            }

            p.expect(T![Eq]);

            // The scope has to outlive the `По` it protects: an expect run
            // outside it would take the `Цикл` the header is still waiting
            // for, and then report that same `Цикл` missing.
            // `По` stops being awaited the moment it is consumed, so the
            // second bound is parsed under `Цикл` alone. Keeping `По` a
            // boundary past its own position leaves a repeated one standing,
            // and the expect for `Цикл` then takes the real `Цикл` instead.
            p.within_boundary(at_to_or_do, |p| {
                expressions::expression(p);

                p.expect(T![KwTo]);
            });

            p.within_boundary(at_do, |p| {
                expressions::expression(p);
            });

            p.expect(T![KwDo]);

            stmt_list(p, T![KwEndDo]);
        });

        p.expect(T![KwEndDo]);

        p.eat(T![Semicolon]);

        m.complete(p, NodeKind::ForStmt);
    }
}

fn try_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.within_boundary(at_try_closer, |p| {
        stmt_list(p, T![KwExcept]);

        p.expect(T![KwExcept]);

        let em = p.start();
        stmt_list(p, T![KwEndTry]);
        em.complete(p, NodeKind::ExceptClause);
    });

    p.expect(T![KwEndTry]);

    p.eat(T![Semicolon]);

    m.complete(p, NodeKind::TryStmt);
}

fn raise_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    if p.at(T![LParen]) {
        parse_raise_call_args(p);
    } else if !at_end_of_statement(p) {
        expressions::expression(p);
    }

    p.eat(T![Semicolon]);

    m.complete(p, NodeKind::RaiseStmt);
}

/// Whether nothing more of this statement can follow: its own separator, the
/// end of the input, or a word an enclosing construct is waiting for.
fn at_end_of_statement(p: &Parser) -> bool {
    p.at(T![Semicolon]) || p.at_end() || p.at_enclosing_boundary()
}

fn parse_raise_call_args(p: &mut Parser) {
    assert!(p.at(T![LParen]));
    p.bump();

    p.within_boundary(super::at_paren_list_punctuation, |p| {
        while !p.at(T![RParen]) && !p.at_end() {
            p.check_iteration_limit();

            if p.at(T![Comma]) {
            } else if !p.at(T![RParen]) {
                expressions::expression(p);
            }

            if p.at(T![Comma]) {
                p.bump();
            } else if !p.at(T![RParen]) {
                break;
            }
        }
    });

    p.expect(T![RParen]);
}

fn goto_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    if p.at(T![Tilde]) {
        p.bump();
    }

    if p.at(T![Ident]) {
        p.bump();
    }

    p.eat(T![Semicolon]);

    m.complete(p, NodeKind::GotoStmt);
}

fn label_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();
    p.expect(T![Ident]);
    p.expect(T![Colon]);
    m.complete(p, NodeKind::LabelStmt);
}

fn execute_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    expressions::expression(p);

    p.eat(T![Semicolon]);

    m.complete(p, NodeKind::ExecuteStmt);
}

fn add_handler_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.within_boundary(at_handler_comma, |p| {
        expressions::expression(p);

        p.expect(T![Comma]);
    });

    expressions::expression(p);

    p.eat(T![Semicolon]);

    m.complete(p, NodeKind::AddHandlerStmt);
}

fn remove_handler_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.within_boundary(at_handler_comma, |p| {
        expressions::expression(p);

        p.expect(T![Comma]);
    });

    expressions::expression(p);

    p.eat(T![Semicolon]);

    m.complete(p, NodeKind::RemoveHandlerStmt);
}

fn assignment_or_call(p: &mut Parser) {
    let m = p.start();

    let is_valid_stmt = expressions::postfix_expression_for_assignment(p);

    if p.eat(T![Eq]) {
        expressions::expression(p);
        m.complete(p, NodeKind::AssignStmt);
        p.eat(T![Semicolon]);
    } else if is_valid_stmt {
        m.complete(p, NodeKind::CallStmt);
        p.eat(T![Semicolon]);
    } else {
        p.error_custom_at_marker(m, "ожидался вызов или присваивание");
        p.eat(T![Semicolon]);
    }
}

fn stmt_list_inner(p: &mut Parser, terminators: &[Sig]) {
    let m = p.start();

    while !p.at_end() && !terminators.iter().any(|t| p.at(*t)) && !p.at_enclosing_boundary() {
        p.check_iteration_limit();

        if p.at_end() || terminators.iter().any(|t| p.at(*t)) || p.at_enclosing_boundary() {
            break;
        }

        statement(p);
    }

    m.complete(p, NodeKind::StmtList);
}
