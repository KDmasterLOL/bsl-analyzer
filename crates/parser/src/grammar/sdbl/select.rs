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
pub(super) fn at_sdbl_keyword(p: &Parser, en: &str, ru: &str) -> bool {
    p.at_keyword(en) || p.at_keyword(ru)
}

/// Helper to consume bilingual SDBL keywords (English or Russian).
pub(super) fn eat_sdbl_keyword(p: &mut Parser, en: &str, ru: &str) -> bool {
    p.eat_keyword(en) || p.eat_keyword(ru)
}

/// Check if current token can start a data source.
///
/// Used for error recovery in FROM clause list parsing.
///
/// # Returns
///
/// `true` if current token can start a data source:
/// - `(` - subquery in parentheses
/// - Identifier - table name
///
/// `false` otherwise (including clause keywords)
fn is_data_source_start(p: &Parser) -> bool {
    match p.current() {
        Some(TokenKind::LParen) => true,                 // Subquery
        Some(TokenKind::Ident) => !is_clause_keyword(p), // Table name (but not clause keyword)
        Some(TokenKind::Ampersand) => true,              // Parameter as data source (&ТЗ)
        _ => false,
    }
}

/// Recover from unexpected tokens in selected field to alias or delimiter.
///
/// Called when expression parsing stopped early (e.g., didn't understand CASE in arithmetic).
/// Consumes all tokens until we find:
/// - AS/КАК keyword (alias start)
/// - Comma (next field)
/// - Clause keyword (FROM, WHERE, etc.)
///
/// **Important:** Handles nested constructs like CASE...END by tracking keywords.
/// Only creates ERROR node if actually consumed at least one token.
///
/// # Example
///
/// ```ignore
/// // After parsing "name" in: name + ВЫБОР КОГДА x ТОГДА y КОНЕЦ КАК alias
/// // Current position: +
/// recover_field_to_alias_or_delimiter(p);  // Consumes: + ВЫБОР ... КОНЕЦ
/// // Current position: КАК (alias start)
/// ```
fn recover_field_to_alias_or_delimiter(p: &mut Parser) {
    let err = p.start();
    let mut case_depth = 0i32; // Track nested CASE expressions
    let mut paren_depth = 0i32; // Track nested parentheses
    let mut consumed_any = false; // Track if we consumed at least one token

    loop {
        p.check_iteration_limit(); // Prevent infinite loops

        // Track CASE/ВЫБОР nesting (CASE can contain commas)
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

        // Track parenthesis nesting
        if p.at(TokenKind::LParen) {
            paren_depth += 1;
            p.bump();
            consumed_any = true;
            continue;
        }

        if p.at(TokenKind::RParen) && paren_depth > 0 {
            paren_depth -= 1;
            p.bump();
            consumed_any = true;
            continue;
        }

        // Only check delimiters when not inside nested constructs
        if case_depth == 0 && paren_depth == 0 {
            // Stop at alias keyword
            if at_sdbl_keyword(p, "AS", "КАК") {
                break;
            }

            // Stop at field delimiter (comma)
            if p.at(TokenKind::Comma) {
                break;
            }

            // Stop at semicolon (end of query)
            if p.at(TokenKind::Semicolon) {
                break;
            }

            // Stop at closing parenthesis (end of subquery in FROM)
            if p.at(TokenKind::RParen) {
                break;
            }

            // Stop at clause keywords
            if is_clause_keyword(p) {
                break;
            }

            // Stop at EOF
            if p.at_end() {
                break;
            }
        }

        // Consume one token
        p.bump();
        consumed_any = true;
    }

    // Only create ERROR node if we actually consumed tokens
    if consumed_any {
        err.complete(p, NodeKind::Error);
    } else {
        err.abandon(p);
    }
}

/// Recover to next delimiter by consuming unexpected tokens in virtual table arguments.
///
/// Similar to expressions::recover_to_delimiter but for virtual table method args context.
/// Tracks parenthesis balance to handle nested calls.
fn recover_to_delimiter_vt(p: &mut Parser) {
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
            if is_clause_keyword(p) {
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

// ============================================================================
// CLEAN-ROOM Slice 6 — select entry wrapper, subquery, UNION clause
// ============================================================================
//
// See `docs/legal/sdbl-clean-room-slice6.md` for authorship and source
// citations. Per-function provenance comments are attached at C2.

/// Parse a SELECT query.
///
/// Grammar: `select-query := subquery trailing-select-clauses*`
///
/// Opens the `SdblSelectQuery` marker around the `subquery` body and the
/// AUTOORDER / ORDER BY / TOTALS BY tail-clause loop. The tail-clause loop
/// itself lives under the LEGACY banner in `select_tail_clauses` because its
/// clean-room rewrite belongs to Slice 11 (clauses after FROM).
pub fn select_query(p: &mut Parser) {
    // local: event-parser entry shell; opens SdblSelectQuery around the
    // subquery body (main query + UNION chain) and the AUTOORDER / ORDER BY /
    // TOTALS BY tail-clause loop. The wrapper itself is glue; the tail-clause
    // loop is Tier B (see select_tail_clauses under the LEGACY banner).
    let m = p.start();
    subquery(p);
    select_tail_clauses(p);
    m.complete(p, NodeKind::SdblSelectQuery);
}

/// Parse a subquery (main query plus any UNION clauses).
///
/// Grammar: `subquery := query (union-clause)*`
pub(super) fn subquery(p: &mut Parser) {
    // ITS pubqlang/10 — a subquery is a single query body optionally followed
    // by UNION / UNION ALL clauses. The UNION loop terminates on a
    // package-level ';' or on any non-UNION token (including EOF);
    // parenthesised subqueries are closed by the caller (data_source) so ')'
    // is handled there, not here.
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

/// Parse a single UNION clause.
///
/// Grammar: `union-clause := UNION [ALL] query`
fn union_clause(p: &mut Parser) {
    // ITS pubqlang/10 — UNION clause: UNION | UNION ALL followed by a query.
    // UNION and UNION ALL share SdblUnionClause; the optional ALL modifier is
    // carried as an IDENT token inside the node, detected post-parse by
    // SdblUnionClause::has_all() (see sdbl-clean-room-slice6.md §Preserved
    // pre-refactor behaviours — the split into SdblUnionAllClause is deferred
    // to Slice 13).
    let m = p.start();

    eat_sdbl_keyword(p, "UNION", "ОБЪЕДИНИТЬ");

    p.skip_trivia();
    eat_sdbl_keyword(p, "ALL", "ВСЕ");

    p.skip_trivia();
    query(p);

    m.complete(p, NodeKind::SdblUnionClause);
}

// ============================================================================
// CLEAN-ROOM Slice 7 — SELECT prefix: field list, aliases, INTO
// ============================================================================
//
// See `docs/legal/sdbl-clean-room-slice7.md` for authorship and source
// citations (landed with C3). Per-function provenance comments are attached
// at C2.

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

    // INTO clause for temporary tables
    p.skip_trivia();
    if at_sdbl_keyword(p, "INTO", "ПОМЕСТИТЬ") {
        into_clause(p);
    }

    // Remaining clauses (FROM → ORDER BY) — Tier B, see `query_body_clauses`
    // under the LEGACY banner. Rewrite deferred to Slices 8 / 9 / 11.
    query_body_clauses(p);

    m.complete(p, NodeKind::SdblQuery);
}

/// Parse selected fields list
///
/// Grammar: `selectedFields: fields+=selectedField (COMMA fields+=selectedField)*`
///
/// Error recovery: Uses parse_delimited_list to handle:
/// - Incomplete fields (e.g., "SELECT Table.| FROM ...")
/// - Empty fields (e.g., "SELECT a, , b")
/// - Invalid tokens between commas
///
/// Recovery stops at clause keywords (FROM, WHERE, etc.) to allow rest of query to parse.
pub(super) fn selected_fields(p: &mut Parser) {
    let m = p.start();

    // Parse fields (comma-separated) with error recovery
    super::expressions::parse_delimited_list(
        p,
        TokenKind::Comma,
        &super::LIST_RECOVERY,
        is_field_start,
        selected_field,
    );

    m.complete(p, NodeKind::SdblFieldList);
}

/// Parse a single selected field
///
/// Grammar: `selectedField: (asteriskField | columnField | expressionField | ...) alias?`
///
/// CRITICAL for AssignAliasFieldsInQuery diagnostic:
/// - Must distinguish asterisk fields (no alias needed)
/// - Must capture alias with/without AS keyword
///
/// ERROR RECOVERY: After parsing expression, if there are unexpected tokens before
/// comma/clause keyword (e.g., unsupported SDBL constructs like CASE in arithmetic),
/// consume them into ERROR node and continue parsing next field.
fn selected_field(p: &mut Parser) {
    let m = p.start();

    // Check for asterisk field (* or Table.*)
    if is_asterisk_start(p) {
        asterisk_field(p);
    } else {
        // Parse expression (column reference, function call, etc.)
        expressions::expression(p);

        // ERROR RECOVERY: After expression, check if we're in a clean state
        // If we see unexpected tokens (not alias, not comma, not clause keyword),
        // it means expression parsing stopped early (unsupported construct)
        // Example: "name + ВЫБОР...КОНЕЦ КАК alias" - after "name", parser stops,
        // we need to consume "+ ВЫБОР...КОНЕЦ" as ERROR
        p.skip_trivia();

        // Check if we're in expected position (alias or end of field)
        let at_expected_position = at_sdbl_keyword(p, "AS", "КАК")
            || (is_identifier_token(p) && !is_clause_keyword(p))
            || p.at(TokenKind::Comma)
            || p.at(TokenKind::Semicolon) // End of query
            || is_clause_keyword(p)
            || p.at_end();

        if !at_expected_position {
            // Unexpected tokens after expression - consume them as ERROR
            // This handles: CASE expressions, type operators, unknown constructs, etc.
            recover_field_to_alias_or_delimiter(p);
            p.skip_trivia();
        }
    }

    // Optional alias
    // Parse alias if we see AS keyword OR an identifier
    // (identifier could be implicit alias or explicit with AS)
    p.skip_trivia(); // CRITICAL: Skip trivia before checking for alias
    if at_sdbl_keyword(p, "AS", "КАК") || is_identifier_token(p) {
        // Lookahead to avoid consuming keywords that start next clause
        if !is_clause_keyword(p) {
            selected_field_alias(p);
        }
    }

    m.complete(p, NodeKind::SdblSelectedField);
}

/// Check if current token can start a selected field.
///
/// Used for error recovery in SELECT field list parsing.
///
/// # Returns
///
/// `true` if current token can start a field:
/// - Expression start tokens (see is_expression_start)
/// - `*` - asterisk field
///
/// `false` otherwise (including clause keywords)
fn is_field_start(p: &Parser) -> bool {
    // Asterisk field (*, Table.*)
    if is_asterisk_start(p) {
        return true;
    }

    // Expression (column, function, literal, etc.)
    super::expressions::is_expression_start(p)
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

/// Parse a field alias (selected-field site).
///
/// Grammar: `alias: AS? name=identifier`
///
/// Split from the former `alias()` helper in Slice 7 C1 (pure refactor): the
/// pre-split function was called from two slices' surfaces — `selected_field`
/// (Slice 7) and `data_source`/table-ref (Slice 8). The body is bit-identical
/// to the pre-C1 `alias()` and to its LEGACY twin `source_alias_legacy`; the
/// split lets Slice 7 clean-room this helper without silently re-authoring
/// Slice 8 source-alias parsing. See `docs/legal/sdbl-clean-room-slice7.md`
/// §Preserved pre-refactor behaviours (landed with C3).
///
/// CRITICAL for AssignAliasFieldsInQuery diagnostic:
/// - AS keyword is optional in grammar but diagnostic requires it
/// - Must track whether AS keyword is present
fn selected_field_alias(p: &mut Parser) {
    let m = p.start();

    // Optional AS keyword
    eat_sdbl_keyword(p, "AS", "КАК");

    p.skip_trivia();

    // ERROR RECOVERY: Check if next token is clause keyword (FROM, WHERE, etc.)
    // This prevents "КАК\nИЗ" from consuming ИЗ as alias name
    if is_clause_keyword(p) {
        // Incomplete AS without alias - create empty ERROR node
        let err = p.start();
        err.complete(p, NodeKind::Error);
        m.complete(p, NodeKind::SdblAlias);
        return;
    }

    // Identifier (mandatory)
    if !p.expect(TokenKind::Ident) {
        // Error recovery: complete anyway
    }

    m.complete(p, NodeKind::SdblAlias);
}

/// Parse INTO clause for temporary tables
///
/// Grammar: `INTO|ПОМЕСТИТЬ tempTableName`
fn into_clause(p: &mut Parser) {
    let m = p.start();

    eat_sdbl_keyword(p, "INTO", "ПОМЕСТИТЬ");
    p.skip_trivia();

    // Parse temporary table name
    if p.at(TokenKind::Ident) {
        let table_m = p.start();
        p.bump();
        table_m.complete(p, NodeKind::SdblTempTableName);
    } else {
        p.error();
    }

    m.complete(p, NodeKind::SdblIntoClause);
}

// ============================================================================
// LEGACY (Slices 8–11 pending)
// ============================================================================
//
// Everything below this banner — `select_tail_clauses` (Slice 11 target),
// `query_body_clauses` (Slices 8 / 9 / 11 target), `source_alias_legacy`
// (Slice 8 target), `from_clause` / `data_source` / `table_ref` (Slice 8),
// `where_clause` / `group_by_clause` / `having_clause` / `order_by_clause` /
// `for_update_clause` / `index_by_clause` / `autoorder_clause` /
// `totals_by_clause` (Slice 11), JOIN family and the `is_join_keyword` /
// `join_clause` helpers (Slice 9), and the `limitations` / `top_clause`
// dispatchers remains Tier B pre-refactor code until the corresponding
// clean-room slice rewrites it. No per-function provenance comments here.

/// Parse the optional AUTOORDER / ORDER BY / TOTALS BY tail-clause loop.
///
/// Extracted verbatim from the pre-C1 `select_query` body as a pure refactor
/// so the Slice 6 `select_query` wrapper earlier in this file can be attested
/// under the Slice 6 clean-room banner without dragging clause-body scope in.
/// Rewrite of this helper is deferred to Slice 11 (clauses after FROM).
fn select_tail_clauses(p: &mut Parser) {
    // Parse AUTOORDER, ORDER BY, and TOTALS BY in any order
    // These clauses are all optional and can appear in any combination
    let mut parsed_autoorder = false;
    let mut parsed_order_by = false;
    let mut parsed_totals_by = false;

    loop {
        p.skip_trivia();

        // Check for AUTOORDER
        if !parsed_autoorder && at_sdbl_keyword(p, "AUTOORDER", "АВТОУПОРЯДОЧИВАНИЕ")
        {
            autoorder_clause(p);
            parsed_autoorder = true;
            continue;
        }

        // Check for ORDER BY
        if !parsed_order_by && at_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ") {
            order_by_clause(p);
            parsed_order_by = true;
            continue;
        }

        // Check for TOTALS BY
        if !parsed_totals_by && at_sdbl_keyword(p, "TOTALS", "ИТОГИ") {
            totals_by_clause(p);
            parsed_totals_by = true;
            continue;
        }

        // No more clauses to parse
        break;
    }
}

/// Parse the optional FROM → ORDER BY clause tail of a single query.
///
/// Extracted verbatim from the pre-C1 `query()` body as a pure refactor so
/// the Slice 7 `query()` wrapper earlier in this file can be attested under
/// the Slice 7 clean-room banner without dragging clause-body scope in.
/// Rewrite of this helper is deferred to Slices 8 (FROM), 9 (JOIN via
/// data_source), and 11 (WHERE / GROUP / HAVING / FOR UPDATE / INDEX BY /
/// ORDER BY).
fn query_body_clauses(p: &mut Parser) {
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
    p.skip_trivia();
    if at_sdbl_keyword(p, "HAVING", "ИМЕЮЩИЕ") {
        having_clause(p);
    }

    // FOR UPDATE clause (optional)
    // Note: We check for FOR UPDATE in one place
    // The function will handle cases where UPDATE is missing
    p.skip_trivia();
    if at_sdbl_keyword(p, "FOR", "ДЛЯ") {
        for_update_clause(p);
    }

    // INDEX BY clause (optional)
    p.skip_trivia();
    if at_sdbl_keyword(p, "INDEX", "ИНДЕКСИРОВАТЬ") {
        index_by_clause(p);
    }

    // ORDER BY clause (optional) - can appear in query()
    p.skip_trivia();
    if at_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ") {
        order_by_clause(p);
    }
}

/// Parse a source alias (FROM-clause data-source and table-ref sites).
///
/// LEGACY twin of `selected_field_alias`: bit-identical body, separate
/// call-site ownership. Clean-room rewrite deferred to Slice 8 (FROM sources
/// and source chains). Whether Slice 8 re-merges the two helpers into a
/// unified clean-room `alias()` is a Slice 8 decision; Slice 7 preserves the
/// split.
fn source_alias_legacy(p: &mut Parser) {
    let m = p.start();

    // Optional AS keyword
    eat_sdbl_keyword(p, "AS", "КАК");

    p.skip_trivia();

    // ERROR RECOVERY: Check if next token is clause keyword (FROM, WHERE, etc.)
    // This prevents "КАК\nИЗ" from consuming ИЗ as alias name
    if is_clause_keyword(p) {
        // Incomplete AS without alias - create empty ERROR node
        let err = p.start();
        err.complete(p, NodeKind::Error);
        m.complete(p, NodeKind::SdblAlias);
        return;
    }

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

    // Parse data sources (comma-separated) with error recovery
    super::expressions::parse_delimited_list(
        p,
        TokenKind::Comma,
        &super::LIST_RECOVERY,
        is_data_source_start,
        data_source,
    );

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
            source_alias_legacy(p);
        }
    } else {
        // Table reference
        table_ref(p);

        p.skip_trivia(); // Skip whitespace before checking for alias

        // Optional alias for table
        if (at_sdbl_keyword(p, "AS", "КАК") || is_identifier_token(p)) && !is_clause_keyword(p) {
            source_alias_legacy(p);
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

    // Parameter as data source: &Parameter (e.g., ИЗ &ТЗ КАК ТЗ)
    if p.at(TokenKind::Ampersand) {
        let pm = p.start();
        p.bump(); // &
        if p.at(TokenKind::Ident) {
            p.bump(); // parameter name
        }
        pm.complete(p, NodeKind::SdblParameter);
        m.complete(p, NodeKind::SdblTableRef);
        return;
    }

    // Parse identifier chain (Table, MDO.Table, MDO.Table.VT)
    if !p.expect(TokenKind::Ident) {
        // Error recovery
        m.complete(p, NodeKind::SdblTableRef);
        return;
    }

    // Parse additional segments (DOT identifier)*
    while p.eat(TokenKind::Dot) {
        p.check_iteration_limit(); // Prevent infinite loops
        p.skip_trivia();

        // ERROR RECOVERY: After DOT, only Ident is valid for table/MDO name
        // Whitelist approach: if NOT Ident, mark incomplete and stop
        if !p.at(TokenKind::Ident) {
            // Incomplete: operators, punctuation, EOF, etc.
            let err = p.start();
            err.complete(p, NodeKind::Error);
            break;
        }

        // Check if this Ident is clause keyword (FROM, WHERE) or AS keyword
        // Prevents "Справочник.\nКАК" from consuming КАК as table name
        if is_clause_keyword(p) || p.at_keyword("AS") || p.at_keyword("КАК") {
            // Incomplete table ref - don't consume keyword
            let err = p.start();
            err.complete(p, NodeKind::Error);
            break;
        }

        // Consume the identifier - it's a valid table/MDO name
        p.bump(); // Ident
    }

    // Check for virtual table method call (e.g., .Обороты(...), .Остатки(...))
    // If next token is '(', parse it as function call with arguments
    p.skip_trivia();
    if p.at(TokenKind::LParen) {
        p.bump(); // (
        p.skip_trivia();

        // Parse arguments (comma-separated expressions)
        // Empty parameters are valid SDBL: .Остатки(, , Авто, ) means "use defaults"
        if !p.at(TokenKind::RParen) {
            // First argument (might be empty — valid for VT params)
            if super::expressions::is_expression_start(p) && !p.at(TokenKind::Comma) {
                super::expressions::expression(p);

                // ERROR RECOVERY: After expression, consume unexpected tokens
                p.skip_trivia();
                if !p.at(TokenKind::Comma) && !p.at(TokenKind::RParen) {
                    recover_to_delimiter_vt(p);
                }
            } else {
                let m = p.start();
                m.complete(p, NodeKind::SdblMissingArg);
            }

            // Parse remaining arguments with error recovery
            while p.eat(TokenKind::Comma) {
                p.check_iteration_limit();
                p.skip_trivia();

                // Empty or trailing argument — valid for VT params
                if p.at(TokenKind::Comma)
                    || p.at(TokenKind::RParen)
                    || !super::expressions::is_expression_start(p)
                {
                    let m = p.start();
                    m.complete(p, NodeKind::SdblMissingArg);
                    if !p.at(TokenKind::Comma) {
                        break;
                    }
                    continue;
                }

                super::expressions::expression(p);

                // ERROR RECOVERY: After each argument expression, check for unexpected tokens
                p.skip_trivia();
                if !p.at(TokenKind::Comma) && !p.at(TokenKind::RParen) {
                    recover_to_delimiter_vt(p);
                }
            }
        }

        p.skip_trivia();
        p.expect(TokenKind::RParen);
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
        || at_sdbl_keyword(p, "GROUP", "СГРУППИРОВАТЬ")
        || at_sdbl_keyword(p, "HAVING", "ИМЕЮЩИЕ")
        || at_sdbl_keyword(p, "ORDER", "УПОРЯДОЧИТЬ")
        || at_sdbl_keyword(p, "UNION", "ОБЪЕДИНИТЬ")
        || at_sdbl_keyword(p, "INTO", "ПОМЕСТИТЬ")
        || at_sdbl_keyword(p, "ON", "ПО")
        || at_sdbl_keyword(p, "FOR", "ДЛЯ") // FOR UPDATE
        || at_sdbl_keyword(p, "INDEX", "ИНДЕКСИРОВАТЬ") // INDEX BY
        || at_sdbl_keyword(p, "AUTOORDER", "АВТОУПОРЯДОЧИВАНИЕ")
        || at_sdbl_keyword(p, "TOTALS", "ИТОГИ")
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

    // Parse join type (LEFT, RIGHT, FULL, INNER).
    // Bare JOIN without an explicit type is accepted as implicit INNER JOIN.
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
/// Keywords are accepted in any order to keep the parser tolerant; strict
/// ordering (where required) is enforced by semantic diagnostics, not here.
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

/// Parse HAVING clause
///
/// Grammar: `HAVING logicalExpression`
fn having_clause(p: &mut Parser) {
    let m = p.start();

    // HAVING/ИМЕЮЩИЕ keyword
    eat_sdbl_keyword(p, "HAVING", "ИМЕЮЩИЕ");
    p.skip_trivia();

    // Parse logical expression
    super::expressions::expression(p);

    m.complete(p, NodeKind::SdblHavingClause);
}

/// Parse FOR UPDATE clause
///
/// Grammar: `FOR UPDATE [mdo]`
fn for_update_clause(p: &mut Parser) {
    let m = p.start();

    // FOR/ДЛЯ keyword
    eat_sdbl_keyword(p, "FOR", "ДЛЯ");
    p.skip_trivia();

    // UPDATE/ИЗМЕНЕНИЯ keyword
    eat_sdbl_keyword(p, "UPDATE", "ИЗМЕНЕНИЯ");
    p.skip_trivia();

    // Optional MDO reference
    // If we see an identifier, it might be an MDO reference
    if p.at(TokenKind::Ident) && !is_clause_keyword(p) {
        // Parse MDO reference (Справочник.Контрагенты)
        // This is a simple dot-separated identifier chain
        p.bump(); // First part
        while p.at(TokenKind::Dot) {
            p.check_iteration_limit();
            p.bump(); // Dot
            if p.at(TokenKind::Ident) {
                p.bump();
            } else {
                break;
            }
        }
    }

    m.complete(p, NodeKind::SdblForUpdate);
}

/// Parse INDEX BY clause
///
/// Grammar: `INDEX BY indexingItem (, indexingItem)*`
/// indexingItem: expression
fn index_by_clause(p: &mut Parser) {
    let m = p.start();

    // INDEX/ИНДЕКСИРОВАТЬ keyword
    eat_sdbl_keyword(p, "INDEX", "ИНДЕКСИРОВАТЬ");
    p.skip_trivia();

    // BY/ПО keyword
    if !at_sdbl_keyword(p, "BY", "ПО") {
        // Error recovery: expected BY after INDEX
        m.complete(p, NodeKind::SdblIndexBy);
        return;
    }
    eat_sdbl_keyword(p, "BY", "ПО");
    p.skip_trivia();

    // Parse indexing items (comma-separated expressions)
    super::expressions::expression(p);

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        p.skip_trivia();
        super::expressions::expression(p);
    }

    m.complete(p, NodeKind::SdblIndexBy);
}

/// Parse AUTOORDER clause
///
/// Grammar: `AUTOORDER`
fn autoorder_clause(p: &mut Parser) {
    let m = p.start();

    // AUTOORDER/АВТОУПОРЯДОЧИВАНИЕ keyword
    eat_sdbl_keyword(p, "AUTOORDER", "АВТОУПОРЯДОЧИВАНИЕ");

    m.complete(p, NodeKind::SdblAutoorder);
}

/// Parse TOTALS BY clause
///
/// Grammar: `TOTALS [selectedFields] BY totalsGroup (, totalsGroup)*`
/// totalsGroup: `OVERALL | expression [ONLY? HIERARCHY] [alias]`
///
/// Simplified implementation: parse as comma-separated expressions
fn totals_by_clause(p: &mut Parser) {
    let m = p.start();

    // TOTALS/ИТОГИ keyword
    eat_sdbl_keyword(p, "TOTALS", "ИТОГИ");
    p.skip_trivia();

    // Check if we have selected fields before BY
    // If we see identifiers/expressions before BY, parse them as fields
    // This is a simplified approach - we parse everything as expressions
    // until we hit BY keyword
    while !p.at_end() {
        p.skip_trivia();

        // Check for BY keyword
        if at_sdbl_keyword(p, "BY", "ПО") {
            break;
        }

        // Check for clause keywords (stop parsing if we hit another clause)
        if is_clause_keyword(p) {
            break;
        }

        // Parse expression/field
        if super::expressions::is_expression_start(p) {
            super::expressions::expression(p);

            // Check for comma
            p.skip_trivia();
            if !p.at(TokenKind::Comma) {
                // No comma, check for BY
                continue;
            }
            p.bump(); // Comma
        } else {
            break;
        }
    }

    // BY/ПО keyword (required)
    if !at_sdbl_keyword(p, "BY", "ПО") {
        // Error recovery: expected BY
        m.complete(p, NodeKind::SdblTotalsBy);
        return;
    }
    eat_sdbl_keyword(p, "BY", "ПО");
    p.skip_trivia();

    // Parse totals groups (comma-separated)
    // For now, we parse as expressions
    // TODO: Add proper support for OVERALL, HIERARCHY, PERIODS
    super::expressions::expression(p);

    while p.eat(TokenKind::Comma) {
        p.check_iteration_limit();
        p.skip_trivia();
        super::expressions::expression(p);
    }

    m.complete(p, NodeKind::SdblTotalsBy);
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

    #[test]
    fn test_error_recovery_incomplete_field_in_middle_of_list() {
        // Real-world case: incomplete field IN THE MIDDLE of field list (not at the end)
        // User types: "Очередь.," - comma after dot without field name
        let input = r#"ВЫБРАТЬ ПЕРВЫЕ 500
    Очередь.,
    Очередь.ЗависимыйОбъектМетаданных КАК ЗависимыйОбъектМетаданных
ИЗ
    РегистрСведений.ОчередьОбновленияКэширующихДанных КАК Очередь
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

        // But FROM clause should still be parsed!
        assert!(
            text.contains("SDBL_FROM_CLAUSE"),
            "FROM clause should be parsed despite incomplete field in middle of list.\nTree: {}",
            text
        );

        // Should have SDBL_DATA_SOURCE (table reference)
        assert!(
            text.contains("SDBL_DATA_SOURCE"),
            "Data source should be in FROM clause.\nTree: {}",
            text
        );

        // Should have WHERE clause
        assert!(
            text.contains("SDBL_WHERE_CLAUSE"),
            "WHERE clause should be parsed.\nTree: {}",
            text
        );

        // Should have multiple SDBL_SELECTED_FIELD (both incomplete and complete fields)
        let field_count = text.matches("SDBL_SELECTED_FIELD").count();
        assert!(
            field_count >= 2,
            "Should have at least 2 selected fields (incomplete + complete). Got: {}",
            field_count
        );
    }

    #[test]
    fn test_tuple_in_in_predicate() {
        // Tuple on left side of IN predicate - row-wise comparison
        let input = r#"ВЫБРАТЬ *
ИЗ Документ.Заказ КАК Заказ
ГДЕ (Заказ.Партнер, Заказ.Контрагент, Заказ.Организация) В
    (ВЫБРАТЬ Т.Партнер, Т.Контрагент, Т.Организация ИЗ ВТ_Данные КАК Т)"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        // Should have TUPLE_EXPR node
        assert!(
            text.contains("SDBL_TUPLE_EXPR"),
            "Expected SDBL_TUPLE_EXPR for tuple in IN predicate.\nTree: {}",
            text
        );

        // Should have IN expression
        assert!(text.contains("SDBL_IN_EXPR"), "Expected SDBL_IN_EXPR.\nTree: {}", text);

        // Should have subquery inside IN
        assert!(
            text.contains("SDBL_SUBQUERY"),
            "Expected subquery inside IN predicate.\nTree: {}",
            text
        );

        // No parse errors expected
        assert!(!parse.has_errors(), "Should parse without errors: {:?}", parse.errors());
    }

    #[test]
    fn test_simple_tuple() {
        // Simple tuple with multiple elements
        // Note: using "Г" instead of "В" because "В" is IN keyword in Russian
        let input = r#"ВЫБРАТЬ * ИЗ Т ГДЕ (А, Б, Г) = (1, 2, 3)"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        // Should have 2 TUPLE_EXPR nodes (left and right of =)
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
        // Single expression in parentheses should NOT be a tuple
        let input = r#"ВЫБРАТЬ (А + Б) КАК Сумма ИЗ Т"#;

        let parse = parse_sdbl(input);
        let text = format!("{:#?}", parse.syntax_node());

        // Should NOT have TUPLE_EXPR
        assert!(
            !text.contains("SDBL_TUPLE_EXPR"),
            "Single parenthesized expression should not create SDBL_TUPLE_EXPR.\nTree: {}",
            text
        );

        // Should have PAREN_EXPR
        assert!(
            text.contains("SDBL_PAREN_EXPR"),
            "Expected SDBL_PAREN_EXPR for single parenthesized expression.\nTree: {}",
            text
        );
    }

    #[test]
    fn test_tuple_in_virtual_table_params() {
        // Tuple inside virtual table parameters
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

        // Should have TUPLE_EXPR
        assert!(
            text.contains("SDBL_TUPLE_EXPR"),
            "Expected SDBL_TUPLE_EXPR in virtual table params.\nTree: {}",
            text
        );

        // Should have TABLE_REF (virtual table)
        assert!(text.contains("SDBL_TABLE_REF"), "Expected SDBL_TABLE_REF.\nTree: {}", text);
    }
}
