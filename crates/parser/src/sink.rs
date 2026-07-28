use parser_error::{ParseError, RecoveryKind};
use syntax::{SyntaxTreeBuilder, TextRange, TextSize};

use crate::{
    event::Event,
    syntax_kind::{node_kind_to_syntax, token_kind_to_syntax},
};

pub struct Sink<'t, 'cache> {
    builder: SyntaxTreeBuilder<'cache>,
    tokens: &'t [lexer::Token],
    token_pos: usize,
    errors: Vec<(TextRange, ParseError)>,
}

impl<'t> Sink<'t, 'static> {
    pub fn new(tokens: &'t [lexer::Token]) -> Self {
        Self { builder: SyntaxTreeBuilder::new(), tokens, token_pos: 0, errors: Vec::new() }
    }
}

impl<'t, 'cache> Sink<'t, 'cache> {
    pub fn with_cache(tokens: &'t [lexer::Token], cache: &'cache mut syntax::NodeCache) -> Self {
        Self {
            builder: SyntaxTreeBuilder::with_cache(cache),
            tokens,
            token_pos: 0,
            errors: Vec::new(),
        }
    }

    pub fn finish(mut self, events: Vec<Event>) -> SyntaxTreeBuilder<'cache> {
        let mut forward_parents = Vec::new();
        let mut skip = vec![false; events.len()];

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

        for i in 0..events.len() {
            match &events[i] {
                Event::Start { kind, forward_parent } => {
                    if skip[i] {
                        continue;
                    }

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

                Event::Placeholder => {}

                Event::Error(err) => {
                    let range = self.compute_error_range(err, None);
                    self.errors.push((range, err.clone()));
                }

                Event::ErrorWithSpan { start_token, err } => {
                    let range = self.compute_error_range(err, Some(*start_token));
                    self.errors.push((range, err.clone()));
                }
            }
        }

        for (range, err) in self.errors.drain(..) {
            self.builder.error(range, err);
        }

        self.builder
    }

    fn token(&mut self, kind: lexer::TokenKind) {
        if let Some(token) = self.tokens.get(self.token_pos) {
            let syntax_kind = token_kind_to_syntax(kind);
            self.builder.token(syntax_kind, &token.text);
            self.token_pos += 1;
        }
    }

    fn compute_error_range(&self, err: &ParseError, span_start_token: Option<usize>) -> TextRange {
        match err.recovery() {
            RecoveryKind::BumpToken => self.previous_token_range_or_zero_at_start(),
            RecoveryKind::MissingToken => {
                let offset = self.current_token_offset_or_source_len();
                TextRange::empty(TextSize::new(offset))
            }
            RecoveryKind::RecoverySpan => {
                let start = span_start_token
                    .map_or_else(|| self.source_len(), |idx| self.token_offset(idx));
                let end = self.current_token_offset_or_source_len();
                self.safe_range(start, end)
            }
            RecoveryKind::Custom => {
                if self.token_pos > 0 {
                    self.previous_token_range_or_zero_at_start()
                } else {
                    let offset = self.current_token_offset_or_source_len();
                    TextRange::empty(TextSize::new(offset))
                }
            }
        }
    }

    fn previous_token_range_or_zero_at_start(&self) -> TextRange {
        if self.token_pos == 0 {
            return TextRange::empty(TextSize::new(0));
        }

        self.tokens.get(self.token_pos - 1).map_or_else(
            || {
                let offset = self.source_len();
                TextRange::empty(TextSize::new(offset))
            },
            |token| {
                let start = self.clamp_offset(token.offset);
                let end = self.clamp_offset(token.offset.saturating_add(token.text.len()));
                self.safe_range(start, end)
            },
        )
    }

    fn current_token_offset_or_source_len(&self) -> u32 {
        self.tokens
            .get(self.token_pos)
            .map_or_else(|| self.source_len(), |token| self.clamp_offset(token.offset))
    }

    fn token_offset(&self, token_pos: usize) -> u32 {
        self.tokens
            .get(token_pos)
            .map_or_else(|| self.source_len(), |token| self.clamp_offset(token.offset))
    }

    fn clamp_offset(&self, offset: usize) -> u32 {
        self.to_u32(offset).min(self.source_len())
    }

    fn safe_range(&self, start: u32, end: u32) -> TextRange {
        let end = end.max(start);
        TextRange::new(TextSize::new(start), TextSize::new(end))
    }

    fn source_len(&self) -> u32 {
        self.tokens
            .last()
            .map_or(0, |token| self.to_u32(token.offset.saturating_add(token.text.len())))
    }

    fn to_u32(&self, offset: usize) -> u32 {
        u32::try_from(offset).unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{event::NodeKind, grammar};

    fn unexpected(recovery: RecoveryKind) -> ParseError {
        ParseError::Unexpected { found: Some(lexer::TokenKind::Ident), recovery }
    }

    fn range(start: u32, end: u32) -> TextRange {
        TextRange::new(TextSize::new(start), TextSize::new(end))
    }

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
    fn error_event_between_forward_parent_starts_preserves_tree_topology() {
        let source = "Тест";
        let tokens = lexer::tokenize(source);
        let events = vec![
            Event::Start { kind: NodeKind::SourceFile, forward_parent: None },
            Event::Start { kind: NodeKind::Ident, forward_parent: Some(4) },
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::Finish,
            Event::Error(unexpected(RecoveryKind::BumpToken)),
            Event::Start { kind: NodeKind::FieldExpr, forward_parent: None },
            Event::Finish,
            Event::Finish,
        ];

        let parse = Sink::new(&tokens).finish(events).finish();

        assert_eq!(parse.errors().len(), 1);
        let root = parse.syntax_node();
        let field = root.children().next().expect("source should contain field expr");
        assert_eq!(field.kind(), syntax::SyntaxKind::FIELD_EXPR);
        let ident = field.children().next().expect("field expr should contain ident");
        assert_eq!(ident.kind(), syntax::SyntaxKind::IDENT);
    }

    #[test]
    fn multi_byte_cyrillic_error_range_is_byte_correct() {
        // The token has to be one no enclosing rule is waiting for: a block
        // closer is reported at the gap without being consumed, so it never
        // carries a range of its own.
        let source = "Процедура Тест() А = Возврат; КонецПроцедуры";
        let tokens = lexer::tokenize(source);
        let unexpected = tokens
            .iter()
            .find(|token| token.kind == lexer::TokenKind::KwReturn)
            .expect("test input should contain Возврат token");
        let parse = crate::parse(source);
        let expected_range = range(
            unexpected.offset as u32,
            unexpected.offset.saturating_add(unexpected.text.len()) as u32,
        );

        let error = parse
            .errors()
            .iter()
            .find(|error| error.range() == expected_range)
            .expect("unexpected Cyrillic token should produce a byte-exact range");
        assert!(source.is_char_boundary(u32::from(error.range().start()) as usize));
        assert!(source.is_char_boundary(u32::from(error.range().end()) as usize));
    }

    #[test]
    fn missing_token_at_eof_uses_zero_width_source_len_range() {
        let source = "Процедура Тест(";
        let parse = crate::parse(source);
        let expected = TextRange::empty(TextSize::new(source.len() as u32));

        assert!(
            parse.errors().iter().any(|error| {
                error.range() == expected
                    && error.structured().recovery() == RecoveryKind::MissingToken
            }),
            "expected a MissingToken diagnostic at EOF, got {:?}",
            parse.errors()
        );
    }

    #[test]
    fn empty_token_stream_returns_clean_parse() {
        let parse = crate::parse("");

        assert!(!parse.has_errors());
        assert_eq!(parse.syntax_node().kind(), syntax::SyntaxKind::SOURCE_FILE);
    }

    #[test]
    fn error_with_span_ranges_from_start_token_to_current_token() {
        let source = "А Б;";
        let tokens = lexer::tokenize(source);
        let events = vec![
            Event::Start { kind: NodeKind::SourceFile, forward_parent: None },
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::Token { kind: lexer::TokenKind::Whitespace },
            Event::Token { kind: lexer::TokenKind::Ident },
            Event::ErrorWithSpan { start_token: 0, err: unexpected(RecoveryKind::RecoverySpan) },
            Event::Token { kind: lexer::TokenKind::Semicolon },
            Event::Finish,
        ];

        let parse = Sink::new(&tokens).finish(events).finish();

        assert_eq!(parse.errors().len(), 1);
        assert_eq!(
            parse.errors()[0].range(),
            range(tokens[0].offset as u32, tokens[3].offset as u32)
        );
    }

    #[test]
    fn test_sink_with_bom() {
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

        assert!(!parse.has_errors(), "File with BOM should parse without errors");
        let root = parse.syntax_node();
        assert_eq!(root.kind(), syntax::SyntaxKind::SOURCE_FILE);

        let error_nodes: Vec<_> =
            root.descendants().filter(|n| n.kind() == syntax::SyntaxKind::ERROR).collect();
        assert!(error_nodes.is_empty(), "Should have no ERROR nodes, found: {:?}", error_nodes);
    }

    #[test]
    fn test_sink_with_bom_and_region() {
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

        assert!(!parse.has_errors(), "File with BOM+CRLF+Region should parse without errors");
        let root = parse.syntax_node();
        assert_eq!(root.kind(), syntax::SyntaxKind::SOURCE_FILE);

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
