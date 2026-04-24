//! SDBL (Structured Data Base Language) grammar module
//!
//! Implements parsing for 1C:Enterprise query language embedded in BSL code.
//!
//! Architecture: Event-based parser
//!
//! ## Token Handling
//!
//! SDBL tokens are converted to BSL TokenKind via sdbl_token_converter.
//! Most SDBL keywords are mapped to TokenKind::Ident, so we check token text
//! to identify keywords.
//!
//! ## Provenance
//!
//! Slice 6 — clean-room: `query_package`, `queries`, `drop_table_query`
//! (this file) and `select_query` wrapper, `subquery`, `union_clause`
//! (in submodule `select`) are authored from ITS pubqlang/10 and /12
//! grammar-shape rules. See `docs/legal/sdbl-clean-room-slice6.md` for the
//! attestation (landed with C3).
//!
//! Slices 7–11 pending: `query` body and all clause bodies (SELECT fields,
//! FROM, WHERE, GROUP, ORDER, TOTALS, JOIN, FOR UPDATE, INDEX BY) remain
//! Tier B until their respective clean-room slices.

pub mod expressions;
pub mod select;

use crate::event::NodeKind;
use crate::parser::Parser;
use crate::token_set::TokenSet;
use lexer::TokenKind;

// ============================================================================
// Recovery Sets
// ============================================================================
//
// TokenSets for error recovery during parsing.
// These define "safe" stopping points where the parser can recover from errors.

/// Recovery set for expressions and list elements.
/// Recovery set for list parsing (SELECT fields, FROM sources, IN value lists, etc.)
///
/// Does NOT include Comma because comma is the delimiter in parse_delimited_list,
/// not a recovery point. Including Comma would cause the list to break before
/// consuming the comma separator (e.g., `IN (VALUE(...), VALUE(...))` would fail).
///
/// Used in:
/// - SELECT field list
/// - FROM data source list
/// - IN predicate value list
pub(super) const LIST_RECOVERY: TokenSet = TokenSet::new(&[
    TokenKind::RParen,    // End of parenthesized expression/list
    TokenKind::Semicolon, // End of query/statement
]);

// ============================================================================
// CLEAN-ROOM Slice 6 — query package, DROP, select entry
// ============================================================================
//
// See `docs/legal/sdbl-clean-room-slice6.md` for authorship and source
// citations. Per-function provenance comments are attached at C2.

/// Entry point for SDBL parsing
///
/// Grammar: `queryPackage: queries (SEMICOLON queries)* SEMICOLON? EOF`
///
/// Parses a complete SDBL query package, which may contain multiple queries
/// separated by semicolons.
///
/// # Examples
///
/// ```sdbl
/// SELECT Name FROM Catalog.Products;
/// SELECT Code FROM Catalog.Products
/// ```
pub fn query_package(p: &mut Parser) {
    let m = p.start();

    // Skip leading trivia (whitespace, newlines) before first query
    p.skip_trivia();

    // Parse first query (mandatory)
    if !p.at_end() {
        queries(p);
    }

    // Parse additional queries (SEMICOLON queries)*
    // Note: Must skip trivia BEFORE checking for semicolon, as queries may end with newlines
    loop {
        p.check_iteration_limit(); // Prevent infinite loops
        p.skip_trivia();

        // Check for semicolon
        if !p.at(TokenKind::Semicolon) {
            break;
        }

        p.bump(); // consume semicolon
        p.skip_trivia();

        // Check for trailing semicolon (allowed but optional)
        if p.at_end() {
            break;
        }

        queries(p);
    }

    m.complete(p, NodeKind::SdblQueryPackage);
}

/// Parse a single query (either SELECT or DROP TABLE)
///
/// Grammar: `queries: selectQuery | dropTableQuery`
fn queries(p: &mut Parser) {
    if select::at_sdbl_keyword(p, "DROP", "УНИЧТОЖИТЬ") {
        drop_table_query(p);
    } else {
        select::select_query(p);
    }
}

/// Parse DROP TABLE query
///
/// Grammar: `dropTableQuery: DROP temporaryTableName=identifier`
fn drop_table_query(p: &mut Parser) {
    let m = p.start();
    select::eat_sdbl_keyword(p, "DROP", "УНИЧТОЖИТЬ");
    p.skip_trivia();
    if p.at(TokenKind::Ident) {
        p.bump();
    } else {
        p.error();
    }
    m.complete(p, NodeKind::SdblDropQuery);
}
