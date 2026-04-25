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
//! ## Provenance
//!
//! Slice 10a — clean-room (in progress): expression backbone (atoms +
//! operator precedence chain + parens / tuple / subquery). Authored from
//! `docs/legal/sdbl-expressions-mini-spec.md` and ITS pubqlang/10 + /12.
//! See `docs/legal/sdbl-clean-room-slice10a.md` for the attestation
//! (landed with C3).
//!
//! Slice 10b — pending: predicates, comparison, column-or-function, CAST,
//! CASE. Bodies remain Tier B under the LEGACY banner; the
//! `comparison_expr_legacy` / `predicate_expr_legacy` shims preserve the
//! Slice 10a → Slice 10b dispatch boundary.

use crate::event::NodeKind;
use crate::parser::Parser;
use lexer::TokenKind;

// ============================================================================
// CLEAN-ROOM Slice 10a — expression backbone
// ============================================================================
//
// See `docs/legal/sdbl-clean-room-slice10a.md` for authorship and source
// citations (landed with C3). Per-function provenance comments are
// attached at C2.
//
// The 17 functions below cover the Slice 10a surface:
//   - Helpers: is_expression_start, is_recovery_point,
//     recover_to_delimiter, parse_delimited_list
//   - Entries: logical_expression, expression
//   - Operator chain: logical_or_expr, logical_and_expr, not_expr,
//     additive_expr, multiplicative_expr, unary_expr
//   - Primary dispatch + atoms: primary_expr, literal_expr,
//     string_literal_or_multi, parameter_expr, paren_or_subquery_expr
//
// Slice 10a's `not_expr` calls into `comparison_expr_legacy` (Slice 10b
// territory, defined under the LEGACY banner below) — that is the only
// Slice-10a → Slice-10b dispatch boundary in this file.

// ============================================================================
// Helper Functions for Error Recovery
// ============================================================================

/// Check if current token can start an expression.
///
/// Used for error recovery in list parsing - allows detecting empty elements
/// and clause keywords that shouldn't be consumed as expressions.
///
/// # Returns
///
/// `true` if current token can start an expression:
/// - Literals: numbers, strings, booleans, null
/// - Identifiers (columns, functions)
/// - Operators: `+`, `-`, `NOT` (unary), `*` (for COUNT(*))
/// - Parentheses: `(` (parenthesized expression or subquery)
/// - Parameters: `&Parameter`
/// - Keywords: `CASE`
///
/// `false` otherwise (including clause keywords like FROM, WHERE, etc.)
pub(super) fn is_expression_start(p: &Parser) -> bool {
    // Check for tokens that can start an expression
    match p.current() {
        // Literals
        Some(TokenKind::Decimal)
        | Some(TokenKind::Float)
        | Some(TokenKind::String)
        | Some(TokenKind::KwTrue)
        | Some(TokenKind::KwFalse)
        | Some(TokenKind::KwNull)
        | Some(TokenKind::KwUndefined) => true,

        // Identifiers (column references, function calls)
        Some(TokenKind::Ident) => {
            // Exclude clause keywords - they're not expressions
            !super::select::is_clause_keyword(p)
        }

        // Unary operators and Star (for COUNT(*) special syntax)
        Some(TokenKind::Plus)
        | Some(TokenKind::Minus)
        | Some(TokenKind::KwNot)
        | Some(TokenKind::Star) => true,

        // Parenthesized expressions or subqueries
        Some(TokenKind::LParen) => true,

        // Parameters (&Parameter)
        Some(TokenKind::Ampersand) => true,

        // CASE expression - check via keyword since it's IDENT token
        _ => p.at_keyword("CASE") || p.at_keyword("ВЫБОР"),
    }
}

/// Check if current position is a recovery point for list parsing.
///
/// Recovery points are tokens where we should stop parsing the current element
/// and either continue to the next element or exit the list entirely.
///
/// # Parameters
///
/// - `recovery_set`: TokenSet of delimiter tokens (Comma, RParen, Semicolon, etc.)
///
/// # Returns
///
/// `true` if at a recovery point (stop current element), `false` otherwise
fn is_recovery_point(p: &Parser, recovery_set: &crate::token_set::TokenSet) -> bool {
    // Check if current token is in the recovery set
    if let Some(kind) = p.current() {
        if recovery_set.contains(kind) {
            return true;
        }
    }

    // Check for clause keywords (FROM, WHERE, etc.)
    // These are always recovery points regardless of recovery_set
    if super::select::is_clause_keyword(p) {
        return true;
    }

    // EOF is also a recovery point
    p.at_end()
}

/// Recover to next delimiter by consuming unexpected tokens.
///
/// Used when we encounter tokens that shouldn't be there (e.g., КАК inside function arguments).
/// Consumes all tokens until we hit a delimiter (comma, rparen, semicolon) or clause keyword.
///
/// **Important:** Tracks parenthesis balance to handle nested function calls like:
/// `ВЫРАЗИТЬ(поле КАК СТРОКА(200))` - must consume until outer `)`, not inner one.
///
/// # Example
///
/// ```ignore
/// // After parsing "поле" in ВЫРАЗИТЬ(поле КАК СТРОКА(200))
/// // Current position: КАК
/// recover_to_delimiter(p);  // Consumes: КАК СТРОКА(200)
/// // Current position: ) (outer rparen)
/// ```
fn recover_to_delimiter(p: &mut Parser) {
    let err = p.start();
    let mut paren_depth = 0i32; // Track nested parentheses

    loop {
        p.check_iteration_limit(); // Prevent infinite loops

        // Track parenthesis nesting
        if p.at(TokenKind::LParen) {
            paren_depth += 1;
            p.bump();
            continue;
        }

        if p.at(TokenKind::RParen) {
            if paren_depth > 0 {
                // This is a closing paren for a nested call - consume it
                paren_depth -= 1;
                p.bump();
                continue;
            } else {
                // This is the closing paren for our function - stop here
                break;
            }
        }

        // Stop at top-level delimiters (when not inside nested parens)
        if paren_depth == 0 {
            if p.at(TokenKind::Comma) || p.at(TokenKind::Semicolon) {
                break;
            }

            // Stop at clause keywords (FROM, WHERE, etc.)
            if super::select::is_clause_keyword(p) {
                break;
            }
        }

        // Stop at EOF
        if p.at_end() {
            break;
        }

        // Consume one token
        p.bump();
    }

    err.complete(p, NodeKind::Error);
}

/// Parse a delimited list of elements with error recovery.
///
/// Generic list parser that handles:
/// - Empty elements (e.g., `a, , b` or `, b`)
/// - Missing elements after delimiter
/// - Recovery at clause keywords and other delimiters
///
/// # Parameters
///
/// - `p`: Parser instance
/// - `delimiter`: Token that separates list elements (e.g., `Comma`)
/// - `recovery_set`: Tokens where parsing should stop (e.g., `RParen`, `Semicolon`)
/// - `is_item_start`: Function to check if current position can start an item
/// - `parse_item`: Closure to parse a single list item
///
/// # Behavior
///
/// 1. Parses first element (mandatory)
/// 2. Loop:
///    - Check for recovery points → break
///    - If no delimiter → break (end of list)
///    - Consume delimiter
///    - Check for empty element (delimiter followed by delimiter or recovery point)
///      → Create ERROR node and continue
///    - Parse next element
///    - Check iteration limit to prevent infinite loops
///
/// # Example
///
/// ```ignore
/// // Parse function arguments: func(a, , c)
/// parse_delimited_list(
///     p,
///     TokenKind::Comma,
///     &LIST_RECOVERY,
///     is_expression_start,
///     |p| expression(p),
/// );
/// ```
pub(super) fn parse_delimited_list<F>(
    p: &mut Parser,
    delimiter: TokenKind,
    recovery_set: &crate::token_set::TokenSet,
    is_item_start: fn(&Parser) -> bool,
    mut parse_item: F,
) where
    F: FnMut(&mut Parser),
{
    // Parse first element (mandatory - caller ensures at least one element)
    parse_item(p);

    loop {
        p.skip_trivia();

        // ERROR RECOVERY: Check if we're at a recovery point
        // (clause keyword, closing delimiter, etc.)
        if is_recovery_point(p, recovery_set) {
            break; // Stop parsing list
        }

        // Check for delimiter (comma, etc.)
        if !p.eat(delimiter) {
            break; // No more elements
        }

        p.check_iteration_limit(); // Prevent infinite loops
        p.skip_trivia();

        // ERROR RECOVERY: Empty element after delimiter
        // Examples: "a, , b" or "func(1, , 3)" or trailing delimiter "a, b,"
        //
        // Check if next token is:
        // 1. Another delimiter (e.g., `,,`)
        // 2. A recovery point (e.g., `)` in `func(1, 2,)`)
        // 3. NOT a valid item start
        if p.at(delimiter) || is_recovery_point(p, recovery_set) || !is_item_start(p) {
            // Create ERROR node for missing element
            let err = p.start();
            err.complete(p, NodeKind::Error);

            // If it was just another delimiter, continue to next iteration
            // Otherwise (recovery point or invalid token), break
            if !p.at(delimiter) {
                break;
            }
            continue;
        }

        // Parse next element
        parse_item(p);
    }
}

/// Entry point for logical expressions (used in WHERE, HAVING clauses)
///
/// Grammar: `logicalExpression: predicate ((AND | OR) predicate)*`
pub fn logical_expression(p: &mut Parser) {
    logical_or_expr(p);
}

/// Entry point for general expressions (used in SELECT fields, etc.)
///
/// Grammar: `expression := logicalExpression`
///
/// Currently identical to `logical_expression`. Slice 12 may merge the
/// two entries; Slice 10a preserves the split for scope discipline (the
/// 14+ call sites in `select.rs` are Slice 7/8/11 territory).
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
        comparison_expr_legacy(p);
    }
}

/// Parse additive expression (+ and -)
///
/// Grammar: `additiveExpression: multiplicativeExpression ((PLUS | MINUS) multiplicativeExpression)*`
fn additive_expr(p: &mut Parser) {
    let m = p.start();

    multiplicative_expr(p);

    loop {
        p.skip_trivia(); // CRITICAL: Skip trivia BEFORE checking for operator!
        if !matches!(p.current(), Some(TokenKind::Plus) | Some(TokenKind::Minus)) {
            break;
        }
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

    loop {
        p.skip_trivia(); // CRITICAL: Skip trivia BEFORE checking for operator!
        if !matches!(
            p.current(),
            Some(TokenKind::Star) | Some(TokenKind::Slash) | Some(TokenKind::Percent)
        ) {
            break;
        }
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
/// Grammar: `primaryExpression: literal | column | functionCall | parameter | LPAREN expression RPAREN | caseExpression | STAR`
///
/// NOTE: `STAR` is included for special syntax like `COUNT(*)` in SDBL.
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

        // Special case: Star token for COUNT(*) syntax
        // In SDBL, asterisk can appear as a standalone argument in aggregate functions
        Some(TokenKind::Star) => {
            let m = p.start();
            p.bump(); // Consume Star
            m.complete(p, NodeKind::SdblLiteral);
        }

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

/// Parse parenthesized expression, tuple, or subquery
///
/// Grammar:
/// ```text
/// LPAREN (subquery | tupleExpr | expression) RPAREN
/// tupleExpr: expression (COMMA expression)+
/// ```
///
/// Lookahead:
/// - SELECT keyword → subquery
/// - After first expression, COMMA → tuple
/// - Otherwise → parenthesized expression
///
/// Tuples are used for row-wise comparison in IN predicates:
/// `(field1, field2, field3) IN (SELECT col1, col2, col3 FROM ...)`
fn paren_or_subquery_expr(p: &mut Parser) {
    let m = p.start();

    p.bump(); // (
    p.skip_trivia();

    // Lookahead: if SELECT keyword, it's a subquery
    if p.at_keyword("SELECT") || p.at_keyword("ВЫБРАТЬ") {
        // Parse subquery
        super::select::subquery(p);
        p.skip_trivia();
        p.expect(TokenKind::RParen);
        m.complete(p, NodeKind::SdblSubqueryExpr);
    } else {
        // Parse first expression
        expression(p);
        p.skip_trivia();

        // Check for comma → tuple expression (expr1, expr2, ...)
        if p.at(TokenKind::Comma) {
            // It's a tuple - parse remaining expressions
            while p.eat(TokenKind::Comma) {
                p.check_iteration_limit();
                p.skip_trivia();

                // Handle trailing comma or empty element
                if p.at(TokenKind::RParen) || !is_expression_start(p) {
                    break;
                }

                expression(p);
                p.skip_trivia();
            }

            p.expect(TokenKind::RParen);
            m.complete(p, NodeKind::SdblTupleExpr);
        } else {
            // Single expression in parentheses
            p.expect(TokenKind::RParen);
            m.complete(p, NodeKind::SdblParenExpr);
        }
    }
}

// ============================================================================
// LEGACY (Slice 10b pending)
// ============================================================================
//
// The functions below are pre-clean-room helpers that the Slice 10a rewrite
// does NOT re-author. Slice 10b (predicates + comparison + column-or-function
// + CAST + CASE) will clean-room-rewrite them. Until then they live under
// the LEGACY banner with the `_legacy` suffix where renamed (`comparison_expr`
// → `comparison_expr_legacy`, `predicate_expr` → `predicate_expr_legacy`).
//
// Slice 10a's `not_expr` calls into `comparison_expr_legacy`; the dispatch
// from `not_expr` is the only Slice-10a → Slice-10b call boundary that this
// rename touches. All NodeKinds emitted by these helpers (SdblComparisonExpr,
// SdblInExpr, SdblInHierarchyExpr, SdblIsNullExpr, SdblBetweenExpr,
// SdblLikeExpr, SdblRefsExpr, SdblColumnRef, SdblFunctionCall, SdblType,
// SdblInlineTableFields, SdblCaseExpr, SdblWhenClause) are preserved
// bit-for-bit until Slice 10b lands.

/// Parse comparison expression
///
/// Grammar:
/// ```text
/// comparisonExpression:
///     additiveExpression ((= | <> | < | <= | > | >=) additiveExpression)?
///   | predicateExpression
/// ```
fn comparison_expr_legacy(p: &mut Parser) {
    predicate_expr_legacy(p);
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
fn predicate_expr_legacy(p: &mut Parser) {
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
                // Use LIST_RECOVERY (not EXPR_RECOVERY) because Comma is the delimiter here,
                // not a recovery point. EXPR_RECOVERY includes Comma which would cause
                // parse_delimited_list to break before consuming the comma separator.
                parse_delimited_list(
                    p,
                    TokenKind::Comma,
                    &super::LIST_RECOVERY,
                    is_expression_start,
                    expression,
                );
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
                p.check_iteration_limit(); // Prevent infinite loops
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
/// Check if identifier is CAST/ВЫРАЗИТЬ function
fn is_cast_function(p: &Parser) -> bool {
    p.at_keyword("CAST") || p.at_keyword("ВЫРАЗИТЬ")
}

/// Parse CAST type specification: СТРОКА(length), ЧИСЛО(precision, scale), etc.
///
/// Grammar: `type: STRING (LPAREN DECIMAL RPAREN)? | NUMBER (LPAREN DECIMAL (COMMA DECIMAL)? RPAREN)? | DATE | BOOLEAN | mdo`
///
/// MDO types: `Справочник.Склады`, `Документ.РеализацияТоваровУслуг`, etc.
fn parse_cast_type(p: &mut Parser) {
    let m = p.start();

    // Type can be Ident (for STRING, NUMBER, DATE, BOOLEAN) or MDO reference (Справочник.Склады)
    if p.at(TokenKind::Ident) {
        // Check if type is NUMBER/ЧИСЛО (needs special handling for 2 parameters)
        let is_number_type = p.at_keyword("NUMBER") || p.at_keyword("ЧИСЛО");

        p.bump(); // Type name (or first part of MDO type)
        p.skip_trivia();

        // Parse MDO type: Справочник.Склады, Документ.РеализацияТоваровУслуг
        // Keep consuming DOT Ident pairs until we hit something else
        while p.at(TokenKind::Dot) {
            p.check_iteration_limit();
            p.bump(); // DOT
            p.skip_trivia();

            if p.at(TokenKind::Ident) {
                p.bump(); // Next part of MDO type
                p.skip_trivia();
            } else {
                // Incomplete MDO type (e.g., "Справочник." without object name)
                let err = p.start();
                err.complete(p, NodeKind::Error);
                break;
            }
        }

        // Check for type parameters: СТРОКА(200), ЧИСЛО(15, 2)
        // Note: MDO types don't have parameters, this is only for primitive types
        if p.at(TokenKind::LParen) {
            p.bump(); // (
            p.skip_trivia();

            // First parameter (length or precision)
            if p.at(TokenKind::Decimal) {
                p.bump();
                p.skip_trivia();

                // Second parameter for NUMBER (scale)
                if is_number_type && p.eat(TokenKind::Comma) {
                    p.skip_trivia();
                    if p.at(TokenKind::Decimal) {
                        p.bump();
                        p.skip_trivia();
                    }
                }
            }

            p.expect(TokenKind::RParen);
        }
    }

    m.complete(p, NodeKind::SdblType);
}

fn column_or_function(p: &mut Parser) {
    let m = p.start();

    // Check if this is CAST/ВЫРАЗИТЬ function before consuming
    let is_cast = is_cast_function(p);

    // First identifier (mandatory)
    p.bump(); // Ident
    p.skip_trivia();

    // Check for DOT (column reference) or LPAREN (function call)
    if p.at(TokenKind::Dot) {
        // Column reference: Table.Column or MDO.Table.Column
        while p.eat(TokenKind::Dot) {
            p.skip_trivia();

            // Tabular part field list: Table.TabPart.(Field1, Field2, ...)
            // Grammar: inlineTableField: column DOT LPAREN selectedFields RPAREN
            if p.at(TokenKind::LParen) {
                inline_table_fields(p);
                break;
            }

            // ERROR RECOVERY: After DOT, only Ident is valid for column/field name
            // Whitelist approach: if NOT Ident, mark incomplete and stop
            if !p.at(TokenKind::Ident) {
                // Incomplete: operators (=, AND), punctuation (,), EOF, etc.
                let err = p.start();
                err.complete(p, NodeKind::Error);
                break;
            }

            // Check if this Ident is actually a clause keyword (FROM, WHERE, etc.)
            // Lexer returns them as Ident in some contexts
            if super::select::is_clause_keyword(p) {
                // Incomplete: "Table.\nFROM" - don't consume FROM
                let err = p.start();
                err.complete(p, NodeKind::Error);
                break;
            }

            // Consume the identifier - it's a valid field name
            p.bump(); // Ident
            p.skip_trivia();
        }
        m.complete(p, NodeKind::SdblColumnRef);
    } else if p.at(TokenKind::LParen) {
        // Function call
        p.bump(); // (
        p.skip_trivia();

        // Parse arguments (comma-separated expressions)
        // Support empty parameters like in BSL: Method(, , value)
        if !p.at(TokenKind::RParen) {
            // DISTINCT/РАЗЛИЧНЫЕ inside aggregate functions: COUNT(DISTINCT expr)
            if p.at_keyword("DISTINCT") || p.at_keyword("РАЗЛИЧНЫЕ") {
                p.bump();
                p.skip_trivia();
            }

            // First argument (might be empty)
            if is_expression_start(p) && !p.at(TokenKind::Comma) {
                expression(p);

                // Special handling for CAST/ВЫРАЗИТЬ: parse КАК type syntax
                if is_cast && (p.at_keyword("AS") || p.at_keyword("КАК")) {
                    p.skip_trivia();
                    p.bump(); // AS/КАК
                    p.skip_trivia();
                    parse_cast_type(p);
                    p.skip_trivia();
                } else {
                    // ERROR RECOVERY: After expression, consume unexpected tokens
                    // Example: func(value AND ...) - after "value", consume "AND"
                    p.skip_trivia();
                    if !p.at(TokenKind::Comma) && !p.at(TokenKind::RParen) {
                        recover_to_delimiter(p);
                    }
                }
            } else if p.at(TokenKind::Comma) {
                // Empty first argument: func(, value) - create ERROR node
                let err = p.start();
                err.complete(p, NodeKind::Error);
            }

            // Parse remaining arguments with error recovery
            while p.eat(TokenKind::Comma) {
                p.check_iteration_limit();
                p.skip_trivia();

                // ERROR RECOVERY: Empty element or invalid token
                // Examples: func(1, , 3) or func(1, 2,) or func(1, FROM ...)
                if p.at(TokenKind::Comma) || p.at(TokenKind::RParen) || !is_expression_start(p) {
                    // Create ERROR node for missing/invalid argument
                    let err = p.start();
                    err.complete(p, NodeKind::Error);

                    // If next token is comma, continue to next argument
                    // Otherwise (RParen or invalid), break
                    if !p.at(TokenKind::Comma) {
                        break;
                    }
                    continue;
                }

                expression(p);

                // ERROR RECOVERY: After each argument expression, check for unexpected tokens
                p.skip_trivia();
                if !p.at(TokenKind::Comma) && !p.at(TokenKind::RParen) {
                    recover_to_delimiter(p);
                }
            }
        }

        p.skip_trivia();
        p.expect(TokenKind::RParen);

        // After closing paren, check for member access on function result
        // Example: ВЫРАЗИТЬ(field КАК Справочник.Склады).Родитель.Наименование
        // This is common in SDBL when accessing fields of CAST result
        p.skip_trivia();
        while p.at(TokenKind::Dot) {
            p.check_iteration_limit();
            p.bump(); // DOT
            p.skip_trivia();

            if p.at(TokenKind::Ident) {
                // Check if this is actually a clause keyword (shouldn't be consumed as field)
                if super::select::is_clause_keyword(p) {
                    // Incomplete: "CAST(...).\nFROM" - don't consume FROM
                    let err = p.start();
                    err.complete(p, NodeKind::Error);
                    break;
                }

                p.bump(); // Field name
                p.skip_trivia();
            } else {
                // Incomplete: "CAST(...)." without field name
                let err = p.start();
                err.complete(p, NodeKind::Error);
                break;
            }
        }

        m.complete(p, NodeKind::SdblFunctionCall);
    } else {
        // Simple column reference (no DOT, no LPAREN)
        m.complete(p, NodeKind::SdblColumnRef);
    }
}

/// Parse inline table field list: `.(Field1, Field2, ...)`
///
/// Grammar: `inlineTableField: column DOT LPAREN selectedFields RPAREN`
///
/// Used for selecting multiple fields from a tabular part:
/// `Table.TabularPart.(Field1, Field2, Ref)`
fn inline_table_fields(p: &mut Parser) {
    let m = p.start();

    p.bump(); // LParen
    p.skip_trivia();

    super::select::selected_fields(p);

    p.skip_trivia();
    p.expect(TokenKind::RParen);

    m.complete(p, NodeKind::SdblInlineTableFields);
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
