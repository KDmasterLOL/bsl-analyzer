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
mod sink;
mod syntax_kind;

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

/// Parse SDBL query string into a Rowan syntax tree.
///
/// SDBL (Structured Data Base Language) is the SQL-like query language
/// embedded within BSL code as string literals.
///
/// # Implementation Status
///
/// **Current:** Basic lexer support with 150+ SDBL tokens (keywords, functions, metadata types)
/// **Future:** Full SDBL grammar parsing (SELECT, FROM, WHERE, JOIN, GROUP BY, ORDER BY, etc.)
///
/// This function provides the entry point for SDBL parsing. Full grammar implementation
/// will be completed in future iterations when SDBL diagnostics are implemented.
///
/// # Example
///
/// ```ignore
/// let parse = parser::parse_sdbl("SELECT Name FROM Catalog.Products");
/// // Full parsing not yet implemented - returns stub tree
/// ```
pub fn parse_sdbl(input: &str) -> syntax::Parse<syntax::SyntaxNode> {
    use lexer::sdbl::tokenize_sdbl;

    let _tokens = tokenize_sdbl(input);

    // TODO: Implement full SDBL grammar parsing
    // For now, return a minimal empty parse tree to indicate SDBL infrastructure is in place
    // We use the same pattern as the main parse() function

    // Build empty syntax tree (just a root node for now)
    let mut builder = syntax::SyntaxTreeBuilder::new();
    builder.start_node(syntax::SyntaxKind::SDBL_QUERY);
    builder.finish_node();
    builder.finish()
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
        // Test that SDBL parser entry point exists and can be called
        let parse = parse_sdbl("SELECT Name FROM Catalog.Products");
        // Should not panic - validates that infrastructure is in place
        // Full parsing functionality will be implemented in future iterations
        assert!(!parse.has_errors()); // No errors since we're not parsing yet
    }
}
