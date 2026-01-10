//! SELECT query parsing for SDBL
//!
//! Implements parsing for SELECT queries including:
//! - Field lists with aliases
//! - FROM clauses with data sources
//! - WHERE clauses
//! - UNION queries
//! - Subqueries

use crate::event::NodeKind;
use crate::parser::Parser;
use lexer::TokenKind;

use super::expressions;

/// Helper to check for bilingual SDBL keywords (English or Russian).
fn at_sdbl_keyword(p: &Parser, en: &str, ru: &str) -> bool {
    p.at_keyword(en) || p.at_keyword(ru)
}

/// Helper to consume bilingual SDBL keywords (English or Russian).
fn eat_sdbl_keyword(p: &mut Parser, en: &str, ru: &str) -> bool {
    p.eat_keyword(en) || p.eat_keyword(ru)
}

/// Parse a SELECT query
///
/// Grammar: `selectQuery: subquery (autoorder | orderBy | totalBy)?`
///
/// Phase 1: Only basic SELECT...FROM...WHERE
/// Phase 2: Add ORDER BY, TOTALS BY, AUTOORDER
pub fn select_query(p: &mut Parser) {
    let m = p.start();

    subquery(p);

    // Phase 2: Add ORDER BY, TOTALS BY, AUTOORDER support
    // if p.at(TokenKind::KwAutoOrder) { ... }
    // if p.at(TokenKind::KwOrder) { order_by(p); }
    // if p.at(TokenKind::KwTotals) { total_by(p); }

    m.complete(p, NodeKind::SdblSelectQuery);
}

/// Parse a subquery (main query + optional UNIONs)
///
/// Grammar: `subquery: main=query orderBy? (unions+=union+)?`
pub(super) fn subquery(p: &mut Parser) {
    let m = p.start();

    // Parse main query
    query(p);

    // Parse UNION clauses
    loop {
        p.skip_trivia(); // Must skip trivia before checking for UNION keyword

        // Stop at semicolons (end of query package item)
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

/// Parse a UNION clause
///
/// Grammar: `union: UNION ALL? query orderBy?`
fn union_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "UNION", "ОБЪЕДИНИТЬ");

    p.skip_trivia(); // Skip whitespace before checking for ALL

    // Optional ALL keyword
    eat_sdbl_keyword(p, "ALL", "ВСЕ");

    p.skip_trivia();

    // Parse the UNION query
    query(p);

    // Phase 2: Add ORDER BY support for UNION queries
    // if p.at_keyword("ORDER") { order_by(p); }

    m.complete(p, NodeKind::SdblUnionClause);
}

/// Parse a single SELECT query
///
/// Grammar:
/// ```text
/// query:
///     SELECT limitations?
///     columns=selectedFields
///     (INTO temporaryTableName)?
///     (FROM dataSources)?
///     (WHERE logicalExpression)?
///     (GROUP BY groupByItem)?
///     (HAVING logicalExpression)?
///     (FOR UPDATE forUpdate)?
///     (INDEX BY indexingItem*)?
/// ```
///
/// Phase 1: SELECT fields FROM sources WHERE condition
/// Phase 2: Add GROUP BY, HAVING, INTO, FOR UPDATE, INDEX BY
fn query(p: &mut Parser) {
    let m = p.start();

    // SELECT/ВЫБРАТЬ keyword (mandatory)
    if !eat_sdbl_keyword(p, "SELECT", "ВЫБРАТЬ") {
        p.error(); // Expected SELECT/ВЫБРАТЬ
        m.complete(p, NodeKind::SdblQuery);
        return;
    }

    p.skip_trivia();

    // Parse limitations (DISTINCT, TOP, ALLOWED) if present
    if is_limitation_keyword(p) {
        limitations(p);
        p.skip_trivia();
    }

    // Selected fields (mandatory)
    selected_fields(p);

    // INTO clause for temporary tables (minimal support - just skip it)
    p.skip_trivia();
    if at_sdbl_keyword(p, "INTO", "ПОМЕСТИТЬ") {
        eat_sdbl_keyword(p, "INTO", "ПОМЕСТИТЬ");
        p.skip_trivia();
        // Skip temporary table name (identifier)
        if p.at(TokenKind::Ident) {
            p.bump();
        }
    }

    // FROM clause (optional)
    p.skip_trivia(); // CRITICAL: Must skip trivia before checking for FROM
    if at_sdbl_keyword(p, "FROM", "ИЗ") {
        from_clause(p);
    }

    // WHERE clause (optional)
    p.skip_trivia(); // CRITICAL: Must skip trivia before checking for WHERE
    if at_sdbl_keyword(p, "WHERE", "ГДЕ") {
        where_clause(p);
    }

    // GROUP BY clause (optional)
    p.skip_trivia();
    if at_sdbl_keyword(p, "GROUP", "СГРУППИРОВАТЬ") {
        group_by_clause(p);
    }

    // HAVING clause (optional)
    // TODO: Implement HAVING support
    // p.skip_trivia();
    // if at_sdbl_keyword(p, "HAVING", "ИМЕЮЩИЕ") {
    //     having_clause(p);
    // }

    // ORDER BY clause (optional)
    p.skip_trivia();
    if at_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ") {
        order_by_clause(p);
    }

    m.complete(p, NodeKind::SdblQuery);
}

/// Parse selected fields list
///
/// Grammar: `selectedFields: fields+=selectedField (COMMA fields+=selectedField)*`
///
/// Error recovery: If we encounter a clause keyword (FROM, WHERE, etc.) while parsing fields,
/// we stop immediately and let the clause parser handle it. This allows completion to work
/// even when field list is incomplete (e.g., "SELECT Table.| FROM ...").
fn selected_fields(p: &mut Parser) {
    let m = p.start();

    // Parse first field (mandatory)
    selected_field(p);

    // Parse additional fields (COMMA field)*
    loop {
        p.skip_trivia(); // CRITICAL: Skip trivia before checking for comma

        // ERROR RECOVERY: Check if we hit a clause keyword
        // This allows incomplete field lists like "SELECT Table.|" to not block FROM parsing
        if is_clause_keyword(p) {
            break; // Let FROM/WHERE/etc. parser handle this token
        }

        if !p.eat(TokenKind::Comma) {
            break; // No more fields
        }

        p.skip_trivia();
        selected_field(p);
    }

    m.complete(p, NodeKind::SdblFieldList);
}

/// Parse a single selected field
///
/// Grammar: `selectedField: (asteriskField | columnField | expressionField | ...) alias?`
///
/// CRITICAL for AssignAliasFieldsInQuery diagnostic:
/// - Must distinguish asterisk fields (no alias needed)
/// - Must capture alias with/without AS keyword
fn selected_field(p: &mut Parser) {
    let m = p.start();

    // Check for asterisk field (* or Table.*)
    if is_asterisk_start(p) {
        asterisk_field(p);
    } else {
        // Parse expression (column reference, function call, etc.)
        expressions::expression(p);
    }

    // Optional alias
    // Parse alias if we see AS keyword OR an identifier
    // (identifier could be implicit alias or explicit with AS)
    p.skip_trivia(); // CRITICAL: Skip trivia before checking for alias
    if at_sdbl_keyword(p, "AS", "КАК") || is_identifier_token(p) {
        // Lookahead to avoid consuming keywords that start next clause
        if !is_clause_keyword(p) {
            alias(p);
        }
    }

    m.complete(p, NodeKind::SdblSelectedField);
}

/// Check if current position starts an asterisk field
///
/// Asterisk field patterns:
/// - `*` - all fields
/// - `Table.*` - all fields from table
fn is_asterisk_start(p: &Parser) -> bool {
    // Case 1: Just asterisk
    if p.at(TokenKind::Star) {
        return true;
    }

    // Case 2: Table.* pattern (requires lookahead)
    if p.at(TokenKind::Ident) {
        if let Some(TokenKind::Dot) = p.nth(1) {
            if let Some(TokenKind::Star) = p.nth(2) {
                return true;
            }
        }
    }

    false
}

/// Parse an asterisk field
///
/// Grammar: `asteriskField: (tableName=identifier DOT)* MUL`
fn asterisk_field(p: &mut Parser) {
    let m = p.start();

    // Optional table name prefix (Table.*)
    // Handle multiple dots (MDO.Table.*)
    while p.at(TokenKind::Ident) {
        if let Some(TokenKind::Dot) = p.nth(1) {
            p.bump(); // Ident
            p.bump(); // Dot
        } else {
            break;
        }
    }

    // Asterisk (mandatory)
    p.expect(TokenKind::Star);

    m.complete(p, NodeKind::SdblAsteriskField);
}

/// Parse a field alias
///
/// Grammar: `alias: AS? name=identifier`
///
/// CRITICAL for AssignAliasFieldsInQuery diagnostic:
/// - AS keyword is optional in grammar but diagnostic requires it
/// - Must track whether AS keyword is present
fn alias(p: &mut Parser) {
    let m = p.start();

    // Optional AS keyword
    eat_sdbl_keyword(p, "AS", "КАК");

    p.skip_trivia();

    // Identifier (mandatory)
    if !p.expect(TokenKind::Ident) {
        // Error recovery: complete anyway
    }

    m.complete(p, NodeKind::SdblAlias);
}

/// Parse FROM clause
///
/// Grammar: `FROM dataSources`
/// where `dataSources: tables+=dataSource (COMMA tables+=dataSource)*`
fn from_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "FROM", "ИЗ");
    p.skip_trivia();

    // Parse data sources (comma-separated)
    data_source(p);

    while p.eat(TokenKind::Comma) {
        p.skip_trivia();
        data_source(p);
    }

    m.complete(p, NodeKind::SdblFromClause);
}

/// Parse a data source (table, subquery, or parameter)
///
/// Grammar:
/// ```text
/// dataSource:
///     (LPAREN dataSource RPAREN)
///   | ((table | subquery) alias? joins+=joinPart*)
/// ```
///
/// Each data source can have zero or more JOINs attached to it.
fn data_source(p: &mut Parser) {
    let m = p.start();

    // Check for subquery in parentheses
    if p.at(TokenKind::LParen) {
        p.bump(); // (
        p.skip_trivia();

        // Parse subquery
        subquery(p);

        p.expect(TokenKind::RParen);
        p.skip_trivia();

        // Optional alias for subquery
        if (at_sdbl_keyword(p, "AS", "КАК") || is_identifier_token(p)) && !is_clause_keyword(p) {
            alias(p);
        }
    } else {
        // Table reference
        table_ref(p);

        p.skip_trivia(); // Skip whitespace before checking for alias

        // Optional alias for table
        if (at_sdbl_keyword(p, "AS", "КАК") || is_identifier_token(p)) && !is_clause_keyword(p) {
            alias(p);
        }
    }

    // Parse JOIN clauses (zero or more)
    p.skip_trivia();
    while is_join_keyword(p) {
        join_clause(p);
        p.skip_trivia();
    }

    m.complete(p, NodeKind::SdblDataSource);
}

/// Parse a table reference
///
/// Grammar (simplified):
/// ```text
/// table:
///     mdo
///   | mdo DOT objectTableName=identifier
///   | tableName=identifier
/// ```
///
/// Patterns:
/// - `Catalog.Products` - MDO reference
/// - `Catalog.Products.SliceLast` - Virtual table
/// - `#TempTable` - Temporary table
/// - `Products` - Simple table name
fn table_ref(p: &mut Parser) {
    let m = p.start();

    // Parse identifier chain (Table, MDO.Table, MDO.Table.VT)
    if !p.expect(TokenKind::Ident) {
        // Error recovery
        m.complete(p, NodeKind::SdblTableRef);
        return;
    }

    // Parse additional segments (DOT identifier)*
    while p.eat(TokenKind::Dot) {
        p.skip_trivia();
        p.expect(TokenKind::Ident);
    }

    m.complete(p, NodeKind::SdblTableRef);
}

/// Parse WHERE clause
///
/// Grammar: `WHERE logicalExpression`
fn where_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "WHERE", "ГДЕ");
    p.skip_trivia();

    // Parse logical expression (AND, OR, NOT, predicates)
    expressions::logical_expression(p);

    m.complete(p, NodeKind::SdblWhereClause);
}

/// Check if current token is an identifier
///
/// Note: Some keywords can be used as identifiers in SDBL
fn is_identifier_token(p: &Parser) -> bool {
    p.at(TokenKind::Ident)
}

/// Check if current token is a clause keyword (FROM, WHERE, GROUP, etc.)
///
/// Used to avoid consuming keywords when parsing aliases and for error recovery.
pub(super) fn is_clause_keyword(p: &Parser) -> bool {
    at_sdbl_keyword(p, "SELECT", "ВЫБРАТЬ")
        || at_sdbl_keyword(p, "FROM", "ИЗ")
        || at_sdbl_keyword(p, "WHERE", "ГДЕ")
        || p.at_keyword("GROUP")
        || p.at_keyword("HAVING")
        || p.at_keyword("ORDER")
        || at_sdbl_keyword(p, "UNION", "ОБЪЕДИНИТЬ")
        || p.at_keyword("INTO")
        || at_sdbl_keyword(p, "ON", "ПО")
        || is_join_keyword(p)
}

/// Check if current position starts a JOIN clause
///
/// Looks for: LEFT/RIGHT/FULL/INNER/OUTER/JOIN keywords
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

/// Parse a JOIN clause
///
/// Grammar:
/// ```text
/// joinPart:
///     (LEFT | RIGHT | FULL | INNER)? OUTER? JOIN
///     source=dataSource (ON | ПО) condition=logicalExpression
/// ```
fn join_clause(p: &mut Parser) {
    let m = p.start();

    // Parse join type (LEFT, RIGHT, FULL, INNER)
    // Note: In ANTLR grammar, JOIN alone defaults to INNER JOIN
    let has_join_type = p.at_keyword("LEFT")
        || p.at_keyword("ЛЕВОЕ")
        || p.at_keyword("RIGHT")
        || p.at_keyword("ПРАВОЕ")
        || p.at_keyword("FULL")
        || p.at_keyword("ПОЛНОЕ")
        || p.at_keyword("INNER")
        || p.at_keyword("ВНУТРЕННЕЕ");

    if has_join_type {
        p.bump();
        p.skip_trivia();
    }

    // Optional OUTER keyword (for LEFT OUTER JOIN, RIGHT OUTER JOIN, FULL OUTER JOIN)
    if p.at_keyword("OUTER") || p.at_keyword("ВНЕШНЕЕ") {
        p.bump();
        p.skip_trivia();
    }

    // JOIN/СОЕДИНЕНИЕ keyword (mandatory)
    if !p.at_keyword("JOIN") && !p.at_keyword("СОЕДИНЕНИЕ") {
        p.error(); // Expected JOIN keyword
        m.complete(p, NodeKind::SdblJoinClause);
        return;
    }
    p.bump(); // Consume JOIN
    p.skip_trivia();

    // Parse joined data source (table or subquery with alias)
    data_source(p);
    p.skip_trivia();

    // ON/ПО keyword (mandatory)
    if !eat_sdbl_keyword(p, "ON", "ПО") {
        p.error(); // Expected ON/ПО
    }
    p.skip_trivia();

    // Parse join condition (logical expression)
    expressions::logical_expression(p);

    m.complete(p, NodeKind::SdblJoinClause);
}

/// Check if current position starts a limitation keyword
///
/// Limitations: DISTINCT, TOP, ALLOWED
fn is_limitation_keyword(p: &Parser) -> bool {
    at_sdbl_keyword(p, "DISTINCT", "РАЗЛИЧНЫЕ")
        || at_sdbl_keyword(p, "TOP", "ПЕРВЫЕ")
        || at_sdbl_keyword(p, "ALLOWED", "РАЗРЕШЕННЫЕ")
}

/// Parse query limitations (DISTINCT, TOP, ALLOWED)
///
/// Grammar (simplified):
/// ```text
/// limitations: (DISTINCT | TOP count | ALLOWED)+
/// ```
///
/// ANTLR grammar has all permutations, but we simplify by accepting keywords in any order.
/// This accepts all valid combinations and some invalid ones (which is acceptable for error recovery).
fn limitations(p: &mut Parser) {
    let m = p.start();

    // Parse keywords in any order until no more limitation keywords found
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

/// Parse TOP clause
///
/// Grammar: `TOP count=DECIMAL`
fn top_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "TOP", "ПЕРВЫЕ");
    p.skip_trivia();

    // Expect a number (count)
    if !p.expect(TokenKind::Decimal) {
        // Error recovery: complete anyway
    }

    m.complete(p, NodeKind::SdblTopClause);
}

/// Parse GROUP BY clause
///
/// Grammar: `GROUP BY expression (, expression)*`
fn group_by_clause(p: &mut Parser) {
    let m = p.start();

    // GROUP/СГРУППИРОВАТЬ keyword
    eat_sdbl_keyword(p, "GROUP", "СГРУППИРОВАТЬ");
    p.skip_trivia();

    // BY/ПО keyword
    if !at_sdbl_keyword(p, "BY", "ПО") {
        // Error recovery: expected BY after GROUP
        m.complete(p, NodeKind::SdblGroupClause);
        return;
    }
    eat_sdbl_keyword(p, "BY", "ПО");
    p.skip_trivia();

    // Parse expressions (comma-separated list)
    super::expressions::expression(p);

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        p.skip_trivia();
        super::expressions::expression(p);
    }

    m.complete(p, NodeKind::SdblGroupClause);
}

/// Parse ORDER BY clause
///
/// Grammar: `ORDER BY orderByItem (, orderByItem)*`
/// orderByItem: expression (ASC | DESC)?
fn order_by_clause(p: &mut Parser) {
    let m = p.start();

    // ORDER/УПОРЯДОЧИТЬ keyword
    eat_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ");
    p.skip_trivia();

    // BY/ПО keyword
    if !at_sdbl_keyword(p, "BY", "ПО") {
        // Error recovery: expected BY after ORDER
        m.complete(p, NodeKind::SdblOrderClause);
        return;
    }
    eat_sdbl_keyword(p, "BY", "ПО");
    p.skip_trivia();

    // Parse order by items (comma-separated list)
    order_by_item(p);

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        p.skip_trivia();
        order_by_item(p);
    }

    m.complete(p, NodeKind::SdblOrderClause);
}

/// Parse single ORDER BY item
///
/// Grammar: `expression (ASC | DESC | ВОЗР | УБЫВ)?`
fn order_by_item(p: &mut Parser) {
    // Parse expression
    super::expressions::expression(p);
    p.skip_trivia();

    // Optional ASC/DESC/ВОЗР/УБЫВ modifier
    if p.at_keyword("ASC") || p.at_keyword("ВОЗР") || p.at_keyword("DESC") || p.at_keyword("УБЫВ")
    {
        p.bump(); // Consume ASC/DESC
        p.skip_trivia();
    }
}

#[cfg(test)]
mod tests {
    use crate::parse_sdbl;

    #[test]
    fn test_error_recovery_incomplete_field_list() {
        // Test that FROM clause is parsed even when SELECT field list is incomplete
        let input = r#"ВЫБРАТЬ
    Очередь.
ИЗ
    РегистрСведений.ОчередьОбновленияКэширующихДанных КАК Очередь"#;

        let parse = parse_sdbl(input);
        let tree_text = format!("{:#?}", parse.syntax_node());

        // Should have ERROR node marking incomplete field
        assert!(
            tree_text.contains("ERROR"),
            "Expected ERROR node for incomplete field.\nTree: {}",
            tree_text
        );

        // But FROM clause should still be parsed!
        assert!(
            tree_text.contains("SDBL_FROM_CLAUSE"),
            "FROM clause should be parsed despite incomplete field list.\nTree: {}",
            tree_text
        );

        // Should have SDBL_DATA_SOURCE (table reference)
        assert!(
            tree_text.contains("SDBL_DATA_SOURCE"),
            "Data source should be in FROM clause.\nTree: {}",
            tree_text
        );
    }

    #[test]
    fn test_error_recovery_complete_query_after_incomplete_field() {
        // More complete test: incomplete field, but FROM and WHERE both present
        let input = r#"ВЫБРАТЬ
    Очередь.
ИЗ
    РегистрСведений.Тест КАК Очередь
ГДЕ
    Очередь.Попыток < 3"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        // Should have ERROR node for incomplete field
        assert!(
            text.contains("ERROR"),
            "Expected ERROR node for incomplete field.\nTree: {}",
            text
        );

        // Should have FROM clause
        assert!(text.contains("SDBL_FROM_CLAUSE"), "FROM clause should be parsed.\nTree: {}", text);

        // Should have WHERE clause
        assert!(
            text.contains("SDBL_WHERE_CLAUSE"),
            "WHERE clause should be parsed.\nTree: {}",
            text
        );
    }
}
