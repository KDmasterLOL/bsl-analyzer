//! SDBL expression parsing
//!
//! Implements parsing for SDBL expressions including:
//! - Logical expressions (AND, OR, NOT)
//! - Comparison operations
//! - Arithmetic operations
//! - Column references
//! - Function calls
//! - Literals and parameters
//!
//! Phase 1 (MVP): Basic expression support for SELECT fields and WHERE clauses
//! Phase 2-3: Complete expression grammar (CASE, predicates, complex operators)

use crate::event::NodeKind;
use crate::parser::Parser;
use lexer::TokenKind;

/// Entry point for logical expressions (used in WHERE, HAVING clauses)
///
/// Grammar: `logicalExpression: predicate ((AND | OR) predicate)*`
pub fn logical_expression(p: &mut Parser) {
    logical_or_expr(p);
}

/// Entry point for general expressions (used in SELECT fields, etc.)
///
/// Grammar: `expression: logicalExpression | caseExpression | ...`
///
/// Phase 1: Same as logical_expression
/// Phase 2: Add CASE expressions, type casts
pub fn expression(p: &mut Parser) {
    logical_or_expr(p);
}

/// Parse OR expression (lowest precedence)
///
/// Grammar: `logicalOrExpression: logicalAndExpression (OR logicalAndExpression)*`
fn logical_or_expr(p: &mut Parser) {
    let m = p.start();

    logical_and_expr(p);

    while p.at(TokenKind::KwOr) {
        p.check_iteration_limit();
        p.bump(); // OR
        p.skip_trivia();
        logical_and_expr(p);
    }

    m.complete(p, NodeKind::SdblLogicalOrExpr);
}

/// Parse AND expression
///
/// Grammar: `logicalAndExpression: notExpression (AND notExpression)*`
fn logical_and_expr(p: &mut Parser) {
    let m = p.start();

    not_expr(p);

    while p.at(TokenKind::KwAnd) {
        p.check_iteration_limit();
        p.bump(); // AND
        p.skip_trivia();
        not_expr(p);
    }

    m.complete(p, NodeKind::SdblLogicalAndExpr);
}

/// Parse NOT expression
///
/// Grammar: `notExpression: NOT* predicate`
fn not_expr(p: &mut Parser) {
    if p.at(TokenKind::KwNot) {
        let m = p.start();
        p.bump(); // NOT
        p.skip_trivia();
        not_expr(p); // Recursive for multiple NOTs
        m.complete(p, NodeKind::SdblNotExpr);
    } else {
        comparison_expr(p);
    }
}

/// Parse comparison expression
///
/// Grammar: `comparisonExpression: additiveExpression ((= | <> | < | <= | > | >=) additiveExpression)?`
fn comparison_expr(p: &mut Parser) {
    let m = p.start();

    additive_expr(p);

    // Check for comparison operators
    if matches!(
        p.current(),
        Some(TokenKind::Eq)
            | Some(TokenKind::Neq)
            | Some(TokenKind::Lt)
            | Some(TokenKind::Le)
            | Some(TokenKind::Gt)
            | Some(TokenKind::Ge)
    ) {
        p.bump(); // comparison operator
        p.skip_trivia();
        additive_expr(p);
        m.complete(p, NodeKind::SdblComparisonExpr);
    } else {
        m.abandon(p);
    }
}

/// Parse additive expression (+ and -)
///
/// Grammar: `additiveExpression: multiplicativeExpression ((PLUS | MINUS) multiplicativeExpression)*`
fn additive_expr(p: &mut Parser) {
    let m = p.start();

    multiplicative_expr(p);

    while matches!(p.current(), Some(TokenKind::Plus) | Some(TokenKind::Minus)) {
        p.check_iteration_limit();
        p.bump(); // + or -
        p.skip_trivia();
        multiplicative_expr(p);
    }

    m.complete(p, NodeKind::SdblAdditiveExpr);
}

/// Parse multiplicative expression (* and /)
///
/// Grammar: `multiplicativeExpression: unaryExpression ((MUL | DIV | MOD) unaryExpression)*`
fn multiplicative_expr(p: &mut Parser) {
    let m = p.start();

    unary_expr(p);

    while matches!(
        p.current(),
        Some(TokenKind::Star) | Some(TokenKind::Slash) | Some(TokenKind::Percent)
    ) {
        p.check_iteration_limit();
        p.bump(); // *, /, %
        p.skip_trivia();
        unary_expr(p);
    }

    m.complete(p, NodeKind::SdblMultiplicativeExpr);
}

/// Parse unary expression (+, -, NOT prefix)
///
/// Grammar: `unaryExpression: (PLUS | MINUS | NOT)? primaryExpression`
fn unary_expr(p: &mut Parser) {
    if matches!(
        p.current(),
        Some(TokenKind::Plus) | Some(TokenKind::Minus) | Some(TokenKind::KwNot)
    ) {
        let m = p.start();
        p.bump(); // unary operator
        p.skip_trivia();
        unary_expr(p); // Recursive for multiple unary operators
        m.complete(p, NodeKind::SdblUnaryExpr);
    } else {
        primary_expr(p);
    }
}

/// Parse primary expression (literals, columns, functions, parenthesized expressions)
///
/// Grammar: `primaryExpression: literal | column | functionCall | parameter | LPAREN expression RPAREN`
fn primary_expr(p: &mut Parser) {
    match p.current() {
        Some(TokenKind::LParen) => paren_or_subquery_expr(p),
        Some(TokenKind::Ident) => column_or_function(p),
        Some(TokenKind::Decimal) | Some(TokenKind::Float) => literal_expr(p),
        Some(TokenKind::String) => literal_expr(p),
        Some(TokenKind::KwTrue) | Some(TokenKind::KwFalse) => literal_expr(p),
        Some(TokenKind::KwNull) | Some(TokenKind::KwUndefined) => literal_expr(p),
        Some(TokenKind::Ampersand) => parameter_expr(p),
        _ => {
            // Error recovery: unexpected token
            let m = p.start();
            p.error();
            m.complete(p, NodeKind::SdblError);
        }
    }
}

/// Parse literal expression (numbers, strings, booleans, null)
fn literal_expr(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Literal token
    m.complete(p, NodeKind::SdblLiteral);
}

/// Parse parameter expression (&Parameter)
///
/// Grammar: `parameter: AMPERSAND identifier`
fn parameter_expr(p: &mut Parser) {
    let m = p.start();
    p.bump(); // &
    p.skip_trivia();

    if !p.expect(TokenKind::Ident) {
        // Error recovery
    }

    m.complete(p, NodeKind::SdblParameter);
}

/// Parse parenthesized expression or subquery
///
/// Grammar: `LPAREN (expression | subquery) RPAREN`
///
/// Lookahead: if SELECT keyword after LPAREN, it's a subquery
fn paren_or_subquery_expr(p: &mut Parser) {
    let m = p.start();

    p.bump(); // (
    p.skip_trivia();

    // Lookahead: if SELECT keyword, it's a subquery
    if p.at_keyword("SELECT") {
        // Parse subquery
        super::select::subquery(p);
        p.skip_trivia();
        p.expect(TokenKind::RParen);
        m.complete(p, NodeKind::SdblSubqueryExpr);
    } else {
        // Parse regular expression
        expression(p);
        p.skip_trivia();
        p.expect(TokenKind::RParen);
        m.complete(p, NodeKind::SdblParenExpr);
    }
}

/// Parse column reference or function call
///
/// Lookahead determines which:
/// - Followed by DOT → column reference (Table.Column)
/// - Followed by LPAREN → function call
/// - Otherwise → simple column reference
///
/// Grammar:
/// ```
/// column: identifier (DOT identifier)*
/// functionCall: identifier LPAREN arguments? RPAREN
/// ```
fn column_or_function(p: &mut Parser) {
    let m = p.start();

    // First identifier (mandatory)
    p.bump(); // Ident
    p.skip_trivia();

    // Check for DOT (column reference) or LPAREN (function call)
    if p.at(TokenKind::Dot) {
        // Column reference: Table.Column or MDO.Table.Column
        while p.eat(TokenKind::Dot) {
            p.skip_trivia();
            if !p.expect(TokenKind::Ident) {
                // Error recovery
                break;
            }
            p.skip_trivia();
        }
        m.complete(p, NodeKind::SdblColumnRef);
    } else if p.at(TokenKind::LParen) {
        // Function call
        p.bump(); // (
        p.skip_trivia();

        // Parse arguments (comma-separated expressions)
        if !p.at(TokenKind::RParen) {
            expression(p);

            while p.eat(TokenKind::Comma) {
                p.check_iteration_limit();
                p.skip_trivia();
                expression(p);
            }
        }

        p.skip_trivia();
        p.expect(TokenKind::RParen);

        m.complete(p, NodeKind::SdblFunctionCall);
    } else {
        // Simple column reference (no DOT, no LPAREN)
        m.complete(p, NodeKind::SdblColumnRef);
    }
}
