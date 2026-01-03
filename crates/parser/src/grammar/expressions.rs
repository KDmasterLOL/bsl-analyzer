//! Expression parsing.

use lexer::TokenKind;

use crate::event::NodeKind;
use crate::parser::Parser;

/// Checks if current token is an identifier or keyword (keywords can be property names)
fn is_ident_or_keyword(p: &Parser) -> bool {
    matches!(
        p.current(),
        Some(TokenKind::Ident)
            // Keywords that can be used as property/method names
            | Some(TokenKind::KwProcedure)
            | Some(TokenKind::KwFunction)
            | Some(TokenKind::KwFor)
            | Some(TokenKind::KwTo)
            | Some(TokenKind::KwWhile)
            | Some(TokenKind::KwDo)
            | Some(TokenKind::KwIf)
            | Some(TokenKind::KwThen)
            | Some(TokenKind::KwElse)
            | Some(TokenKind::KwElsIf)
            | Some(TokenKind::KwTry)
            | Some(TokenKind::KwExcept)
            | Some(TokenKind::KwReturn)
            | Some(TokenKind::KwBreak)
            | Some(TokenKind::KwContinue)
            | Some(TokenKind::KwVar)
            | Some(TokenKind::KwNew)
            | Some(TokenKind::KwExecute)
            | Some(TokenKind::KwAnd)
            | Some(TokenKind::KwOr)
            | Some(TokenKind::KwNot)
            | Some(TokenKind::KwTrue)
            | Some(TokenKind::KwFalse)
            | Some(TokenKind::KwAsync)
            | Some(TokenKind::KwAwait)
    )
}

/// Parses an expression.
pub fn expression(p: &mut Parser) {
    or_expr(p);
}

/// Parses a postfix expression for use in assignment left-hand side.
/// This allows parsing "Var", "Obj.Field", "Arr[Index]" without consuming `=` as comparison.
pub fn postfix_expression_for_assignment(p: &mut Parser) {
    postfix_expr(p);
}

fn or_expr(p: &mut Parser) {
    let mut lhs = p.start();
    and_expr(p);

    while p.at(TokenKind::KwOr) {
        p.check_iteration_limit();
        let m = lhs.complete(p, NodeKind::Expr).precede(p);
        p.bump();
        p.skip_trivia();
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

    while p.at(TokenKind::KwAnd) {
        p.check_iteration_limit();
        let m = lhs.complete(p, NodeKind::Expr).precede(p);
        p.bump();
        p.skip_trivia();
        let rhs = p.start();
        not_expr(p);
        rhs.complete(p, NodeKind::Expr);
        lhs = m.complete(p, NodeKind::BinaryExpr).precede(p);
    }

    lhs.complete(p, NodeKind::Expr);
}

fn not_expr(p: &mut Parser) {
    if p.at(TokenKind::KwNot) {
        let m = p.start();
        p.bump();
        p.skip_trivia();
        not_expr(p);
        m.complete(p, NodeKind::UnaryExpr);
    } else {
        comparison_expr(p);
    }
}

fn comparison_expr(p: &mut Parser) {
    let m = p.start();
    let lhs = p.start();
    additive_expr(p);

    match p.current() {
        // Include Eq for comparisons (needed for diagnostics like IdenticalExpressions)
        // Context determines if it's assignment or comparison
        Some(TokenKind::Eq) | Some(TokenKind::Neq) | Some(TokenKind::Lt) | Some(TokenKind::Le)
        | Some(TokenKind::Gt) | Some(TokenKind::Ge) => {
            lhs.complete(p, NodeKind::Expr);
            p.bump();
            p.skip_trivia();
            let rhs = p.start();
            additive_expr(p);
            rhs.complete(p, NodeKind::Expr);
            m.complete(p, NodeKind::BinaryExpr);
        }
        _ => {
            lhs.abandon(p);
            m.abandon(p);
        }
    }
}

fn additive_expr(p: &mut Parser) {
    let mut lhs = p.start();
    multiplicative_expr(p);

    while matches!(p.current(), Some(TokenKind::Plus) | Some(TokenKind::Minus)) {
        p.check_iteration_limit();
        let m = lhs.complete(p, NodeKind::Expr).precede(p);
        p.bump();
        p.skip_trivia();
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

    while matches!(
        p.current(),
        Some(TokenKind::Star) | Some(TokenKind::Slash) | Some(TokenKind::Percent)
    ) {
        p.check_iteration_limit();
        let m = lhs.complete(p, NodeKind::Expr).precede(p);
        p.bump();
        p.skip_trivia();
        let rhs = p.start();
        unary_expr(p);
        rhs.complete(p, NodeKind::Expr);
        lhs = m.complete(p, NodeKind::BinaryExpr).precede(p);
    }

    lhs.complete(p, NodeKind::Expr);
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
        p.check_iteration_limit();
        p.skip_trivia();
        match p.current() {
            Some(TokenKind::Dot) => {
                p.bump();
                p.skip_trivia();
                // After dot, accept identifiers OR keywords as property names
                // (e.g., Объект.По, Объект.Для - keywords used as property names)
                if is_ident_or_keyword(p) {
                    p.bump();
                } else {
                    p.error(); // Expected property name after dot
                               // ERROR RECOVERY: Don't consume token - just exit loop
                               // Let upper-level parser decide what to do with it
                    break;
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
        Some(TokenKind::Decimal) | Some(TokenKind::Float) | Some(TokenKind::Date) => {
            let m = p.start();
            p.bump();
            m.complete(p, NodeKind::Literal);
        }
        Some(TokenKind::String) => {
            let m = p.start();
            p.bump(); // Single-line string
            m.complete(p, NodeKind::Literal);
        }
        Some(TokenKind::StringStart) => {
            let m = p.start();
            // Multi-line string: StringStart (Newline Whitespace? StringPart)* StringTail
            // Don't call skip_trivia() - newlines are part of the string structure
            p.bump(); // StringStart

            // Consume everything until STRING_TAIL (without skipping trivia)
            loop {
                p.check_iteration_limit();
                match p.current() {
                    Some(TokenKind::StringTail) => {
                        p.bump();
                        break;
                    }
                    Some(TokenKind::Newline)
                    | Some(TokenKind::Whitespace)
                    | Some(TokenKind::StringPart) => {
                        p.bump();
                    }
                    None => {
                        p.error(); // Unclosed string (EOF)
                        break;
                    }
                    _ => {
                        p.error(); // Unexpected token in multiline string
                        break;
                    }
                }
            }
            m.complete(p, NodeKind::Literal);
        }
        Some(TokenKind::StringPart) | Some(TokenKind::StringTail) => {
            // These should only appear after StringStart
            p.error(); // Unexpected string fragment
        }
        Some(TokenKind::KwTrue) | Some(TokenKind::KwFalse) => {
            let m = p.start();
            p.bump();
            m.complete(p, NodeKind::Literal);
        }
        Some(TokenKind::KwUndefined) | Some(TokenKind::KwNull) => {
            let m = p.start();
            p.bump();
            m.complete(p, NodeKind::Literal);
        }
        Some(TokenKind::KwAwait) => {
            await_expr(p);
        }
        Some(TokenKind::Ident) => {
            let m = p.start();
            p.bump();
            m.complete(p, NodeKind::Ident);
        }
        Some(TokenKind::LParen) => {
            let m = p.start();
            p.bump();
            p.skip_trivia();
            expression(p);
            p.skip_trivia();
            p.expect(TokenKind::RParen);
            m.complete(p, NodeKind::ParenExpr);
        }
        Some(TokenKind::KwNew) => {
            new_expr(p);
        }
        Some(TokenKind::Question) => {
            ternary_expr(p);
        }
        _ => {
            // Error recovery: consume unexpected token and create error node
            p.error();
        }
    }
}

fn await_expr(p: &mut Parser) {
    p.bump(); // Await
    p.skip_trivia();
    expression(p);
}

fn new_expr(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Новый
    p.skip_trivia();

    if p.at(TokenKind::Ident) {
        p.bump();
    }

    p.skip_trivia();

    if p.at(TokenKind::LParen) {
        arg_list(p);
    }
    m.complete(p, NodeKind::NewExpr);
}

fn ternary_expr(p: &mut Parser) {
    let m = p.start();
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
    m.complete(p, NodeKind::TernaryExpr);
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
            p.check_iteration_limit();
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
