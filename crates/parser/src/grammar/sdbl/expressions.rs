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

    loop {
        p.skip_trivia();
        if p.at(TokenKind::KwOr) {
            p.check_iteration_limit();
            p.bump(); // OR
            p.skip_trivia();
            logical_and_expr(p);
        } else {
            break;
        }
    }

    m.complete(p, NodeKind::SdblLogicalOrExpr);
}

/// Parse AND expression
///
/// Grammar: `logicalAndExpression: notExpression (AND notExpression)*`
fn logical_and_expr(p: &mut Parser) {
    let m = p.start();

    not_expr(p);

    loop {
        p.skip_trivia();
        if p.at(TokenKind::KwAnd) {
            p.check_iteration_limit();
            p.bump(); // AND
            p.skip_trivia();
            not_expr(p);
        } else {
            break;
        }
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
/// Grammar:
/// ```text
/// comparisonExpression:
///     additiveExpression ((= | <> | < | <= | > | >=) additiveExpression)?
///   | predicateExpression
/// ```
fn comparison_expr(p: &mut Parser) {
    predicate_expr(p);
}

/// Parse predicate expression (IN, BETWEEN, IS NULL, etc.)
///
/// Grammar:
/// ```text
/// predicateExpression:
///     additiveExpression
///       ( (IN | В) LPAREN (subquery | valueList) RPAREN
///       | BETWEEN expr AND expr
///       | IS (NOT)? NULL
///       | (= | <> | < | <= | > | >=) additiveExpression
///       )?
/// ```
fn predicate_expr(p: &mut Parser) {
    let m = p.start();

    additive_expr(p);

    p.skip_trivia();

    // Check for optional NOT before predicates (NOT IN, NOT BETWEEN, NOT LIKE)
    // The NOT token is recorded in the predicate node and processed during lowering
    if p.at(TokenKind::KwNot) {
        p.bump(); // NOT / НЕ
        p.skip_trivia();
    }

    // Check for IN predicate (IN or IN HIERARCHY)
    if p.at(TokenKind::KwIn) {
        p.bump(); // IN / В
        p.skip_trivia();

        // Check for HIERARCHY after IN
        if p.at_keyword("HIERARCHY") || p.at_keyword("ИЕРАРХИИ") {
            p.bump(); // HIERARCHY / ИЕРАРХИИ
            p.skip_trivia();

            // IN HIERARCHY expects single expression in parentheses
            if !p.expect(TokenKind::LParen) {
                m.complete(p, NodeKind::SdblInHierarchyExpr);
                return;
            }
            p.skip_trivia();

            // Parse hierarchy root expression
            expression(p);

            p.skip_trivia();
            p.expect(TokenKind::RParen);
            m.complete(p, NodeKind::SdblInHierarchyExpr);
        } else {
            // Regular IN predicate
            if !p.expect(TokenKind::LParen) {
                m.complete(p, NodeKind::SdblInExpr);
                return;
            }
            p.skip_trivia();

            // Check if it's a subquery or value list
            if p.at_keyword("SELECT") || p.at_keyword("ВЫБРАТЬ") {
                // Parse subquery
                super::select::subquery(p);
            } else {
                // Parse value list: expr, expr, ...
                expression(p);
                while p.eat(TokenKind::Comma) {
                    p.skip_trivia();
                    expression(p);
                }
            }

            p.skip_trivia();
            p.expect(TokenKind::RParen);
            m.complete(p, NodeKind::SdblInExpr);
        }
    }
    // Check for IS NULL predicate
    else if p.at_keyword("IS") || p.at_keyword("ЕСТЬ") {
        p.bump(); // IS / ЕСТЬ
        p.skip_trivia();

        // Optional NOT
        if p.at(TokenKind::KwNot) {
            p.bump(); // NOT / НЕ
            p.skip_trivia();
        }

        // Expect NULL keyword
        if !p.at_keyword("NULL") {
            // Error recovery: expected NULL after IS [NOT]
            m.abandon(p);
            return;
        }
        p.bump(); // NULL

        m.complete(p, NodeKind::SdblIsNullExpr);
    }
    // Check for BETWEEN predicate
    else if p.at_keyword("BETWEEN") || p.at_keyword("МЕЖДУ") {
        p.bump(); // BETWEEN / МЕЖДУ
        p.skip_trivia();

        // Parse low expression
        additive_expr(p);
        p.skip_trivia();

        // Expect AND keyword
        if !p.at(TokenKind::KwAnd) {
            // Error recovery: expected AND in BETWEEN
            m.complete(p, NodeKind::SdblBetweenExpr);
            return;
        }
        p.bump(); // AND / И
        p.skip_trivia();

        // Parse high expression
        additive_expr(p);

        m.complete(p, NodeKind::SdblBetweenExpr);
    }
    // Check for LIKE predicate
    else if p.at_keyword("LIKE") || p.at_keyword("ПОДОБНО") {
        p.bump(); // LIKE / ПОДОБНО
        p.skip_trivia();

        // Parse pattern expression
        additive_expr(p);
        p.skip_trivia();

        // Optional ESCAPE clause
        if p.at_keyword("ESCAPE") || p.at_keyword("СПЕЦСИМВОЛ") {
            p.bump(); // ESCAPE / СПЕЦСИМВОЛ
            p.skip_trivia();
            additive_expr(p);
        }

        m.complete(p, NodeKind::SdblLikeExpr);
    }
    // Check for REFS predicate (ССЫЛКА)
    else if p.at_keyword("REFS") || p.at_keyword("ССЫЛКА") {
        p.bump(); // REFS / ССЫЛКА
        p.skip_trivia();

        // Parse MDO reference (e.g., Справочник.ПолныеРоли)
        // For now, treat it as a simple path of identifiers separated by dots
        if p.at(TokenKind::Ident) {
            p.bump(); // First identifier (e.g., Справочник)
            p.skip_trivia();

            // Parse remaining parts (e.g., .ПолныеРоли)
            while p.eat(TokenKind::Dot) {
                p.skip_trivia();
                if p.at(TokenKind::Ident) {
                    p.bump(); // Next identifier
                    p.skip_trivia();
                } else {
                    break;
                }
            }
        }

        m.complete(p, NodeKind::SdblRefsExpr);
    }
    // Check for comparison operators
    else if matches!(
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

/// Parse primary expression (literals, columns, functions, parenthesized expressions, CASE)
///
/// Grammar: `primaryExpression: literal | column | functionCall | parameter | LPAREN expression RPAREN | caseExpression`
fn primary_expr(p: &mut Parser) {
    // Check for CASE keyword first (using at_keyword since CASE might be IDENT token)
    if p.at_keyword("CASE") || p.at_keyword("ВЫБОР") {
        case_expr(p);
        return;
    }

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
    // Special handling for String tokens to detect multiString
    if p.at(TokenKind::String) {
        string_literal_or_multi(p);
    } else {
        // Other literals (numbers, booleans, null)
        let m = p.start();
        p.bump();
        m.complete(p, NodeKind::SdblLiteral);
    }
}

/// Parse string literal or multiString
///
/// Grammar: `multiString: STR+`
///
/// Creates SDBL_MULTI_STRING if multiple consecutive String tokens.
/// For single String token, creates SDBL_LITERAL (even if it contains newlines).
///
/// NOTE: The diagnostic handler will check BOTH:
/// 1. SDBL_MULTI_STRING nodes (multiple strings)
/// 2. SDBL_LITERAL nodes with String tokens containing newlines
fn string_literal_or_multi(p: &mut Parser) {
    let m = p.start();

    // Bump first String token
    p.bump();

    // Check for consecutive String tokens (multiString: STR+)
    let mut count = 1;
    while p.at(TokenKind::String) {
        p.bump();
        count += 1;
    }

    // Create SDBL_MULTI_STRING only if multiple consecutive strings
    if count > 1 {
        m.complete(p, NodeKind::SdblMultiString);
    } else {
        m.complete(p, NodeKind::SdblLiteral);
    }
}

/// Parse parameter expression (&Parameter)
///
/// Grammar: `parameter: AMPERSAND identifier`
///
/// SDBL &Parameter may be lexed as one token or two (Ampersand + Ident). Handle both.
/// NOTE: Do NOT skip trivia between & and identifier - parameters must be written
/// without whitespace: `&Param`, not `& Param`. Skipping trivia causes parser to
/// consume following keywords (like ON/ПО) as part of parameter name.
fn parameter_expr(p: &mut Parser) {
    let m = p.start();
    p.bump(); // Consume AMPERSAND

    // Only consume identifier if it immediately follows & (no whitespace)
    if p.at(TokenKind::Ident) {
        p.bump();
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
/// ```text
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

            // ERROR RECOVERY: Check if next token is a clause keyword
            // This prevents "Очередь.\nИЗ" from parsing ИЗ as field name
            if super::select::is_clause_keyword(p) {
                // Mark incomplete column ref as error WITHOUT consuming the keyword
                // This allows FROM/WHERE/etc. parser to see and handle the keyword
                let err = p.start();
                err.complete(p, NodeKind::Error); // Empty ERROR node marking the issue
                break; // Stop parsing column ref, let clause parser handle it
            }

            // ERROR RECOVERY: Check if next token is comma or EOF
            // This prevents "Очередь.," from consuming the comma
            if p.at(TokenKind::Comma) || p.at_end() {
                // Incomplete column ref - create ERROR but DON'T consume comma
                let err = p.start();
                err.complete(p, NodeKind::Error); // Empty ERROR node
                break; // Let selected_fields() see the comma
            }

            if !p.expect(TokenKind::Ident) {
                // Error recovery (p.expect already created ERROR node)
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

/// Parse CASE expression
///
/// Grammar:
/// ```text
/// caseExpression:
///     CASE [operand]
///     (WHEN condition THEN result)+
///     [ELSE elseResult]
///     END
/// ```
///
/// Two forms:
/// - Simple CASE: `CASE operand WHEN value THEN result ...`
/// - Searched CASE: `CASE WHEN condition THEN result ...`
fn case_expr(p: &mut Parser) {
    let m = p.start();

    p.bump(); // CASE / ВЫБОР
    p.skip_trivia();

    // Check if this is a simple CASE (has operand) or searched CASE (no operand)
    // Lookahead: if next token is WHEN, it's searched CASE
    let is_searched_case = p.at_keyword("WHEN") || p.at_keyword("КОГДА");

    if !is_searched_case {
        // Simple CASE: parse operand expression
        expression(p);
        p.skip_trivia();
    }

    // Parse one or more WHEN clauses
    let mut has_when = false;
    while p.at_keyword("WHEN") || p.at_keyword("КОГДА") {
        has_when = true;
        when_clause(p);
        p.skip_trivia();
    }

    if !has_when {
        // Error recovery: CASE without WHEN clauses
        p.error();
    }

    // Optional ELSE clause
    if p.at_keyword("ELSE") || p.at_keyword("ИНАЧЕ") {
        p.bump(); // ELSE / ИНАЧЕ
        p.skip_trivia();
        expression(p);
        p.skip_trivia();
    }

    // Required END keyword
    if !p.at_keyword("END") && !p.at_keyword("КОНЕЦ") {
        // Error recovery: expected END after CASE
        p.error();
    } else {
        p.bump(); // END / КОНЕЦ
    }

    m.complete(p, NodeKind::SdblCaseExpr);
}

/// Parse WHEN clause in CASE expression
///
/// Grammar: `WHEN condition THEN result`
fn when_clause(p: &mut Parser) {
    let m = p.start();

    p.bump(); // WHEN / КОГДА
    p.skip_trivia();

    // Parse condition expression
    expression(p);
    p.skip_trivia();

    // Expect THEN keyword
    if !p.at_keyword("THEN") && !p.at_keyword("ТОГДА") {
        // Error recovery: expected THEN after WHEN condition
        m.complete(p, NodeKind::SdblWhenClause);
        return;
    }
    p.bump(); // THEN / ТОГДА
    p.skip_trivia();

    // Parse result expression
    expression(p);

    m.complete(p, NodeKind::SdblWhenClause);
}
