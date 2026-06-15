use crate::event::NodeKind;
use crate::parser::Parser;
use lexer::TokenKind;
use parser_error::{ParseError, RecoveryKind};
use smallvec::smallvec;

use super::expressions;

pub(super) fn at_sdbl_keyword(p: &Parser, en: &str, ru: &str) -> bool {
    p.at_keyword(en) || p.at_keyword(ru)
}

pub(super) fn eat_sdbl_keyword(p: &mut Parser, en: &str, ru: &str) -> bool {
    p.eat_keyword(en) || p.eat_keyword(ru)
}

fn recover_field_to_alias_or_delimiter(p: &mut Parser) {
    let err = p.start();
    let mut case_depth = 0i32;
    let mut paren_depth = 0i32;
    let mut consumed_any = false;
    let mut nested_query_starts: Vec<i32> = Vec::new();

    loop {
        p.check_iteration_limit();

        if p.at_keyword("CASE") || p.at_keyword("ВЫБОР") {
            case_depth += 1;
            p.bump();
            consumed_any = true;
            continue;
        }

        if (p.at_keyword("END") || p.at_keyword("КОНЕЦ")) && case_depth > 0 {
            case_depth -= 1;
            p.bump();
            consumed_any = true;
            continue;
        }

        if p.at(TokenKind::LParen) {
            paren_depth += 1;
            p.bump();
            consumed_any = true;
            p.skip_trivia();
            if is_query_starter_or_combiner_keyword(p) {
                nested_query_starts.push(paren_depth);
            }
            continue;
        }

        if p.at(TokenKind::RParen) && paren_depth > 0 {
            if let Some(&d) = nested_query_starts.last() {
                if d == paren_depth {
                    nested_query_starts.pop();
                }
            }
            paren_depth -= 1;
            p.bump();
            consumed_any = true;
            continue;
        }

        let at_top_level = case_depth == 0 && paren_depth == 0;
        let inside_nested_query = !nested_query_starts.is_empty();
        if is_clause_keyword(p) {
            let stop = if at_top_level {
                true
            } else if inside_nested_query {
                false
            } else {
                !is_query_starter_or_combiner_keyword(p)
            };
            if stop {
                break;
            }
        }
        if p.at(TokenKind::Semicolon) {
            break;
        }
        if p.at_end() {
            break;
        }

        if case_depth == 0 && paren_depth == 0 {
            if at_sdbl_keyword(p, "AS", "КАК") {
                break;
            }
            if p.at(TokenKind::Comma) {
                break;
            }
            if p.at(TokenKind::RParen) {
                break;
            }
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

pub fn select_query(p: &mut Parser) {
    let m = p.start();
    subquery(p);
    select_tail_clauses(p);
    m.complete(p, NodeKind::SdblSelectQuery);
}

pub(super) fn subquery(p: &mut Parser) {
    let m = p.start();

    query(p);

    loop {
        p.skip_trivia();

        if p.at(TokenKind::Semicolon) {
            break;
        }

        if !at_sdbl_keyword(p, "UNION", "ОБЪЕДИНИТЬ") {
            break;
        }

        union_clause(p);
    }

    m.complete(p, NodeKind::SdblSubquery);
}

fn union_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "UNION", "ОБЪЕДИНИТЬ");

    p.skip_trivia();
    eat_sdbl_keyword(p, "ALL", "ВСЕ");

    p.skip_trivia();
    query(p);

    m.complete(p, NodeKind::SdblUnionClause);
}

fn query(p: &mut Parser) {
    let m = p.start();

    if !eat_sdbl_keyword(p, "SELECT", "ВЫБРАТЬ") {
        p.error_custom("ожидалось 'ВЫБРАТЬ' / 'SELECT'");
        m.complete(p, NodeKind::SdblQuery);
        return;
    }

    p.skip_trivia();
    if is_limitation_keyword(p) {
        limitations(p);
        p.skip_trivia();
    }

    selected_fields(p);

    p.skip_trivia();
    if at_sdbl_keyword(p, "INTO", "ПОМЕСТИТЬ") {
        into_clause(p);
    }

    query_body_clauses(p);

    m.complete(p, NodeKind::SdblQuery);
}

pub(super) fn selected_fields(p: &mut Parser) {
    let m = p.start();

    super::expressions::parse_delimited_list(
        p,
        TokenKind::Comma,
        &super::LIST_RECOVERY,
        is_field_start,
        selected_field,
    );

    m.complete(p, NodeKind::SdblFieldList);
}

fn selected_field(p: &mut Parser) {
    let m = p.start();

    if is_asterisk_start(p) {
        asterisk_field(p);
    } else {
        expressions::expression(p);
        p.skip_trivia();

        let at_expected_position = at_sdbl_keyword(p, "AS", "КАК")
            || (is_identifier_token(p) && !is_clause_keyword(p))
            || p.at(TokenKind::Comma)
            || p.at(TokenKind::Semicolon)
            || is_clause_keyword(p)
            || p.at_end();

        if !at_expected_position {
            recover_field_to_alias_or_delimiter(p);
            p.skip_trivia();
        }
    }

    p.skip_trivia();
    if (at_sdbl_keyword(p, "AS", "КАК") || is_identifier_token(p)) && !is_clause_keyword(p) {
        selected_field_alias(p);
    }

    m.complete(p, NodeKind::SdblSelectedField);
}

fn is_field_start(p: &Parser) -> bool {
    if is_asterisk_start(p) {
        return true;
    }
    super::expressions::is_expression_start(p)
}

fn is_asterisk_start(p: &Parser) -> bool {
    if p.at(TokenKind::Star) {
        return true;
    }

    if p.at(TokenKind::Ident) {
        if let Some(TokenKind::Dot) = p.nth(1) {
            if let Some(TokenKind::Star) = p.nth(2) {
                return true;
            }
        }
    }

    false
}

fn asterisk_field(p: &mut Parser) {
    let m = p.start();

    while p.at(TokenKind::Ident) {
        if let Some(TokenKind::Dot) = p.nth(1) {
            p.bump();
            p.bump();
        } else {
            break;
        }
    }

    p.expect(TokenKind::Star);

    m.complete(p, NodeKind::SdblAsteriskField);
}

fn selected_field_alias(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "AS", "КАК");
    p.skip_trivia();

    // SDBL keywords are not reserved as alias names: `КАК Итоги`, `AS Inner` are
    // valid field aliases. See `source_alias` for the rationale.
    if is_body_clause_keyword(p) {
        let err = p.start();
        p.emit_error_at_marker(
            err,
            ParseError::Custom {
                message: "ожидался алиас, встречено ключевое слово",
                recovery: RecoveryKind::RecoverySpan,
            },
        );
        m.complete(p, NodeKind::SdblAlias);
        return;
    }

    let _ = p.expect(TokenKind::Ident);

    m.complete(p, NodeKind::SdblAlias);
}

fn into_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "INTO", "ПОМЕСТИТЬ");
    p.skip_trivia();

    if p.at(TokenKind::Ident) {
        let table_m = p.start();
        p.bump();
        table_m.complete(p, NodeKind::SdblTempTableName);
    } else {
        p.error_custom("ожидалось имя временной таблицы после 'ПОМЕСТИТЬ' / 'INTO'");
    }

    m.complete(p, NodeKind::SdblIntoClause);
}

fn is_data_source_start(p: &Parser) -> bool {
    match p.current() {
        Some(TokenKind::LParen) => true,
        Some(TokenKind::Ampersand) => true,
        Some(TokenKind::Ident) => !is_clause_keyword(p),
        _ => false,
    }
}

fn from_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "FROM", "ИЗ");
    p.skip_trivia();

    super::expressions::parse_delimited_list(
        p,
        TokenKind::Comma,
        &super::LIST_RECOVERY,
        is_data_source_start,
        data_source,
    );

    m.complete(p, NodeKind::SdblFromClause);
}

fn data_source(p: &mut Parser) {
    let m = p.start();

    if p.at(TokenKind::LParen) {
        p.bump();
        p.skip_trivia();
        subquery(p);
        p.expect(TokenKind::RParen);
    } else {
        table_ref(p);
    }

    p.skip_trivia();
    if (at_sdbl_keyword(p, "AS", "КАК") || is_identifier_token(p)) && !is_clause_keyword(p) {
        source_alias(p);
    }

    p.skip_trivia();
    while is_join_keyword(p) {
        join_clause(p);
        p.skip_trivia();
    }

    m.complete(p, NodeKind::SdblDataSource);
}

fn table_ref(p: &mut Parser) {
    let m = p.start();

    if p.at(TokenKind::Ampersand) {
        let pm = p.start();
        p.bump();
        if p.at(TokenKind::Ident) {
            p.bump();
        }
        pm.complete(p, NodeKind::SdblParameter);
        m.complete(p, NodeKind::SdblTableRef);
        return;
    }

    if !p.expect(TokenKind::Ident) {
        m.complete(p, NodeKind::SdblTableRef);
        return;
    }

    while p.eat(TokenKind::Dot) {
        p.check_iteration_limit();
        p.skip_trivia();

        if !super::expressions::at_property_name(p) {
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

        if is_clause_keyword(p) || p.at_keyword("AS") || p.at_keyword("КАК") {
            let err = p.start();
            p.emit_error_at_marker(
                err,
                ParseError::Custom {
                    message: "ожидалось имя объекта, встречено ключевое слово",
                    recovery: RecoveryKind::RecoverySpan,
                },
            );
            break;
        }

        p.bump();
    }

    p.skip_trivia();
    virtual_table_args(p);

    m.complete(p, NodeKind::SdblTableRef);
}

fn source_alias(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "AS", "КАК");
    p.skip_trivia();

    // SDBL keywords are not reserved as alias names: `КАК Итоги`, `AS Inner` are
    // valid source aliases, recognised here because clause keywords are matched by
    // text on `Ident` tokens. Only a primary body clause after AS signals an omitted
    // alias and is left for its clause parser.
    if is_body_clause_keyword(p) {
        let err = p.start();
        p.emit_error_at_marker(
            err,
            ParseError::Custom {
                message: "ожидался алиас источника, встречено ключевое слово",
                recovery: RecoveryKind::RecoverySpan,
            },
        );
        m.complete(p, NodeKind::SdblAlias);
        return;
    }

    p.expect(TokenKind::Ident);

    m.complete(p, NodeKind::SdblAlias);
}

fn is_join_keyword(p: &Parser) -> bool {
    p.at_keyword("LEFT")
        || p.at_keyword("ЛЕВОЕ")
        || p.at_keyword("RIGHT")
        || p.at_keyword("ПРАВОЕ")
        || p.at_keyword("FULL")
        || p.at_keyword("ПОЛНОЕ")
        || p.at_keyword("INNER")
        || p.at_keyword("ВНУТРЕННЕЕ")
        || p.at_keyword("JOIN")
        || p.at_keyword("СОЕДИНЕНИЕ")
}

fn join_clause(p: &mut Parser) {
    let m = p.start();

    let has_type = p.at_keyword("LEFT")
        || p.at_keyword("ЛЕВОЕ")
        || p.at_keyword("RIGHT")
        || p.at_keyword("ПРАВОЕ")
        || p.at_keyword("FULL")
        || p.at_keyword("ПОЛНОЕ")
        || p.at_keyword("INNER")
        || p.at_keyword("ВНУТРЕННЕЕ");
    if has_type {
        p.bump();
        p.skip_trivia();
    }

    if p.at_keyword("OUTER") || p.at_keyword("ВНЕШНЕЕ") {
        p.bump();
        p.skip_trivia();
    }

    if !p.at_keyword("JOIN") && !p.at_keyword("СОЕДИНЕНИЕ") {
        p.error_custom("ожидалось 'СОЕДИНЕНИЕ' / 'JOIN'");
        m.complete(p, NodeKind::SdblJoinClause);
        return;
    }
    p.bump();
    p.skip_trivia();

    data_source(p);
    p.skip_trivia();

    if !eat_sdbl_keyword(p, "ON", "ПО") {
        p.error_custom("ожидалось 'ПО' / 'ON' в соединении");
    }
    p.skip_trivia();

    expressions::logical_expression(p);

    m.complete(p, NodeKind::SdblJoinClause);
}

fn select_tail_clauses(p: &mut Parser) {
    let mut parsed_autoorder = false;
    let mut parsed_order_by = false;
    let mut parsed_totals_by = false;

    loop {
        p.skip_trivia();

        if !parsed_autoorder && at_sdbl_keyword(p, "AUTOORDER", "АВТОУПОРЯДОЧИВАНИЕ")
        {
            autoorder_clause(p);
            parsed_autoorder = true;
            continue;
        }

        if !parsed_order_by && at_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ") {
            order_by_clause(p);
            parsed_order_by = true;
            continue;
        }

        if !parsed_totals_by && at_sdbl_keyword(p, "TOTALS", "ИТОГИ") {
            totals_by_clause(p);
            parsed_totals_by = true;
            continue;
        }

        break;
    }
}

fn query_body_clauses(p: &mut Parser) {
    p.skip_trivia();
    if at_sdbl_keyword(p, "FROM", "ИЗ") {
        from_clause(p);
    }

    p.skip_trivia();
    if at_sdbl_keyword(p, "WHERE", "ГДЕ") {
        where_clause(p);
    }

    p.skip_trivia();
    if at_sdbl_keyword(p, "GROUP", "СГРУППИРОВАТЬ") {
        group_by_clause(p);
    }

    p.skip_trivia();
    if at_sdbl_keyword(p, "HAVING", "ИМЕЮЩИЕ") {
        having_clause(p);
    }

    p.skip_trivia();
    if at_sdbl_keyword(p, "FOR", "ДЛЯ") {
        for_update_clause(p);
    }

    p.skip_trivia();
    if at_sdbl_keyword(p, "INDEX", "ИНДЕКСИРОВАТЬ") {
        index_by_clause(p);
    }

    p.skip_trivia();
    if at_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ") {
        order_by_clause(p);
    }
}

fn where_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "WHERE", "ГДЕ");
    p.skip_trivia();

    expressions::logical_expression(p);

    m.complete(p, NodeKind::SdblWhereClause);
}

pub(super) fn is_clause_keyword(p: &Parser) -> bool {
    at_sdbl_keyword(p, "SELECT", "ВЫБРАТЬ")
        || at_sdbl_keyword(p, "FROM", "ИЗ")
        || at_sdbl_keyword(p, "WHERE", "ГДЕ")
        || at_sdbl_keyword(p, "GROUP", "СГРУППИРОВАТЬ")
        || at_sdbl_keyword(p, "HAVING", "ИМЕЮЩИЕ")
        || at_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ")
        || at_sdbl_keyword(p, "UNION", "ОБЪЕДИНИТЬ")
        || at_sdbl_keyword(p, "INTO", "ПОМЕСТИТЬ")
        || at_sdbl_keyword(p, "ON", "ПО")
        || at_sdbl_keyword(p, "FOR", "ДЛЯ")
        || at_sdbl_keyword(p, "INDEX", "ИНДЕКСИРОВАТЬ")
        || at_sdbl_keyword(p, "AUTOORDER", "АВТОУПОРЯДОЧИВАНИЕ")
        || at_sdbl_keyword(p, "TOTALS", "ИТОГИ")
        || is_join_keyword(p)
}

pub(super) fn is_likely_clause_start_after_dot(p: &Parser) -> bool {
    is_clause_keyword(p)
        || at_sdbl_keyword(p, "AS", "КАК")
        || at_sdbl_keyword(p, "ASC", "ВОЗР")
        || at_sdbl_keyword(p, "DESC", "УБЫВ")
        || at_sdbl_keyword(p, "CASE", "ВЫБОР")
        || at_sdbl_keyword(p, "WHEN", "КОГДА")
        || at_sdbl_keyword(p, "THEN", "ТОГДА")
        || at_sdbl_keyword(p, "ELSE", "ИНАЧЕ")
        || at_sdbl_keyword(p, "END", "КОНЕЦ")
        || at_sdbl_keyword(p, "BETWEEN", "МЕЖДУ")
        || at_sdbl_keyword(p, "LIKE", "ПОДОБНО")
        || at_sdbl_keyword(p, "ESCAPE", "СПЕЦСИМВОЛ")
        || at_sdbl_keyword(p, "ALL", "ВСЕ")
        || at_sdbl_keyword(p, "DISTINCT", "РАЗЛИЧНЫЕ")
        || at_sdbl_keyword(p, "TOP", "ПЕРВЫЕ")
        || at_sdbl_keyword(p, "HIERARCHY", "ИЕРАРХИИ")
        || at_sdbl_keyword(p, "DROP", "УНИЧТОЖИТЬ")
        || at_sdbl_keyword(p, "BY", "ПО")
}

pub(super) fn is_query_starter_or_combiner_keyword(p: &Parser) -> bool {
    at_sdbl_keyword(p, "SELECT", "ВЫБРАТЬ") || at_sdbl_keyword(p, "UNION", "ОБЪЕДИНИТЬ")
}

// Primary clause keywords that begin a new query section. When one of these
// appears where an alias is expected the alias was almost certainly omitted, so
// the alias parser leaves the keyword for its clause instead of swallowing it.
// `ИТОГИ`, join keywords and `ПО`/`ДЛЯ`/`ИНДЕКСИРОВАТЬ` are excluded: they are
// valid alias names (`КАК Итоги`, `AS Inner`) and not reserved.
pub(super) fn is_body_clause_keyword(p: &Parser) -> bool {
    at_sdbl_keyword(p, "SELECT", "ВЫБРАТЬ")
        || at_sdbl_keyword(p, "FROM", "ИЗ")
        || at_sdbl_keyword(p, "WHERE", "ГДЕ")
        || at_sdbl_keyword(p, "GROUP", "СГРУППИРОВАТЬ")
        || at_sdbl_keyword(p, "HAVING", "ИМЕЮЩИЕ")
        || at_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ")
        || at_sdbl_keyword(p, "UNION", "ОБЪЕДИНИТЬ")
        || at_sdbl_keyword(p, "INTO", "ПОМЕСТИТЬ")
}

fn group_by_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "GROUP", "СГРУППИРОВАТЬ");
    p.skip_trivia();

    if !at_sdbl_keyword(p, "BY", "ПО") {
        m.complete(p, NodeKind::SdblGroupClause);
        return;
    }
    eat_sdbl_keyword(p, "BY", "ПО");
    p.skip_trivia();

    super::expressions::expression(p);

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        p.skip_trivia();
        super::expressions::expression(p);
    }

    m.complete(p, NodeKind::SdblGroupClause);
}

fn order_by_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ");
    p.skip_trivia();

    if !at_sdbl_keyword(p, "BY", "ПО") {
        m.complete(p, NodeKind::SdblOrderClause);
        return;
    }
    eat_sdbl_keyword(p, "BY", "ПО");
    p.skip_trivia();

    order_by_item(p);

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        p.skip_trivia();
        order_by_item(p);
    }

    m.complete(p, NodeKind::SdblOrderClause);
}

fn order_by_item(p: &mut Parser) {
    super::expressions::expression(p);
    p.skip_trivia();

    if p.at_keyword("ASC") || p.at_keyword("ВОЗР") || p.at_keyword("DESC") || p.at_keyword("УБЫВ")
    {
        p.bump();
        p.skip_trivia();
    }

    if p.at_keyword("HIERARCHY") || p.at_keyword("ИЕРАРХИЯ") {
        p.bump();
        p.skip_trivia();
    }
}

fn having_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "HAVING", "ИМЕЮЩИЕ");
    p.skip_trivia();

    super::expressions::expression(p);

    m.complete(p, NodeKind::SdblHavingClause);
}

fn for_update_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "FOR", "ДЛЯ");
    p.skip_trivia();

    eat_sdbl_keyword(p, "UPDATE", "ИЗМЕНЕНИЯ");
    p.skip_trivia();

    if p.at(TokenKind::Ident) && !is_clause_keyword(p) {
        p.bump();
        while p.at(TokenKind::Dot) {
            p.check_iteration_limit();
            p.bump();
            if super::expressions::at_property_name(p) {
                p.bump();
            } else {
                break;
            }
        }
    }

    m.complete(p, NodeKind::SdblForUpdate);
}

fn index_by_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "INDEX", "ИНДЕКСИРОВАТЬ");
    p.skip_trivia();

    if !at_sdbl_keyword(p, "BY", "ПО") {
        m.complete(p, NodeKind::SdblIndexBy);
        return;
    }
    eat_sdbl_keyword(p, "BY", "ПО");
    p.skip_trivia();

    super::expressions::expression(p);

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        p.skip_trivia();
        super::expressions::expression(p);
    }

    m.complete(p, NodeKind::SdblIndexBy);
}

fn autoorder_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "AUTOORDER", "АВТОУПОРЯДОЧИВАНИЕ");

    m.complete(p, NodeKind::SdblAutoorder);
}

fn totals_by_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "TOTALS", "ИТОГИ");
    p.skip_trivia();

    while !p.at_end() {
        p.skip_trivia();

        if at_sdbl_keyword(p, "BY", "ПО") {
            break;
        }

        if is_clause_keyword(p) {
            break;
        }

        if super::expressions::is_expression_start(p) {
            super::expressions::expression(p);

            p.skip_trivia();
            if !p.at(TokenKind::Comma) {
                continue;
            }
            p.bump();
        } else {
            break;
        }
    }

    if !at_sdbl_keyword(p, "BY", "ПО") {
        m.complete(p, NodeKind::SdblTotalsBy);
        return;
    }
    eat_sdbl_keyword(p, "BY", "ПО");
    p.skip_trivia();

    totals_group_item(p);

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        p.skip_trivia();
        totals_group_item(p);
    }

    m.complete(p, NodeKind::SdblTotalsBy);
}

fn totals_group_item(p: &mut Parser) {
    super::expressions::expression(p);
    p.skip_trivia();

    if p.at_keyword("ONLY") || p.at_keyword("ТОЛЬКО") {
        p.bump();
        p.skip_trivia();
    }

    if p.at_keyword("HIERARCHY") || p.at_keyword("ИЕРАРХИЯ") {
        p.bump();
        p.skip_trivia();
    }
}

fn is_identifier_token(p: &Parser) -> bool {
    p.at(TokenKind::Ident)
}

fn is_limitation_keyword(p: &Parser) -> bool {
    at_sdbl_keyword(p, "DISTINCT", "РАЗЛИЧНЫЕ")
        || at_sdbl_keyword(p, "TOP", "ПЕРВЫЕ")
        || at_sdbl_keyword(p, "ALLOWED", "РАЗРЕШЕННЫЕ")
}

fn limitations(p: &mut Parser) {
    let m = p.start();

    while is_limitation_keyword(p) {
        if at_sdbl_keyword(p, "DISTINCT", "РАЗЛИЧНЫЕ") {
            eat_sdbl_keyword(p, "DISTINCT", "РАЗЛИЧНЫЕ");
        } else if at_sdbl_keyword(p, "TOP", "ПЕРВЫЕ") {
            top_clause(p);
        } else if at_sdbl_keyword(p, "ALLOWED", "РАЗРЕШЕННЫЕ") {
            eat_sdbl_keyword(p, "ALLOWED", "РАЗРЕШЕННЫЕ");
        }
        p.skip_trivia();
    }

    m.complete(p, NodeKind::SdblLimitations);
}

fn top_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "TOP", "ПЕРВЫЕ");
    p.skip_trivia();

    p.expect(TokenKind::Decimal);

    m.complete(p, NodeKind::SdblTopClause);
}

fn recover_to_delimiter_vt(p: &mut Parser) {
    let recovery = p.start();
    let mut consumed_any = false;
    let mut paren_depth: u32 = 0;
    let mut nested_query_starts: Vec<u32> = Vec::new();

    loop {
        p.check_iteration_limit();

        if p.at(TokenKind::LParen) {
            paren_depth += 1;
            p.bump();
            consumed_any = true;
            p.skip_trivia();
            if is_query_starter_or_combiner_keyword(p) {
                nested_query_starts.push(paren_depth);
            }
            continue;
        }
        if p.at(TokenKind::RParen) {
            if paren_depth == 0 {
                break;
            }
            if let Some(&d) = nested_query_starts.last() {
                if d == paren_depth {
                    nested_query_starts.pop();
                }
            }
            paren_depth -= 1;
            p.bump();
            consumed_any = true;
            continue;
        }

        let inside_nested_query = !nested_query_starts.is_empty();

        if is_clause_keyword(p) {
            let stop = if paren_depth == 0 {
                true
            } else if inside_nested_query {
                false
            } else {
                !is_query_starter_or_combiner_keyword(p)
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
            recovery,
            ParseError::Custom {
                message: "пропуск некорректного фрагмента",
                recovery: RecoveryKind::RecoverySpan,
            },
        );
    } else {
        recovery.abandon(p);
    }
}

fn virtual_table_args(p: &mut Parser) {
    if !p.at(TokenKind::LParen) {
        return;
    }
    p.bump();
    p.skip_trivia();

    if p.at(TokenKind::RParen) {
        p.expect(TokenKind::RParen);
        return;
    }

    if super::expressions::is_expression_start(p) && !p.at(TokenKind::Comma) {
        super::expressions::expression(p);
        p.skip_trivia();
        if !p.at(TokenKind::Comma) && !p.at(TokenKind::RParen) {
            recover_to_delimiter_vt(p);
        }
    } else {
        let m = p.start();
        m.complete(p, NodeKind::SdblMissingArg);
    }

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        p.skip_trivia();

        let empty_slot = p.at(TokenKind::Comma)
            || p.at(TokenKind::RParen)
            || !super::expressions::is_expression_start(p);
        if empty_slot {
            let m = p.start();
            m.complete(p, NodeKind::SdblMissingArg);
            if !p.at(TokenKind::Comma) {
                break;
            }
            continue;
        }

        super::expressions::expression(p);
        p.skip_trivia();
        if !p.at(TokenKind::Comma) && !p.at(TokenKind::RParen) {
            recover_to_delimiter_vt(p);
        }
    }

    p.skip_trivia();
    if p.at(TokenKind::RParen) {
        p.bump();
    } else if is_clause_keyword(p) {
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
}

#[cfg(test)]
mod tests {
    use crate::parse_sdbl;

    #[test]
    fn test_error_recovery_incomplete_field_list() {
        let input = r#"ВЫБРАТЬ
    Очередь.
ИЗ
    РегистрСведений.ОчередьОбновленияКэширующихДанных КАК Очередь"#;

        let parse = parse_sdbl(input);
        let tree_text = format!("{:#?}", parse.syntax_node());

        assert!(
            tree_text.contains("ERROR"),
            "Expected ERROR node for incomplete field.\nTree: {}",
            tree_text
        );

        assert!(
            tree_text.contains("SDBL_FROM_CLAUSE"),
            "FROM clause should be parsed despite incomplete field list.\nTree: {}",
            tree_text
        );

        assert!(
            tree_text.contains("SDBL_DATA_SOURCE"),
            "Data source should be in FROM clause.\nTree: {}",
            tree_text
        );
    }

    #[test]
    fn test_error_recovery_complete_query_after_incomplete_field() {
        let input = r#"ВЫБРАТЬ
    Очередь.
ИЗ
    РегистрСведений.Тест КАК Очередь
ГДЕ
    Очередь.Попыток < 3"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        assert!(
            text.contains("ERROR"),
            "Expected ERROR node for incomplete field.\nTree: {}",
            text
        );

        assert!(text.contains("SDBL_FROM_CLAUSE"), "FROM clause should be parsed.\nTree: {}", text);

        assert!(
            text.contains("SDBL_WHERE_CLAUSE"),
            "WHERE clause should be parsed.\nTree: {}",
            text
        );
    }

    #[test]
    fn test_error_recovery_incomplete_field_in_middle_of_list() {
        let input = r#"ВЫБРАТЬ ПЕРВЫЕ 500
    Очередь.,
    Очередь.ЗависимыйОбъектМетаданных КАК ЗависимыйОбъектМетаданных
ИЗ
    РегистрСведений.ОчередьОбновленияКэширующихДанных КАК Очередь
ГДЕ
    Очередь.Попыток < 3"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        assert!(
            text.contains("ERROR"),
            "Expected ERROR node for incomplete field.\nTree: {}",
            text
        );

        assert!(
            text.contains("SDBL_FROM_CLAUSE"),
            "FROM clause should be parsed despite incomplete field in middle of list.\nTree: {}",
            text
        );

        assert!(
            text.contains("SDBL_DATA_SOURCE"),
            "Data source should be in FROM clause.\nTree: {}",
            text
        );

        assert!(
            text.contains("SDBL_WHERE_CLAUSE"),
            "WHERE clause should be parsed.\nTree: {}",
            text
        );

        let field_count = text.matches("SDBL_SELECTED_FIELD").count();
        assert!(
            field_count >= 2,
            "Should have at least 2 selected fields (incomplete + complete). Got: {}",
            field_count
        );
    }

    #[test]
    fn test_tuple_in_in_predicate() {
        let input = r#"ВЫБРАТЬ *
ИЗ Документ.Заказ КАК Заказ
ГДЕ (Заказ.Партнер, Заказ.Контрагент, Заказ.Организация) В
    (ВЫБРАТЬ Т.Партнер, Т.Контрагент, Т.Организация ИЗ ВТ_Данные КАК Т)"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        assert!(
            text.contains("SDBL_TUPLE_EXPR"),
            "Expected SDBL_TUPLE_EXPR for tuple in IN predicate.\nTree: {}",
            text
        );

        assert!(text.contains("SDBL_IN_EXPR"), "Expected SDBL_IN_EXPR.\nTree: {}", text);

        assert!(
            text.contains("SDBL_SUBQUERY"),
            "Expected subquery inside IN predicate.\nTree: {}",
            text
        );

        assert!(!parse.has_errors(), "Should parse without errors: {:?}", parse.errors());
    }

    #[test]
    fn test_simple_tuple() {
        let input = r#"ВЫБРАТЬ * ИЗ Т ГДЕ (А, Б, Г) = (1, 2, 3)"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        let tuple_count = text.matches("SDBL_TUPLE_EXPR").count();
        assert!(
            tuple_count >= 2,
            "Expected at least 2 SDBL_TUPLE_EXPR nodes. Got: {}\nTree: {}",
            tuple_count,
            text
        );
    }

    #[test]
    fn test_paren_expr_not_tuple() {
        let input = r#"ВЫБРАТЬ (А + Б) КАК Сумма ИЗ Т"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        assert!(
            !text.contains("SDBL_TUPLE_EXPR"),
            "Single parenthesized expression should not create SDBL_TUPLE_EXPR.\nTree: {}",
            text
        );

        assert!(
            text.contains("SDBL_PAREN_EXPR"),
            "Expected SDBL_PAREN_EXPR for single parenthesized expression.\nTree: {}",
            text
        );
    }

    #[test]
    fn test_tuple_in_virtual_table_params() {
        let input = r#"ВЫБРАТЬ *
ИЗ РегистрНакопления.Расчеты.Обороты(
    &Начало,
    &Конец,
    ,
    (Аналитика.Партнер, Аналитика.Контрагент) В
        (ВЫБРАТЬ Т.Партнер, Т.Контрагент ИЗ ВТ_Данные КАК Т)
) КАК Обороты"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        assert!(
            text.contains("SDBL_TUPLE_EXPR"),
            "Expected SDBL_TUPLE_EXPR in virtual table params.\nTree: {}",
            text
        );

        assert!(text.contains("SDBL_TABLE_REF"), "Expected SDBL_TABLE_REF.\nTree: {}", text);
    }
}
