mod event;
pub mod grammar;
mod parser;
mod sdbl_token_converter;
mod sink;
mod syntax_kind;
pub mod token_set;

use lexer::tokenize;

pub use crate::parser::Parser;

pub fn parse(input: &str) -> syntax::Parse<syntax::SyntaxNode> {
    let tokens = tokenize(input);
    let mut p = Parser::new(&tokens);
    grammar::source_file(&mut p);
    let events = p.finish();

    let sink = sink::Sink::new(&tokens);
    let builder = sink.finish(events);
    builder.finish()
}

pub fn parse_with_shared_cache(input: &str) -> syntax::Parse<syntax::SyntaxNode> {
    let tokens = tokenize(input);
    let mut p = Parser::new(&tokens);
    grammar::source_file(&mut p);
    let events = p.finish();

    syntax::with_shared_node_cache(|cache| {
        let sink = sink::Sink::with_cache(&tokens, cache);
        let builder = sink.finish(events);
        builder.finish()
    })
}

pub fn parse_sdbl(input: &str) -> syntax::Parse<syntax::SyntaxNode> {
    use lexer::sdbl::tokenize_sdbl;

    let sdbl_tokens = tokenize_sdbl(input);

    let tokens = sdbl_token_converter::convert_sdbl_tokens(&sdbl_tokens);

    let mut p = parser::Parser::new(&tokens);
    grammar::sdbl::query_package(&mut p);
    let events = p.finish();

    let sink = sink::Sink::new(&tokens);
    let builder = sink.finish(events);
    builder.finish()
}

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

    fn assert_parses(code: &str) {
        let parse = parse(code);
        if parse.has_errors() {
            for e in parse.errors() {
                eprintln!("parse error: {e:?}");
            }
            panic!("valid 1C code must parse without errors:\n{code}");
        }
    }

    // Date literals carry optional separators and a time component; assignments
    // using them must parse cleanly (see lexer `Date` token).

    #[test]
    fn iso_datetime_literal_assignment_parses() {
        assert_parses("Процедура П()\n\tВремяНачала = '0001-01-01 09:00:00';\nКонецПроцедуры\n");
    }

    #[test]
    fn digits_spaced_time_literal_assignment_parses() {
        assert_parses("Процедура П()\n\tВремяНачала = '00010101 22:00';\nКонецПроцедуры\n");
    }

    #[test]
    #[ignore]
    fn test_event_balance_large_file() {
        use crate::event::Event;

        let input = include_str!("../tests/fixtures/Module.bsl");
        let tokens = tokenize(input);

        let mut p = Parser::new(&tokens);
        grammar::source_file(&mut p);
        let events = p.finish();

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
        let parse = parse_sdbl("SELECT Name FROM Catalog.Products");

        if parse.has_errors() {
            for error in parse.errors() {
                eprintln!("Parse error: {:?}", error);
            }
        }

        let root = parse.syntax_node();
        assert_eq!(root.kind(), syntax::SyntaxKind::SDBL_QUERY_PACKAGE);
    }
}
