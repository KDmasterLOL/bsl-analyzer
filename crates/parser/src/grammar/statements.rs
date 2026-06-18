use lexer::TokenKind;
use parser_error::{ParseError, RecoveryKind};

use crate::event::NodeKind;
use crate::parser::Parser;

use super::expressions;

pub fn stmt_list(p: &mut Parser, terminator: TokenKind) {
    let m = p.start();

    while !p.at_end() && !p.at(terminator) {
        p.check_iteration_limit();
        p.skip_trivia();

        if p.at_end() || p.at(terminator) {
            break;
        }

        statement(p);
    }

    m.complete(p, NodeKind::StmtList);
}

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
        Some(TokenKind::Tilde) => label_stmt(p),
        Some(TokenKind::KwExecute) => execute_stmt(p),
        Some(TokenKind::KwAddHandler) => add_handler_stmt(p),
        Some(TokenKind::KwRemoveHandler) => remove_handler_stmt(p),
        Some(TokenKind::KwVar) => super::items::var_declaration(p),
        Some(TokenKind::PreRegion) => super::preprocessor_region(p),
        Some(TokenKind::PreEndRegion) => super::preprocessor_end_region(p),
        Some(TokenKind::PreIf) => super::preprocessor_if(p),
        Some(TokenKind::PreDelete) => super::preprocessor_delete(p),
        Some(TokenKind::PreInsert) => super::preprocessor_insert(p),
        Some(TokenKind::Ident) => {
            assignment_or_call(p);
        }
        _ => {
            if p.current().is_some() {
                let m = p.start();
                expressions::expression(p);
                p.skip_trivia();
                m.complete(p, NodeKind::CallStmt);
                p.eat(TokenKind::Semicolon);
            }
        }
    }
}

fn return_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.skip_trivia();

    if !p.at(TokenKind::Semicolon)
        && !p.at(TokenKind::KwEndFunction)
        && !p.at(TokenKind::KwEndProcedure)
        && !p.at(TokenKind::KwEndIf)
        && !p.at(TokenKind::KwElsIf)
        && !p.at(TokenKind::KwElse)
        && !p.at(TokenKind::KwEndDo)
        && !p.at(TokenKind::KwExcept)
        && !p.at(TokenKind::KwEndTry)
    {
        expressions::expression(p);
    }

    p.skip_trivia();
    p.eat(TokenKind::Semicolon);

    m.complete(p, NodeKind::ReturnStmt);
}

fn if_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.skip_trivia();
    expressions::expression(p);

    p.skip_trivia();
    p.expect(TokenKind::KwThen);

    p.skip_trivia();
    stmt_list_inner(p, &[TokenKind::KwElsIf, TokenKind::KwElse, TokenKind::KwEndIf]);

    while p.at(TokenKind::KwElsIf) {
        p.check_iteration_limit();
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
    p.bump();

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
    p.bump();

    p.skip_trivia();

    if p.at(TokenKind::KwEach) {
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
    p.bump();

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
    p.bump();

    p.skip_trivia();

    if p.at(TokenKind::LParen) {
        parse_raise_call_args(p);
    } else if !at_bare_raise_boundary(p) {
        expressions::expression(p);
    }

    p.skip_trivia();
    p.eat(TokenKind::Semicolon);

    m.complete(p, NodeKind::RaiseStmt);
}

fn at_bare_raise_boundary(p: &Parser) -> bool {
    matches!(
        p.current(),
        Some(
            TokenKind::Semicolon
                | TokenKind::KwEndTry
                | TokenKind::KwExcept
                | TokenKind::KwEndIf
                | TokenKind::KwElsIf
                | TokenKind::KwElse
                | TokenKind::KwEndDo
                | TokenKind::KwEndProcedure
                | TokenKind::KwEndFunction
                | TokenKind::PreElsIf
                | TokenKind::PreElse
                | TokenKind::PreEndIf
        ) | None
    )
}

fn parse_raise_call_args(p: &mut Parser) {
    assert!(p.at(TokenKind::LParen));
    p.bump();

    p.skip_trivia();

    while !p.at(TokenKind::RParen) && !p.at_end() {
        p.skip_trivia();

        if p.at(TokenKind::Comma) {
        } else if !p.at(TokenKind::RParen) {
            expressions::expression(p);
        }

        p.skip_trivia();

        if p.at(TokenKind::Comma) {
            p.bump();
        } else if !p.at(TokenKind::RParen) {
            break;
        }
    }

    p.expect(TokenKind::RParen);
}

fn goto_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

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
    p.bump();
    p.expect(TokenKind::Ident);
    p.expect(TokenKind::Colon);
    m.complete(p, NodeKind::LabelStmt);
}

fn execute_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.skip_trivia();

    expressions::expression(p);

    p.skip_trivia();
    p.eat(TokenKind::Semicolon);

    m.complete(p, NodeKind::ExecuteStmt);
}

fn add_handler_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.skip_trivia();

    expressions::expression(p);

    p.skip_trivia();
    p.expect(TokenKind::Comma);

    p.skip_trivia();

    expressions::expression(p);

    p.skip_trivia();
    p.eat(TokenKind::Semicolon);

    m.complete(p, NodeKind::AddHandlerStmt);
}

fn remove_handler_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.skip_trivia();

    expressions::expression(p);

    p.skip_trivia();
    p.expect(TokenKind::Comma);

    p.skip_trivia();

    expressions::expression(p);

    p.skip_trivia();
    p.eat(TokenKind::Semicolon);

    m.complete(p, NodeKind::RemoveHandlerStmt);
}

fn assignment_or_call(p: &mut Parser) {
    let m = p.start();

    let is_valid_stmt = expressions::postfix_expression_for_assignment(p);

    p.skip_trivia();

    if p.eat(TokenKind::Eq) {
        p.skip_trivia();
        expressions::expression(p);
        p.skip_trivia();
        m.complete(p, NodeKind::AssignStmt);
        p.eat(TokenKind::Semicolon);
    } else if is_valid_stmt {
        m.complete(p, NodeKind::CallStmt);
        p.eat(TokenKind::Semicolon);
    } else {
        p.emit_error_at_marker(
            m,
            ParseError::Custom {
                message: "ожидался вызов или присваивание",
                recovery: RecoveryKind::RecoverySpan,
            },
        );
        p.eat(TokenKind::Semicolon);
    }
}

fn stmt_list_inner(p: &mut Parser, terminators: &[TokenKind]) {
    let m = p.start();

    while !p.at_end() && !terminators.iter().any(|t| p.at(*t)) {
        p.check_iteration_limit();
        p.skip_trivia();

        if p.at_end() || terminators.iter().any(|t| p.at(*t)) {
            break;
        }

        statement(p);
    }

    m.complete(p, NodeKind::StmtList);
}
