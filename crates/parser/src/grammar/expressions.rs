//! Expression parsing.

use lexer::TokenKind;

use crate::event::NodeKind;
use crate::parser::Parser;

/// Parses an expression.
pub fn expression(p: &mut Parser) {
    or_expr(p);
}

fn or_expr(p: &mut Parser) {
    let mut lhs = p.start();
    and_expr(p);

    while p.at(TokenKind::KwOr) {
        let m = lhs.complete(p, NodeKind::Expr).precede(p);
        p.bump();
        p.skip_trivia();
        and_expr(p);
        lhs = m;
    }

    lhs.complete(p, NodeKind::Expr);
}

fn and_expr(p: &mut Parser) {
    not_expr(p);

    while p.at(TokenKind::KwAnd) {
        p.bump();
        p.skip_trivia();
        not_expr(p);
    }
}

fn not_expr(p: &mut Parser) {
    if p.at(TokenKind::KwNot) {
        p.bump();
        p.skip_trivia();
        not_expr(p);
    } else {
        comparison_expr(p);
    }
}

fn comparison_expr(p: &mut Parser) {
    additive_expr(p);

    match p.current() {
        Some(TokenKind::Eq)
        | Some(TokenKind::Neq)
        | Some(TokenKind::Lt)
        | Some(TokenKind::Le)
        | Some(TokenKind::Gt)
        | Some(TokenKind::Ge) => {
            p.bump();
            p.skip_trivia();
            additive_expr(p);
        }
        _ => {}
    }
}

fn additive_expr(p: &mut Parser) {
    multiplicative_expr(p);

    while matches!(p.current(), Some(TokenKind::Plus) | Some(TokenKind::Minus)) {
        p.bump();
        p.skip_trivia();
        multiplicative_expr(p);
    }
}

fn multiplicative_expr(p: &mut Parser) {
    unary_expr(p);

    while matches!(
        p.current(),
        Some(TokenKind::Star) | Some(TokenKind::Slash) | Some(TokenKind::Percent)
    ) {
        p.bump();
        p.skip_trivia();
        unary_expr(p);
    }
}

fn unary_expr(p: &mut Parser) {
    match p.current() {
        Some(TokenKind::Plus) | Some(TokenKind::Minus) => {
            p.bump();
            p.skip_trivia();
            unary_expr(p);
        }
        _ => postfix_expr(p),
    }
}

fn postfix_expr(p: &mut Parser) {
    primary_expr(p);

    loop {
        p.skip_trivia();
        match p.current() {
            Some(TokenKind::Dot) => {
                p.bump();
                p.skip_trivia();
                if p.at(TokenKind::Ident) {
                    p.bump();
                }
            }
            Some(TokenKind::LBracket) => {
                p.bump();
                p.skip_trivia();
                expression(p);
                p.skip_trivia();
                p.expect(TokenKind::RBracket);
            }
            Some(TokenKind::LParen) => {
                arg_list(p);
            }
            _ => break,
        }
    }
}

fn primary_expr(p: &mut Parser) {
    match p.current() {
        Some(TokenKind::Number) | Some(TokenKind::String) | Some(TokenKind::Date) => {
            p.bump();
        }
        Some(TokenKind::KwTrue) | Some(TokenKind::KwFalse) => {
            p.bump();
        }
        Some(TokenKind::KwUndefined) | Some(TokenKind::KwNull) => {
            p.bump();
        }
        Some(TokenKind::Ident) => {
            p.bump();
        }
        Some(TokenKind::LParen) => {
            p.bump();
            p.skip_trivia();
            expression(p);
            p.skip_trivia();
            p.expect(TokenKind::RParen);
        }
        Some(TokenKind::KwNew) => {
            new_expr(p);
        }
        Some(TokenKind::Question) => {
            ternary_expr(p);
        }
        _ => {
            // Error recovery
        }
    }
}

fn new_expr(p: &mut Parser) {
    p.bump(); // Новый
    p.skip_trivia();

    if p.at(TokenKind::Ident) {
        p.bump();
    }

    p.skip_trivia();

    if p.at(TokenKind::LParen) {
        arg_list(p);
    }
}

fn ternary_expr(p: &mut Parser) {
    p.bump(); // ?
    p.skip_trivia();
    p.expect(TokenKind::LParen);
    p.skip_trivia();
    expression(p); // condition
    p.skip_trivia();
    p.expect(TokenKind::Comma);
    p.skip_trivia();
    expression(p); // then
    p.skip_trivia();
    p.expect(TokenKind::Comma);
    p.skip_trivia();
    expression(p); // else
    p.skip_trivia();
    p.expect(TokenKind::RParen);
}

fn arg_list(p: &mut Parser) {
    let m = p.start();
    p.bump(); // (

    p.skip_trivia();

    if !p.at(TokenKind::RParen) {
        // First argument (might be empty)
        if !p.at(TokenKind::Comma) && !p.at(TokenKind::RParen) {
            expression(p);
        }

        while p.eat(TokenKind::Comma) {
            p.skip_trivia();
            if !p.at(TokenKind::Comma) && !p.at(TokenKind::RParen) {
                expression(p);
            }
        }
    }

    p.skip_trivia();
    p.expect(TokenKind::RParen);

    m.complete(p, NodeKind::ArgList);
}
