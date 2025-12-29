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
}
