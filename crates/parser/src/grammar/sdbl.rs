//! SDBL (Structured Data Base Language) grammar module
//!
//! Implements parsing for 1C:Enterprise query language embedded in BSL code.
//!
//! Grammar reference: SDBLParser.g4 from bsl-parser
//! Architecture: Event-based parser (like rust-analyzer)
//!
//! ## Token Handling
//!
//! SDBL tokens are converted to BSL TokenKind via sdbl_token_converter.
//! Most SDBL keywords are mapped to TokenKind::Ident, so we check token text
//! to identify keywords.

pub mod expressions;
pub mod select;

use crate::event::NodeKind;
use crate::parser::Parser;
use lexer::TokenKind;

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

    // Parse first query (mandatory)
    if !p.at_end() {
        queries(p);
    }

    // Parse additional queries (SEMICOLON queries)*
    // Note: Must skip trivia BEFORE checking for semicolon, as queries may end with newlines
    loop {
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
///
/// Currently only SELECT queries are supported (Phase 1 MVP).
/// DROP TABLE support will be added in Phase 4.
fn queries(p: &mut Parser) {
    // Phase 1: Only SELECT queries
    // Phase 4: Add DROP TABLE support
    //
    // if p.at(TokenKind::KwDrop) {
    //     drop_table_query(p);
    // } else {
    //     select::select_query(p);
    // }

    select::select_query(p);
}
