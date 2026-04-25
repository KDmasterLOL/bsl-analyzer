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
//! Slice 10a — clean-room (complete, landed with C3 2026-04-25):
//! expression backbone (atoms + operator precedence chain + parens /
//! tuple / subquery). Authored from
//! `docs/legal/sdbl-expressions-mini-spec.md` and the local 1C ITS
//! pubqlang dump (chapters 22 §WHERE / precedence ladder, 40 §Литералы
//! and §Арифметические операции, 60 §Передача параметров в запрос).
//! The clean-room claim follows the same convention as Slices 1, 2,
//! 6, 7, 8 (see their attestations under `docs/legal/sdbl-clean-room-
//! slice{1,2,6,7,8}.md`): **independent derivation from the cited
//! sources plus the project's local compatibility constraints, not
//! textual novelty**. Where the resulting event-parser shape closely
//! matches the pre-clean-room implementation, that is the natural
//! convergence on a single ITS-derived grammar shape, not consultation
//! of pre-C1 bodies during authorship. See
//! `docs/legal/sdbl-clean-room-slice10a.md` for the attestation.
//!
//! Slice 10b — clean-room (complete, landed with C3 2026-04-25):
//! predicates, comparison, column-or-function dispatch, CAST type
//! spec, CASE expressions. The 8 functions under the
//! `CLEAN-ROOM Slice 10b` banner — `comparison_expr`,
//! `predicate_expr`, `column_or_function`, `inline_table_fields`,
//! `is_cast_function`, `parse_cast_type`, `case_expr`,
//! `when_clause` — were re-authored in C2 from ITS pubqlang
//! chapters 21, 22, 23, 27, 32, 40 and the C0a-extended
//! `docs/legal/sdbl-expressions-mini-spec.md`; each function body
//! carries an ITS-cited or `// local: …` per-function provenance
//! comment. The two `_legacy` suffixes used during the Slice 10a
//! authorship period (`comparison_expr_legacy`,
//! `predicate_expr_legacy`) were retired in C1; `not_expr` now
//! calls `comparison_expr` directly. The C2 commit also landed
//! the `column_or_function` clause-keyword recovery fix
//! (codex Round-1 finding 2). See
//! `docs/legal/sdbl-clean-room-slice10b.md` for the attestation.

use crate::event::NodeKind;
use crate::parser::Parser;
use lexer::TokenKind;

// ============================================================================
// CLEAN-ROOM Slice 10a — expression backbone
// ============================================================================
//
// Authored from `docs/legal/sdbl-expressions-mini-spec.md` (the C0a
// clean-room reference; see its §Non-consultation statement and §ITS
// coverage verification) and the local 1C ITS pubqlang dump
// (chapters 22, 40, 60). Per-function provenance comments cite the
// specific ITS chapter or mini-spec section that authorises each
// function's grammar shape. The clean-room claim is independent
// derivation from the cited sources, not textual novelty — see the
// module-level Provenance docstring above for the project-wide
// convention.
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
// Slice 10a's `not_expr` calls into `comparison_expr` (Slice 10b
// territory, defined under the `CLEAN-ROOM Slice 10b` banner below)
// — that is the only Slice-10a → Slice-10b dispatch boundary in
// this file.

// ============================================================================
// Helper Functions for Error Recovery
// ============================================================================

/// Check if current token can start an expression.
///
/// Used for error recovery in list parsing — detects empty elements and
/// clause keywords that should not be consumed as expressions.
///
/// Accept set: `Decimal`, `Float`, `String`, `KwTrue`, `KwFalse`,
/// `KwUndefined`, non-clause-keyword `Ident`, `Plus`, `Minus`, `KwNot`,
/// `Star`, `LParen`, `Ampersand`, plus the keyword probes
/// `at_keyword("CASE" | "ВЫБОР" | "NULL")`.
pub(super) fn is_expression_start(p: &Parser) -> bool {
    // Authored from sdbl-expressions-mini-spec.md §Recovery — accept-set
    // for list-parsing item detection, and §Atoms accept-lead tokens.
    // ITS pubqlang/40 §Литералы covers numeric, string, boolean
    // (Истина/Ложь), Null, Неопределено; pubqlang/60 §Передача параметров
    // в запрос covers the `&Identifier` parameter prefix; pubqlang/22
    // §Условие отбора covers the `НЕ` (NOT) prefix operator. Note: bare
    // `NULL` arrives as `Ident` (per `sdbl_token_converter.rs:57`,
    // `LitNull → TokenKind::Ident`) and must be detected via
    // `at_keyword("NULL")`; the historical `Some(TokenKind::KwNull)` arm
    // was unreachable dead code and is dropped here.
    match p.current() {
        Some(TokenKind::Decimal)
        | Some(TokenKind::Float)
        | Some(TokenKind::String)
        | Some(TokenKind::KwTrue)
        | Some(TokenKind::KwFalse)
        | Some(TokenKind::KwUndefined) => true,

        Some(TokenKind::Ident) => {
            // Generic identifier may be a column ref / function call /
            // bare NULL literal. Reject only when it's a clause keyword
            // (FROM, WHERE, GROUP, ORDER, …) — those terminate the
            // expression position and must not be consumed.
            !super::select::is_clause_keyword(p)
        }

        Some(TokenKind::Plus)
        | Some(TokenKind::Minus)
        | Some(TokenKind::KwNot)
        | Some(TokenKind::Star) => true,

        Some(TokenKind::LParen) => true,

        Some(TokenKind::Ampersand) => true,

        // CASE / ВЫБОР and bare NULL all arrive as Ident from the
        // converter; the Ident arm above already accepts them as
        // expression starts via the !is_clause_keyword guard. The
        // `_` fallthrough below is **unreachable** under the current
        // `Parser::at_keyword` API (which only returns true when the
        // current token is Ident — and the Ident arm has already
        // matched). It is kept for textual symmetry with
        // `primary_expr`'s keyword-probe pattern, not as a defence
        // against future converter changes; if such a change ever
        // routes one of these keywords to a non-Ident TokenKind,
        // both this function and `primary_expr` must grow an
        // explicit `Some(TokenKind::…)` arm.
        _ => p.at_keyword("CASE") || p.at_keyword("ВЫБОР") || p.at_keyword("NULL"),
    }
}

/// Check if current position is a recovery point for list parsing.
///
/// Recovery points are positions where the list parser must stop the
/// current element and either continue to the next element or exit the
/// list entirely. They are: any token in the caller's `recovery_set`,
/// any clause keyword (FROM / WHERE / …), or end-of-input.
fn is_recovery_point(p: &Parser, recovery_set: &crate::token_set::TokenSet) -> bool {
    // local: event-parser predicate; mini-spec §Recovery — list-parsing
    // recovery points. Three independent triggers, OR-combined.
    if let Some(kind) = p.current() {
        if recovery_set.contains(kind) {
            return true;
        }
    }

    if super::select::is_clause_keyword(p) {
        return true;
    }

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
    // local: paren-depth tracking recovery; mini-spec §Recovery —
    // nested-paren recovery preserves the inner `(...)` depth so that
    // `ВЫРАЗИТЬ(поле КАК СТРОКА(200))` recovery walks to the outer `)`,
    // not the inner one. Wraps the consumed run in one Error marker.
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
    // local: generic delimited-list helper; mini-spec §Recovery —
    // empty-element recovery emits Error for missing items between
    // delimiters (e.g. `a, , b`) and on trailing delimiters
    // (e.g. `a, b,`). Used by SELECT field list, FROM source list,
    // IN value list, INDEX BY items.

    // Parse first element (mandatory — caller ensures at least one
    // element).
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

/// Entry point for logical expressions (used in WHERE, HAVING, ON
/// clause bodies).
///
/// Grammar: `logicalExpression := logicalOrExpression`
pub fn logical_expression(p: &mut Parser) {
    // ITS pubqlang/22 §Условие отбора — the WHERE / HAVING / JOIN ON
    // clause body is a logical expression. Delegates to logical_or_expr
    // which sits at the bottom of the precedence ladder per pubqlang/22
    // («сначала вычисляются простые логические выражения, затем
    // операции НЕ, затем операции И, в последнюю очередь – операции
    // ИЛИ»).
    logical_or_expr(p);
}

/// Entry point for general expressions (used in SELECT fields, ORDER BY
/// items, GROUP BY items, function call args, etc.).
///
/// Grammar: `expression := logicalExpression`
///
/// Currently identical to `logical_expression`. Slice 12 may merge the
/// two entries; Slice 10a preserves the split for scope discipline (the
/// 14+ call sites in `select.rs` are Slice 7/8/11 territory).
pub fn expression(p: &mut Parser) {
    // Mini-spec §Expression entry points — currently identical to
    // logical_expression; kept distinct for the Slice 12 future split
    // between general-expression and logical-expression contexts.
    logical_or_expr(p);
}

/// Parse OR expression — loosest binding in the logical chain.
///
/// Grammar: `logicalOrExpression := logicalAndExpression (OR
/// logicalAndExpression)*`. Emits a single FLAT `SdblLogicalOrExpr`
/// wrapper containing all operands and `KwOr` operator tokens (HIR
/// `lower_binary_expr` collects the flat children list and detects
/// the operator from `node.text()`).
fn logical_or_expr(p: &mut Parser) {
    // ITS pubqlang/22 — OR is the loosest-binding logical operator
    // («в последнюю очередь – операции ИЛИ»). Mini-spec §AST-shape
    // invariant #1: FLAT wrapper, one marker before the loop, one
    // m.complete after; chained `a OR b OR c` produces a single
    // SdblLogicalOrExpr with 3 operand children + 2 KwOr tokens.
    // Mini-spec §Trivia handling: skip_trivia BEFORE the operator
    // probe so `a\nOR\nb` is recognised.
    let m = p.start();

    logical_and_expr(p);

    loop {
        p.skip_trivia();
        if p.at(TokenKind::KwOr) {
            p.check_iteration_limit();
            p.bump(); // OR / ИЛИ
            p.skip_trivia();
            logical_and_expr(p);
        } else {
            break;
        }
    }

    m.complete(p, NodeKind::SdblLogicalOrExpr);
}

/// Parse AND expression — tighter than OR, looser than NOT.
///
/// Grammar: `logicalAndExpression := notExpression (AND notExpression)*`.
/// Emits a single FLAT `SdblLogicalAndExpr` wrapper.
fn logical_and_expr(p: &mut Parser) {
    // ITS pubqlang/22 — AND binds tighter than OR, looser than NOT
    // («затем операции НЕ, затем операции И»). Mini-spec §AST-shape
    // invariant #1: FLAT wrapper. Mini-spec §Trivia handling.
    let m = p.start();

    not_expr(p);

    loop {
        p.skip_trivia();
        if p.at(TokenKind::KwAnd) {
            p.check_iteration_limit();
            p.bump(); // AND / И
            p.skip_trivia();
            not_expr(p);
        } else {
            break;
        }
    }

    m.complete(p, NodeKind::SdblLogicalAndExpr);
}

/// Parse NOT expression — tightest-binding logical operator.
///
/// Grammar: `notExpression := NOT* comparisonExpression`. Right-
/// recursive for multi-NOT inputs (`НЕ НЕ A` → nested
/// `SdblNotExpr( SdblNotExpr( A ) )`).
fn not_expr(p: &mut Parser) {
    // ITS pubqlang/22 — NOT binds tightest among logical ops («сначала
    // вычисляются простые логические выражения, затем операции НЕ»).
    // Mini-spec §Operator-binding pin item 1: right-recursive multi-
    // NOT produces nested SdblNotExpr wrappers. The non-NOT branch
    // delegates to Slice 10b legacy comparison/predicate dispatch
    // (rewritten by Slice 10b at C2).
    if p.at(TokenKind::KwNot) {
        let m = p.start();
        p.bump(); // NOT / НЕ
        p.skip_trivia();
        not_expr(p); // Right-recursive for chained `НЕ НЕ A`.
        m.complete(p, NodeKind::SdblNotExpr);
    } else {
        comparison_expr(p);
    }
}

/// Parse additive expression — `+` and `-` operators.
///
/// Grammar: `additiveExpression := multiplicativeExpression ((PLUS |
/// MINUS) multiplicativeExpression)*`. Emits a single FLAT
/// `SdblAdditiveExpr` wrapper for chained additions (mini-spec
/// §AST-shape #1).
fn additive_expr(p: &mut Parser) {
    // ITS pubqlang/40 §Арифметические операции — «+, -, /, *»; +/-
    // operators bind looser than */÷. Mini-spec §AST-shape #1: FLAT
    // wrapper opens before the loop and closes after; chained
    // `a + b + c` produces a single SdblAdditiveExpr with 3 operands
    // and 2 PLUS tokens. Mini-spec §Trivia handling (CRITICAL): the
    // p.skip_trivia() call MUST precede the operator probe so
    // `a\n+\nb` is recognised.
    let m = p.start();

    multiplicative_expr(p);

    loop {
        p.skip_trivia(); // CRITICAL: skip_trivia BEFORE the operator probe.
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

/// Parse multiplicative expression — `*`, `/`, `%`.
///
/// Grammar: `multiplicativeExpression := unaryExpression ((STAR | SLASH
/// | PERCENT) unaryExpression)*`. Emits a single FLAT
/// `SdblMultiplicativeExpr` wrapper.
///
/// **Note on `%`:** ITS pubqlang/40 explicitly states «Операция
/// получения остатка % в языке запросов не поддерживается» — modulo
/// is NOT documented as a supported SDBL operator. The current parser
/// (and this clean-room rewrite) accept `Percent` as a *local
/// IDE-recovery allowance* so user input containing `%` produces a
/// recoverable tree rather than an immediate parse error. This is
/// recorded in `sdbl-expressions-mini-spec.md` §IDE-recovery
/// allowances and §ITS coverage verification as the single non-ITS
/// extension in Slice 10a.
fn multiplicative_expr(p: &mut Parser) {
    // ITS pubqlang/40 §Арифметические операции — *, /. The Percent
    // arm is a local IDE allowance, NOT ITS-spec'd. Mini-spec
    // §AST-shape #1 (FLAT wrapper) + §Trivia handling.
    let m = p.start();

    unary_expr(p);

    loop {
        p.skip_trivia(); // CRITICAL: skip_trivia BEFORE the operator probe.
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

/// Parse unary expression — `+`, `-`, `NOT` prefix.
///
/// Grammar: `unaryExpression := (PLUS | MINUS | KwNot)?
/// primaryExpression`. Right-recursive for chained unary operators
/// (`- - А` → nested `SdblUnaryExpr( SdblUnaryExpr( А ) )`).
fn unary_expr(p: &mut Parser) {
    // Mini-spec §Operator precedence — unary +/-/NOT binds tighter
    // than the multiplicative chain. Mini-spec §Operator-binding pin
    // item 2: right-recursive nesting via `unary_expr(p)` in the
    // recursive arm. ITS pubqlang/40 covers unary plus / minus
    // through the arithmetic-operator inventory; KwNot prefix on
    // arithmetic operands is a local extension.
    if matches!(
        p.current(),
        Some(TokenKind::Plus) | Some(TokenKind::Minus) | Some(TokenKind::KwNot)
    ) {
        let m = p.start();
        p.bump(); // unary operator
        p.skip_trivia();
        unary_expr(p); // Right-recursive for chained `- - А`.
        m.complete(p, NodeKind::SdblUnaryExpr);
    } else {
        primary_expr(p);
    }
}

/// Parse primary expression — atoms.
///
/// Grammar (mini-spec §Atoms primary dispatch):
/// ```text
/// primaryExpression
///   := caseExpression                  (at_keyword "CASE" / "ВЫБОР")
///    | nullLiteral                     (at_keyword "NULL")
///    | parenOrSubqueryExpression       (LPAREN)
///    | parameterExpression             (Ampersand + Ident)
///    | literalExpression               (Decimal | Float | String | KwTrue | KwFalse | KwUndefined)
///    | starLiteral                     (Star — emits SdblLiteral)
///    | columnOrFunctionCall            (generic Ident, NOT a clause keyword)
///    | error-fallback (SdblError)      (anything else)
/// ```
///
/// Order is significant: keyword probes (`CASE`, `ВЫБОР`, `NULL`)
/// MUST run before generic `Ident` dispatch, because all three arrive
/// as `TokenKind::Ident` from `sdbl_token_converter` and would
/// otherwise be consumed as column references.
fn primary_expr(p: &mut Parser) {
    // ITS pubqlang/40 §Литералы — literal forms include numeric,
    // string, boolean (Истина/Ложь), Null, Неопределено; pubqlang/40
    // also documents ВЫБОР (CASE) and ВЫРАЗИТЬ (CAST) operations.
    // ITS pubqlang/60 §Передача параметров в запрос — `&Identifier`
    // parameter prefix. Mini-spec §Atoms primary dispatch — keyword
    // probes (CASE/ВЫБОР, bare NULL) run BEFORE the generic Ident
    // arm because the converter delivers these spellings as Ident
    // (per `sdbl_token_converter.rs:57` for NULL); without the
    // pre-Ident probe bare `NULL` would be consumed as an
    // SdblColumnRef instead of an SdblLiteral.
    if p.at_keyword("CASE") || p.at_keyword("ВЫБОР") {
        case_expr(p);
        return;
    }

    if p.at_keyword("NULL") {
        // Bare NULL literal (e.g. `SELECT NULL`, `WHERE x = NULL`).
        // Mini-spec §Atoms literal grammar — nullLiteral emits
        // SdblLiteral wrapping the single Ident token.
        let m = p.start();
        p.bump(); // Consume the NULL Ident token.
        m.complete(p, NodeKind::SdblLiteral);
        return;
    }

    match p.current() {
        Some(TokenKind::LParen) => paren_or_subquery_expr(p),
        Some(TokenKind::Decimal) | Some(TokenKind::Float) => literal_expr(p),
        Some(TokenKind::String) => literal_expr(p),
        Some(TokenKind::KwTrue) | Some(TokenKind::KwFalse) => literal_expr(p),
        Some(TokenKind::KwUndefined) => literal_expr(p),
        Some(TokenKind::Ampersand) => parameter_expr(p),

        // Star atom for COUNT(*) syntax — emits SdblLiteral wrapping
        // the single Star token (mini-spec §Atoms — Star).
        Some(TokenKind::Star) => {
            let m = p.start();
            p.bump(); // Consume Star.
            m.complete(p, NodeKind::SdblLiteral);
        }

        // Generic Ident (column ref / function call / inline tabular
        // fields). Slice 10b owns the body via `column_or_function`.
        Some(TokenKind::Ident) => column_or_function(p),

        _ => {
            // Error recovery — unexpected token at the head of an
            // expression position. Emits SdblError (NOT generic
            // Error) so downstream consumers can filter on the kind.
            let m = p.start();
            p.error();
            m.complete(p, NodeKind::SdblError);
        }
    }
}

/// Parse a literal expression — numeric, string, boolean, or
/// undefined/null forms.
///
/// Grammar (mini-spec §Atoms literal grammar):
/// ```text
/// literalExpression
///   := numericLiteral                  (Decimal | Float)
///    | stringLiteralOrMulti            (String, possibly chained)
///    | booleanLiteral                  (KwTrue | KwFalse)
///    | undefinedLiteral                (KwUndefined)
/// ```
fn literal_expr(p: &mut Parser) {
    // ITS pubqlang/40 §Литералы — «число, строка (в кавычках),
    // булево (значения Истина и Ложь), Null, Неопределено». Note that
    // the bare NULL literal is dispatched by `primary_expr` directly
    // (the keyword probe runs before this function); this dispatcher
    // handles only the literals delivered as dedicated TokenKinds.
    if p.at(TokenKind::String) {
        string_literal_or_multi(p);
    } else {
        let m = p.start();
        p.bump();
        m.complete(p, NodeKind::SdblLiteral);
    }
}

/// Parse a single string literal or a multi-string concatenation.
///
/// Grammar (mini-spec §Atoms — string literal multi-part IDE
/// recovery): `stringLiteralOrMulti := String+`. Emits
/// `SdblMultiString` for 2+ consecutive `String` tokens (local IDE
/// allowance for multi-line BSL query strings); emits `SdblLiteral`
/// wrapping a single `String` token otherwise.
///
/// The single-string `SdblLiteral` keeps `String` as a *direct token
/// child*; the multi-line-string diagnostic at
/// `crates/ide-diagnostics/src/handlers/multiline_string_in_query.rs`
/// scans for a `String` direct token of `SdblLiteral` to detect
/// embedded newlines.
fn string_literal_or_multi(p: &mut Parser) {
    // Mini-spec §Atoms string literal — IDE-recovery allowance for
    // multi-line BSL queries split across consecutive String tokens.
    // Not ITS-spec'd; documented under §IDE-recovery allowances.
    let m = p.start();

    p.bump(); // First String token.

    let mut count = 1;
    while p.at(TokenKind::String) {
        p.bump();
        count += 1;
    }

    if count > 1 {
        m.complete(p, NodeKind::SdblMultiString);
    } else {
        m.complete(p, NodeKind::SdblLiteral);
    }
}

/// Parse a parameter expression — `&Identifier`.
///
/// Grammar (mini-spec §Atoms — parameter no-trivia contract):
/// `parameterExpression := Ampersand Ident?`. The `Ident` is
/// optional to preserve the bare-`&` IDE-recovery allowance (Slice 8
/// attestation §Preserved-behaviour #7 already locks the same shape
/// for the FROM-context production).
fn parameter_expr(p: &mut Parser) {
    // ITS pubqlang/60 §Передача параметров в запрос — concrete
    // examples `&ЧастьНаименования`, `&ДатаНачала`. Mini-spec
    // §Atoms parameter no-trivia contract: NO `p.skip_trivia()`
    // between the Ampersand bump and the Ident bump — the lexer
    // is responsible for whether `& T` (with whitespace) yields one
    // or two tokens; the parser-side guarantee is "no skip_trivia
    // between the two bumps".
    let m = p.start();
    p.bump(); // Consume Ampersand.

    // Identifier guarded by p.at, NOT p.expect — bare `&` at EOF
    // or before a clause keyword still completes SdblParameter for
    // IDE recovery (mini-spec §AST-shape invariant #3).
    if p.at(TokenKind::Ident) {
        p.bump();
    }

    m.complete(p, NodeKind::SdblParameter);
}

/// Parse a parenthesised expression, tuple, or subquery.
///
/// Grammar (mini-spec §Atoms paren dispatch):
/// ```text
/// parenOrSubqueryExpression
///   := '(' (subqueryHead | expressionTail) ')'
/// subqueryHead    := (SELECT | ВЫБРАТЬ) ...    ↦ SdblSubqueryExpr
/// expressionTail
///   := expression                              ↦ SdblParenExpr
///    | expression (COMMA expression)+          ↦ SdblTupleExpr
/// ```
///
/// **SELECT-only lookahead:** the subquery branch is entered only on
/// the post-`(` keyword `SELECT` / `ВЫБРАТЬ`. Every other input —
/// including `(&T)`, `(1)`, `(1, 2)`, `(T + 1)` — routes to the
/// expression branch. This is the **opposite** routing decision from
/// the FROM-context `data_source` (Slice 8), where any `(` routes to
/// subquery-source. Tests at
/// `crates/parser/tests/sdbl_parser_tests.rs:1435-1467` lock the
/// distinction.
fn paren_or_subquery_expr(p: &mut Parser) {
    // ITS pubqlang/40 demonstrates parenthesised expressions and
    // tuple-style usage in IN predicates («оператор МЕЖДУ … вместе с
    // границами диапазона»; concrete examples of `(field1, field2)`
    // tuples appear in the IN-predicate examples). Mini-spec §Atoms
    // paren dispatch — SELECT-keyword lookahead routes to subquery;
    // otherwise expression(s) → SdblParenExpr (single) or
    // SdblTupleExpr (2+).
    let m = p.start();

    p.bump(); // Consume LParen.
    p.skip_trivia();

    if p.at_keyword("SELECT") || p.at_keyword("ВЫБРАТЬ") {
        // Subquery in expression context (e.g. `IN (SELECT ...)`).
        super::select::subquery(p);
        p.skip_trivia();
        p.expect(TokenKind::RParen);
        m.complete(p, NodeKind::SdblSubqueryExpr);
    } else {
        // Parse the first expression — single-paren or tuple.
        expression(p);
        p.skip_trivia();

        if p.at(TokenKind::Comma) {
            // Tuple — 2+ comma-separated expressions for row-wise
            // comparison in IN predicates: `(a, b) IN (SELECT ...)`.
            while p.eat(TokenKind::Comma) {
                p.check_iteration_limit();
                p.skip_trivia();

                // Trailing-comma / empty-middle-element recovery:
                // bail out without parsing a missing operand to
                // avoid producing spurious sub-trees.
                if p.at(TokenKind::RParen) || !is_expression_start(p) {
                    break;
                }

                expression(p);
                p.skip_trivia();
            }

            p.expect(TokenKind::RParen);
            m.complete(p, NodeKind::SdblTupleExpr);
        } else {
            // Single parenthesised expression.
            p.expect(TokenKind::RParen);
            m.complete(p, NodeKind::SdblParenExpr);
        }
    }
}

// ============================================================================
// CLEAN-ROOM Slice 10b — predicates, comparison, function calls, CAST, CASE
// ============================================================================
//
// The 8 functions below — `comparison_expr`, `predicate_expr`,
// `is_cast_function`, `parse_cast_type`, `column_or_function`,
// `inline_table_fields`, `case_expr`, `when_clause` — were
// authored as a clean-room re-derivation against ITS pubqlang
// chapters 21, 22, 23, 27, 32, 40 (via the local dump at
// `/home/itrous/src/tools_migration/its/dump/`) and the
// C0a-extended `docs/legal/sdbl-expressions-mini-spec.md`. C1
// performed the pre-refactor renames
// (`comparison_expr_legacy` → `comparison_expr`,
// `predicate_expr_legacy` → `predicate_expr`) and replaced the
// previous LEGACY banner with this clean-room banner; C2
// re-authored each function body from the cited sources, attached
// per-function ITS / mini-spec provenance comments, and landed
// the `column_or_function` clause-keyword recovery fix
// (codex Round-1 finding 2). See
// `docs/legal/sdbl-clean-room-slice10b.md` for the attestation.
//
// Slice 10a's `not_expr` calls `comparison_expr` directly — that
// is the only Slice-10a → Slice-10b dispatch boundary in this
// file. All 13 NodeKinds emitted by these functions
// (SdblComparisonExpr, SdblInExpr, SdblInHierarchyExpr,
// SdblIsNullExpr, SdblBetweenExpr, SdblLikeExpr, SdblRefsExpr,
// SdblColumnRef, SdblFunctionCall, SdblType,
// SdblInlineTableFields, SdblCaseExpr, SdblWhenClause) retain
// their pre-C1 child-attachment shapes and are locked by the C0b
// Bucket-A regression-gate tests in
// `crates/parser/tests/sdbl_parser_tests.rs` plus the C3
// spec-driven acceptance suite in
// `crates/parser/tests/sdbl_slice10b_predicates.rs`.

/// Parse comparison expression
///
/// Grammar:
/// ```text
/// comparisonExpression:
///     additiveExpression ((= | <> | < | <= | > | >=) additiveExpression)?
///   | predicateExpression
/// ```
fn comparison_expr(p: &mut Parser) {
    // ITS pubqlang/22 §Условие отбора — comparison and predicate
    // share the same precedence slot below NOT, so `comparison_expr`
    // is a 1:1 dispatcher shim to `predicate_expr` which holds the
    // 7-branch dispatch (IN / IS NULL / BETWEEN / LIKE / REFS /
    // comparison / fall-through). Mini-spec §Comparison.
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
    // ITS provenance (verified against the local pubqlang dump in
    // C2; see mini-spec §ITS coverage verification table):
    //   - BETWEEN/МЕЖДУ → pubqlang/22 §Условие отбора (листинг
    //     1.33 канonical `Дата МЕЖДУ ДАТАВРЕМЯ(...) И ДАТАВРЕМЯ(...)`);
    //   - LIKE/ПОДОБНО → pubqlang/23 §Шаблон (листинг 1.34
    //     канonical `Наименование ПОДОБНО "%Иван%"`);
    //   - IS NULL/ЕСТЬ NULL → pubqlang/27 канonical
    //     `КОГДА (Товары.Производитель) ЕСТЬ NULL ТОГДА "NULL"`;
    //   - IN HIERARCHY/В ИЕРАРХИИ → pubqlang/32 (листинг 1.51
    //     канonical `Товары.Ссылка В ИЕРАРХИИ (&ГруппаТоваров)`);
    //   - REFS/ССЫЛКА → pubqlang/40 канonical
    //     `(ОстаткиТоваров.Регистратор ССЫЛКА Документ.ПриходнаяНакладная)`.
    //
    // local: the IN value-list shape `<expr> В (<v1>, <v2>, ...)`,
    // the IN-with-subquery form, the six comparison operators
    // (`= <> < <= > >=`), and the ESCAPE/СПЕЦСИМВОЛ optional
    // clause are NOT enumerated in the dumped ITS chapters
    // (verified absent in the §ITS coverage verification rows of
    // the C0a-extended mini-spec); they are preserved as local
    // IDE-recovery allowances under mini-spec §IDE-recovery
    // allowances #10, #13, the §Predicates §SdblInExpr shape, and
    // the §Comparison section. Mini-spec §Predicates.
    let m = p.start();

    additive_expr(p);

    p.skip_trivia();

    // Optional NOT prefix consumed BEFORE probing IN / BETWEEN /
    // LIKE so the predicate node carries the NOT token as a direct
    // child of the eventual IN / BETWEEN / LIKE node. If no
    // predicate / comparison branch matches, the marker is
    // abandoned and the consumed NOT remains as a stray token —
    // mini-spec §IDE-recovery allowances #14.
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

        // local: optional ESCAPE / СПЕЦСИМВОЛ clause — NOT
        // documented in dumped ITS chapters 23 + 60; preserved as
        // a parser-accepted IDE-recovery allowance. Mini-spec
        // §IDE-recovery allowances #13.
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
    // local: predicate for CAST/ВЫРАЗИТЬ keyword pair, called from
    // `column_or_function` BEFORE the Ident bump so the resulting
    // `is_cast` flag is available for the LParen branch's КАК-type
    // recovery. Mini-spec §CAST type specification.
    p.at_keyword("CAST") || p.at_keyword("ВЫРАЗИТЬ")
}

/// Parse CAST type specification: СТРОКА(length), ЧИСЛО(precision, scale), etc.
///
/// Grammar: `type: STRING (LPAREN DECIMAL RPAREN)? | NUMBER (LPAREN DECIMAL (COMMA DECIMAL)? RPAREN)? | DATE | BOOLEAN | mdo`
///
/// MDO types: `Справочник.Склады`, `Документ.РеализацияТоваровУслуг`, etc.
fn parse_cast_type(p: &mut Parser) {
    // ITS pubqlang/40 §ВЫРАЗИТЬ — primitive type with optional
    // (size[, scale]) parameters OR MDO chain. Recognised primitive
    // types: СТРОКА/STRING, ЧИСЛО/NUMBER, ДАТА/DATE, БУЛЕВО/BOOLEAN.
    // The MDO branch is greedy `('.' Ident)*`. Mini-spec §CAST type
    // specification.
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
    // ITS pubqlang/10 + /12 — column reference and function call
    // dispatch. Bilingual aggregate-function names (СУММА/SUM,
    // КОЛИЧЕСТВО/COUNT, СРЕДНЕЕ/AVG, МАКСИМУМ/MAX, МИНИМУМ/MIN)
    // are documented across pubqlang/40 examples. CAST member
    // access (ВЫРАЗИТЬ(... КАК ...).Поле) is pubqlang/40 §ВЫРАЗИТЬ.
    // Mini-spec §Column references and function calls.
    let m = p.start();

    // CAST detection runs BEFORE the Ident bump so the resulting
    // `is_cast` flag is available for the LParen branch's КАК-type
    // recovery. Mini-spec §CAST type specification.
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

            // First argument (might be empty). The clause-keyword
            // guard is defensive — `is_expression_start` already
            // filters clause keywords on the Ident arm; the
            // explicit check makes the recovery contract textual
            // and self-documenting at the call site. Codex Round-1
            // finding 2 → C2 fix.
            if is_expression_start(p)
                && !p.at(TokenKind::Comma)
                && !super::select::is_clause_keyword(p)
            {
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

                // ERROR RECOVERY: Empty element, invalid token, or
                // clause keyword in argument position. Examples:
                // `func(1, , 3)`, `func(1, 2,)`, `func(1, FROM ...)`.
                // Codex Round-1 finding 2 → C2 fix: the explicit
                // `is_clause_keyword` check is defensive
                // (`is_expression_start` already filters clause
                // keywords on the Ident arm) and complements the
                // mid-loop break-out when the function call is
                // unterminated.
                if p.at(TokenKind::Comma)
                    || p.at(TokenKind::RParen)
                    || !is_expression_start(p)
                    || super::select::is_clause_keyword(p)
                {
                    // Create ERROR node for missing/invalid argument
                    let err = p.start();
                    err.complete(p, NodeKind::Error);

                    // If next token is comma, continue to next argument.
                    // Otherwise (RParen, invalid, or clause keyword),
                    // break out of the loop.
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

        // Codex Round-1 finding 2 → C2 fix. If the function call
        // is unterminated at a clause keyword, leave the keyword
        // for the outer parser; emit a zero-width Error so the
        // parse is marked as recovering. Without this guard,
        // `p.expect(RParen)` falls through to `Parser::error()`
        // which BUMPS the current token, consuming the clause
        // keyword as a child of `SdblFunctionCall` and breaking the
        // outer SELECT body. Regression gates:
        // `test_func_call_clause_keyword_recovery` (EN) and
        // `test_russian_func_call_clause_keyword_recovery` (RU) in
        // `crates/parser/tests/sdbl_parser_tests.rs`.
        if super::select::is_clause_keyword(p) {
            let err = p.start();
            err.complete(p, NodeKind::Error);
        } else {
            p.expect(TokenKind::RParen);
        }

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
    // local: tabular-part field-list IDE recovery; the dumped ITS
    // chapters do not directly document the `Table.TabPart.(F1, F2)`
    // shape. The dispatch boundary `column_or_function` →
    // `inline_table_fields` → `selected_fields` is the only
    // Slice-10b → Slice-7 reach. Mini-spec §Inline tabular field
    // syntax.
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
    // ITS pubqlang/40 §ВЫБОР — simple vs searched CASE, mandatory
    // END/КОНЕЦ. Canonical example: `ВЫБОР КОГДА Товары.ЭтоГруппа =
    // ИСТИНА ТОГДА "Это группа" ИНАЧЕ "Это элемент" КОНЕЦ`.
    // Child-order invariant locked by HIR
    // `crates/sdbl-hir/src/lower/expr/case_expr.rs:40-45`: the first
    // child node distinguishes simple (operand first) vs searched
    // (SdblWhenClause first). Mini-spec §CASE expressions.
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
    // ITS pubqlang/40 §ВЫБОР — WHEN/КОГДА condition THEN/ТОГДА
    // result. Direct child of SdblCaseExpr. The HIR consumer at
    // `case_expr.rs:51-89` reads `node.children()` and assumes
    // exactly two child expression nodes (condition + result).
    // Mini-spec §CASE expressions.
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
