//! The parser state machine.

use lexer::{Token, TokenKind};

use crate::event::{Event, NodeKind};

/// Parser for BSL language.
pub struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    events: Vec<Event>,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0, events: Vec::new() }
    }

    pub fn finish(self) -> Vec<Event> {
        self.events
    }

    /// Returns the current token kind.
    pub fn current(&self) -> Option<TokenKind> {
        self.nth(0)
    }

    /// Returns the nth token kind (0-indexed).
    pub fn nth(&self, n: usize) -> Option<TokenKind> {
        self.tokens.get(self.pos + n).map(|t| t.kind)
    }

    /// Checks if the current token matches the given kind.
    pub fn at(&self, kind: TokenKind) -> bool {
        self.current() == Some(kind)
    }

    /// Checks if at the end of input.
    pub fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    /// Consumes the current token if it matches the given kind.
    pub fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consumes the current token.
    pub fn bump(&mut self) {
        if let Some(kind) = self.current() {
            self.events.push(Event::Token { kind });
            self.pos += 1;
        }
    }

    /// Expects the current token to be of the given kind.
    pub fn expect(&mut self, kind: TokenKind) -> bool {
        if self.eat(kind) {
            true
        } else {
            self.error();
            false
        }
    }

    /// Starts a new node.
    pub fn start(&mut self) -> Marker {
        let pos = self.events.len();
        self.events.push(Event::Placeholder);
        Marker { pos }
    }

    /// Adds an error node.
    pub fn error(&mut self) {
        let m = self.start();
        if !self.at_end() {
            self.bump();
        }
        m.complete(self, NodeKind::Error);
    }

    /// Skips whitespace and comments.
    pub fn skip_trivia(&mut self) {
        while let Some(kind) = self.current() {
            match kind {
                TokenKind::Comment | TokenKind::Newline => self.bump(),
                _ => break,
            }
        }
    }
}

/// A marker for a started node.
pub struct Marker {
    pos: usize,
}

impl Marker {
    /// Completes the node with the given kind.
    pub fn complete(self, p: &mut Parser, kind: NodeKind) -> CompletedMarker {
        let event = &mut p.events[self.pos];
        *event = Event::Start { kind, forward_parent: None };
        p.events.push(Event::Finish);
        CompletedMarker { pos: self.pos }
    }

    /// Abandons this marker.
    pub fn abandon(self, p: &mut Parser) {
        if self.pos == p.events.len() - 1 {
            if let Some(Event::Placeholder) = p.events.last() {
                p.events.pop();
            }
        }
    }
}

/// A completed marker that can be used for precede.
pub struct CompletedMarker {
    pos: usize,
}

impl CompletedMarker {
    /// Wraps the completed node in a new parent.
    pub fn precede(self, p: &mut Parser) -> Marker {
        let new_pos = p.events.len();
        p.events.push(Event::Placeholder);

        if let Event::Start { forward_parent, .. } = &mut p.events[self.pos] {
            *forward_parent = Some(new_pos - self.pos);
        }

        Marker { pos: new_pos }
    }
}
