//! Parser for BSL (1C:Enterprise) language.
//!
//! This crate parses BSL tokens into a syntax tree.
//! The parser uses an event-based approach similar to rust-analyzer.

mod event;
pub mod grammar;
mod parser;
mod sink;

use lexer::tokenize;

pub use crate::parser::Parser;

/// Parse BSL source code and return parsing events.
pub fn parse(input: &str) -> Parse {
    let tokens = tokenize(input);
    let mut p = Parser::new(&tokens);
    grammar::source_file(&mut p);
    let events = p.finish();
    Parse { events }
}

/// The result of parsing.
#[derive(Debug)]
pub struct Parse {
    pub events: Vec<event::Event>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let result = parse("Процедура Тест() КонецПроцедуры");
        assert!(!result.events.is_empty());
    }
}
