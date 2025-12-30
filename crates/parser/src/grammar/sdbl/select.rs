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
    while p.at_keyword("UNION") {
        union_clause(p);
    }

    m.complete(p, NodeKind::SdblSubquery);
}

/// Parse a UNION clause
///
/// Grammar: `union: UNION ALL? query orderBy?`
fn union_clause(p: &mut Parser) {
    let m = p.start();

    p.expect_keyword("UNION");

    // Optional ALL keyword
    p.eat_keyword("ALL");

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
/// ```
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

    // SELECT keyword (mandatory)
    if !p.expect_keyword("SELECT") {
        // Error recovery: try to continue parsing
        m.complete(p, NodeKind::SdblQuery);
        return;
    }

    p.skip_trivia();

    // Phase 2: Parse limitations (DISTINCT, TOP, ALLOWED)
    // limitations(p);

    // Selected fields (mandatory)
    selected_fields(p);

    // Phase 2: INTO clause for temporary tables
    // if p.at_keyword("INTO") { into_clause(p); }

    // FROM clause (optional)
    if p.at_keyword("FROM") {
        from_clause(p);
    }

    // WHERE clause (optional)
    if p.at_keyword("WHERE") {
        where_clause(p);
    }

    // Phase 2: GROUP BY, HAVING, FOR UPDATE, INDEX BY
    // if p.at(TokenKind::KwGroup) { group_by_clause(p); }
    // if p.at(TokenKind::KwHaving) { having_clause(p); }
    // if p.at(TokenKind::KwFor) { for_update_clause(p); }
    // if p.at(TokenKind::KwIndex) { index_by_clause(p); }

    m.complete(p, NodeKind::SdblQuery);
}

/// Parse selected fields list
///
/// Grammar: `selectedFields: fields+=selectedField (COMMA fields+=selectedField)*`
fn selected_fields(p: &mut Parser) {
    let m = p.start();

    // Parse first field (mandatory)
    selected_field(p);

    // Parse additional fields (COMMA field)*
    while p.eat(TokenKind::Comma) {
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
    if p.at_keyword("AS") || is_identifier_token(p) {
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
    p.eat_keyword("AS");

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
///
/// Phase 1: Basic table references and subqueries
/// Phase 2: Add JOINs, virtual tables, external data sources
fn from_clause(p: &mut Parser) {
    let m = p.start();

    p.expect_keyword("FROM");
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
/// Grammar (simplified for Phase 1):
/// ```
/// dataSource:
///     (LPAREN dataSource RPAREN)
///   | ((table | subquery) alias?)
/// ```
///
/// Phase 2: Add JOINs, virtual tables
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
        if (p.at_keyword("AS") || is_identifier_token(p)) && !is_clause_keyword(p) {
            alias(p);
        }
    } else {
        // Table reference
        table_ref(p);

        // Optional alias for table
        if (p.at_keyword("AS") || is_identifier_token(p)) && !is_clause_keyword(p) {
            alias(p);
        }
    }

    // Phase 2: Parse JOIN clauses
    // while is_join_keyword(p) { join_part(p); }

    m.complete(p, NodeKind::SdblDataSource);
}

/// Parse a table reference
///
/// Grammar (simplified):
/// ```
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

    p.expect_keyword("WHERE");
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
/// Used to avoid consuming keywords when parsing aliases
fn is_clause_keyword(p: &Parser) -> bool {
    p.at_keyword("FROM")
        || p.at_keyword("WHERE")
        || p.at_keyword("GROUP")
        || p.at_keyword("HAVING")
        || p.at_keyword("ORDER")
        || p.at_keyword("UNION")
        || p.at_keyword("INTO")
}
