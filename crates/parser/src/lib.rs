//! Parser for BSL (1C:Enterprise) language.
//!
//! This crate parses BSL tokens into a syntax tree.
//! The parser uses an event-based approach similar to rust-analyzer.

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
}
