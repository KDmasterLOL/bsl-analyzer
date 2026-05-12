//! Sink for converting parser events into Rowan syntax tree.
//!
//! This module builds a SyntaxTreeBuilder from the events produced by the parser.

use syntax::{SyntaxTreeBuilder, TextSize};

use crate::{
    event::Event,
    syntax_kind::{node_kind_to_syntax, token_kind_to_syntax},
};

/// Builds a Rowan syntax tree from parser events and tokens.
///
/// This processes the event stream and constructs a lossless syntax tree
/// with all trivia (whitespace, comments) preserved.
///
/// The lifetime parameter `'cache` allows using a shared `NodeCache` for
/// token deduplication across multiple parses.
pub struct Sink<'t, 'cache> {
    builder: SyntaxTreeBuilder<'cache>,
    tokens: &'t [lexer::Token],
    token_pos: usize,
    errors: Vec<String>,
}

impl<'t> Sink<'t, 'static> {
    /// Create a new sink with the given tokens and its own internal cache.
    pub fn new(tokens: &'t [lexer::Token]) -> Self {
        Self { builder: SyntaxTreeBuilder::new(), tokens, token_pos: 0, errors: Vec::new() }
    }
}

impl<'t, 'cache> Sink<'t, 'cache> {
    /// Create a new sink with the given tokens and a shared cache.
    pub fn with_cache(tokens: &'t [lexer::Token], cache: &'cache mut syntax::NodeCache) -> Self {
        Self {
            builder: SyntaxTreeBuilder::with_cache(cache),
            tokens,
            token_pos: 0,
            errors: Vec::new(),
        }
    }

    /// Process all events and finish building the tree.
    pub fn finish(mut self, events: Vec<Event>) -> SyntaxTreeBuilder<'cache> {
        // Process events with forward_parent resolution
        let mut forward_parents = Vec::new();
        let mut skip = vec![false; events.len()];

        // First pass: mark events that are forward parents as already processed
        for i in 0..events.len() {
            if let Event::Start { forward_parent: Some(fwd), .. } = &events[i] {
                let mut idx = i + fwd;
                while let Event::Start { forward_parent, .. } = &events[idx] {
                    skip[idx] = true;
                    if let Some(next_fwd) = forward_parent {
                        idx += next_fwd;
                    } else {
                        break;
                    }
                }
            }
        }

        // Second pass: process events
        for i in 0..events.len() {
            match &events[i] {
                Event::Start { kind, forward_parent } => {
                    // Skip if this event was already processed as a forward_parent
                    if skip[i] {
                        continue;
                    }

                    // Collect all forward parents
                    forward_parents.clear();
                    forward_parents.push(*kind);

                    let mut idx = i;
                    let mut fp = *forward_parent;
                    while let Some(fwd) = fp {
                        idx += fwd;
                        if let Event::Start { kind, forward_parent } = &events[idx] {
                            forward_parents.push(*kind);
                            fp = *forward_parent;
                        } else {
                            unreachable!("forward_parent must point to Start event");
                        }
                    }

                    // Start nodes in reverse order (outermost first)
                    for kind in forward_parents.iter().rev() {
                        self.builder.start_node(node_kind_to_syntax(*kind));
                    }
                }

                Event::Finish => {
                    self.builder.finish_node();
                }

                Event::Token { kind } => {
                    self.token(*kind);
                }

                Event::Placeholder => {
                    // Placeholders should have been replaced during event processing
                }

                Event::Error(_) => {
                    // Deferred to Slice B.2: range computation + push to self.errors.
                }

                Event::ErrorWithSpan { .. } => {
                    // Deferred to Slice B.2: range computation with marker start + push.
                }
            }
        }

        // Add any remaining errors
        for (idx, error) in self.errors.iter().enumerate() {
            let offset = if idx < self.tokens.len() {
                self.tokens[idx].offset
            } else {
                self.tokens.last().map(|t| t.offset + t.text.len()).unwrap_or(0)
            };
            self.builder.error(error.clone(), TextSize::from(offset as u32));
        }

        self.builder
    }

    /// Process a single token.
    ///
    /// Note: Trivia (whitespace, comments) are already in the event stream from the parser
    /// calling bump() on them, so we don't need to consume trivia here.
    fn token(&mut self, kind: lexer::TokenKind) {
        // Add the token
        if let Some(token) = self.tokens.get(self.token_pos) {
            let syntax_kind = token_kind_to_syntax(kind);
            self.builder.token(syntax_kind, &token.text);
            self.token_pos += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar;

    #[test]
    fn test_sink_simple() {
        let source = "Процедура Тест() КонецПроцедуры";
        let tokens = lexer::tokenize(source);
        let mut parser = crate::Parser::new(&tokens);
        grammar::source_file(&mut parser);
        let events = parser.finish();

        let sink = Sink::new(&tokens);
        let builder = sink.finish(events);
        let parse = builder.finish();

        assert!(!parse.has_errors());
        let root = parse.syntax_node();
        assert_eq!(root.kind(), syntax::SyntaxKind::SOURCE_FILE);
    }

    #[test]
    fn test_sink_with_bom() {
        // UTF-8 BOM at start of file
        let source = "\u{FEFF}Процедура Тест() КонецПроцедуры";
        let tokens = lexer::tokenize(source);

        eprintln!("=== Tokens with BOM ===");
        for (i, token) in tokens.iter().enumerate() {
            eprintln!("{}: {:?} @ {} = {:?}", i, token.kind, token.offset, token.text);
        }

        let mut parser = crate::Parser::new(&tokens);
        grammar::source_file(&mut parser);
        let events = parser.finish();

        let sink = Sink::new(&tokens);
        let builder = sink.finish(events);
        let parse = builder.finish();

        eprintln!("=== Syntax tree ===");
        eprintln!("{:#?}", parse.syntax_node());

        // Should parse without errors (BOM is trivia)
        assert!(!parse.has_errors(), "File with BOM should parse without errors");
        let root = parse.syntax_node();
        assert_eq!(root.kind(), syntax::SyntaxKind::SOURCE_FILE);

        // Check that there are no ERROR nodes
        let error_nodes: Vec<_> =
            root.descendants().filter(|n| n.kind() == syntax::SyntaxKind::ERROR).collect();
        assert!(error_nodes.is_empty(), "Should have no ERROR nodes, found: {:?}", error_nodes);
    }

    #[test]
    fn test_sink_with_bom_and_region() {
        // UTF-8 BOM + CRLF + #Область (like real 1C files)
        let source =
            "\u{FEFF}\r\n#Область Test\r\nПроцедура Тест()\r\nКонецПроцедуры\r\n#КонецОбласти";
        let tokens = lexer::tokenize(source);

        eprintln!("=== Tokens with BOM+CRLF+Region ===");
        for (i, token) in tokens.iter().enumerate() {
            eprintln!("{}: {:?} @ {} = {:?}", i, token.kind, token.offset, token.text);
        }

        let mut parser = crate::Parser::new(&tokens);
        grammar::source_file(&mut parser);
        let events = parser.finish();

        let sink = Sink::new(&tokens);
        let builder = sink.finish(events);
        let parse = builder.finish();

        eprintln!("=== Syntax tree ===");
        eprintln!("{:#?}", parse.syntax_node());

        // Should parse without errors (BOM is trivia)
        assert!(!parse.has_errors(), "File with BOM+CRLF+Region should parse without errors");
        let root = parse.syntax_node();
        assert_eq!(root.kind(), syntax::SyntaxKind::SOURCE_FILE);

        // Check that there are no ERROR nodes
        let error_nodes: Vec<_> =
            root.descendants().filter(|n| n.kind() == syntax::SyntaxKind::ERROR).collect();
        assert!(error_nodes.is_empty(), "Should have no ERROR nodes, found: {:?}", error_nodes);
    }

    #[test]
    fn test_sink_multiple_variables() {
        let source = r#"
Перем Первая;
Перем Вторая Экспорт;
Перем Третья;
"#;
        eprintln!("=== Source ===\n{}", source);

        let tokens = lexer::tokenize(source);
        eprintln!("=== Tokens from lexer ===");
        for (i, token) in tokens.iter().enumerate() {
            eprintln!(
                "{}: {:?} @ {}..{} = {:?}",
                i,
                token.kind,
                token.offset,
                token.offset + token.text.len(),
                token.text
            );
        }

        let mut parser = crate::Parser::new(&tokens);
        grammar::source_file(&mut parser);
        let events = parser.finish();

        let sink = Sink::new(&tokens);
        let builder = sink.finish(events);
        let parse = builder.finish();

        eprintln!("=== Final syntax tree ===");
        eprintln!("{:#?}", parse.syntax_node());
    }
}
