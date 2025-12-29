//! Statement parsing.

use lexer::TokenKind;

use crate::event::NodeKind;
use crate::parser::Parser;

use super::expressions;

/// Parses a list of statements until the given terminator.
pub fn stmt_list(p: &mut Parser, terminator: TokenKind) {
    let m = p.start();

    while !p.at_end() && !p.at(terminator) {
        p.skip_trivia();

        if p.at_end() || p.at(terminator) {
            break;
        }

        statement(p);
    }

    m.complete(p, NodeKind::StmtList);
}

/// Parses a single statement.
pub fn statement(p: &mut Parser) {
    p.skip_trivia();

    match p.current() {
        Some(TokenKind::Semicolon) => {
            let m = p.start();
            p.bump();
            m.complete(p, NodeKind::EmptyStmt);
        }
        Some(TokenKind::KwReturn) => return_stmt(p),
        Some(TokenKind::KwIf) => if_stmt(p),
        Some(TokenKind::KwWhile) => while_stmt(p),
        Some(TokenKind::KwFor) => for_stmt(p),
        Some(TokenKind::KwTry) => try_stmt(p),
        Some(TokenKind::KwRaise) => raise_stmt(p),
        Some(TokenKind::KwBreak) => {
            let m = p.start();
            p.bump();
            p.skip_trivia();
            p.eat(TokenKind::Semicolon);
            m.complete(p, NodeKind::BreakStmt);
        }
        Some(TokenKind::KwContinue) => {
            let m = p.start();
            p.bump();
            p.skip_trivia();
            p.eat(TokenKind::Semicolon);
            m.complete(p, NodeKind::ContinueStmt);
        }
        Some(TokenKind::KwGoto) => goto_stmt(p),
        Some(TokenKind::Label) => label_stmt(p),
        Some(TokenKind::KwVar) => super::items::var_declaration(p),
        Some(TokenKind::KwBeginTransaction) | Some(TokenKind::KwCommitTransaction) | Some(TokenKind::KwRollbackTransaction) => {
            let m = p.start();
            p.bump();
            p.skip_trivia();
            p.eat(TokenKind::Semicolon);
            m.complete(p, NodeKind::CallStmt);
        }
        Some(TokenKind::Ident) => {
            // Could be assignment or call
            assignment_or_call(p);
        }
        _ => {
            // Try to parse as expression statement
            if p.current().is_some() {
                let m = p.start();
                expressions::expression(p);
                p.skip_trivia();
                p.eat(TokenKind::Semicolon);
                m.complete(p, NodeKind::CallStmt);
            }
        }
    }
}

fn return_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Возврат

    p.skip_trivia();

    // Optional return value
    if !p.at(TokenKind::Semicolon) && !p.at(TokenKind::KwEndFunction) && !p.at(TokenKind::KwEndProcedure) {
        expressions::expression(p);
    }

    p.skip_trivia();
    p.eat(TokenKind::Semicolon);

    m.complete(p, NodeKind::ReturnStmt);
}

fn if_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Если

    p.skip_trivia();
    expressions::expression(p);

    p.skip_trivia();
    p.expect(TokenKind::KwThen);

    p.skip_trivia();
    stmt_list_inner(p, &[TokenKind::KwElsIf, TokenKind::KwElse, TokenKind::KwEndIf]);

    // ElsIf clauses
    while p.at(TokenKind::KwElsIf) {
        let em = p.start();
        p.bump();
        p.skip_trivia();
        expressions::expression(p);
        p.skip_trivia();
        p.expect(TokenKind::KwThen);
        p.skip_trivia();
        stmt_list_inner(p, &[TokenKind::KwElsIf, TokenKind::KwElse, TokenKind::KwEndIf]);
        em.complete(p, NodeKind::ElseIfClause);
    }

    // Else clause
    if p.at(TokenKind::KwElse) {
        let em = p.start();
        p.bump();
        p.skip_trivia();
        stmt_list_inner(p, &[TokenKind::KwEndIf]);
        em.complete(p, NodeKind::ElseClause);
    }

    p.skip_trivia();
    p.expect(TokenKind::KwEndIf);

    p.skip_trivia();
    p.eat(TokenKind::Semicolon);

    m.complete(p, NodeKind::IfStmt);
}

fn while_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Пока

    p.skip_trivia();
    expressions::expression(p);

    p.skip_trivia();
    p.expect(TokenKind::KwDo);

    p.skip_trivia();
    stmt_list(p, TokenKind::KwEndDo);

    p.skip_trivia();
    p.expect(TokenKind::KwEndDo);

    p.skip_trivia();
    p.eat(TokenKind::Semicolon);

    m.complete(p, NodeKind::WhileStmt);
}

fn for_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Для

    p.skip_trivia();

    if p.at(TokenKind::KwEach) {
        // For Each
        p.bump();
        p.skip_trivia();

        if p.at(TokenKind::Ident) {
            p.bump();
        }

        p.skip_trivia();
        p.expect(TokenKind::KwIn);

        p.skip_trivia();
        expressions::expression(p);

        p.skip_trivia();
        p.expect(TokenKind::KwDo);

        p.skip_trivia();
        stmt_list(p, TokenKind::KwEndDo);

        p.skip_trivia();
        p.expect(TokenKind::KwEndDo);

        p.skip_trivia();
        p.eat(TokenKind::Semicolon);

        m.complete(p, NodeKind::ForEachStmt);
    } else {
        // Regular For
        if p.at(TokenKind::Ident) {
            p.bump();
        }

        p.skip_trivia();
        p.expect(TokenKind::Eq);

        p.skip_trivia();
        expressions::expression(p);

        p.skip_trivia();
        p.expect(TokenKind::KwTo);

        p.skip_trivia();
        expressions::expression(p);

        p.skip_trivia();
        p.expect(TokenKind::KwDo);

        p.skip_trivia();
        stmt_list(p, TokenKind::KwEndDo);

        p.skip_trivia();
        p.expect(TokenKind::KwEndDo);

        p.skip_trivia();
        p.eat(TokenKind::Semicolon);

        m.complete(p, NodeKind::ForStmt);
    }
}

fn try_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Попытка

    p.skip_trivia();
    stmt_list(p, TokenKind::KwExcept);

    p.skip_trivia();
    p.expect(TokenKind::KwExcept);

    let em = p.start();
    p.skip_trivia();
    stmt_list(p, TokenKind::KwEndTry);
    em.complete(p, NodeKind::ExceptClause);

    p.skip_trivia();
    p.expect(TokenKind::KwEndTry);

    p.skip_trivia();
    p.eat(TokenKind::Semicolon);

    m.complete(p, NodeKind::TryStmt);
}

fn raise_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump(); // ВызватьИсключение

    p.skip_trivia();

    // Optional expression
    if !p.at(TokenKind::Semicolon) {
        expressions::expression(p);
    }

    p.skip_trivia();
    p.eat(TokenKind::Semicolon);

    m.complete(p, NodeKind::RaiseStmt);
}

fn goto_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Перейти

    p.skip_trivia();

    if p.at(TokenKind::Tilde) {
        p.bump();
    }

    if p.at(TokenKind::Ident) {
        p.bump();
    }

    p.skip_trivia();
    p.eat(TokenKind::Semicolon);

    m.complete(p, NodeKind::GotoStmt);
}

fn label_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Label
    m.complete(p, NodeKind::LabelStmt);
}

fn assignment_or_call(p: &mut Parser) {
    let m = p.start();

    expressions::expression(p);

    p.skip_trivia();

    if p.eat(TokenKind::Eq) {
        p.skip_trivia();
        expressions::expression(p);
        p.skip_trivia();
        p.eat(TokenKind::Semicolon);
        m.complete(p, NodeKind::AssignStmt);
    } else {
        p.eat(TokenKind::Semicolon);
        m.complete(p, NodeKind::CallStmt);
    }
}

fn stmt_list_inner(p: &mut Parser, terminators: &[TokenKind]) {
    let m = p.start();

    while !p.at_end() && !terminators.iter().any(|t| p.at(*t)) {
        p.skip_trivia();

        if p.at_end() || terminators.iter().any(|t| p.at(*t)) {
            break;
        }

        statement(p);
    }

    m.complete(p, NodeKind::StmtList);
}
