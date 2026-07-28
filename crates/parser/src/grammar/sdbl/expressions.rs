use crate::event::NodeKind;
use crate::parser::Parser;
use lexer::TokenKind;
use parser_error::{ParseError, RecoveryKind};
use smallvec::smallvec;

pub(super) fn is_expression_start(p: &Parser) -> bool {
    match p.current() {
        Some(TokenKind::Decimal)
        | Some(TokenKind::Float)
        | Some(TokenKind::String)
        | Some(TokenKind::Date)
        | Some(TokenKind::KwTrue)
        | Some(TokenKind::KwFalse)
        | Some(TokenKind::KwUndefined) => true,

        // The word that begins the next query begins no expression. Saying
        // otherwise sends a rule that will refuse it a token it must not
        // take, and a loop that expects an expression to be consumed then
        // has nothing to consume and no reason to stop.
        Some(TokenKind::Ident) => {
            !super::select::is_clause_keyword(p) && !super::at_query_boundary(p)
        }

        Some(TokenKind::Plus)
        | Some(TokenKind::Minus)
        | Some(TokenKind::KwNot)
        | Some(TokenKind::Star) => true,

        Some(TokenKind::LParen) => true,

        Some(TokenKind::Ampersand) => true,

        _ => p.at_keyword("CASE") || p.at_keyword("ВЫБОР") || p.at_keyword("NULL"),
    }
}

pub(super) fn at_property_name(p: &Parser) -> bool {
    matches!(
        p.current(),
        Some(
            TokenKind::Ident
                | TokenKind::KwIn
                | TokenKind::KwAnd
                | TokenKind::KwOr
                | TokenKind::KwNot
                | TokenKind::KwTrue
                | TokenKind::KwFalse
                | TokenKind::KwUndefined
        )
    )
}

// =====================================================================
// CLEAN-ROOM Slice 12 — expression-level recovery
//
// None of this is the query language: the official grammar has no
// opinion on malformed input. These helpers exist so that one bad
// expression does not cost the editor the rest of the query, and each is
// justified on that ground alone.
//
// Provenance: `docs/legal/sdbl-clean-room-slice12.md`, entries A8–A9.
// =====================================================================

fn is_recovery_point(p: &Parser, recovery_set: &crate::token_set::TokenSet) -> bool {
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

fn recover_to_delimiter(p: &mut Parser) {
    let err = p.start();
    let mut consumed_any = false;
    let mut paren_depth = 0i32;
    let mut brace_depth = 0i32;
    let mut nested_query_starts: Vec<i32> = Vec::new();

    loop {
        p.check_iteration_limit();

        if p.at(TokenKind::LBrace) {
            brace_depth += 1;
            p.bump();
            consumed_any = true;
            continue;
        }

        if p.at(TokenKind::RBrace) {
            if brace_depth > 0 {
                brace_depth -= 1;
            }
            p.bump();
            consumed_any = true;
            continue;
        }

        // An extension region is opaque text, so the parser's own group
        // count ignores parens inside one. This scan has to agree, or its
        // idea of which `)` closes the caller's `(` drifts from the
        // parser's and it walks out through a boundary that is still open.
        if brace_depth > 0 {
            if p.at_end() || p.at(TokenKind::Semicolon) {
                break;
            }
            p.bump();
            consumed_any = true;
            continue;
        }

        if p.at(TokenKind::LParen) {
            paren_depth += 1;
            p.bump();
            consumed_any = true;
            p.skip_trivia();
            if super::select::is_query_starter_or_combiner_keyword(p) {
                nested_query_starts.push(paren_depth);
            }
            continue;
        }

        if p.at(TokenKind::RParen) {
            if paren_depth > 0 {
                if let Some(&d) = nested_query_starts.last() {
                    if d == paren_depth {
                        nested_query_starts.pop();
                    }
                }
                paren_depth -= 1;
                p.bump();
                consumed_any = true;
                continue;
            } else {
                break;
            }
        }

        let inside_nested_query = !nested_query_starts.is_empty();

        if paren_depth == 0 && brace_depth == 0 && super::at_query_boundary(p) {
            break;
        }

        // After a qualifying dot the word is a field name, not this query's
        // clause; the drain reads it that way and a skip that does not
        // hands the enclosing query a clause it never had.
        if brace_depth == 0
            && super::select::is_clause_keyword(p)
            && p.prev_significant() != Some(TokenKind::Dot)
        {
            let stop = if paren_depth == 0 {
                true
            } else if inside_nested_query {
                false
            } else {
                !super::select::is_query_starter_or_combiner_keyword(p)
            };
            if stop {
                break;
            }
        }

        // A separator ends the skipped fragment at any depth: it is the
        // boundary between package members, and no depth of ours outranks it.
        if p.at(TokenKind::Semicolon) {
            break;
        }

        if paren_depth == 0 && brace_depth == 0 && p.at(TokenKind::Comma) {
            break;
        }

        if p.at_end() {
            break;
        }

        p.bump();
        consumed_any = true;
    }

    if consumed_any {
        p.emit_error_at_marker(
            err,
            ParseError::Custom {
                message: "пропуск некорректного фрагмента",
                recovery: RecoveryKind::RecoverySpan,
            },
        );
    } else {
        err.abandon(p);
    }
}

pub(super) fn parse_delimited_list<F>(
    p: &mut Parser,
    delimiter: TokenKind,
    recovery_set: &crate::token_set::TokenSet,
    is_item_start: fn(&Parser) -> bool,
    mut parse_item: F,
) where
    F: FnMut(&mut Parser),
{
    // A list starting on its delimiter is missing its first item, not
    // holding one that happens to be a comma. Handing the delimiter to the
    // item rule costs the list the item that follows it, which the rule
    // reads as that one's alias.
    if p.at(delimiter) {
        let err = p.start();
        p.emit_error_at_marker(
            err,
            ParseError::Custom {
                message: "пропущен элемент списка",
                recovery: RecoveryKind::RecoverySpan,
            },
        );
    } else {
        parse_item(p);
    }

    loop {
        p.skip_trivia();

        if is_recovery_point(p, recovery_set) {
            break;
        }

        if !p.eat(delimiter) {
            break;
        }

        p.check_iteration_limit();
        p.skip_trivia();

        if p.at(delimiter) || is_recovery_point(p, recovery_set) || !is_item_start(p) {
            let err = p.start();
            p.emit_error_at_marker(
                err,
                ParseError::Custom {
                    message: "пропущен элемент списка",
                    recovery: RecoveryKind::RecoverySpan,
                },
            );

            if !p.at(delimiter) {
                break;
            }
            continue;
        }

        parse_item(p);
    }
}

pub fn logical_expression(p: &mut Parser) {
    logical_or_expr(p);
}

pub fn expression(p: &mut Parser) {
    logical_or_expr(p);
}

fn logical_or_expr(p: &mut Parser) {
    let m = p.start();

    logical_and_expr(p);

    loop {
        p.skip_trivia();
        if p.at(TokenKind::KwOr) {
            p.check_iteration_limit();
            p.bump();
            p.skip_trivia();
            logical_and_expr(p);
        } else {
            break;
        }
    }

    m.complete(p, NodeKind::SdblLogicalOrExpr);
}

fn logical_and_expr(p: &mut Parser) {
    let m = p.start();

    not_expr(p);

    loop {
        p.skip_trivia();
        if p.at(TokenKind::KwAnd) {
            p.check_iteration_limit();
            p.bump();
            p.skip_trivia();
            not_expr(p);
        } else {
            break;
        }
    }

    m.complete(p, NodeKind::SdblLogicalAndExpr);
}

fn not_expr(p: &mut Parser) {
    if p.at(TokenKind::KwNot) {
        let m = p.start();
        p.bump();
        p.skip_trivia();
        not_expr(p);
        m.complete(p, NodeKind::SdblNotExpr);
    } else {
        comparison_expr(p);
    }
}

fn additive_expr(p: &mut Parser) {
    let m = p.start();

    multiplicative_expr(p);

    loop {
        p.skip_trivia();
        if !matches!(p.current(), Some(TokenKind::Plus) | Some(TokenKind::Minus)) {
            break;
        }
        p.check_iteration_limit();
        p.bump();
        p.skip_trivia();
        multiplicative_expr(p);
    }

    m.complete(p, NodeKind::SdblAdditiveExpr);
}

fn multiplicative_expr(p: &mut Parser) {
    let m = p.start();

    unary_expr(p);

    loop {
        p.skip_trivia();
        if !matches!(
            p.current(),
            Some(TokenKind::Star) | Some(TokenKind::Slash) | Some(TokenKind::Percent)
        ) {
            break;
        }
        p.check_iteration_limit();
        p.bump();
        p.skip_trivia();
        unary_expr(p);
    }

    m.complete(p, NodeKind::SdblMultiplicativeExpr);
}

fn unary_expr(p: &mut Parser) {
    if matches!(
        p.current(),
        Some(TokenKind::Plus) | Some(TokenKind::Minus) | Some(TokenKind::KwNot)
    ) {
        let m = p.start();
        p.bump();
        p.skip_trivia();
        unary_expr(p);
        m.complete(p, NodeKind::SdblUnaryExpr);
    } else {
        primary_expr(p);
    }
}

fn primary_expr(p: &mut Parser) {
    if p.at_keyword("CASE") || p.at_keyword("ВЫБОР") {
        case_expr(p);
        return;
    }

    if p.at_keyword("NULL") {
        let m = p.start();
        p.bump();
        m.complete(p, NodeKind::SdblLiteral);
        return;
    }

    match p.current() {
        Some(TokenKind::LParen) => paren_or_subquery_expr(p),
        Some(TokenKind::Decimal) | Some(TokenKind::Float) => literal_expr(p),
        Some(TokenKind::String) => literal_expr(p),
        // The lexer has taken `'ГГГГММДД[ЧЧММСС]'` as one token since the
        // token set was re-derived; nothing here ever accepted it, so a
        // whole class of literal was an error wherever it stood.
        Some(TokenKind::Date) => literal_expr(p),
        Some(TokenKind::KwTrue) | Some(TokenKind::KwFalse) => literal_expr(p),
        Some(TokenKind::KwUndefined) => literal_expr(p),
        Some(TokenKind::Ampersand) => parameter_expr(p),

        Some(TokenKind::Star) => {
            let m = p.start();
            p.bump();
            m.complete(p, NodeKind::SdblLiteral);
        }

        // A query keyword is never a column name. Where nothing is open to
        // make it a subquery, it begins the package's next member, and an
        // operand missing in front of it has to be reported at the gap: the
        // boundary check in `error` then leaves the keyword where it is.
        // Taking it instead would cost the package that whole member — and
        // an operand left unwritten is exactly what an editor buffer holds.
        Some(TokenKind::Ident)
            if (p.open_group_count() > 0 || !super::at_query_boundary(p))
                && !at_a_keyword_that_cannot_be_a_field(p) =>
        {
            column_or_function(p)
        }

        // A keyword reaching here is an operand the text never wrote. What is
        // missing is an operand and not a field name: a literal, a parameter,
        // a call or a parenthesised expression would all have done, and naming
        // one of them would name the wrong thing. The recovery choice stays
        // the generic one, so a word no rule awaits is still consumed.
        Some(TokenKind::Ident) => {
            let m = p.start();
            p.error_custom("ожидался операнд, встречено ключевое слово");
            m.complete(p, NodeKind::SdblError);
        }

        _ => {
            let m = p.start();
            p.error_unexpected();
            m.complete(p, NodeKind::SdblError);
        }
    }
}

/// A clause keyword standing where only a field name can stand.
///
/// A bare name here is a field, and the source closes that position to
/// keywords: «Имена таблиц и полей не могут совпадать с ключевыми словами
/// языка запросов». A name carrying a dot is a different thing — the
/// qualifier of a chain, which may be the alias of a source, and an alias is
/// allowed to spell a keyword. `ПО Зак.Ссылка = Итоги.Регистратор` reads a
/// source aliased `КАК Итоги`, and refusing the word would cost that join its
/// condition.
/// A clause keyword standing where only a field name can stand.
///
/// The alias separator is not among them, though `А + КАК Б` has no reading in
/// which `КАК` is the operand: this rule is also reached where an expression
/// has legitimately ended and its alias follows, and refusing `КАК` there cost
/// eight production queries their `ИТОГИ … КАК Имя`. Telling the two apart
/// needs to know whether the expression behind this position is complete,
/// which is the caller's knowledge and not this one's.
fn at_a_keyword_that_cannot_be_a_field(p: &Parser) -> bool {
    p.names_are_fields()
        && (super::select::is_clause_keyword(p) || super::at_a_list_separator(p))
        && !next_is_a_qualifying_dot(p)
}

fn next_is_a_qualifying_dot(p: &Parser) -> bool {
    matches!(p.nth_non_trivia(0), Some(TokenKind::Dot))
}

fn literal_expr(p: &mut Parser) {
    if p.at(TokenKind::String) {
        string_literal_or_multi(p);
    } else {
        let m = p.start();
        p.bump();
        m.complete(p, NodeKind::SdblLiteral);
    }
}

fn string_literal_or_multi(p: &mut Parser) {
    let m = p.start();

    p.bump();

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

fn parameter_expr(p: &mut Parser) {
    let m = p.start();
    p.bump();

    if p.at(TokenKind::Ident) {
        p.bump();
    }

    m.complete(p, NodeKind::SdblParameter);
}

fn paren_or_subquery_expr(p: &mut Parser) {
    let m = p.start();

    p.bump();
    p.skip_trivia();

    if p.at_keyword("SELECT") || p.at_keyword("ВЫБРАТЬ") {
        super::select::subquery(p);
        p.skip_trivia();
        p.expect(TokenKind::RParen);
        m.complete(p, NodeKind::SdblSubqueryExpr);
    } else {
        expression(p);
        p.skip_trivia();

        if p.at(TokenKind::Comma) {
            while p.eat(TokenKind::Comma) {
                p.check_iteration_limit();
                p.skip_trivia();

                if p.at(TokenKind::RParen) || !is_expression_start(p) {
                    break;
                }

                expression(p);
                p.skip_trivia();
            }

            p.expect(TokenKind::RParen);
            m.complete(p, NodeKind::SdblTupleExpr);
        } else {
            p.expect(TokenKind::RParen);
            m.complete(p, NodeKind::SdblParenExpr);
        }
    }
}

fn comparison_expr(p: &mut Parser) {
    predicate_expr(p);
}

fn predicate_expr(p: &mut Parser) {
    let m = p.start();

    additive_expr(p);

    p.skip_trivia();

    if p.at(TokenKind::KwNot) {
        p.bump();
        p.skip_trivia();
    }

    if p.at(TokenKind::KwIn) {
        p.bump();
        p.skip_trivia();

        if p.at_keyword("HIERARCHY") || p.at_keyword("ИЕРАРХИИ") {
            p.bump();
            p.skip_trivia();

            if !p.expect(TokenKind::LParen) {
                m.complete(p, NodeKind::SdblInHierarchyExpr);
                return;
            }
            p.skip_trivia();

            if p.at_keyword("SELECT") || p.at_keyword("ВЫБРАТЬ") {
                super::select::subquery(p);
            } else {
                expression(p);
            }

            p.skip_trivia();
            p.expect(TokenKind::RParen);
            m.complete(p, NodeKind::SdblInHierarchyExpr);
        } else {
            if !p.expect(TokenKind::LParen) {
                m.complete(p, NodeKind::SdblInExpr);
                return;
            }
            p.skip_trivia();

            if p.at_keyword("SELECT") || p.at_keyword("ВЫБРАТЬ") {
                super::select::subquery(p);
            } else {
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
    } else if p.at_keyword("IS") || p.at_keyword("ЕСТЬ") {
        p.bump();
        p.skip_trivia();

        if p.at(TokenKind::KwNot) {
            p.bump();
            p.skip_trivia();
        }

        if !p.at_keyword("NULL") {
            m.abandon(p);
            return;
        }
        p.bump();

        m.complete(p, NodeKind::SdblIsNullExpr);
    } else if p.at_keyword("BETWEEN") || p.at_keyword("МЕЖДУ") {
        p.bump();
        p.skip_trivia();

        additive_expr(p);
        p.skip_trivia();

        if !p.at(TokenKind::KwAnd) {
            m.complete(p, NodeKind::SdblBetweenExpr);
            return;
        }
        p.bump();
        p.skip_trivia();

        additive_expr(p);

        m.complete(p, NodeKind::SdblBetweenExpr);
    } else if p.at_keyword("LIKE") || p.at_keyword("ПОДОБНО") {
        p.bump();
        p.skip_trivia();

        additive_expr(p);
        p.skip_trivia();

        if p.at_keyword("ESCAPE") || p.at_keyword("СПЕЦСИМВОЛ") {
            p.bump();
            p.skip_trivia();
            additive_expr(p);
        }

        m.complete(p, NodeKind::SdblLikeExpr);
    } else if p.at_keyword("REFS") || p.at_keyword("ССЫЛКА") {
        p.bump();
        p.skip_trivia();

        if super::at_field_name(p) {
            p.bump();
            p.skip_trivia();

            while super::eat_qualifying_dot(p) {
                p.check_iteration_limit();
                p.skip_trivia();
                if !super::at_table_name_component(p) {
                    // A word stands here and cannot be part of the name: say
                    // so, or it is taken in silence. Nothing standing here at
                    // all is the unfinished path of a query built by
                    // concatenation, which `QueryParseError` already reports —
                    // and reporting it twice, from a layer that does not know
                    // it is a fragment, is worse than not reporting it here.
                    if at_property_name(p) {
                        report_missing_name(p, "ожидалось имя объекта после '.'");
                    }
                    break;
                }
                p.bump();
                p.skip_trivia();
            }
        } else {
            // The source is explicit that what stands here is a table:
            // «проверяется, является ли значение выражения, указанного слева
            // от него, ссылкой на таблицу, указанную справа».
            report_missing_name(p, "ожидалось имя таблицы после 'ССЫЛКА' / 'REFS'");
        }

        m.complete(p, NodeKind::SdblRefsExpr);
    } else if matches!(
        p.current(),
        Some(TokenKind::Eq)
            | Some(TokenKind::Neq)
            | Some(TokenKind::Lt)
            | Some(TokenKind::Le)
            | Some(TokenKind::Gt)
            | Some(TokenKind::Ge)
    ) {
        p.bump();
        p.skip_trivia();
        additive_expr(p);
        m.complete(p, NodeKind::SdblComparisonExpr);
    } else {
        m.abandon(p);
    }
}

fn is_cast_function(p: &Parser) -> bool {
    p.at_keyword("CAST") || p.at_keyword("ВЫРАЗИТЬ")
}

fn parse_cast_type(p: &mut Parser) {
    let m = p.start();

    if super::at_field_name(p) {
        let is_number_type = p.at_keyword("NUMBER") || p.at_keyword("ЧИСЛО");

        p.bump();
        p.skip_trivia();

        while super::eat_qualifying_dot(p) {
            p.check_iteration_limit();
            p.skip_trivia();

            if !super::at_table_name_component(p) {
                // Saying «expected an identifier» of a word that IS one reads
                // as nonsense; what is wrong with it is that it is a keyword.
                if at_property_name(p) {
                    report_missing_name(p, "ожидалось имя объекта после '.'");
                } else {
                    let err = p.start();
                    let found = p.current();
                    p.emit_error_at_marker(
                        err,
                        ParseError::Expected {
                            expected: smallvec![TokenKind::Ident],
                            found,
                            recovery: RecoveryKind::RecoverySpan,
                        },
                    );
                }
                break;
            }

            p.bump();
            p.skip_trivia();
        }

        if p.at(TokenKind::LParen) {
            p.bump();
            p.skip_trivia();

            if p.at(TokenKind::Decimal) {
                p.bump();
                p.skip_trivia();

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
    } else {
        report_missing_name(p, "ожидался тип после 'КАК' / 'AS'");
    }

    m.complete(p, NodeKind::SdblType);
}

/// Reports a name the text does not hold, at the gap where it belongs.
///
/// A position that requires a name and finds none used to leave the node
/// empty and say nothing, so a query meaning something else parsed clean.
///
/// The message names what the position wants rather than what stands there:
/// a clause keyword is one thing that cannot be a name here, a parameter is
/// another, and a message that names only the first is wrong for the second.
/// The report goes in the gap, because whatever stands here is not this
/// rule's to take.
fn report_missing_name(p: &mut Parser, message: &'static str) {
    let m = p.start();
    p.emit_error_at_marker(m, ParseError::Custom { message, recovery: RecoveryKind::RecoverySpan });
}

fn column_or_function(p: &mut Parser) {
    let m = p.start();

    let is_cast = is_cast_function(p);

    p.bump();
    p.skip_trivia();

    if p.at(TokenKind::Dot) || super::at_a_qualifying_dot(p) {
        while super::eat_qualifying_dot(p) {
            let crossed_newline = p.skip_trivia_crossing_newline();

            if p.at(TokenKind::LParen) {
                inline_table_fields(p);
                break;
            }

            if !at_property_name(p) {
                let err = p.start();
                let found = p.current();
                p.emit_error_at_marker(
                    err,
                    ParseError::Expected {
                        expected: smallvec![TokenKind::Ident],
                        found,
                        recovery: RecoveryKind::RecoverySpan,
                    },
                );
                break;
            }

            // A keyword spelled right after a dot is a member name — SDBL keywords are
            // not reserved as field names (`Объект.Конец`, `Объект.Выбор`, `Объект.Итоги`).
            // Dangling-dot recovery is kept narrowly: across a newline any clause start
            // is recovered, and on the same line only the alias separator AS/КАК — which
            // is never a field name — breaks out instead of being swallowed.
            let dangling_dot_recovery = if crossed_newline {
                super::select::is_likely_clause_start_after_dot(p) || super::at_query_boundary(p)
            } else {
                super::select::at_sdbl_keyword(p, "AS", "КАК")
            };
            if dangling_dot_recovery {
                let err = p.start();
                p.emit_error_at_marker(
                    err,
                    ParseError::Custom {
                        message: "ожидалось имя поля, встречено ключевое слово",
                        recovery: RecoveryKind::RecoverySpan,
                    },
                );
                break;
            }

            p.bump();
            p.skip_trivia();
        }
        m.complete(p, NodeKind::SdblColumnRef);
    } else if p.at(TokenKind::LParen) {
        p.bump();
        p.skip_trivia();

        if !p.at(TokenKind::RParen) {
            if p.at_keyword("DISTINCT") || p.at_keyword("РАЗЛИЧНЫЕ") {
                p.bump();
                p.skip_trivia();
            }

            if is_expression_start(p)
                && !p.at(TokenKind::Comma)
                && !super::select::is_clause_keyword(p)
            {
                expression(p);

                if is_cast && (p.at_keyword("AS") || p.at_keyword("КАК")) {
                    p.skip_trivia();
                    p.bump();
                    p.skip_trivia();
                    parse_cast_type(p);
                    p.skip_trivia();
                } else {
                    p.skip_trivia();
                    if !p.at(TokenKind::Comma) && !p.at(TokenKind::RParen) {
                        recover_to_delimiter(p);
                    }
                }
            } else if p.at(TokenKind::Comma) {
                let err = p.start();
                p.emit_error_at_marker(
                    err,
                    ParseError::Custom {
                        message: "пропущен первый аргумент",
                        recovery: RecoveryKind::RecoverySpan,
                    },
                );
            }

            while p.eat(TokenKind::Comma) {
                p.check_iteration_limit();
                p.skip_trivia();

                if p.at(TokenKind::Comma)
                    || p.at(TokenKind::RParen)
                    || !is_expression_start(p)
                    || super::select::is_clause_keyword(p)
                {
                    let err = p.start();
                    p.emit_error_at_marker(
                        err,
                        ParseError::Custom {
                            message: "пропущен аргумент функции",
                            recovery: RecoveryKind::RecoverySpan,
                        },
                    );

                    if !p.at(TokenKind::Comma) {
                        break;
                    }
                    continue;
                }

                expression(p);

                p.skip_trivia();
                if !p.at(TokenKind::Comma) && !p.at(TokenKind::RParen) {
                    recover_to_delimiter(p);
                }
            }
        }

        p.skip_trivia();

        if super::select::is_clause_keyword(p) {
            let err = p.start();
            let found = p.current();
            p.emit_error_at_marker(
                err,
                ParseError::Expected {
                    expected: smallvec![TokenKind::RParen],
                    found,
                    recovery: RecoveryKind::RecoverySpan,
                },
            );
        } else {
            p.expect(TokenKind::RParen);
        }

        p.skip_trivia();
        while super::eat_qualifying_dot(p) {
            p.check_iteration_limit();
            let crossed_newline = p.skip_trivia_crossing_newline();

            if at_property_name(p) {
                // Same-line keyword after a dot is a member name; across a newline a
                // clause keyword is a dangling-dot recovery point. See the column-ref
                // dot loop above.
                if crossed_newline
                    && (super::select::is_clause_keyword(p) || super::at_query_boundary(p))
                {
                    let err = p.start();
                    p.emit_error_at_marker(
                        err,
                        ParseError::Custom {
                            message: "ожидалось имя поля, встречено ключевое слово",
                            recovery: RecoveryKind::RecoverySpan,
                        },
                    );
                    break;
                }

                p.bump();
                p.skip_trivia();
            } else {
                let err = p.start();
                let found = p.current();
                p.emit_error_at_marker(
                    err,
                    ParseError::Expected {
                        expected: smallvec![TokenKind::Ident],
                        found,
                        recovery: RecoveryKind::RecoverySpan,
                    },
                );
                break;
            }
        }

        m.complete(p, NodeKind::SdblFunctionCall);
    } else {
        m.complete(p, NodeKind::SdblColumnRef);
    }
}

fn inline_table_fields(p: &mut Parser) {
    let m = p.start();

    p.bump();
    p.skip_trivia();

    super::select::selected_fields(p);

    p.skip_trivia();
    p.expect(TokenKind::RParen);

    m.complete(p, NodeKind::SdblInlineTableFields);
}

fn case_expr(p: &mut Parser) {
    let m = p.start();

    p.bump();
    p.skip_trivia();

    let is_searched_case = p.at_keyword("WHEN") || p.at_keyword("КОГДА");

    if !is_searched_case {
        expression(p);
        p.skip_trivia();
    }

    let mut has_when = false;
    while p.at_keyword("WHEN") || p.at_keyword("КОГДА") {
        has_when = true;
        when_clause(p);
        p.skip_trivia();
    }

    if !has_when {
        p.error_custom("в выражении CASE отсутствует 'КОГДА' / 'WHEN'");
    }

    if p.at_keyword("ELSE") || p.at_keyword("ИНАЧЕ") {
        p.bump();
        p.skip_trivia();
        expression(p);
        p.skip_trivia();
    }

    if !p.at_keyword("END") && !p.at_keyword("КОНЕЦ") {
        p.error_custom("ожидалось 'КОНЕЦ' / 'END' в выражении CASE");
    } else {
        p.bump();
    }

    m.complete(p, NodeKind::SdblCaseExpr);
}

fn when_clause(p: &mut Parser) {
    let m = p.start();

    p.bump();
    p.skip_trivia();

    expression(p);
    p.skip_trivia();

    if !p.at_keyword("THEN") && !p.at_keyword("ТОГДА") {
        m.complete(p, NodeKind::SdblWhenClause);
        return;
    }
    p.bump();
    p.skip_trivia();

    expression(p);

    m.complete(p, NodeKind::SdblWhenClause);
}
