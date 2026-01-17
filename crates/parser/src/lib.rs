//! Parser for BSL (1C:Enterprise) language.
//!
//! This crate parses BSL tokens into a syntax tree.
//! The parser uses an event-based approach similar to rust-analyzer.
//!
//! ## SDBL Support
//!
//! The parser also supports SDBL (query language) parsing via the `parse_sdbl` function.
//! SDBL is the SQL-like query language embedded in BSL string literals.

mod event;
pub mod grammar;
mod parser;
mod sdbl_token_converter;
mod sink;
mod syntax_kind;
pub mod token_set;

use lexer::tokenize;

pub use crate::parser::Parser;

/// Parse BSL source code into a Rowan syntax tree.
///
/// This is the main entry point for parsing. It tokenizes the input,
/// runs the parser, and builds a lossless syntax tree.
///
/// # Example
///
/// ```
/// let parse = parser::parse("Процедура Тест() КонецПроцедуры");
/// assert!(!parse.has_errors());
/// ```
pub fn parse(input: &str) -> syntax::Parse<syntax::SyntaxNode> {
    let tokens = tokenize(input);
    let mut p = Parser::new(&tokens);
    grammar::source_file(&mut p);
    let events = p.finish();

    // Build syntax tree from events
    let sink = sink::Sink::new(&tokens);
    let builder = sink.finish(events);
    builder.finish()
}

/// Parse BSL source code using a shared thread-local cache for token deduplication.
///
/// This version significantly reduces memory usage when parsing many files
/// by sharing common tokens (keywords, punctuation, etc.) across parses.
///
/// # Example
///
/// ```
/// let parse = parser::parse_with_shared_cache("Процедура Тест() КонецПроцедуры");
/// assert!(!parse.has_errors());
/// ```
pub fn parse_with_shared_cache(input: &str) -> syntax::Parse<syntax::SyntaxNode> {
    let tokens = tokenize(input);
    let mut p = Parser::new(&tokens);
    grammar::source_file(&mut p);
    let events = p.finish();

    // Build syntax tree from events using shared cache
    syntax::with_shared_node_cache(|cache| {
        let sink = sink::Sink::with_cache(&tokens, cache);
        let builder = sink.finish(events);
        builder.finish()
    })
}

/// Parse SDBL query string into a Rowan syntax tree.
///
/// SDBL (Structured Data Base Language) is the SQL-like query language
/// embedded within BSL code as string literals.
///
/// # Implementation Status
///
/// **Phase 1 (MVP):** Basic SELECT parsing for AssignAliasFieldsInQuery diagnostic
/// - SELECT fields with aliases (AS keyword tracking)
/// - FROM clause with table references and subqueries
/// - WHERE clause with logical expressions
/// - UNION queries
///
/// **Phase 2-4:** Complete SDBL grammar (JOINs, GROUP BY, ORDER BY, etc.)
///
/// # Example
///
/// ```ignore
/// let parse = parser::parse_sdbl("SELECT Name AS ProductName FROM Catalog.Products");
/// assert!(!parse.has_errors());
/// ```
pub fn parse_sdbl(input: &str) -> syntax::Parse<syntax::SyntaxNode> {
    use lexer::sdbl::tokenize_sdbl;

    // Tokenize SDBL query
    let sdbl_tokens = tokenize_sdbl(input);

    // Convert SDBL tokens to parser-compatible format
    let tokens = sdbl_token_converter::convert_sdbl_tokens(&sdbl_tokens);

    // Create parser and parse using SDBL grammar
    let mut p = parser::Parser::new(&tokens);
    grammar::sdbl::query_package(&mut p);
    let events = p.finish();

    // Build syntax tree from events
    let sink = sink::Sink::new(&tokens);
    let builder = sink.finish(events);
    builder.finish()
}

/// Parse SDBL query using a shared thread-local cache for token deduplication.
pub fn parse_sdbl_with_shared_cache(input: &str) -> syntax::Parse<syntax::SyntaxNode> {
    use lexer::sdbl::tokenize_sdbl;

    let sdbl_tokens = tokenize_sdbl(input);
    let tokens = sdbl_token_converter::convert_sdbl_tokens(&sdbl_tokens);

    let mut p = parser::Parser::new(&tokens);
    grammar::sdbl::query_package(&mut p);
    let events = p.finish();

    syntax::with_shared_node_cache(|cache| {
        let sink = sink::Sink::with_cache(&tokens, cache);
        let builder = sink.finish(events);
        builder.finish()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let parse = parse("Процедура Тест() КонецПроцедуры");
        assert!(!parse.has_errors());
        let root = parse.syntax_node();
        assert_eq!(root.kind(), syntax::SyntaxKind::SOURCE_FILE);
    }

    #[test]
    fn test_parse_with_trivia() {
        let parse = parse("// Comment\nПроцедура Тест()\nКонецПроцедуры");
        assert!(!parse.has_errors());
    }

    #[test]
    #[ignore] // Takes ~30ms, only run when investigating parser balance issues
    fn test_event_balance_large_file() {
        use crate::event::Event;

        let input = include_str!("../tests/fixtures/Module.bsl");
        let tokens = tokenize(input);

        let mut p = Parser::new(&tokens);
        grammar::source_file(&mut p);
        let events = p.finish();

        // Count event types
        let start_count = events.iter().filter(|e| matches!(e, Event::Start { .. })).count();
        let finish_count = events.iter().filter(|e| matches!(e, Event::Finish)).count();

        eprintln!("\n=== Event Balance Analysis ===");
        eprintln!("File size: {} bytes", input.len());
        eprintln!("Total events: {}", events.len());
        eprintln!("Start events: {}", start_count);
        eprintln!("Finish events: {}", finish_count);
        eprintln!("Balance: {} (should be 0)", start_count as i32 - finish_count as i32);

        assert_eq!(
            start_count, finish_count,
            "Events must be balanced! Start: {}, Finish: {}",
            start_count, finish_count
        );
    }

    #[test]
    fn test_parse_sdbl_entry_point() {
        // Test that SDBL parser works with basic SELECT query
        let parse = parse_sdbl("SELECT Name FROM Catalog.Products");

        // Should parse without errors
        if parse.has_errors() {
            for error in parse.errors() {
                eprintln!("Parse error: {:?}", error);
            }
        }

        let root = parse.syntax_node();
        // Root should be SDBL_QUERY_PACKAGE (not SDBL_QUERY)
        assert_eq!(root.kind(), syntax::SyntaxKind::SDBL_QUERY_PACKAGE);
    }
}
