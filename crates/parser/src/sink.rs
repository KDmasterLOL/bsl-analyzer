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
pub struct Sink<'t> {
    builder: SyntaxTreeBuilder,
    tokens: &'t [lexer::Token],
    token_pos: usize,
    errors: Vec<String>,
}

impl<'t> Sink<'t> {
    /// Create a new sink with the given tokens.
    pub fn new(tokens: &'t [lexer::Token]) -> Self {
        Self { builder: SyntaxTreeBuilder::new(), tokens, token_pos: 0, errors: Vec::new() }
    }

    /// Process all events and finish building the tree.
    pub fn finish(mut self, events: Vec<Event>) -> SyntaxTreeBuilder {
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

    /// Process a single token, inserting trivia before it.
    fn token(&mut self, kind: lexer::TokenKind) {
        // Insert whitespace trivia before the token
        self.eat_trivia();

        // Add the token itself
        if let Some(token) = self.tokens.get(self.token_pos) {
            let syntax_kind = token_kind_to_syntax(kind);
            self.builder.token(syntax_kind, &token.text);
            self.token_pos += 1;
        }
    }

    /// Consume and insert whitespace/comments as trivia tokens.
    fn eat_trivia(&mut self) {
        use lexer::TokenKind;

        while let Some(token) = self.tokens.get(self.token_pos) {
            // Skip only comments as trivia.
            // Newlines are NOT trivia in BSL - they are significant tokens
            // that the parser explicitly requests.
            match token.kind {
                TokenKind::Comment => {
                    let syntax_kind = token_kind_to_syntax(token.kind);
                    self.builder.token(syntax_kind, &token.text);
                    self.token_pos += 1;
                }
                _ => break,
            }
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
