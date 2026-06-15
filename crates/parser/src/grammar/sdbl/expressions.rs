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
        | Some(TokenKind::KwTrue)
        | Some(TokenKind::KwFalse)
        | Some(TokenKind::KwUndefined) => true,

        Some(TokenKind::Ident) => !super::select::is_clause_keyword(p),

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
    let mut nested_query_starts: Vec<i32> = Vec::new();

    loop {
        p.check_iteration_limit();

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

        if super::select::is_clause_keyword(p) {
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

        if paren_depth == 0 && (p.at(TokenKind::Comma) || p.at(TokenKind::Semicolon)) {
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
    parse_item(p);

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
        Some(TokenKind::KwTrue) | Some(TokenKind::KwFalse) => literal_expr(p),
        Some(TokenKind::KwUndefined) => literal_expr(p),
        Some(TokenKind::Ampersand) => parameter_expr(p),

        Some(TokenKind::Star) => {
            let m = p.start();
            p.bump();
            m.complete(p, NodeKind::SdblLiteral);
        }

        Some(TokenKind::Ident) => column_or_function(p),

        _ => {
            let m = p.start();
            p.error_unexpected();
            m.complete(p, NodeKind::SdblError);
        }
    }
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

            expression(p);

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

        if p.at(TokenKind::Ident) {
            p.bump();
            p.skip_trivia();

            while p.eat(TokenKind::Dot) {
                p.check_iteration_limit();
                p.skip_trivia();
                if at_property_name(p) {
                    p.bump();
                    p.skip_trivia();
                } else {
                    break;
                }
            }
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

    if p.at(TokenKind::Ident) {
        let is_number_type = p.at_keyword("NUMBER") || p.at_keyword("ЧИСЛО");

        p.bump();
        p.skip_trivia();

        while p.at(TokenKind::Dot) {
            p.check_iteration_limit();
            p.bump();
            p.skip_trivia();

            if at_property_name(p) {
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
    }

    m.complete(p, NodeKind::SdblType);
}

fn column_or_function(p: &mut Parser) {
    let m = p.start();

    let is_cast = is_cast_function(p);

    p.bump();
    p.skip_trivia();

    if p.at(TokenKind::Dot) {
        while p.at(TokenKind::Dot) {
            p.bump();
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
                super::select::is_likely_clause_start_after_dot(p)
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
        while p.at(TokenKind::Dot) {
            p.check_iteration_limit();
            p.bump();
            let crossed_newline = p.skip_trivia_crossing_newline();

            if at_property_name(p) {
                // Same-line keyword after a dot is a member name; across a newline a
                // clause keyword is a dangling-dot recovery point. See the column-ref
                // dot loop above.
                if crossed_newline && super::select::is_clause_keyword(p) {
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
