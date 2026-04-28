//! Expression parsing.

use lexer::TokenKind;

use crate::event::NodeKind;
use crate::parser::{CompletedMarker, Parser};

/// Checks if current token is an identifier or keyword (keywords can be property names in BSL)
fn is_ident_or_keyword(p: &Parser) -> bool {
    matches!(
        p.current(),
        Some(TokenKind::Ident)
            // All keywords can be used as property/method names after dot in BSL
            // e.g. Поток.Перейти(), Объект.Для, Объект.Новый
            | Some(TokenKind::KwProcedure)
            | Some(TokenKind::KwEndProcedure)
            | Some(TokenKind::KwFunction)
            | Some(TokenKind::KwEndFunction)
            | Some(TokenKind::KwExport)
            | Some(TokenKind::KwVal)
            | Some(TokenKind::KwIf)
            | Some(TokenKind::KwThen)
            | Some(TokenKind::KwElsIf)
            | Some(TokenKind::KwElse)
            | Some(TokenKind::KwEndIf)
            | Some(TokenKind::KwFor)
            | Some(TokenKind::KwEach)
            | Some(TokenKind::KwIn)
            | Some(TokenKind::KwTo)
            | Some(TokenKind::KwWhile)
            | Some(TokenKind::KwDo)
            | Some(TokenKind::KwEndDo)
            | Some(TokenKind::KwReturn)
            | Some(TokenKind::KwContinue)
            | Some(TokenKind::KwBreak)
            | Some(TokenKind::KwGoto)
            | Some(TokenKind::KwTry)
            | Some(TokenKind::KwExcept)
            | Some(TokenKind::KwEndTry)
            | Some(TokenKind::KwRaise)
            | Some(TokenKind::KwVar)
            | Some(TokenKind::KwNew)
            | Some(TokenKind::KwExecute)
            | Some(TokenKind::KwAddHandler)
            | Some(TokenKind::KwRemoveHandler)
            | Some(TokenKind::KwAsync)
            | Some(TokenKind::KwAwait)
            | Some(TokenKind::KwAnd)
            | Some(TokenKind::KwOr)
            | Some(TokenKind::KwNot)
            | Some(TokenKind::KwTrue)
            | Some(TokenKind::KwFalse)
            | Some(TokenKind::KwUndefined)
            | Some(TokenKind::KwNull)
    )
}

/// Parses an expression.
pub fn expression(p: &mut Parser) {
    or_expr(p);
}

/// Parses a postfix expression for use in assignment left-hand side.
/// This allows parsing "Var", "Obj.Field", "Arr[Index]" without consuming `=` as comparison.
/// Returns true if the expression ends with a call (has parentheses).
pub fn postfix_expression_for_assignment(p: &mut Parser) -> bool {
    postfix_expr_with_call_info(p)
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
    let mut lhs = p.start();
    additive_expr(p);
    let mut saw_comparison = false;

    while matches!(
        p.current(),
        // Include Eq for comparisons (needed for diagnostics like IdenticalExpressions)
        // Context determines if it's assignment or comparison
        Some(
            TokenKind::Eq
                | TokenKind::Neq
                | TokenKind::Lt
                | TokenKind::Le
                | TokenKind::Gt
                | TokenKind::Ge
        )
    ) {
        saw_comparison = true;
        let m = lhs.complete(p, NodeKind::Expr).precede(p);
        p.bump();
        p.skip_trivia();
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
    postfix_expr_with_call_info(p);
}

/// Parses a postfix expression and returns whether it's a valid statement.
/// Valid statements: calls `Foo()` and index access `Arr[i]` (may have side effects).
/// Invalid: bare identifiers `Foo` or field access `Foo.Bar` without call/index.
fn postfix_expr_with_call_info(p: &mut Parser) -> bool {
    let Some(mut lhs) = primary_expr(p) else {
        return false;
    };

    let mut is_valid_statement = false;

    loop {
        p.check_iteration_limit();
        p.skip_trivia();
        match p.current() {
            Some(TokenKind::Dot) => {
                // Wrap the base in a FieldExpr
                let m = lhs.precede(p);
                p.bump();
                p.skip_trivia();
                // After dot, accept identifiers OR keywords as property names
                // (e.g., Объект.По, Объект.Для - keywords used as property names)
                if is_ident_or_keyword(p) {
                    p.bump();
                    lhs = m.complete(p, NodeKind::FieldExpr);
                    is_valid_statement = false;
                } else {
                    p.error(); // Expected property name after dot
                               // ERROR RECOVERY: Complete as FieldExpr anyway, exit loop
                    m.complete(p, NodeKind::FieldExpr);
                    break;
                }
            }
            Some(TokenKind::LBracket) => {
                // Wrap the base in an IndexExpr
                // Index access is valid as statement (may trigger getter with side effects)
                let m = lhs.precede(p);
                p.bump();
                p.skip_trivia();
                expression(p);
                p.skip_trivia();
                p.expect(TokenKind::RBracket);
                lhs = m.complete(p, NodeKind::IndexExpr);
                is_valid_statement = true;
            }
            Some(TokenKind::LParen) => {
                // Wrap the base in a CallExpr
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
        Some(TokenKind::Decimal) | Some(TokenKind::Float) | Some(TokenKind::Date) => {
            let m = p.start();
            p.bump();
            Some(m.complete(p, NodeKind::Literal))
        }
        Some(TokenKind::String) | Some(TokenKind::StringStart) => Some(string_literal(p)),
        Some(TokenKind::StringPart) | Some(TokenKind::StringTail) => {
            // These should only appear after StringStart
            p.error(); // Unexpected string fragment
            None
        }
        Some(TokenKind::KwTrue) | Some(TokenKind::KwFalse) => {
            let m = p.start();
            p.bump();
            Some(m.complete(p, NodeKind::Literal))
        }
        Some(TokenKind::KwUndefined) | Some(TokenKind::KwNull) => {
            let m = p.start();
            p.bump();
            Some(m.complete(p, NodeKind::Literal))
        }
        Some(TokenKind::KwAwait) => Some(await_expr(p)),
        Some(TokenKind::Ident) => {
            let m = p.start();
            p.bump();
            Some(m.complete(p, NodeKind::Ident))
        }
        Some(TokenKind::LParen) => {
            let m = p.start();
            p.bump();
            p.skip_trivia();
            expression(p);
            p.skip_trivia();
            p.expect(TokenKind::RParen);
            Some(m.complete(p, NodeKind::ParenExpr))
        }
        Some(TokenKind::KwNew) => Some(new_expr(p)),
        Some(TokenKind::Question) => Some(ternary_expr(p)),
        _ => {
            // Error recovery: consume unexpected token and create error node
            p.error();
            None
        }
    }
}

fn string_literal(p: &mut Parser) -> CompletedMarker {
    let m = p.start();

    loop {
        match p.current() {
            Some(TokenKind::String) => {
                p.bump();
            }
            Some(TokenKind::StringStart) => {
                p.bump();
                string_continuation_tail(p);
            }
            _ => break,
        }

        if !at_adjacent_string_literal(p) {
            break;
        }

        p.skip_trivia();
    }

    m.complete(p, NodeKind::Literal)
}

fn at_adjacent_string_literal(p: &Parser) -> bool {
    match p.current() {
        Some(TokenKind::String | TokenKind::StringStart) => true,
        Some(TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comment | TokenKind::Bom) => {
            matches!(p.nth_non_trivia(0), Some(TokenKind::String | TokenKind::StringStart))
        }
        _ => false,
    }
}

fn string_continuation_tail(p: &mut Parser) {
    loop {
        p.check_iteration_limit();
        match p.current() {
            Some(TokenKind::StringTail) | Some(TokenKind::String) => {
                p.bump();
                break;
            }
            Some(TokenKind::Newline)
            | Some(TokenKind::Whitespace)
            | Some(TokenKind::Comment)
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
}

fn await_expr(p: &mut Parser) -> CompletedMarker {
    let m = p.start();
    p.bump(); // Await
    p.skip_trivia();
    expression(p);
    m.complete(p, NodeKind::AwaitExpr)
}

fn new_expr(p: &mut Parser) -> CompletedMarker {
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
    m.complete(p, NodeKind::NewExpr)
}

fn ternary_expr(p: &mut Parser) -> CompletedMarker {
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
    m.complete(p, NodeKind::TernaryExpr)
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
